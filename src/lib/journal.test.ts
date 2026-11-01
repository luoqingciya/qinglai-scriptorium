import { describe, expect, it } from "vitest";

import {
  clearJournal,
  clearLastVault,
  readJournal,
  readLastVault,
  writeJournal,
  writeLastVault,
  type KeyValueStore,
} from "./journal";

/** 内存版 KeyValueStore，行为对齐 localStorage 的字符串语义 */
function memoryStore(): KeyValueStore & { dump: () => Map<string, string> } {
  const map = new Map<string, string>();
  return {
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, v),
    removeItem: (k) => void map.delete(k),
    dump: () => map,
  };
}

describe("崩溃恢复日志", () => {
  it("写入后能读回", () => {
    const store = memoryStore();
    writeJournal({ path: "卷一/001-楔子.md", body: "正文。\n", at: 1767000000000 }, store);

    const entry = readJournal(store);
    expect(entry).not.toBeNull();
    expect(entry?.path).toBe("卷一/001-楔子.md");
    expect(entry?.body).toBe("正文。\n");
    expect(entry?.at).toBe(1767000000000);
  });

  it("中文与换行原样保留", () => {
    const store = memoryStore();
    const body = "他说：「走吧。」\n\n她没应声。\n——\n";
    writeJournal({ path: "a.md", body, at: 1 }, store);
    expect(readJournal(store)?.body).toBe(body);
  });

  it("清除后读到 null", () => {
    const store = memoryStore();
    writeJournal({ path: "a.md", body: "x", at: 1 }, store);
    clearJournal(store);
    expect(readJournal(store)).toBeNull();
  });

  it("损坏的 JSON 不会抛出，且会被清掉", () => {
    const store = memoryStore();
    store.setItem("qinglai.journal.v1", "{不是合法 JSON");
    expect(readJournal(store)).toBeNull();
    expect(store.dump().has("qinglai.journal.v1")).toBe(false);
  });

  it("结构不对的日志被丢弃", () => {
    const store = memoryStore();
    store.setItem("qinglai.journal.v1", JSON.stringify({ path: "a.md" }));
    expect(readJournal(store)).toBeNull();
    expect(store.dump().has("qinglai.journal.v1")).toBe(false);
  });

  it("没有存储可用时静默降级", () => {
    expect(readJournal(null)).toBeNull();
    expect(() => writeJournal({ path: "a.md", body: "x", at: 1 }, null)).not.toThrow();
    expect(() => clearJournal(null)).not.toThrow();
  });

  it("setItem 抛错时不影响写作", () => {
    const broken: KeyValueStore = {
      getItem: () => null,
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
      removeItem: () => {
        throw new Error("QuotaExceededError");
      },
    };
    expect(() => writeJournal({ path: "a.md", body: "x", at: 1 }, broken)).not.toThrow();
    expect(() => clearJournal(broken)).not.toThrow();
  });

  it("上次稿库路径可存取与清除", () => {
    const store = memoryStore();
    expect(readLastVault(store)).toBeNull();

    writeLastVault("D:/稿子/清籁斋", store);
    expect(readLastVault(store)).toBe("D:/稿子/清籁斋");

    clearLastVault(store);
    expect(readLastVault(store)).toBeNull();
  });
});
