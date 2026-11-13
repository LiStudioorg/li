//! 类型标注解析。

use li_ast::{Ident, Type, TypeKind};
use li_errors::Span;
use li_lexer::TokenKind;

use super::Parser;

impl Parser {
    /// 解析基础类型、泛型类型或具名类型。
    pub(super) fn parse_type(&mut self) -> Option<Type> {
        let token = self.peek().clone();
        let start = token.span.start;

        match token.kind {
            TokenKind::IntegerType => {
                self.advance();
                Some(Type::new(TypeKind::Integer, token.span))
            }
            TokenKind::FloatType => {
                self.advance();
                Some(Type::new(TypeKind::Float, token.span))
            }
            TokenKind::BoolType => {
                self.advance();
                Some(Type::new(TypeKind::Bool, token.span))
            }
            TokenKind::StringType => {
                self.advance();
                Some(Type::new(TypeKind::String, token.span))
            }
            TokenKind::ListType => {
                self.advance();
                self.expect_kind(&TokenKind::Less, "`<`")?;
                let element = self.parse_type()?;
                let close = self.expect_kind(&TokenKind::Greater, "`>`")?;
                let span = Span::new(start, close.span.end);
                Some(Type::new(TypeKind::List(Box::new(element)), span))
            }
            TokenKind::OptionalType => {
                self.advance();
                self.expect_kind(&TokenKind::Less, "`<`")?;
                let inner = self.parse_type()?;
                let close = self.expect_kind(&TokenKind::Greater, "`>`")?;
                let span = Span::new(start, close.span.end);
                Some(Type::new(TypeKind::Optional(Box::new(inner)), span))
            }
            TokenKind::ResultType => {
                self.advance();
                self.expect_kind(&TokenKind::Less, "`<`")?;
                let value = self.parse_type()?;
                self.expect_kind(&TokenKind::Comma, "`,`")?;
                let error = self.parse_type()?;
                let close = self.expect_kind(&TokenKind::Greater, "`>`")?;
                let span = Span::new(start, close.span.end);
                Some(Type::new(
                    TypeKind::Result {
                        value: Box::new(value),
                        error: Box::new(error),
                    },
                    span,
                ))
            }
            TokenKind::Identifier(name) => {
                self.advance();
                Some(Type::new(
                    TypeKind::Named(Ident::new(name, token.span)),
                    token.span,
                ))
            }
            _ => {
                self.error_expected("类型");
                None
            }
        }
    }
}
