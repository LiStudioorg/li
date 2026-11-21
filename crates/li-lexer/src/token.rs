//! Token 定义。
//!
//! 全角标点会在词法分析阶段被归一化为与半角标点相同的 Token，
//! 因此语法分析器不需要区分 `（` 与 `(`、`；` 与 `;` 等写法。

use std::fmt;

use li_errors::Span;

/// Token 的种类。
///
/// 带数据的变体保存词法分析后的值：标识符保存名称，
/// 字符串保存解码后的内容，整数与小数保存解析后的数值。
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    // ---- 标识符与字面量 ----
    /// 标识符，例如 `温度`、`count` 或 `_内部变量2`。
    Identifier(String),
    /// 整数字面量。
    Integer(i64),
    /// 小数字面量。
    Float(f64),
    /// 字符串字面量，保存解码后的内容。
    String(String),

    // ---- 关键字 ----
    /// `让` / `let`。
    Let,
    /// `可变` / `mut`。
    Mut,
    /// `函数` / `fn` / `func` / `function`。
    Func,
    /// `返回` / `return`。
    Return,
    /// `如果` / `if`。
    If,
    /// `则` / `then`。
    Then,
    /// `否则` / `else`。
    Else,
    /// `当` / `while`。
    While,
    /// `跳出` / `break`。
    Break,
    /// `继续` / `continue`。
    Continue,
    /// `匹配` / `match`。
    Match,
    /// `枚举项` / `variant`，用于匹配模式中表示"这是某个枚举的某项"。
    Variant,
    /// `成功` / `ok`。
    Success,
    /// `失败` / `err`。
    Failure,
    /// `模块` / `module`。
    Module,
    /// `导入` / `import`。
    Import,
    /// `真` / `true`。
    True,
    /// `假` / `false`。
    False,
    /// `空` / `null`。
    Null,
    /// `类型` / `type`。
    Type,
    /// `打印` / `print`。
    Print,
    /// `输入` / `input`。
    Input,
    /// `视图` / `view`，用于 `.lui` 视图文件。
    View,
    /// `不安全` / `unsafe`，用于底层内存操作块。
    Unsafe,
    /// `异步` / `async`，异步函数修饰符（当前版本仅预留）。
    Async,
    /// `等待` / `await`，等待异步结果（当前版本仅预留）。
    Await,

    // ---- 基础类型关键字 ----
    /// `整数` / `int`。
    IntegerType,
    /// `小数` / `float`。
    FloatType,
    /// `布尔` / `bool`。
    BoolType,
    /// `字符串` / `string`。
    StringType,
    /// `列表` / `list`。
    ListType,
    /// `字典` / `dictionary` / `map`。
    DictionaryType,
    /// `可选` / `option` / `optional`。
    OptionalType,
    /// `结果` / `result`。
    ResultType,
    /// `枚举` / `enum`，用户自定义枚举类型。
    EnumType,
    /// `原生指针` / `rawptr` / `ptr`。
    RawPointerType,
    /// `任务` / `task`，异步函数调用返回的句柄类型。
    TaskType,
    /// `@`，检查注解的前缀符号，例如 `@检查(不可变性: 报错)`。
    At,
    /// `#无垃圾回收` 文件头指令，关闭垃圾回收、面向 no_std 目标。
    NoGc,
    /// `外部` / `extern`，C 语言 ABI 函数声明关键字。
    Extern,
    /// `内联汇编` / `asm`，内联汇编语句（当前版本仅预留，语义阶段报未实现）。
    InlineAsm,
    /// `通道` / `channel`，并发通道类型（当前版本仅预留）。
    ChannelType,
    /// `共享` / `shared`，共享所有权类型（当前版本仅预留）。
    SharedType,
    /// `借用` / `borrow`，借用类型（当前版本仅预留）。
    BorrowType,

    // ---- 运算符 ----
    /// `加` 或 `+`。
    Plus,
    /// `减` 或 `-`。
    Minus,
    /// `乘` 或 `*`。
    Star,
    /// `除` 或 `/`。
    Slash,
    /// `等于` 或 `==`。
    EqualEqual,
    /// `小于` 或 `<`。
    Less,
    /// `大于` 或 `>`。
    Greater,
    /// `且` 或 `&&`。
    AndAnd,
    /// `或` 或 `||`。
    OrOr,
    /// `非` 或 `!`。
    Not,
    /// 赋值 `=`。
    Assign,
    /// 箭头 `->`。
    Arrow,
    /// 粗箭头 `=>`。
    FatArrow,

    // ---- 分隔符 ----
    /// 左圆括号。
    LParen,
    /// 右圆括号。
    RParen,
    /// 左花括号。
    LBrace,
    /// 右花括号。
    RBrace,
    /// 左方括号。
    LBracket,
    /// 右方括号。
    RBracket,
    /// 逗号。
    Comma,
    /// 分号。
    Semicolon,
    /// 冒号。
    Colon,
    /// 双冒号，用于模块路径。
    ColonColon,
    /// 点号，用于字段访问。
    Dot,

    /// 文件结束。
    Eof,
}

