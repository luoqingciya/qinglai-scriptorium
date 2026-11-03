//! 文档读取与保存。
//!
//! 保存路径的核心原则：**只替换正文，front matter 字节不动**。
//! 这样无论 YAML 序列化器如何演进，用户手写的元数据都不会漂移。

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
