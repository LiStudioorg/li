//! 中立中间表示的数据结构。
//!
//! 该 IR 与任何具体后端解耦，保留 li 的静态类型信息，并使用
//! 三地址码描述数据流、使用基本块与控制流终止指令描述控制流图。
//! 每个基本块以 [`Label`] 标识，块末尾的 [`Terminator`] 对应
//! `JUMP`、`BRANCH` 或 `RETURN` 指令。

use std::fmt;

use li_errors::Span;

/// IR 中保留的静态类型。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IrType {
    /// 64 位有符号整数。
    Integer,
    /// 64 位浮点数。
    Float,
    /// 布尔值。
    Bool,
    /// 字符串。
    String,
    /// 空值。
    Null,
    /// 列表类型。
    List(Box<IrType>),
    /// 字典类型。
    Dictionary {
        /// 键类型。
        key: Box<IrType>,
        /// 值类型。
        value: Box<IrType>,
    },
    /// 原生指针类型。
    RawPointer(Box<IrType>),
    /// 可选类型。
    Optional(Box<IrType>),
    /// 结果类型。
    Result {
        /// 成功值类型。
        ok: Box<IrType>,
        /// 失败值类型。
        error: Box<IrType>,
    },
    /// 程序内声明的结构体类型。
    Struct(String),
    /// 无法确定或不受支持的类型。
    Unknown,
}

impl IrType {
    /// 是否为可选类型。
    pub fn is_optional(&self) -> bool {
        matches!(self, Self::Optional(_))
    }

    /// 是否为结果类型。
    pub fn is_result(&self) -> bool {
        matches!(self, Self::Result { .. })
    }

    /// 返回可选类型的内部类型。
    pub fn optional_inner(&self) -> Option<&IrType> {
        match self {
            Self::Optional(inner) => Some(inner),
            _ => None,
        }
    }

    /// 返回结果类型的成功值类型。
    pub fn result_ok(&self) -> Option<&IrType> {
        match self {
            Self::Result { ok, .. } => Some(ok),
            _ => None,
        }
    }

    /// 返回结果类型的失败值类型。
    pub fn result_error(&self) -> Option<&IrType> {
        match self {
            Self::Result { error, .. } => Some(error),
            _ => None,
        }
    }

    /// 是否包含原生指针类型。
    pub fn contains_raw_pointer(&self) -> bool {
        match self {
            Self::RawPointer(_) => true,
            Self::List(element) => element.contains_raw_pointer(),
            Self::Dictionary { key, value } => {
                key.contains_raw_pointer() || value.contains_raw_pointer()
            }
            Self::Optional(inner) => inner.contains_raw_pointer(),
            Self::Result { ok, error } => ok.contains_raw_pointer() || error.contains_raw_pointer(),
            _ => false,
        }
    }
}

impl fmt::Display for IrType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer => f.write_str("整数"),
            Self::Float => f.write_str("小数"),
            Self::Bool => f.write_str("布尔"),
            Self::String => f.write_str("字符串"),
            Self::Null => f.write_str("空"),
            Self::List(element) => write!(f, "列表<{element}>"),
            Self::Dictionary { key, value } => write!(f, "字典<{key}, {value}>"),
            Self::RawPointer(inner) => write!(f, "原生指针<{inner}>"),
            Self::Optional(inner) => write!(f, "可选<{inner}>"),
            Self::Result { ok, error } => write!(f, "结果<{ok}, {error}>"),
            Self::Struct(name) => f.write_str(name),
            Self::Unknown => f.write_str("未知类型"),
        }
    }
}

/// 字面量常量。
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
    /// 空值字面量。
    Null,
}

impl Constant {
    /// 返回常量对应的类型。
    pub fn ty(&self) -> IrType {
        match self {
            Self::Integer(_) => IrType::Integer,
            Self::Float(_) => IrType::Float,
            Self::Bool(_) => IrType::Bool,
            Self::String(_) => IrType::String,
            Self::Null => IrType::Null,
        }
    }
}

impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value:?}"),
            Self::Bool(value) => f.write_str(if *value { "真" } else { "假" }),
            Self::String(value) => write!(f, "「{value}」"),
            Self::Null => f.write_str("空"),
        }
    }
}

