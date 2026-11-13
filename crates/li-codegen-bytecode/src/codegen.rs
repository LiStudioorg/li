//! 把中立 IR 翻译为栈式字节码。
//!
//! 翻译规则保持简单直接：
//! - 每个 IR 指令先把操作数压栈，再执行对应运算，最后把结果 `STORE` 到局部槽位；
//! - 基本块按顺序展开，跳转指令在全部指令生成后回填目标地址；
//! - 参数、局部变量与临时值统一映射为函数内的槽位下标。

use std::collections::HashMap;

use li_errors::Span;
use li_ir::{
    BasicBlock, BinaryOp, Function, Instruction as IrInstruction, Label, Operand, Place,
    Program as IrProgram, TempId, Terminator, UnaryOp,
};

use crate::model::{
    Constant, FunctionCode, Instruction, InstructionArg, Opcode, Program, BYTECODE_VERSION,
};

/// 常量池去重键。浮点数按二进制位比较，确保 `NaN` 等特殊值也能稳定去重。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ConstantKey {
    Integer(i64),
    Float(u64),
    Bool(bool),
    String(String),
    Null,
}

impl ConstantKey {
    fn from_constant(constant: &Constant) -> Self {
        match constant {
            Constant::Integer(value) => Self::Integer(*value),
            Constant::Float(value) => Self::Float(value.to_bits()),
            Constant::Bool(value) => Self::Bool(*value),
            Constant::String(value) => Self::String(value.clone()),
            Constant::Null => Self::Null,
        }
    }
}

/// 把 IR 程序翻译为字节码程序。
pub fn generate(program: &IrProgram) -> Program {
    let mut constants = Vec::new();
    let mut constant_indices = HashMap::new();

    let functions = program
        .functions
        .iter()
        .map(|function| generate_function(function, &mut constants, &mut constant_indices))
        .collect();

    let entry_function = program.function("主").or_else(|| program.function("main"));

    let mut entry_code = Vec::new();
    if let Some(function) = entry_function {
        let span = Span::default();

        if !function.params.is_empty() {
            let null_index =
                intern_constant(&mut constants, &mut constant_indices, &Constant::Null);
            for _ in 0..function.params.len() {
                entry_code.push(Instruction::new(
                    Opcode::PushConst,
                    vec![InstructionArg::Integer(null_index)],
                    span,
                ));
            }
        }
        entry_code.push(Instruction::new(
            Opcode::Call,
            vec![
                InstructionArg::Name(function.name.clone()),
                InstructionArg::Integer(function.params.len()),
            ],
            span,
        ));
        entry_code.push(Instruction::new(Opcode::Pop, Vec::new(), span));
    }
    entry_code.push(Instruction::new(Opcode::Halt, Vec::new(), Span::default()));

    Program {
        version: BYTECODE_VERSION,
        entry: entry_function.map(|function| function.name.clone()),
        constants,
        functions,
        entry_code,
    }
}

fn intern_constant(
    constants: &mut Vec<Constant>,
    indices: &mut HashMap<ConstantKey, usize>,
    constant: &Constant,
) -> usize {
    let key = ConstantKey::from_constant(constant);

    if let Some(index) = indices.get(&key) {
        return *index;
    }

    let index = constants.len();
    constants.push(constant.clone());
    indices.insert(key, index);
    index
}

/// 计算函数需要的临时值数量，包含返回指令中直接使用临时值的情况。
fn temp_count(function: &Function) -> usize {
    let mut count = function.temp_count();

    for block in &function.blocks {
        if let Terminator::Return(Some(Operand::Place(Place::Temp(id)))) = &block.terminator {
            count = count.max(id.0 + 1);
        }
    }

    count
}

/// 生成单个函数。
fn generate_function(
    function: &Function,
    constants: &mut Vec<Constant>,
    constant_indices: &mut HashMap<ConstantKey, usize>,
) -> FunctionCode {
    let mut emitter = FunctionEmitter::new(function, constants, constant_indices);

    // 入口块必须位于地址 0，虚拟机从上往下执行。
    let mut blocks: Vec<&BasicBlock> = Vec::with_capacity(function.blocks.len());
    if let Some(entry) = function.block(function.entry) {
        blocks.push(entry);
    }
    for block in &function.blocks {
        if block.label != function.entry {
            blocks.push(block);
        }
    }

    for block in blocks {
        emitter
            .label_addresses
            .insert(block.label, emitter.code.len());

        for (index, instruction) in block.instructions.iter().enumerate() {
            let span = block
                .instruction_spans
                .get(index)
                .copied()
                .unwrap_or_default();
            emitter.emit_ir_instruction(instruction, span);
        }

        emitter.emit_terminator(&block.terminator, block.terminator_span);
    }

    emitter.finish(function)
}

