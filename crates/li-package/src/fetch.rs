//! 依赖获取：把本地路径与 Git 仓库复制到项目内的隐藏目录。
//!
//! 所有依赖统一安装到 `<项目>/.li/依赖/<名称>`。获取过程先写入临时暂存
//! 目录，全部成功后再替换旧目录并写入 `li.lock`，避免失败时留下半成品。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use li_errors::{Diagnostic, DiagnosticBag};

use crate::lockfile::{LockedPackage, LockedSource, Lockfile, DEPENDENCY_DIR, LOCK_FILE};
use crate::manifest::{Dependency, DependencySource, Manifest};

/// 一次获取操作的结果。
#[derive(Clone, Debug)]
pub struct FetchResult {
    /// 生成的锁文件。
    pub lockfile: Lockfile,
    /// 获取到的依赖摘要，按声明顺序保存。
    pub fetched: Vec<FetchedDependency>,
}

/// 单个依赖的获取摘要，用于 CLI 输出。
#[derive(Clone, Debug)]
pub struct FetchedDependency {
    /// 依赖名称。
    pub name: String,
    /// 来源描述。
    pub source: String,
    /// Git 提交；本地非 Git 目录为空。
    pub commit: Option<String>,
}

/// 解析 `li.toml`，获取全部依赖并写入 `li.lock`。
pub fn fetch(root: &Path) -> Result<FetchResult, DiagnosticBag> {
    let manifest = Manifest::load(root)?;
    let previous = Lockfile::load(root)?;
    let dependencies_dir = root.join(DEPENDENCY_DIR);

    if let Err(error) = fs::create_dir_all(&dependencies_dir) {
        return Err(io_error(
            format!("无法创建依赖目录「{}」", dependencies_dir.display()),
            &error,
        ));
    }

    let staging = dependencies_dir.join(format!(".暂存-{}", std::process::id()));
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }

    if let Err(error) = fs::create_dir_all(&staging) {
        return Err(io_error(
            format!("无法创建依赖暂存目录「{}」", staging.display()),
            &error,
        ));
    }

    let result = fetch_all(root, &manifest, previous.as_ref(), &staging);
    match result {
        Ok((lockfile, fetched)) => {
            if let Err(error) = install_all(&staging, &dependencies_dir, &manifest) {
                let _ = fs::remove_dir_all(&staging);
                return Err(error);
            }
            let _ = fs::remove_dir_all(&staging);

            lockfile.write(root)?;

            Ok(FetchResult { lockfile, fetched })
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

/// 获取全部依赖到暂存目录。
fn fetch_all(
    root: &Path,
    manifest: &Manifest,
    previous: Option<&Lockfile>,
    staging: &Path,
) -> Result<(Lockfile, Vec<FetchedDependency>), DiagnosticBag> {
    let mut lockfile = Lockfile::new();
    let mut fetched = Vec::with_capacity(manifest.dependencies.len());

    for dependency in &manifest.dependencies {
        let (package, summary) = fetch_dependency(root, dependency, previous, staging)?;
        lockfile.packages.insert(dependency.name.clone(), package);
        fetched.push(summary);
    }

    Ok((lockfile, fetched))
}

/// 获取单个依赖。
fn fetch_dependency(
    root: &Path,
    dependency: &Dependency,
    previous: Option<&Lockfile>,
    staging: &Path,
) -> Result<(LockedPackage, FetchedDependency), DiagnosticBag> {
    let destination = staging.join(&dependency.name);

    match &dependency.source {
        DependencySource::Path { path } => {
            let source = resolve_local_path(root, path);
            let source = match fs::canonicalize(&source) {
                Ok(source) => source,
                Err(error) => {
                    return Err(io_error(
                        format!(
                            "无法读取本地依赖「{}」的路径「{}」",
                            dependency.name,
                            source.display()
                        ),
                        &error,
                    ));
                }
            };

            if !source.is_dir() {
                return Err(bag(diagnostic(format!(
                    "本地依赖「{}」的路径「{}」不是目录",
                    dependency.name,
                    source.display()
                ))));
            }

            let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
            if source == root || root.starts_with(&source) {
                return Err(bag(diagnostic(format!(
                    "本地依赖「{}」不能指向项目根目录或其上级目录",
                    dependency.name
                ))
                .with_note("请把依赖放在项目目录之外，或指向项目内的独立子目录")));
            }

            let commit = git_head(&source).ok();
            copy_directory(&source, &destination, &dependency.name)?;

            let summary = FetchedDependency {
                name: dependency.name.clone(),
                source: format!("路径 {}", path.display()),
                commit: commit.clone(),
            };
            let package = LockedPackage {
                name: dependency.name.clone(),
                source: LockedSource::Path {
                    path: path.to_string_lossy().into_owned(),
                    commit,
                },
            };
            Ok((package, summary))
        }
        DependencySource::Git { url, version } => {
            let repository = resolve_git_url(root, url);
            let status = Command::new("git")
                .arg("clone")
                .arg("--quiet")
                .arg(&repository)
                .arg(&destination)
                .output();

            match status {
                Ok(output) if output.status.success() => {}
                Ok(_) => {
                    return Err(bag(diagnostic(format!(
                        "无法克隆 Git 依赖「{}」",
                        dependency.name
                    ))
                    .with_note("请确认仓库地址可访问，且本机已安装 git")
                    .with_note(format!("仓库：{url}"))));
                }
                Err(error) => {
                    return Err(bag(diagnostic(format!(
                        "无法启动 git 以获取依赖「{}」",
                        dependency.name
                    ))
                    .with_note(io_error_text(&error))));
                }
            }

            // 已存在锁文件时优先检出锁定提交，保证同一份 li.lock 拉取的源码完全一致。
            let locked_commit = previous
                .and_then(|lockfile| lockfile.packages.get(&dependency.name))
                .and_then(|package| match &package.source {
                    LockedSource::Git {
                        url: locked_url,
                        version: locked_version,
                        commit,
                    } if locked_url == url && locked_version == version => Some(commit.clone()),
                    _ => None,
                });

            let checkout = match &locked_commit {
                Some(commit) => Some((commit.clone(), true)),
                None => version.clone().map(|version| (version, false)),
            };

            if let Some((target, locked)) = checkout {
                let status = Command::new("git")
                    .arg("-C")
                    .arg(&destination)
                    .args(["checkout", "--quiet", &target])
                    .output();

                match status {
                    Ok(output) if output.status.success() => {}
                    Ok(_) if locked => {
                        return Err(bag(diagnostic(format!(
                            "无法切换到 Git 依赖「{}」在 li.lock 中锁定的提交",
                            dependency.name
                        ))
                        .with_expected(format!("提交 {target}"))
                        .with_note("请删除 li.lock 后重新运行「li 获取」以更新依赖")));
                    }
                    Ok(_) => {
                        return Err(bag(diagnostic(format!(
                            "无法切换到 Git 依赖「{}」的版本「{target}」",
                            dependency.name
                        ))
                        .with_note("请确认该标签、分支或提交存在于仓库中")));
                    }
                    Err(error) => {
                        return Err(bag(diagnostic(format!(
                            "无法启动 git 以切换依赖「{}」的版本",
                            dependency.name
                        ))
                        .with_note(io_error_text(&error))));
                    }
                }
            }

            let commit = match git_head(&destination) {
                Ok(commit) => commit,
                Err(message) => {
                    return Err(bag(diagnostic(format!(
                        "无法读取 Git 依赖「{}」的提交哈希",
                        dependency.name
                    ))
                    .with_note(message)));
                }
            };

            let summary = FetchedDependency {
                name: dependency.name.clone(),
                source: format!("Git {url}"),
                commit: Some(commit.clone()),
            };
            let package = LockedPackage {
                name: dependency.name.clone(),
                source: LockedSource::Git {
                    url: url.clone(),
                    version: version.clone(),
                    commit,
                },
            };
            Ok((package, summary))
        }
    }
}

/// 把暂存目录中的依赖替换到正式目录。
fn install_all(
    staging: &Path,
    dependencies_dir: &Path,
    manifest: &Manifest,
) -> Result<(), DiagnosticBag> {
    for dependency in &manifest.dependencies {
        let source = staging.join(&dependency.name);
        let destination = dependencies_dir.join(&dependency.name);

        if destination.exists() {
            if let Err(error) = fs::remove_dir_all(&destination) {
                return Err(io_error(
                    format!("无法清理旧依赖目录「{}」", destination.display()),
                    &error,
                ));
            }
        }

        if let Err(error) = fs::rename(&source, &destination) {
            return Err(io_error(
                format!("无法安装依赖「{}」", dependency.name),
                &error,
            ));
        }
    }

    Ok(())
}

/// 检查项目依赖是否已按锁文件就绪。
///
/// 没有声明依赖时返回 `None`；否则返回已校验的锁文件。
pub fn ensure_ready(root: &Path, manifest: &Manifest) -> Result<Option<Lockfile>, DiagnosticBag> {
    if manifest.dependencies.is_empty() {
        return Ok(None);
    }

    let Some(lockfile) = Lockfile::load(root)? else {
        return Err(bag(diagnostic(format!(
            "缺少依赖锁文件「{}」",
            root.join(LOCK_FILE).display()
        ))
        .with_note("请先运行「li 获取」拉取依赖并生成锁文件")));
    };

    lockfile.validate_manifest(&manifest.dependencies)?;

    for dependency in &manifest.dependencies {
        let directory = root.join(DEPENDENCY_DIR).join(&dependency.name);
        if !directory.is_dir() {
            return Err(bag(diagnostic(format!(
                "依赖「{}」尚未就绪",
                dependency.name
            ))
            .with_expected(format!("目录「{}」", directory.display()))
            .with_note("请先运行「li 获取」")));
        }

        if let Some(package) = lockfile.packages.get(&dependency.name) {
            if let LockedSource::Git { commit, .. } = &package.source {
                let actual = match git_head(&directory) {
                    Ok(actual) => actual,
                    Err(message) => {
                        return Err(bag(diagnostic(format!(
                            "无法校验依赖「{}」的 Git 提交",
                            dependency.name
                        ))
                        .with_note(message)
                        .with_note("请重新运行「li 获取」")));
                    }
                };

                if &actual != commit {
                    return Err(bag(diagnostic(format!(
                        "依赖「{}」的提交与锁文件不一致",
                        dependency.name
                    ))
                    .with_expected(format!("提交 {commit}"))
                    .with_actual(format!("提交 {actual}"))
                    .with_note("请重新运行「li 获取」")));
                }
            }
        }
    }

    Ok(Some(lockfile))
}

/// 读取项目入口文件：优先使用 `[项目].入口`，否则按常见布局探测。
pub fn project_entry(root: &Path, manifest: &Manifest) -> Result<PathBuf, DiagnosticBag> {
    if let Some(entry) = &manifest.project.entry {
        let path = root.join(entry);
        if path.is_file() {
            return Ok(path);
        }
        return Err(bag(diagnostic(format!(
            "项目配置声明的入口「{}」不存在",
            path.display()
        ))
        .with_expected(format!("入口文件「{}」", entry.display()))
        .with_note("请在 `li.toml` 的 `[项目]` 中修正 `入口`，或创建该文件")));
    }

    for candidate in common_entries(root) {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(bag(diagnostic("未找到项目入口文件")
        .with_expected(format!(
            "「{}」或「{}」",
            root.join("主.li").display(),
            root.join("src/主.li").display()
        ))
        .with_note(
            "请在 `li.toml` 的 `[项目]` 中声明 `入口`，或创建默认入口文件",
        )))
}

/// 按常见布局列出候选入口。
fn common_entries(root: &Path) -> Vec<PathBuf> {
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    let mut candidates = vec![
        root.join("主.li"),
        root.join("src/主.li"),
        root.join("lib.li"),
        root.join("src/lib.li"),
    ];

    if !name.is_empty() {
        candidates.push(root.join(format!("{name}.li")));
        candidates.push(root.join("src").join(format!("{name}.li")));
    }

    candidates
}

/// 解析本地路径依赖；相对路径相对于项目根目录。
fn resolve_local_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// 解析 Git 地址；本地相对路径转换为相对于项目根目录的路径。
fn resolve_git_url(root: &Path, url: &str) -> String {
    if url.contains("://") || url.starts_with("git@") {
        return url.to_owned();
    }

    let path = root.join(url);
    if path.exists() {
        path.to_string_lossy().into_owned()
    } else {
        url.to_owned()
    }
}

/// 读取 Git 仓库当前提交。
fn git_head(directory: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("无法启动 git：{}", io_error_text(&error)))?;

    if !output.status.success() {
        return Err("该目录不是可用的 Git 仓库，或尚无提交".to_owned());
    }

    let commit = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if commit.is_empty() {
        return Err("Git 返回了空提交哈希".to_owned());
    }

    Ok(commit)
}

/// 递归复制依赖目录，跳过版本控制与构建产物。
fn copy_directory(
    source: &Path,
    destination: &Path,
    dependency_name: &str,
) -> Result<(), DiagnosticBag> {
    if let Err(error) = fs::create_dir_all(destination) {
        return Err(io_error(
            format!("无法创建依赖目录「{}」", destination.display()),
            &error,
        ));
    }

    let entries = match fs::read_dir(source) {
        Ok(entries) => entries,
        Err(error) => {
            return Err(io_error(
                format!("无法读取依赖目录「{}」", source.display()),
                &error,
            ));
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                return Err(io_error(
                    format!("无法读取依赖「{dependency_name}」的文件列表"),
                    &error,
                ));
            }
        };

        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if matches!(name.as_ref(), ".git" | ".li" | "target") {
            continue;
        }

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                return Err(io_error(
                    format!("无法读取依赖「{dependency_name}」的文件信息"),
                    &error,
                ));
            }
        };

        // 不跟随符号链接，避免把项目外部内容或循环目录复制进来。
        if file_type.is_symlink() {
            continue;
        }

        let source_path = entry.path();
        let destination_path = destination.join(&file_name);

        if file_type.is_dir() {
            copy_directory(&source_path, &destination_path, dependency_name)?;
        } else {
            if let Err(error) = fs::copy(&source_path, &destination_path) {
                return Err(io_error(
                    format!(
                        "无法复制依赖「{dependency_name}」的文件「{}」",
                        source_path.display()
                    ),
                    &error,
                ));
            }
        }
    }

    Ok(())
}

/// 创建一个不带源码位置的错误诊断。
fn diagnostic(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(message)
}

/// 把单条诊断包装成诊断集合。
fn bag(diagnostic: Diagnostic) -> DiagnosticBag {
    let mut diagnostics = DiagnosticBag::new();
    diagnostics.push(diagnostic);
    diagnostics
}

/// 创建输入输出错误诊断。
fn io_error(message: impl Into<String>, error: &std::io::Error) -> DiagnosticBag {
    let mut diagnostics = DiagnosticBag::new();
    diagnostics.push(diagnostic(message).with_note(io_error_text(error)));
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