/// 字符串与列表的内置方法。
///
/// 内置方法是语言标准库的一部分，而不是某个目标平台的细节；
/// IR 只记录方法的稳定标识，由各后端映射为原生操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinMethod {
    /// 字符串长度，按 Unicode 字符计数。
    StringLength,
    /// 字符串拼接。
    StringConcat,
    /// 字符串转整数，失败时返回错误说明。
    StringParseInteger,
    /// 字符串是否包含子串。
    StringContains,
    /// 按 Unicode 字符下标取字符。
    StringChar,
    /// 按 Unicode 字符下标取切片。
    StringSlice,
    /// 查找子串，返回 Unicode 字符下标。
    StringFind,
    /// 替换全部子串。
    StringReplace,
    /// 按分隔符分割字符串。
    StringSplit,
    /// 列表长度。
    ListLength,
    /// 列表追加元素。
    ListAdd,
    /// 列表按下标获取元素，越界返回空。
    ListGet,
    /// 列表按下标删除元素。
    ListRemove,
    /// 字典长度。
    DictionaryLength,
    /// 字典插入键值对。
    DictionaryInsert,
    /// 字典按键获取值。
    DictionaryGet,
    /// 字典按键删除。
    DictionaryRemove,
    /// 字典是否包含键。
    DictionaryContainsKey,
}

impl BuiltinMethod {
    /// 返回 `.lic` 与诊断中使用的稳定名称。
    pub const fn qualified_name(self) -> &'static str {
        match self {
            Self::StringLength => "字符串.长度",
            Self::StringConcat => "字符串.拼接",
            Self::StringParseInteger => "字符串.转整数",
            Self::StringContains => "字符串.包含",
            Self::StringChar => "字符串.字符",
            Self::StringSlice => "字符串.切片",
            Self::StringFind => "字符串.查找",
            Self::StringReplace => "字符串.替换",
            Self::StringSplit => "字符串.分割",
            Self::ListLength => "列表.长度",
            Self::ListAdd => "列表.添加",
            Self::ListGet => "列表.获取",
            Self::ListRemove => "列表.删除",
            Self::DictionaryLength => "字典.长度",
            Self::DictionaryInsert => "字典.插入",
            Self::DictionaryGet => "字典.获取",
            Self::DictionaryRemove => "字典.删除",
            Self::DictionaryContainsKey => "字典.包含键",
        }
    }

    /// 根据稳定名称解析内置方法。
    pub fn from_qualified_name(name: &str) -> Option<Self> {
        let method = match name {
            "字符串.长度" => Self::StringLength,
            "字符串.拼接" => Self::StringConcat,
            "字符串.转整数" => Self::StringParseInteger,
            "字符串.包含" => Self::StringContains,
            "字符串.字符" => Self::StringChar,
            "字符串.切片" => Self::StringSlice,
            "字符串.查找" => Self::StringFind,
            "字符串.替换" => Self::StringReplace,
            "字符串.分割" => Self::StringSplit,
            "列表.长度" => Self::ListLength,
            "列表.添加" => Self::ListAdd,
            "列表.获取" => Self::ListGet,
            "列表.删除" => Self::ListRemove,
            "字典.长度" => Self::DictionaryLength,
            "字典.插入" => Self::DictionaryInsert,
            "字典.获取" => Self::DictionaryGet,
            "字典.删除" => Self::DictionaryRemove,
            "字典.包含键" => Self::DictionaryContainsKey,
            _ => return None,
        };
        Some(method)
    }

    /// 返回方法的实参个数，不包含接收者。
    pub const fn arity(self) -> usize {
        match self {
            Self::StringLength
            | Self::StringParseInteger
            | Self::ListLength
            | Self::DictionaryLength => 0,
            Self::StringConcat
            | Self::StringContains
            | Self::StringChar
            | Self::StringFind
            | Self::StringSplit
            | Self::ListAdd
            | Self::ListGet
            | Self::ListRemove
            | Self::DictionaryGet
            | Self::DictionaryRemove
            | Self::DictionaryContainsKey => 1,
            Self::StringSlice | Self::StringReplace | Self::DictionaryInsert => 2,
        }
    }
}

