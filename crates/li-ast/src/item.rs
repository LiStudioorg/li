//! 顶层程序项节点。

use li_errors::Span;

use crate::common::Ident;
use crate::stmt::Block;
use crate::ty::Type;
use crate::view::ViewDecl;

/// 一份完整的 li 源文件。
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    /// 顶层程序项，按源码顺序保存。
    pub items: Vec<Item>,
    /// 是否声明了 `#无垃圾回收` 文件头指令。
    ///
    /// 该标记表示程序面向 no_std 目标：不依赖垃圾回收，堆分配类型
    /// （字符串、列表、字典）会产生黄色警告，底层内存操作仍须放在
    /// `不安全` 块中。
    pub no_gc: bool,
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
    /// 枚举声明，例如 `枚举 词法类型 { 标识符(字符串), 数字(整数), 符号 }`。
    Enum(EnumDecl),
    /// `.lui` 视图声明。
    View(ViewDecl),
}

impl Item {
    /// 返回程序项对应的源码区间。
    pub fn span(&self) -> Span {
        match self {
            Self::Module(item) => item.span,
            Self::Import(item) => item.span,
            Self::Function(item) => item.span,
            Self::Type(item) => item.span,
            Self::Enum(item) => item.span,
            Self::View(item) => item.span,
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
    /// 是否为异步函数；`异步 函数` 声明的函数调用后得到 `任务<T>`。
    pub is_async: bool,
    /// 是否为 C-ABI 外部函数；声明时没有函数体，由宿主动态链接库提供
    /// 实现。LLVM 后端直接生成 `declare`，VM / JS 后端无法在运行时绑定，
    /// 会报红色致命错误并拦截编译。
    pub is_extern: bool,
    /// 函数体；外部函数允许为空块（解析器保证占位）。
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

/// 枚举声明，对应 `枚举 词法类型 { 标识符(字符串), 数字(整数), 符号 }`。
///
/// 每个枚举项（variant）可以携带 0 到 N 个参数，用于表达
/// 「标识符(字符串)、数字(整数)、符号」这类带载荷的枚举；
/// 匹配时必须穷尽所有枚举项，未覆盖的项由 Sema 报红色错误。
#[derive(Clone, Debug, PartialEq)]
pub struct EnumDecl {
    /// 枚举名称。
    pub name: Ident,
    /// 枚举项列表，按源码顺序保存。
    pub variants: Vec<EnumVariant>,
    /// 整个枚举声明在源码中的区间。
    pub span: Span,
}

/// 一个枚举项，例如 `标识符(字符串)` 或 `符号`。
#[derive(Clone, Debug, PartialEq)]
pub struct EnumVariant {
    /// 枚举项名称。
    pub name: Ident,
    /// 枚举项的参数列表；没有参数表示「裸」枚举项。
    pub args: Vec<Type>,
    /// 整个枚举项在源码中的区间。
    pub span: Span,
}
