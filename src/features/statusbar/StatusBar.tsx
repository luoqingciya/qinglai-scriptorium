import { useWorkspace } from "../../store/workspace";
import type { SaveState } from "../../store/workspace";
import { useTheme } from "../../lib/theme-store";

const SAVE_LABEL: Record<SaveState, string> = {
  idle: "就绪",
  dirty: "待保存",
  saving: "保存中",
  saved: "已保存",
  conflict: "冲突",
  error: "出错",
};

export function StatusBar() {
  const snapshot = useWorkspace((s) => s.snapshot);
  const doc = useWorkspace((s) => s.doc);
  const words = useWorkspace((s) => s.words);
  const saveState = useWorkspace((s) => s.saveState);
  const focusMode = useWorkspace((s) => s.focusMode);
  const toggleFocus = useWorkspace((s) => s.toggleFocus);

  const themeLabel = useTheme((s) => s.label);
  const cycleTheme = useTheme((s) => s.cycle);

  return (
    <footer className="statusbar">
      <span className="status-path">{doc ? doc.path : "—"}</span>
      <span className="status-spacer" />
      {doc && (
        <>
          <span className="status-item">本章 {words.total} 字</span>
          <span className="status-sep" />
        </>
      )}
      {snapshot && (
        <span className="status-item">全库 {snapshot.totalWords} 字</span>
      )}
      <span className="status-sep" />
      <span className={`status-save is-${saveState}`}>{SAVE_LABEL[saveState]}</span>
      <span className="status-sep" />
      <button
        type="button"
        className="status-button"
        title={focusMode ? "退出专注模式（F11）" : "进入专注模式（F11）"}
        onClick={toggleFocus}
      >
        {focusMode ? "退出专注" : "专注"}
      </button>
      <span className="status-sep" />
      <button
        type="button"
        className="status-button"
        title="切换主题"
        onClick={cycleTheme}
      >
        {themeLabel}
      </button>
    </footer>
  );
}
