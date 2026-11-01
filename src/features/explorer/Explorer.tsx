import { useMemo, useState } from "react";
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
  const createDoc = useWorkspace((s) => s.createDoc);
  const renameDoc = useWorkspace((s) => s.renameDoc);
  const deleteDoc = useWorkspace((s) => s.deleteDoc);
  const moveDoc = useWorkspace((s) => s.moveDoc);

  /** 正在新建时，记录目标目录（相对路径，空串代表根目录） */
  const [newDir, setNewDir] = useState<string | null>(null);
  const [newTitle, setNewTitle] = useState("");
  /** 正在重命名的章节 */
  const [renaming, setRenaming] = useState<{ path: string; title: string } | null>(null);
  /** 已点过一次删除、等待确认的章节 */
  const [confirming, setConfirming] = useState<string | null>(null);

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

  const activeDir = useMemo(() => {
    if (!snapshot) return "";
    return snapshot.chapters.find((c) => c.path === activePath)?.dir ?? "";
  }, [snapshot, activePath]);

  async function pickVault() {
    const picked = await open({ directory: true, multiple: false, title: "选择稿库目录" });
    if (typeof picked === "string") await openVault(picked);
  }

  async function submitCreate() {
    const title = newTitle.trim();
    if (!title || newDir === null) return;
    await createDoc(newDir, title);
    setNewDir(null);
    setNewTitle("");
  }

  async function submitRename() {
    if (!renaming) return;
    const title = renaming.title.trim();
    const target = renaming;
    setRenaming(null);
    if (!title) return;
    await renameDoc(target.path, title);
  }

  return (
    <aside className="explorer">
      <header className="explorer-head">
        <div className="explorer-title">{snapshot ? snapshot.title : "清籁斋"}</div>
        {snapshot && (
          <button
            type="button"
            className="ghost-button"
            title={`在当前目录新建章节（${activeDir || "根目录"}）`}
            onClick={() => {
              setNewDir(activeDir);
              setNewTitle("");
            }}
          >
            新建
          </button>
        )}
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
          {newDir !== null && (
            <div className="inline-create">
              <input
                autoFocus
                className="inline-input"
                value={newTitle}
                placeholder="新章节标题，回车确认"
                onChange={(e) => setNewTitle(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void submitCreate();
                  if (e.key === "Escape") {
                    setNewDir(null);
                    setNewTitle("");
                  }
                }}
                onBlur={() => {
                  // 输入为空时收起，避免留下一个空白输入框
                  if (!newTitle.trim()) setNewDir(null);
                }}
              />
              <div className="inline-hint">新建于 {newDir || "根目录"}</div>
            </div>
          )}

          {groups.map(([dir, chapters]) => (
            <section key={dir || "__root__"} className="chapter-group">
              {dir && <div className="chapter-group-title">{dir}</div>}
              {chapters.map((chapter, idx) => {
                const isActive = chapter.path === activePath;
                const isRenaming = renaming?.path === chapter.path;
                const canUp = idx > 0;
                const canDown = idx < chapters.length - 1;

                return (
                  <div
                    key={chapter.path}
                    className={`chapter-row${isActive ? " is-active" : ""}`}
                  >
                    {isRenaming ? (
                      <input
                        autoFocus
                        className="inline-input"
                        value={renaming.title}
                        onChange={(e) => setRenaming({ path: chapter.path, title: e.target.value })}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") void submitRename();
                          if (e.key === "Escape") setRenaming(null);
                        }}
                        onBlur={() => void submitRename()}
                      />
                    ) : (
                      <>
                        <button
                          type="button"
                          className="chapter-main"
                          title={chapter.path}
                          onClick={() => void openDoc(chapter.path)}
                          onDoubleClick={() =>
                            setRenaming({ path: chapter.path, title: chapter.title })
                          }
                        >
                          <span className="chapter-name">{chapter.title}</span>
                          <span className="chapter-meta">
                            {chapter.status && (
                              <span className="chapter-status">
                                {STATUS_LABEL[chapter.status] ?? ""}
                              </span>
                            )}
                            <span className="chapter-words">{chapter.words}</span>
                          </span>
                        </button>

                        <div className="chapter-actions">
                          <button
                            type="button"
                            title="上移一位"
                            disabled={!canUp}
                            onClick={() => void moveDoc(chapter.path, "up")}
                          >
                            ↑
                          </button>
                          <button
                            type="button"
                            title="下移一位"
                            disabled={!canDown}
                            onClick={() => void moveDoc(chapter.path, "down")}
                          >
                            ↓
                          </button>
                          <button
                            type="button"
                            title="重命名（也可双击标题）"
                            onClick={() =>
                              setRenaming({ path: chapter.path, title: chapter.title })
                            }
                          >
                            改
                          </button>
                          <button
                            type="button"
                            className={confirming === chapter.path ? "is-danger" : ""}
                            title="移入回收站 .trash/"
                            onMouseLeave={() =>
                              setConfirming((c) => (c === chapter.path ? null : c))
                            }
                            onClick={() => {
                              if (confirming === chapter.path) {
                                setConfirming(null);
                                void deleteDoc(chapter.path);
                              } else {
                                setConfirming(chapter.path);
                              }
                            }}
                          >
                            {confirming === chapter.path ? "确认" : "删"}
                          </button>
                        </div>
                      </>
                    )}
                  </div>
                );
              })}
            </section>
          ))}

          {snapshot.chapters.length === 0 && newDir === null && (
            <div className="explorer-empty">
              <p>这个目录下还没有 Markdown 稿件。</p>
              <p className="hint">点右上角「新建」写下第一章。</p>
            </div>
          )}
        </nav>
      )}
    </aside>
  );
}
