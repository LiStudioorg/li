//! 兼容中文键名的 TOML 预处理。
//!
//! TOML 标准只允许 ASCII 裸键，而 li 的 `li.toml` 与 `li.lock` 需要
//! `[项目]`、`[依赖]`、`名称`、`路径` 这样的中文键。这里在交给 `toml`
//! 解析器之前，把非 ASCII 裸键改写为带引号的等价形式，同时记录输出字符
//! 到原始源码的偏移，保证后续诊断仍能指向真实的行列位置。

use li_errors::{DiagnosticBag, Position, Span};

/// 预处理后的 TOML 源码及其偏移映射。
pub(crate) struct QuotedToml {
    /// 可以直接交给 `toml` 解析的文本。
    pub text: String,
    /// 每个输出字符对应的原始字节偏移。
    origins: Vec<usize>,
}

impl QuotedToml {
    /// 把输出文本中的字节偏移映射回原始源码的字节偏移。
    pub fn original_offset(&self, output_offset: usize) -> usize {
        for (index, _) in self.text.char_indices() {
            if index >= output_offset {
                return self.origins[index];
            }
        }
        self.origins.last().copied().unwrap_or(0)
    }
}

/// 把源码中的非 ASCII 裸键改写为带引号的键。
pub(crate) fn quote_bare_keys(source: &str) -> QuotedToml {
    let chars: Vec<(char, usize)> = source
        .char_indices()
        .map(|(offset, ch)| (ch, offset))
        .collect();

    let mut builder = Builder::default();
    let mut start = 0;

    while start < chars.len() {
        let end = chars[start..]
            .iter()
            .position(|(ch, _)| *ch == '\n')
            .map(|offset| start + offset)
            .unwrap_or(chars.len());

        process_line(&chars[start..end], &mut builder);

        if end < chars.len() {
            builder.push_char('\n', chars[end].1);
        }
        start = end + 1;
    }

    QuotedToml {
        text: builder.text,
        origins: builder.origins,
    }
}

/// 输出构建器，同时维护每个字符的原始偏移。
#[derive(Default)]
struct Builder {
    text: String,
    origins: Vec<usize>,
}

impl Builder {
    fn push_char(&mut self, ch: char, origin: usize) {
        self.text.push(ch);
        self.origins.push(origin);
    }

    fn push(&mut self, value: (char, usize)) {
        self.push_char(value.0, value.1);
    }

    fn copy(&mut self, slice: &[(char, usize)]) {
        for &value in slice {
            self.push(value);
        }
    }
}

/// 处理一行：表头、键值对或普通内容。
fn process_line(line: &[(char, usize)], out: &mut Builder) {
    if line.is_empty() {
        return;
    }

    let comment = find_comment(line);
    let (content, comment_part) = line.split_at(comment);

    let first = content
        .iter()
        .position(|(ch, _)| !ch.is_whitespace())
        .unwrap_or(content.len());

    let trimmed = &content[first..];

    if trimmed.first().is_some_and(|(ch, _)| *ch == '[') {
        out.copy(&content[..first]);
        process_header(trimmed, out);
    } else if let Some(assignment) = find_assignment(trimmed) {
        out.copy(&content[..first]);
        let (key, rest) = trimmed.split_at(assignment);
        quote_key_path(key, out);
        out.push(rest[0]);
        process_value(&rest[1..], out);
    } else {
        out.copy(content);
    }

    out.copy(comment_part);
}

