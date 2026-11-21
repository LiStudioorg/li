//! li-lexer 的详细集成测试。
//!
//! 覆盖中英文关键字与标识符、全角/半角标点、运算符、
//! 数字、字符串、嵌套注释以及错误的行列定位。

use li_errors::Position;
use li_lexer::{lex, Token, TokenKind};

/// 返回去掉文件结束标记后的 Token 种类。
fn kinds(source: &str) -> Vec<TokenKind> {
    tokens(source)
        .into_iter()
        .filter(|token| !token.is_eof())
        .map(|token| token.kind)
        .collect()
}

/// 返回完整 Token 序列，断言词法分析成功。
fn tokens(source: &str) -> Vec<Token> {
    lex(source).unwrap_or_else(|errors| panic!("词法分析失败：{}", errors))
}

/// 构造标识符 Token 的期望值。
fn ident(name: &str) -> TokenKind {
    TokenKind::Identifier(name.to_owned())
}

#[test]
fn recognizes_all_chinese_keywords() {
    let cases = [
        ("让", TokenKind::Let),
        ("可变", TokenKind::Mut),
        ("函数", TokenKind::Func),
        ("返回", TokenKind::Return),
        ("如果", TokenKind::If),
        ("则", TokenKind::Then),
        ("否则", TokenKind::Else),
        ("当", TokenKind::While),
        ("跳出", TokenKind::Break),
        ("继续", TokenKind::Continue),
        ("匹配", TokenKind::Match),
        ("成功", TokenKind::Success),
        ("失败", TokenKind::Failure),
        ("模块", TokenKind::Module),
        ("导入", TokenKind::Import),
        ("真", TokenKind::True),
        ("假", TokenKind::False),
        ("空", TokenKind::Null),
        ("类型", TokenKind::Type),
        ("打印", TokenKind::Print),
        ("输入", TokenKind::Input),
        ("视图", TokenKind::View),
        ("整数", TokenKind::IntegerType),
        ("小数", TokenKind::FloatType),
        ("字符串", TokenKind::StringType),
        ("布尔", TokenKind::BoolType),
        ("列表", TokenKind::ListType),
        ("可选", TokenKind::OptionalType),
        ("结果", TokenKind::ResultType),
    ];

    for (source, expected) in cases {
        assert_eq!(kinds(source), vec![expected], "中文关键字 `{source}`");
    }
}

#[test]
fn recognizes_all_english_keyword_aliases() {
    let cases = [
        ("let", TokenKind::Let),
        ("mut", TokenKind::Mut),
        ("fn", TokenKind::Func),
        ("func", TokenKind::Func),
        ("function", TokenKind::Func),
        ("return", TokenKind::Return),
        ("if", TokenKind::If),
        ("then", TokenKind::Then),
        ("else", TokenKind::Else),
        ("while", TokenKind::While),
        ("break", TokenKind::Break),
        ("continue", TokenKind::Continue),
        ("match", TokenKind::Match),
        ("ok", TokenKind::Success),
        ("success", TokenKind::Success),
        ("err", TokenKind::Failure),
        ("error", TokenKind::Failure),
        ("failure", TokenKind::Failure),
        ("module", TokenKind::Module),
        ("import", TokenKind::Import),
        ("true", TokenKind::True),
        ("false", TokenKind::False),
        ("null", TokenKind::Null),
        ("nil", TokenKind::Null),
        ("type", TokenKind::Type),
        ("print", TokenKind::Print),
        ("input", TokenKind::Input),
        ("view", TokenKind::View),
        ("int", TokenKind::IntegerType),
        ("integer", TokenKind::IntegerType),
        ("float", TokenKind::FloatType),
        ("bool", TokenKind::BoolType),
        ("boolean", TokenKind::BoolType),
        ("string", TokenKind::StringType),
        ("str", TokenKind::StringType),
        ("list", TokenKind::ListType),
        ("option", TokenKind::OptionalType),
        ("optional", TokenKind::OptionalType),
        ("result", TokenKind::ResultType),
    ];

    for (source, expected) in cases {
        assert_eq!(kinds(source), vec![expected], "英文关键字 `{source}`");
    }
}