impl fmt::Display for BuiltinMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.qualified_name())
    }
}

/// 语言级内置函数，主要提供宿主能力。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinFunction {
    /// 读取整个文本文件。
    ReadText,
    /// 写入文本文件。
    WriteText,
    /// 创建一个空字典。
    NewDictionary,
    /// 当前 Unix 时间戳（秒）。
    SystemTimestamp,
    /// 命令行参数列表。
    SystemArguments,
    /// 按键读取环境变量。
    SystemEnvironment,
    /// 判断字符串是否匹配正则。
    RegexMatch,
    /// 按正则全局替换字符串。
    RegexReplace,
    /// 解析 JSON 对象为字符串字典（简化版）。
    JsonParse,
    /// 把字符串字典生成紧凑 JSON 文本。
    JsonGenerate,
    /// 发起同步 HTTP GET 请求。
    NetworkGet,
    /// Base64 编码文本。
    CryptoBase64Encode,
    /// 计算文本的 SHA-256，返回小写十六进制摘要。
    CryptoSha256,
}

impl BuiltinFunction {
    /// 返回 `.lic` 与诊断中使用的稳定名称。
    pub const fn name(self) -> &'static str {
        match self {
            Self::ReadText => "读取文本",
            Self::WriteText => "写入文本",
            Self::NewDictionary => "新建字典",
            Self::SystemTimestamp => "系统.时间戳",
            Self::SystemArguments => "系统.获取参数",
            Self::SystemEnvironment => "系统.获取环境变量",
            Self::RegexMatch => "正则.匹配",
            Self::RegexReplace => "正则.替换",
            Self::JsonParse => "JSON.解析",
            Self::JsonGenerate => "JSON.生成",
            Self::NetworkGet => "网络.获取",
            Self::CryptoBase64Encode => "加密.Base64编码",
            Self::CryptoSha256 => "加密.SHA256",
        }
    }

    /// 根据稳定名称解析内置函数。
    pub fn from_name(name: &str) -> Option<Self> {
        let function = match name {
            "读取文本" => Self::ReadText,
            "写入文本" => Self::WriteText,
            "新建字典" => Self::NewDictionary,
            "系统.时间戳" => Self::SystemTimestamp,
            "系统.获取参数" => Self::SystemArguments,
            "系统.获取环境变量" => Self::SystemEnvironment,
            "正则.匹配" => Self::RegexMatch,
            "正则.替换" => Self::RegexReplace,
            "JSON.解析" => Self::JsonParse,
            "JSON.生成" => Self::JsonGenerate,
            "网络.获取" => Self::NetworkGet,
            "加密.Base64编码" => Self::CryptoBase64Encode,
            "加密.SHA256" => Self::CryptoSha256,
            _ => return None,
        };
        Some(function)
    }

    /// 返回实参个数。
    pub const fn arity(self) -> usize {
        match self {
            Self::NewDictionary | Self::SystemTimestamp | Self::SystemArguments => 0,
            Self::ReadText
            | Self::SystemEnvironment
            | Self::JsonParse
            | Self::JsonGenerate
            | Self::NetworkGet
            | Self::CryptoSha256
            | Self::CryptoBase64Encode => 1,
            Self::WriteText | Self::RegexMatch => 2,
            Self::RegexReplace => 3,
        }
    }
}

impl fmt::Display for BuiltinFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// 基本块标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Label(pub usize);

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{}", self.0)
    }
}

/// 临时值编号。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TempId(pub usize);

/// 可写入的位置：局部变量或临时值。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// 具名局部变量。
    Local(String),
    /// 编译器生成的临时值。
    Temp(TempId),
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(name) => f.write_str(name),
            Self::Temp(id) => write!(f, "t{}", id.0),
        }
    }
}

/// 指令操作数：位置或常量。
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    /// 来自局部变量或临时值。
    Place(Place),
    /// 来自字面量常量。
    Constant(Constant),
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Place(place) => write!(f, "{place}"),
            Self::Constant(constant) => write!(f, "{constant}"),
        }
    }
}

/// 一元运算符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    /// 算术取负。
    Negate,
    /// 逻辑非。
    Not,
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Negate => f.write_str("负"),
            Self::Not => f.write_str("非"),
        }
    }
}

