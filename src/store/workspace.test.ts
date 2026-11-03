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
  searchVault: vi.fn(),
  backlinks: vi.fn(),
  outlinks: vi.fn(),
  danglingLinks: vi.fn(),
  resolveLink: vi.fn(),
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
    focusMode: false,
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

describe("专注模式", () => {
  it("toggleFocus 切换状态并持久化", () => {
    expect(useWorkspace.getState().focusMode).toBe(false);

    useWorkspace.getState().toggleFocus();
    expect(useWorkspace.getState().focusMode).toBe(true);
    expect(localStorage.getItem("qinglai.focus.v1")).toBe("1");

    useWorkspace.getState().toggleFocus();
    expect(useWorkspace.getState().focusMode).toBe(false);
    expect(localStorage.getItem("qinglai.focus.v1")).toBe("0");
  });
});

describe("双链与反链", () => {
  it("打开文档时自动拉取反链", async () => {
    mocked.readDoc.mockResolvedValue(makeDoc("a.md", "正文"));
    mocked.backlinks.mockResolvedValue([
      { path: "b.md", title: "乙", dir: "", count: 2, contexts: ["…[[甲]]…"] },
    ]);

    await useWorkspace.getState().openDoc("a.md");
    // loadBacklinks 是 fire-and-forget，等一轮微任务
    await Promise.resolve();

    expect(mocked.backlinks).toHaveBeenCalledWith("a.md");
    expect(useWorkspace.getState().backlinks).toHaveLength(1);
  });

  it("切章后旧的反链请求结果被丢弃", async () => {
    mocked.readDoc.mockResolvedValue(makeDoc("a.md", "正文"));
    // 模拟慢请求：a.md 的反链在切到 b.md 之后才返回
    let resolveA: (v: unknown) => void = () => {};
    mocked.backlinks.mockImplementationOnce(
      () => new Promise((r) => (resolveA = r as (v: unknown) => void)),
    );
    mocked.backlinks.mockResolvedValueOnce([]);

    await useWorkspace.getState().openDoc("a.md");

    mocked.readDoc.mockResolvedValue(makeDoc("b.md", "另一章"));
    await useWorkspace.getState().openDoc("b.md");

    // 此时 a.md 的请求才回来
    resolveA([{ path: "c.md", title: "丙", dir: "", count: 1, contexts: ["x"] }]);
    await Promise.resolve();
    await Promise.resolve();

    // 当前是 b.md，a.md 的迟到结果不能覆盖
    expect(useWorkspace.getState().activePath).toBe("b.md");
    expect(useWorkspace.getState().backlinks).toEqual([]);
  });

  it("反链查询失败时留空，不打断写作", async () => {
    mocked.readDoc.mockResolvedValue(makeDoc("a.md", "正文"));
    mocked.backlinks.mockRejectedValue("索引未就绪");

    await useWorkspace.getState().openDoc("a.md");
    await Promise.resolve();

    expect(useWorkspace.getState().backlinks).toEqual([]);
    expect(useWorkspace.getState().error).toBeNull();
  });

  it("按标题跳转：解析成功则打开目标章", async () => {
    mocked.resolveLink.mockResolvedValue("b.md");
    mocked.readDoc.mockResolvedValue(makeDoc("b.md", "乙的正文"));

    await useWorkspace.getState().openByTitle("乙");

    expect(mocked.resolveLink).toHaveBeenCalledWith("乙");
    expect(useWorkspace.getState().activePath).toBe("b.md");
  });

  it("按标题跳转：目标不存在时给出明确提示", async () => {
    mocked.resolveLink.mockResolvedValue(null);

    await useWorkspace.getState().openByTitle("不存在的章");

    expect(useWorkspace.getState().error).toContain("找不到章节");
    expect(mocked.readDoc).not.toHaveBeenCalled();
  });

  it("toggleBacklinks 切换面板展开状态", () => {
    expect(useWorkspace.getState().backlinksOpen).toBe(false);
    useWorkspace.getState().toggleBacklinks();
    expect(useWorkspace.getState().backlinksOpen).toBe(true);
    useWorkspace.getState().toggleBacklinks();
    expect(useWorkspace.getState().backlinksOpen).toBe(false);
  });
});