#[test]
fn lexes_mixed_chinese_and_english_code() {
    let source = "让 甲 = 1; let 乙 = 2; \
                  函数 加一(值: 整数) -> 整数 { 返回 值 加 1; } \
                  fn add(x: int) -> int { return x + 1; }";

    let expected = vec![
        TokenKind::Let,
        ident("甲"),
        TokenKind::Assign,
        TokenKind::Integer(1),
        TokenKind::Semicolon,
        TokenKind::Let,
        ident("乙"),
        TokenKind::Assign,
        TokenKind::Integer(2),
        TokenKind::Semicolon,
        TokenKind::Func,
        ident("加一"),
        TokenKind::LParen,
        ident("值"),
        TokenKind::Colon,
        TokenKind::IntegerType,
        TokenKind::RParen,
        TokenKind::Arrow,
        TokenKind::IntegerType,
        TokenKind::LBrace,
        TokenKind::Return,
        ident("值"),
        TokenKind::Plus,
        TokenKind::Integer(1),
        TokenKind::Semicolon,
        TokenKind::RBrace,
        TokenKind::Func,
        ident("add"),
        TokenKind::LParen,
        ident("x"),
        TokenKind::Colon,
        TokenKind::IntegerType,
        TokenKind::RParen,
        TokenKind::Arrow,
        TokenKind::IntegerType,
        TokenKind::LBrace,
        TokenKind::Return,
        ident("x"),
        TokenKind::Plus,
        TokenKind::Integer(1),
        TokenKind::Semicolon,
        TokenKind::RBrace,
    ];

    assert_eq!(kinds(source), expected);
}

#[test]
fn normalizes_fullwidth_and_halfwidth_symbols() {
    let fullwidth = "让 x＝（1＋2）＊3；如果（x 大于 0）则｛打印（x）；｝否则｛打印（0）；｝";
    let halfwidth = "让 x = (1 + 2) * 3; 如果 (x 大于 0) 则 { 打印 (x); } 否则 { 打印 (0); }";
    assert_eq!(kinds(fullwidth), kinds(halfwidth));

    assert_eq!(kinds("！真 ＆＆ 假 ｜｜ 真"), kinds("!真 && 假 || 真"));
    assert_eq!(kinds("（甲）－＞乙"), kinds("(甲) -> 乙"));
    assert_eq!(kinds("甲 → 乙"), kinds("甲 -> 乙"));
    assert_eq!(kinds("甲 ⇒ 乙"), kinds("甲 => 乙"));
    assert_eq!(kinds("甲 ＝＞ 乙"), kinds("甲 => 乙"));
    assert_eq!(kinds("甲 ＝＝ 乙"), kinds("甲 == 乙"));
    assert_eq!(kinds("模块：：工具"), kinds("模块::工具"));
    assert_eq!(
        kinds("［甲，乙］；对象．字段"),
        kinds("[甲, 乙]; 对象.字段")
    );
    assert_eq!(kinds("6／2 减 1＊3"), kinds("6/2 减 1*3"));
}

#[test]
fn skips_single_line_and_nested_multi_line_comments() {
    let source = "# 单行注释\n\
                  // 英文单行注释\n\
                  让 甲 = 1; # 行尾注释\n\
                  #* 外层\n\
                     #* 内层 *#\n\
                     仍在外层\n\
                  *#\n\
                  /* C 外层\n\
                     /* C 内层 */\n\
                     C 外层结束 */\n\
                  #* 混合 /* C 内层 */ 仍然安全 *#\n\
                  让 乙 = 2;";

    assert_eq!(
        kinds(source),
        vec![
            TokenKind::Let,
            ident("甲"),
            TokenKind::Assign,
            TokenKind::Integer(1),
            TokenKind::Semicolon,
            TokenKind::Let,
            ident("乙"),
            TokenKind::Assign,
            TokenKind::Integer(2),
            TokenKind::Semicolon,
        ]
    );
}

#[test]
fn reports_unclosed_nested_hash_comment() {
    let errors = lex("让 x = 1;\n#* 未闭合\n#* 内层 *#").unwrap_err();
    assert_eq!(errors.error_count(), 1);

    let message = errors.to_string();
    assert!(message.contains("第 2 行第 1 列"), "实际诊断：{message}");
    assert!(message.contains("多行注释未闭合"), "实际诊断：{message}");
    assert!(message.contains("期望 `*#`"), "实际诊断：{message}");
    assert!(message.contains("实际 文件结尾"), "实际诊断：{message}");
}

