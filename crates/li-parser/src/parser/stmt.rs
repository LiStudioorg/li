//! 语句、语句块、控制流与匹配模式解析。

use li_ast::{
    AssignStmt, Block, BreakStmt, ContinueStmt, ExprStmt, Ident, IfStmt, InlineAsmStmt, LetStmt,
    Literal, MatchArm, MatchStmt, Pattern, PatternKind, PrintStmt, ReturnStmt, Stmt, UnsafeStmt,
    WhileStmt,
};
use li_errors::Span;
use li_lexer::TokenKind;

use super::Parser;

impl Parser {
    /// 解析由花括号包围的语句块。
    ///
    /// 单条语句解析失败后进入恐慌模式，跳到 `;` 或 `}`，然后继续解析后续语句。
    pub(super) fn parse_block(&mut self) -> Option<Block> {
        let open = self.expect_kind(&TokenKind::LBrace, "`{`")?;
        let mut stmts = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_eof() {
            if self.eat(&TokenKind::Semicolon) {
                continue;
            }

            let before = self.current;
            match self.parse_statement() {
                Some(stmt) => stmts.push(stmt),
                None => self.synchronize_statement(),
            }

            if self.current == before {
                self.advance();
            }
        }

        let end = self.finish_block();
        Some(Block {
            stmts,
            span: Span::new(open.span.start, end),
        })
    }

