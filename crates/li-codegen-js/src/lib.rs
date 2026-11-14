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

/// 生成的 JavaScript 运行时辅助函数。
///
/// 这些函数统一实现内置方法、文件 I/O 与显示格式，确保与 VM 后端的
/// 可见行为一致。字典使用 `Map`，保持插入顺序。
const RUNTIME: &str = r#"
let __li_fs = null;
try {
    if (typeof require === "function") {
        __li_fs = require("fs");
    }
} catch (_) {
    __li_fs = null;
}

function __li_成功(值) {
    return { __li_成功: true, __li_值: 值 };
}

function __li_失败(错误) {
    return { __li_成功: false, __li_错误: 错误 };
}

function __li_可选(值) {
    if (值 === null || 值 === undefined) {
        return { __li_成功: false };
    }
    return { __li_成功: true, __li_值: 值 };
}

function __li_显示(值) {
    if (值 === null || 值 === undefined) {
        return "空";
    }
    switch (typeof 值) {
        case "boolean":
            return 值 ? "真" : "假";
        case "number":
            return String(值);
        case "string":
            return 值;
        case "bigint":
            return String(值);
        default:
            break;
    }
    if (Array.isArray(值)) {
        return "[" + 值.map(__li_显示).join(", ") + "]";
    }
    if (值 instanceof Map) {
        const 项 = Array.from(值.entries())
            .map(([键, 内容]) => __li_显示(键) + ": " + __li_显示(内容))
            .join(", ");
        return "{" + 项 + "}";
    }
    if (typeof 值 === "object") {
        if (Object.prototype.hasOwnProperty.call(值, "__li_成功")) {
            if (值.__li_成功) {
                return "成功(" + __li_显示(值.__li_值) + ")";
            }
            return Object.prototype.hasOwnProperty.call(值, "__li_错误")
                ? "失败(" + __li_显示(值.__li_错误) + ")"
                : "空";
        }
        const 字段 = Object.keys(值)
            .filter((键) => !键.startsWith("__li_"))
            .sort()
            .map((键) => 键 + ": " + __li_显示(值[键]))
            .join(", ");
        return 值.__li_类型
            ? 值.__li_类型 + " { " + 字段 + " }"
            : "{ " + 字段 + " }";
    }
    return String(值);
}

function __li_期望字符串(值, 名称) {
    if (typeof 值 !== "string") {
        throw new Error("内置调用「" + 名称 + "」期望字符串，实际为「" + __li_显示(值) + "」");
    }
    return 值;
}

function __li_期望整数(值, 名称) {
    if (typeof 值 !== "number" || !Number.isInteger(值)) {
        throw new Error("内置调用「" + 名称 + "」期望整数，实际为「" + __li_显示(值) + "」");
    }
    return 值;
}

function __li_期望列表(值, 名称) {
    if (!Array.isArray(值)) {
        throw new Error("内置调用「" + 名称 + "」期望列表，实际为「" + __li_显示(值) + "」");
    }
    return 值;
}

function __li_期望字典(值, 名称) {
    if (!(值 instanceof Map)) {
        throw new Error("内置调用「" + 名称 + "」期望字典，实际为「" + __li_显示(值) + "」");
    }
    return 值;
}

function __li_字典键(值) {
    const 类型 = typeof 值;
    if (类型 === "string" || 类型 === "boolean" ||
        (类型 === "number" && !Number.isNaN(值))) {
        return 值;
    }
    throw new Error("字典键必须是字符串、整数、小数或布尔，实际为「" + __li_显示(值) + "」");
}

function __li_解析整数(文本) {
    if (!/^[+-]?\d+$/.test(文本)) {
        return __li_失败("不是有效的整数：「" + 文本 + "」");
    }
    try {
        const 值 = BigInt(文本);
        if (值 < -9223372036854775808n || 值 > 9223372036854775807n) {
            return __li_失败("整数超出 64 位范围：「" + 文本 + "」");
        }
        return __li_成功(Number(值));
    } catch (_) {
        return __li_失败("整数超出 64 位范围：「" + 文本 + "」");
    }
}

