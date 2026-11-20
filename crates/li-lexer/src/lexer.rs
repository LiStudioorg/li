//! li 语言词法分析器实现。

use li_errors::{Diagnostic, DiagnosticBag, Position, Span};

use crate::token::{Token, TokenKind};

/// li 源代码词法分析器。
///
/// 分析器在产生 Token 的同时记录源码区间。全角标点、英文关键字别名
/// 和中文运算符词都会归一化为统一的 [`TokenKind`]。
pub struct Lexer<'a> {
    /// 待分析的源码。
    source: &'a str,
    /// 当前字节偏移。
    offset: usize,
    /// 当前行号，从 1 开始。
    line: usize,
    /// 当前列号，从 1 开始，按 Unicode 字符计数。
    column: usize,
    /// 词法阶段收集到的诊断。
    diagnostics: DiagnosticBag,
}

impl<'a> Lexer<'a> {
    /// 创建针对一份源码的词法分析器。
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            line: 1,
            column: 1,
            diagnostics: DiagnosticBag::new(),
        }
    }

    /// 执行词法分析。
    ///
    /// 成功时返回包含 [`TokenKind::Eof`] 的完整 Token 序列；
    /// 只要出现任何词法错误，就返回包含全部中文诊断的 [`DiagnosticBag`]。
    pub fn tokenize(mut self) -> Result<Vec<Token>, DiagnosticBag> {
        let mut tokens = Vec::new();

        loop {
            self.skip_trivia();
            let start = self.position();

            if self.peek().is_none() {
                tokens.push(Token::new(TokenKind::Eof, Span::point(start)));
                break;
            }

            if let Some(kind) = self.next_token(start) {
                tokens.push(Token::new(kind, Span::new(start, self.position())));
            }
        }

        if self.diagnostics.has_errors() {
            Err(self.diagnostics)
        } else {
            Ok(tokens)
        }
    }

    /// 跳过空白、单行注释与可嵌套的多行注释。
    fn skip_trivia(&mut self) {
        loop {
            while matches!(self.peek(), Some(ch) if ch.is_whitespace()) {
                self.advance();
            }

            if self.starts_with("#*") {
                self.skip_block_comment(CommentStyle::Hash);
            } else if self.starts_with("/*") {
                self.skip_block_comment(CommentStyle::C);
            } else if self.starts_with("#") || self.starts_with("//") {
                self.skip_line_comment();
            } else {
                break;
            }
        }
    }

    /// 跳过一行注释，保留行尾换行符供后续空白处理。
    fn skip_line_comment(&mut self) {
        while matches!(self.peek(), Some(ch) if ch != '\n') {
            self.advance();
        }
    }

    /// 跳过可嵌套的多行注释。
    ///
    /// `#* ... *#` 与 `/* ... */` 可以各自嵌套，也可以相互嵌套；
    /// 结束符必须与栈顶的起始定界符匹配。
    fn skip_block_comment(&mut self, style: CommentStyle) {
        let start = self.position();
        self.consume_two();

        let mut stack = vec![style];
        while let Some(current) = stack.last().copied() {
            if self.starts_with("#*") {
                self.consume_two();
                stack.push(CommentStyle::Hash);
            } else if self.starts_with("/*") {
                self.consume_two();
                stack.push(CommentStyle::C);
            } else if (current == CommentStyle::Hash && self.starts_with("*#"))
                || (current == CommentStyle::C && self.starts_with("*/"))
            {
                self.consume_two();
                stack.pop();
            } else if self.peek().is_some() {
                self.advance();
            } else {
                let expected = match current {
                    CommentStyle::Hash => "`*#`",
                    CommentStyle::C => "`*/`",
                };
                self.diagnostics.push(
                    Diagnostic::error("多行注释未闭合")
                        .with_span(Span::new(start, self.position()))
                        .with_expected(expected)
                        .with_actual("文件结尾")
                        .with_note("多行注释允许嵌套"),
                );
                return;
            }
        }
    }

    /// 读取下一个 Token；无法恢复的错误已经写入诊断集合。
    fn next_token(&mut self, start: Position) -> Option<TokenKind> {
        let ch = self.peek()?;

        if ch == '「' {
            return self.lex_string(start);
        }
        if matches!(ch, '"' | '\'') {
            self.advance();
            self.diagnostics.push(
                Diagnostic::error("字符串必须使用中文书名号「」作为定界符")
                    .with_span(Span::new(start, self.position()))
                    .with_expected("`「...」`")
                    .with_actual(format!("`{ch}`")),
            );
            return None;
        }
        if is_identifier_start(ch) {
            return Some(self.lex_identifier());
        }
        if ch.is_ascii_digit() {
            return self.lex_number(start);
        }

        self.lex_symbol(start)
    }

    /// 读取标识符或关键字。
    fn lex_identifier(&mut self) -> TokenKind {
        let begin = self.offset;
        self.advance();

        while matches!(self.peek(), Some(ch) if is_identifier_continue(ch)) {
            self.advance();
        }

        let text = &self.source[begin..self.offset];
        keyword_kind(text).unwrap_or_else(|| TokenKind::Identifier(text.to_owned()))
    }

    /// 读取整数字面量或小数字面量。
    fn lex_number(&mut self, start: Position) -> Option<TokenKind> {
        let begin = self.offset;

        while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
            self.advance();
        }

        // 只有“点号后面紧跟数字”才视为小数，避免把字段访问的点号吞掉。
        let is_float = self.peek() == Some('.')
            && matches!(self.peek_second(), Some(ch) if ch.is_ascii_digit());

        if is_float {
            self.advance();
            while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
                self.advance();
            }
        }

        let text = &self.source[begin..self.offset];
        if is_float {
            match text.parse::<f64>() {
                Ok(value) if value.is_finite() => Some(TokenKind::Float(value)),
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error("小数字面量超出范围")
                            .with_span(Span::new(start, self.position()))
                            .with_expected("有限范围内的小数")
                            .with_actual(format!("`{}`", text)),
                    );
                    None
                }
            }
        } else {
            match text.parse::<i64>() {
                Ok(value) => Some(TokenKind::Integer(value)),
                Err(_) => {
                    self.diagnostics.push(
                        Diagnostic::error("整数字面量超出范围")
                            .with_span(Span::new(start, self.position()))
                            .with_expected("64 位有符号整数范围内的值")
                            .with_actual(format!("`{}`", text)),
                    );
                    None
                }
            }
        }
    }

    /// 读取以 `「」` 定界的字符串字面量。
    ///
    /// 支持 `\n`、`\r`、`\t`、`\\`、`\「` 与 `\」` 转义。
    /// 字符串不能直接跨行，需要换行时请使用 `\n`。
    fn lex_string(&mut self, start: Position) -> Option<TokenKind> {
        self.advance(); // 消费开头的 `「`
        let mut value = String::new();

        loop {
            let Some(ch) = self.peek() else {
                self.report_unclosed_string(start, "文件结尾");
                return None;
            };

            match ch {
                '」' => {
                    self.advance();
                    return Some(TokenKind::String(value));
                }
                '\n' | '\r' => {
                    self.report_unclosed_string(start, "换行");
                    return None;
                }
                '\\' => {
                    let escape_start = self.position();
                    self.advance();

                    let Some(escaped) = self.peek() else {
                        self.report_unclosed_string(start, "文件结尾");
                        return None;
                    };

                    if matches!(escaped, '\n' | '\r') {
                        self.report_unclosed_string(start, "换行");
                        return None;
                    }

                    let decoded = match escaped {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        '\\' => '\\',
                        '「' => '「',
                        '」' => '」',
                        'd' => 'd',
                        'D' => 'D',
                        'w' => 'w',
                        'W' => 'W',
                        '.' => '.',
                        '*' => '*',
                        '+' => '+',
                        '?' => '?',
                        's' => 's',
                        'S' => 'S',
                        '/' => '/',
                        '(' => '(',
                        ')' => ')',
                        '[' => '[',
                        ']' => ']',
                        '{' => '{',
                        '}' => '}',
                        '$' => '$',
                        '^' => '^',
                        '|' => '|',
                        '"' => '"',
                        '\'' => '\'',
                        'b' => '\u{0008}',
                        'f' => '\u{000C}',
                        '0' => '\0',
                        _ => {
                            self.advance();
                            self.diagnostics.push(
                                Diagnostic::error("无效的转义序列")
                                    .with_span(Span::new(escape_start, self.position()))
                                    .with_expected("`\\n`、`\\r`、`\\t`、`\\\\`、`\\「` 或 `\\」`")
                                    .with_actual(format!("`\\{}`", escaped)),
                            );
                            continue;
                        }
                    };

                    self.advance();
                    value.push(decoded);
                }
                _ => {
                    value.push(ch);
                    self.advance();
                }
            }
        }
    }

    /// 记录字符串未闭合的诊断。
    fn report_unclosed_string(&mut self, start: Position, actual: &'static str) {
        self.diagnostics.push(
            Diagnostic::error("字符串未闭合")
                .with_span(Span::new(start, self.position()))
                .with_expected("`」`")
                .with_actual(actual)
                .with_note("字符串字面量不能直接跨行，换行请使用 `\\n`"),
        );
    }

    /// 读取标点、运算符或箭头。
    fn lex_symbol(&mut self, start: Position) -> Option<TokenKind> {
        let current = self.peek()?;

        // Unicode 箭头直接对应箭头 Token。
        match current {
            '→' => {
                self.advance();
                return Some(TokenKind::Arrow);
            }
            '⇒' => {
                self.advance();
                return Some(TokenKind::FatArrow);
            }
            _ => {}
        }

        let normalized = normalize_symbol(current);

        match normalized {
            '=' => {
                let next = self.peek_second_normalized();
                if next == Some('=') {
                    self.consume_two();
                    Some(TokenKind::EqualEqual)
                } else if next == Some('>') {
                    self.consume_two();
                    Some(TokenKind::FatArrow)
                } else {
                    self.advance();
                    Some(TokenKind::Assign)
                }
            }
            '-' => {
                if self.peek_second_normalized() == Some('>') {
                    self.consume_two();
                    Some(TokenKind::Arrow)
                } else {
                    self.advance();
                    Some(TokenKind::Minus)
                }
            }
            ':' => {
                if self.peek_second_normalized() == Some(':') {
                    self.consume_two();
                    Some(TokenKind::ColonColon)
                } else {
                    self.advance();
                    Some(TokenKind::Colon)
                }
            }
            '&' => {
                if self.peek_second_normalized() == Some('&') {
                    self.consume_two();
                    Some(TokenKind::AndAnd)
                } else {
                    self.advance();
                    self.report_invalid_operator(start, current, "`&&` 或 `且`");
                    None
                }
            }
            '|' => {
                if self.peek_second_normalized() == Some('|') {
                    self.consume_two();
                    Some(TokenKind::OrOr)
                } else {
                    self.advance();
                    self.report_invalid_operator(start, current, "`||` 或 `或`");
                    None
                }
            }
            _ => {
                self.advance();
                let kind = match normalized {
                    '(' => TokenKind::LParen,
                    ')' => TokenKind::RParen,
                    '{' => TokenKind::LBrace,
                    '}' => TokenKind::RBrace,
                    '[' => TokenKind::LBracket,
                    ']' => TokenKind::RBracket,
                    '+' => TokenKind::Plus,
                    '*' => TokenKind::Star,
                    '/' => TokenKind::Slash,
                    '<' => TokenKind::Less,
                    '>' => TokenKind::Greater,
                    '!' => TokenKind::Not,
                    ',' => TokenKind::Comma,
                    ';' => TokenKind::Semicolon,
                    '.' => TokenKind::Dot,
                    ':' => TokenKind::Colon,
                    '@' => TokenKind::At,
                    _ => {
                        self.report_illegal_character(start, current);
                        return None;
                    }
                };
                Some(kind)
            }
        }
    }

    /// 记录非法运算符的诊断。
    fn report_invalid_operator(&mut self, start: Position, actual: char, expected: &'static str) {
        self.diagnostics.push(
            Diagnostic::error("非法运算符")
                .with_span(Span::new(start, self.position()))
                .with_expected(expected)
                .with_actual(format!("`{}`", actual)),
        );
    }

    /// 记录非法字符的诊断。
    fn report_illegal_character(&mut self, start: Position, actual: char) {
        self.diagnostics.push(
            Diagnostic::error("非法字符")
                .with_span(Span::new(start, self.position()))
                .with_expected("有效的 li 源码字符")
                .with_actual(format!("`{}`", actual)),
        );
    }

    /// 当前字符，不消费。
    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    /// 当前字符之后的第二个字符，不消费。
    fn peek_second(&self) -> Option<char> {
        self.remaining().chars().nth(1)
    }

    /// 第二个字符经过全角标点归一化后的结果。
    fn peek_second_normalized(&self) -> Option<char> {
        self.peek_second().map(normalize_symbol)
    }

    /// 是否以指定文本开头。
    fn starts_with(&self, prefix: &str) -> bool {
        self.remaining().starts_with(prefix)
    }

    /// 尚未消费的源码。
    fn remaining(&self) -> &str {
        &self.source[self.offset..]
    }

    /// 当前源码位置。
    fn position(&self) -> Position {
        Position::new(self.line, self.column, self.offset)
    }

    /// 消费两个字符，调用方必须保证前面至少还有两个字符。
    fn consume_two(&mut self) {
        self.advance();
        self.advance();
    }

    /// 消费一个字符并更新行列位置。
    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();

        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }

        Some(ch)
    }
}

