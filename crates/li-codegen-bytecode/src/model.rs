//! 字节码的数据结构、指令集定义与结构校验。
//!
//! 字节码采用精简的栈式指令集：每个函数拥有一段线性指令序列，
//! 指令地址就是该函数内的指令下标。跳转指令保存目标指令地址，
//! 所有局部变量与临时值统一映射为局部槽位。

use std::collections::HashSet;
use std::fmt;

use li_errors::Span;
use li_ir::{BuiltinFunction, BuiltinMethod};

/// 当前 `.lic` 文件格式的版本号。
pub const BYTECODE_VERSION: u32 = 1;

/// 栈式虚拟机指令。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Opcode {
    /// 压入常量池中的常量。
    PushConst,
    /// 把局部槽位的值压入操作数栈。
    Load,
    /// 从操作数栈弹出值并写入局部槽位。
    Store,
    /// 丢弃栈顶值。
    Pop,
    /// 整数或小数加法。
    Add,
    /// 整数或小数减法。
    Subtract,
    /// 整数或小数乘法。
    Multiply,
    /// 整数或小数除法。
    Divide,
    /// 算术取负。
    Negate,
    /// 逻辑非。
    Not,
    /// 小于比较。
    Less,
    /// 大于比较。
    Greater,
    /// 相等比较。
    Equal,
    /// 逻辑与。
    And,
    /// 逻辑或。
    Or,
    /// 无条件跳转。
    Jump,
    /// 栈顶为假时跳转，并弹出条件值。
    JumpIfFalse,
    /// 调用函数，后面紧跟实参个数。
    Call,
    /// 调用字符串或列表的内置方法，后面紧跟方法名与实参个数。
    InvokeBuiltin,
    /// 弹出返回值并结束当前函数。
    Return,
    /// 不返回值并结束当前函数。
    ReturnVoid,
    /// 停止虚拟机。
    Halt,
    /// 弹出栈顶值并输出到控制台。
    Print,
    /// 读取一行标准输入并压入栈顶。
    Input,
    /// 从栈顶弹出指定数量的元素构造列表。
    MakeList,
    /// 读取结构体字段。
    GetField,
    /// 写入结构体字段。
    SetField,
    /// 读取列表下标。
    GetIndex,
    /// 写入列表下标。
    SetIndex,
    /// 把栈顶值包装为成功可选值；参数为 1，参数为 0 时构造空值。
    WrapOptional,
    /// 把栈顶值包装为成功结果。
    WrapResult,
    /// 判断可选值或结果值是否成功。
    IsSuccess,
    /// 解包成功值。
    Unwrap,
    /// 解包失败值。
    UnwrapError,
    /// 空指令。
    Nop,
}

