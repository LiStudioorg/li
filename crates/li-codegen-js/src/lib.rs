//! li 语言中立 IR 的 JavaScript 后端。
//!
//! 后端只消费 [`li_ir::Program`]，不依赖 AST。基础块会生成一个
//! `switch` 状态机，因此任意 CFG（包括 `当` 循环与 `跳出`）都能
//! 转换为合法的 JavaScript。`打印` 映射为 `console.log`。

mod escape;
mod names;

use std::collections::HashMap;

use li_ir::{
    BinaryOp, Constant, Function, Instruction, IrType, Operand, Place, Program, Terminator, UnaryOp,
};

use escape::{float_literal, string_literal};
use names::{is_js_identifier, NameMangler};

/// 把中立 IR 生成为 JavaScript 源码。
pub fn generate(program: &Program) -> String {
    let mut generator = Generator::new(program);
    generator.generate()
}

struct Generator<'a> {
    program: &'a Program,
    function_names: HashMap<String, String>,
}

impl<'a> Generator<'a> {
    fn new(program: &'a Program) -> Self {
        let mut mangler = NameMangler::new();
        let function_names = program
            .functions
            .iter()
            .map(|function| (function.name.clone(), mangler.mangle(&function.name)))
            .collect();

        Self {
            program,
            function_names,
        }
    }

    fn generate(&mut self) -> String {
        let mut writer = Writer::new();
        writer.line("// 本文件由 li 编译器自动生成，请勿手动编辑。");
        writer.line("\"use strict\";");

        for function in &self.program.functions {
            writer.line("");
            self.emit_function(&mut writer, function);
        }

        if let Some(main) = self
            .program
            .function("主")
            .or_else(|| self.program.function("main"))
        {
            writer.line("");
            writer.line("// 程序入口");
            writer.line(format!("{}();", self.function_js_name(&main.name)));
        }

        writer.finish()
    }

