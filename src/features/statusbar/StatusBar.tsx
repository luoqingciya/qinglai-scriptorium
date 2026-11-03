import { save } from "@tauri-apps/plugin-dialog";

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
  const backlinks = useWorkspace((s) => s.backlinks);
  const backlinksOpen = useWorkspace((s) => s.backlinksOpen);
  const toggleBacklinks = useWorkspace((s) => s.toggleBacklinks);
  const exporting = useWorkspace((s) => s.exporting);
  const exportBook = useWorkspace((s) => s.exportBook);
  const toggleStats = useWorkspace((s) => s.toggleStats);
  const todayStat = useWorkspace((s) => s.todayStat);
  const outlineOpen = useWorkspace((s) => s.outlineOpen);
  const toggleOutline = useWorkspace((s) => s.toggleOutline);

  const themeLabel = useTheme((s) => s.label);
  const cycleTheme = useTheme((s) => s.cycle);

  const backlinkTotal = backlinks.reduce((sum, hit) => sum + hit.count, 0);

  async function onExport() {
    const dest = await save({
      title: "导出全库为 Word 文档",
      defaultPath: `${snapshot?.title ?? "未命名"}.docx`,
      filters: [{ name: "Word 文档", extensions: ["docx"] }],
    });
    if (typeof dest === "string") await exportBook(dest);
  }

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
        <button
          type="button"
          className="status-item status-item-button"
          title="查看写作统计"
          onClick={toggleStats}
        >
          全库 {snapshot.totalWords} 字
        </button>
      )}
      {todayStat && todayStat.delta !== 0 && (
        <>
          <span className="status-sep" />
          <span className="status-item status-today">
            今日 {todayStat.delta > 0 ? "+" : ""}
            {todayStat.delta}
          </span>
        </>
      )}
      <span className="status-sep" />
      <span className={`status-save is-${saveState}`}>{SAVE_LABEL[saveState]}</span>
      {doc && (
        <>
          <span className="status-sep" />
          <button
            type="button"
            className={`status-button${backlinksOpen ? " is-on" : ""}`}
            title={backlinksOpen ? "收起反链面板" : "展开反链面板"}
            onClick={toggleBacklinks}
          >
            反链 {backlinkTotal}
          </button>
        </>
      )}
      <span className="status-sep" />
      {snapshot && (
        <button
          type="button"
          className={`status-button${outlineOpen ? " is-on" : ""}`}
          title={outlineOpen ? "回到编辑" : "查看全书大纲"}
          onClick={toggleOutline}
        >
          大纲
        </button>
      )}
      <span className="status-sep" />
      {snapshot && (
        <button
          type="button"
          className="status-button"
          title="导出全库为 Word 文档（宋体小四、1.5 倍行距、首行缩进 2 字符）"
          disabled={exporting}
          onClick={() => void onExport()}
        >
          {exporting ? "导出中…" : "导出 docx"}
        </button>
      )}
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
