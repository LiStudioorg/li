//! li 语言中立 IR 的 LLVM 文本后端骨架。
//!
//! 该后端不依赖 `inkwell` 或 `llvm-config`，而是直接生成可被
//! `clang` / `llc` 消费的文本 LLVM IR（`.ll`）。当前覆盖整数、
//! 小数、布尔、字符串与基本控制流，足以验证“IR → LLVM IR”映射；
//! 列表、字典、可选值、结果值与内置方法等运行时对象仍由 VM / JS
//! 后端提供，后续会在 LLVM 运行时库中逐步实现。
//!
//! 所有不支持的指令都会生成带源码位置的中文诊断，而不是 panic。

use std::collections::HashMap;
use std::fmt::Write as _;

use li_errors::{Diagnostic, DiagnosticBag, Span};
use li_ir::{
    BasicBlock, BinaryOp, Constant, Function, Instruction, IrType, Operand, Place, Program,
    Terminator, UnaryOp,
};

/// 把中立 IR 生成为文本 LLVM IR。
pub fn generate(program: &Program) -> Result<String, DiagnosticBag> {
    generate_with_target(program, None)
}

/// 生成带目标三元组的文本 LLVM IR；交叉编译时写入 `target triple`，
/// 避免 clang 因宿主默认三元组与 `--target` 不一致而给出覆盖警告。
pub fn generate_for_target(program: &Program, target: &str) -> Result<String, DiagnosticBag> {
    generate_with_target(program, Some(target))
}

fn generate_with_target(program: &Program, target: Option<&str>) -> Result<String, DiagnosticBag> {
    let (module, diagnostics) = generate_reporting(program, target);

    match module {
        Some(module) if !diagnostics.has_errors() => Ok(module),
        _ => Err(diagnostics),
    }
}

/// 生成 LLVM IR，同时把黄色警告（例如"暂只支持纯标签枚举"）交给调用方。
///
/// 返回值第一项为 `None` 表示存在致命错误、未生成模块；否则为文本 LLVM IR。
/// 第二项是完整的诊断集合，调用方应把其中的警告打印到 stderr，
/// 但不要因为警告而中断编译。
pub fn generate_reporting(
    program: &Program,
    target: Option<&str>,
) -> (Option<String>, DiagnosticBag) {
    let mut generator = Generator::new();
    let module = generator.generate(program, target);

    let module = if generator.diagnostics.has_errors() {
        None
    } else {
        Some(module)
    };

    (module, generator.diagnostics)
}

struct Generator {
    strings: StringPool,
    diagnostics: DiagnosticBag,
    uses_printf: bool,
}

impl Generator {
    fn new() -> Self {
        Self {
            strings: StringPool::default(),
            diagnostics: DiagnosticBag::new(),
            uses_printf: false,
        }
    }

    fn generate(&mut self, program: &Program, target: Option<&str>) -> String {
        // 枚举预检：LLVM 骨架目前只支持"纯标签枚举"（所有枚举项都没有载荷）；
        // 带载荷的枚举需要 GC 运行时对象，报黄字警告拦截。
        self.warn_enum_support(program);

        let mut functions = String::new();

        for function in &program.functions {
            self.emit_function(&mut functions, function);
        }

        let mut output = String::new();
        output.push_str("; 本文件由 li 编译器生成，目标为文本 LLVM IR。\n");
        output.push_str("; 可使用 clang -O2 <文件.ll> -o <可执行文件> 生成原生二进制。\n\n");

        if let Some(triple) = target {
            writeln!(output, "target triple = \"{triple}\"").expect("写入字符串不会失败");
            output.push('\n');
        }

        if self.uses_printf {
            output.push_str("declare i32 @printf(ptr, ...)\n\n");
        }

        output.push_str(&self.strings.render());
        output.push('\n');
        output.push_str(&functions);
        self.emit_main(&mut output, program);
        output
    }

