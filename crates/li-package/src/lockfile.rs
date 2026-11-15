//! `li.lock` 依赖锁文件的读写。
//!
//! 锁文件记录每个依赖的精确来源。Git 依赖必须保存完整提交哈希，
//! 从而保证每次获取到的源码完全一致；本地路径依赖保存声明路径，
//! 如果来源目录本身是 Git 仓库，也会额外记录当前提交。
//!
//! 锁文件使用 TOML 语法，但格式由编译器生成，用户通常不需要手动编辑。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use li_errors::{Diagnostic, DiagnosticBag, Position, Span};
use toml::Value;

use crate::manifest::{Dependency, DependencySource};
use crate::toml_compat::{attach_missing_spans, quote_bare_keys};

/// 依赖锁文件名。
pub const LOCK_FILE: &str = "li.lock";

/// 相对项目根目录的依赖安装目录。
pub const DEPENDENCY_DIR: &str = ".li/依赖";

/// 当前锁文件格式版本。
pub const LOCK_VERSION: i64 = 1;

/// 一份依赖锁文件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lockfile {
    /// 按依赖名称排序的锁定条目。
    pub packages: BTreeMap<String, LockedPackage>,
}

/// 单个依赖的锁定信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedPackage {
    /// 依赖名称。
    pub name: String,
    /// 锁定来源。
    pub source: LockedSource,
}

/// 锁定来源。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockedSource {
    /// 本地路径依赖。
    Path {
        /// `li.toml` 中的声明路径。
        path: String,
        /// 来源目录的 Git 提交；不是 Git 仓库时为空。
        commit: Option<String>,
    },
    /// Git 依赖。
    Git {
        /// 仓库地址。
        url: String,
        /// 声明的版本。
        version: Option<String>,
        /// 获取时的完整提交哈希。
        commit: String,
    },
}

impl Lockfile {
    /// 创建空锁文件。
    pub fn new() -> Self {
        Self {
            packages: BTreeMap::new(),
        }
    }