/// 找到字符串之外的注释起点。
fn find_comment(line: &[(char, usize)]) -> usize {
    let mut index = 0;
    let mut in_string = None;

    while index < line.len() {
        let ch = line[index].0;
        if let Some(quote) = in_string {
            if quote == '"' && ch == '\\' {
                index += 2;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
        } else if ch == '"' || ch == '\'' {
            in_string = Some(ch);
        } else if ch == '#' {
            return index;
        }
        index += 1;
    }

    line.len()
}

/// 找到字符串之外的第一个 `=`。
fn find_assignment(line: &[(char, usize)]) -> Option<usize> {
    let mut index = 0;
    let mut in_string = None;

    while index < line.len() {
        let ch = line[index].0;
        if let Some(quote) = in_string {
            if quote == '"' && ch == '\\' {
                index += 2;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
        } else if ch == '"' || ch == '\'' {
            in_string = Some(ch);
        } else if ch == '=' {
            return Some(index);
        }
        index += 1;
    }

    None
}

/// 处理表头 `[依赖.名称]` 或 `[[依赖]]`。
fn process_header(header: &[(char, usize)], out: &mut Builder) {
    let mut index = 0;

    while index < header.len() && header[index].0 == '[' {
        out.push(header[index]);
        index += 1;
    }

    let path_start = index;
    let mut in_string = None;

    while index < header.len() {
        let ch = header[index].0;
        if let Some(quote) = in_string {
            if quote == '"' && ch == '\\' {
                index += 2;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
        } else if ch == '"' || ch == '\'' {
            in_string = Some(ch);
        } else if ch == ']' {
            break;
        }
        index += 1;
    }

    quote_key_path(&header[path_start..index], out);

    while index < header.len() && header[index].0 == ']' {
        out.push(header[index]);
        index += 1;
    }

    out.copy(&header[index..]);
}

/// 处理键值对右侧的普通值与内联表。
fn process_value(value: &[(char, usize)], out: &mut Builder) {
    let mut index = 0;
    let mut in_string = None;
    let mut table_depth: usize = 0;
    let mut expect_key = false;

    while index < value.len() {
        let (ch, origin) = value[index];

        if let Some(quote) = in_string {
            out.push_char(ch, origin);
            if quote == '"' && ch == '\\' && index + 1 < value.len() {
                index += 1;
                out.push(value[index]);
                index += 1;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            index += 1;
            continue;
        }

        match ch {
            '"' | '\'' => {
                in_string = Some(ch);
                out.push_char(ch, origin);
                index += 1;
            }
            '{' => {
                table_depth += 1;
                expect_key = true;
                out.push_char(ch, origin);
                index += 1;
            }
            '}' => {
                table_depth = table_depth.saturating_sub(1);
                expect_key = false;
                out.push_char(ch, origin);
                index += 1;
            }
            ',' if table_depth > 0 => {
                expect_key = true;
                out.push_char(ch, origin);
                index += 1;
            }
            _ if expect_key && table_depth > 0 => {
                if ch.is_whitespace() {
                    out.push_char(ch, origin);
                    index += 1;
                    continue;
                }

                let key_start = index;
                let mut cursor = index;
                let mut key_string = None;
                let mut found = false;

                while cursor < value.len() {
                    let current = value[cursor].0;
                    if let Some(quote) = key_string {
                        if quote == '"' && current == '\\' {
                            cursor += 2;
                            continue;
                        }
                        if current == quote {
                            key_string = None;
                        }
                    } else if current == '"' || current == '\'' {
                        key_string = Some(current);
                    } else if current == '=' {
                        found = true;
                        break;
                    } else if current == ',' || current == '}' {
                        break;
                    }
                    cursor += 1;
                }

                if found {
                    quote_key_path(&value[key_start..cursor], out);
                    out.push_char('=', value[cursor].1);
                    expect_key = false;
                    index = cursor + 1;
                } else {
                    out.push_char(ch, origin);
                    index += 1;
                }
            }
            _ => {
                out.push_char(ch, origin);
                index += 1;
            }
        }
    }
}

/// 处理点分隔的键路径，逐段加引号。
fn quote_key_path(path: &[(char, usize)], out: &mut Builder) {
    let mut start = 0;
    let mut index = 0;
    let mut in_string = None;

    loop {
        let split = index == path.len() || (in_string.is_none() && path[index].0 == '.');

        if split {
            quote_key_segment(&path[start..index], out);
            if index < path.len() {
                out.push(path[index]);
            }
            start = index + 1;
            index += 1;

            if start > path.len() {
                break;
            }
            continue;
        }

        let ch = path[index].0;
        if let Some(quote) = in_string {
            if quote == '"' && ch == '\\' {
                index += 2;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
        } else if ch == '"' || ch == '\'' {
            in_string = Some(ch);
        }
        index += 1;
    }
}

/// 处理单个键片段：需要时补充双引号。
fn quote_key_segment(segment: &[(char, usize)], out: &mut Builder) {
    let mut start = 0;
    let mut end = segment.len();

    while start < end && segment[start].0.is_whitespace() {
        start += 1;
    }
    while end > start && segment[end - 1].0.is_whitespace() {
        end -= 1;
    }

    let segment = &segment[start..end];
    if segment.is_empty() {
        return;
    }

    if is_quoted(segment) {
        out.copy(segment);
        return;
    }

    if segment
        .iter()
        .all(|(ch, _)| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
    {
        out.copy(segment);
        return;
    }

    let origin = segment[0].1;
    out.push_char('"', origin);
    for &(ch, offset) in segment {
        if ch == '"' || ch == '\\' {
            out.push_char('\\', offset);
        }
        out.push_char(ch, offset);
    }
    out.push_char('"', origin);
}

/// 判断键片段是否已经被引号包裹。
fn is_quoted(segment: &[(char, usize)]) -> bool {
    match (segment.first(), segment.last()) {
        (Some((first, _)), Some((last, _))) => {
            segment.len() >= 2 && (*first == '"' || *first == '\'') && first == last
        }
        _ => false,
    }
}

/// 为缺少源码区间的配置诊断补充位置。
///
/// TOML 解析器只提供整体文本，不保留每个键的区间。这里从诊断消息中
/// 提取最可能的键名，并在原始源码中定位；确实无法定位时退回第 1 行
/// 第 1 列，保证诊断始终带有行列号。
pub(crate) fn attach_missing_spans(diagnostics: DiagnosticBag, source: &str) -> DiagnosticBag {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            if diagnostic.span().is_some() {
                return diagnostic;
            }

            let span = locate_hint(source, diagnostic.message())
                .unwrap_or_else(|| Span::point(Position::new(1, 1, 0)));
            diagnostic.with_span(span)
        })
        .collect()
}

/// 根据诊断消息中的键名提示定位源码位置。
fn locate_hint(source: &str, message: &str) -> Option<Span> {
    for hint in message_hints(message) {
        if let Some(offset) = source.find(hint.as_str()) {
            let (line, column) = line_column(source, offset);
            return Some(Span::point(Position::new(line, column, offset)));
        }
    }

    for keyword in [
        "项目", "依赖", "名称", "版本", "入口", "路径", "来源", "提交", "地址", "git",
    ] {
        if let Some(offset) = source.find(keyword) {
            let (line, column) = line_column(source, offset);
            return Some(Span::point(Position::new(line, column, offset)));
        }
    }

    None
}

/// 提取诊断消息中由 `「」` 或反引号包裹的候选键名。
fn message_hints(message: &str) -> Vec<String> {
    let mut hints = Vec::new();
    let mut chars = message.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        let close = match ch {
            '「' => '」',
            '`' => '`',
            _ => continue,
        };

        let start = chars
            .peek()
            .map(|(index, _)| *index)
            .unwrap_or(message.len());
        let mut end = start;
        for (index, current) in chars.by_ref() {
            if current == close {
                end = index;
                break;
            }
            end = index + current.len_utf8();
        }

        if end > start {
            let hint = &message[start..end];
            if !hint.is_empty() && !hints.iter().any(|existing| existing == hint) {
                hints.push(hint.to_owned());
            }
        }
    }

    hints
}

/// 把字节偏移转换为从 1 开始的行列号。
fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let mut line = 1;
    let mut column = 1;

    for (index, ch) in source.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    (line, column)
}

#[cfg(test)]
mod tests {
    use super::quote_bare_keys;

    #[test]
    fn quotes_chinese_table_headers_and_keys() {
        let source = "[项目]\n名称 = \"示例\"\n\n[依赖]\n本地工具 = { 路径 = \"../工具\" }\n";
        let quoted = quote_bare_keys(source);
        assert_eq!(
            quoted.text,
            "[\"项目\"]\n\"名称\"= \"示例\"\n\n[\"依赖\"]\n\"本地工具\"= { \"路径\"= \"../工具\" }\n"
        );
    }

    #[test]
    fn keeps_ascii_and_existing_quoted_keys() {
        let source = "[依赖.math_lib]\n路径 = \"../库\"\n\"版本\" = \"v1\"\n";
        let quoted = quote_bare_keys(source);
        assert_eq!(
            quoted.text,
            "[\"依赖\".math_lib]\n\"路径\"= \"../库\"\n\"版本\"= \"v1\"\n"
        );
    }

    #[test]
    fn does_not_touch_string_values_or_comments() {
        let source = "名称 = \"[项目] # 不是注释\" # [依赖]\n";
        let quoted = quote_bare_keys(source);
        assert_eq!(quoted.text, "\"名称\"= \"[项目] # 不是注释\" # [依赖]\n");
    }

    #[test]
    fn maps_output_offsets_back_to_source() {
        let source = "[项目]\n";
        let quoted = quote_bare_keys(source);
        let offset = quoted.text.find('项').expect("缺少中文键");
        assert_eq!(quoted.original_offset(offset), source.find('项').unwrap());
    }
}
