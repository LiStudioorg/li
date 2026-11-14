//! 类型标注节点。

use li_errors::Span;

use crate::common::Ident;

/// 一个类型标注，例如 `整数`、`列表<字符串>` 或 `结果<整数, 字符串>`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Type {
    /// 类型的具体形式。
    pub kind: TypeKind,
    /// 整个类型标注在源码中的区间。
    pub span: Span,
}

impl Type {
    /// 创建一个类型节点。
    pub const fn new(kind: TypeKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// 类型的具体形式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeKind {
    /// 整数类型（i64）。
    Integer,
    /// 小数类型（f64）。
    Float,
    /// 布尔类型。
    Bool,
    /// 字符串类型。
    String,
    /// 列表类型，例如 `列表<整数>`。
    List(Box<Type>),
    /// 字典类型，例如 `字典<字符串, 整数>`。
    Dictionary {
        /// 键类型。
        key: Box<Type>,
        /// 值类型。
        value: Box<Type>,
    },
    /// 原生指针类型，例如 `原生指针<整数>`。
    RawPointer(Box<Type>),
    /// 可选类型，例如 `可选<字符串>`。
    Optional(Box<Type>),
    /// 结果类型，例如 `结果<整数, 字符串>`。
    Result {
        /// 成功时的值类型。
        value: Box<Type>,
        /// 失败时的错误类型。
        error: Box<Type>,
    },
    /// 用户声明的具名类型，例如结构体名称。
    Named(Ident),
}