    /// 从项目根目录读取 `li.lock`；文件不存在时返回 `None`。
    pub fn load(root: &Path) -> Result<Option<Self>, DiagnosticBag> {
        let path = root.join(LOCK_FILE);
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                let mut diagnostics = DiagnosticBag::new();
                diagnostics.push(
                    Diagnostic::error(format!("无法读取依赖锁文件「{}」", path.display()))
                        .with_note(io_error_text(&error)),
                );
                return Err(diagnostics);
            }
        };

        Self::parse(&source, &path).map(Some)
    }

    /// 解析锁文件文本。
    pub fn parse(source: &str, path: &Path) -> Result<Self, DiagnosticBag> {
        let quoted = quote_bare_keys(source);
        let value: Value = match toml::from_str(&quoted.text) {
            Ok(value) => value,
            Err(error) => return Err(toml_error(source, &quoted, path, &error)),
        };

        let mut diagnostics = DiagnosticBag::new();
        let Some(table) = value.as_table() else {
            diagnostics.push(
                Diagnostic::error("依赖锁文件的顶层必须是表")
                    .with_note(format!("文件：{}", path.display())),
            );
            return Err(attach_missing_spans(diagnostics, source));
        };

        match table.get("版本") {
            Some(Value::Integer(version)) if *version == LOCK_VERSION => {}
            Some(Value::Integer(version)) => diagnostics.push(
                Diagnostic::error(format!("依赖锁文件版本「{version}」不受支持"))
                    .with_expected(format!("{LOCK_VERSION}"))
                    .with_note("请重新运行「li 获取」生成本版本的锁文件"),
            ),
            Some(_) => diagnostics.push(
                Diagnostic::error("依赖锁文件的 `版本` 必须是整数")
                    .with_note(format!("文件：{}", path.display())),
            ),
            None => diagnostics.push(
                Diagnostic::error("依赖锁文件缺少 `版本` 字段")
                    .with_note("请重新运行「li 获取」生成锁文件"),
            ),
        }

        let mut packages = BTreeMap::new();

        match table.get("依赖") {
            Some(Value::Table(entries)) => {
                for (name, value) in entries {
                    match parse_locked(name, value) {
                        Ok(package) => {
                            packages.insert(name.clone(), package);
                        }
                        Err(diagnostic) => diagnostics.push(diagnostic),
                    }
                }
            }
            Some(_) => diagnostics.push(
                Diagnostic::error("依赖锁文件的 `依赖` 必须是表")
                    .with_note(format!("文件：{}", path.display())),
            ),
            None => {}
        }

        if diagnostics.has_errors() {
            Err(attach_missing_spans(diagnostics, source))
        } else {
            Ok(Self { packages })
        }
    }

    /// 把锁文件写入项目根目录。
    pub fn write(&self, root: &Path) -> Result<(), DiagnosticBag> {
        let path = root.join(LOCK_FILE);
        let temp = root.join(format!(".{LOCK_FILE}.tmp"));

        if let Err(error) = fs::write(&temp, self.render()) {
            return Err(write_error(&path, &error));
        }

        if let Err(error) = fs::rename(&temp, &path) {
            let _ = fs::remove_file(&temp);
            return Err(write_error(&path, &error));
        }

        Ok(())
    }

    /// 生成锁文件文本。
    pub fn render(&self) -> String {
        let mut output = String::new();
        output.push_str("# 本文件由「li 获取」自动生成，请勿手动编辑。\n");
        output.push_str(&format!("\"版本\" = {LOCK_VERSION}\n"));

        for (name, package) in &self.packages {
            output.push_str(&format!("\n[\"依赖\".{}]\n", quote_key(name)));

            match &package.source {
                LockedSource::Path { path, commit } => {
                    output.push_str("\"来源\" = \"路径\"\n");
                    output.push_str(&format!("\"路径\" = {}\n", quote_string(path)));
                    if let Some(commit) = commit {
                        output.push_str(&format!("\"提交\" = {}\n", quote_string(commit)));
                    }
                }
                LockedSource::Git {
                    url,
                    version,
                    commit,
                } => {
                    output.push_str("\"来源\" = \"git\"\n");
                    output.push_str(&format!("\"地址\" = {}\n", quote_string(url)));
                    if let Some(version) = version {
                        output.push_str(&format!("\"版本\" = {}\n", quote_string(version)));
                    }
                    output.push_str(&format!("\"提交\" = {}\n", quote_string(commit)));
                }
            }
        }

        output
    }

    /// 检查锁文件是否与当前 `li.toml` 一致。
    pub fn validate_manifest(&self, dependencies: &[Dependency]) -> Result<(), DiagnosticBag> {
        let mut diagnostics = DiagnosticBag::new();

        for dependency in dependencies {
            match self.packages.get(&dependency.name) {
                Some(package) => {
                    if !package.matches(&dependency.source) {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "依赖「{}」的来源与 li.lock 不一致",
                                dependency.name
                            ))
                            .with_expected(describe_dependency(&dependency.source))
                            .with_actual(describe_locked(&package.source))
                            .with_note("请重新运行「li 获取」更新锁文件"),
                        );
                    }
                }
                None => diagnostics.push(
                    Diagnostic::error(format!("依赖「{}」不在 li.lock 中", dependency.name))
                        .with_note("请重新运行「li 获取」生成完整锁文件"),
                ),
            }
        }

        if diagnostics.has_errors() {
            Err(diagnostics)
        } else {
            Ok(())
        }
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self::new()
    }
}

impl LockedPackage {
    /// 判断锁定来源是否与依赖声明一致。
    pub fn matches(&self, source: &DependencySource) -> bool {
        match (&self.source, source) {
            (LockedSource::Path { path, .. }, DependencySource::Path { path: declared }) => {
                path == &declared.to_string_lossy()
            }
            (
                LockedSource::Git {
                    url: locked_url,
                    version: locked_version,
                    ..
                },
                DependencySource::Git {
                    url,
                    version: declared_version,
                },
            ) => locked_url == url && locked_version == declared_version,
            _ => false,
        }
    }
}

