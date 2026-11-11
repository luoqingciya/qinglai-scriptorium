//! 导出 docx 与 epub。
//!
//! 与读写 command 不同，这里的 `dest` 是**绝对路径**：导出目标通常落在稿库之外
//! （桌面、投稿目录），不能套用 `safe_join`。路径来自前端调起的系统保存对话框，
//! 是用户显式选择的结果。
//!
//! 仍做两条底线校验：必须是 `.docx` 后缀、父目录必须已存在。
//! 这样即使用户手输路径打错，也只会得到明确报错，不会在意外位置生成文件。

use std::path::Path;

use tauri::State;

use super::vault::require_root;
use crate::engine::export::{self, ChapterInput, ExportOptions, ExportStats};
use crate::engine::search::DocInput;
use crate::engine::vault::safe_join;
use crate::AppState;

/// 导出若干章节为 docx。
///
/// `paths` 为空时导出全库（按扫描顺序）。顺序即 `paths` 的顺序，
/// 因此前端应传入当前目录树里显示的顺序。
#[tauri::command]
pub fn export_docx(
    paths: Vec<String>,
    dest: String,
    options: Option<ExportOptions>,
    state: State<'_, AppState>,
) -> Result<ExportStats, String> {
    let root = require_root(&state)?;

    if !dest.to_lowercase().ends_with(".docx") {
        return Err("导出文件必须以 .docx 结尾".to_string());
    }
    let dest_path = Path::new(&dest);
    if let Some(parent) = dest_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.is_dir() {
            return Err(format!("目标目录不存在：{}", parent.display()));
        }
    }

    // 未指定章节时导出全库，顺序取自扫描结果
    let targets = if paths.is_empty() {
        let outcome = crate::engine::vault::scan_with_docs(&root)?;
        outcome
            .snapshot
            .chapters
            .into_iter()
            .map(|c| c.path)
            .collect()
    } else {
        paths
    };

    let mut chapters = Vec::with_capacity(targets.len());
    for rel in &targets {
        let abs = safe_join(&root, rel).ok_or_else(|| format!("非法路径：{rel}"))?;
        let text = std::fs::read_to_string(&abs).map_err(|e| format!("读取 {rel} 失败：{e}"))?;
        // DocInput::from_text 已经剥掉 front matter 并解析出标题，直接复用
        let input = DocInput::from_text(rel.clone(), &text);
        chapters.push(ChapterInput {
            title: input.title,
            body: input.body,
        });
    }

    let config = crate::engine::vault::load_config(&root);
    let book_title = config.title.unwrap_or_else(|| {
        root.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "未命名".to_string())
    });

    let opts = options.unwrap_or_default();
    let (bytes, stats) = export::build(&book_title, &chapters, &opts)?;

    crate::engine::atomic::write_atomic(dest_path, &bytes)
        .map_err(|e| format!("写入 docx 失败：{e}"))?;

    Ok(stats)
}

/// 导出全库为 EPUB 3.0 电子书。
///
/// 与 docx 不同，这里不支持指定章节：成书本来就是全库，且目录需要 `dir`
/// 来还原卷层级，直接取 `scan_with_docs` 的结果最省一次磁盘遍历
/// （`docs` 里已经带 path / title / dir / body）。
#[tauri::command]
pub fn export_epub(dest: String, state: State<'_, AppState>) -> Result<ExportStats, String> {
    let root = require_root(&state)?;

    if !dest.to_lowercase().ends_with(".epub") {
        return Err("导出文件必须以 .epub 结尾".to_string());
    }
    let dest_path = Path::new(&dest);
    if let Some(parent) = dest_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.is_dir() {
            return Err(format!("目标目录不存在：{}", parent.display()));
        }
    }

    let outcome = crate::engine::vault::scan_with_docs(&root)?;
    let chapters: Vec<crate::engine::epub::EpubChapter> = outcome
        .docs
        .iter()
        .map(|d| crate::engine::epub::EpubChapter {
            title: d.title.clone(),
            dir: d.dir.clone(),
            body: d.body.clone(),
        })
        .collect();

    let config = crate::engine::vault::load_config(&root);
    let book_title = config.title.unwrap_or_else(|| {
        root.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "未命名".to_string())
    });

    let (bytes, stats) = crate::engine::epub::build(&book_title, &chapters)?;

    crate::engine::atomic::write_atomic(dest_path, &bytes)
        .map_err(|e| format!("写入 epub 失败：{e}"))?;

    Ok(stats)
}