/// 对整份源码执行词法分析。
///
/// 这是 [`Lexer::tokenize`] 的便捷函数，适合 CLI 与测试直接调用。
pub fn lex(source: &str) -> Result<Vec<Token>, DiagnosticBag> {
    Lexer::new(source).tokenize()
}

/// 多行注释的起始定界符风格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommentStyle {
    /// `#* ... *#`
    Hash,
    /// `/* ... */`
    C,
}

/// 判断字符是否可以作为标识符的首字符。
///
/// `char::is_alphabetic` 覆盖汉字与拉丁字母；下划线也可作为首字符。
fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

/// 判断字符是否可以作为标识符的后续字符。
fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

/// 将常见全角标点归一化为对应的半角字符。
fn normalize_symbol(ch: char) -> char {
    match ch {
        '＝' => '=',
        '（' => '(',
        '）' => ')',
        '｛' => '{',
        '｝' => '}',
        '［' => '[',
        '］' => ']',
        '＋' => '+',
        '－' | '−' => '-',
        '＊' => '*',
        '／' => '/',
        '＜' => '<',
        '＞' => '>',
        '！' => '!',
        '＆' => '&',
        '｜' => '|',
        '，' => ',',
        '；' => ';',
        '：' => ':',
        '．' => '.',
        _ => ch,
    }
}

