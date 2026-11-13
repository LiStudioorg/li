//! li 语言的递归下降语法分析器。
//!
//! - 表达式使用 Pratt 优先级攀升，严格遵循 C 系优先级；
//! - 语句级错误采用恐慌模式恢复到 `;` 或 `}`，尽量一次报告多个错误；
//! - 所有诊断均来自 `li-errors`，包含行号、列号、期望内容与实际内容。

mod parser;

pub use parser::{parse, parse_source, Parser};
