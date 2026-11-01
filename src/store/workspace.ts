import { create } from "zustand";

import * as ipc from "../lib/ipc";
import type { DocPayload, VaultSnapshot, WordCount } from "../lib/types";

/** 自动保存防抖间隔。800ms 是「几乎感觉不到延迟」与「不会每敲一个字都写盘」的折中。 */
export const AUTOSAVE_DEBOUNCE_MS = 800;

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
  words: WordCount;
  saveState: SaveState;
  error: string | null;

  openVault: (path: string) => Promise<void>;
  refresh: () => Promise<void>;
  openDoc: (path: string) => Promise<void>;
  createDoc: (dir: string, title: string) => Promise<void>;
  renameDoc: (path: string, title: string) => Promise<void>;
  deleteDoc: (path: string) => Promise<void>;
  setBody: (markdown: string) => void;
  adoptBaseline: (markdown: string) => void;
  flush: () => Promise<void>;
  dismissError: () => void;
}

let debounceTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleSave() {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    void useWorkspace.getState().flush();
  }, AUTOSAVE_DEBOUNCE_MS);
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
        words: doc.words,
        saveState: "idle",
        error: null,
      });
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
      words: EMPTY_WORDS,
      saveState: "idle",
    });

  return {
    snapshot: null,
    activePath: null,
    doc: null,
    body: "",
    lastSaved: "",
    words: EMPTY_WORDS,
    saveState: "idle",
    error: null,

    openVault: async (path) => {
      try {
        const snapshot = await ipc.openVault(path);
        clearActive();
        set({ snapshot, error: null });
        const first = snapshot.chapters[0];
        if (first) await get().openDoc(first.path);
      } catch (err) {
        set({ error: String(err), saveState: "error" });
      }
    },

    refresh: async () => {
      try {
        const snapshot = await ipc.scanVault();
        set({ snapshot });
      } catch (err) {
        set({ error: String(err), saveState: "error" });
      }
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

    setBody: (markdown) => {
      if (get().body === markdown) return;
      set({ body: markdown, saveState: "dirty" });
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
        return;
      }

      set({ saveState: "saving" });
      try {
        const outcome = await ipc.saveDoc(doc.path, body, doc.stamp);
        if (outcome.kind === "conflict") {
          set({
            saveState: "conflict",
            error: "磁盘上的文件已被外部修改，本次改动未写入。请重新打开该章。",
          });
          return;
        }

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

    dismissError: () => set({ error: null }),
  };
});