    fn emit_function(&mut self, output: &mut String, function: &Function) {
        if function.is_async {
            self.report(
                first_span(function),
                format!("LLVM 后端暂不支持异步函数「{}」", function.name),
            );
            return;
        }

        let return_type = match llvm_type(&function.return_type) {
            Ok(ty) => ty,
            Err(message) => {
                self.report(
                    first_span(function),
                    format!(
                        "LLVM 后端暂不支持函数「{}」的返回类型：{message}",
                        function.name
                    ),
                );
                return;
            }
        };

        let mut place_types = collect_place_types(function);
        for param in &function.params {
            place_types
                .entry(Place::Local(param.name.clone()))
                .or_insert_with(|| param.ty.clone());
        }

        let mut slots = SlotMap::default();
        let mut entry = String::new();

        for param in &function.params {
            if let Err(message) =
                slots.declare(&mut entry, Place::Local(param.name.clone()), &param.ty)
            {
                self.report(
                    first_span(function),
                    format!("LLVM 后端暂不支持参数「{}」：{message}", param.name),
                );
                return;
            }
        }

        for local in &function.locals {
            if let Err(message) =
                slots.declare(&mut entry, Place::Local(local.name.clone()), &local.ty)
            {
                self.report(
                    first_span(function),
                    format!("LLVM 后端暂不支持局部变量「{}」：{message}", local.name),
                );
                return;
            }
        }

        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Some((dest, ty)) = destination_type(instruction) {
                    if let Err(message) = slots.declare(&mut entry, dest, ty) {
                        self.report(
                            block
                                .instruction_spans
                                .first()
                                .copied()
                                .unwrap_or_else(|| first_span(function)),
                            format!("LLVM 后端暂不支持临时值类型：{message}"),
                        );
                        return;
                    }
                }
            }
        }

        let params = function
            .params
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                llvm_type(&param.ty)
                    .ok()
                    .map(|ty| format!("{ty} %arg{index}"))
            })
            .collect::<Vec<_>>()
            .join(", ");

        writeln!(
            output,
            "define {return_type} @\"{}\"({params}) {{",
            function.name
        )
        .expect("写入字符串不会失败");
        output.push_str("entry:\n");
        output.push_str(&entry);

        for (index, param) in function.params.iter().enumerate() {
            let Ok(ty) = llvm_type(&param.ty) else {
                continue;
            };
            let slot = slots.get(&Place::Local(param.name.clone()));
            if let Some(slot) = slot {
                writeln!(output, "  store {ty} %arg{index}, ptr %slot{}", slot.id)
                    .expect("写入字符串不会失败");
            }
        }

        output.push_str("  br label %L0\n\n");

        for block in &function.blocks {
            self.emit_block(output, function, block, &place_types, &slots);
        }

        output.push_str("}\n\n");
    }

    fn emit_block(
        &mut self,
        output: &mut String,
        function: &Function,
        block: &BasicBlock,
        place_types: &HashMap<Place, IrType>,
        slots: &SlotMap,
    ) {
        writeln!(output, "L{}:", block.label.0).expect("写入字符串不会失败");
        let mut registers = RegisterAllocator::for_block(block.label.0);

        for (index, instruction) in block.instructions.iter().enumerate() {
            let span = block
                .instruction_spans
                .get(index)
                .copied()
                .unwrap_or_else(|| first_span(function));

            if let Err(message) = self.emit_instruction(
                output,
                instruction,
                place_types,
                slots,
                &mut registers,
                span,
            ) {
                self.report(span, message);
            }
        }

        if let Err(message) = self.emit_terminator(
            output,
            function,
            &block.terminator,
            place_types,
            slots,
            &mut registers,
            block.terminator_span,
        ) {
            self.report(block.terminator_span, message);
        }

        output.push('\n');
    }

    fn emit_instruction(
        &mut self,
        output: &mut String,
        instruction: &Instruction,
        place_types: &HashMap<Place, IrType>,
        slots: &SlotMap,
        registers: &mut RegisterAllocator,
        _span: Span,
    ) -> Result<(), String> {
        match instruction {
            Instruction::Const { dest, value, ty } => {
                let value = constant_value(self, value, ty)?;
                store_value(output, slots, dest, ty, &value)?;
            }
            Instruction::Copy { dest, src, ty } => {
                let value = load_operand(self, output, src, place_types, slots, registers)?;
                store_value(output, slots, dest, ty, &value)?;
            }
            Instruction::Unary {
                dest,
                op,
                operand,
                ty,
            } => {
                let value = load_operand(self, output, operand, place_types, slots, registers)?;
                let result = registers.next();
                let llvm_ty = llvm_type(ty)?;
                match (op, ty) {
                    (UnaryOp::Negate, IrType::Integer) => {
                        writeln!(output, "  {result} = sub i64 0, {value}")
                            .expect("写入字符串不会失败");
                    }
                    (UnaryOp::Negate, IrType::Float) => {
                        writeln!(output, "  {result} = fneg double {value}")
                            .expect("写入字符串不会失败");
                    }
                    (UnaryOp::Not, IrType::Bool) => {
                        writeln!(output, "  {result} = xor i1 {value}, true")
                            .expect("写入字符串不会失败");
                    }
                    _ => {
                        return Err(format!("LLVM 后端暂不支持一元运算「{op:?}」（{llvm_ty}）"));
                    }
                }
                store_value(output, slots, dest, ty, &result)?;
            }
            Instruction::Binary {
                dest,
                op,
                left,
                right,
                ty,
            } => {
                let operand_ty = match op {
                    BinaryOp::Equal | BinaryOp::Less | BinaryOp::Greater => {
                        operand_type(left, place_types)?
                    }
                    _ => ty.clone(),
                };
                let left = load_operand(self, output, left, place_types, slots, registers)?;
                let right = load_operand(self, output, right, place_types, slots, registers)?;
                let result = registers.next();
                let expression = binary_expression(op, &operand_ty, ty, &left, &right, &result)?;
                writeln!(output, "  {expression}").expect("写入字符串不会失败");
                store_value(output, slots, dest, ty, &result)?;
            }
            Instruction::Call {
                dest,
                function,
                args,
                ty,
            } => {
                let mut values = Vec::with_capacity(args.len());
                for argument in args {
                    // LLVM 文本 IR 要求每个实参同时携带类型，例如
                    // `i64 %r0`；只写 `%r0` 会被 clang 判为语法错误。
                    let argument_ty = operand_type(argument, place_types)?;
                    let value =
                        load_operand(self, output, argument, place_types, slots, registers)?;
                    values.push(format!("{} {value}", llvm_type(&argument_ty)?));
                }
                let ret = llvm_type(ty)?;
                let result = registers.next();
                writeln!(
                    output,
                    "  {result} = call {ret} @\"{function}\"({})",
                    values.join(", ")
                )
                .expect("写入字符串不会失败");

                if let Some(dest) = dest {
                    store_value(output, slots, dest, ty, &result)?;
                }
            }
            Instruction::Print { value } => {
                let operand_type = operand_type(value, place_types)?;
                let value = load_operand(self, output, value, place_types, slots, registers)?;
                self.emit_print(output, &operand_type, &value, registers)?;
            }
            Instruction::Nop => {}
            other => {
                return Err(format!(
                    "LLVM 后端暂不支持指令：{}",
                    instruction_name(other)
                ));
            }
        }

        Ok(())
    }

    fn emit_print(
        &mut self,
        output: &mut String,
        ty: &IrType,
        value: &str,
        registers: &mut RegisterAllocator,
    ) -> Result<(), String> {
        self.uses_printf = true;
        let result = registers.next();
        match ty {
            IrType::Integer => {
                let format = self.strings.intern("%lld\n");
                writeln!(
                    output,
                    "  {result} = call i32 (ptr, ...) @printf(ptr {format}, i64 {value})"
                )
                .expect("写入字符串不会失败");
            }
            IrType::Float => {
                let format = self.strings.intern("%f\n");
                writeln!(
                    output,
                    "  {result} = call i32 (ptr, ...) @printf(ptr {format}, double {value})"
                )
                .expect("写入字符串不会失败");
            }
            IrType::String => {
                let format = self.strings.intern("%s\n");
                writeln!(
                    output,
                    "  {result} = call i32 (ptr, ...) @printf(ptr {format}, ptr {value})"
                )
                .expect("写入字符串不会失败");
            }
            IrType::Bool => {
                let truthy = self.strings.intern("真");
                let falsy = self.strings.intern("假");
                let format = self.strings.intern("%s\n");
                let selected = registers.next();
                writeln!(
                    output,
                    "  {selected} = select i1 {value}, ptr {truthy}, ptr {falsy}"
                )
                .expect("写入字符串不会失败");
                writeln!(
                    output,
                    "  {result} = call i32 (ptr, ...) @printf(ptr {format}, ptr {selected})"
                )
                .expect("写入字符串不会失败");
            }
            IrType::Null => {
                let text = self.strings.intern("空");
                let format = self.strings.intern("%s\n");
                writeln!(
                    output,
                    "  {result} = call i32 (ptr, ...) @printf(ptr {format}, ptr {text})"
                )
                .expect("写入字符串不会失败");
            }
            other => {
                return Err(format!("LLVM 后端暂不支持打印类型「{other}」"));
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_terminator(
        &mut self,
        output: &mut String,
        function: &Function,
        terminator: &Terminator,
        place_types: &HashMap<Place, IrType>,
        slots: &SlotMap,
        registers: &mut RegisterAllocator,
        _span: Span,
    ) -> Result<(), String> {
        match terminator {
            Terminator::Jump(target) => {
                writeln!(output, "  br label %L{}", target.0).expect("写入字符串不会失败");
            }
            Terminator::Branch {
                condition,
                then_label,
                else_label,
            } => {
                let condition =
                    load_operand(self, output, condition, place_types, slots, registers)?;
                writeln!(
                    output,
                    "  br i1 {condition}, label %L{}, label %L{}",
                    then_label.0, else_label.0
                )
                .expect("写入字符串不会失败");
            }
            Terminator::Return(Some(value)) => {
                let value = load_operand(self, output, value, place_types, slots, registers)?;
                let ty = llvm_type(&function.return_type)?;
                writeln!(output, "  ret {ty} {value}").expect("写入字符串不会失败");
            }
            Terminator::Return(None) => {
                if llvm_type(&function.return_type)? == "void" {
                    output.push_str("  ret void\n");
                } else {
                    output.push_str("  unreachable\n");
                }
            }
        }

        Ok(())
    }

    fn emit_main(&mut self, output: &mut String, program: &Program) {
        let Some(main) = program.function("主") else {
            return;
        };

        if !main.params.is_empty() {
            self.report(
                first_span(main),
                "LLVM 后端暂不支持带参数的入口函数「主」".to_owned(),
            );
            return;
        }

        let return_type = match llvm_type(&main.return_type) {
            Ok(ty) => ty,
            Err(message) => {
                self.report(first_span(main), message);
                return;
            }
        };

        output.push_str("define i32 @main() {\nentry:\n");
        match return_type {
            "i64" => {
                output.push_str("  %li_main = call i64 @\"主\"()\n");
                output.push_str("  %li_exit = trunc i64 %li_main to i32\n");
                output.push_str("  ret i32 %li_exit\n");
            }
            "i1" => {
                output.push_str("  %li_main = call i1 @\"主\"()\n");
                output.push_str("  %li_exit = zext i1 %li_main to i32\n");
                output.push_str("  ret i32 %li_exit\n");
            }
            "double" => {
                output.push_str("  %li_main = call double @\"主\"()\n");
                output.push_str("  %li_exit = fptosi double %li_main to i32\n");
                output.push_str("  ret i32 %li_exit\n");
            }
            _ => {
                output.push_str("  call void @\"主\"()\n");
                output.push_str("  ret i32 0\n");
            }
        }
        output.push_str("}\n");
    }

    /// 枚举预检：LLVM 骨架目前只支持"纯标签枚举"（所有枚举项都没有载荷）；
    /// 带载荷的枚举需要 GC 运行时对象，报黄字警告拦截。
    ///
    /// 当前 IR 层不区分"纯标签"与"带载荷"，统一用 `IrType::Enum(name)` 表示；
    /// 因此只要发现 `IrType::Enum`，就保守地报黄字警告。
    fn warn_enum_support(&mut self, program: &Program) {
        for function in &program.functions {
            let mut has_enum = false;

            if matches!(function.return_type, IrType::Enum(_)) {
                has_enum = true;
            }
            for param in &function.params {
                if matches!(param.ty, IrType::Enum(_)) {
                    has_enum = true;
                }
            }
            for local in &function.locals {
                if matches!(local.ty, IrType::Enum(_)) {
                    has_enum = true;
                }
            }

            if has_enum {
                let span = first_span(function);
                self.diagnostics.push(
                    Diagnostic::warning(format!(
                        "LLVM 后端暂只支持纯标签枚举；函数「{}」使用了枚举类型，\
                         若该枚举携带堆数据（带载荷），运行期行为未定义；\
                         建议改用 VM 或 JS 后端编译",
                        function.name
                    ))
                    .with_span(span),
                );
            }
        }
    }

    fn report(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(message).with_span(span));
    }
}

fn first_span(function: &Function) -> Span {
    function
        .blocks
        .first()
        .map(|block| block.terminator_span)
        .unwrap_or_default()
}

fn llvm_type(ty: &IrType) -> Result<&'static str, String> {
    let name = match ty {
        IrType::Integer => "i64",
        IrType::Float => "double",
        IrType::Bool => "i1",
        IrType::String => "ptr",
        IrType::RawPointer(_) => "ptr",
        IrType::Null => "ptr",
        // 枚举类型：LLVM 骨架目前只支持"纯标签枚举"（所有枚举项都没有
        // 载荷）；带载荷的枚举需要 GC 运行时对象，由调用方在降级阶段
        // 报黄色警告拦截，不进入代码生成。
        IrType::Enum(_) => "i64",
        // 未知类型：用 ptr 占位，避免整个编译失败；实际值由上层语义
        // 检查（Sema）保证不会真正参与运算。
        IrType::Unknown => "ptr",
        other => {
            return Err(format!(
                "类型「{other}」需要 GC 运行时对象，当前 LLVM 骨架尚未实现"
            ));
        }
    };
    Ok(name)
}