/// 二元运算符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    /// 加法。
    Add,
    /// 减法。
    Subtract,
    /// 乘法。
    Multiply,
    /// 除法。
    Divide,
    /// 相等比较。
    Equal,
    /// 小于比较。
    Less,
    /// 大于比较。
    Greater,
    /// 逻辑与。
    And,
    /// 逻辑或。
    Or,
}

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Equal => "==",
            Self::Less => "<",
            Self::Greater => ">",
            Self::And => "且",
            Self::Or => "或",
        };
        f.write_str(text)
    }
}

/// 一条三地址码指令。
#[derive(Clone, Debug, PartialEq)]
pub enum Instruction {
    /// 把常量写入目标位置。
    Const {
        /// 目标位置。
        dest: Place,
        /// 常量值。
        value: Constant,
        /// 目标类型。
        ty: IrType,
    },
    /// 把操作数复制到目标位置。
    Copy {
        /// 目标位置。
        dest: Place,
        /// 来源操作数。
        src: Operand,
        /// 目标类型。
        ty: IrType,
    },
    /// 一元运算。
    Unary {
        /// 目标位置。
        dest: Place,
        /// 运算符。
        op: UnaryOp,
        /// 操作数。
        operand: Operand,
        /// 结果类型。
        ty: IrType,
    },
    /// 二元运算。
    Binary {
        /// 目标位置。
        dest: Place,
        /// 运算符。
        op: BinaryOp,
        /// 左操作数。
        left: Operand,
        /// 右操作数。
        right: Operand,
        /// 结果类型。
        ty: IrType,
    },
    /// 函数调用。
    Call {
        /// 可选的返回值目标位置。
        dest: Option<Place>,
        /// 被调用的函数名。
        function: String,
        /// 实参列表。
        args: Vec<Operand>,
        /// 返回类型。
        ty: IrType,
    },
    /// 调用字符串或列表的内置方法。
    BuiltinCall {
        /// 目标位置。
        dest: Place,
        /// 方法接收者。
        receiver: Operand,
        /// 内置方法标识。
        method: BuiltinMethod,
        /// 实参列表，不包含接收者。
        args: Vec<Operand>,
        /// 返回类型。
        ty: IrType,
    },
    /// 调用语言级内置函数。
    BuiltinFunctionCall {
        /// 目标位置。
        dest: Place,
        /// 内置函数标识。
        function: BuiltinFunction,
        /// 实参列表。
        args: Vec<Operand>,
        /// 返回类型。
        ty: IrType,
    },
    /// 读取标准输入。
    Input {
        /// 目标位置。
        dest: Place,
        /// 结果类型。
        ty: IrType,
    },
    /// 构造列表。
    MakeList {
        /// 目标位置。
        dest: Place,
        /// 元素列表。
        elements: Vec<Operand>,
        /// 列表类型。
        ty: IrType,
    },
    /// 读取结构体字段。
    GetField {
        /// 目标位置。
        dest: Place,
        /// 结构体对象。
        object: Operand,
        /// 字段名称。
        field: String,
        /// 字段类型。
        ty: IrType,
    },
    /// 写入结构体字段。
    SetField {
        /// 结构体对象。
        object: Operand,
        /// 字段名称。
        field: String,
        /// 新值。
        value: Operand,
    },
    /// 读取列表下标。
    GetIndex {
        /// 目标位置。
        dest: Place,
        /// 列表对象。
        object: Operand,
        /// 下标。
        index: Operand,
        /// 元素类型。
        ty: IrType,
    },
    /// 写入列表下标。
    SetIndex {
        /// 列表对象。
        object: Operand,
        /// 下标。
        index: Operand,
        /// 新值。
        value: Operand,
    },
    /// 把值包装成成功可选值；没有值时表示空。
    WrapOptional {
        /// 目标位置。
        dest: Place,
        /// 成功值；`None` 表示空值。
        value: Option<Operand>,
        /// 可选类型。
        ty: IrType,
    },
    /// 把值包装成成功结果。
    WrapResult {
        /// 目标位置。
        dest: Place,
        /// 成功值。
        value: Operand,
        /// 结果类型。
        ty: IrType,
    },
    /// 判断可选值或结果值是否成功。
    IsSuccess {
        /// 目标位置。
        dest: Place,
        /// 待判断的值。
        value: Operand,
        /// 结果类型，固定为布尔。
        ty: IrType,
    },
    /// 取出可选值或结果值的成功内容。
    Unwrap {
        /// 目标位置。
        dest: Place,
        /// 待解包的值。
        value: Operand,
        /// 成功内容类型。
        ty: IrType,
    },
    /// 取出结果值的失败内容。
    UnwrapError {
        /// 目标位置。
        dest: Place,
        /// 待解包的值。
        value: Operand,
        /// 失败内容类型。
        ty: IrType,
    },
    /// 打印一个值。
    Print {
        /// 待打印的值。
        value: Operand,
    },
    /// 空指令，便于后续优化占位。
    Nop,
}

