//! 文档读取与保存。
//!
//! 保存路径的核心原则：**只替换正文，front matter 字节不动**。
//! 这样无论 YAML 序列化器如何演进，用户手写的元数据都不会漂移。

use chrono::Local;
use serde::Serialize;
use tauri::State;

use super::vault::require_root;
use crate::engine::atomic::{self, FileStamp};
use crate::engine::md;
use crate::engine::search as engine_search;
use crate::engine::vault::{self as engine_vault, safe_join};
use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocPayload {
    pub path: String,
    pub body: String,
    /// front matter 解析结果，解析失败或无元数据时为 None
    pub front: Option<md::FrontMatter>,
    pub has_front_matter: bool,
    pub stamp: FileStamp,
    pub words: md::WordCount,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SaveOutcome {
    /// 已落盘
    Saved { stamp: FileStamp, words: md::WordCount },
    /// 磁盘文件已被外部修改，拒绝覆盖
    Conflict { stamp: FileStamp },
    /// 内容与磁盘一致，跳过写入
    Unchanged { stamp: FileStamp },
}

/// 读取一篇文档。
#[tauri::command]
pub fn read_doc(path: String, state: State<'_, AppState>) -> Result<DocPayload, String> {
    let root = require_root(&state)?;
    let abs = safe_join(&root, &path).ok_or_else(|| format!("非法路径：{path}"))?;

    let text = std::fs::read_to_string(&abs).map_err(|e| format!("读取失败：{e}"))?;
    let split = md::split(&text);
    let config = engine_vault::load_config(&root);
    let stamp = FileStamp::of(&abs).map_err(|e| format!("读取文件戳失败：{e}"))?;

    Ok(DocPayload {
        path,
        body: split.body.to_string(),
        front: split.yaml.and_then(md::parse_front_matter),
        has_front_matter: split.yaml.is_some(),
        stamp,
        words: md::count_words(split.body, config.count_punctuation),
    })
}

/// 保存一篇文档。
///
/// `expected` 是前端上次读到的文件戳。若与磁盘当前状态不一致，说明文件被外部编辑器
/// 改过，此时返回 `Conflict` 而不写入 —— 宁可不保存，也不静默覆盖。
#[tauri::command]
pub fn save_doc(
    path: String,
    body: String,
    expected: Option<FileStamp>,
    state: State<'_, AppState>,
) -> Result<SaveOutcome, String> {
    let root = require_root(&state)?;
    let abs = safe_join(&root, &path).ok_or_else(|| format!("非法路径：{path}"))?;

    let original = std::fs::read_to_string(&abs).map_err(|e| format!("读取原文件失败：{e}"))?;
    let current = FileStamp::of(&abs).map_err(|e| format!("读取文件戳失败：{e}"))?;

    if let Some(expected) = &expected {
        if expected != &current {
            return Ok(SaveOutcome::Conflict { stamp: current });
        }
    }

    let split = md::split(&original);
    if split.body == body {
        return Ok(SaveOutcome::Unchanged { stamp: current });
    }

    let next = md::replace_body(&original, &body);

    // 快照存的是「即将被覆盖的旧正文」。失败不阻断保存：
    // 内容安全优先于历史记录，历史只是附加能力。
    let _ = crate::engine::versions::record(&root, &path, split.body, Local::now());

    atomic::write_atomic(&abs, next.as_bytes()).map_err(|e| format!("写入失败：{e}"))?;

    // 两个索引都做增量更新，否则刚写完就搜索/查反链会读到旧内容。
    // 共用同一份 DocInput，避免重复解析 front matter。
    let input = engine_search::DocInput::from_text(path.clone(), &next);
    if let Ok(mut index) = state.index.lock() {
        index.upsert(input.clone());
    }
    if let Ok(mut links) = state.links.lock() {
        links.upsert(&input);
    }
    if let Ok(mut characters) = state.characters.lock() {
        characters.upsert(&input);
    }

    let stamp = FileStamp::of(&abs).map_err(|e| format!("写入后读取文件戳失败：{e}"))?;
    let config = engine_vault::load_config(&root);
    Ok(SaveOutcome::Saved {
        stamp,
        words: md::count_words(&body, config.count_punctuation),
    })
}

/// 列出某章的历史版本，最新的在前。
#[tauri::command]
pub fn list_versions(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<crate::engine::versions::Snapshot>, String> {
    let root = require_root(&state)?;
    // 只校验路径合法性，不要求文件当前存在（已删除的章也可能有历史）
    crate::engine::vault::safe_join(&root, &path)
        .ok_or_else(|| format!("非法路径：{path}"))?;

    Ok(crate::engine::versions::list(&root, &path))
}

/// 读取某个版本的内容，供预览。
#[tauri::command]
pub fn read_version(hash: String, state: State<'_, AppState>) -> Result<String, String> {
    let root = require_root(&state)?;
    crate::engine::versions::read_version(&root, &hash)
        .ok_or_else(|| format!("找不到版本 {hash}，可能已被清理"))
}

/// 恢复到某个历史版本，返回恢复后的正文。
///
/// 恢复前会把**当前正文强制存一档**（不受 5 分钟节流限制），
/// 所以「恢复错了」还能再退回来。
///
/// 只替换正文，front matter 保留当前值 —— `id`、`order` 这些由系统维护，
/// 跟着回退反而会出问题。
#[tauri::command]
pub fn restore_version(
    path: String,
    hash: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let root = require_root(&state)?;
    let abs = crate::engine::vault::safe_join(&root, &path)
        .ok_or_else(|| format!("非法路径：{path}"))?;

    let content = crate::engine::versions::read_version(&root, &hash)
        .ok_or_else(|| format!("找不到版本 {hash}，可能已被清理"))?;

    let original = std::fs::read_to_string(&abs).map_err(|e| format!("读取原文件失败：{e}"))?;
    let split = md::split(&original);

    // 先存当前内容，再覆盖。顺序不能反：反了就是「先毁掉再备份」。
    let _ = crate::engine::versions::record_forced(&root, &path, split.body, Local::now());

    let next = md::replace_body(&original, &content);
    atomic::write_atomic(&abs, next.as_bytes()).map_err(|e| format!("写入失败：{e}"))?;

    // 索引跟着更新，否则恢复后搜索与反链仍是旧内容
    let input = engine_search::DocInput::from_text(path.clone(), &next);
    if let Ok(mut index) = state.index.lock() {
        index.upsert(input.clone());
    }
    if let Ok(mut links) = state.links.lock() {
        links.upsert(&input);
    }
    if let Ok(mut characters) = state.characters.lock() {
        characters.upsert(&input);
    }

    Ok(content)
}
