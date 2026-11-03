import { useEffect, useMemo, useRef, useState } from "react";

import { STATUS_LABEL } from "../../lib/labels";
import type { ChapterEntry } from "../../lib/types";
import { useWorkspace } from "../../store/workspace";

/**
 * 大纲视图：一屏看全书脉络。
 *
 * 占据主区域而非侧栏 —— 侧栏 280px 放不下摘要，而摘要正是大纲存在的意义。
 *
 * 摘要存在每章的 front matter 里（`summary`），不是独立的大纲文件。
 * 这样重命名、移动、重排章节时摘要自动跟随，不会出现「大纲指向已不存在的章」。
 */
export function OutlineView() {
  const snapshot = useWorkspace((s) => s.snapshot);
  const activePath = useWorkspace((s) => s.activePath);
  const openDoc = useWorkspace((s) => s.openDoc);
  const close = useWorkspace((s) => s.toggleOutline);
  const setSummary = useWorkspace((s) => s.setSummary);

  /** 正在编辑的摘要。同时只编辑一行。 */
  const [editing, setEditing] = useState<{ path: string; text: string } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) inputRef.current?.focus();
  }, [editing?.path]);

  // Esc 关闭大纲（编辑中时 Esc 只取消编辑，不关面板）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (editing) {
        setEditing(null);
        return;
      }
      close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [editing, close]);

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

  const withSummary = useMemo(
    () => (snapshot?.chapters ?? []).filter((c) => c.summary).length,
    [snapshot],
  );

  if (!snapshot) {
    return (
      <div className="empty-state">
        <p>尚未打开稿库。</p>
      </div>
    );
  }

  async function commit() {
    if (!editing) return;
    const { path, text } = editing;
    setEditing(null);
    await setSummary(path, text);
  }

  function openChapter(path: string) {
    void openDoc(path);
    close();
  }

  const total = snapshot.chapters.length;

  return (
    <div className="outline">
      <header className="outline-head">
        <span className="outline-title">{snapshot.title}</span>
        <span className="outline-meta">
          {total} 章 · {snapshot.totalWords} 字 · 摘要 {withSummary}/{total}
        </span>
        <button type="button" className="ghost-button" onClick={close}>
          回到编辑
        </button>
      </header>

      <div className="outline-body">
        {total === 0 && (
          <div className="explorer-empty">
            <p>这个稿库还没有章节。</p>
          </div>
        )}

        {groups.map(([dir, chapters]) => (
          <section key={dir || "__root__"} className="outline-group">
            <div className="outline-group-title">{dir || "根目录"}</div>

            {chapters.map((chapter) => {
              const isEditing = editing?.path === chapter.path;

              return (
                <div
                  key={chapter.path}
                  className={`outline-row${chapter.path === activePath ? " is-active" : ""}`}
                >
                  <button
                    type="button"
                    className="outline-chapter"
                    title={chapter.path}
                    onClick={() => openChapter(chapter.path)}
                  >
                    {chapter.title}
                  </button>

                  {isEditing ? (
                    <input
                      ref={inputRef}
                      className="outline-summary-input"
                      value={editing.text}
                      placeholder="一句话概括这一章发生了什么，回车保存"
                      onChange={(e) => setEditing({ path: chapter.path, text: e.target.value })}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void commit();
                        if (e.key === "Escape") setEditing(null);
                      }}
                      onBlur={() => void commit()}
                    />
                  ) : (
                    <button
                      type="button"
                      className={`outline-summary${chapter.summary ? "" : " is-empty"}`}
                      title={chapter.summary ?? "点击写摘要"}
                      onClick={() => setEditing({ path: chapter.path, text: chapter.summary ?? "" })}
                    >
                      {chapter.summary ?? "＋ 写摘要"}
                    </button>
                  )}

                  <span className="outline-words">{chapter.words}</span>
                  {chapter.status && (
                    <span className="outline-status">{STATUS_LABEL[chapter.status]}</span>
                  )}
                </div>
              );
            })}
          </section>
        ))}
      </div>
    </div>
  );
}
