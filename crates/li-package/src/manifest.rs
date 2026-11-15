//! `li.toml` 项目配置解析。
//!
//! 配置文件使用 TOML 语法，键名与用户可见内容保持中文：
//!
//! ```toml
//! [项目]
//! 名称 = "示例"
//! 版本 = "0.1.0"
//!
//! [依赖]
//! 本地工具 = { 路径 = "../我的工具" }
//! 网络库 = { git = "https://github.com/xxx/net.git", 版本 = "v1.0" }
//! ```
//!
//! 第一版只支持本地路径与 Git 仓库两种依赖来源。所有诊断均为中文，
//! 格式错误会尽量给出行列位置。

use std::fs;
use std::path::{Path, PathBuf};

use li_errors::{Diagnostic, DiagnosticBag, Position, Span};
use toml::Value;

use crate::toml_compat::{attach_missing_spans, quote_bare_keys};

/// 项目配置文件名。
pub const MANIFEST_FILE: &str = "li.toml";

/// 一份解析完成的 `li.toml`。
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    /// `[项目]` 表中的项目信息。
    pub project: ProjectInfo,
    /// `[依赖]` 表中的依赖，保持文件中的声明顺序。
    pub dependencies: Vec<Dependency>,
}

/// `[项目]` 表中的信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectInfo {
    /// 项目名称；未声明时使用项目目录名。
    pub name: String,
    /// 项目版本。
    pub version: Option<String>,
    /// 可选的入口文件，相对于项目根目录。
    pub entry: Option<PathBuf>,
}

/// 一条依赖声明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// 依赖名称，也是 `导入` 时使用的名称。
    pub name: String,
    /// 依赖来源。
    pub source: DependencySource,
}

/// 依赖来源。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DependencySource {
    /// 本地路径，相对于项目根目录，也允许绝对路径。
    Path {
        /// 声明中的路径。
        path: PathBuf,
    },
    /// Git 仓库。
    Git {
        /// 仓库地址。
        url: String,
        /// 可选的版本、标签、分支或提交。
        version: Option<String>,
    },
}

impl Manifest {
    /// 从项目根目录读取并解析 `li.toml`。
    pub fn load(root: &Path) -> Result<Self, DiagnosticBag> {
        let path = root.join(MANIFEST_FILE);
        let source = fs::read_to_string(&path).map_err(|error| {
            let mut diagnostics = DiagnosticBag::new();
            diagnostics.push(
                Diagnostic::error(format!("无法读取项目配置「{}」", path.display()))
                    .with_note(io_error_text(&error)),
            );
            diagnostics
        })?;

        Self::parse(&source, &path, root)
    }

    /// 解析配置文本；`path` 仅用于诊断提示。
    pub fn parse(source: &str, path: &Path, root: &Path) -> Result<Self, DiagnosticBag> {
        let quoted = quote_bare_keys(source);
        let value: Value = match toml::from_str(&quoted.text) {
            Ok(value) => value,
            Err(error) => return Err(toml_error(source, &quoted, path, &error)),
        };

        let table = match value.as_table() {
            Some(table) => table,
            None => {
                let mut diagnostics = DiagnosticBag::new();
                diagnostics.push(
                    Diagnostic::error("项目配置的顶层必须是表")
                        .with_note(format!("文件：{}", path.display())),
                );
                return Err(attach_missing_spans(diagnostics, source));
            }
        };

        let mut diagnostics = DiagnosticBag::new();

        let project = match table.get("项目") {
            Some(value) => parse_project(value, root, path, &mut diagnostics),
            None => ProjectInfo {
                name: default_project_name(root),
                version: None,
                entry: None,
            },
        };

        let mut dependencies = Vec::new();
        match table.get("依赖") {
            Some(Value::Table(entries)) => {
                for (name, value) in entries {
                    match parse_dependency(name, value, path) {
                        Ok(dependency) => dependencies.push(dependency),
                        Err(diagnostic) => diagnostics.push(diagnostic),
                    }
                }
            }
            Some(other) => diagnostics.push(
                Diagnostic::error("`依赖` 必须是表")
                    .with_expected("形如 `[依赖]` 的表")
                    .with_actual(value_kind(other).to_string())
                    .with_note(format!("文件：{}", path.display())),
            ),
            None => {}
        }

        if diagnostics.has_errors() {
            Err(attach_missing_spans(diagnostics, source))
        } else {
            Ok(Self {
                project,
                dependencies,
            })
        }
    }