fn constant_value(
    generator: &mut Generator,
    constant: &Constant,
    ty: &IrType,
) -> Result<String, String> {
    let value = match constant {
        Constant::Integer(value) => value.to_string(),
        Constant::Float(value) => format!("{value:.17e}"),
        Constant::Bool(true) => "true".to_owned(),
        Constant::Bool(false) => "false".to_owned(),
        Constant::String(value) => generator.strings.intern(value),
        Constant::Null => "null".to_owned(),
    };

    match (constant, ty) {
        (Constant::Integer(_), IrType::Integer)
        | (Constant::Float(_), IrType::Float)
        | (Constant::Bool(_), IrType::Bool)
        | (Constant::String(_), IrType::String)
        | (Constant::Null, IrType::RawPointer(_))
        | (Constant::Null, IrType::Null) => Ok(value),
        (Constant::Null, IrType::String) => Ok("null".to_owned()),
        // 枚举项构造器：字符串判别符 + 未知/枚举类型。
        (Constant::String(_), IrType::Unknown | IrType::Enum(_)) => Ok(value),
        // 枚举判别符：整数索引 + 枚举类型（纯标签枚举降级为 i64）。
        (Constant::Integer(_), IrType::Enum(_)) => Ok(value),
        _ => Err(format!("常量 {constant:?} 与类型「{ty}」不匹配")),
    }
}