impl Instruction {
    /// 遍历指令中出现的全部位置，包括目标位置与操作数。
    pub fn for_each_place(&self, visitor: &mut impl FnMut(&Place)) {
        match self {
            Self::Const { dest, .. } | Self::Input { dest, .. } => visitor(dest),
            Self::Copy { dest, src, .. } => {
                visitor(dest);
                if let Operand::Place(place) = src {
                    visitor(place);
                }
            }
            Self::Unary { dest, operand, .. } => {
                visitor(dest);
                if let Operand::Place(place) = operand {
                    visitor(place);
                }
            }
            Self::Binary {
                dest, left, right, ..
            } => {
                visitor(dest);
                if let Operand::Place(place) = left {
                    visitor(place);
                }
                if let Operand::Place(place) = right {
                    visitor(place);
                }
            }
            Self::Call { dest, args, .. } => {
                if let Some(dest) = dest {
                    visitor(dest);
                }
                for argument in args {
                    if let Operand::Place(place) = argument {
                        visitor(place);
                    }
                }
            }
            Self::BuiltinCall {
                dest,
                receiver,
                args,
                ..
            } => {
                visitor(dest);
                if let Operand::Place(place) = receiver {
                    visitor(place);
                }
                for argument in args {
                    if let Operand::Place(place) = argument {
                        visitor(place);
                    }
                }
            }
            Self::BuiltinFunctionCall { dest, args, .. } => {
                visitor(dest);
                for argument in args {
                    if let Operand::Place(place) = argument {
                        visitor(place);
                    }
                }
            }
            Self::MakeList { dest, elements, .. } => {
                visitor(dest);
                for element in elements {
                    if let Operand::Place(place) = element {
                        visitor(place);
                    }
                }
            }
            Self::GetField { dest, object, .. } => {
                visitor(dest);
                if let Operand::Place(place) = object {
                    visitor(place);
                }
            }
            Self::SetField { object, value, .. } => {
                if let Operand::Place(place) = object {
                    visitor(place);
                }
                if let Operand::Place(place) = value {
                    visitor(place);
                }
            }
            Self::GetIndex {
                dest,
                object,
                index,
                ..
            } => {
                visitor(dest);
                if let Operand::Place(place) = object {
                    visitor(place);
                }
                if let Operand::Place(place) = index {
                    visitor(place);
                }
            }
            Self::SetIndex {
                object,
                index,
                value,
            } => {
                if let Operand::Place(place) = object {
                    visitor(place);
                }
                if let Operand::Place(place) = index {
                    visitor(place);
                }
                if let Operand::Place(place) = value {
                    visitor(place);
                }
            }
            Self::WrapOptional { dest, value, .. } => {
                visitor(dest);
                if let Some(Operand::Place(place)) = value {
                    visitor(place);
                }
            }
            Self::WrapResult { dest, value, .. }
            | Self::IsSuccess { dest, value, .. }
            | Self::Unwrap { dest, value, .. }
            | Self::UnwrapError { dest, value, .. } => {
                visitor(dest);
                if let Operand::Place(place) = value {
                    visitor(place);
                }
            }
            Self::Print { value } => {
                if let Operand::Place(place) = value {
                    visitor(place);
                }
            }
            Self::Nop => {}
        }
    }
}

