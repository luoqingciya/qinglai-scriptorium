import { useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { STATUS_LABEL } from "../../lib/labels";
import type { ChapterEntry, SearchHit } from "../../lib/types";
import { useWorkspace } from "../../store/workspace";
import { CharacterList } from "../characters/CharacterList";

const SEARCH_DEBOUNCE_MS = 200;

/** 侧栏当前显示的内容 */
type ExplorerView = "chapters" | "characters";

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** 把摘要里的关键词包成 <mark>。先 split 出交替片段，再按小写比对判定。 */
function highlight(text: string, query: string): ReactNode {
  const terms = query
    .split(/\s+/)
    .map((t) => t.trim())
    .filter(Boolean);
  if (terms.length === 0) return text;

  const pattern = new RegExp(`(${terms.map(escapeRegExp).join("|")})`, "gi");
  const lowered = new Set(terms.map((t) => t.toLowerCase()));

  return text.split(pattern).map((part, index) =>
    lowered.has(part.toLowerCase()) ? <mark key={index}>{part}</mark> : part,
  );
}

function SearchResults({ hits, query }: { hits: SearchHit[]; query: string }) {
  const openDoc = useWorkspace((s) => s.openDoc);

  return (
    <div className="search-results">
      {hits.map((hit) => (
        <button
          key={hit.path}
          type="button"
          className="search-hit"
          title={hit.path}
          onClick={() => void openDoc(hit.path)}
        >
          <div className="search-hit-head">
            <span className="search-hit-title">{hit.title}</span>
            <span className="search-hit-count">{hit.count}</span>
          </div>
          {hit.dir && <div className="search-hit-dir">{hit.dir}</div>}
          <div className="search-hit-snippet">{highlight(hit.snippet, query)}</div>
        </button>
      ))}
    </div>
  );
}

export function Explorer() {
  const snapshot = useWorkspace((s) => s.snapshot);
  const activePath = useWorkspace((s) => s.activePath);
  const openVault = useWorkspace((s) => s.openVault);
  const openDoc = useWorkspace((s) => s.openDoc);
  const createDoc = useWorkspace((s) => s.createDoc);
  const renameDoc = useWorkspace((s) => s.renameDoc);
  const deleteDoc = useWorkspace((s) => s.deleteDoc);
  const moveDoc = useWorkspace((s) => s.moveDoc);
  const runSearch = useWorkspace((s) => s.runSearch);
  const clearSearch = useWorkspace((s) => s.clearSearch);
  const searchResults = useWorkspace((s) => s.searchResults);
  const searching = useWorkspace((s) => s.searching);

  /** 正在新建时，记录目标目录（相对路径，空串代表根目录） */
  const [newDir, setNewDir] = useState<string | null>(null);
  const [newTitle, setNewTitle] = useState("");
  /** 正在重命名的章节 */
  const [renaming, setRenaming] = useState<{ path: string; title: string } | null>(null);
  /** 已点过一次删除、等待确认的章节 */
  const [confirming, setConfirming] = useState<string | null>(null);
  /** 检索输入框的即时值，与防抖后的请求分开 */
  const [query, setQuery] = useState("");
  const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** 侧栏视图：章节树 / 人物 */
  const [view, setView] = useState<ExplorerView>("chapters");

  const isSearching = query.trim().length > 0;

  function onQueryChange(value: string) {
    setQuery(value);
    if (searchTimer.current) clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(() => {
      searchTimer.current = null;
      void runSearch(value);
    }, SEARCH_DEBOUNCE_MS);
  }

  function resetSearch() {
    if (searchTimer.current) clearTimeout(searchTimer.current);
    searchTimer.current = null;
    setQuery("");
    clearSearch();
  }

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
        <div className="explorer-tabs" role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={view === "chapters"}
            className={`explorer-tab${view === "chapters" ? " is-active" : ""}`}
            onClick={() => setView("chapters")}
          >
            章节
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={view === "characters"}
            className={`explorer-tab${view === "characters" ? " is-active" : ""}`}
            onClick={() => setView("characters")}
          >
            人物
          </button>
        </div>
      )}

      {snapshot && view === "characters" && (
        <nav className="explorer-body">
          <CharacterList />
        </nav>
      )}

      {snapshot && view === "chapters" && (
        <nav className="explorer-body">
          <div className="search-box">
            <input
              className="search-input"
              value={query}
              placeholder="搜索全库，空格分隔多个关键词"
              onChange={(e) => onQueryChange(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") resetSearch();
              }}
            />
            {isSearching && (
              <button
                type="button"
                className="search-clear"
                title="清空检索（Esc）"
                onClick={resetSearch}
              >
                ×
              </button>
            )}
          </div>

          {isSearching ? (
            searching && searchResults.length === 0 ? (
              <div className="explorer-empty">
                <p>搜索中…</p>
              </div>
            ) : searchResults.length === 0 ? (
              <div className="explorer-empty">
                <p>没有匹配的章节。</p>
                <p className="hint">多个关键词之间是「且」的关系。</p>
              </div>
            ) : (
              <SearchResults hits={searchResults} query={query} />
            )
          ) : (
            <>
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
            </>
          )}
        </nav>
      )}
    </aside>
  );
}
