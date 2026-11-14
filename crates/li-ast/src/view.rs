//! `.lui` 视图文件节点。
//!
//! 视图语法刻意保持声明式与扁平化：
//!
//! ```text
//! 视图 {
//!     标题: 「你好，li」,
//!     容器 {
//!         文本 { 「你好」 }
//!         按钮 { 类: 「主按钮」, 文本 { 「点我」 } }
//!     }
//! }
//! ```
//!
//! - 视图根节点可写文档属性，例如 `标题`、`语言` 与 `描述`；
//! - 普通节点写作 `标签名 { 属性: 值, 子节点 }`；
//! - `文本 { 「内容」 }` 表示纯文本叶子节点；
//! - 字符串仍严格遵守 li 的 `「」` 定界符规范。

use li_errors::Span;

use crate::common::Ident;

/// 一个 `.lui` 文件中的视图声明。
#[derive(Clone, Debug, PartialEq)]
pub struct ViewDecl {
    /// 视图根节点的属性，例如文档标题与语言。
    pub attributes: Vec<ViewAttribute>,
    /// 视图根节点的内容。
    pub content: ViewContent,
    /// 整个视图声明在源码中的区间。
    pub span: Span,
}

/// 视图中的一个元素节点。
#[derive(Clone, Debug, PartialEq)]
pub struct ViewNode {
    /// 中文标签名，例如 `容器`、`按钮` 或 `列表项`。
    pub name: Ident,
    /// 写在子节点之前的属性列表。
    pub attributes: Vec<ViewAttribute>,
    /// 节点内容：空、纯文本或子节点列表。
    pub content: ViewContent,
    /// 整个节点在源码中的区间。
    pub span: Span,
}

/// 视图节点的内容。
#[derive(Clone, Debug, PartialEq)]
pub enum ViewContent {
    /// 空节点，例如 `换行 { }`。
    Empty,
    /// 纯文本内容，对应 `文本 { 「你好」 }` 或 `段落 { 「你好」 }`。
    Text(String),
    /// 子节点列表。
    Children(Vec<ViewNode>),
}

/// 视图节点上的一个属性。
#[derive(Clone, Debug, PartialEq)]
pub struct ViewAttribute {
    /// 属性名，例如 `类`、`标识` 或 `链接`。
    pub name: Ident,
    /// 属性值。
    pub value: ViewValue,
    /// 整个属性在源码中的区间。
    pub span: Span,
}

/// 一个带源码位置的视图属性值。
#[derive(Clone, Debug, PartialEq)]
pub struct ViewValue {
    /// 属性值的具体形式。
    pub kind: ViewValueKind,
    /// 属性值在源码中的区间。
    pub span: Span,
}

/// 视图属性值支持的简单字面量形式。
#[derive(Clone, Debug, PartialEq)]
pub enum ViewValueKind {
    /// 字符串值，例如 `「主按钮」`。
    Text(String),
    /// 整数值。
    Integer(i64),
    /// 小数值。
    Float(f64),
    /// 布尔值，对应 `真` 或 `假`。
    Bool(bool),
    /// 标识符值，例如 `按钮`。
    Identifier(String),
}
