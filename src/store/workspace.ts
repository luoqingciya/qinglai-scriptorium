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

export const useWorkspace = create<WorkspaceState>()((set, get) => ({
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
      set({
        snapshot,
        activePath: null,
        doc: null,
        body: "",
        lastSaved: "",
        words: EMPTY_WORDS,
        saveState: "idle",
        error: null,
      });
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
    set({ activePath: path, error: null, saveState: "idle" });

    try {
      const doc = await ipc.readDoc(path);
      set({
        doc,
        body: doc.body,
        lastSaved: doc.body,
        words: doc.words,
        saveState: "idle",
      });
    } catch (err) {
      set({ doc: null, error: String(err), saveState: "error" });
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
}));