fn load_operand(
    generator: &mut Generator,
    output: &mut String,
    operand: &Operand,
    place_types: &HashMap<Place, IrType>,
    slots: &SlotMap,
    registers: &mut RegisterAllocator,
) -> Result<String, String> {
    match operand {
        Operand::Constant(constant) => {
            let ty = constant_type(constant);
            constant_value(generator, constant, &ty)
        }
        Operand::Place(place) => {
            let slot = slots
                .get(place)
                .ok_or_else(|| format!("内部错误：位置「{place}」没有分配 LLVM 槽位"))?;
            let ty = place_types
                .get(place)
                .ok_or_else(|| format!("内部错误：位置「{place}」缺少静态类型"))?;
            let llvm_ty = llvm_type(ty)?;
            let result = registers.next();
            writeln!(output, "  {result} = load {llvm_ty}, ptr %slot{}", slot.id)
                .expect("写入字符串不会失败");
            Ok(result)
        }
    }
}

fn store_value(
    output: &mut String,
    slots: &SlotMap,
    dest: &Place,
    ty: &IrType,
    value: &str,
) -> Result<(), String> {
    let slot = slots
        .get(dest)
        .ok_or_else(|| format!("内部错误：位置「{dest}」没有分配 LLVM 槽位"))?;
    let llvm_ty = llvm_type(ty)?;
    writeln!(output, "  store {llvm_ty} {value}, ptr %slot{}", slot.id)
        .expect("写入字符串不会失败");
    Ok(())
}