    fn parse_statement(&mut self) -> Option<Stmt> {
        match &self.peek().kind {
            TokenKind::Let => self.parse_let_statement(),
            TokenKind::If => self.parse_if_statement(),
            TokenKind::While => self.parse_while_statement(),
            TokenKind::Break => self.parse_break_statement(),
            TokenKind::Continue => self.parse_continue_statement(),
            TokenKind::Return => self.parse_return_statement(),
            TokenKind::Match => self.parse_match_statement(),
            TokenKind::Unsafe => self.parse_unsafe_statement(),
            TokenKind::InlineAsm => self.parse_inline_asm_statement(),
            TokenKind::Print => self.parse_print_statement(),
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_let_statement(&mut self) -> Option<Stmt> {
        let let_token = self.advance();
        let mutable = self.eat(&TokenKind::Mut);
        let name = self.parse_identifier("变量名")?;
        let ty = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect_kind(&TokenKind::Assign, "`=`")?;
        let initializer = self.parse_expression()?;
        let end = self.finish_statement(initializer.span.end);
        let span = Span::new(let_token.span.start, end);

        Some(Stmt::Let(LetStmt {
            name,
            mutable,
            ty,
            initializer,
            span,
        }))
    }

    fn parse_if_statement(&mut self) -> Option<Stmt> {
        let if_token = self.advance();
        let condition = self.parse_expression()?;
        self.expect_kind(&TokenKind::Then, "`则`")?;
        let then_branch = self.parse_block()?;

        let mut end = then_branch.span.end;
        let else_branch = if self.eat(&TokenKind::Else) {
            let branch = self.parse_block()?;
            end = branch.span.end;
            Some(branch)
        } else {
            None
        };

        Some(Stmt::If(IfStmt {
            condition,
            then_branch,
            else_branch,
            span: Span::new(if_token.span.start, end),
        }))
    }

    fn parse_while_statement(&mut self) -> Option<Stmt> {
        let while_token = self.advance();
        let condition = self.parse_expression()?;
        let body = self.parse_block()?;
        let body_end = body.span.end;

        Some(Stmt::While(WhileStmt {
            condition,
            body,
            span: Span::new(while_token.span.start, body_end),
        }))
    }

    /// 解析 `不安全 { ... }` 块。
    fn parse_unsafe_statement(&mut self) -> Option<Stmt> {
        let unsafe_token = self.advance();
        let body = self.parse_block()?;
        let span = Span::new(unsafe_token.span.start, body.span.end);

        Some(Stmt::Unsafe(UnsafeStmt { body, span }))
    }

    /// 解析 `内联汇编「模板文本」;` 语句。
    ///
    /// 当前版本只保留语法：语义与 IR 阶段会给出中文“暂未实现”提示并
    /// 拦截，避免在无后端支持时静默生成错误代码。
    fn parse_inline_asm_statement(&mut self) -> Option<Stmt> {
        let asm_token = self.advance();
        let template = match &self.peek().kind {
            TokenKind::String(value) => value.clone(),
            _ => {
                self.error_expected("内联汇编模板字符串 `「...」`");
                return None;
            }
        };
        let template_token = self.advance();
        let end = self.finish_statement(template_token.span.end);

        Some(Stmt::InlineAsm(InlineAsmStmt {
            template,
            span: Span::new(asm_token.span.start, end),
        }))
    }

    fn parse_break_statement(&mut self) -> Option<Stmt> {
        let token = self.advance();
        let end = self.finish_statement(token.span.end);

        Some(Stmt::Break(BreakStmt {
            span: Span::new(token.span.start, end),
        }))
    }

    fn parse_continue_statement(&mut self) -> Option<Stmt> {
        let token = self.advance();
        let end = self.finish_statement(token.span.end);

        Some(Stmt::Continue(ContinueStmt {
            span: Span::new(token.span.start, end),
        }))
    }

    fn parse_return_statement(&mut self) -> Option<Stmt> {
        let token = self.advance();
        let value =
            if self.check(&TokenKind::Semicolon) || self.check(&TokenKind::RBrace) || self.is_eof()
            {
                None
            } else {
                Some(self.parse_expression()?)
            };

        let fallback = value
            .as_ref()
            .map(|expression| expression.span.end)
            .unwrap_or(token.span.end);
        let end = self.finish_statement(fallback);

        Some(Stmt::Return(ReturnStmt {
            value,
            span: Span::new(token.span.start, end),
        }))
    }

    fn parse_print_statement(&mut self) -> Option<Stmt> {
        let token = self.advance();

        let (value, fallback) = if self.check(&TokenKind::LParen) {
            self.advance();
            let value = self.parse_expression()?;
            let close = self.expect_kind(&TokenKind::RParen, "`)`")?;
            (value, close.span.end)
        } else {
            let value = self.parse_expression()?;
            let end = value.span.end;
            (value, end)
        };

        let end = self.finish_statement(fallback);
        Some(Stmt::Print(PrintStmt {
            value,
            span: Span::new(token.span.start, end),
        }))
    }

    fn parse_match_statement(&mut self) -> Option<Stmt> {
        let match_token = self.advance();
        let value = self.parse_expression()?;
        self.expect_kind(&TokenKind::LBrace, "`{`")?;

        let mut arms = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_eof() {
            let before = self.current;

            match self.parse_match_arm() {
                Some(arm) => {
                    arms.push(arm);

                    if self.eat(&TokenKind::Comma) {
                        continue;
                    }
                    if self.check(&TokenKind::RBrace) {
                        break;
                    }

                    self.error_expected("`,` 或 `}`");
                    self.recover_match_arm();
                }
                None => self.recover_match_arm(),
            }

            if self.current == before {
                self.advance();
            }
        }

        let end = self.finish_block();
        Some(Stmt::Match(MatchStmt {
            value,
            arms,
            span: Span::new(match_token.span.start, end),
        }))
    }

    fn parse_match_arm(&mut self) -> Option<MatchArm> {
        let pattern = self.parse_pattern()?;
        self.expect_kind(&TokenKind::FatArrow, "`=>`")?;
        let body = self.parse_block()?;

        Some(MatchArm {
            span: Span::new(pattern.span.start, body.span.end),
            pattern,
            body,
        })
    }

    /// 匹配分支恢复：跳到逗号后，或停在右花括号之前。
    fn recover_match_arm(&mut self) {
        while !self.is_eof() && !self.check(&TokenKind::Comma) && !self.check(&TokenKind::RBrace) {
            self.advance();
        }

        if self.check(&TokenKind::Comma) {
            self.advance();
        }
    }

    /// 解析表达式语句或赋值语句。
    ///
    /// 赋值按语言规范放在最低优先级：先由 Pratt 解析左侧表达式，
    /// 再在出现 `=` 时解析右侧完整表达式。
    fn parse_expression_statement(&mut self) -> Option<Stmt> {
        let target = self.parse_expression()?;

        if self.eat(&TokenKind::Assign) {
            let value = self.parse_expression()?;
            let end = self.finish_statement(value.span.end);
            let span = Span::new(target.span.start, end);

            Some(Stmt::Assign(AssignStmt {
                target,
                value,
                span,
            }))
        } else {
            let end = self.finish_statement(target.span.end);
            let span = Span::new(target.span.start, end);

            Some(Stmt::Expr(ExprStmt { expr: target, span }))
        }
    }

    /// 解析匹配模式。
    ///
    /// 当前核心语法为 `成功(...)` 与 `失败(...)`，同时保留通配、
    /// 名称绑定和字面量模式，便于后续扩展。
    fn parse_pattern(&mut self) -> Option<Pattern> {
        let token = self.peek().clone();
        let span = token.span;

        match token.kind {
            TokenKind::Success => {
                self.advance();
                let (binding, full_span) = self.parse_result_binding(span)?;
                Some(Pattern {
                    kind: PatternKind::Success(binding),
                    span: full_span,
                })
            }
            TokenKind::Failure => {
                self.advance();
                let (binding, full_span) = self.parse_result_binding(span)?;
                Some(Pattern {
                    kind: PatternKind::Failure(binding),
                    span: full_span,
                })
            }
            TokenKind::Identifier(name) => {
                self.advance();
                if name == "_" {
                    return Some(Pattern {
                        kind: PatternKind::Wildcard,
                        span,
                    });
                }
                Some(Pattern {
                    kind: PatternKind::Binding(Ident::new(name, span)),
                    span,
                })
            }
            TokenKind::Variant => {
                // 枚举项模式：`枚举项 标识符(文本)` 或 `枚举项 符号`。
                // `枚举项` 是显式关键字，与名称绑定（裸标识符）互斥。
                let _ = self.advance();
                let variant = self.parse_identifier("枚举项名称")?;
                let mut bindings = Vec::new();
                if self.check(&TokenKind::LParen) {
                    self.advance();
                    if !self.check(&TokenKind::RParen) {
                        loop {
                            let binding = self.parse_identifier("绑定名称");
                            bindings.push(binding);
                            if self.eat(&TokenKind::Comma) {
                                if self.check(&TokenKind::RParen) {
                                    break;
                                }
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect_kind(&TokenKind::RParen, "`)`")?;
                }
                Some(Pattern {
                    kind: PatternKind::Enum { variant, bindings },
                    span,
                })
            }
            TokenKind::Integer(value) => {
                self.advance();
                Some(Pattern {
                    kind: PatternKind::Literal(Literal::Integer(value)),
                    span,
                })
            }
            TokenKind::Float(value) => {
                self.advance();
                Some(Pattern {
                    kind: PatternKind::Literal(Literal::Float(value)),
                    span,
                })
            }
            TokenKind::String(value) => {
                self.advance();
                Some(Pattern {
                    kind: PatternKind::Literal(Literal::String(value)),
                    span,
                })
            }
            TokenKind::True => {
                self.advance();
                Some(Pattern {
                    kind: PatternKind::Literal(Literal::Bool(true)),
                    span,
                })
            }
            TokenKind::False => {
                self.advance();
                Some(Pattern {
                    kind: PatternKind::Literal(Literal::Bool(false)),
                    span,
                })
            }
            TokenKind::Null => {
                self.advance();
                Some(Pattern {
                    kind: PatternKind::Literal(Literal::Null),
                    span,
                })
            }
            _ => {
                self.error_expected("匹配模式（`成功`、`失败`、字面量或 `_`）");
                None
            }
        }
    }

    fn parse_result_binding(&mut self, start: Span) -> Option<(Option<Ident>, Span)> {
        if !self.eat(&TokenKind::LParen) {
            return Some((None, start));
        }

        let binding = if self.check(&TokenKind::RParen) {
            None
        } else {
            Some(self.parse_identifier("模式绑定名称")?)
        };
        let close = self.expect_kind(&TokenKind::RParen, "`)`")?;

        Some((binding, Span::new(start.start, close.span.end)))
    }
}