impl TokenKind {
    /// 是否为文件结束标记。
    pub fn is_eof(&self) -> bool {
        matches!(self, Self::Eof)
    }

    /// 返回关键字或中文运算符的规范中文写法。
    ///
    /// 视图文件允许把关键字用作标签名或属性名，例如 `列表 { ... }`
    /// 与 `类型: 「button」`。词法阶段只保存 Token 种类，因此这里提供
    /// 一份从种类回到规范中文文本的映射，供视图解析器还原名称。
    /// 标点、字面量与文件结束标记返回 [`None`]。
    pub fn keyword_name(&self) -> Option<&'static str> {
        let name = match self {
            Self::Let => "让",
            Self::Mut => "可变",
            Self::Func => "函数",
            Self::Return => "返回",
            Self::If => "如果",
            Self::Then => "则",
            Self::Else => "否则",
            Self::While => "当",
            Self::Break => "跳出",
            Self::Continue => "继续",
            Self::Match => "匹配",
            Self::Variant => "枚举项",
            Self::Success => "成功",
            Self::Failure => "失败",
            Self::Module => "模块",
            Self::Import => "导入",
            Self::True => "真",
            Self::False => "假",
            Self::Null => "空",
            Self::Type => "类型",
            Self::Print => "打印",
            Self::Input => "输入",
            Self::View => "视图",
            Self::Unsafe => "不安全",
            Self::Async => "异步",
            Self::Await => "等待",
            Self::IntegerType => "整数",
            Self::FloatType => "小数",
            Self::BoolType => "布尔",
            Self::StringType => "字符串",
            Self::ListType => "列表",
            Self::DictionaryType => "字典",
            Self::OptionalType => "可选",
            Self::ResultType => "结果",
            Self::EnumType => "枚举",
            Self::RawPointerType => "原生指针",
            Self::TaskType => "任务",
            Self::At => "@",
            Self::NoGc => "#无垃圾回收",
            Self::Extern => "外部",
            Self::InlineAsm => "内联汇编",
            Self::ChannelType => "通道",
            Self::SharedType => "共享",
            Self::BorrowType => "借用",
            Self::Plus => "加",
            Self::Minus => "减",
            Self::Star => "乘",
            Self::Slash => "除",
            Self::EqualEqual => "等于",
            Self::Less => "小于",
            Self::Greater => "大于",
            Self::AndAnd => "且",
            Self::OrOr => "或",
            Self::Not => "非",
            _ => return None,
        };

        Some(name)
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identifier(name) => write!(f, "标识符 `{}`", name),
            Self::Integer(value) => write!(f, "整数 `{}`", value),
            Self::Float(value) => write!(f, "小数 `{}`", value),
            Self::String(value) => write!(f, "字符串「{}」", value),

