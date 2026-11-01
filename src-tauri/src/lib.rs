//! 清籁斋 · 面向长篇小说与笔记的个人写作工作台。
//!
//! 设计前提：稿件以 Markdown + YAML front matter 存于磁盘，是唯一真相源；
//! 任何索引、缓存都视为可丢弃的派生结构。详见 `docs/architecture.md`。

mod commands;
mod engine;

use std::path::PathBuf;
use std::sync::Mutex;

/// 应用级状态：当前打开的稿库根目录。
///
/// 前端不持有任意路径的读写权限，所有文件操作都经 command 并在此校验根目录边界。
#[derive(Default)]
pub struct AppState {
    pub root: Mutex<Option<PathBuf>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::vault::open_vault,
            commands::vault::scan_vault,
            commands::vault::create_doc,
            commands::vault::rename_doc,
            commands::vault::delete_doc,
            commands::vault::reorder_doc,
            commands::doc::read_doc,
            commands::doc::save_doc,
            commands::stats::count_words,
        ])
        .run(tauri::generate_context!())
        .expect("清籁斋启动失败");
}