/// 将标识符文本映射为关键字或中文运算符 Token。
///
/// `则` 虽然不在最初的关键字清单中，但条件语法 `如果 ... 则 ...`
/// 需要它，因此这里一并作为关键字处理。
fn keyword_kind(text: &str) -> Option<TokenKind> {
    let kind = match text {
        "让" | "let" => TokenKind::Let,
        "可变" | "mut" => TokenKind::Mut,
        "函数" | "fn" | "func" | "function" => TokenKind::Func,
        "返回" | "return" => TokenKind::Return,
        "如果" | "if" => TokenKind::If,
        "则" | "then" => TokenKind::Then,
        "否则" | "else" => TokenKind::Else,
        "当" | "while" => TokenKind::While,
        "跳出" | "break" => TokenKind::Break,
        "继续" | "continue" => TokenKind::Continue,
        "匹配" | "match" => TokenKind::Match,
        "成功" | "ok" | "success" => TokenKind::Success,
        "失败" | "err" | "error" | "failure" => TokenKind::Failure,
        "模块" | "module" => TokenKind::Module,
        "导入" | "import" => TokenKind::Import,
        "真" | "true" => TokenKind::True,
        "假" | "false" => TokenKind::False,
        "空" | "null" | "nil" => TokenKind::Null,
        "类型" | "type" => TokenKind::Type,
        "打印" | "print" => TokenKind::Print,
        "输入" | "input" => TokenKind::Input,
        "视图" | "view" => TokenKind::View,
        "不安全" | "unsafe" => TokenKind::Unsafe,
        "异步" | "async" => TokenKind::Async,
        "等待" | "await" => TokenKind::Await,

        "整数" | "int" | "integer" => TokenKind::IntegerType,
        "小数" | "float" => TokenKind::FloatType,
        "布尔" | "bool" | "boolean" => TokenKind::BoolType,
        "字符串" | "string" | "str" => TokenKind::StringType,
        "列表" | "list" => TokenKind::ListType,
        "字典" | "dictionary" | "map" => TokenKind::DictionaryType,
        "可选" | "option" | "optional" => TokenKind::OptionalType,
        "结果" | "result" => TokenKind::ResultType,
        "原生指针" | "rawptr" | "raw_pointer" | "ptr" => TokenKind::RawPointerType,
        "任务" | "task" => TokenKind::TaskType,
        "通道" | "channel" => TokenKind::ChannelType,
        "共享" | "shared" => TokenKind::SharedType,
        "借用" | "borrow" => TokenKind::BorrowType,

        "加" => TokenKind::Plus,
        "减" => TokenKind::Minus,
        "乘" => TokenKind::Star,
        "除" => TokenKind::Slash,
        "等于" => TokenKind::EqualEqual,
        "小于" => TokenKind::Less,
        "大于" => TokenKind::Greater,
        "且" => TokenKind::AndAnd,
        "或" => TokenKind::OrOr,
        "非" => TokenKind::Not,

        _ => return None,
    };

    Some(kind)
}
