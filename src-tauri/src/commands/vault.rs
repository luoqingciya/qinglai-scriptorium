//! 稿库相关的 command。

use std::path::PathBuf;

use tauri::State;

use crate::engine::vault as engine_vault;
use crate::AppState;

/// 取出当前稿库根目录，未打开时给出明确错误而不是 panic。
pub fn require_root(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let guard = state.root.lock().map_err(|_| "状态锁已中毒".to_string())?;
    guard.clone().ok_or_else(|| "尚未打开稿库".to_string())
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

    engine_vault::scan(&root)
}

/// 重新扫描当前稿库。
#[tauri::command]
pub fn scan_vault(state: State<'_, AppState>) -> Result<engine_vault::VaultSnapshot, String> {
    let root = require_root(&state)?;
    engine_vault::scan(&root)
}