fn binary_expression(
    op: &BinaryOp,
    operand_ty: &IrType,
    result_ty: &IrType,
    left: &str,
    right: &str,
    result: &str,
) -> Result<String, String> {
    let instruction = match op {
        BinaryOp::Add => "add",
        BinaryOp::Subtract => "sub",
        BinaryOp::Multiply => "mul",
        BinaryOp::Divide if matches!(operand_ty, IrType::Integer) => "sdiv",
        BinaryOp::Divide if matches!(operand_ty, IrType::Float) => "fdiv",
        BinaryOp::Divide => {
            return Err(format!("LLVM 后端暂不支持类型「{operand_ty}」的除法"));
        }
        BinaryOp::And if matches!(operand_ty, IrType::Bool) => "and",
        BinaryOp::Or if matches!(operand_ty, IrType::Bool) => "or",
        BinaryOp::And | BinaryOp::Or => {
            return Err(format!("LLVM 后端暂不支持类型「{operand_ty}」的逻辑运算"));
        }
        BinaryOp::Equal => match operand_ty {
            IrType::Integer | IrType::Bool => "icmp eq",
            IrType::RawPointer(_) => "icmp eq",
            IrType::Float => "fcmp oeq",
            // 枚举类型：用指针比较（字符串判别符）。
            IrType::Enum(_) => "icmp eq",
            other => {
                return Err(format!("LLVM 后端暂不支持类型「{other}」的相等比较"));
            }
        },
        BinaryOp::Less => match operand_ty {
            IrType::Integer => "icmp slt",
            IrType::Float => "fcmp olt",
            other => {
                return Err(format!("LLVM 后端暂不支持类型「{other}」的小于比较"));
            }
        },
        BinaryOp::Greater => match operand_ty {
            IrType::Integer => "icmp sgt",
            IrType::Float => "fcmp ogt",
            other => {
                return Err(format!("LLVM 后端暂不支持类型「{other}」的大于比较"));
            }
        },
    };

    let llvm_operand = llvm_type(operand_ty)?;
    let is_comparison = matches!(op, BinaryOp::Equal | BinaryOp::Less | BinaryOp::Greater);

    if is_comparison && !matches!(result_ty, IrType::Bool) {
        return Err(format!(
            "内部错误：比较运算的结果类型应为布尔，实际为「{result_ty}」"
        ));
    }

    if !is_comparison && operand_ty != result_ty {
        return Err(format!(
            "内部错误：运算的操作数类型「{operand_ty}」与结果类型「{result_ty}」不一致"
        ));
    }

    Ok(format!(
        "{result} = {instruction} {llvm_operand} {left}, {right}"
    ))
}

