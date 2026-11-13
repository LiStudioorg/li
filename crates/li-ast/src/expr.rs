//! 表达式节点。

use li_errors::Span;

use crate::common::{Ident, Literal};

/// 一个表达式及其源码区间。
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    /// 表达式的具体形式。
    pub kind: ExprKind,
    /// 整个表达式在源码中的区间。
    pub span: Span,
}

impl Expr {
    /// 创建一个表达式节点。
    pub const fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// 表达式的具体形式。
#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    /// 字面量，例如 `42`、`「你好」`、`真` 或 `空`。
    Literal(Literal),
    /// 变量或其他名称引用。
    Identifier(Ident),
    /// 输入表达式 `输入`。
    Input,
    /// 一元运算，例如 `非 条件` 或 `-甲`。
    Unary {
        /// 运算符。
        op: UnaryOp,
        /// 操作数。
        operand: Box<Expr>,
    },
    /// 二元运算，例如 `甲 加 乙` 或 `甲 < 乙`。
    Binary {
        /// 左操作数。
        left: Box<Expr>,
        /// 运算符。
        op: BinaryOp,
        /// 右操作数。
        right: Box<Expr>,
    },
    /// 函数调用，例如 `求和(1, 2)`。
    Call {
        /// 被调用的表达式。
        callee: Box<Expr>,
        /// 实参列表。
        args: Vec<Expr>,
    },
    /// 字段访问，例如 `用户.名字`。
    FieldAccess {
        /// 字段所属的对象。
        object: Box<Expr>,
        /// 字段名称。
        field: Ident,
    },
    /// 下标访问，例如 `列表[0]`。
    Index {
        /// 被索引的对象。
        object: Box<Expr>,
        /// 下标表达式。
        index: Box<Expr>,
    },
    /// 列表字面量，例如 `[1, 2, 3]`。
    ListLiteral(Vec<Expr>),
    /// 括号表达式，保留括号以便后续工具还原源码结构。
    Parenthesized(Box<Expr>),
}

/// 一元运算符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    /// 算术取负。
    Negate,
    /// 逻辑非，对应 `非` 或 `!`。
    Not,
}

/// 二元运算符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    /// 加法，对应 `加` 或 `+`。
    Add,
    /// 减法，对应 `减` 或 `-`。
    Subtract,
    /// 乘法，对应 `乘` 或 `*`。
    Multiply,
    /// 除法，对应 `除` 或 `/`。
    Divide,
    /// 相等比较，对应 `等于` 或 `==`。
    Equal,
    /// 小于比较，对应 `小于` 或 `<`。
    Less,
    /// 大于比较，对应 `大于` 或 `>`。
    Greater,
    /// 逻辑与，对应 `且` 或 `&&`。
    And,
    /// 逻辑或，对应 `或` 或 `||`。
    Or,
}
