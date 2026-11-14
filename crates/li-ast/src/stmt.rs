//! 语句与语句块节点。

use li_errors::Span;

use crate::common::Ident;
use crate::expr::Expr;
use crate::pattern::Pattern;
use crate::ty::Type;

/// 由一对花括号包围的语句块。
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// 块内语句，按源码顺序保存。
    pub stmts: Vec<Stmt>,
    /// 整个语句块在源码中的区间，通常包含两侧花括号。
    pub span: Span,
}

/// 一条语句。
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// 变量声明。
    Let(LetStmt),
    /// 赋值语句。
    Assign(AssignStmt),
    /// 表达式语句。
    Expr(ExprStmt),
    /// 返回语句。
    Return(ReturnStmt),
    /// 条件语句。
    If(IfStmt),
    /// 循环语句。
    While(WhileStmt),
    /// 跳出当前循环。
    Break(BreakStmt),
    /// 继续下一次循环。
    Continue(ContinueStmt),
    /// 模式匹配语句。
    Match(MatchStmt),
    /// 不安全块，用于隔离底层内存操作。
    Unsafe(UnsafeStmt),
    /// 打印语句。
    Print(PrintStmt),
}

impl Stmt {
    /// 返回语句对应的源码区间。
    pub fn span(&self) -> Span {
        match self {
            Self::Let(stmt) => stmt.span,
            Self::Assign(stmt) => stmt.span,
            Self::Expr(stmt) => stmt.span,
            Self::Return(stmt) => stmt.span,
            Self::If(stmt) => stmt.span,
            Self::While(stmt) => stmt.span,
            Self::Break(stmt) => stmt.span,
            Self::Continue(stmt) => stmt.span,
            Self::Match(stmt) => stmt.span,
            Self::Unsafe(stmt) => stmt.span,
            Self::Print(stmt) => stmt.span,
        }
    }
}

/// 变量声明，例如 `让 可变 计数 = 0;`。
#[derive(Clone, Debug, PartialEq)]
pub struct LetStmt {
    /// 变量名。
    pub name: Ident,
    /// 是否由 `可变` 修饰。
    pub mutable: bool,
    /// 可选的类型标注。
    pub ty: Option<Type>,
    /// 初始化表达式。当前语法要求变量声明必须初始化。
    pub initializer: Expr,
    /// 整条声明语句的源码区间。
    pub span: Span,
}

/// 赋值语句，例如 `计数 = 计数 加 1;`。
#[derive(Clone, Debug, PartialEq)]
pub struct AssignStmt {
    /// 赋值目标，语义分析阶段会检查它是否可写。
    pub target: Expr,
    /// 右侧的值。
    pub value: Expr,
    /// 整条赋值语句的源码区间。
    pub span: Span,
}

/// 表达式语句，例如 `求和(1, 2);`。
#[derive(Clone, Debug, PartialEq)]
pub struct ExprStmt {
    /// 被求值的表达式。
    pub expr: Expr,
    /// 整条语句的源码区间。
    pub span: Span,
}

/// 返回语句，例如 `返回 结果;`。
#[derive(Clone, Debug, PartialEq)]
pub struct ReturnStmt {
    /// 可选的返回值；省略时表示不带值返回。
    pub value: Option<Expr>,
    /// 整条返回语句的源码区间。
    pub span: Span,
}

/// 条件语句。
#[derive(Clone, Debug, PartialEq)]
pub struct IfStmt {
    /// 条件表达式，必须是 `布尔` 类型。
    pub condition: Expr,
    /// 条件为真时执行的语句块。
    pub then_branch: Block,
    /// 可选的否则分支语句块。
    pub else_branch: Option<Block>,
    /// 整条条件语句的源码区间。
    pub span: Span,
}

/// 当型循环语句。
#[derive(Clone, Debug, PartialEq)]
pub struct WhileStmt {
    /// 循环条件，必须是 `布尔` 类型。
    pub condition: Expr,
    /// 循环体。
    pub body: Block,
    /// 整条循环语句的源码区间。
    pub span: Span,
}

/// 跳出语句。
#[derive(Clone, Debug, PartialEq)]
pub struct BreakStmt {
    /// 语句在源码中的区间。
    pub span: Span,
}

/// 继续语句。
#[derive(Clone, Debug, PartialEq)]
pub struct ContinueStmt {
    /// 语句在源码中的区间。
    pub span: Span,
}

/// 模式匹配语句。
#[derive(Clone, Debug, PartialEq)]
pub struct MatchStmt {
    /// 被匹配的表达式。
    pub value: Expr,
    /// 匹配分支，按源码顺序保存。
    pub arms: Vec<MatchArm>,
    /// 整条匹配语句的源码区间。
    pub span: Span,
}

/// 一个匹配分支，例如 `成功(值) => { 打印(值); }`。
#[derive(Clone, Debug, PartialEq)]
pub struct MatchArm {
    /// 分支模式。
    pub pattern: Pattern,
    /// 模式命中后执行的语句块。
    pub body: Block,
    /// 整个分支的源码区间。
    pub span: Span,
}

/// 不安全块，例如 `不安全 { 指针 = 分配(8); }`。
///
/// 底层指针解引用、手动内存分配与释放等操作必须位于该块内；
/// 离开块后立即恢复严格的安全检查。
#[derive(Clone, Debug, PartialEq)]
pub struct UnsafeStmt {
    /// 块内语句。
    pub body: Block,
    /// 整个不安全块在源码中的区间。
    pub span: Span,
}

/// 打印语句，例如 `打印(结果);`。
#[derive(Clone, Debug, PartialEq)]
pub struct PrintStmt {
    /// 要打印的表达式。
    pub value: Expr,
    /// 整条打印语句的源码区间。
    pub span: Span,
}
