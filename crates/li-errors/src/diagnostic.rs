//! 统一的中文诊断信息。
//!
//! 一条诊断由级别、消息、可选的源码区间、期望值、实际值、错误码和附注组成。
//! 输出格式固定为：
//!
//! ```text
//! 第 3 行第 5 列：类型不匹配，期望 整数，实际 字符串
//! ```
//!
//! 检查规则可以独立配置等级（`报错` / `警告` / `忽略`）；默认所有语义检查
//! 等级为「警告」，由 `li.toml` 的 `[检查]` 表或函数/模块上的 `@检查(...)`
//! 注解覆盖。

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

/// 检查规则的等级；由 `[检查]` 表或 `@检查(...)` 注解配置。
///
/// 默认等级为 [`RuleLevel::Warning`]；`li.toml` 与 `@检查(...)` 注解
/// 按「注解 > 项目配置 > 默认」的优先级覆盖。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RuleLevel {
    /// 报错：发现该规则时产生 Error 诊断并中断编译。
    Error,
    /// 警告（默认）：发现该规则时产生 Warning 诊断，编译继续。
    #[default]
    Warning,
    /// 忽略：发现该规则时不产生任何诊断，静默通过。
    Ignore,
}

impl RuleLevel {
    /// 返回该等级对应的中文名称。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Error => "报错",
            Self::Warning => "警告",
            Self::Ignore => "忽略",
        }
    }

    /// 判断该等级是否产生 Error 诊断。
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }

    /// 判断该等级是否完全静默。
    pub const fn is_ignoring(self) -> bool {
        matches!(self, Self::Ignore)
    }

    /// 解析「报错」/「警告」/「忽略」三种中文等级名。
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "报错" | "error" | "Error" => Some(Self::Error),
            "警告" | "warning" | "Warning" => Some(Self::Warning),
            "忽略" | "ignore" | "Ignore" => Some(Self::Ignore),
            _ => None,
        }
    }
}

impl fmt::Display for RuleLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// 检查规则名；每条规则对应一类可配置的检查（如不可变性、内存安全）。
///
/// 第一版内置规则：
/// - [`Rule::Immutability`]：不可变变量重赋值；
/// - [`Rule::MemorySafety`]：原生指针使用（预留，M11 实现）；
/// - [`Rule::BorrowChecking`]：借用检查（预留，M13 实现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    /// 不可变变量重赋值。
    Immutability,
    /// 内存安全（原生指针使用约束；M11 启用）。
    MemorySafety,
    /// 借用检查（M13 启用）。
    BorrowChecking,
    /// 未知规则；由用户自定义，默认按「警告」处理。
    Unknown,
}

impl Rule {
    /// 返回规则对应的中文名称。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Immutability => "不可变性",
            Self::MemorySafety => "内存安全",
            Self::BorrowChecking => "借用检查",
            Self::Unknown => "未知",
        }
    }

    /// 解析「不可变性」/「内存安全」/「借用检查」三种中文规则名。
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "不可变性" | "immutable" | "Immutability" => Some(Self::Immutability),
            "内存安全" | "memory-safety" | "MemorySafety" => Some(Self::MemorySafety),
            "借用检查" | "borrow" | "BorrowChecking" => Some(Self::BorrowChecking),
            _ => None,
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// 规则等级配置表；`li.toml` 的 `[检查]` 表解析结果。
///
/// 未显式声明的规则使用 [`RuleLevel::Warning`] 作为默认。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleConfig {
    levels: std::collections::HashMap<Rule, RuleLevel>,
}

impl RuleConfig {
    /// 创建空配置；全部规则使用默认等级（`警告`）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置某条规则的等级；`忽略` 等级意味着静默通过。
    pub fn set(&mut self, rule: Rule, level: RuleLevel) {
        self.levels.insert(rule, level);
    }

    /// 返回某条规则的等级；未配置时返回默认的 [`RuleLevel::Warning`]。
    pub fn level_of(&self, rule: Rule) -> RuleLevel {
        self.levels.get(&rule).copied().unwrap_or_default()
    }

    /// 是否显式配置了某条规则。
    pub fn has(&self, rule: Rule) -> bool {
        self.levels.contains_key(&rule)
    }

    /// 全部显式配置的规则数量。
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// 配置是否为空。
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// 遍历全部显式配置的规则。
    pub fn iter(&self) -> impl Iterator<Item = (&Rule, &RuleLevel)> {
        self.levels.iter()
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
