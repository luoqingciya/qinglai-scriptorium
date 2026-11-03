//! 写作统计历史。
//!
//! ## 这是「索引不落盘」原则的一个例外
//!
//! 检索索引、双链索引都能从当前文件全量重建，所以不落盘。但**每日字数**是时间序列，
//! 一旦不记录就永久丢失 —— 当前稿件里没有「昨天有多少字」这个信息。
//!
//! 因此统计必须持久化。为了不违背「文件即真相」，它被放在稿库内的
//! `.qinglai/stats.json`：
//!
//! - 属于稿库的一部分，换机器打开同一稿库能看到同样的曲线；
//! - 隐藏目录，扫描时被跳过，不会混进章节列表；
//! - **丢了不影响任何稿件**，最坏情况是趋势图从头开始。
//!
//! ## 记录什么
//!
//! 每日**最后一次**的全库总字数，而不是「当日新增」。因为删改会让字数下降，
//! 记增量会出现负数累积，反而失真。存总量、由相邻两日求差，才是作者真正关心的
//! 「这本书写到多少字了」。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// 统计目录（稿库内隐藏目录）
pub const STATS_DIR: &str = ".qinglai";
pub const STATS_FILE: &str = "stats.json";

/// 保留天数。两年足够看出创作节奏，又不至于让文件无限增长。
const KEEP_DAYS: usize = 730;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    /// 当日最后一次记录的全库总字数
    pub words: u64,
    /// 记录时刻（RFC 3339，本地时区）
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsHistory {
    pub version: u32,
    /// 日期（`YYYY-MM-DD`）→ 当日统计。BTreeMap 保证按日期有序。
    pub days: BTreeMap<String, DayStat>,
}

impl Default for StatsHistory {
    fn default() -> Self {
        Self {
            version: 1,
            days: BTreeMap::new(),
        }
    }
}

/// 趋势图上的一个点。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendPoint {
    pub date: String,
    pub words: u64,
    /// 相对**上一个有记录的日子**的字数变化，可为负（删改）
    pub delta: i64,
}

fn stats_path(root: &Path) -> std::path::PathBuf {
    root.join(STATS_DIR).join(STATS_FILE)
}

