/**
 * 自动保存与崩溃恢复的状态机测试。
 *
 * 这一层的坑在于：防抖、落盘、日志三者时序交错，任何一处顺序写反都会造成
 * 「日志已清但没落盘」或「恢复的内容被基线覆盖」这类静默丢字。
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipc", () => ({
  openVault: vi.fn(),
  scanVault: vi.fn(),
  readDoc: vi.fn(),
  saveDoc: vi.fn(),
  createDoc: vi.fn(),
  renameDoc: vi.fn(),
  deleteDoc: vi.fn(),
  reorderDoc: vi.fn(),
  countWords: vi.fn(),
}));

import * as ipc from "../lib/ipc";
import { clearJournal, readJournal, writeJournal } from "../lib/journal";
import type { DocPayload, VaultSnapshot, WordCount } from "../lib/types";
import { JOURNAL_DEBOUNCE_MS, useWorkspace } from "./workspace";

const mocked = vi.mocked(ipc);

const WORDS: WordCount = { cjk: 0, latin: 0, punctuation: 0, total: 0 };

function makeDoc(path: string, body: string): DocPayload {
  return {
    path,
    body,
    front: null,
    hasFrontMatter: false,
    stamp: { mtimeMs: 1, len: body.length },
    words: WORDS,
  };
}

function makeSnapshot(paths: string[]): VaultSnapshot {
  return {
    root: "/vault",
    title: "测试库",
    countPunctuation: false,
    chapters: paths.map((path, i) => ({
      path,
      dir: "",
      title: path,
      order: i + 1,
      status: null,
      words: 0,
      id: null,
    })),
    totalWords: 0,
    scannedMs: 0,
  };
}

function fakeLocalStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllTimers();
  vi.resetAllMocks();
  vi.stubGlobal("localStorage", fakeLocalStorage());

  useWorkspace.setState({
    snapshot: null,
    activePath: null,
    doc: null,
    body: "",
    lastSaved: "",
    editorContent: "",
    docEpoch: 0,
    pendingRecovery: null,
    words: WORDS,
    saveState: "idle",
    error: null,
  });
});

describe("恢复日志的写入时机", () => {
  it("输入后先写日志，不等自动保存", async () => {
    useWorkspace.setState({
      activePath: "a.md",
      doc: makeDoc("a.md", "旧内容"),
      body: "旧内容",
      lastSaved: "旧内容",
    });

    useWorkspace.getState().setBody("新内容");
    expect(readJournal()).toBeNull();

    await vi.advanceTimersByTimeAsync(JOURNAL_DEBOUNCE_MS);
    expect(readJournal()?.body).toBe("新内容");
    expect(readJournal()?.path).toBe("a.md");
  });

  it("日志比自动保存先落地（丢失窗口更小）", async () => {
    useWorkspace.setState({
      activePath: "a.md",
      doc: makeDoc("a.md", "旧内容"),
      body: "旧内容",
      lastSaved: "旧内容",
    });

    useWorkspace.getState().setBody("新内容");
    await vi.advanceTimersByTimeAsync(JOURNAL_DEBOUNCE_MS);

    expect(readJournal()?.body).toBe("新内容");
    // 此时自动保存还没触发
    expect(mocked.saveDoc).not.toHaveBeenCalled();
  });

  it("成功落盘后日志被清除", async () => {
    mocked.saveDoc.mockResolvedValue({
      kind: "saved",
      stamp: { mtimeMs: 2, len: 6 },
      words: WORDS,
    });
    useWorkspace.setState({
      activePath: "a.md",
      doc: makeDoc("a.md", "旧内容"),
      body: "旧内容",
      lastSaved: "旧内容",
    });

    useWorkspace.getState().setBody("新内容");
    await vi.advanceTimersByTimeAsync(JOURNAL_DEBOUNCE_MS);
    expect(readJournal()).not.toBeNull();

    await useWorkspace.getState().flush();
    expect(readJournal()).toBeNull();
    expect(useWorkspace.getState().saveState).toBe("saved");
  });

  it("冲突时保留日志，改动仍可找回", async () => {
    mocked.saveDoc.mockResolvedValue({ kind: "conflict", stamp: { mtimeMs: 9, len: 9 } });
    useWorkspace.setState({
      activePath: "a.md",
      doc: makeDoc("a.md", "旧内容"),
      body: "旧内容",
      lastSaved: "旧内容",
    });

    useWorkspace.getState().setBody("新内容");
    await vi.advanceTimersByTimeAsync(JOURNAL_DEBOUNCE_MS);
    await useWorkspace.getState().flush();

    expect(useWorkspace.getState().saveState).toBe("conflict");
    expect(readJournal()?.body).toBe("新内容");
  });

  it("未改动时 flush 不写盘", async () => {
    useWorkspace.setState({
      activePath: "a.md",
      doc: makeDoc("a.md", "内容"),
      body: "内容",
      lastSaved: "内容",
    });

    await useWorkspace.getState().flush();
    expect(mocked.saveDoc).not.toHaveBeenCalled();
  });
});

describe("启动时的恢复判定", () => {
  it("日志与磁盘不同则提示恢复", async () => {
    useWorkspace.setState({ snapshot: makeSnapshot(["a.md"]) });
    writeJournal({ path: "a.md", body: "日志里的内容", at: 1000 });
    mocked.readDoc.mockResolvedValue(makeDoc("a.md", "磁盘上的内容"));

    await useWorkspace.getState().checkJournal();

    expect(useWorkspace.getState().pendingRecovery?.body).toBe("日志里的内容");
    expect(readJournal()).not.toBeNull();
  });

  it("日志与磁盘相同则静默清除", async () => {
    useWorkspace.setState({ snapshot: makeSnapshot(["a.md"]) });
    writeJournal({ path: "a.md", body: "一样的内容", at: 1000 });
    mocked.readDoc.mockResolvedValue(makeDoc("a.md", "一样的内容"));

    await useWorkspace.getState().checkJournal();

    expect(useWorkspace.getState().pendingRecovery).toBeNull();
    expect(readJournal()).toBeNull();
  });

  it("章节已不存在则清除日志", async () => {
    useWorkspace.setState({ snapshot: makeSnapshot(["b.md"]) });
    writeJournal({ path: "a.md", body: "内容", at: 1000 });

    await useWorkspace.getState().checkJournal();

    expect(useWorkspace.getState().pendingRecovery).toBeNull();
    expect(readJournal()).toBeNull();
  });

  it("没有日志时不做任何事", async () => {
    useWorkspace.setState({ snapshot: makeSnapshot(["a.md"]) });
    await useWorkspace.getState().checkJournal();
    expect(mocked.readDoc).not.toHaveBeenCalled();
    expect(useWorkspace.getState().pendingRecovery).toBeNull();
  });
});

describe("恢复执行", () => {
  it("把日志内容直接落盘，并重建编辑器", async () => {
    mocked.saveDoc.mockResolvedValue({
      kind: "saved",
      stamp: { mtimeMs: 5, len: 9 },
      words: WORDS,
    });
    useWorkspace.setState({
      snapshot: makeSnapshot(["a.md"]),
      activePath: "a.md",
      doc: makeDoc("a.md", "磁盘上的内容"),
      body: "磁盘上的内容",
      lastSaved: "磁盘上的内容",
      editorContent: "磁盘上的内容",
      pendingRecovery: { path: "a.md", body: "日志里的内容", at: 1000 },
    });

    await useWorkspace.getState().applyRecovery();

    expect(mocked.saveDoc).toHaveBeenCalledWith("a.md", "日志里的内容", expect.anything());
    const state = useWorkspace.getState();
    expect(state.body).toBe("日志里的内容");
    expect(state.lastSaved).toBe("日志里的内容");
    expect(state.editorContent).toBe("日志里的内容");
    expect(state.docEpoch).toBeGreaterThan(0);
    expect(state.pendingRecovery).toBeNull();
    expect(readJournal()).toBeNull();
  });

  it("丢弃恢复会清掉日志且不改正文", async () => {
    writeJournal({ path: "a.md", body: "日志里的内容", at: 1000 });
    useWorkspace.setState({
      pendingRecovery: { path: "a.md", body: "日志里的内容", at: 1000 },
      body: "磁盘上的内容",
    });

    useWorkspace.getState().discardRecovery();

    expect(useWorkspace.getState().pendingRecovery).toBeNull();
    expect(readJournal()).toBeNull();
    expect(useWorkspace.getState().body).toBe("磁盘上的内容");
    expect(mocked.saveDoc).not.toHaveBeenCalled();
  });

  it("章节已被移走时恢复失败并清日志", async () => {
    useWorkspace.setState({
      activePath: "other.md",
      doc: makeDoc("other.md", "别的章"),
      pendingRecovery: { path: "a.md", body: "日志里的内容", at: 1000 },
    });
    mocked.readDoc.mockRejectedValue("文件不存在");
    clearJournal();

    await useWorkspace.getState().applyRecovery();

    expect(mocked.saveDoc).not.toHaveBeenCalled();
    expect(useWorkspace.getState().error).toContain("恢复失败");
  });
});
