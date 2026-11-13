//! 源码位置与源文件辅助类型。
//!
//! 编译器中的所有定位信息都使用从 1 开始的行号和列号。
//! 列号按 Unicode 字符计数，因此一个中文字符只占一列，
//! 更符合阅读中文源码时的直觉。

use std::fmt;

/// 源码中的一个位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// 行号，从 1 开始。
    pub line: usize,
    /// 列号，从 1 开始，按 Unicode 字符计数。
    pub column: usize,
    /// 从文件开头算起的字节偏移量。
    pub offset: usize,
}

impl Position {
    /// 创建一个源码位置。
    pub const fn new(line: usize, column: usize, offset: usize) -> Self {
        Self {
            line,
            column,
            offset,
        }
    }

    /// 源码的起始位置，即第 1 行第 1 列。
    pub const fn start() -> Self {
        Self::new(1, 1, 0)
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::start()
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "第 {} 行第 {} 列", self.line, self.column)
    }
}

/// 源码中的一段区间，采用左闭右开语义 `[start, end)`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    /// 区间起点。
    pub start: Position,
    /// 区间终点，本身不包含在区间内。
    pub end: Position,
}

impl Span {
    /// 由起点和终点创建区间。
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    /// 创建一个只包含单个位置的区间，常用于指向某个符号。
    pub const fn point(position: Position) -> Self {
        Self::new(position, position)
    }

    /// 区间的字节长度。
    pub const fn byte_len(&self) -> usize {
        self.end.offset.saturating_sub(self.start.offset)
    }

    /// 区间是否为空。
    pub const fn is_empty(&self) -> bool {
        self.start.offset == self.end.offset
    }

    /// 合并两个区间，得到能够同时覆盖二者的最小区间。
    ///
    /// 这里按字节偏移比较位置；调用方应保证两个区间来自同一份源码。
    pub fn merge(self, other: Self) -> Self {
        let start = if self.start.offset <= other.start.offset {
            self.start
        } else {
            other.start
        };
        let end = if self.end.offset >= other.end.offset {
            self.end
        } else {
            other.end
        };
        Self::new(start, end)
    }
}

/// 一份已经载入内存的源码文件。
///
/// 该类型只负责保存文本并完成“字节偏移 → 行列位置”的换算，
/// 不参与词法分析或语法分析。
#[derive(Clone, Debug)]
pub struct SourceFile {
    /// 文件名或测试中使用的展示名称。
    name: String,
    /// 完整的源码文本。
    text: String,
    /// 每一行首个字符的字节偏移，第一项恒为 0。
    line_starts: Vec<usize>,
}

impl SourceFile {
    /// 创建源文件，并预先计算每一行的起始偏移。
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];

        for (index, ch) in text.char_indices() {
            if ch == '\n' {
                line_starts.push(index + ch.len_utf8());
            }
        }

        Self {
            name: name.into(),
            text,
            line_starts,
        }
    }

    /// 返回文件名或展示名称。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 返回完整源码文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 返回总行数。以换行符结尾的文件会包含最后一行的空行。
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// 将字节偏移换算为行列位置。
    ///
    /// 若偏移越界，会被限制到文件末尾；若偏移落在多字节字符内部，
    /// 会向前回退到该字符的起始位置。列号按 Unicode 字符计数。
    pub fn position_at(&self, offset: usize) -> Position {
        let mut offset = offset.min(self.text.len());

        // 保证偏移落在字符边界上，避免切片时触发 panic。
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }

        let line_index = match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let line_start = self.line_starts[line_index];
        let column = self.text[line_start..offset].chars().count() + 1;

        Position {
            line: line_index + 1,
            column,
            offset,
        }
    }

    /// 根据起止字节偏移创建区间，并自动计算行列位置。
    pub fn span(&self, start_offset: usize, end_offset: usize) -> Span {
        let start_offset = start_offset.min(self.text.len());
        let end_offset = end_offset.max(start_offset).min(self.text.len());

        Span::new(self.position_at(start_offset), self.position_at(end_offset))
    }

    /// 返回指定行的文本，不包含行尾的 `\n` 或 `\r\n`。
    ///
    /// 行号从 1 开始；越界时返回 [`None`]。
    pub fn line(&self, line: usize) -> Option<&str> {
        if line == 0 || line > self.line_starts.len() {
            return None;
        }

        let start = self.line_starts[line - 1];
        let end = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.text.len());
        let raw = &self.text[start..end];
        let without_newline = raw.strip_suffix('\n').unwrap_or(raw);
        let without_carriage_return = without_newline
            .strip_suffix('\r')
            .unwrap_or(without_newline);

        Some(without_carriage_return)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_line_one_column_one() {
        let file = SourceFile::new("示例.li", "让 甲 = 1;");

        assert_eq!(file.position_at(0), Position::new(1, 1, 0));
        assert_eq!(file.name(), "示例.li");
        assert_eq!(file.line_count(), 1);
    }

    #[test]
    fn clamps_offsets_to_valid_character_boundaries() {
        let file = SourceFile::new("示例.li", "让 甲 = 1;");
        let inside_character = file.text().find('甲').unwrap() + 1;

        let position = file.position_at(inside_character);

        assert_eq!(position.line, 1);
        assert_eq!(position.column, 3);
        assert_eq!(position.offset, file.text().find('甲').unwrap());
    }
}
