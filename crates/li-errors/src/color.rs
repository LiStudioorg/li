//! 诊断输出的着色支持。
//!
//! 提供 [`DiagnosticBag::render`] 的统一渲染入口：
//! - 错误用红色 `\u{1b}[31m...\u{1b}[0m` 包裹；
//! - 警告用黄色 `\u{1b}[33m...\u{1b}[0m` 包裹；
//! - 提示与默认输出保持原色。
//!
//! 当标准输出不是 TTY 时（例如被管道重定向到文件或终端），
//! 渲染函数会自动关闭颜色，避免污染日志。

use crate::diagnostic::{DiagnosticBag, Severity};

/// 当 stdout 是否为 TTY，用于决定是否启用 ANSI 颜色。
///
/// 调用方可以缓存该值避免重复系统调用；默认情况下渲染入口
/// 通过 [`DiagnosticBag::render`] 自动处理。
pub fn is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// 按诊断等级渲染整份诊断；错误红色、警告黄色、提示原色。
///
/// 当 [`is_tty`] 为 `false` 时，输出不带任何 ANSI 控制序列。
pub fn render(bag: &DiagnosticBag, color: bool) -> String {
    let mut out = String::new();
    for (index, diagnostic) in bag.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }

        let text = diagnostic.to_string();
        let body = match diagnostic.severity() {
            Severity::Error if color => {
                format!("\u{1b}[31m{text}\u{1b}[0m")
            }
            Severity::Warning if color => {
                format!("\u{1b}[33m{text}\u{1b}[0m")
            }
            _ => text,
        };
        out.push_str(&body);
    }
    out
}
