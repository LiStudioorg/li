//! HTML 文本与属性值转义。

/// 转义 HTML 文本内容。
///
/// 文本中出现的 `&`、`<`、`>`、`"` 与 `'` 都会转换为实体，
/// 因此即使字符串来自用户输入，也不会破坏文档结构。
pub(crate) fn text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(ch),
        }
    }

    output
}

/// 转义双引号包围的 HTML 属性值。
pub(crate) fn attribute(value: &str) -> String {
    text(value)
}
