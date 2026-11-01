import { useWorkspace } from "../../store/workspace";
import type { SaveState } from "../../store/workspace";

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
    </footer>
  );
}