#[test]
fn reports_unclosed_c_style_comment() {
    let message = lex("/* 未闭合").unwrap_err().to_string();

    assert!(message.contains("第 1 行第 1 列"), "实际诊断：{message}");
    assert!(message.contains("多行注释未闭合"), "实际诊断：{message}");
    assert!(message.contains("期望 `*/`"), "实际诊断：{message}");
}

#[test]
fn accepts_chinese_identifiers_and_keyword_prefixes() {
    let source = "让 加一 = 如果状态; 让 _内部变量2 = 温度; 让 count_2 = 加一;";

    assert_eq!(
        kinds(source),
        vec![
            TokenKind::Let,
            ident("加一"),
            TokenKind::Assign,
            ident("如果状态"),
            TokenKind::Semicolon,
            TokenKind::Let,
            ident("_内部变量2"),
            TokenKind::Assign,
            ident("温度"),
            TokenKind::Semicolon,
            TokenKind::Let,
            ident("count_2"),
            TokenKind::Assign,
            ident("加一"),
            TokenKind::Semicolon,
        ]
    );
}

#[test]
fn lexes_strings_and_escape_sequences() {
    let source =
        r#"「你好，世界」 「」 「第一行\n第二行\t完成」 「他说：\「你好\」」 「反斜杠\\」"#;

    assert_eq!(
        kinds(source),
        vec![
            TokenKind::String("你好，世界".to_owned()),
            TokenKind::String(String::new()),
            TokenKind::String("第一行\n第二行\t完成".to_owned()),
            TokenKind::String("他说：「你好」".to_owned()),
            TokenKind::String("反斜杠\\".to_owned()),
        ]
    );
}

#[test]
fn treats_comment_markers_inside_strings_as_plain_text() {
    let source = "「# 不是注释」 「// 不是注释」 「/* 不是注释 */」 「#* 不是注释 *#」";

    assert_eq!(
        kinds(source),
        vec![
            TokenKind::String("# 不是注释".to_owned()),
            TokenKind::String("// 不是注释".to_owned()),
            TokenKind::String("/* 不是注释 */".to_owned()),
            TokenKind::String("#* 不是注释 *#".to_owned()),
        ]
    );
}

#[test]
fn reports_unclosed_string_at_eof() {
    let message = lex("让 文本 = 「未闭合").unwrap_err().to_string();

    assert!(message.contains("第 1 行第 8 列"), "实际诊断：{message}");
    assert!(message.contains("字符串未闭合"), "实际诊断：{message}");
    assert!(message.contains("期望 `」`"), "实际诊断：{message}");
    assert!(message.contains("实际 文件结尾"), "实际诊断：{message}");
}

#[test]
fn reports_string_broken_by_a_newline() {
    let message = lex("「第一行\n第二行」").unwrap_err().to_string();

    assert!(message.contains("第 1 行第 1 列"), "实际诊断：{message}");
    assert!(message.contains("字符串未闭合"), "实际诊断：{message}");
    assert!(message.contains("实际 换行"), "实际诊断：{message}");
}

