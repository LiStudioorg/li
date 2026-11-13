//! AST 的公共基础节点。

use li_errors::Span;

/// 标识符，同时保留它在源码中的位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    /// 标识符名称，不含引号。
    pub name: String,
    /// 标识符在源码中的区间。
    pub span: Span,
}

impl Ident {
    /// 创建一个标识符节点。
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self {
            name: name.into(),
            span,
        }
    }

    /// 返回标识符名称。
    pub fn as_str(&self) -> &str {
        &self.name
    }
}

/// 字面量。
///
/// 字符串字面量保存解码后的内容，定界符 `「」` 不属于内容的一部分。
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    /// 整数字面量，对应 `整数` 类型。
    Integer(i64),
    /// 小数字面量，对应 `小数` 类型。
    Float(f64),
    /// 布尔字面量。
    Bool(bool),
    /// 字符串字面量。
    String(String),
    /// 空值字面量。
    Null,
}
