//! 清籁斋 · 面向长篇小说与笔记的个人写作工作台。
//!
//! 设计前提：稿件以 Markdown + YAML front matter 存于磁盘，是唯一真相源；
//! 任何索引、缓存都视为可丢弃的派生结构。详见 `docs/architecture.md`。

mod commands;
mod engine;

use std::path::PathBuf;
use std::sync::Mutex;

/// 应用级状态：当前打开的稿库根目录与内存检索索引。
///
/// 前端不持有任意路径的读写权限，所有文件操作都经 command 并在此校验根目录边界。
#[derive(Default)]
pub struct AppState {
    pub(crate) root: Mutex<Option<PathBuf>>,
    /// 全文检索索引。纯内存、可随时重建，与「文件即真相」的前提一致。
    pub(crate) index: Mutex<engine::search::SearchIndex>,
    /// `[[双链]]` 的引用索引。同样纯内存、随扫描重建。
    pub(crate) links: Mutex<engine::links::LinkIndex>,
    /// 人物出场索引。同样纯内存、随扫描重建。
    pub(crate) characters: Mutex<engine::characters::CharacterIndex>,
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
            commands::vault::update_summary,
            commands::search::search_vault,
            commands::links::backlinks,
            commands::links::outlinks,
            commands::links::dangling_links,
            commands::links::resolve_link,
            commands::characters::list_characters,
            commands::export::export_docx,
            commands::doc::read_doc,
            commands::doc::save_doc,
            commands::doc::list_versions,
            commands::doc::read_version,
            commands::doc::restore_version,
            commands::stats::count_words,
            commands::stats::record_stats,
            commands::stats::stats_history,
        ])
        .run(tauri::generate_context!())
        .expect("清籁斋启动失败");
}
