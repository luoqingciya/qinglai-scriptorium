//! 人物索引 command。
//!
//! 一次返回全部角色及其出场章节。角色通常几十个、每个出场几章，
//! 整份数据不到 100KB —— 一次拿全比让前端按需二次请求更简单，
//! 点击角色时也能立即展开，没有等待。

use tauri::State;

use super::vault::require_root;
use crate::engine::characters::CharacterCard;
use crate::AppState;

/// 全部角色，按出场章数降序。
#[tauri::command]
pub fn list_characters(state: State<'_, AppState>) -> Result<Vec<CharacterCard>, String> {
    require_root(&state)?;

    let index = state
        .characters
        .lock()
        .map_err(|_| "状态锁已中毒".to_string())?;

    Ok(index.list())
}
