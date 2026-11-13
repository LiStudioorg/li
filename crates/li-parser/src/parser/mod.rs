//! 递归下降解析器核心：程序项、模块、导入、函数与类型声明。
//!
//! 表达式使用 Pratt 解析，见 `expr` 子模块；语句与匹配模式见 `stmt`；
//! 类型标注见 `ty`。所有错误都写入 `li-errors` 的诊断集合，并在语句层
//! 使用恐慌模式恢复到 `;` 或 `}` 后继续解析。

use li_ast::{
    FieldDef, FuncDecl, Ident, ImportDecl, Item, ModuleDecl, ModulePath, Param, Program, TypeDecl,
};
use li_errors::{Diagnostic, DiagnosticBag, Position, Span};
use li_lexer::{Token, TokenKind};

mod expr;
mod stmt;
mod ty;

/// li 语言递归下降解析器。
pub struct Parser {
    /// 词法分析器产生的 Token 序列，末尾始终有一个 `Eof`。
    tokens: Vec<Token>,
    /// 当前 Token 下标。
    current: usize,
    /// 解析阶段收集到的诊断。
    diagnostics: DiagnosticBag,
}

impl Parser {
    /// 创建解析器；若 Token 序列没有文件结束标记，会自动补一个。
    pub fn new(mut tokens: Vec<Token>) -> Self {
        if tokens.last().map(Token::is_eof) != Some(true) {
            let position = tokens
                .last()
                .map(|token| token.span.end)
                .unwrap_or_else(Position::start);
            tokens.push(Token::new(TokenKind::Eof, Span::point(position)));
        }

        Self {
            tokens,
            current: 0,
            diagnostics: DiagnosticBag::new(),
        }
    }

    /// 解析整份程序。存在任何语法错误时返回全部诊断。
    pub fn parse(mut self) -> Result<Program, DiagnosticBag> {
        let program = self.parse_program();

        if self.diagnostics.has_errors() {
            Err(self.diagnostics)
        } else {
            Ok(program)
        }
    }

    fn parse_program(&mut self) -> Program {
        let start = self.peek().span.start;
        let mut items = Vec::new();

        while !self.is_eof() {
            let before = self.current;

            match self.parse_item() {
                Some(item) => {
                    items.push(item);
                    // 顶层项允许带一个可选分号。
                    self.eat(&TokenKind::Semicolon);
                }
                None => self.synchronize_item(false),
            }

            // 保证错误恢复时一定前进，避免死循环。
            if self.current == before {
                self.advance();
            }
        }

        let end = items
            .last()
            .map(|item| item.span().end)
            .unwrap_or_else(|| self.peek().span.start);

        Program {
            items,
            span: Span::new(start, end),
        }
    }

    fn parse_item(&mut self) -> Option<Item> {
        match &self.peek().kind {
            TokenKind::Module => self.parse_module().map(Item::Module),
            TokenKind::Import => self.parse_import().map(Item::Import),
            TokenKind::Func => self.parse_function().map(Item::Function),
            TokenKind::Type => self.parse_type_decl().map(Item::Type),
            _ => {
                self.error_expected("`模块`、`导入`、`函数` 或 `类型`");
                None
            }
        }
    }

    fn parse_module(&mut self) -> Option<ModuleDecl> {
        let module_token = self.advance();
        let name = self.parse_identifier("模块名称")?;
        self.expect_kind(&TokenKind::LBrace, "`{`")?;

        let mut items = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_eof() {
            let before = self.current;

            match self.parse_item() {
                Some(item) => {
                    items.push(item);
                    self.eat(&TokenKind::Semicolon);
                }
                None => self.synchronize_item(true),
            }

            if self.current == before {
                self.advance();
            }
        }

        let end = self.finish_block();
        Some(ModuleDecl {
            name,
            items,
            span: Span::new(module_token.span.start, end),
        })
    }

    fn parse_import(&mut self) -> Option<ImportDecl> {
        let import_token = self.advance();
        let first = self.parse_identifier("模块名称")?;
        let path_start = first.span.start;
        let mut segments = vec![first];

        // 路径既接受 `工具::数学`，也兼容 `工具.数学`。
        while self.eat(&TokenKind::ColonColon) || self.eat(&TokenKind::Dot) {
            let segment = self.parse_identifier("模块名")?;
            segments.push(segment);
        }

        let path_end = segments
            .last()
            .map(|segment| segment.span.end)
            .unwrap_or(path_start);
        let end = self.finish_statement(path_end);

        Some(ImportDecl {
            path: ModulePath {
                segments,
                span: Span::new(path_start, path_end),
            },
            span: Span::new(import_token.span.start, end),
        })
    }

    fn parse_function(&mut self) -> Option<FuncDecl> {
        let function_token = self.advance();
        let name = self.parse_identifier("函数名称")?;
        self.expect_kind(&TokenKind::LParen, "`(`")?;
        let params = self.parse_params()?;
        self.expect_kind(&TokenKind::RParen, "`)`")?;
        self.expect_kind(&TokenKind::Arrow, "`->`")?;
        let return_type = self.parse_type()?;
        let body = self.parse_block()?;
        let body_end = body.span.end;

        Some(FuncDecl {
            name,
            params,
            return_type,
            body,
            span: Span::new(function_token.span.start, body_end),
        })
    }