/// 基本块末尾的控制流终止指令。
#[derive(Clone, Debug, PartialEq)]
pub enum Terminator {
    /// 无条件跳转。
    Jump(Label),
    /// 条件分支。
    Branch {
        /// 条件操作数，必须是布尔值。
        condition: Operand,
        /// 条件为真时的目标。
        then_label: Label,
        /// 条件为假时的目标。
        else_label: Label,
    },
    /// 函数返回。
    Return(Option<Operand>),
}

impl Terminator {
    /// 返回该终止指令的所有后继块。
    pub fn successors(&self) -> Vec<Label> {
        match self {
            Self::Jump(target) => vec![*target],
            Self::Branch {
                then_label,
                else_label,
                ..
            } => vec![*then_label, *else_label],
            Self::Return(_) => Vec::new(),
        }
    }
}

/// 基本块。
#[derive(Clone, Debug, PartialEq)]
pub struct BasicBlock {
    /// 块标签，用于跳转与分支。
    pub label: Label,
    /// 三地址码指令序列。
    pub instructions: Vec<Instruction>,
    /// 与 `instructions` 一一对应的源码区间。
    pub instruction_spans: Vec<Span>,
    /// 块末尾的控制流终止指令。
    pub terminator: Terminator,
    /// 终止指令对应的源码区间。
    pub terminator_span: Span,
}

/// 局部变量声明。
#[derive(Clone, Debug, PartialEq)]
pub struct Local {
    /// IR 内部唯一的变量名。
    pub name: String,
    /// 静态类型。
    pub ty: IrType,
    /// 是否允许重新赋值。
    pub mutable: bool,
}

/// 函数参数。
#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    /// 参数名称。
    pub name: String,
    /// 参数类型。
    pub ty: IrType,
}

/// 一个 IR 函数。
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    /// 函数名称。
    pub name: String,
    /// 参数列表。
    pub params: Vec<Parameter>,
    /// 返回类型。
    pub return_type: IrType,
    /// 局部变量声明。
    pub locals: Vec<Local>,
    /// 控制流图的基本块集合。
    pub blocks: Vec<BasicBlock>,
    /// 入口块标签。
    pub entry: Label,
}

impl Function {
    /// 按标签查找基本块。
    pub fn block(&self, label: Label) -> Option<&BasicBlock> {
        self.blocks.iter().find(|block| block.label == label)
    }

    /// 返回临时值数量，供后端分配临时变量。
    pub fn temp_count(&self) -> usize {
        let mut count = 0;

        for block in &self.blocks {
            for instruction in &block.instructions {
                instruction.for_each_place(&mut |place| {
                    if let Place::Temp(id) = place {
                        count = count.max(id.0 + 1);
                    }
                });
            }

            if let Terminator::Branch {
                condition: Operand::Place(Place::Temp(id)),
                ..
            } = &block.terminator
            {
                count = count.max(id.0 + 1);
            }
        }

        count
    }
}

/// 结构体字段。
#[derive(Clone, Debug, PartialEq)]
pub struct StructField {
    /// 字段名称。
    pub name: String,
    /// 字段类型。
    pub ty: IrType,
}

/// 结构体声明。
#[derive(Clone, Debug, PartialEq)]
pub struct StructDecl {
    /// 结构体名称。
    pub name: String,
    /// 字段列表。
    pub fields: Vec<StructField>,
}

/// 一份完整的中立 IR 程序。
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    /// 结构体声明。
    pub structs: Vec<StructDecl>,
    /// 函数定义。
    pub functions: Vec<Function>,
    /// 是否包含 `不安全` 块。
    ///
    /// 该标记让不支持底层内存操作的后端在代码生成前直接拒绝程序。
    pub uses_unsafe: bool,
}

