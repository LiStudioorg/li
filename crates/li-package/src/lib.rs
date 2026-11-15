//! li 语言的项目配置、依赖获取与模块链接。
//!
//! 本 crate 负责“里程碑 14”的项目级能力：
//! - 解析项目根目录的 [`MANIFEST_FILE`]（`li.toml`）；
//! - 读取与生成 [`LOCK_FILE`]（`li.lock`），锁定精确来源与提交；
//! - 把本地路径与 Git 依赖安装到 [`.li/依赖`](DEPENDENCY_DIR)；
//! - 在语义检查前把 `导入 包名;` 解析为独立的模块树并合并进 AST。
//!
//! 所有诊断均为中文，配置文件格式错误会尽量附带行列位置。

mod fetch;
mod lockfile;
mod manifest;
mod resolve;
mod toml_compat;

pub use fetch::{ensure_ready, fetch, project_entry, FetchResult, FetchedDependency};
pub use lockfile::{
    LockedPackage, LockedSource, Lockfile, DEPENDENCY_DIR, LOCK_FILE, LOCK_VERSION,
};
pub use manifest::{
    is_valid_dependency_name, Dependency, DependencySource, Manifest, ProjectInfo, MANIFEST_FILE,
};
pub use resolve::{find_project_root, has_imports, prepare_compile};
