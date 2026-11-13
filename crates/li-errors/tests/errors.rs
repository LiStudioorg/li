//! li-errors 的公共 API 集成测试。
//!
//! 这些测试验证错误信息的最终中文格式、行列定位规则，
//! 以及诊断集合的基本行为。

use li_errors::{Diagnostic, DiagnosticBag, Position, Severity, SourceFile, Span};

#[test]
fn type_mismatch_has_the_expected_chinese_format() {
    // 严格对齐语言规范中的示例格式。
    let span = Span::point(Position::new(3, 5, 20));
    let diagnostic = Diagnostic::type_mismatch(span, "整数", "字符串");

    assert_eq!(
        diagnostic.to_string(),
        "第 3 行第 5 列：类型不匹配，期望 整数，实际 字符串"
    );
    assert_eq!(diagnostic.severity(), Severity::Error);
    assert_eq!(diagnostic.expected(), Some("整数"));
    assert_eq!(diagnostic.actual(), Some("字符串"));
}

#[test]
fn source_file_counts_columns_by_unicode_characters() {
    let source = SourceFile::new("示例.li", "让 甲 = 「你好」;\r\n让 乙 = 2;\n");

    // 行尾的 `\r\n` 不应出现在行文本中。
    assert_eq!(source.line(1), Some("让 甲 = 「你好」;"));
    assert_eq!(source.line(2), Some("让 乙 = 2;"));
    assert_eq!(source.line(3), Some(""));
    assert_eq!(source.line_count(), 3);

    // 中文字符按一列计算：第 2 行的“乙”位于第 3 列。
    let offset = source.text().find('乙').unwrap();
    let position = source.position_at(offset);
    assert_eq!(position, Position::new(2, 3, offset));

    // 文件末尾换行之后仍存在一个空行。
    assert_eq!(
        source.position_at(source.text().len()),
        Position::new(3, 1, source.text().len())
    );

    // 区间换算同样使用字符列号。
    let string_start = source.text().find('你').unwrap();
    let span = source.span(string_start, string_start + '你'.len_utf8());
    assert_eq!(span.start.line, 1);
    assert_eq!(span.start.column, 8);
}

#[test]
fn diagnostic_bag_reports_errors_and_renders_in_order() {
    let mut bag = DiagnosticBag::new();
    assert!(!bag.has_errors());

    bag.push(
        Diagnostic::error("缺少右括号")
            .with_span(Span::new(Position::new(1, 5, 4), Position::new(1, 8, 7))),
    );
    bag.push(Diagnostic::warning("变量 `x` 未被使用"));

    assert_eq!(bag.len(), 2);
    assert_eq!(bag.error_count(), 1);
    assert!(bag.has_errors());

    let rendered = bag.to_string();
    assert!(rendered.contains("第 1 行第 5 列：缺少右括号"));
    assert!(rendered.contains("警告：变量 `x` 未被使用"));
}

#[test]
fn warning_with_a_span_keeps_its_level() {
    let diagnostic =
        Diagnostic::warning("未使用的变量 `x`").with_span(Span::point(Position::new(2, 3, 10)));

    assert_eq!(
        diagnostic.to_string(),
        "第 2 行第 3 列：警告：未使用的变量 `x`"
    );
}

#[test]
fn spans_can_be_merged_and_measured() {
    let first = Span::new(Position::new(1, 1, 0), Position::new(1, 3, 2));
    let second = Span::new(Position::new(1, 5, 4), Position::new(1, 7, 6));
    let merged = first.merge(second);

    assert_eq!(merged.start, Position::new(1, 1, 0));
    assert_eq!(merged.end, Position::new(1, 7, 6));
    assert_eq!(merged.byte_len(), 6);
    assert!(!merged.is_empty());
}