impl Opcode {
    /// 返回 `.lic` 文本中的指令助记符。
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Self::PushConst => "PUSH_CONST",
            Self::Load => "LOAD",
            Self::Store => "STORE",
            Self::Pop => "POP",
            Self::Add => "ADD",
            Self::Subtract => "SUB",
            Self::Multiply => "MUL",
            Self::Divide => "DIV",
            Self::Negate => "NEG",
            Self::Not => "NOT",
            Self::Less => "LT",
            Self::Greater => "GT",
            Self::Equal => "EQ",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Jump => "JUMP",
            Self::JumpIfFalse => "JUMP_IF_FALSE",
            Self::Call => "CALL",
            Self::InvokeBuiltin => "INVOKE_BUILTIN",
            Self::Return => "RETURN",
            Self::ReturnVoid => "RETURN_VOID",
            Self::Halt => "HALT",
            Self::Print => "PRINT",
            Self::Input => "INPUT",
            Self::MakeList => "MAKE_LIST",
            Self::GetField => "GET_FIELD",
            Self::SetField => "SET_FIELD",
            Self::GetIndex => "GET_INDEX",
            Self::SetIndex => "SET_INDEX",
            Self::WrapOptional => "WRAP_OPTIONAL",
            Self::WrapResult => "WRAP_RESULT",
            Self::IsSuccess => "IS_SUCCESS",
            Self::Unwrap => "UNWRAP",
            Self::UnwrapError => "UNWRAP_ERROR",
            Self::Nop => "NOP",
        }
    }

    /// 根据助记符解析指令。
    pub fn from_mnemonic(text: &str) -> Option<Self> {
        let opcode = match text {
            "PUSH_CONST" => Self::PushConst,
            "LOAD" => Self::Load,
            "STORE" => Self::Store,
            "POP" => Self::Pop,
            "ADD" => Self::Add,
            "SUB" => Self::Subtract,
            "MUL" => Self::Multiply,
            "DIV" => Self::Divide,
            "NEG" => Self::Negate,
            "NOT" => Self::Not,
            "LT" => Self::Less,
            "GT" => Self::Greater,
            "EQ" => Self::Equal,
            "AND" => Self::And,
            "OR" => Self::Or,
            "JUMP" => Self::Jump,
            "JUMP_IF_FALSE" => Self::JumpIfFalse,
            "CALL" => Self::Call,
            "INVOKE_BUILTIN" => Self::InvokeBuiltin,
            "RETURN" => Self::Return,
            "RETURN_VOID" => Self::ReturnVoid,
            "HALT" => Self::Halt,
            "PRINT" => Self::Print,
            "INPUT" => Self::Input,
            "MAKE_LIST" => Self::MakeList,
            "GET_FIELD" => Self::GetField,
            "SET_FIELD" => Self::SetField,
            "GET_INDEX" => Self::GetIndex,
            "SET_INDEX" => Self::SetIndex,
            "WRAP_OPTIONAL" => Self::WrapOptional,
            "WRAP_RESULT" => Self::WrapResult,
            "IS_SUCCESS" => Self::IsSuccess,
            "UNWRAP" => Self::Unwrap,
            "UNWRAP_ERROR" => Self::UnwrapError,
            "NOP" => Self::Nop,
            _ => return None,
        };
        Some(opcode)
    }
}

impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.mnemonic())
    }
}

/// 指令参数。槽位、常量下标、跳转地址使用整数，函数名与字段名使用名称。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstructionArg {
    /// 无符号整数参数。
    Integer(usize),
    /// 名称参数。
    Name(String),
}

impl fmt::Display for InstructionArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => write!(f, "{value}"),
            Self::Name(value) => f.write_str(value),
        }
    }
}

/// 一条字节码指令。
#[derive(Clone, Debug, PartialEq)]
pub struct Instruction {
    /// 操作码。
    pub opcode: Opcode,
    /// 参数列表。
    pub args: Vec<InstructionArg>,
    /// 该指令对应的 li 源码区间，用于运行时错误定位。
    pub span: Span,
}

impl Instruction {
    /// 创建一条指令。
    pub fn new(opcode: Opcode, args: Vec<InstructionArg>, span: Span) -> Self {
        Self { opcode, args, span }
    }

    /// 读取指定位置的整数参数。
    pub fn integer_arg(&self, index: usize) -> Option<usize> {
        match self.args.get(index) {
            Some(InstructionArg::Integer(value)) => Some(*value),
            _ => None,
        }
    }

    /// 读取指定位置的名称参数。
    pub fn name_arg(&self, index: usize) -> Option<&str> {
        match self.args.get(index) {
            Some(InstructionArg::Name(value)) => Some(value),
            _ => None,
        }
    }

