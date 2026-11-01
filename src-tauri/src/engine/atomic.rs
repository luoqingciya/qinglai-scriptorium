//! 原子写与文件戳。
//!
//! 「写临时文件 → fsync → rename」保证任何时刻崩溃都不会留下半截文件。
//! 文件戳用于检测磁盘文件是否被外部编辑器改动，避免静默覆盖。

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum AtomicError {
    #[error("写入失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("目标路径没有父目录: {0}")]
    NoParent(PathBuf),
}

/// 文件指纹：修改时间 + 字节长度。用于冲突检测。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    pub mtime_ms: i64,
    pub len: u64,
}

impl FileStamp {
    pub fn of(path: &Path) -> std::io::Result<Self> {
        let meta = fs::metadata(path)?;
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Ok(Self {
            mtime_ms,
            len: meta.len(),
        })
    }
}

/// 原子写。目标文件要么是旧内容，要么是新内容，不存在中间态。
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), AtomicError> {
    let parent = path
        .parent()
        .ok_or_else(|| AtomicError::NoParent(path.to_path_buf()))?;
    fs::create_dir_all(parent)?;

    let tmp = parent.join(format!(".{}.tmp", unique_suffix(path)));
    {
        let mut file = File::create(&tmp)?;
        file.write_all(contents)?;
        // 必须在 rename 之前落盘，否则断电后可能拿到空文件
        file.sync_all()?;
    }

    // Windows 上 std::fs::rename 使用 MoveFileEx + REPLACE_EXISTING，可覆盖已存在的目标
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(err.into())
        }
    }
}

fn unique_suffix(path: &Path) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "doc".to_string());
    format!("{}-{}-{}-{}", name, std::process::id(), nanos, seq)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 写入后可读回相同内容() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("章节.md");
        write_atomic(&path, "正文第一段。\n".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "正文第一段。\n");
    }

    #[test]
    fn 覆盖写入不残留临时文件() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("章节.md");
        write_atomic(&path, "第一版".as_bytes()).unwrap();
        write_atomic(&path, "第二版".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "第二版");

        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件");
    }

    #[test]
    fn 自动创建父目录() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("卷一·听风").join("001-楔子.md");
        write_atomic(&path, "正文".as_bytes()).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn 文件戳在内容变化后不同() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        write_atomic(&path, "短".as_bytes()).unwrap();
        let before = FileStamp::of(&path).unwrap();
        write_atomic(&path, "长一些的内容".as_bytes()).unwrap();
        let after = FileStamp::of(&path).unwrap();
        assert_ne!(before, after, "内容变化后文件戳必须不同");
    }

    #[test]
    fn 未变化时文件戳相同() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        write_atomic(&path, "稳定".as_bytes()).unwrap();
        assert_eq!(FileStamp::of(&path).unwrap(), FileStamp::of(&path).unwrap());
    }

    #[test]
    fn 中文文件名可用() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("卷一·听风").join("楔子（初稿）.md");
        write_atomic(&path, "清籁斋".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "清籁斋");
    }
}
