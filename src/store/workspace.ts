import { create } from "zustand";

import * as ipc from "../lib/ipc";
import {
  clearJournal,
  clearLastVault,
  readJournal,
  readLastVault,
  writeJournal,
  writeLastVault,
  type JournalEntry,
} from "../lib/journal";
import type {
  BacklinkHit,
  Direction,
  DocPayload,
  SearchHit,
  VaultSnapshot,
  WordCount,
} from "../lib/types";

/** 自动保存防抖间隔。800ms 是「几乎感觉不到延迟」与「不会每敲一个字都写盘」的折中。 */
export const AUTOSAVE_DEBOUNCE_MS = 800;

/** 专注模式的持久化 key */
const FOCUS_KEY = "qinglai.focus.v1";

function readFocus(): boolean {
  try {
    return localStorage.getItem(FOCUS_KEY) === "1";
  } catch {
    return false;
  }
}

function writeFocus(on: boolean) {
  try {
    localStorage.setItem(FOCUS_KEY, on ? "1" : "0");
  } catch {
    // 写入失败不影响使用
  }
}

/**
 * 恢复日志的写入间隔，比自动保存短得多。
 *
 * 日志写 localStorage，代价远低于落盘，所以可以用更密的节奏，
 * 把「强杀进程」的丢失窗口从 800ms 压到 150ms 量级。
 */
export const JOURNAL_DEBOUNCE_MS = 150;

export type SaveState = "idle" | "dirty" | "saving" | "saved" | "conflict" | "error";

const EMPTY_WORDS: WordCount = { cjk: 0, latin: 0, punctuation: 0, total: 0 };

interface WorkspaceState {
  snapshot: VaultSnapshot | null;
  activePath: string | null;
  doc: DocPayload | null;
  /** 编辑器当前正文 */
  body: string;
  /** 最后一次确认已落盘的正文，用于跳过无谓写入 */
  lastSaved: string;
  /** 编辑器挂载时使用的初始内容 */
  editorContent: string;
  /** 每次需要强制重建编辑器时递增，参与 EditorPane 的 key */
  docEpoch: number;
  /** 启动时发现的未落盘日志，等待用户决定 */
  pendingRecovery: JournalEntry | null;
  words: WordCount;
  saveState: SaveState;
  error: string | null;

  /** 当前检索词 */
  searchQuery: string;
  searchResults: SearchHit[];
  searching: boolean;
  runSearch: (query: string) => Promise<void>;
  clearSearch: () => void;

  /** 专注模式：隐藏目录树，编辑区铺满 */
  focusMode: boolean;
  toggleFocus: () => void;

  /** 当前章的反链 */
  backlinks: BacklinkHit[];
  /** 反链面板是否展开。默认收起，不打扰写作。 */
  backlinksOpen: boolean;
  toggleBacklinks: () => void;
  loadBacklinks: (path: string) => Promise<void>;
  /** 按标题跳转，供 `[[双链]]` 点击使用 */
  openByTitle: (title: string) => Promise<void>;

  openVault: (path: string) => Promise<void>;
  refresh: () => Promise<void>;
  openDoc: (path: string) => Promise<void>;
  createDoc: (dir: string, title: string) => Promise<void>;
  renameDoc: (path: string, title: string) => Promise<void>;
  deleteDoc: (path: string) => Promise<void>;
  moveDoc: (path: string, direction: Direction) => Promise<void>;
  setBody: (markdown: string) => void;
  adoptBaseline: (markdown: string) => void;
  flush: () => Promise<void>;
  restoreLastVault: () => Promise<void>;
  checkJournal: () => Promise<void>;
  applyRecovery: () => Promise<void>;
  discardRecovery: () => void;
  dismissError: () => void;
}

let saveTimer: ReturnType<typeof setTimeout> | null = null;
let journalTimer: ReturnType<typeof setTimeout> | null = null;

/** 检索请求序号，用于丢弃过期结果（打字快时会有多个请求在飞） */
let searchSeq = 0;

function scheduleSave() {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = null;
    void useWorkspace.getState().flush();
  }, AUTOSAVE_DEBOUNCE_MS);
}

function scheduleJournal(path: string, body: string) {
  if (journalTimer) clearTimeout(journalTimer);
  journalTimer = setTimeout(() => {
    journalTimer = null;
    writeJournal({ path, body, at: Date.now() });
  }, JOURNAL_DEBOUNCE_MS);
}