#[test]
fn rejects_unknown_escape_sequences() {
    let message = lex(r#"「错误\q转义」"#).unwrap_err().to_string();

    assert!(message.contains("无效的转义序列"), "实际诊断：{message}");
    assert!(message.contains(r"实际 `\q`"), "实际诊断：{message}");
}

#[test]
fn rejects_ascii_quote_string_delimiters() {
    let message = lex("\"你好\"").unwrap_err().to_string();

    assert!(message.contains("第 1 行第 1 列"), "实际诊断：{message}");
    assert!(
        message.contains("字符串必须使用中文书名号「」作为定界符"),
        "实际诊断：{message}"
    );
    assert!(message.contains("期望 `「...」`"), "实际诊断：{message}");
    assert!(message.contains("实际 `\"`"), "实际诊断：{message}");
}

#[test]
fn lexes_integer_and_float_literals() {
    assert_eq!(
        kinds("0 42 3.25 100.0 007"),
        vec![
            TokenKind::Integer(0),
            TokenKind::Integer(42),
            TokenKind::Float(3.25),
            TokenKind::Float(100.0),
            TokenKind::Integer(7),
        ]
    );

    // 点号后没有数字时不能被吞进数字字面量。
    assert_eq!(
        kinds("1.名字"),
        vec![TokenKind::Integer(1), TokenKind::Dot, ident("名字")]
    );
}

#[test]
fn reports_integer_overflow() {
    let message = lex("9223372036854775808").unwrap_err().to_string();

    assert!(message.contains("第 1 行第 1 列"), "实际诊断：{message}");
    assert!(
        message.contains("整数字面量超出范围"),
        "实际诊断：{message}"
    );
    assert!(
        message.contains("实际 `9223372036854775808`"),
        "实际诊断：{message}"
    );
}

#[test]
fn normalizes_chinese_and_symbolic_operators() {
    let cases = [
        ("甲 加 乙", TokenKind::Plus),
        ("甲 + 乙", TokenKind::Plus),
        ("甲 减 乙", TokenKind::Minus),
        ("甲 - 乙", TokenKind::Minus),
        ("甲 乘 乙", TokenKind::Star),
        ("甲 * 乙", TokenKind::Star),
        ("甲 除 乙", TokenKind::Slash),
        ("甲 / 乙", TokenKind::Slash),
        ("甲 等于 乙", TokenKind::EqualEqual),
        ("甲 == 乙", TokenKind::EqualEqual),
        ("甲 小于 乙", TokenKind::Less),
        ("甲 < 乙", TokenKind::Less),
        ("甲 大于 乙", TokenKind::Greater),
        ("甲 > 乙", TokenKind::Greater),
        ("甲 且 乙", TokenKind::AndAnd),
        ("甲 && 乙", TokenKind::AndAnd),
        ("甲 或 乙", TokenKind::OrOr),
        ("甲 || 乙", TokenKind::OrOr),
    ];

    for (source, operator) in cases {
        assert_eq!(
            kinds(source),
            vec![ident("甲"), operator, ident("乙")],
            "运算符表达式 `{source}`"
        );
    }

    assert_eq!(kinds("非 真"), vec![TokenKind::Not, TokenKind::True]);
    assert_eq!(kinds("!假"), vec![TokenKind::Not, TokenKind::False]);
}

#[test]
fn rejects_single_ampersand_and_pipe_operators() {
    for source in ["甲 & 乙", "甲 | 乙"] {
        let message = lex(source).unwrap_err().to_string();
        assert!(message.contains("非法运算符"), "实际诊断：{message}");
    }
}

#[test]
fn lexes_all_punctuation_tokens() {
    assert_eq!(
        kinds("(){}[],;:. -> => :: =="),
        vec![
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::LBracket,
            TokenKind::RBracket,
            TokenKind::Comma,
            TokenKind::Semicolon,
            TokenKind::Colon,
            TokenKind::Dot,
            TokenKind::Arrow,
            TokenKind::FatArrow,
            TokenKind::ColonColon,
            TokenKind::EqualEqual,
        ]
    );
}

#[test]
fn tracks_unicode_columns_and_crlf_line_endings() {
    let source = "让 甲 = 1;\r\n让 温度 = 「凉」;";
    let all_tokens = tokens(source);

    let temperature = all_tokens
        .iter()
        .find(|token| matches!(&token.kind, TokenKind::Identifier(name) if name == "温度"))
        .expect("应找到标识符“温度”");
    let offset = source.find("温度").unwrap();
    assert_eq!(temperature.span.start, Position::new(2, 3, offset));
    assert_eq!(
        temperature.span.end,
        Position::new(2, 5, offset + "温度".len())
    );

    let first = &all_tokens[0];
    assert_eq!(first.span.start, Position::new(1, 1, 0));
    assert_eq!(first.span.end, Position::new(1, 2, "让".len()));

    let string = all_tokens
        .iter()
        .find(|token| matches!(token.kind, TokenKind::String(_)))
        .expect("应找到字符串字面量");
    assert_eq!(
        string.span.start,
        Position::new(2, 8, source.find('「').unwrap())
    );
    assert_eq!(
        string.span.end,
        Position::new(2, 11, source.find('「').unwrap() + "「凉」".len())
    );
}

#[test]
fn reports_illegal_character_with_exact_location() {
    let message = lex("让 x = $;").unwrap_err().to_string();

    assert!(message.contains("第 1 行第 7 列"), "实际诊断：{message}");
    assert!(message.contains("非法字符"), "实际诊断：{message}");
    assert!(message.contains("实际 `$`"), "实际诊断：{message}");
}

#[test]
fn skips_unicode_whitespace_and_accepts_empty_source() {
    assert_eq!(
        kinds("让\u{3000}甲\u{3000}＝\u{3000}1；"),
        vec![
            TokenKind::Let,
            ident("甲"),
            TokenKind::Assign,
            TokenKind::Integer(1),
            TokenKind::Semicolon,
        ]
    );

    let all_tokens = tokens("");
    assert_eq!(all_tokens.len(), 1);
    assert!(all_tokens[0].is_eof());
    assert_eq!(all_tokens[0].span.start, Position::new(1, 1, 0));
}

#[test]
fn reports_eof_position_after_a_trailing_newline() {
    let source = "让 x = 1;\n";
    let all_tokens = tokens(source);
    let eof = all_tokens.last().expect("应包含文件结束标记");

    assert!(eof.is_eof());
    assert_eq!(eof.span.start, Position::new(2, 1, source.len()));
}

#[test]
fn token_kind_display_uses_chinese_descriptions() {
    assert_eq!(TokenKind::Eof.to_string(), "文件结尾");
    assert_eq!(ident("温度").to_string(), "标识符 `温度`");
    assert_eq!(TokenKind::If.to_string(), "`如果`");
    assert_eq!(TokenKind::EqualEqual.to_string(), "`==`");
}

#[test]
fn lexes_unsafe_and_raw_pointer_keywords() {
    let all_tokens = tokens("不安全 原生指针<整数> 不安全 unsafe rawptr");

    let kinds = all_tokens
        .iter()
        .map(|token| token.kind.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Unsafe,
            TokenKind::RawPointerType,
            TokenKind::Less,
            TokenKind::IntegerType,
            TokenKind::Greater,
            TokenKind::Unsafe,
            TokenKind::Unsafe,
            TokenKind::RawPointerType,
            TokenKind::Eof,
        ]
    );
    assert_eq!(TokenKind::Unsafe.to_string(), "`不安全`");
    assert_eq!(TokenKind::RawPointerType.to_string(), "`原生指针`");
}

