//! 统一的中文诊断信息。
//!
//! 一条诊断由级别、消息、可选的源码区间、期望值、实际值、错误码和附注组成。
//! 输出格式固定为：
//!
//! ```text
//! 第 3 行第 5 列：类型不匹配，期望 整数，实际 字符串
//! ```
//!
//! 安全类检查（不可变性、内存安全、借用检查等）一律强制产生「警告」
//! 诊断（黄色），编译继续；只有真正的语法错误、类型不匹配等致命问题
//! 才抛出红色 Error 拦截编译。不提供任何忽略开关。

use std::fmt;

use crate::source::Span;

/// 诊断信息的级别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    /// 编译错误：存在时必须阻止后续处理。
    Error,
    /// 警告：不阻止编译，但提示潜在问题。
    Warning,
    /// 提示：对错误或代码的补充说明。
    Note,
}

impl Severity {
    /// 返回该级别对应的中文名称。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Error => "错误",
            Self::Warning => "警告",
            Self::Note => "提示",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// 一条编译诊断信息。
///
/// 诊断本身不依赖具体的源文件，只保存行列位置；
/// 需要在源码中展示上下文时，可以配合 [`crate::SourceFile`] 使用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    severity: Severity,
    code: Option<&'static str>,
    message: String,
    span: Option<Span>,
    expected: Option<String>,
    actual: Option<String>,
    notes: Vec<String>,
}

impl Diagnostic {
    /// 创建一个错误级别的诊断。
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Severity::Error, message)
    }

    /// 创建一个警告级别的诊断。
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, message)
    }

    /// 创建一个提示级别的诊断。
    pub fn note(message: impl Into<String>) -> Self {
        Self::new(Severity::Note, message)
    }

    /// 创建“类型不匹配”错误，这是语义分析阶段最常见的诊断。
    pub fn type_mismatch(
        span: Span,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self::error("类型不匹配")
            .with_span(span)
            .with_expected(expected)
            .with_actual(actual)
    }

    /// 创建语法错误，附带期望的语法结构和实际遇到的 Token。
    pub fn syntax_error(
        span: Span,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self::error("语法错误")
            .with_span(span)
            .with_expected(expected)
            .with_actual(actual)
    }

    fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            code: None,
            message: message.into(),
            span: None,
            expected: None,
            actual: None,
            notes: Vec::new(),
        }
    }

    /// 设置诊断级别。
    #[must_use]
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// 设置稳定错误码，例如 `L0201`。
    #[must_use]
    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    /// 设置诊断对应的源码区间。
    #[must_use]
    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    /// 设置期望的类型或语法结构。
    #[must_use]
    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    /// 设置实际遇到的类型或语法结构。
    #[must_use]
    pub fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
        self
    }

    /// 追加一条附注，多条附注会依次输出。
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// 返回诊断级别。
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// 返回稳定错误码。
    pub const fn code(&self) -> Option<&'static str> {
        self.code
    }

    /// 返回诊断消息。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 返回诊断对应的源码区间。
    pub const fn span(&self) -> Option<Span> {
        self.span
    }

    /// 返回期望的类型或语法结构。
    pub fn expected(&self) -> Option<&str> {
        self.expected.as_deref()
    }

    /// 返回实际遇到的类型或语法结构。
    pub fn actual(&self) -> Option<&str> {
        self.actual.as_deref()
    }

    /// 返回全部附注。
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// 是否为错误级别。
    pub const fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error)
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(span) = self.span {
            write!(f, "{}：", span.start)?;

            // 错误是默认级别；警告和提示需要显式标出级别。
            if self.severity != Severity::Error {
                write!(f, "{}：", self.severity)?;
            }
        } else {
            // 缺少定位时，所有级别都显式标出，便于用户识别等级。
            write!(f, "{}：", self.severity)?;
        }

        f.write_str(&self.message)?;

        if let Some(expected) = &self.expected {
            write!(f, "，期望 {}", expected)?;
        }
        if let Some(actual) = &self.actual {
            write!(f, "，实际 {}", actual)?;
        }
        if let Some(code) = self.code {
            write!(f, "（错误码 {}）", code)?;
        }
        for note in &self.notes {
            write!(f, "；{}", note)?;
        }

        Ok(())
    }
}

impl std::error::Error for Diagnostic {}

/// 按产生顺序收集一次编译过程中的所有诊断。
///
/// 词法分析、语法分析和语义分析共用该容器，便于一次性向用户报告多个问题。
#[derive(Clone, Debug, Default)]
pub struct DiagnosticBag {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBag {
    /// 创建空诊断集合。
    pub fn new() -> Self {
        Self::default()
    }

    /// 用单条诊断构造集合；便于快速把错误包成可传播的 `Err`。
    pub fn from_single(diagnostic: Diagnostic) -> Self {
        let mut bag = Self::new();
        bag.push(diagnostic);
        bag
    }

    /// 添加一条诊断。
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// 判断集合是否为空。
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// 返回诊断总数。
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// 判断是否包含错误级别的诊断。
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// 返回错误级别的诊断数量。
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_error())
            .count()
    }

    /// 返回警告级别的诊断数量。
    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity() == Severity::Warning)
            .count()
    }

    /// 判断是否包含警告级别的诊断。
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity() == Severity::Warning)
    }

    /// 以切片形式访问全部诊断。
    pub fn as_slice(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// 清空集合。
    pub fn clear(&mut self) {
        self.diagnostics.clear();
    }

    /// 按顺序遍历诊断。
    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }

    /// 取出内部诊断列表。
    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

impl fmt::Display for DiagnosticBag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{}", diagnostic)?;
        }
        Ok(())
    }
}

impl Extend<Diagnostic> for DiagnosticBag {
    fn extend<T: IntoIterator<Item = Diagnostic>>(&mut self, iter: T) {
        self.diagnostics.extend(iter);
    }
}

impl FromIterator<Diagnostic> for DiagnosticBag {
    fn from_iter<T: IntoIterator<Item = Diagnostic>>(iter: T) -> Self {
        Self {
            diagnostics: iter.into_iter().collect(),
        }
    }
}

impl IntoIterator for DiagnosticBag {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.diagnostics.into_iter()
    }
}

impl<'a> IntoIterator for &'a DiagnosticBag {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.diagnostics.iter()
    }
}
