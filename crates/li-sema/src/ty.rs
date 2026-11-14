//! 语义分析使用的内部类型表示。
//!
//! 与 AST 的 [`li_ast::Type`] 不同，这里的类型已经完成具名类型解析：
//! 结构体名称会变成 [`Ty::Struct`]，未知类型则降级为 [`Ty::Error`]，
//! 以便在后续检查中抑制连锁诊断。

use std::fmt;

/// 语义分析阶段的类型。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ty {
    /// 64 位有符号整数。
    Integer,
    /// 64 位浮点数。
    Float,
    /// 布尔值。
    Bool,
    /// 字符串。
    String,
    /// 空值，只允许出现在可选类型上下文中。
    Null,
    /// 列表。
    List(Box<Ty>),
    /// 字典。
    Dictionary {
        /// 键类型。
        key: Box<Ty>,
        /// 值类型。
        value: Box<Ty>,
    },
    /// 可选类型。
    Optional(Box<Ty>),
    /// 结果类型。
    Result {
        /// 成功值的类型。
        ok: Box<Ty>,
        /// 失败值的类型。
        error: Box<Ty>,
    },
    /// 原生指针类型。
    RawPointer(Box<Ty>),
    /// 程序内声明的结构体类型。
    Struct(String),
    /// 类型无法确定；使用该类型时不重复报告错误。
    Error,
}

impl Ty {
    /// 是否为数字类型。
    pub(crate) fn is_numeric(&self) -> bool {
        matches!(self, Self::Integer | Self::Float)
    }

    /// 是否表示已经报告过的错误类型。
    pub(crate) fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }

    /// 是否可以作为字典键。
    ///
    /// 第一版字典由宿主哈希表实现，因此键必须是可稳定哈希的标量类型。
    pub(crate) fn is_hashable(&self) -> bool {
        matches!(
            self,
            Self::Integer | Self::Float | Self::Bool | Self::String | Self::Error
        )
    }

    /// 是否为原生指针。
    pub(crate) fn is_raw_pointer(&self) -> bool {
        matches!(self, Self::RawPointer(_))
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer => f.write_str("整数"),
            Self::Float => f.write_str("小数"),
            Self::Bool => f.write_str("布尔"),
            Self::String => f.write_str("字符串"),
            Self::Null => f.write_str("空"),
            Self::List(element) => write!(f, "列表<{element}>"),
            Self::Dictionary { key, value } => write!(f, "字典<{key}, {value}>"),
            Self::Optional(inner) => write!(f, "可选<{inner}>"),
            Self::Result { ok, error } => write!(f, "结果<{ok}, {error}>"),
            Self::RawPointer(inner) => write!(f, "原生指针<{inner}>"),
            Self::Struct(name) => f.write_str(name),
            Self::Error => f.write_str("错误类型"),
        }
    }
}
