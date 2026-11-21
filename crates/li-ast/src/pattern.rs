//! 模式匹配节点。

use li_errors::Span;

use crate::common::{Ident, Literal};

/// 一个匹配模式及其源码区间。
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    /// 模式的具体形式。
    pub kind: PatternKind,
    /// 整个模式在源码中的区间。
    pub span: Span,
}

/// 模式的具体形式。
#[derive(Clone, Debug, PartialEq)]
pub enum PatternKind {
    /// 通配模式 `_`，匹配任意值但不绑定名称。
    Wildcard,
    /// 名称绑定模式。
    Binding(Ident),
    /// 字面量模式。
    Literal(Literal),
    /// 结果成功模式，例如 `成功` 或 `成功(值)`。
    Success(Option<Ident>),
    /// 结果失败模式，例如 `失败` 或 `失败(错误)`。
    Failure(Option<Ident>),
    /// 枚举项模式，例如 `标识符(文本)` 或 `符号`。
    ///
    /// 携带 0 到 N 个绑定名称；匹配时要求枚举项的载荷类型与绑定类型一致。
    Enum {
        /// 枚举项名称。
        variant: Ident,
        /// 枚举项的载荷绑定，按声明顺序对应。
        bindings: Vec<Option<Ident>>,
    },
}