    /// 按名称查找依赖。
    pub fn dependency(&self, name: &str) -> Option<&Dependency> {
        self.dependencies
            .iter()
            .find(|dependency| dependency.name == name)
    }
}

/// 解析 `[项目]` 表。
fn parse_project(
    value: &Value,
    root: &Path,
    path: &Path,
    diagnostics: &mut DiagnosticBag,
) -> ProjectInfo {
    let Some(table) = value.as_table() else {
        diagnostics.push(
            Diagnostic::error("`项目` 必须是表")
                .with_expected("形如 `[项目]` 的表")
                .with_actual(value_kind(value))
                .with_note(format!("文件：{}", path.display())),
        );
        return ProjectInfo {
            name: default_project_name(root),
            version: None,
            entry: None,
        };
    };

    let mut name = default_project_name(root);
    if let Some(value) = table.get("名称") {
        match value.as_str() {
            Some(text) if !text.trim().is_empty() => name = text.to_owned(),
            Some(_) => diagnostics.push(
                Diagnostic::error("项目名称不能为空")
                    .with_note(format!("文件：{}", path.display())),
            ),
            None => diagnostics.push(
                Diagnostic::error("`项目.名称` 必须是字符串")
                    .with_expected("字符串")
                    .with_actual(value_kind(value))
                    .with_note(format!("文件：{}", path.display())),
            ),
        }
    }

    let version = match table.get("版本") {
        Some(value) => match value.as_str() {
            Some(text) if !text.trim().is_empty() => Some(text.to_owned()),
            Some(_) => {
                diagnostics.push(
                    Diagnostic::error("项目版本不能为空")
                        .with_note(format!("文件：{}", path.display())),
                );
                None
            }
            None => {
                diagnostics.push(
                    Diagnostic::error("`项目.版本` 必须是字符串")
                        .with_expected("字符串")
                        .with_actual(value_kind(value))
                        .with_note(format!("文件：{}", path.display())),
                );
                None
            }
        },
        None => None,
    };

    let entry = match table.get("入口") {
        Some(value) => match value.as_str() {
            Some(text) if !text.trim().is_empty() => Some(PathBuf::from(text)),
            Some(_) => {
                diagnostics.push(
                    Diagnostic::error("项目入口不能为空")
                        .with_note(format!("文件：{}", path.display())),
                );
                None
            }
            None => {
                diagnostics.push(
                    Diagnostic::error("`项目.入口` 必须是字符串")
                        .with_expected("字符串")
                        .with_actual(value_kind(value))
                        .with_note(format!("文件：{}", path.display())),
                );
                None
            }
        },
        None => None,
    };

    ProjectInfo {
        name,
        version,
        entry,
    }
}

