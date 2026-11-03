//! 字数统计与写作历史。

use chrono::Local;
use serde::Serialize;
use tauri::State;

use super::vault::require_root;
use crate::engine::md;
use crate::engine::stats as engine_stats;
use crate::AppState;

#[tauri::command]
pub fn count_words(text: String, count_punctuation: Option<bool>) -> md::WordCount {
    md::count_words(&text, count_punctuation.unwrap_or(false))
}

/// 今日记录的结果，供状态栏显示「今日 +N 字」。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayStat {
    pub date: String,
    pub words: u64,
    /// 相对上一个记录日的增量
    pub delta: i64,
}

/// 记录一次当前全库总字数。
///
/// 日期取**本地时区**：作者认知里的「今天」是墙上时钟，不是 UTC。
/// 同一天多次调用只保留最后一次，所以前端可以放心重复调用（仍建议节流）。
#[tauri::command]
pub fn record_stats(total_words: u64, state: State<'_, AppState>) -> Result<TodayStat, String> {
    let root = require_root(&state)?;

    let now = Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let at = now.to_rfc3339();

    let history = engine_stats::record(&root, &date, total_words, &at)?;
    let delta = engine_stats::today_delta(&history, &date).unwrap_or(0);

    Ok(TodayStat {
        date,
        words: total_words,
        delta,
    })
}

/// 最近若干天的写作趋势，按日期正序。
#[tauri::command]
pub fn stats_history(
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<engine_stats::TrendPoint>, String> {
    let root = require_root(&state)?;
    let history = engine_stats::load(&root);
    Ok(engine_stats::trend(&history, limit.unwrap_or(30)))
}
