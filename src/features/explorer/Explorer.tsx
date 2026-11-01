import { useMemo } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import type { ChapterEntry } from "../../lib/types";
import { useWorkspace } from "../../store/workspace";

const STATUS_LABEL: Record<string, string> = {
  draft: "草稿",
  revising: "修订",
  done: "完稿",
};

export function Explorer() {
  const snapshot = useWorkspace((s) => s.snapshot);
  const activePath = useWorkspace((s) => s.activePath);
  const openVault = useWorkspace((s) => s.openVault);
  const openDoc = useWorkspace((s) => s.openDoc);

  const groups = useMemo(() => {
    if (!snapshot) return [];
    const map = new Map<string, ChapterEntry[]>();
    for (const chapter of snapshot.chapters) {
      const bucket = map.get(chapter.dir);
      if (bucket) bucket.push(chapter);
      else map.set(chapter.dir, [chapter]);
    }
    return [...map.entries()];
  }, [snapshot]);

  async function pickVault() {
    const picked = await open({ directory: true, multiple: false, title: "选择稿库目录" });
    if (typeof picked === "string") await openVault(picked);
  }

  return (
    <aside className="explorer">
      <header className="explorer-head">
        <div className="explorer-title">{snapshot ? snapshot.title : "清籁斋"}</div>
        <button type="button" className="ghost-button" onClick={() => void pickVault()}>
          {snapshot ? "换库" : "打开稿库"}
        </button>
      </header>

      {!snapshot && (
        <div className="explorer-empty">
          <p>尚未打开稿库。</p>
          <p className="hint">
            选一个目录作为稿库根，稿件以 Markdown 存在其中，本应用不引入任何私有格式。
          </p>
        </div>
      )}

      {snapshot && (
        <nav className="explorer-body">
          {groups.map(([dir, chapters]) => (
            <section key={dir || "__root__"} className="chapter-group">
              {dir && <div className="chapter-group-title">{dir}</div>}
              {chapters.map((chapter) => (
                <button
                  key={chapter.path}
                  type="button"
                  className={`chapter-item${chapter.path === activePath ? " is-active" : ""}`}
                  onClick={() => void openDoc(chapter.path)}
                  title={chapter.path}
                >
                  <span className="chapter-name">{chapter.title}</span>
                  <span className="chapter-meta">
                    {chapter.status && (
                      <span className="chapter-status">{STATUS_LABEL[chapter.status] ?? ""}</span>
                    )}
                    <span className="chapter-words">{chapter.words}</span>
                  </span>
                </button>
              ))}
            </section>
          ))}
          {snapshot.chapters.length === 0 && (
            <div className="explorer-empty">
              <p>这个目录下还没有 Markdown 稿件。</p>
            </div>
          )}
        </nav>
      )}
    </aside>
  );
}
