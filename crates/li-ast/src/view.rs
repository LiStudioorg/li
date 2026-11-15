//! `.lui` 视图文件节点。
//!
//! 视图语法刻意保持声明式与扁平化：
//!
//! ```text
//! 视图 {
//!     标题: 「计数器」,
//!     状态 {
//!         让 可变 次数 = 0;
//!     },
//!     容器 {
//!         文本 { 「当前计数：」, 次数 }
//!         按钮 {
//!             点击 { 次数 = 次数 加 1; }
//!             文本 { 「点我加一」 }
//!         }
//!     }
//! }
//! ```
//!
//! - 视图根节点可写文档属性，例如 `标题`、`语言` 与 `描述`；
//! - 普通节点写作 `标签名 { 属性: 值, 子节点 }`；
//! - `文本 { 「内容」 }` 表示纯文本叶子节点；
//! - `状态 { ... }` 只在视图根节点出现，声明交互状态；
//! - 事件写作 `点击 { 语句 }`，事件体是普通 li 语句块；
//! - 文本与属性值可以嵌入表达式，例如 `文本 { 次数 }` 与 `隐藏: { 次数 大于 5 }`；
//! - 字符串仍严格遵守 li 的 `「」` 定界符规范。

use li_errors::Span;

use crate::common::Ident;
use crate::expr::Expr;
use crate::stmt::Stmt;

/// 一个 `.lui` 文件中的视图声明。
#[derive(Clone, Debug, PartialEq)]
pub struct ViewDecl {
    /// 视图根节点的属性，例如文档标题与语言。
    pub attributes: Vec<ViewAttribute>,
    /// 视图状态声明，按源码顺序保存的 `让` 语句。
    pub state: Vec<Stmt>,
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
    /// 该节点绑定的事件处理器。
    pub events: Vec<ViewEvent>,
    /// 节点内容：空、纯文本或子节点列表。
    pub content: ViewContent,
    /// 整个节点在源码中的区间。
    pub span: Span,
}

/// 节点上绑定的一个事件处理器。
#[derive(Clone, Debug, PartialEq)]
pub struct ViewEvent {
    /// 中文事件名，例如 `点击`、`输入` 或 `提交`。
    pub name: Ident,
    /// 事件处理语句块。
    pub body: crate::stmt::Block,
    /// 整个事件绑定在源码中的区间。
    pub span: Span,
}

/// 视图节点的内容。
#[derive(Clone, Debug, PartialEq)]
pub enum ViewContent {
    /// 空节点，例如 `换行 { }`。
    Empty,
    /// 纯文本内容，对应 `文本 { 「你好」 }`。
    Text(String),
    /// 由静态文本与表达式混合组成的内容，例如 `文本 { 「计数：」, 次数 }`。
    Parts(Vec<ViewTextPart>),
    /// 子节点列表。
    Children(Vec<ViewNode>),
}

/// 文本内容中的一段。
#[derive(Clone, Debug, PartialEq)]
pub struct ViewTextPart {
    /// 片段的具体形式。
    pub kind: ViewTextPartKind,
    /// 片段在源码中的区间。
    pub span: Span,
}

/// 文本片段的具体形式。
#[derive(Clone, Debug, PartialEq)]
pub enum ViewTextPartKind {
    /// 静态文本。
    Text(String),
    /// 需要运行时求值的表达式。
    Expression(Expr),
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

/// 视图属性值支持的形式。
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
    /// 需要运行时求值的绑定表达式，写作 `{ 表达式 }`。
    Expression(Expr),
}
