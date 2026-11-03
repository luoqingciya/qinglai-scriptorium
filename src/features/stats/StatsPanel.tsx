import { useEffect } from "react";

import { useWorkspace } from "../../store/workspace";
import type { TrendPoint } from "../../lib/types";

const CHART_W = 300;
const CHART_H = 110;
const PAD_X = 6;
const PAD_TOP = 14;
const BASELINE = CHART_H - 18;

/**
 * 把趋势点映射成柱状图。
 *
 * 只画「正增量」的柱子 —— 删改造成的负值用一条贴底的细灰线表示。
 * 不用红绿区分涨跌：那是股票的语言，写作里「字数减少」不是亏损，
 * 用颜色强调反而让人误读。
 */
function bars(points: TrendPoint[]) {
  if (points.length === 0) return [];

  const slot = (CHART_W - PAD_X * 2) / points.length;
  const barW = Math.max(2, Math.min(10, slot - 2));
  const maxDelta = Math.max(1, ...points.map((p) => p.delta));

  return points.map((point, i) => {
    const x = PAD_X + slot * i + (slot - barW) / 2;
    const positive = point.delta > 0;
    const h = positive
      ? Math.max(2, (point.delta / maxDelta) * (BASELINE - PAD_TOP))
      : 0;

    return {
      key: point.date,
      x,
      y: positive ? BASELINE - h : BASELINE,
      w: barW,
      h: positive ? h : 2,
      positive,
      delta: point.delta,
      words: point.words,
    };
  });
}

function shortDate(date: string): string {
  // YYYY-MM-DD → MM-DD
  return date.length >= 10 ? date.slice(5) : date;
}

export function StatsPanel() {
  const trend = useWorkspace((s) => s.trend);
  const todayStat = useWorkspace((s) => s.todayStat);
  const snapshot = useWorkspace((s) => s.snapshot);
  const close = useWorkspace((s) => s.toggleStats);

  // Esc 关闭
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);

  const rects = bars(trend);
  const totalDelta = trend.reduce((sum, p) => sum + Math.max(0, p.delta), 0);
  const first = trend[0]?.date;
  const last = trend[trend.length - 1]?.date;

  return (
    <div
      className="stats-overlay"
      role="dialog"
      aria-label="写作统计"
      onClick={close}
    >
      <div className="stats-card" onClick={(e) => e.stopPropagation()}>
        <header className="stats-head">
          <span className="stats-title">写作统计</span>
          <button type="button" className="stats-close" title="关闭（Esc）" onClick={close}>
            ×
          </button>
        </header>

        <div className="stats-summary">
          <div className="stats-metric">
            <span className="stats-value">{snapshot ? snapshot.totalWords : 0}</span>
            <span className="stats-label">全库字数</span>
          </div>
          <div className="stats-metric">
            <span className="stats-value">
              {todayStat ? (todayStat.delta > 0 ? `+${todayStat.delta}` : todayStat.delta) : "—"}
            </span>
            <span className="stats-label">今日</span>
          </div>
          <div className="stats-metric">
            <span className="stats-value">{trend.length}</span>
            <span className="stats-label">记录天数</span>
          </div>
        </div>

        {trend.length === 0 ? (
          <div className="stats-empty">
            <p>还没有写作记录。</p>
            <p className="hint">开始写作并保存后，这里会显示每天的进度。</p>
          </div>
        ) : (
          <>
            <svg
              className="stats-chart"
              viewBox={`0 0 ${CHART_W} ${CHART_H}`}
              role="img"
              aria-label={`最近 ${trend.length} 天的每日新增字数`}
            >
              <line
                x1={PAD_X}
                y1={BASELINE}
                x2={CHART_W - PAD_X}
                y2={BASELINE}
                stroke="var(--border)"
                strokeWidth={1}
              />
              {rects.map((r) => (
                <rect
                  key={r.key}
                  x={r.x}
                  y={r.y}
                  width={r.w}
                  height={r.h}
                  rx={r.positive ? 1.5 : 0}
                  fill={r.positive ? "var(--accent)" : "var(--text-faint)"}
                  opacity={r.positive ? 0.85 : 0.5}
                >
                  <title>
                    {r.key}：{r.delta >= 0 ? `+${r.delta}` : r.delta} 字（累计 {r.words}）
                  </title>
                </rect>
              ))}
            </svg>

            <div className="stats-axis">
              <span>{first ? shortDate(first) : ""}</span>
              <span>{last ? shortDate(last) : ""}</span>
            </div>

            <div className="stats-foot">
              最近 {trend.length} 次记录共新增 {totalDelta} 字
            </div>
          </>
        )}
      </div>
    </div>
  );
}