impl Program {
    /// 按名称查找函数。
    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|function| function.name == name)
    }

    /// 生成便于调试和测试的文本形式。
    ///
    /// 文本中显式包含 `LABEL`、`JUMP`、`BRANCH` 与 `RETURN`，
    /// 便于检查 AST 是否已经正确降级为 CFG。
    pub fn to_text(&self) -> String {
        let mut output = String::new();

        for declaration in &self.structs {
            output.push_str(&format!("结构体 {} {{\n", declaration.name));
            for field in &declaration.fields {
                output.push_str(&format!("    {}: {},\n", field.name, field.ty));
            }
            output.push_str("}\n\n");
        }

        for function in &self.functions {
            let params = function
                .params
                .iter()
                .map(|parameter| format!("{}: {}", parameter.name, parameter.ty))
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str(&format!(
                "函数 {}({}) -> {} {{\n",
                function.name, params, function.return_type
            ));

            for local in &function.locals {
                let mutable = if local.mutable { "可变 " } else { "" };
                output.push_str(&format!(
                    "    局部 {}{}: {};\n",
                    mutable, local.name, local.ty
                ));
            }

            for block in &function.blocks {
                output.push_str(&format!("    LABEL {}\n", block.label));
                for instruction in &block.instructions {
                    output.push_str(&format!("    {}\n", format_instruction(instruction)));
                }
                output.push_str(&format!("    {}\n", format_terminator(&block.terminator)));
            }

            output.push_str("}\n\n");
        }

        output
    }
}

fn format_instruction(instruction: &Instruction) -> String {
    match instruction {
        Instruction::Const { dest, value, ty } => {
            format!("{dest} = {value} : {ty}")
        }
        Instruction::Copy { dest, src, ty } => {
            format!("{dest} = {src} : {ty}")
        }
        Instruction::Unary {
            dest,
            op,
            operand,
            ty,
        } => format!("{dest} = {op} {operand} : {ty}"),
        Instruction::Binary {
            dest,
            op,
            left,
            right,
            ty,
        } => format!("{dest} = {left} {op} {right} : {ty}"),
        Instruction::Call {
            dest,
            function,
            args,
            ty,
        } => {
            let arguments = args
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let target = dest
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "_".to_owned());
            format!("{target} = 调用 {function}({arguments}) : {ty}")
        }
        Instruction::BuiltinCall {
            dest,
            receiver,
            method,
            args,
            ty,
        } => {
            let arguments = std::iter::once(receiver)
                .chain(args.iter())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{dest} = 内置调用 {method}({arguments}) : {ty}")
        }
        Instruction::BuiltinFunctionCall {
            dest,
            function,
            args,
            ty,
        } => {
            let arguments = args
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{dest} = 内置函数 {function}({arguments}) : {ty}")
        }
        Instruction::Input { dest, ty } => format!("{dest} = 输入 : {ty}"),
        Instruction::MakeList { dest, elements, ty } => {
            let items = elements
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{dest} = [{items}] : {ty}")
        }
        Instruction::GetField {
            dest,
            object,
            field,
            ty,
        } => format!("{dest} = {object}.{field} : {ty}"),
        Instruction::SetField {
            object,
            field,
            value,
        } => format!("{object}.{field} = {value}"),
        Instruction::GetIndex {
            dest,
            object,
            index,
            ty,
        } => format!("{dest} = {object}[{index}] : {ty}"),
        Instruction::SetIndex {
            object,
            index,
            value,
        } => format!("{object}[{index}] = {value}"),
        Instruction::WrapOptional { dest, value, ty } => match value {
            Some(value) => format!("{dest} = 成功({value}) : {ty}"),
            None => format!("{dest} = 空 : {ty}"),
        },
        Instruction::WrapResult { dest, value, ty } => {
            format!("{dest} = 成功({value}) : {ty}")
        }
        Instruction::IsSuccess { dest, value, ty } => {
            format!("{dest} = 是否成功({value}) : {ty}")
        }
        Instruction::Unwrap { dest, value, ty } => {
            format!("{dest} = 解包({value}) : {ty}")
        }
        Instruction::UnwrapError { dest, value, ty } => {
            format!("{dest} = 解包错误({value}) : {ty}")
        }
        Instruction::Print { value } => format!("打印 {value}"),
        Instruction::Nop => "空指令".to_owned(),
    }
}

fn format_terminator(terminator: &Terminator) -> String {
    match terminator {
        Terminator::Jump(target) => format!("JUMP {target}"),
        Terminator::Branch {
            condition,
            then_label,
            else_label,
        } => format!("BRANCH {condition}, {then_label}, {else_label}"),
        Terminator::Return(Some(value)) => format!("RETURN {value}"),
        Terminator::Return(None) => "RETURN".to_owned(),
    }
}
