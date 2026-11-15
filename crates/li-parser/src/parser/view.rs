//! `.lui` 视图声明解析。
//!
//! 视图语法只包含视图根、元素节点、属性、事件与状态：
//!
//! ```text
//! 视图 {
//!     状态 { 让 可变 次数 = 0; }
//!     容器 {
//!         文本 { 「计数：」, 次数 }
//!         按钮 { 点击 { 次数 = 次数 加 1; } 文本 { 「加一」 } }
//!     }
//! }
//! ```
//!
//! 解析器允许逗号或分号作为可选的项分隔符，因此下面两种写法等价：
//!
//! ```text
//! 容器 { 类: 「卡片」, 文本 { 「你好」 } }
//! 容器 { 类: 「卡片」 文本 { 「你好」 } }
//! ```
//!
//! 属性必须写在子节点之前；`文本 { 「...」 }` 中的字符串使用
//! li 统一的 `「」` 定界符，不接受 ASCII 引号。属性值写作 `{ 表达式 }`
//! 时表示运行时绑定。

use li_ast::{
    Ident, Stmt, ViewAttribute, ViewContent, ViewDecl, ViewEvent, ViewNode, ViewTextPart,
    ViewTextPartKind, ViewValue, ViewValueKind,
};
use li_errors::{Diagnostic, Position, Span};
use li_lexer::{Token, TokenKind};

use super::Parser;

/// 解析器能够识别的事件名。只有出现在此表中的名称才会被当作事件，
/// 其余 `名称 { ... }` 仍然是子节点。
const VIEW_EVENT_NAMES: &[&str] = &[
    "点击",
    "双击",
    "输入",
    "改变",
    "提交",
    "聚焦",
    "失焦",
    "按下",
    "松开",
    "鼠标进入",
    "鼠标离开",
];

/// 视图节点体解析后的内部结果。
struct ParsedViewBody {
    /// 节点属性。
    attributes: Vec<ViewAttribute>,
    /// 节点事件。
    events: Vec<ViewEvent>,
    /// 视图状态声明，仅根节点使用。
    state: Vec<Stmt>,
    /// 节点内容。
    content: ViewContent,
    /// 右花括号结束位置。
    end: Position,
}

impl Parser {
    /// 解析顶层 `视图 { ... }` 声明。
    pub(super) fn parse_view(&mut self) -> Option<ViewDecl> {
        let view_token = self.advance();
        self.expect_kind(&TokenKind::LBrace, "`{`")?;
        let body = self.parse_view_body(true)?;

        Some(ViewDecl {
            attributes: body.attributes,
            state: body.state,
            content: body.content,
            span: Span::new(view_token.span.start, body.end),
        })
    }

