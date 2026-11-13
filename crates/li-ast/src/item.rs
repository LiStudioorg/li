//! 顶层程序项节点。

use li_errors::Span;

use crate::common::Ident;
use crate::stmt::Block;
use crate::ty::Type;

/// 一份完整的 li 源文件。
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    /// 顶层程序项，按源码顺序保存。
    pub items: Vec<Item>,
    /// 整个源文件在源码中的区间。
    pub span: Span,
}

/// 顶层程序项。
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// 模块声明，例如 `模块 工具 { ... }`。
    Module(ModuleDecl),
    /// 导入声明，例如 `导入 工具::数学;`。
    Import(ImportDecl),
    /// 函数声明。
    Function(FuncDecl),
    /// 类型声明，对应结构体定义。
    Type(TypeDecl),
}

impl Item {
    /// 返回程序项对应的源码区间。
    pub fn span(&self) -> Span {
        match self {
            Self::Module(item) => item.span,
            Self::Import(item) => item.span,
            Self::Function(item) => item.span,
            Self::Type(item) => item.span,
        }
    }
}

/// 模块声明。
#[derive(Clone, Debug, PartialEq)]
pub struct ModuleDecl {
    /// 模块名称。
    pub name: Ident,
    /// 模块内部的顶层程序项。
    pub items: Vec<Item>,
    /// 整个模块声明在源码中的区间。
    pub span: Span,
}

/// 模块路径，例如 `工具::数学`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModulePath {
    /// 路径中的名称片段。
    pub segments: Vec<Ident>,
    /// 整个路径在源码中的区间。
    pub span: Span,
}

/// 导入声明。
#[derive(Clone, Debug, PartialEq)]
pub struct ImportDecl {
    /// 被导入的模块路径。
    pub path: ModulePath,
    /// 整条导入声明在源码中的区间。
    pub span: Span,
}

/// 函数声明。
///
/// li 要求函数显式标注每个参数的类型与返回类型。
#[derive(Clone, Debug, PartialEq)]
pub struct FuncDecl {
    /// 函数名称。
    pub name: Ident,
    /// 参数列表。
    pub params: Vec<Param>,
    /// 返回类型。
    pub return_type: Type,
    /// 函数体。
    pub body: Block,
    /// 整个函数声明在源码中的区间。
    pub span: Span,
}

/// 函数参数。
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    /// 参数名称。
    pub name: Ident,
    /// 参数类型。
    pub ty: Type,
    /// 整个参数在源码中的区间。
    pub span: Span,
}

/// 类型声明，对应 `类型 名字 { 字段: 类型, }` 形式的结构体定义。
#[derive(Clone, Debug, PartialEq)]
pub struct TypeDecl {
    /// 类型名称。
    pub name: Ident,
    /// 字段定义列表。
    pub fields: Vec<FieldDef>,
    /// 整个类型声明在源码中的区间。
    pub span: Span,
}

/// 结构体字段定义。
#[derive(Clone, Debug, PartialEq)]
pub struct FieldDef {
    /// 字段名称。
    pub name: Ident,
    /// 字段类型。
    pub ty: Type,
    /// 整个字段定义在源码中的区间。
    pub span: Span,
}
