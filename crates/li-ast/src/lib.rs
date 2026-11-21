//! li 语言的抽象语法树（AST）。
//!
//! 该 crate 只描述语法结构，不依赖词法分析器或语义分析的实现细节。
//! 所有节点都携带 [`li_errors::Span`]，便于后续阶段报告精确的行列位置。
//!
//! 语法树按以下层次组织：
//! - [`Program`] 与 [`Item`]：顶层程序项，包括模块、导入、函数、类型声明与视图声明；
//! - [`Stmt`] 与 [`Block`]：函数体中的语句与语句块；
//! - [`Expr`]：表达式；
//! - [`Type`]：类型标注；
//! - [`Pattern`]：匹配模式。

mod common;
mod expr;
mod item;
mod pattern;
mod stmt;
mod ty;
mod view;

pub use common::{Ident, Literal};
pub use expr::{BinaryOp, EnumVariantExpr, Expr, ExprKind, UnaryOp};
pub use item::{
    EnumDecl, EnumVariant, FieldDef, FuncDecl, ImportDecl, Item, ModuleDecl, ModulePath, Param,
    Program, TypeDecl,
};
pub use pattern::{Pattern, PatternKind};
pub use stmt::{
    AssignStmt, Block, BreakStmt, ContinueStmt, ExprStmt, IfStmt, InlineAsmStmt, LetStmt, MatchArm,
    MatchStmt, PrintStmt, ReturnStmt, Stmt, UnsafeStmt, WhileStmt,
};
pub use ty::{Type, TypeKind};
pub use view::{
    ViewAttribute, ViewContent, ViewDecl, ViewEvent, ViewNode, ViewTextPart, ViewTextPartKind,
    ViewValue, ViewValueKind,
};