/// 解析一条依赖。
// 诊断对象较大，返回 `Diagnostic` 比装箱更适合这里的短小调用链。
#[allow(clippy::result_large_err)]
fn parse_dependency(name: &str, value: &Value, path: &Path) -> Result<Dependency, Diagnostic> {
    if !is_valid_dependency_name(name) {
        return Err(Diagnostic::error(format!("依赖名称「{name}」无效"))
            .with_expected("由中文、字母、数字、下划线或连字符组成，且不含路径分隔符")
            .with_note(format!("文件：{}", path.display())));
    }

    let Some(table) = value.as_table() else {
        return Err(Diagnostic::error(format!("依赖「{name}」的声明必须是表"))
            .with_expected("形如 `{ 路径 = \"../库\" }` 或 `{ git = \"...\" }`")
            .with_actual(value_kind(value))
            .with_note(format!("文件：{}", path.display())));
    };

    let path_value = table.get("路径");
    let git_value = table.get("git");
    let version_value = table.get("版本");

    let source = match (path_value, git_value) {
        (Some(path_value), None) => {
            if version_value.is_some() {
                return Err(Diagnostic::error(format!("本地依赖「{name}」不能声明版本"))
                    .with_note("`版本` 只适用于 Git 依赖")
                    .with_note(format!("文件：{}", path.display())));
            }

            let Some(text) = path_value.as_str() else {
                return Err(
                    Diagnostic::error(format!("依赖「{name}」的 `路径` 必须是字符串"))
                        .with_expected("字符串")
                        .with_actual(value_kind(path_value))
                        .with_note(format!("文件：{}", path.display())),
                );
            };

            if text.trim().is_empty() {
                return Err(Diagnostic::error(format!("依赖「{name}」的路径不能为空"))
                    .with_note(format!("文件：{}", path.display())));
            }

            DependencySource::Path {
                path: PathBuf::from(text),
            }
        }
        (None, Some(git_value)) => {
            let Some(text) = git_value.as_str() else {
                return Err(
                    Diagnostic::error(format!("依赖「{name}」的 `git` 必须是字符串"))
                        .with_expected("字符串")
                        .with_actual(value_kind(git_value))
                        .with_note(format!("文件：{}", path.display())),
                );
            };

            if text.trim().is_empty() {
                return Err(
                    Diagnostic::error(format!("依赖「{name}」的 Git 地址不能为空"))
                        .with_note(format!("文件：{}", path.display())),
                );
            }

            let version = match version_value {
                Some(value) => match value.as_str() {
                    Some(text) if !text.trim().is_empty() => Some(text.to_owned()),
                    Some(_) => {
                        return Err(Diagnostic::error(format!("依赖「{name}」的版本不能为空"))
                            .with_note(format!("文件：{}", path.display())));
                    }
                    None => {
                        return Err(Diagnostic::error(format!(
                            "依赖「{name}」的 `版本` 必须是字符串"
                        ))
                        .with_expected("字符串")
                        .with_actual(value_kind(value))
                        .with_note(format!("文件：{}", path.display())));
                    }
                },
                None => None,
            };

            DependencySource::Git {
                url: text.to_owned(),
                version,
            }
        }
        (Some(_), Some(_)) => {
            return Err(
                Diagnostic::error(format!("依赖「{name}」不能同时声明 `路径` 与 `git`"))
                    .with_expected("二者只能选一个")
                    .with_note(format!("文件：{}", path.display())),
            );
        }
        (None, None) => {
            return Err(Diagnostic::error(format!("依赖「{name}」缺少来源"))
                .with_expected("`路径` 或 `git`")
                .with_note(format!("文件：{}", path.display())));
        }
    };

    Ok(Dependency {
        name: name.to_owned(),
        source,
    })
}

/// 判断依赖名称是否可以安全地作为目录名使用。
pub fn is_valid_dependency_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
}

/// 默认项目名称：目录名，无法取得时使用固定文本。
fn default_project_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("li项目")
        .to_owned()
}

/// 把 TOML 解析错误转换为全中文诊断，并保留大致行列位置。
fn toml_error(
    source: &str,
    quoted: &crate::toml_compat::QuotedToml,
    path: &Path,
    error: &toml::de::Error,
) -> DiagnosticBag {
    let mut diagnostic = Diagnostic::error("项目配置格式无效")
        .with_expected("合法的 TOML 表、键值对与字符串")
        .with_note("请检查引号、逗号、花括号与表头是否完整")
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

/// 描述 TOML 值的类型，用于中文诊断。
fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "字符串",
        Value::Integer(_) => "整数",
        Value::Float(_) => "小数",
        Value::Boolean(_) => "布尔",
        Value::Datetime(_) => "日期时间",
        Value::Array(_) => "数组",
        Value::Table(_) => "表",
    }
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