function __li_调用内置(名称, 参数) {
    const 接收者 = 参数[0];
    switch (名称) {
        case "字符串.长度":
            return Array.from(__li_期望字符串(接收者, 名称)).length;
        case "字符串.拼接":
            return __li_期望字符串(接收者, 名称) + __li_期望字符串(参数[1], 名称);
        case "字符串.转整数":
            return __li_解析整数(__li_期望字符串(接收者, 名称));
        case "字符串.包含":
            return __li_期望字符串(接收者, 名称).includes(__li_期望字符串(参数[1], 名称));
        case "字符串.字符": {
            const 字符 = Array.from(__li_期望字符串(接收者, 名称));
            const 下标 = __li_期望整数(参数[1], 名称);
            return 下标 >= 0 && 下标 < 字符.length
                ? __li_可选(字符[下标])
                : { __li_成功: false };
        }
        case "字符串.切片": {
            const 字符 = Array.from(__li_期望字符串(接收者, 名称));
            const 起始 = Math.max(0, Math.min(__li_期望整数(参数[1], 名称), 字符.length));
            const 结束 = Math.max(0, Math.min(__li_期望整数(参数[2], 名称), 字符.length));
            return 起始 < 结束 ? 字符.slice(起始, 结束).join("") : "";
        }
        case "字符串.查找": {
            const 文本 = __li_期望字符串(接收者, 名称);
            const 字节 = 文本.indexOf(__li_期望字符串(参数[1], 名称));
            return 字节 < 0
                ? { __li_成功: false }
                : __li_可选(Array.from(文本.slice(0, 字节)).length);
        }
        case "字符串.替换":
            return __li_期望字符串(接收者, 名称).split(__li_期望字符串(参数[1], 名称))
                .join(__li_期望字符串(参数[2], 名称));
        case "字符串.分割": {
            const 文本 = __li_期望字符串(接收者, 名称);
            const 分隔符 = __li_期望字符串(参数[1], 名称);
            return 分隔符 === "" ? Array.from(文本) : 文本.split(分隔符);
        }
        case "列表.长度":
            return __li_期望列表(接收者, 名称).length;
        case "列表.添加":
            __li_期望列表(接收者, 名称).push(参数[1]);
            return 接收者;
        case "列表.获取": {
            const 列表 = __li_期望列表(接收者, 名称);
            const 下标 = __li_期望整数(参数[1], 名称);
            return 下标 >= 0 && 下标 < 列表.length
                ? __li_可选(列表[下标])
                : { __li_成功: false };
        }
        case "列表.删除": {
            const 列表 = __li_期望列表(接收者, 名称);
            const 下标 = __li_期望整数(参数[1], 名称);
            if (下标 < 0 || 下标 >= 列表.length) {
                return false;
            }
            列表.splice(下标, 1);
            return true;
        }
        case "字典.长度":
            return __li_期望字典(接收者, 名称).size;
        case "字典.插入": {
            const 字典 = __li_期望字典(接收者, 名称);
            字典.set(__li_字典键(参数[1]), 参数[2]);
            return 接收者;
        }
        case "字典.获取": {
            const 字典 = __li_期望字典(接收者, 名称);
            const 键 = __li_字典键(参数[1]);
            return 字典.has(键) ? __li_可选(字典.get(键)) : { __li_成功: false };
        }
        case "字典.删除": {
            const 字典 = __li_期望字典(接收者, 名称);
            return 字典.delete(__li_字典键(参数[1]));
        }
        case "字典.包含键": {
            const 字典 = __li_期望字典(接收者, 名称);
            return 字典.has(__li_字典键(参数[1]));
        }
        case "读取文本": {
            const 路径 = __li_期望字符串(参数[0], 名称);
            if (__li_fs === null) {
                return __li_失败("当前 JavaScript 环境不支持文件读写");
            }
            try {
                return __li_成功(__li_fs.readFileSync(路径, "utf8"));
            } catch (错误) {
                return __li_失败("无法读取文件「" + 路径 + "」：" + 错误.message);
            }
        }
        case "写入文本": {
            const 路径 = __li_期望字符串(参数[0], 名称);
            const 内容 = __li_期望字符串(参数[1], 名称);
            if (__li_fs === null) {
                return __li_失败("当前 JavaScript 环境不支持文件读写");
            }
            try {
                __li_fs.writeFileSync(路径, 内容, "utf8");
                const 字节数 = typeof Buffer === "function"
                    ? Buffer.byteLength(内容, "utf8")
                    : 内容.length;
                return __li_成功(字节数);
            } catch (错误) {
                return __li_失败("无法写入文件「" + 路径 + "」：" + 错误.message);
            }
        }
        case "新建字典":
            return new Map();
        default:
            throw new Error("未知的内置方法或函数「" + 名称 + "」");
    }
}
"#;

/// 把中立 IR 生成为 JavaScript 源码。
///
/// JavaScript 目标不支持裸指针与手动内存操作；遇到 `不安全` 块或
/// `原生指针<T>` 时直接返回中文错误，由调用方报告。
pub fn generate(program: &Program) -> Result<String, String> {
    if program.uses_unsafe || program_uses_raw_pointer(program) {
        return Err(
            "当前目标平台不支持底层内存操作：JavaScript 后端不支持「不安全」块或「原生指针<T>」"
                .to_owned(),
        );
    }

    let mut generator = Generator::new(program);
    Ok(generator.generate())
}

/// 检查程序签名或局部变量中是否出现原生指针。
fn program_uses_raw_pointer(program: &Program) -> bool {
    program.functions.iter().any(|function| {
        function.return_type.contains_raw_pointer()
            || function
                .params
                .iter()
                .any(|param| param.ty.contains_raw_pointer())
            || function
                .locals
                .iter()
                .any(|local| local.ty.contains_raw_pointer())
    })
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
        writer.line(RUNTIME.trim_end());

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
            Instruction::BuiltinCall {
                dest,
                receiver,
                method,
                args,
                ..
            } => {
                let mut operands = Vec::with_capacity(args.len() + 1);
                operands.push(self.operand_js_name(receiver, local_names));
                for argument in args {
                    operands.push(self.operand_js_name(argument, local_names));
                }
                writer.line(format!(
                    "{} = __li_调用内置({}, [{}]);",
                    self.place_js_name(dest, local_names),
                    string_literal(method.qualified_name()),
                    operands.join(", ")
                ));
            }
            Instruction::BuiltinFunctionCall {
                dest,
                function,
                args,
                ..
            } => {
                let operands = args
                    .iter()
                    .map(|argument| self.operand_js_name(argument, local_names))
                    .collect::<Vec<_>>()
                    .join(", ");
                writer.line(format!(
                    "{} = __li_调用内置({}, [{}]);",
                    self.place_js_name(dest, local_names),
                    string_literal(function.name()),
                    operands
                ));
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
                    "console.log(__li_显示({}));",
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
