//! 版本快照：把被覆盖的内容存起来，随时可以退回去。
//!
//! ## 为什么需要
//!
//! 现有三层保护都只防「写坏」，不防「改错」：
//! 原子写防半截文件，崩溃日志防未落盘，`.trash/` 防误删。
//! 但「我昨天那段写得更好，改回去」没有退路。
//!
//! ## 存的是「被覆盖的内容」，不是「新内容」
//!
//! 保存前把**即将被覆盖的旧正文**存为快照。这样：
//!
//! - 当前内容就是磁盘上的文件，不需要重复存一份；
//! - 历史 = 所有曾经存在过、后来被覆盖的版本，语义干净；
//! - 恢复 = 从历史取一版写回，写回前把当前内容也存一档（避免恢复后后悔）。
//!
//! ## 5 分钟节流
//!
//! 自动保存每 800ms 就可能触发一次。不节流的话，一天 8 小时能产生上千个快照，
//! manifest 会膨胀到几百 KB，而其中绝大多数差异只有几个字符。
//!
//! 5 分钟意味着一天最多 96 个快照/章，够用且不臃肿。
//!
//! ## 内容寻址
//!
//! blob 按 sha256 存，相同内容只占一份。改回去再改回来不会产生两份副本。
//!
//! 存放位置：`.qinglai/versions/`，与写作统计同属「不可重建的历史数据」。
//! 丢了不影响稿件，只是失去退回能力。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 版本目录（稿库内隐藏目录，扫描时被跳过）
pub const VERSIONS_DIR: &str = ".qinglai/versions";
const BLOBS_DIR: &str = "blobs";
const MANIFEST_FILE: &str = "manifest.json";

/// 同一章两次快照的最小间隔（秒）
const MIN_INTERVAL_SECS: i64 = 300;

/// 每章保留的快照数，超出后丢最老的
const KEEP_PER_FILE: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// 记录时刻（RFC 3339，本地时区）
    pub at: String,
    /// 内容 sha256（十六进制）
    pub hash: String,
    /// 内容字节数，供 UI 显示
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    /// 相对路径 → 快照列表（时间正序，末尾最新）
    pub files: BTreeMap<String, Vec<Snapshot>>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: 1,
            files: BTreeMap::new(),
        }
    }
}

