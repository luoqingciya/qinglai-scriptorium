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
    engine_vault::delete_document(&root, &path)
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