/// 解析单个锁文件条目。
// 诊断对象较大，返回 `Diagnostic` 比装箱更适合这里的短小调用链。
#[allow(clippy::result_large_err)]
fn parse_locked(name: &str, value: &Value) -> Result<LockedPackage, Diagnostic> {
    let Some(table) = value.as_table() else {
        return Err(
            Diagnostic::error(format!("锁文件中的依赖「{name}」必须是表"))
                .with_note("请重新运行「li 获取」生成锁文件"),
        );
    };

    let source = match table.get("来源").and_then(Value::as_str) {
        Some("路径") => {
            let Some(path) = table.get("路径").and_then(Value::as_str) else {
                return Err(
                    Diagnostic::error(format!("锁文件中的本地依赖「{name}」缺少 `路径`"))
                        .with_note("请重新运行「li 获取」生成锁文件"),
                );
            };
            let commit = table.get("提交").and_then(Value::as_str).map(str::to_owned);
            LockedSource::Path {
                path: path.to_owned(),
                commit,
            }
        }
        Some("git") => {
            let Some(url) = table.get("地址").and_then(Value::as_str) else {
                return Err(
                    Diagnostic::error(format!("锁文件中的 Git 依赖「{name}」缺少 `地址`"))
                        .with_note("请重新运行「li 获取」生成锁文件"),
                );
            };
            let commit = table
                .get("提交")
                .and_then(Value::as_str)
                .filter(|commit| !commit.is_empty())
                .ok_or_else(|| {
                    Diagnostic::error(format!("锁文件中的 Git 依赖「{name}」缺少精确提交"))
                        .with_note("完整提交哈希是保证源码一致性的必要条件")
                        .with_note("请重新运行「li 获取」生成锁文件")
                })?;
            let version = table.get("版本").and_then(Value::as_str).map(str::to_owned);
            LockedSource::Git {
                url: url.to_owned(),
                version,
                commit: commit.to_owned(),
            }
        }
        Some(other) => {
            return Err(
                Diagnostic::error(format!("锁文件中的依赖「{name}」来源未知"))
                    .with_expected("`路径` 或 `git`")
                    .with_actual(format!("「{other}」"))
                    .with_note("请重新运行「li 获取」生成锁文件"),
            );
        }
        None => {
            return Err(
                Diagnostic::error(format!("锁文件中的依赖「{name}」缺少 `来源`"))
                    .with_note("请重新运行「li 获取」生成锁文件"),
            );
        }
    };

    Ok(LockedPackage {
        name: name.to_owned(),
        source,
    })
}

/// 为诊断生成依赖来源描述。
fn describe_dependency(source: &DependencySource) -> String {
    match source {
        DependencySource::Path { path } => format!("路径「{}」", path.display()),
        DependencySource::Git {
            url,
            version: Some(version),
        } => format!("Git「{url}」版本「{version}」"),
        DependencySource::Git { url, version: None } => format!("Git「{url}」"),
    }
}

/// 为诊断生成锁定来源描述。
fn describe_locked(source: &LockedSource) -> String {
    match source {
        LockedSource::Path { path, .. } => format!("路径「{path}」"),
        LockedSource::Git {
            url,
            version: Some(version),
            ..
        } => format!("Git「{url}」版本「{version}」"),
        LockedSource::Git {
            url, version: None, ..
        } => format!("Git「{url}」"),
    }
}

/// 判断锁文件键是否需要加引号。
fn quote_key(key: &str) -> String {
    if key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        key.to_owned()
    } else {
        quote_string(key)
    }
}

/// 转义 TOML 基本字符串。
fn quote_string(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() + 2);
    escaped.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

/// 把 TOML 解析错误转换为全中文诊断。
fn toml_error(
    source: &str,
    quoted: &crate::toml_compat::QuotedToml,
    path: &Path,
    error: &toml::de::Error,
) -> DiagnosticBag {
    let mut diagnostic = Diagnostic::error("依赖锁文件格式无效")
        .with_expected("由「li 获取」生成的合法锁文件")
        .with_note("请删除该文件后重新运行「li 获取」")
        .with_note(format!("文件：{}", path.display()));

    if let Some(span) = error.span() {
        let offset = quoted.original_offset(span.start);
        let (line, column) = line_column(source, offset);
        diagnostic = diagnostic.with_span(Span::point(Position::new(line, column, offset)));
    }

    let mut diagnostics = DiagnosticBag::new();
    diagnostics.push(diagnostic);
    diagnostics
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

/// 生成写入失败的诊断。
fn write_error(path: &Path, error: &std::io::Error) -> DiagnosticBag {
    let mut diagnostics = DiagnosticBag::new();
    diagnostics.push(
        Diagnostic::error(format!("无法写入依赖锁文件「{}」", path.display()))
            .with_note(io_error_text(error)),
    );
    diagnostics
}

/// 把输入输出错误转换为中文说明。
fn io_error_text(error: &std::io::Error) -> &'static str {
    use std::io::ErrorKind;

    match error.kind() {
        ErrorKind::NotFound => "文件不存在",
        ErrorKind::PermissionDenied => "权限不足",
        ErrorKind::InvalidData => "文件内容不是有效的 UTF-8",
        ErrorKind::InvalidInput => "路径或参数无效",
        ErrorKind::AlreadyExists => "文件已存在",
        ErrorKind::WriteZero => "写入失败",
        ErrorKind::UnexpectedEof => "文件内容不完整",
        _ => "输入输出错误",
    }
}