struct FunctionEmitter<'a> {
    slots: Vec<String>,
    slot_indices: HashMap<Place, usize>,
    constants: &'a mut Vec<Constant>,
    constant_indices: &'a mut HashMap<ConstantKey, usize>,
    code: Vec<Instruction>,
    label_addresses: HashMap<Label, usize>,
    fixups: Vec<(usize, Label)>,
}

impl<'a> FunctionEmitter<'a> {
    fn new(
        function: &Function,
        constants: &'a mut Vec<Constant>,
        constant_indices: &'a mut HashMap<ConstantKey, usize>,
    ) -> Self {
        let mut slots = Vec::new();
        let mut slot_indices = HashMap::new();

        for parameter in &function.params {
            let index = slots.len();
            slots.push(parameter.name.clone());
            slot_indices.insert(Place::Local(parameter.name.clone()), index);
        }

        for local in &function.locals {
            let index = slots.len();
            slots.push(local.name.clone());
            slot_indices.insert(Place::Local(local.name.clone()), index);
        }

        for id in 0..temp_count(function) {
            let index = slots.len();
            slots.push(format!("t{}", id));
            slot_indices.insert(Place::Temp(TempId(id)), index);
        }

        Self {
            slots,
            slot_indices,
            constants,
            constant_indices,
            code: Vec::new(),
            label_addresses: HashMap::new(),
            fixups: Vec::new(),
        }
    }

    fn finish(mut self, function: &Function) -> FunctionCode {
        for (address, label) in self.fixups {
            let target = *self
                .label_addresses
                .get(&label)
                .expect("内部错误：IR 引用了不存在的基本块标签");
            if let Some(instruction) = self.code.get_mut(address) {
                if let Some(slot) = instruction.args.first_mut() {
                    *slot = InstructionArg::Integer(target);
                }
            }
        }

        FunctionCode {
            name: function.name.clone(),
            arity: function.params.len(),
            slots: self.slots,
            code: self.code,
        }
    }

    fn emit(&mut self, opcode: Opcode, args: Vec<InstructionArg>, span: Span) -> usize {
        let address = self.code.len();
        self.code.push(Instruction::new(opcode, args, span));
        address
    }

    fn slot_of(&self, place: &Place) -> usize {
        *self
            .slot_indices
            .get(place)
            .expect("内部错误：IR 中的位置没有分配到局部槽位")
    }

    fn intern(&mut self, constant: &Constant) -> usize {
        intern_constant(self.constants, self.constant_indices, constant)
    }

    fn push_operand(&mut self, operand: &Operand, span: Span) {
        match operand {
            Operand::Constant(constant) => {
                let index = self.intern(&Constant::from(constant));
                self.emit(
                    Opcode::PushConst,
                    vec![InstructionArg::Integer(index)],
                    span,
                );
            }
            Operand::Place(place) => {
                let slot = self.slot_of(place);
                self.emit(Opcode::Load, vec![InstructionArg::Integer(slot)], span);
            }
        }
    }

    fn emit_store(&mut self, dest: &Place, span: Span) {
        let slot = self.slot_of(dest);
        self.emit(Opcode::Store, vec![InstructionArg::Integer(slot)], span);
    }

