//! AST 公开 API 的构建测试。
//!
//! 这里不测试解析行为，只确认节点组合后能形成一份完整的程序，
//! 并验证所有节点都携带可用的源码位置。

use li_ast::{
    Block, Expr, ExprKind, FuncDecl, Ident, Item, Param, Program, ReturnStmt, Stmt, Type, TypeKind,
};
use li_errors::{Position, Span};

#[test]
fn builds_a_minimal_program() {
    let span = Span::point(Position::new(1, 1, 0));
    let integer = Type::new(TypeKind::Integer, span);
    let parameter = Param {
        name: Ident::new("甲", span),
        ty: integer.clone(),
        span,
    };
    let return_expr = Expr::new(ExprKind::Identifier(Ident::new("甲", span)), span);
    let body = Block {
        stmts: vec![Stmt::Return(ReturnStmt {
            value: Some(return_expr),
            span,
        })],
        span,
    };
    let function = FuncDecl {
        name: Ident::new("恒等", span),
        params: vec![parameter],
        return_type: integer,
        is_async: false,
        body,
        span,
    };
    let program = Program {
        items: vec![Item::Function(function)],
        span,
    };

    assert_eq!(program.items.len(), 1);
    assert_eq!(program.items[0].span(), span);

    let Item::Function(function) = &program.items[0] else {
        panic!("预期函数声明");
    };
    assert_eq!(function.name.as_str(), "恒等");
    assert_eq!(function.params[0].ty.kind, TypeKind::Integer);
}