    /// 检查参数数量与参数类型是否与操作码匹配。
    pub fn validate(&self) -> Result<(), String> {
        let expected = match self.opcode {
            Opcode::PushConst
            | Opcode::Load
            | Opcode::Store
            | Opcode::Jump
            | Opcode::JumpIfFalse
            | Opcode::MakeList
            | Opcode::WrapOptional => 1,
            Opcode::Call | Opcode::InvokeBuiltin => 2,
            Opcode::GetField | Opcode::SetField => 1,
            _ => 0,
        };

        if self.args.len() != expected {
            return Err(format!(
                "指令「{}」需要 {} 个参数，实际 {} 个",
                self.opcode,
                expected,
                self.args.len()
            ));
        }

        match self.opcode {
            Opcode::PushConst
            | Opcode::Load
            | Opcode::Store
            | Opcode::Jump
            | Opcode::JumpIfFalse
            | Opcode::MakeList
            | Opcode::WrapOptional => {
                if self.integer_arg(0).is_none() {
                    return Err(format!("指令「{}」的第一个参数必须是整数", self.opcode));
                }
            }
            Opcode::Call => {
                if self.name_arg(0).is_none() {
                    return Err("指令「CALL」的第一个参数必须是函数名".to_owned());
                }
                if self.integer_arg(1).is_none() {
                    return Err("指令「CALL」的第二个参数必须是实参个数".to_owned());
                }
            }
            Opcode::InvokeBuiltin => {
                let name = self.name_arg(0).unwrap_or("");
                let Some(argc) = self.integer_arg(1) else {
                    return Err("指令「INVOKE_BUILTIN」的第二个参数必须是实参个数".to_owned());
                };

                if let Some(method) = BuiltinMethod::from_qualified_name(name) {
                    if argc != method.arity() {
                        return Err(format!(
                            "内置方法「{name}」需要 {} 个实参，实际为 {argc} 个",
                            method.arity()
                        ));
                    }
                } else if let Some(function) = BuiltinFunction::from_name(name) {
                    if argc != function.arity() {
                        return Err(format!(
                            "内置函数「{name}」需要 {} 个实参，实际为 {argc} 个",
                            function.arity()
                        ));
                    }
                } else {
                    return Err(format!("未知的内置方法或函数「{name}」"));
                }
            }
            Opcode::GetField | Opcode::SetField => {
                if self.name_arg(0).is_none() {
                    return Err(format!("指令「{}」的参数必须是字段名", self.opcode));
                }
                if self.name_arg(0).is_some_and(str::is_empty) {
                    return Err(format!("指令「{}」的字段名不能为空", self.opcode));
                }
            }
            _ => {}
        }

        Ok(())
    }
}

/// 常量池中的常量。
#[derive(Clone, Debug, PartialEq)]
pub enum Constant {
    /// 整数字面量。
    Integer(i64),
    /// 小数字面量。
    Float(f64),
    /// 布尔字面量。
    Bool(bool),
    /// 字符串字面量。
    String(String),
    /// 空值。
    Null,
}

impl From<&li_ir::Constant> for Constant {
    fn from(constant: &li_ir::Constant) -> Self {
        match constant {
            li_ir::Constant::Integer(value) => Self::Integer(*value),
            li_ir::Constant::Float(value) => Self::Float(*value),
            li_ir::Constant::Bool(value) => Self::Bool(*value),
            li_ir::Constant::String(value) => Self::String(value.clone()),
            li_ir::Constant::Null => Self::Null,
        }
    }
}

/// 一个字节码函数。
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionCode {
    /// 函数名称。
    pub name: String,
    /// 参数个数。参数占用从 0 开始的前若干个局部槽位。
    pub arity: usize,
    /// 局部槽位名称，仅用于调试与错误信息。
    pub slots: Vec<String>,
    /// 指令序列。入口函数的第一条指令位于地址 0。
    pub code: Vec<Instruction>,
}

/// 一份完整的字节码程序。
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    /// 文件格式版本。
    pub version: u32,
    /// 程序入口函数名称。
    pub entry: Option<String>,
    /// 常量池。
    pub constants: Vec<Constant>,
    /// 函数列表。
    pub functions: Vec<FunctionCode>,
    /// 程序入口代码，负责调用入口函数并停机。
    pub entry_code: Vec<Instruction>,
}

impl Program {
    /// 按名称查找函数。
    pub fn function(&self, name: &str) -> Option<&FunctionCode> {
        self.functions.iter().find(|function| function.name == name)
    }

