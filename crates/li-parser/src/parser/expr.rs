//! 表达式解析：Pratt 优先级攀升与后缀链。

use li_ast::{BinaryOp, Expr, ExprKind, Ident, Literal, UnaryOp};
use li_errors::Span;
use li_lexer::TokenKind;

use super::Parser;

// 绑定力遵循 C 系优先级：
// 乘除 > 加减 > 比较 > 且 > 或。
// 赋值由语句解析层按最低优先级单独处理，不进入二元表达式。
const OR_BINDING_POWER: (u8, u8) = (10, 11);
const AND_BINDING_POWER: (u8, u8) = (20, 21);
const COMPARISON_BINDING_POWER: (u8, u8) = (30, 31);
const SUM_BINDING_POWER: (u8, u8) = (40, 41);
const PRODUCT_BINDING_POWER: (u8, u8) = (50, 51);

impl Parser {
    /// 解析一个完整表达式。
    pub(super) fn parse_expression(&mut self) -> Option<Expr> {
        self.parse_precedence(0)
    }

    /// Pratt 核心：按最小绑定力解析二元表达式。
    fn parse_precedence(&mut self, min_binding_power: u8) -> Option<Expr> {
        let mut left = self.parse_unary()?;

        while let Some((operator, left_power, right_power)) = self.binary_operator() {
            if left_power < min_binding_power {
                break;
            }

            self.advance();
            let right = self.parse_precedence(right_power)?;
            let span = left.span.merge(right.span);

            left = Expr::new(
                ExprKind::Binary {
                    left: Box::new(left),
                    op: operator,
                    right: Box::new(right),
                },
                span,
            );
        }

        Some(left)
    }

    /// 返回当前 Token 对应的二元运算符及其左右绑定力。
    fn binary_operator(&self) -> Option<(BinaryOp, u8, u8)> {
        let (operator, (left, right)) = match &self.peek().kind {
            TokenKind::OrOr => (BinaryOp::Or, OR_BINDING_POWER),
            TokenKind::AndAnd => (BinaryOp::And, AND_BINDING_POWER),
            TokenKind::EqualEqual => (BinaryOp::Equal, COMPARISON_BINDING_POWER),
            TokenKind::Less => (BinaryOp::Less, COMPARISON_BINDING_POWER),
            TokenKind::Greater => (BinaryOp::Greater, COMPARISON_BINDING_POWER),
            TokenKind::Plus => (BinaryOp::Add, SUM_BINDING_POWER),
            TokenKind::Minus => (BinaryOp::Subtract, SUM_BINDING_POWER),
            TokenKind::Star => (BinaryOp::Multiply, PRODUCT_BINDING_POWER),
            TokenKind::Slash => (BinaryOp::Divide, PRODUCT_BINDING_POWER),
            _ => return None,
        };

        Some((operator, left, right))
    }

    /// 解析一元运算；一元运算优先级高于所有二元运算符。
    fn parse_unary(&mut self) -> Option<Expr> {
        let token = self.peek().clone();
        let operator = match token.kind {
            TokenKind::Minus => Some(UnaryOp::Negate),
            TokenKind::Not => Some(UnaryOp::Not),
            _ => None,
        };

        if let Some(operator) = operator {
            self.advance();
            let operand = self.parse_unary()?;
            let span = Span::new(token.span.start, operand.span.end);

            return Some(Expr::new(
                ExprKind::Unary {
                    op: operator,
                    operand: Box::new(operand),
                },
                span,
            ));
        }

        self.parse_postfix()
    }