    /// 解析花括号内部的内容；调用前左花括号已经被消费。
    ///
    /// `is_root` 为真时允许 `状态 { ... }` 声明。
    fn parse_view_body(&mut self, is_root: bool) -> Option<ParsedViewBody> {
        let mut attributes = Vec::new();
        let mut state = Vec::new();
        let mut events = Vec::new();
        let mut children = Vec::new();
        let mut parts: Vec<ViewTextPart> = Vec::new();
        let mut has_content = false;

        while !self.check(&TokenKind::RBrace) && !self.is_eof() {
            // 逗号与分号都是可选分隔符，允许跨行书写。
            if self.eat(&TokenKind::Comma) || self.eat(&TokenKind::Semicolon) {
                continue;
            }

            let before = self.current;

            if self.is_view_name() && self.check_next(&TokenKind::Colon) {
                if has_content {
                    self.diagnostics.push(
                        Diagnostic::error("视图属性必须写在子节点之前")
                            .with_span(self.peek().span)
                            .with_note("请把属性移动到第一个子节点之前"),
                    );
                }

                match self.parse_view_attribute() {
                    Some(attribute) => attributes.push(attribute),
                    None => self.recover_view_item(),
                }
            } else if self.is_state_keyword() {
                if !is_root {
                    self.diagnostics.push(
                        Diagnostic::error("`状态` 只能写在视图根节点")
                            .with_span(self.peek().span)
                            .with_note("请把状态声明移动到 `视图 { ... }` 的直接层级"),
                    );
                }
                self.advance();
                match self.parse_block() {
                    Some(block) => {
                        if is_root {
                            state.extend(block.stmts);
                        }
                    }
                    None => self.recover_view_item(),
                }
            } else if self.is_event_name() {
                has_content = true;
                match self.parse_view_event() {
                    Some(event) => events.push(event),
                    None => self.recover_view_item(),
                }
            } else if let TokenKind::String(value) = self.peek().kind.clone() {
                if !children.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error("字符串文本之后不能再写子节点")
                            .with_span(self.peek().span)
                            .with_note("请把文本放进独立的 `文本 { 「...」 }` 节点"),
                    );
                    self.advance();
                } else {
                    has_content = true;
                    let span = self.peek().span;
                    self.advance();
                    parts.push(ViewTextPart {
                        kind: ViewTextPartKind::Text(value),
                        span,
                    });
                }
            } else if self.is_binding_expression() {
                if !children.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error("表达式文本之后不能再写子节点")
                            .with_span(self.peek().span)
                            .with_note("请把表达式放进独立的 `文本 { 表达式 }` 节点"),
                    );
                }

                has_content = true;
                let span = self.peek().span;
                match self.parse_expression() {
                    Some(expression) => parts.push(ViewTextPart {
                        kind: ViewTextPartKind::Expression(expression),
                        span,
                    }),
                    None => self.recover_view_item(),
                }
            } else if self.is_view_name() {
                if !parts.is_empty() {
                    self.diagnostics.push(
                        Diagnostic::error("字符串文本之后不能再写子节点")
                            .with_span(self.peek().span)
                            .with_note("请把文本放进独立的 `文本 { 「...」 }` 节点"),
                    );
                }

                has_content = true;
                match self.parse_view_node() {
                    Some(node) => children.push(node),
                    None => self.recover_view_item(),
                }
            } else {
                self.error_expected("视图属性、事件、子节点或 `}`");
                self.recover_view_item();
            }

            // 保证错误恢复时一定前进，避免死循环。
            if self.current == before {
                self.advance();
            }
        }

        let end = self.finish_block();
        let content = if !children.is_empty() {
            ViewContent::Children(children)
        } else if parts.is_empty() {
            ViewContent::Empty
        } else if parts.len() == 1 {
            let part = parts.pop().expect("parts 只有一个元素");
            match part.kind {
                ViewTextPartKind::Text(text) => ViewContent::Text(text),
                kind => ViewContent::Parts(vec![ViewTextPart {
                    kind,
                    span: part.span,
                }]),
            }
        } else {
            ViewContent::Parts(parts)
        };

        Some(ParsedViewBody {
            attributes,
            state,
            events,
            content,
            end,
        })
    }

    /// 解析 `标签名 { ... }` 形式的子节点。
    fn parse_view_node(&mut self) -> Option<ViewNode> {
        let name = self.parse_view_name("视图标签名")?;
        self.expect_kind(&TokenKind::LBrace, "`{`")?;
        let body = self.parse_view_body(false)?;

        Some(ViewNode {
            span: Span::new(name.span.start, body.end),
            name,
            attributes: body.attributes,
            events: body.events,
            content: body.content,
        })
    }

    /// 解析 `事件名 { 语句 }`。
    fn parse_view_event(&mut self) -> Option<ViewEvent> {
        let name = self.parse_view_name("事件名")?;
        let body = self.parse_block()?;

        Some(ViewEvent {
            span: Span::new(name.span.start, body.span.end),
            name,
            body,
        })
    }

    /// 解析 `属性名: 值`。
    fn parse_view_attribute(&mut self) -> Option<ViewAttribute> {
        let name = self.parse_view_name("属性名")?;
        self.expect_kind(&TokenKind::Colon, "`:`")?;
        let value = self.parse_view_value()?;

        Some(ViewAttribute {
            span: Span::new(name.span.start, value.span.end),
            name,
            value,
        })
    }

    /// 解析属性值：字符串、整数、小数、布尔值、标识符或 `{ 表达式 }`。
    fn parse_view_value(&mut self) -> Option<ViewValue> {
        if self.check(&TokenKind::LBrace) {
            let open = self.advance();
            let expression = self.parse_expression()?;
            let close = self.expect_kind(&TokenKind::RBrace, "`}`")?;
            return Some(ViewValue {
                span: Span::new(open.span.start, close.span.end),
                kind: ViewValueKind::Expression(expression),
            });
        }

        let token = self.peek().clone();

        let kind = match &token.kind {
            TokenKind::String(value) => ViewValueKind::Text(value.clone()),
            TokenKind::Integer(value) => ViewValueKind::Integer(*value),
            TokenKind::Float(value) => ViewValueKind::Float(*value),
            TokenKind::True => ViewValueKind::Bool(true),
            TokenKind::False => ViewValueKind::Bool(false),
            TokenKind::Identifier(name) => ViewValueKind::Identifier(name.clone()),
            kind => match kind.keyword_name() {
                Some(name) if !matches!(kind, TokenKind::Null) => {
                    ViewValueKind::Identifier(name.to_owned())
                }
                _ => {
                    self.error_expected(
                        "属性值（字符串、数字、`真`、`假`、标识符或 `{ 表达式 }`）",
                    );
                    return None;
                }
            },
        };

        self.advance();
        Some(ViewValue {
            kind,
            span: token.span,
        })
    }

    /// 读取一个标签名或属性名。
    ///
    /// 关键字可以直接作为视图名称使用，例如 `列表 { ... }` 与
    /// `类型: 「button」`；词法阶段已经丢失原始英文别名，因此统一
    /// 还原为规范中文写法。
    fn parse_view_name(&mut self, expected: &str) -> Option<Ident> {
        let token = self.peek().clone();

        let Some(name) = Self::view_name_text(&token) else {
            self.error_expected(expected);
            return None;
        };

        self.advance();
        Some(Ident::new(name, token.span))
    }

    /// 当前 Token 是否可以作为视图名称。
    fn is_view_name(&self) -> bool {
        Self::view_name_text(self.peek()).is_some()
    }

    /// 当前 Token 是否为视图根部的 `状态` 关键字。
    fn is_state_keyword(&self) -> bool {
        matches!(&self.peek().kind, TokenKind::Identifier(name) if name == "状态")
            && self.check_next(&TokenKind::LBrace)
    }

    /// 当前 Token 是否为已知事件名，且后面紧跟语句块。
    fn is_event_name(&self) -> bool {
        let Some(name) = Self::view_name_text(self.peek()) else {
            return false;
        };

        VIEW_EVENT_NAMES.contains(&name) && self.check_next(&TokenKind::LBrace)
    }

    /// 当前位置是否是一个等待解析的文本表达式。
    ///
    /// 子节点必须写成 `名称 { ... }`，属性必须写成 `名称: ...`；
    /// 其余可解析的表达式都视为文本插值。
    fn is_binding_expression(&self) -> bool {
        if !self.is_view_name() || self.check_next(&TokenKind::LBrace) {
            return false;
        }

        !self.check_next(&TokenKind::Colon)
    }

    /// 从 Token 中取出视图名称文本。
    fn view_name_text(token: &Token) -> Option<&str> {
        match &token.kind {
            TokenKind::Identifier(name) => Some(name),
            kind => kind.keyword_name(),
        }
    }

    /// 视图项级错误恢复：跳到下一个属性、子节点或右花括号。
    fn recover_view_item(&mut self) {
        let mut consumed = false;

        while !self.is_eof() && !self.check(&TokenKind::RBrace) {
            if self.check(&TokenKind::Comma) || self.check(&TokenKind::Semicolon) {
                self.advance();
                return;
            }

            // 已经跳过一个损坏项后，遇到新的合法项就停止，避免吞掉后续节点。
            if consumed
                && (matches!(self.peek().kind, TokenKind::String(_))
                    || (self.is_view_name()
                        && (self.check_next(&TokenKind::LBrace)
                            || self.check_next(&TokenKind::Colon))))
            {
                return;
            }

            self.advance();
            consumed = true;
        }
    }
}
