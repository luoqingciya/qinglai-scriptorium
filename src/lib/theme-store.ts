/**
 * 主题状态。
 *
 * 用一个独立的 zustand store 而非塞进 workspace store，是因为主题
 * 与稿库无关——没打开任何稿库时也应该能切主题。
 */
import { create } from "zustand";

import { nextTheme, readTheme, themeLabel, writeTheme, type Theme } from "../lib/theme";

interface ThemeState {
  theme: Theme;
  label: string;
  cycle: () => void;
}

/** 把 data-theme 写到 <html> 上，CSS 变量据此切换 */
function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
}

export const useTheme = create<ThemeState>((set, get) => ({
  theme: readTheme(),
  label: themeLabel(readTheme()),

  cycle: () => {
    const next = nextTheme(get().theme);
    writeTheme(next);
    applyTheme(next);
    set({ theme: next, label: themeLabel(next) });
  },
}));

/**
 * 在模块加载时立刻把持久化的主题应用到 DOM。
 *
 * 放在这里而非 React 的 useEffect，是为了避免「先用默认暗色渲染一帧，
 * 再闪到亮色」的 FOUC（flash of unstyled content）。
 */
applyTheme(readTheme());