            Self::Let => f.write_str("`让`"),
            Self::Mut => f.write_str("`可变`"),
            Self::Func => f.write_str("`函数`"),
            Self::Return => f.write_str("`返回`"),
            Self::If => f.write_str("`如果`"),
            Self::Then => f.write_str("`则`"),
            Self::Else => f.write_str("`否则`"),
            Self::While => f.write_str("`当`"),
            Self::Break => f.write_str("`跳出`"),
            Self::Continue => f.write_str("`继续`"),
            Self::Match => f.write_str("`匹配`"),
            Self::Variant => f.write_str("`枚举项`"),
            Self::Success => f.write_str("`成功`"),
            Self::Failure => f.write_str("`失败`"),
            Self::Module => f.write_str("`模块`"),
            Self::Import => f.write_str("`导入`"),
            Self::True => f.write_str("`真`"),
            Self::False => f.write_str("`假`"),
            Self::Null => f.write_str("`空`"),
            Self::Type => f.write_str("`类型`"),
            Self::Print => f.write_str("`打印`"),
            Self::Input => f.write_str("`输入`"),
            Self::View => f.write_str("`视图`"),
            Self::Unsafe => f.write_str("`不安全`"),
            Self::Async => f.write_str("`异步`"),
            Self::Await => f.write_str("`等待`"),

            Self::IntegerType => f.write_str("`整数`"),
            Self::FloatType => f.write_str("`小数`"),
            Self::BoolType => f.write_str("`布尔`"),
            Self::StringType => f.write_str("`字符串`"),
            Self::ListType => f.write_str("`列表`"),
            Self::DictionaryType => f.write_str("`字典`"),
            Self::OptionalType => f.write_str("`可选`"),
            Self::ResultType => f.write_str("`结果`"),
            Self::EnumType => f.write_str("`枚举`"),
            Self::RawPointerType => f.write_str("`原生指针`"),
            Self::TaskType => f.write_str("`任务`"),
            Self::At => f.write_str("`@`"),
            Self::NoGc => f.write_str("`#无垃圾回收`"),
            Self::Extern => f.write_str("`外部`"),
            Self::InlineAsm => f.write_str("`内联汇编`"),
            Self::ChannelType => f.write_str("`通道`"),
            Self::SharedType => f.write_str("`共享`"),
            Self::BorrowType => f.write_str("`借用`"),

            Self::Plus => f.write_str("`+`"),
            Self::Minus => f.write_str("`-`"),
            Self::Star => f.write_str("`*`"),
            Self::Slash => f.write_str("`/`"),
            Self::EqualEqual => f.write_str("`==`"),
            Self::Less => f.write_str("`<`"),
            Self::Greater => f.write_str("`>`"),
            Self::AndAnd => f.write_str("`&&`"),
            Self::OrOr => f.write_str("`||`"),
            Self::Not => f.write_str("`!`"),
            Self::Assign => f.write_str("`=`"),
            Self::Arrow => f.write_str("`->`"),
            Self::FatArrow => f.write_str("`=>`"),

            Self::LParen => f.write_str("`(`"),
            Self::RParen => f.write_str("`)`"),
            Self::LBrace => f.write_str("`{`"),
            Self::RBrace => f.write_str("`}`"),
            Self::LBracket => f.write_str("`[`"),
            Self::RBracket => f.write_str("`]`"),
            Self::Comma => f.write_str("`,`"),
            Self::Semicolon => f.write_str("`;`"),
            Self::Colon => f.write_str("`:`"),
            Self::ColonColon => f.write_str("`::`"),
            Self::Dot => f.write_str("`.`"),

            Self::Eof => f.write_str("文件结尾"),
        }
    }
}

/// 一个带源码位置的 Token。
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// Token 的种类与携带的数据。
    pub kind: TokenKind,
    /// Token 在源码中的区间。
    pub span: Span,
}

impl Token {
    /// 创建一个 Token。
    pub const fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// 是否为文件结束标记。
    pub fn is_eof(&self) -> bool {
        self.kind.is_eof()
    }
}
