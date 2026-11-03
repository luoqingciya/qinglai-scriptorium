//! `[[双链]]` 相关 command。
//!
//! 索引常驻内存，随扫描重建、随保存增量更新。这里只做查询，
//! 不做任何文件写入 —— 双链是派生信息，改不了稿件。

use tauri::State;

use super::vault::require_root;
use crate::engine::links::{BacklinkHit, DanglingLink};
use crate::AppState;

/// 查询某章的反链：哪些章节引用了它。
#[tauri::command]
pub fn backlinks(path: String, state: State<'_, AppState>) -> Result<Vec<BacklinkHit>, String> {
    require_root(&state)?;

    let links = state
        .links
        .lock()
        .map_err(|_| "状态锁已中毒".to_string())?;

    Ok(links.backlinks(&path))
}

/// 查询某章指向了哪些标题（去重，保持首次出现顺序）。
#[tauri::command]
pub fn outlinks(path: String, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    require_root(&state)?;

    let links = state
        .links
        .lock()
        .map_err(|_| "状态锁已中毒".to_string())?;

    Ok(links.outlinks(&path))
}

/// 全库悬空链接：指向了不存在的章节。
#[tauri::command]
pub fn dangling_links(state: State<'_, AppState>) -> Result<Vec<DanglingLink>, String> {
    require_root(&state)?;

    let links = state
        .links
        .lock()
        .map_err(|_| "状态锁已中毒".to_string())?;

    Ok(links.dangling())
}

/// 把 `[[标题]]` 解析成章节路径，供编辑器内点击跳转。
///
/// 找不到时返回 `None`，前端据此提示「目标不存在」而不是静默无反应。
#[tauri::command]
pub fn resolve_link(target: String, state: State<'_, AppState>) -> Result<Option<String>, String> {
    require_root(&state)?;

    let links = state
        .links
        .lock()
        .map_err(|_| "状态锁已中毒".to_string())?;

    Ok(links.resolve(&target))
}
