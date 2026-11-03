/**
 * 主题偏好持久化。
 *
 * 与崩溃恢复日志不同，这是一个「下次启动也保留」的用户偏好，
 * 不是短期兜底数据。
 */

const KEY = "qinglai.theme.v1";

export type Theme = "dark" | "light" | "sepia";

const ORDER: Theme[] = ["dark", "light", "sepia"];

const LABELS: Record<Theme, string> = {
  dark: "暗色",
  light: "亮色",
  sepia: "护眼",
};

export function nextTheme(current: Theme): Theme {
  const idx = ORDER.indexOf(current);
  return ORDER[(idx + 1) % ORDER.length] ?? "dark";
}

export function themeLabel(theme: Theme): string {
  return LABELS[theme];
}

export function readTheme(): Theme {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw === "dark" || raw === "light" || raw === "sepia") return raw;
  } catch {
    // localStorage 不可用（隐私模式 / 损坏）→ 静默降级到默认
  }
  return "dark";
}

export function writeTheme(theme: Theme): void {
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // 写入失败不影响使用，主题仍然生效（只是下次启动不保留）
  }
}