#[test]
fn reserves_concurrency_keywords() {
    assert_eq!(
        kinds("异步 等待 通道<整数> 共享<字符串> 借用<整数>"),
        vec![
            TokenKind::Async,
            TokenKind::Await,
            TokenKind::ChannelType,
            TokenKind::Less,
            TokenKind::IntegerType,
            TokenKind::Greater,
            TokenKind::SharedType,
            TokenKind::Less,
            TokenKind::StringType,
            TokenKind::Greater,
            TokenKind::BorrowType,
            TokenKind::Less,
            TokenKind::IntegerType,
            TokenKind::Greater,
        ]
    );

    assert_eq!(
        kinds("async await channel shared borrow"),
        vec![
            TokenKind::Async,
            TokenKind::Await,
            TokenKind::ChannelType,
            TokenKind::SharedType,
            TokenKind::BorrowType,
        ]
    );

    assert_eq!(TokenKind::Async.to_string(), "`异步`");
    assert_eq!(TokenKind::Await.to_string(), "`等待`");
    assert_eq!(TokenKind::ChannelType.to_string(), "`通道`");
    assert_eq!(TokenKind::SharedType.to_string(), "`共享`");
    assert_eq!(TokenKind::BorrowType.to_string(), "`借用`");
}

#[test]
fn recognizes_no_gc_directive_but_not_as_comment() {
    // `#无垃圾回收` 是文件头指令；普通 `#` 行注释仍应被跳过。
    assert_eq!(
        kinds("#无垃圾回收\n# 这是注释\n函数 主() -> 整数 { 返回 0; }")[0],
        TokenKind::NoGc
    );
    assert_eq!(TokenKind::NoGc.to_string(), "`#无垃圾回收`");
}
