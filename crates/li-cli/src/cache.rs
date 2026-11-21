//! 全局编译缓存与 `li 清理` 支持。
//!
//! 原生编译会产生中间文件（`.ll` 与目标文件）。把它们写在源码目录旁边会
//! 让工作区越用越脏，因此这里统一放到用户级缓存目录：
//!
//! 1. `LI_CACHE_DIR` 环境变量（便于测试与 CI 隔离）；
//! 2. `XDG_CACHE_HOME/li`；
//! 3. `$HOME/.cache/li`。
//!
//! 缓存文件名由源码内容、目标三元组与优化模式共同决定的内容哈希生成，
//! 因此相同输入会复用同一份中间产物，既能加速重复编译，也便于整体清理。

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// 缓存内容哈希使用的初始值（FNV-1a 64 位偏移基数）。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位质数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 返回全局缓存根目录；无法确定用户目录时返回 `None`。
pub fn cache_root() -> Option<PathBuf> {
    if let Some(directory) = env::var_os("LI_CACHE_DIR") {
        return Some(PathBuf::from(directory));
    }

    if let Some(directory) = env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(directory).join("li"));
    }

    env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache").join("li"))
}

/// 计算一段字节内容的 FNV-1a 64 位哈希。
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// 根据若干片段拼出稳定的缓存键（十六进制哈希文本）。
pub fn cache_key(parts: &[&str]) -> String {
    let joined = parts.join("\u{1f}");
    format!("{:016x}", content_hash(joined.as_bytes()))
}

/// 返回原生编译中间产物所在目录，并在需要时创建它。
///
/// 创建失败时返回 `None`，调用方应退回到临时目录，保证编译本身不中断。
pub fn native_cache_dir() -> Option<PathBuf> {
    let directory = cache_root()?.join("native");
    fs::create_dir_all(&directory).ok()?;
    Some(directory)
}

/// 统计目录内的文件数量与总字节数；目录不存在时返回 `(0, 0)`。
pub fn directory_usage(directory: &Path) -> (usize, u64) {
    let Ok(entries) = fs::read_dir(directory) else {
        return (0, 0);
    };

    let mut files = 0;
    let mut bytes = 0;
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            let (inner_files, inner_bytes) = directory_usage(&entry.path());
            files += inner_files;
            bytes += inner_bytes;
        } else {
            files += 1;
            bytes += metadata.len();
        }
    }

    (files, bytes)
}

/// 删除整个全局缓存，返回被清理的文件数与字节数。
///
/// 缓存目录不存在时视为已清理干净，返回 `Ok((0, 0))`。
pub fn clean() -> io::Result<(usize, u64)> {
    match cache_root() {
        Some(root) => clean_at(&root),
        None => Ok((0, 0)),
    }
}

/// 删除指定缓存目录，返回被清理的文件数与字节数。
pub fn clean_at(root: &Path) -> io::Result<(usize, u64)> {
    if !root.exists() {
        return Ok((0, 0));
    }

    let usage = directory_usage(root);
    fs::remove_dir_all(root)?;
    Ok(usage)
}

/// 把字节数格式化为便于阅读的文本，例如 `1.5 MB`。
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 创建本次测试专用的临时目录，避免与其它测试互相干扰。
    fn temp_directory(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let directory = env::temp_dir().join(format!("li-cache-{label}-{stamp}"));
        fs::create_dir_all(&directory).expect("无法创建临时目录");
        directory
    }

    #[test]
    fn content_hash_is_stable_and_content_sensitive() {
        assert_eq!(content_hash(b"abc"), content_hash(b"abc"));
        assert_ne!(content_hash(b"abc"), content_hash(b"abd"));
    }

    #[test]
    fn cache_key_separates_parts() {
        assert_eq!(
            cache_key(&["甲", "乙"]),
            cache_key(&["甲", "乙"]),
            "相同输入必须生成相同键"
        );
        assert_ne!(
            cache_key(&["甲乙"]),
            cache_key(&["甲", "乙"]),
            "分隔符必须防止片段串接歧义"
        );
    }

    #[test]
    fn directory_usage_and_clean_report_freed_space() {
        let root = temp_directory("usage");
        let nested = root.join("native");
        fs::create_dir_all(&nested).expect("无法创建子目录");
        fs::write(root.join("甲.ll"), b"12345").expect("无法写入文件");
        fs::write(nested.join("乙.ll"), b"123").expect("无法写入文件");

        let (files, bytes) = directory_usage(&root);
        assert_eq!(files, 2, "应统计两层目录中的文件");
        assert_eq!(bytes, 8, "应累计所有文件字节数");

        let (removed_files, removed_bytes) = clean_at(&root).expect("清理应成功");
        assert_eq!(removed_files, 2);
        assert_eq!(removed_bytes, 8);
        assert!(!root.exists(), "清理后缓存目录应被删除");

        let (files, bytes) = clean_at(&root).expect("重复清理应成功");
        assert_eq!((files, bytes), (0, 0), "目录不存在时应返回零");
    }

    #[test]
    fn formats_sizes_in_readable_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }
}