    /// 校验字节码的结构、常量下标、槽位下标、跳转地址与函数调用。
    pub fn validate(&self) -> Result<(), String> {
        if self.version != BYTECODE_VERSION {
            return Err(format!(
                "不支持的字节码版本 {}，期望版本 {}",
                self.version, BYTECODE_VERSION
            ));
        }

        let mut names = HashSet::new();
        for function in &self.functions {
            if !names.insert(function.name.as_str()) {
                return Err(format!("重复定义函数「{}」", function.name));
            }
            if function.arity > function.slots.len() {
                return Err(format!(
                    "函数「{}」声明了 {} 个参数，但只有 {} 个局部槽位",
                    function.name,
                    function.arity,
                    function.slots.len()
                ));
            }
            if function.code.is_empty() {
                return Err(format!("函数「{}」的指令序列不能为空", function.name));
            }

            for (address, instruction) in function.code.iter().enumerate() {
                let context = format!("函数「{}」第 {} 条指令", function.name, address);
                self.validate_instruction(instruction, Some(function), &context)?;
            }
        }

        for (address, instruction) in self.entry_code.iter().enumerate() {
            let context = format!("程序入口第 {} 条指令", address);
            self.validate_instruction(instruction, None, &context)?;
        }
        if self.entry_code.is_empty() {
            return Err("程序入口代码不能为空".to_owned());
        }

        if let Some(entry) = &self.entry {
            if self.function(entry).is_none() {
                return Err(format!("程序入口函数「{entry}」不存在"));
            }
        }

        Ok(())
    }

    fn validate_instruction(
        &self,
        instruction: &Instruction,
        owner: Option<&FunctionCode>,
        context: &str,
    ) -> Result<(), String> {
        instruction
            .validate()
            .map_err(|message| format!("{context}：{message}"))?;

        let code_len = owner.map_or(self.entry_code.len(), |function| function.code.len());

        match instruction.opcode {
            Opcode::PushConst => {
                let index = instruction.integer_arg(0).unwrap_or(usize::MAX);
                if index >= self.constants.len() {
                    return Err(format!(
                        "{context}：常量下标 {index} 越界，常量池长度为 {}",
                        self.constants.len()
                    ));
                }
            }
            Opcode::Load | Opcode::Store => {
                let index = instruction.integer_arg(0).unwrap_or(usize::MAX);
                match owner {
                    Some(function) if index < function.slots.len() => {}
                    Some(function) => {
                        return Err(format!(
                            "{context}：局部槽位 {index} 越界，函数「{}」只有 {} 个槽位",
                            function.name,
                            function.slots.len()
                        ));
                    }
                    None => {
                        return Err(format!("{context}：程序入口代码不能读写局部变量"));
                    }
                }
            }
            Opcode::Jump | Opcode::JumpIfFalse => {
                let target = instruction.integer_arg(0).unwrap_or(usize::MAX);
                if target >= code_len {
                    return Err(format!(
                        "{context}：跳转目标 {target} 越界，指令序列长度为 {code_len}"
                    ));
                }
            }
            Opcode::Call => {
                let name = instruction.name_arg(0).unwrap_or("");
                let argc = instruction.integer_arg(1).unwrap_or(usize::MAX);
                let Some(function) = self.function(name) else {
                    return Err(format!("{context}：调用了未定义的函数「{name}」"));
                };
                if argc != function.arity {
                    return Err(format!(
                        "{context}：调用函数「{name}」需要 {} 个实参，实际传入 {argc} 个",
                        function.arity
                    ));
                }
            }
            Opcode::WrapOptional => {
                let flag = instruction.integer_arg(0).unwrap_or(usize::MAX);
                if flag > 1 {
                    return Err(format!(
                        "{context}：WRAP_OPTIONAL 的参数只能是 0 或 1，实际为 {flag}"
                    ));
                }
            }
            _ => {}
        }

        Ok(())
    }
}
