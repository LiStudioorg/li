//! JavaScript 字面量转义。

/// 把字符串转换为带双引号的 JavaScript 字符串字面量。
pub(crate) fn string_literal(value: &str) -> String {
    let mut output = String::from("\"");

    for ch in value.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\u{2028}' => output.push_str("\\u2028"),
            '\u{2029}' => output.push_str("\\u2029"),
            ch if ch < '\u{20}' => output.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => output.push(ch),
        }
    }

    output.push('"');
    output
}

/// 把小数字面量转换为 JavaScript 数字字面量。
pub(crate) fn float_literal(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }

    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity".to_owned()
        } else {
            "Infinity".to_owned()
        };
    }

    let mut text = format!("{value:?}");

    // Rust 会把 1.0 格式化为 `1`，这里补上小数点，明确保持小数类型。
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        text.push_str(".0");
    }

    text
}