    fn emit_ir_instruction(&mut self, instruction: &IrInstruction, span: Span) {
        match instruction {
            IrInstruction::Const { dest, value, .. } => {
                let constant = Constant::from(value);
                let index = self.intern(&constant);
                self.emit(
                    Opcode::PushConst,
                    vec![InstructionArg::Integer(index)],
                    span,
                );
                self.emit_store(dest, span);
            }
            IrInstruction::Copy { dest, src, .. } => {
                self.push_operand(src, span);
                self.emit_store(dest, span);
            }
            IrInstruction::Unary {
                dest, op, operand, ..
            } => {
                self.push_operand(operand, span);
                let opcode = match op {
                    UnaryOp::Negate => Opcode::Negate,
                    UnaryOp::Not => Opcode::Not,
                };
                self.emit(opcode, Vec::new(), span);
                self.emit_store(dest, span);
            }
            IrInstruction::Binary {
                dest,
                op,
                left,
                right,
                ..
            } => {
                self.push_operand(left, span);
                self.push_operand(right, span);
                let opcode = match op {
                    BinaryOp::Add => Opcode::Add,
                    BinaryOp::Subtract => Opcode::Subtract,
                    BinaryOp::Multiply => Opcode::Multiply,
                    BinaryOp::Divide => Opcode::Divide,
                    BinaryOp::Equal => Opcode::Equal,
                    BinaryOp::Less => Opcode::Less,
                    BinaryOp::Greater => Opcode::Greater,
                    BinaryOp::And => Opcode::And,
                    BinaryOp::Or => Opcode::Or,
                };
                self.emit(opcode, Vec::new(), span);
                self.emit_store(dest, span);
            }
            IrInstruction::Call {
                dest,
                function,
                args,
                ..
            } => {
                for argument in args {
                    self.push_operand(argument, span);
                }
                self.emit(
                    Opcode::Call,
                    vec![
                        InstructionArg::Name(function.clone()),
                        InstructionArg::Integer(args.len()),
                    ],
                    span,
                );
                match dest {
                    Some(dest) => self.emit_store(dest, span),
                    None => {
                        self.emit(Opcode::Pop, Vec::new(), span);
                    }
                }
            }
            IrInstruction::Input { dest, .. } => {
                self.emit(Opcode::Input, Vec::new(), span);
                self.emit_store(dest, span);
            }
            IrInstruction::MakeList { dest, elements, .. } => {
                for element in elements {
                    self.push_operand(element, span);
                }
                self.emit(
                    Opcode::MakeList,
                    vec![InstructionArg::Integer(elements.len())],
                    span,
                );
                self.emit_store(dest, span);
            }
            IrInstruction::GetField {
                dest,
                object,
                field,
                ..
            } => {
                self.push_operand(object, span);
                self.emit(
                    Opcode::GetField,
                    vec![InstructionArg::Name(field.clone())],
                    span,
                );
                self.emit_store(dest, span);
            }
            IrInstruction::SetField {
                object,
                field,
                value,
            } => {
                self.push_operand(object, span);
                self.push_operand(value, span);
                self.emit(
                    Opcode::SetField,
                    vec![InstructionArg::Name(field.clone())],
                    span,
                );
            }
            IrInstruction::GetIndex {
                dest,
                object,
                index,
                ..
            } => {
                self.push_operand(object, span);
                self.push_operand(index, span);
                self.emit(Opcode::GetIndex, Vec::new(), span);
                self.emit_store(dest, span);
            }
            IrInstruction::SetIndex {
                object,
                index,
                value,
            } => {
                self.push_operand(object, span);
                self.push_operand(index, span);
                self.push_operand(value, span);
                self.emit(Opcode::SetIndex, Vec::new(), span);
            }
            IrInstruction::WrapOptional { dest, value, .. } => {
                let flag = match value {
                    Some(value) => {
                        self.push_operand(value, span);
                        1
                    }
                    None => 0,
                };
                self.emit(
                    Opcode::WrapOptional,
                    vec![InstructionArg::Integer(flag)],
                    span,
                );
                self.emit_store(dest, span);
            }
            IrInstruction::WrapResult { dest, value, .. } => {
                self.push_operand(value, span);
                self.emit(Opcode::WrapResult, Vec::new(), span);
                self.emit_store(dest, span);
            }
            IrInstruction::IsSuccess { dest, value, .. } => {
                self.push_operand(value, span);
                self.emit(Opcode::IsSuccess, Vec::new(), span);
                self.emit_store(dest, span);
            }
            IrInstruction::Unwrap { dest, value, .. } => {
                self.push_operand(value, span);
                self.emit(Opcode::Unwrap, Vec::new(), span);
                self.emit_store(dest, span);
            }
            IrInstruction::UnwrapError { dest, value, .. } => {
                self.push_operand(value, span);
                self.emit(Opcode::UnwrapError, Vec::new(), span);
                self.emit_store(dest, span);
            }
            IrInstruction::Print { value } => {
                self.push_operand(value, span);
                self.emit(Opcode::Print, Vec::new(), span);
            }
            IrInstruction::Nop => {
                self.emit(Opcode::Nop, Vec::new(), span);
            }
        }
    }

    fn emit_terminator(&mut self, terminator: &Terminator, span: Span) {
        match terminator {
            Terminator::Jump(target) => {
                let address = self.emit(Opcode::Jump, vec![InstructionArg::Integer(0)], span);
                self.fixups.push((address, *target));
            }
            Terminator::Branch {
                condition,
                then_label,
                else_label,
            } => {
                self.push_operand(condition, span);
                let false_jump =
                    self.emit(Opcode::JumpIfFalse, vec![InstructionArg::Integer(0)], span);
                self.fixups.push((false_jump, *else_label));
                let true_jump = self.emit(Opcode::Jump, vec![InstructionArg::Integer(0)], span);
                self.fixups.push((true_jump, *then_label));
            }
            Terminator::Return(Some(value)) => {
                self.push_operand(value, span);
                self.emit(Opcode::Return, Vec::new(), span);
            }
            Terminator::Return(None) => {
                self.emit(Opcode::ReturnVoid, Vec::new(), span);
            }
        }
    }
}
