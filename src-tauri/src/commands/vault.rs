//! 稿库相关的 command。

use std::path::{Path, PathBuf};

use tauri::State;

use crate::engine::characters::CharacterIndex;
use crate::engine::links::LinkIndex;
use crate::engine::search::SearchIndex;
use crate::engine::vault as engine_vault;
use crate::AppState;

/// 取出当前稿库根目录，未打开时给出明确错误而不是 panic。
pub fn require_root(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let guard = state.root.lock().map_err(|_| "状态锁已中毒".to_string())?;
    guard.clone().ok_or_else(|| "尚未打开稿库".to_string())
}

/// 扫描稿库并重建内存索引。检索与双链共用同一次磁盘遍历。
fn scan_and_index(
    root: &Path,
    state: &State<'_, AppState>,
) -> Result<engine_vault::VaultSnapshot, String> {
    let outcome = engine_vault::scan_with_docs(root)?;

    // 双链与人物索引先建：它们借用 docs，检索索引最后 move 走，省一次正文克隆
    {
        let mut links = state
            .links
            .lock()
            .map_err(|_| "状态锁已中毒".to_string())?;
        *links = LinkIndex::from_docs(&outcome.docs);
    }

    {
        let mut characters = state
            .characters
            .lock()
            .map_err(|_| "状态锁已中毒".to_string())?;
        *characters = CharacterIndex::from_docs(&outcome.docs);
    }

    {
        let mut index = state
            .index
            .lock()
            .map_err(|_| "状态锁已中毒".to_string())?;
        *index = SearchIndex::from_docs(outcome.docs);
    }

    Ok(outcome.snapshot)
}

/// 打开稿库：记录根目录并立即扫描一次。
#[tauri::command]
pub fn open_vault(
    path: String,
    state: State<'_, AppState>,
) -> Result<engine_vault::VaultSnapshot, String> {
    let candidate = PathBuf::from(&path);
    if !candidate.is_dir() {
        return Err(format!("不是有效目录：{path}"));
    }
    // canonicalize 消掉 `..` 与符号链接，后续 safe_join 才有意义
    let root = candidate
        .canonicalize()
        .map_err(|e| format!("无法解析目录 {path}：{e}"))?;

    {
        let mut guard = state.root.lock().map_err(|_| "状态锁已中毒".to_string())?;
        *guard = Some(root.clone());
    }

    scan_and_index(&root, &state)
}

/// 重新扫描当前稿库。
#[tauri::command]
pub fn scan_vault(state: State<'_, AppState>) -> Result<engine_vault::VaultSnapshot, String> {
    let root = require_root(&state)?;
    scan_and_index(&root, &state)
}

/// 在指定目录下新建一章，返回新文件的相对路径。
#[tauri::command]
pub fn create_doc(
    dir: String,
    title: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let root = require_root(&state)?;
    engine_vault::create_document(&root, &dir, &title)
}

/// 重命名一章，返回重命名后的相对路径（文件名可能变化）。
#[tauri::command]
pub fn rename_doc(
    path: String,
    title: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let root = require_root(&state)?;
    engine_vault::rename_document(&root, &path, &title)
}

/// 删除一章（移入稿库内 `.trash/`），返回回收站中的相对路径。
#[tauri::command]
pub fn delete_doc(path: String, state: State<'_, AppState>) -> Result<String, String> {
    let root = require_root(&state)?;
    let moved = engine_vault::delete_document(&root, &path)?;

    // 前端随后会 refresh 全量重建，但这里先摘掉边，避免「删除到重建之间」
    // 这一小段窗口里反链仍指向已不存在的章节。
    if let Ok(mut links) = state.links.lock() {
        links.forget(&path);
    }
    if let Ok(mut characters) = state.characters.lock() {
        characters.forget(&path);
    }

    Ok(moved)
}

/// 同目录内上移或下移一章，返回发生的路径变化（前端据此修正当前打开的文件）。
#[tauri::command]
pub fn reorder_doc(
    path: String,
    direction: engine_vault::Direction,
    state: State<'_, AppState>,
) -> Result<Vec<engine_vault::PathRename>, String> {
    let root = require_root(&state)?;
    engine_vault::reorder_document(&root, &path, direction)
}

/// 改写一章的情节摘要。
///
/// 只替换 front matter 里的 `summary` 一行，正文与其他元数据字节不动
/// （走 `md::set_scalar`，与改标题同一套机制）。
///
/// 空串表示「清空摘要」：字段会留下 `summary: ""`，扫描时归一成 `None`，
/// UI 显示为「未写摘要」。不删行是有意的——删行需要另一套逻辑，
/// 而空值在语义上已经足够。
#[tauri::command]
pub fn update_summary(
    path: String,
    summary: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let root = require_root(&state)?;
    let abs = crate::engine::vault::safe_join(&root, &path)
        .ok_or_else(|| format!("非法路径：{path}"))?;

    let original = std::fs::read_to_string(&abs).map_err(|e| format!("读取失败：{e}"))?;
    let next = crate::engine::md::set_scalar(&original, "summary", summary.trim());

    if next == original {
        return Ok(());
    }

    crate::engine::atomic::write_atomic(&abs, next.as_bytes())
        .map_err(|e| format!("写入失败：{e}"))?;

    Ok(())
}
