//! li 编译器前端的统一诊断基础设施。
//!
//! 本 crate 负责三件事：
//! - 用 [`Position`] 与 [`Span`] 表示源码中的行、列和区间；
//! - 用 [`Diagnostic`] 描述中文错误信息，包括期望值与实际值；
//! - 用 [`DiagnosticBag`] 按顺序收集一次编译产生的全部诊断。
//!
//! 行号与列号都从 1 开始，诊断信息的格式与 CLI 输出保持一致：
//!
//! ```
//! use li_errors::{Diagnostic, Position, Span};
//!
//! let span = Span::point(Position::new(3, 5, 20));
//! let diagnostic = Diagnostic::type_mismatch(span, "整数", "字符串");
//! assert_eq!(
//!     diagnostic.to_string(),
//!     "第 3 行第 5 列：类型不匹配，期望 整数，实际 字符串"
//! );
//! ```

pub mod diagnostic;
pub mod source;

pub use diagnostic::{Diagnostic, DiagnosticBag, Severity};
pub use source::{Position, SourceFile, Span};
