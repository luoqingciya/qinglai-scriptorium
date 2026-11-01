//! 全文检索 command。索引常驻内存，随扫描重建、随保存增量更新。

use tauri::State;

use super::vault::require_root;
use crate::engine::search::SearchHit;
use crate::AppState;

/// 检索当前稿库。多个关键词按 AND 处理。
#[tauri::command]
pub fn search_vault(
    query: String,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<SearchHit>, String> {
    // 未开库时给出明确错误，而不是返回空结果让人以为「搜不到」
    require_root(&state)?;

    let index = state
        .index
        .lock()
        .map_err(|_| "状态锁已中毒".to_string())?;

    Ok(index.search(&query, limit.unwrap_or(50)))
}