    fn emit_function(&mut self, writer: &mut Writer, function: &Function) {
        let mut mangler = NameMangler::new();
        let mut local_names = HashMap::new();

        for param in &function.params {
            let name = mangler.mangle(&param.name);
            local_names.insert(param.name.clone(), name);
        }

        for local in &function.locals {
            let name = mangler.mangle(&local.name);
            local_names.insert(local.name.clone(), name);
        }

        let params = function
            .params
            .iter()
            .map(|param| self.local_js_name(&param.name, &local_names))
            .collect::<Vec<_>>()
            .join(", ");

        writer.open(format!(
            "function {}({params}) {{",
            self.function_js_name(&function.name)
        ));

        let temp_count = function.temp_count();
        if temp_count > 0 {
            let temps = (0..temp_count)
                .map(|index| format!("__t{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            writer.line(format!("let {temps};"));
        }

        for local in &function.locals {
            writer.line(format!(
                "let {};",
                self.local_js_name(&local.name, &local_names)
            ));
        }

        writer.line(format!("let __li_状态 = {};", function.entry.0));
        writer.open("while (true) {");
        writer.open("switch (__li_状态) {");

        for block in &function.blocks {
            writer.open(format!("case {}: {{", block.label.0));

            for instruction in &block.instructions {
                self.emit_instruction(writer, instruction, &local_names);
            }

            self.emit_terminator(writer, &block.terminator, &local_names);
            writer.close("}");
        }

        writer.open("default: {");
        writer.line("throw new Error(\"内部错误：遇到未知的基本块\");");
        writer.close("}");

        writer.close("}");
        writer.close("}");
        writer.close("}");
    }

    fn emit_instruction(
        &self,
        writer: &mut Writer,
        instruction: &Instruction,
        local_names: &HashMap<String, String>,
    ) {
        match instruction {
            Instruction::Const { dest, value, .. } => {
                writer.line(format!(
                    "{} = {};",
                    self.place_js_name(dest, local_names),
                    self.constant_text(value)
                ));
            }
            Instruction::Copy { dest, src, .. } => {
                writer.line(format!(
                    "{} = {};",
                    self.place_js_name(dest, local_names),
                    self.operand_js_name(src, local_names)
                ));
            }
            Instruction::Unary {
                dest, op, operand, ..
            } => {
                let operator = match op {
                    UnaryOp::Negate => "-",
                    UnaryOp::Not => "!",
                };
                writer.line(format!(
                    "{} = {}({});",
                    self.place_js_name(dest, local_names),
                    operator,
                    self.operand_js_name(operand, local_names)
                ));
            }
            Instruction::Binary {
                dest,
                op,
                left,
                right,
                ty,
            } => {
                let left = self.operand_js_name(left, local_names);
                let right = self.operand_js_name(right, local_names);
                let expression = match op {
                    BinaryOp::Add => format!("{left} + {right}"),
                    BinaryOp::Subtract => format!("{left} - {right}"),
                    BinaryOp::Multiply => format!("{left} * {right}"),
                    BinaryOp::Divide if matches!(ty, IrType::Integer) => {
                        format!("Math.trunc({left} / {right})")
                    }
                    BinaryOp::Divide => format!("{left} / {right}"),
                    BinaryOp::Equal => format!("{left} === {right}"),
                    BinaryOp::Less => format!("{left} < {right}"),
                    BinaryOp::Greater => format!("{left} > {right}"),
                    BinaryOp::And => format!("{left} && {right}"),
                    BinaryOp::Or => format!("{left} || {right}"),
                };

                writer.line(format!(
                    "{} = {};",
                    self.place_js_name(dest, local_names),
                    expression
                ));
            }
            Instruction::Call {
                dest,
                function,
                args,
                ..
            } => {
                let arguments = args
                    .iter()
                    .map(|argument| self.operand_js_name(argument, local_names))
                    .collect::<Vec<_>>()
                    .join(", ");
                let call = format!("{}({arguments})", self.function_js_name(function));

                match dest {
                    Some(dest) => writer.line(format!(
                        "{} = {};",
                        self.place_js_name(dest, local_names),
                        call
                    )),
                    None => writer.line(format!("{call};")),
                }
            }
            Instruction::Input { dest, .. } => {
                writer.line(format!(
                    "{} = typeof prompt === \"function\" ? (prompt(\"请输入：\") ?? \"\") : \"\";",
                    self.place_js_name(dest, local_names)
                ));
            }
            Instruction::MakeList { dest, elements, .. } => {
                let elements = elements
                    .iter()
                    .map(|element| self.operand_js_name(element, local_names))
                    .collect::<Vec<_>>()
                    .join(", ");
                writer.line(format!(
                    "{} = [{elements}];",
                    self.place_js_name(dest, local_names)
                ));
            }
            Instruction::GetField {
                dest,
                object,
                field,
                ..
            } => {
                writer.line(format!(
                    "{} = {};",
                    self.place_js_name(dest, local_names),
                    self.property_access(&self.operand_js_name(object, local_names), field)
                ));
            }
            Instruction::SetField {
                object,
                field,
                value,
            } => {
                writer.line(format!(
                    "{} = {};",
                    self.property_access(&self.operand_js_name(object, local_names), field),
                    self.operand_js_name(value, local_names)
                ));
            }
            Instruction::GetIndex {
                dest,
                object,
                index,
                ..
            } => {
                writer.line(format!(
                    "{} = {}[{}];",
                    self.place_js_name(dest, local_names),
                    self.operand_js_name(object, local_names),
                    self.operand_js_name(index, local_names)
                ));
            }
            Instruction::SetIndex {
                object,
                index,
                value,
            } => {
                writer.line(format!(
                    "{}[{}] = {};",
                    self.operand_js_name(object, local_names),
                    self.operand_js_name(index, local_names),
                    self.operand_js_name(value, local_names)
                ));
            }
            Instruction::WrapOptional { dest, value, .. } => {
                let dest = self.place_js_name(dest, local_names);
                match value {
                    Some(value) => writer.line(format!(
                        "{dest} = {{ __li_成功: true, __li_值: {} }};",
                        self.operand_js_name(value, local_names)
                    )),
                    None => writer.line(format!("{dest} = {{ __li_成功: false }};")),
                }
            }
            Instruction::WrapResult { dest, value, .. } => {
                writer.line(format!(
                    "{} = {{ __li_成功: true, __li_值: {} }};",
                    self.place_js_name(dest, local_names),
                    self.operand_js_name(value, local_names)
                ));
            }
            Instruction::IsSuccess { dest, value, .. } => {
                writer.line(format!(
                    "{} = ({}).__li_成功 === true;",
                    self.place_js_name(dest, local_names),
                    self.operand_js_name(value, local_names)
                ));
            }
            Instruction::Unwrap { dest, value, .. } => {
                writer.line(format!(
                    "{} = ({}).__li_值;",
                    self.place_js_name(dest, local_names),
                    self.operand_js_name(value, local_names)
                ));
            }
            Instruction::UnwrapError { dest, value, .. } => {
                writer.line(format!(
                    "{} = ({}).__li_错误;",
                    self.place_js_name(dest, local_names),
                    self.operand_js_name(value, local_names)
                ));
            }
            Instruction::Print { value } => {
                writer.line(format!(
                    "console.log({});",
                    self.operand_js_name(value, local_names)
                ));
            }
            Instruction::Nop => writer.line(";"),
        }
    }

    fn emit_terminator(
        &self,
        writer: &mut Writer,
        terminator: &Terminator,
        local_names: &HashMap<String, String>,
    ) {
        match terminator {
            Terminator::Jump(target) => {
                writer.line(format!("__li_状态 = {};", target.0));
                writer.line("continue;");
            }
            Terminator::Branch {
                condition,
                then_label,
                else_label,
            } => {
                writer.line(format!(
                    "__li_状态 = {} ? {} : {};",
                    self.operand_js_name(condition, local_names),
                    then_label.0,
                    else_label.0
                ));
                writer.line("continue;");
            }
            Terminator::Return(Some(value)) => {
                writer.line(format!(
                    "return {};",
                    self.operand_js_name(value, local_names)
                ));
            }
            Terminator::Return(None) => writer.line("return;"),
        }
    }

    fn function_js_name(&self, name: &str) -> String {
        match self.function_names.get(name) {
            Some(name) => name.clone(),
            None => {
                let mut mangler = NameMangler::new();
                mangler.mangle(name)
            }
        }
    }

    fn local_js_name(&self, name: &str, local_names: &HashMap<String, String>) -> String {
        match local_names.get(name) {
            Some(name) => name.clone(),
            None => {
                let mut mangler = NameMangler::new();
                mangler.mangle(name)
            }
        }
    }

    fn place_js_name(&self, place: &Place, local_names: &HashMap<String, String>) -> String {
        match place {
            Place::Local(name) => self.local_js_name(name, local_names),
            Place::Temp(id) => format!("__t{}", id.0),
        }
    }

    fn operand_js_name(&self, operand: &Operand, local_names: &HashMap<String, String>) -> String {
        match operand {
            Operand::Place(place) => self.place_js_name(place, local_names),
            Operand::Constant(constant) => self.constant_text(constant),
        }
    }

    fn constant_text(&self, constant: &Constant) -> String {
        match constant {
            Constant::Integer(value) => value.to_string(),
            Constant::Float(value) => float_literal(*value),
            Constant::Bool(value) => {
                if *value {
                    "true".to_owned()
                } else {
                    "false".to_owned()
                }
            }
            Constant::String(value) => string_literal(value),
            Constant::Null => "null".to_owned(),
        }
    }

    fn property_access(&self, object: &str, field: &str) -> String {
        if is_js_identifier(field) {
            format!("{object}.{field}")
        } else {
            format!("{object}[{}]", string_literal(field))
        }
    }
}

/// 带缩进的源码写入器。
struct Writer {
    output: String,
    indent: usize,
}

impl Writer {
    fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
        }
    }

    fn line(&mut self, text: impl AsRef<str>) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(text.as_ref());
        self.output.push('\n');
    }

    fn open(&mut self, text: impl AsRef<str>) {
        self.line(text);
        self.indent += 1;
    }

    fn close(&mut self, text: impl AsRef<str>) {
        self.indent = self.indent.saturating_sub(1);
        self.line(text);
    }

    fn finish(self) -> String {
        self.output
    }
}