fn collect_place_types(function: &Function) -> HashMap<Place, IrType> {
    let mut types = HashMap::new();

    for local in &function.locals {
        types.insert(Place::Local(local.name.clone()), local.ty.clone());
    }

    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Some((dest, ty)) = destination_type(instruction) {
                types.insert(dest, ty.clone());
            }
        }
    }

    types
}

fn destination_type(instruction: &Instruction) -> Option<(Place, &IrType)> {
    match instruction {
        Instruction::Const { dest, ty, .. }
        | Instruction::Copy { dest, ty, .. }
        | Instruction::Unary { dest, ty, .. }
        | Instruction::Binary { dest, ty, .. }
        | Instruction::Await { dest, ty, .. }
        | Instruction::BuiltinCall { dest, ty, .. }
        | Instruction::BuiltinFunctionCall { dest, ty, .. }
        | Instruction::Input { dest, ty }
        | Instruction::MakeList { dest, ty, .. }
        | Instruction::GetField { dest, ty, .. }
        | Instruction::GetIndex { dest, ty, .. }
        | Instruction::WrapOptional { dest, ty, .. }
        | Instruction::WrapResult { dest, ty, .. }
        | Instruction::IsSuccess { dest, ty, .. }
        | Instruction::Unwrap { dest, ty, .. }
        | Instruction::UnwrapError { dest, ty, .. } => Some((dest.clone(), ty)),
        Instruction::Call { dest, ty, .. } => dest.clone().map(|dest| (dest, ty)),
        Instruction::SetField { .. }
        | Instruction::SetIndex { .. }
        | Instruction::Print { .. }
        | Instruction::Nop => None,
    }
}