pub fn sha256_hex(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

fn versions_root(root: &Path) -> PathBuf {
    root.join(VERSIONS_DIR)
}

fn blob_path(root: &Path, hash: &str) -> PathBuf {
    // 按前两位分桶，避免单目录堆积上万个文件
    let bucket = hash.get(0..2).unwrap_or("00");
    versions_root(root).join(BLOBS_DIR).join(bucket).join(hash)
}

/// 读取 manifest。文件不存在或损坏都退化为空 —— 版本历史是附加能力，
/// 任何情况下都不该阻断稿件读写。
pub fn load_manifest(root: &Path) -> Manifest {
    let Ok(text) = std::fs::read_to_string(versions_root(root).join(MANIFEST_FILE)) else {
        return Manifest::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_manifest(root: &Path, manifest: &Manifest) -> Result<(), String> {
    let dir = versions_root(root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建版本目录失败：{e}"))?;

    let text = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("序列化版本清单失败：{e}"))?;

    crate::engine::atomic::write_atomic(&dir.join(MANIFEST_FILE), text.as_bytes())
        .map_err(|e| format!("写入版本清单失败：{e}"))
}

fn write_blob(root: &Path, hash: &str, content: &[u8]) -> Result<(), String> {
    let path = blob_path(root, hash);
    if path.is_file() {
        return Ok(()); // 内容寻址：同一内容只存一份
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建版本分桶失败：{e}"))?;
    }

    crate::engine::atomic::write_atomic(&path, content)
        .map_err(|e| format!("写入版本内容失败：{e}"))
}

/// 读取某个版本的内容。
pub fn read_version(root: &Path, hash: &str) -> Option<String> {
    let path = blob_path(root, hash);
    std::fs::read_to_string(path).ok()
}

/// 记录一次快照。`content` 是**即将被覆盖的旧正文**。
///
/// 返回 `None` 表示本次跳过（距上次不足 5 分钟，或内容与上次快照完全相同）。
pub fn record(
    root: &Path,
    path: &str,
    content: &str,
    now: DateTime<Local>,
) -> Result<Option<Snapshot>, String> {
    record_inner(root, path, content, now, false)
}

/// 强制记录，跳过节流。
///
/// 用于「恢复前先把当前内容存一档」：此时用户刚点下恢复，如果因为 5 分钟节流
/// 没存上，恢复后就再也找不回被覆盖的那一版了。
///
/// 内容去重仍然生效 —— 内容没变就没必要记第二次。
pub fn record_forced(
    root: &Path,
    path: &str,
    content: &str,
    now: DateTime<Local>,
) -> Result<Option<Snapshot>, String> {
    record_inner(root, path, content, now, true)
}

fn record_inner(
    root: &Path,
    path: &str,
    content: &str,
    now: DateTime<Local>,
    force: bool,
) -> Result<Option<Snapshot>, String> {
    let mut manifest = load_manifest(root);
    let entries = manifest.files.entry(path.to_string()).or_default();

    let hash = sha256_hex(content.as_bytes());

    if let Some(last) = entries.last() {
        // 内容没变就不必记第二次
        if last.hash == hash {
            return Ok(None);
        }

        // 距上次快照不足间隔，跳过（强制记录时不受此限）
        if !force {
            if let Ok(last_at) = DateTime::parse_from_rfc3339(&last.at) {
                let elapsed = now.signed_duration_since(last_at.with_timezone(&Local));
                if elapsed.num_seconds() < MIN_INTERVAL_SECS {
                    return Ok(None);
                }
            }
        }
    }

    write_blob(root, &hash, content.as_bytes())?;

    let snapshot = Snapshot {
        at: now.to_rfc3339(),
        hash,
        bytes: content.len(),
    };
    entries.push(snapshot.clone());

    // 超出上限时丢最老的
    let trimmed = if entries.len() > KEEP_PER_FILE {
        let drop_count = entries.len() - KEEP_PER_FILE;
        entries.drain(0..drop_count);
        true
    } else {
        false
    };

    save_manifest(root, &manifest)?;

    // 只有真的丢了条目才 GC，避免每次保存都遍历全部 blob
    if trimmed {
        gc(root, &manifest);
    }

    Ok(Some(snapshot))
}

/// 某章的快照列表，最新的在前（UI 直接按此顺序展示）。
pub fn list(root: &Path, path: &str) -> Vec<Snapshot> {
    let manifest = load_manifest(root);
    let mut entries = manifest.files.get(path).cloned().unwrap_or_default();
    entries.reverse();
    entries
}

/// 删除所有未被 manifest 引用的 blob。
///
/// 只有在 manifest 已落盘之后调用才安全：先写完清单再删内容，
/// 中途崩溃最多留下孤儿 blob，不会丢引用。
fn gc(root: &Path, manifest: &Manifest) {
    let mut referenced: BTreeSet<&str> = BTreeSet::new();
    for entries in manifest.files.values() {
        for snapshot in entries {
            referenced.insert(snapshot.hash.as_str());
        }
    }

    let blobs_root = versions_root(root).join(BLOBS_DIR);
    let Ok(buckets) = std::fs::read_dir(&blobs_root) else {
        return;
    };

    for bucket in buckets.flatten() {
        let Ok(files) = std::fs::read_dir(bucket.path()) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().to_string();
            if !referenced.contains(name.as_str()) {
                // 删不掉也不算错，下次 GC 还会遇到
                let _ = std::fs::remove_file(file.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn temp_root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn at(secs: i64) -> DateTime<Local> {
        Local.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn 记录后能读回内容() {
        let dir = temp_root();
        let snap = record(dir.path(), "a.md", "第一版正文", at(0)).unwrap().unwrap();

        assert_eq!(read_version(dir.path(), &snap.hash).as_deref(), Some("第一版正文"));
    }

    #[test]
    fn 首次记录无需等待间隔() {
        let dir = temp_root();
        let snap = record(dir.path(), "a.md", "正文", at(0)).unwrap();
        assert!(snap.is_some());
    }

    #[test]
    fn 间隔内的第二次记录被跳过() {
        let dir = temp_root();
        record(dir.path(), "a.md", "第一版", at(0)).unwrap();
        let second = record(dir.path(), "a.md", "第二版", at(60)).unwrap();

        assert!(second.is_none(), "5 分钟内不该再记");
        assert_eq!(list(dir.path(), "a.md").len(), 1);
    }

    #[test]
    fn 超过间隔后记录() {
        let dir = temp_root();
        record(dir.path(), "a.md", "第一版", at(0)).unwrap();
        let second = record(dir.path(), "a.md", "第二版", at(301)).unwrap();

        assert!(second.is_some());
        assert_eq!(list(dir.path(), "a.md").len(), 2);
    }

    #[test]
    fn 内容未变不重复记录() {
        let dir = temp_root();
        record(dir.path(), "a.md", "同样的正文", at(0)).unwrap();
        // 时间已过间隔，但内容没变
        let second = record(dir.path(), "a.md", "同样的正文", at(1000)).unwrap();

        assert!(second.is_none());
        assert_eq!(list(dir.path(), "a.md").len(), 1);
    }

    #[test]
    fn 相同内容只占一份_blob() {
        let dir = temp_root();
        record(dir.path(), "a.md", "相同", at(0)).unwrap();
        record(dir.path(), "b.md", "相同", at(0)).unwrap();

        let blobs: Vec<_> = walkdir::WalkDir::new(versions_root(dir.path()).join(BLOBS_DIR))
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .collect();
        assert_eq!(blobs.len(), 1, "内容寻址应去重");
    }

    #[test]
    fn 列表按时间倒序() {
        let dir = temp_root();
        record(dir.path(), "a.md", "第一版", at(0)).unwrap();
        record(dir.path(), "a.md", "第二版", at(400)).unwrap();
        record(dir.path(), "a.md", "第三版", at(800)).unwrap();

        let entries = list(dir.path(), "a.md");
        assert_eq!(entries.len(), 3);
        // 最新的在前
        assert_eq!(read_version(dir.path(), &entries[0].hash).as_deref(), Some("第三版"));
        assert_eq!(read_version(dir.path(), &entries[2].hash).as_deref(), Some("第一版"));
    }

    #[test]
    fn 不同章节互不干扰() {
        let dir = temp_root();
        record(dir.path(), "a.md", "甲的正文", at(0)).unwrap();
        record(dir.path(), "b.md", "乙的正文", at(0)).unwrap();

        assert_eq!(list(dir.path(), "a.md").len(), 1);
        assert_eq!(list(dir.path(), "b.md").len(), 1);
    }

    #[test]
    fn 超出上限时丢最老的并清理_blob() {
        let dir = temp_root();
        for i in 0..(KEEP_PER_FILE + 5) {
            record(
                dir.path(),
                "a.md",
                &format!("第 {i} 版"),
                at(i as i64 * 400),
            )
            .unwrap();
        }

        let entries = list(dir.path(), "a.md");
        assert_eq!(entries.len(), KEEP_PER_FILE);

        // 最老的 5 个内容应已被 GC
        for i in 0..5 {
            let hash = sha256_hex(format!("第 {i} 版").as_bytes());
            assert!(
                read_version(dir.path(), &hash).is_none(),
                "第 {i} 版应已被清理"
            );
        }

        // 最新的还在
        let newest = sha256_hex(format!("第 {} 版", KEEP_PER_FILE + 4).as_bytes());
        assert!(read_version(dir.path(), &newest).is_some());
    }

    #[test]
    fn 清单损坏时退化为空且不崩溃() {
        let dir = temp_root();
        std::fs::create_dir_all(versions_root(dir.path())).unwrap();
        std::fs::write(versions_root(dir.path()).join(MANIFEST_FILE), "{ 不是 json").unwrap();

        assert!(list(dir.path(), "a.md").is_empty());
        // 仍能继续记录
        let snap = record(dir.path(), "a.md", "正文", at(0)).unwrap();
        assert!(snap.is_some());
    }

    #[test]
    fn 版本目录不影响章节扫描() {
        let dir = temp_root();
        std::fs::write(dir.path().join("001-楔子.md"), "---\ntitle: 楔子\n---\n正文\n").unwrap();
        record(dir.path(), "001-楔子.md", "旧正文", at(0)).unwrap();

        let snapshot = crate::engine::vault::scan_with_docs(dir.path()).unwrap().snapshot;
        assert_eq!(snapshot.chapters.len(), 1);
    }

    #[test]
    fn 不存在的版本返回_none() {
        let dir = temp_root();
        assert!(read_version(dir.path(), "deadbeef").is_none());
    }

    #[test]
    fn 强制记录跳过节流() {
        let dir = temp_root();
        record(dir.path(), "a.md", "第一版", at(0)).unwrap();

        // 普通记录会被 5 分钟节流挡住
        assert!(record(dir.path(), "a.md", "第二版", at(10)).unwrap().is_none());
        // 强制记录不受限
        assert!(record_forced(dir.path(), "a.md", "第二版", at(10)).unwrap().is_some());
        assert_eq!(list(dir.path(), "a.md").len(), 2);
    }

    #[test]
    fn 强制记录仍做内容去重() {
        let dir = temp_root();
        record(dir.path(), "a.md", "同样的内容", at(0)).unwrap();

        // 内容没变，强制也不记
        assert!(record_forced(dir.path(), "a.md", "同样的内容", at(10))
            .unwrap()
            .is_none());
        assert_eq!(list(dir.path(), "a.md").len(), 1);
    }
}