    fn parse_params(&mut self) -> Option<Vec<Param>> {
        let mut params = Vec::new();

        if self.check(&TokenKind::RParen) {
            return Some(params);
        }

        loop {
            let name = self.parse_identifier("参数名称")?;
            self.expect_kind(&TokenKind::Colon, "`:`")?;
            let ty = self.parse_type()?;
            let span = Span::new(name.span.start, ty.span.end);
            params.push(Param { name, ty, span });

            if self.eat(&TokenKind::Comma) {
                // 允许参数列表末尾多一个逗号。
                if self.check(&TokenKind::RParen) {
                    break;
                }
                continue;
            }

            break;
        }

        Some(params)
    }

    fn parse_type_decl(&mut self) -> Option<TypeDecl> {
        let type_token = self.advance();
        let name = self.parse_identifier("类型名称")?;
        self.expect_kind(&TokenKind::LBrace, "`{`")?;

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_eof() {
            if self.eat(&TokenKind::Comma) {
                continue;
            }

            let before = self.current;
            match self.parse_field() {
                Some(field) => {
                    fields.push(field);

                    if self.eat(&TokenKind::Comma) {
                        continue;
                    }
                    if self.check(&TokenKind::RBrace) {
                        break;
                    }

                    self.error_expected("`,` 或 `}`");
                    self.recover_type_field();
                }
                None => self.recover_type_field(),
            }

            if self.current == before {
                self.advance();
            }
        }

        let end = self.finish_block();
        Some(TypeDecl {
            name,
            fields,
            span: Span::new(type_token.span.start, end),
        })
    }

    fn parse_field(&mut self) -> Option<FieldDef> {
        let name = self.parse_identifier("字段名称")?;
        self.expect_kind(&TokenKind::Colon, "`:`")?;
        let ty = self.parse_type()?;

        Some(FieldDef {
            span: Span::new(name.span.start, ty.span.end),
            name,
            ty,
        })
    }

    /// 跳过损坏的字段，直到逗号或右花括号。
    fn recover_type_field(&mut self) {
        while !self.is_eof() && !self.check(&TokenKind::Comma) && !self.check(&TokenKind::RBrace) {
            self.advance();
        }

        if self.check(&TokenKind::Comma) {
            self.advance();
        }
    }

    // ---- 通用 Token 操作与恢复 ----

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn is_eof(&self) -> bool {
        self.peek().is_eof()
    }

    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if self.current + 1 < self.tokens.len() {
            self.current += 1;
        }
        token
    }

    fn expect_kind(&mut self, kind: &TokenKind, expected: &str) -> Option<Token> {
        if self.check(kind) {
            Some(self.advance())
        } else {
            self.error_expected(expected);
            None
        }
    }

    fn parse_identifier(&mut self, expected: &str) -> Option<Ident> {
        let token = self.peek().clone();

        if let TokenKind::Identifier(name) = token.kind {
            self.advance();
            Some(Ident::new(name, token.span))
        } else {
            self.error_expected(expected);
            None
        }
    }

    fn error_expected(&mut self, expected: &str) {
        let span = self.peek().span;
        let actual = self.peek().kind.to_string();
        self.diagnostics
            .push(Diagnostic::syntax_error(span, expected, actual));
    }

    /// 消费必需的分号；缺失时只记录诊断，不丢弃已构造的语句。
    fn finish_statement(&mut self, fallback_end: Position) -> Position {
        if self.check(&TokenKind::Semicolon) {
            self.advance().span.end
        } else {
            self.error_expected("`;`");
            fallback_end
        }
    }

    /// 消费语句块或模块的右花括号；末尾缺失时记录诊断。
    fn finish_block(&mut self) -> Position {
        if self.check(&TokenKind::RBrace) {
            self.advance().span.end
        } else {
            self.error_expected("`}`");
            self.peek().span.start
        }
    }

    /// 语句级错误恢复：跳到分号之后，或停在右花括号之前。
    fn synchronize_statement(&mut self) {
        while !self.is_eof() {
            if self.check(&TokenKind::RBrace) {
                return;
            }
            if self.check(&TokenKind::Semicolon) {
                self.advance();
                return;
            }
            self.advance();
        }
    }

    /// 程序项级错误恢复：跳到下一个顶层项，模块内还会停在 `}`。
    fn synchronize_item(&mut self, in_module: bool) {
        while !self.is_eof() {
            if in_module && self.check(&TokenKind::RBrace) {
                return;
            }

            match &self.peek().kind {
                TokenKind::Module | TokenKind::Import | TokenKind::Func | TokenKind::Type => return,
                _ => {}
            }

            self.advance();
        }
    }
}

/// 对 Token 序列执行语法分析。
pub fn parse(tokens: Vec<Token>) -> Result<Program, DiagnosticBag> {
    Parser::new(tokens).parse()
}

/// 直接从源码执行词法分析与语法分析。
pub fn parse_source(source: &str) -> Result<Program, DiagnosticBag> {
    let tokens = li_lexer::lex(source)?;
    parse(tokens)
}