/// 读取统计历史。
///
/// 文件不存在、损坏、版本不认识，一律退化为空历史 —— 统计是纯附加信息，
/// 任何情况下都不该阻断应用启动。
pub fn load(root: &Path) -> StatsHistory {
    let Ok(text) = std::fs::read_to_string(stats_path(root)) else {
        return StatsHistory::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save(root: &Path, history: &StatsHistory) -> Result<(), String> {
    let dir = root.join(STATS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建统计目录失败：{e}"))?;

    let text =
        serde_json::to_string_pretty(history).map_err(|e| format!("序列化统计失败：{e}"))?;

    // 与其他写入一样走原子写：崩溃不留半截 JSON
    crate::engine::atomic::write_atomic(&stats_path(root), text.as_bytes())
        .map_err(|e| format!("写入统计失败：{e}"))
}

/// 记录一次当日字数。
///
/// 同一天多次调用只保留最后一次（覆盖），因为存的是「当日结束时的总字数」。
pub fn record(root: &Path, date: &str, words: u64, at: &str) -> Result<StatsHistory, String> {
    let mut history = load(root);

    history.days.insert(
        date.to_string(),
        DayStat {
            words,
            at: at.to_string(),
        },
    );

    // 超出保留期时丢掉最老的记录
    while history.days.len() > KEEP_DAYS {
        let Some(oldest) = history.days.keys().next().cloned() else {
            break;
        };
        history.days.remove(&oldest);
    }

    save(root, &history)?;
    Ok(history)
}

/// 取最近 `limit` 个**有记录**的日子，按时间正序，附带与前一记录点的差值。
///
/// 只返回有记录的日子：没写作的那天不该在图上占一个「0 字」的柱子，
/// 那会让「停更」看起来像「写了一堆空白」。
pub fn trend(history: &StatsHistory, limit: usize) -> Vec<TrendPoint> {
    let mut points: Vec<TrendPoint> = Vec::new();
    let mut previous: Option<u64> = None;

    for (date, stat) in &history.days {
        let delta = match previous {
            Some(prev) => stat.words as i64 - prev as i64,
            None => 0,
        };
        points.push(TrendPoint {
            date: date.clone(),
            words: stat.words,
            delta,
        });
        previous = Some(stat.words);
    }

    // 只保留末尾 limit 个，但差值已按完整序列算好，不会因为截断而错
    if points.len() > limit {
        points.split_off(points.len() - limit)
    } else {
        points
    }
}

/// 今日相对上一个记录日的增量。用于状态栏「今日 +N 字」。
pub fn today_delta(history: &StatsHistory, today: &str) -> Option<i64> {
    let mut previous: Option<u64> = None;
    let mut result = None;

    for (date, stat) in &history.days {
        if date.as_str() == today {
            result = Some(match previous {
                Some(prev) => stat.words as i64 - prev as i64,
                None => 0,
            });
            break;
        }
        previous = Some(stat.words);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn 空目录返回空历史() {
        let dir = temp_root();
        let history = load(dir.path());
        assert!(history.days.is_empty());
        assert_eq!(history.version, 1);
    }

    #[test]
    fn 记录后能读回() {
        let dir = temp_root();
        record(dir.path(), "2026-11-03", 48210, "2026-11-03T19:00:00+08:00").unwrap();

        let history = load(dir.path());
        assert_eq!(history.days.get("2026-11-03").unwrap().words, 48210);
    }

    #[test]
    fn 同一天多次记录只保留最后一次() {
        let dir = temp_root();
        record(dir.path(), "2026-11-03", 1000, "2026-11-03T10:00:00+08:00").unwrap();
        record(dir.path(), "2026-11-03", 1500, "2026-11-03T20:00:00+08:00").unwrap();

        let history = load(dir.path());
        assert_eq!(history.days.len(), 1);
        assert_eq!(history.days.get("2026-11-03").unwrap().words, 1500);
    }

    #[test]
    fn 统计文件损坏时退化为空历史() {
        let dir = temp_root();
        std::fs::create_dir_all(dir.path().join(STATS_DIR)).unwrap();
        std::fs::write(dir.path().join(STATS_DIR).join(STATS_FILE), "{ 这不是 json").unwrap();

        let history = load(dir.path());
        assert!(history.days.is_empty());
    }

    #[test]
    fn 统计文件不影响章节扫描() {
        let dir = temp_root();
        std::fs::write(dir.path().join("001-楔子.md"), "---\ntitle: 楔子\n---\n正文\n").unwrap();
        record(dir.path(), "2026-11-03", 100, "2026-11-03T10:00:00+08:00").unwrap();

        let outcome = crate::engine::vault::scan_with_docs(dir.path()).unwrap();
        assert_eq!(outcome.snapshot.chapters.len(), 1);
        assert_eq!(outcome.snapshot.chapters[0].title, "楔子");
    }

    #[test]
    fn 趋势差值按相邻记录点计算() {
        let dir = temp_root();
        record(dir.path(), "2026-11-01", 1000, "2026-11-01T20:00:00+08:00").unwrap();
        record(dir.path(), "2026-11-03", 1800, "2026-11-03T20:00:00+08:00").unwrap();
        record(dir.path(), "2026-11-05", 2500, "2026-11-05T20:00:00+08:00").unwrap();

        let points = trend(&load(dir.path()), 30);
        assert_eq!(points.len(), 3);

        assert_eq!(points[0].delta, 0); // 首个点没有基准
        // 11-02 没记录，但 11-03 的增量相对 11-01 算，跨过空白日
        assert_eq!(points[1].delta, 800);
        assert_eq!(points[2].delta, 700);
    }

    #[test]
    fn 删改导致字数为负时差值为负() {
        let dir = temp_root();
        record(dir.path(), "2026-11-01", 2000, "2026-11-01T20:00:00+08:00").unwrap();
        record(dir.path(), "2026-11-02", 1800, "2026-11-02T20:00:00+08:00").unwrap();

        let points = trend(&load(dir.path()), 30);
        assert_eq!(points[1].delta, -200);
    }

    #[test]
    fn 趋势只保留末尾_limit_个() {
        let dir = temp_root();
        for day in 1..=10 {
            record(
                dir.path(),
                &format!("2026-11-{day:02}"),
                day as u64 * 100,
                &format!("2026-11-{day:02}T20:00:00+08:00"),
            )
            .unwrap();
        }

        let points = trend(&load(dir.path()), 3);
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].date, "2026-11-08");
        assert_eq!(points[2].date, "2026-11-10");
    }

    #[test]
    fn 今日增量() {
        let dir = temp_root();
        record(dir.path(), "2026-11-01", 1000, "2026-11-01T20:00:00+08:00").unwrap();
        record(dir.path(), "2026-11-03", 1450, "2026-11-03T20:00:00+08:00").unwrap();

        let history = load(dir.path());
        assert_eq!(today_delta(&history, "2026-11-03"), Some(450));
        assert_eq!(today_delta(&history, "2026-11-04"), None);
    }

    #[test]
    fn 首日增量是零而不是全文字数() {
        let dir = temp_root();
        record(dir.path(), "2026-11-01", 3000, "2026-11-01T20:00:00+08:00").unwrap();

        let history = load(dir.path());
        // 没有前一天可比，不能把全文字数当成「今天写的」
        assert_eq!(today_delta(&history, "2026-11-01"), Some(0));
    }

    #[test]
    fn 超过保留期时丢掉最老的记录() {
        let dir = temp_root();
        let mut history = StatsHistory::default();
        for i in 0..(KEEP_DAYS + 5) {
            history.days.insert(
                format!("2020-01-{:03}", i % 999),
                DayStat {
                    words: i as u64,
                    at: String::new(),
                },
            );
        }
        // 用 insert 直连，避免 730 次文件写
        save(dir.path(), &history).unwrap();
        assert!(load(dir.path()).days.len() > KEEP_DAYS);

        // record 会触发裁剪
        record(dir.path(), "2026-11-03", 1, "").unwrap();
        assert!(load(dir.path()).days.len() <= KEEP_DAYS + 1);
    }
}