export const useWorkspace = create<WorkspaceState>()((set, get) => {
  /** 无条件从磁盘载入一篇文档。openDoc 的重入判断在外层做。 */
  const loadDoc = async (path: string) => {
    try {
      const doc = await ipc.readDoc(path);
      set({
        activePath: path,
        doc,
        body: doc.body,
        lastSaved: doc.body,
        editorContent: doc.body,
        docEpoch: get().docEpoch + 1,
        words: doc.words,
        saveState: "idle",
        error: null,
      });
      // 反链查询不阻塞打开：查不到也不该影响写作
      void get().loadBacklinks(path);
    } catch (err) {
      set({ doc: null, activePath: path, error: String(err), saveState: "error" });
    }
  };

  const clearActive = () =>
    set({
      activePath: null,
      doc: null,
      body: "",
      lastSaved: "",
      editorContent: "",
      docEpoch: get().docEpoch + 1,
      words: EMPTY_WORDS,
      saveState: "idle",
      backlinks: [],
    });

  return {
    snapshot: null,
    activePath: null,
    doc: null,
    body: "",
    lastSaved: "",
    editorContent: "",
    docEpoch: 0,
    pendingRecovery: null,
    words: EMPTY_WORDS,
    saveState: "idle",
    error: null,
    searchQuery: "",
    searchResults: [],
    searching: false,
    focusMode: readFocus(),
    backlinks: [],
    backlinksOpen: false,

    toggleFocus: () => {
      const next = !get().focusMode;
      writeFocus(next);
      set({ focusMode: next });
    },

    toggleBacklinks: () => set({ backlinksOpen: !get().backlinksOpen }),

    loadBacklinks: async (path) => {
      try {
        const hits = await ipc.backlinks(path);
        // 切章后旧请求可能后到，只认当前章的结果
        if (get().activePath !== path) return;
        set({ backlinks: hits });
      } catch {
        // 反链是派生信息，查不到就留空，不打断写作
        if (get().activePath === path) set({ backlinks: [] });
      }
    },

    /**
     * 按标题跳转。
     *
     * 标题由用户手写，可能拼错、可能指向已改名的章节 —— 解析不到时给出明确提示，
     * 而不是静默无反应（静默会让人以为点击失效）。
     */
    openByTitle: async (title) => {
      try {
        const path = await ipc.resolveLink(title);
        if (!path) {
          set({ error: `找不到章节「${title}」，它可能已被改名或删除。` });
          return;
        }
        await get().openDoc(path);
      } catch (err) {
        set({ error: String(err) });
      }
    },

    openVault: async (path) => {
      try {
        // 换库时丢弃上一个库的残留日志：日志里的路径是相对路径，
        // 跨库沿用可能把 A 库的内容写进 B 库的同名章节。
        const previous = readLastVault();
        if (previous && previous !== path) clearJournal();

        const snapshot = await ipc.openVault(path);
        writeLastVault(path);
        clearActive();
        set({ snapshot, error: null });
        const first = snapshot.chapters[0];
        if (first) await get().openDoc(first.path);
        await get().checkJournal();
      } catch (err) {
        // 上次记住的目录可能已被移动或删除，别让它一直报错
        clearLastVault();
        set({ error: String(err), saveState: "error" });
      }
    },

    refresh: async () => {
      try {
        const snapshot = await ipc.scanVault();
        set({ snapshot });
        // 章节有增删改时，检索结果里的路径可能已失效，顺手重跑一次
        const query = get().searchQuery;
        if (query.trim()) await get().runSearch(query);
        // 增删改也会改变反链（比如删了一章，引用它的地方就悬空了）
        const active = get().activePath;
        if (active) void get().loadBacklinks(active);
      } catch (err) {
        set({ error: String(err), saveState: "error" });
      }
    },

    runSearch: async (query) => {
      const seq = ++searchSeq;
      set({ searchQuery: query });

      if (!query.trim()) {
        set({ searchResults: [], searching: false });
        return;
      }

      set({ searching: true });
      try {
        const hits = await ipc.searchVault(query, 50);
        // 打字快时会有多个请求在飞，只认最后一个
        if (seq !== searchSeq) return;
        set({ searchResults: hits, searching: false });
      } catch (err) {
        if (seq !== searchSeq) return;
        set({ searchResults: [], searching: false, error: String(err) });
      }
    },

    clearSearch: () => {
      searchSeq += 1;
      set({ searchQuery: "", searchResults: [], searching: false });
    },

    openDoc: async (path) => {
      if (get().activePath === path) return;
      // 切章之前必须把当前章的改动落盘，否则会丢字
      await get().flush();
      await loadDoc(path);
    },

    createDoc: async (dir, title) => {
      try {
        await get().flush();
        const path = await ipc.createDoc(dir, title);
        await get().refresh();
        await loadDoc(path);
      } catch (err) {
        set({ error: String(err) });
      }
    },

    renameDoc: async (path, title) => {
      try {
        // 先落盘：重命名会改文件名，编辑器会按新路径重建
        await get().flush();
        const wasActive = get().activePath === path;
        const next = await ipc.renameDoc(path, title);
        await get().refresh();
        if (wasActive) await loadDoc(next);
      } catch (err) {
        set({ error: String(err) });
      }
    },

    deleteDoc: async (path) => {
      try {
        const wasActive = get().activePath === path;
        if (wasActive) await get().flush();
        await ipc.deleteDoc(path);
        if (wasActive) clearActive();
        await get().refresh();
      } catch (err) {
        set({ error: String(err) });
      }
    },

    moveDoc: async (path, direction) => {
      try {
        // 重排会改文件名，先落盘再动
        await get().flush();
        const renames = await ipc.reorderDoc(path, direction);
        if (renames.length === 0) return;
        await get().refresh();

        // 当前打开的文件可能因换位而改名，需要按新路径重新载入
        const active = get().activePath;
        if (!active) return;
        const hit = renames.find((r) => r.from === active);
        if (hit) await loadDoc(hit.to);
      } catch (err) {
        set({ error: String(err) });
      }
    },

    setBody: (markdown) => {
      if (get().body === markdown) return;
      set({ body: markdown, saveState: "dirty" });

      const path = get().doc?.path;
      if (path) scheduleJournal(path, markdown);
      scheduleSave();
    },

    /**
     * 编辑器初始化后回传它自己的序列化基线。
     *
     * Milkdown 序列化出的 Markdown 可能与磁盘原文有细微差异（空行、列表符号等），
     * 用基线而非原文做比较，才能保证「打开但不编辑 → 不写盘」。
     * 若用户已经先敲了字，则忽略这次基线，避免覆盖。
     */
    adoptBaseline: (markdown) => {
      const { doc, body } = get();
      if (!doc) return;
      if (body !== doc.body) return;
      set({ body: markdown, lastSaved: markdown });
    },

    flush: async () => {
      const { doc, body, lastSaved } = get();
      if (!doc) return;
      if (body === lastSaved) {
        if (get().saveState === "dirty") set({ saveState: "saved" });
        clearJournal();
        return;
      }

      set({ saveState: "saving" });
      try {
        const outcome = await ipc.saveDoc(doc.path, body, doc.stamp);
        if (outcome.kind === "conflict") {
          // 保留日志：用户重新打开该章后仍有机会找回这次改动
          set({
            saveState: "conflict",
            error: "磁盘上的文件已被外部修改，本次改动未写入。请重新打开该章。",
          });
          return;
        }

        clearJournal();
        set({
          saveState: "saved",
          lastSaved: body,
          doc: {
            ...doc,
            stamp: outcome.stamp,
            ...(outcome.kind === "saved" ? { words: outcome.words } : {}),
          },
          ...(outcome.kind === "saved" ? { words: outcome.words } : {}),
        });

        // 保存期间又有新输入，安排下一轮
        if (get().body !== body) scheduleSave();
      } catch (err) {
        set({ saveState: "error", error: String(err) });
      }
    },

    /** 启动时恢复上次打开的稿库（openVault 内部会顺带检查崩溃残留） */
    restoreLastVault: async () => {
      const path = readLastVault();
      if (!path) return;
      await get().openVault(path);
    },

    /**
     * 检查是否有崩溃残留。只有「日志内容与磁盘不同」才提示，
     * 否则说明那次改动其实已经落盘，静默清掉即可。
     */
    checkJournal: async () => {
      const journal = readJournal();
      if (!journal) return;

      const snapshot = get().snapshot;
      if (!snapshot) return;

      // 章节可能已被删除或改名，此时日志已无意义
      if (!snapshot.chapters.some((c) => c.path === journal.path)) {
        clearJournal();
        return;
      }

      try {
        const disk = await ipc.readDoc(journal.path);
        if (disk.body === journal.body) {
          clearJournal();
          return;
        }
        set({ pendingRecovery: journal });
      } catch {
        clearJournal();
      }
    },

    /**
     * 恢复日志内容：直接落盘，再让编辑器以新内容重建。
     *
     * 不走「标记脏 → 等自动保存」那条路，是因为重建编辑器会触发
     * adoptBaseline，那条路径上很容易把待保存状态抹掉。
     */
    applyRecovery: async () => {
      const journal = get().pendingRecovery;
      if (!journal) return;
      set({ pendingRecovery: null });

      if (get().activePath !== journal.path) {
        await get().openDoc(journal.path);
      }

      const doc = get().doc;
      if (!doc || doc.path !== journal.path) {
        clearJournal();
        set({ error: "恢复失败：找不到对应章节，可能已被移动或删除。" });
        return;
      }

      set({ saveState: "saving" });
      try {
        const outcome = await ipc.saveDoc(doc.path, journal.body, doc.stamp);
        if (outcome.kind === "conflict") {
          set({
            saveState: "conflict",
            error: "恢复失败：磁盘上的文件已被外部修改，请重新打开该章后再试。",
          });
          return;
        }

        clearJournal();
        set({
          body: journal.body,
          lastSaved: journal.body,
          editorContent: journal.body,
          docEpoch: get().docEpoch + 1,
          doc: {
            ...doc,
            body: journal.body,
            stamp: outcome.stamp,
            ...(outcome.kind === "saved" ? { words: outcome.words } : {}),
          },
          ...(outcome.kind === "saved" ? { words: outcome.words } : {}),
          saveState: "saved",
        });
      } catch (err) {
        set({ saveState: "error", error: String(err) });
      }
    },

    discardRecovery: () => {
      clearJournal();
      set({ pendingRecovery: null });
    },

    dismissError: () => set({ error: null }),
  };
});