fn operand_type(operand: &Operand, place_types: &HashMap<Place, IrType>) -> Result<IrType, String> {
    match operand {
        Operand::Constant(constant) => Ok(constant_type(constant)),
        Operand::Place(place) => place_types
            .get(place)
            .cloned()
            .ok_or_else(|| format!("内部错误：位置「{place}」缺少静态类型")),
    }
}

fn constant_type(constant: &Constant) -> IrType {
    match constant {
        Constant::Integer(_) => IrType::Integer,
        Constant::Float(_) => IrType::Float,
        Constant::Bool(_) => IrType::Bool,
        Constant::String(_) => IrType::String,
        Constant::Null => IrType::Null,
    }
}

fn instruction_name(instruction: &Instruction) -> &'static str {
    match instruction {
        Instruction::Const { .. } => "常量",
        Instruction::Copy { .. } => "复制",
        Instruction::Unary { .. } => "一元运算",
        Instruction::Binary { .. } => "二元运算",
        Instruction::Call { .. } => "函数调用",
        Instruction::Await { .. } => "等待",
        Instruction::BuiltinCall { .. } => "内置方法",
        Instruction::BuiltinFunctionCall { .. } => "内置函数",
        Instruction::Input { .. } => "标准输入",
        Instruction::MakeList { .. } => "列表构造",
        Instruction::GetField { .. } => "字段读取",
        Instruction::SetField { .. } => "字段写入",
        Instruction::GetIndex { .. } => "下标读取",
        Instruction::SetIndex { .. } => "下标写入",
        Instruction::WrapOptional { .. } => "可选值包装",
        Instruction::WrapResult { .. } => "结果值包装",
        Instruction::IsSuccess { .. } => "成功判断",
        Instruction::Unwrap { .. } => "成功值解包",
        Instruction::UnwrapError { .. } => "失败值解包",
        Instruction::Print { .. } => "打印",
        Instruction::Nop => "空操作",
    }
}

struct RegisterAllocator {
    prefix: String,
    next: usize,
}

impl RegisterAllocator {
    /// 为每个基本块创建独立的寄存器前缀，避免同一函数内重名。
    fn for_block(label: usize) -> Self {
        Self {
            prefix: format!("b{label}_"),
            next: 0,
        }
    }

    fn next(&mut self) -> String {
        let register = format!("%{}r{}", self.prefix, self.next);
        self.next += 1;
        register
    }
}

#[derive(Clone)]
struct Slot {
    id: usize,
}

#[derive(Default)]
struct SlotMap {
    slots: HashMap<Place, Slot>,
    next: usize,
}

impl SlotMap {
    fn declare(&mut self, output: &mut String, place: Place, ty: &IrType) -> Result<(), String> {
        if self.slots.contains_key(&place) {
            return Ok(());
        }

        let llvm_ty = llvm_type(ty)?;
        let id = self.next;
        self.next += 1;
        writeln!(output, "  %slot{id} = alloca {llvm_ty}").expect("写入字符串不会失败");
        self.slots.insert(place, Slot { id });
        Ok(())
    }

    fn get(&self, place: &Place) -> Option<&Slot> {
        self.slots.get(place)
    }
}

#[derive(Default)]
struct StringPool {
    entries: Vec<(String, String)>,
    lookup: HashMap<String, String>,
}

impl StringPool {
    fn intern(&mut self, text: &str) -> String {
        if let Some(name) = self.lookup.get(text) {
            return name.clone();
        }

        let name = format!("@.li.str.{}", self.entries.len());
        self.lookup.insert(text.to_owned(), name.clone());
        self.entries.push((name.clone(), text.to_owned()));
        name
    }

    fn render(&self) -> String {
        let mut output = String::new();

        for (name, text) in &self.entries {
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(0);
            writeln!(
                output,
                "{name} = private unnamed_addr constant [{} x i8] c\"{}\"",
                bytes.len(),
                escape_llvm_string(&bytes)
            )
            .expect("写入字符串不会失败");
        }

        output
    }
}

fn escape_llvm_string(bytes: &[u8]) -> String {
    let mut escaped = String::new();

    for &byte in bytes {
        match byte {
            b'"' => escaped.push_str("\\22"),
            b'\\' => escaped.push_str("\\5C"),
            0x20..=0x7E => escaped.push(byte as char),
            _ => write!(escaped, "\\{byte:02X}").expect("写入字符串不会失败"),
        }
    }

    escaped
}