    /// 解析函数调用、字段访问与下标访问组成的后缀链。
    fn parse_postfix(&mut self) -> Option<Expr> {
        let mut expression = self.parse_primary()?;

        loop {
            if self.check(&TokenKind::LParen) {
                self.advance();
                let args = self.parse_expression_list(&TokenKind::RParen)?;
                let close = self.expect_kind(&TokenKind::RParen, "`)`")?;
                let span = Span::new(expression.span.start, close.span.end);

                expression = Expr::new(
                    ExprKind::Call {
                        callee: Box::new(expression),
                        args,
                    },
                    span,
                );
            } else if self.eat(&TokenKind::Dot) {
                let token = self.peek().clone();
                let field_name = match &token.kind {
                    TokenKind::Identifier(name) => name.clone(),
                    // 允许关键字作为字段名，例如 `正则.匹配`、`数据.类型`。
                    other => match other.keyword_name() {
                        Some(name) => name.to_owned(),
                        None => {
                            self.error_expected("字段名称");
                            return None;
                        }
                    },
                };
                let _ = self.advance();
                let field = Ident::new(field_name, token.span);
                let span = Span::new(expression.span.start, token.span.end);

                expression = Expr::new(
                    ExprKind::FieldAccess {
                        object: Box::new(expression),
                        field,
                    },
                    span,
                );
            } else if self.check(&TokenKind::LBracket) {
                self.advance();
                let index = self.parse_expression()?;
                let close = self.expect_kind(&TokenKind::RBracket, "`]`")?;
                let span = Span::new(expression.span.start, close.span.end);

                expression = Expr::new(
                    ExprKind::Index {
                        object: Box::new(expression),
                        index: Box::new(index),
                    },
                    span,
                );
            } else {
                break;
            }
        }

        Some(expression)
    }

    /// 解析最基础的表达式。
    fn parse_primary(&mut self) -> Option<Expr> {
        if self.check(&TokenKind::LParen) {
            return self.parse_parenthesized();
        }
        if self.check(&TokenKind::LBracket) {
            return self.parse_list_literal();
        }
        if self.check(&TokenKind::Await) {
            return self.parse_await();
        }

        let token = self.peek().clone();
        let span = token.span;

        let kind = match token.kind {
            TokenKind::Integer(value) => ExprKind::Literal(Literal::Integer(value)),
            TokenKind::Float(value) => ExprKind::Literal(Literal::Float(value)),
            TokenKind::String(value) => ExprKind::Literal(Literal::String(value)),
            TokenKind::True => ExprKind::Literal(Literal::Bool(true)),
            TokenKind::False => ExprKind::Literal(Literal::Bool(false)),
            TokenKind::Null => ExprKind::Literal(Literal::Null),
            TokenKind::Identifier(name) => ExprKind::Identifier(Ident::new(name, span)),
            TokenKind::Input => ExprKind::Input,
            _ => {
                self.error_expected("表达式");
                return None;
            }
        };

        self.advance();
        Some(Expr::new(kind, span))
    }

    /// 解析 `等待 表达式`；等待是前缀运算符，优先级高于二元运算。
    fn parse_await(&mut self) -> Option<Expr> {
        let await_token = self.advance();
        let value = self.parse_unary()?;
        let span = Span::new(await_token.span.start, value.span.end);

        Some(Expr::new(ExprKind::Await(Box::new(value)), span))
    }

    fn parse_parenthesized(&mut self) -> Option<Expr> {
        let open = self.advance();
        let inner = self.parse_expression()?;
        let close = self.expect_kind(&TokenKind::RParen, "`)`")?;
        let span = Span::new(open.span.start, close.span.end);

        Some(Expr::new(ExprKind::Parenthesized(Box::new(inner)), span))
    }

    fn parse_list_literal(&mut self) -> Option<Expr> {
        let open = self.advance();
        let items = self.parse_expression_list(&TokenKind::RBracket)?;
        let close = self.expect_kind(&TokenKind::RBracket, "`]`")?;
        let span = Span::new(open.span.start, close.span.end);

        Some(Expr::new(ExprKind::ListLiteral(items), span))
    }

    /// 解析逗号分隔的表达式列表，允许空列表与尾随逗号。
    fn parse_expression_list(&mut self, close: &TokenKind) -> Option<Vec<Expr>> {
        let mut expressions = Vec::new();

        if self.check(close) {
            return Some(expressions);
        }

        loop {
            expressions.push(self.parse_expression()?);

            if self.eat(&TokenKind::Comma) {
                if self.check(close) {
                    break;
                }
                continue;
            }

            break;
        }

        Some(expressions)
    }
}
