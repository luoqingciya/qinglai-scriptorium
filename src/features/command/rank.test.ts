import { describe, expect, it } from "vitest";

import { rank, type Rankable } from "./rank";

interface Item extends Rankable {
  id: string;
}

const items: Item[] = [
  { id: "a", title: "楔子", keywords: "xiezi prologue" },
  { id: "b", title: "夜航", keywords: "yehang chapter" },
  { id: "c", title: "沈砚", keywords: "shenyan character" },
  { id: "d", title: "打开稿库", keywords: "open vault" },
  { id: "e", title: "楔子·修订版", keywords: "" },
];

const ids = (query: string, list: readonly Item[] = items) =>
  rank(query, list).map((i) => i.id);

describe("命令检索排序", () => {
  it("空查询原样返回，不打乱顺序", () => {
    expect(ids("")).toEqual(["a", "b", "c", "d", "e"]);
    expect(ids("   ")).toEqual(["a", "b", "c", "d", "e"]);
  });

  it("中文子串匹配", () => {
    expect(ids("楔子")).toEqual(["a", "e"]);
  });

  it("英文关键词匹配且大小写不敏感", () => {
    expect(ids("yehang")).toEqual(["b"]);
    expect(ids("YEHANG")).toEqual(["b"]);
    expect(ids("OPEN")).toEqual(["d"]);
  });

  it("多个词按且组合", () => {
    // "open" 命中 d 的关键词，"稿库" 命中 d 的标题
    expect(ids("open 稿库")).toEqual(["d"]);
    // 两个词分别落在不同条目上，不该同时命中
    expect(ids("楔子 open")).toEqual([]);
  });

  it("前缀命中排在中间命中之前", () => {
    const list: Item[] = [
      { id: "mid", title: "新楔子", keywords: "" },
      { id: "prefix", title: "楔子", keywords: "" },
    ];
    expect(ids("楔子", list)).toEqual(["prefix", "mid"]);
  });

  it("短标题排在长标题之前", () => {
    expect(ids("楔子")).toEqual(["a", "e"]);
  });

  it("同分保持原顺序，结果不抖动", () => {
    const list: Item[] = [
      { id: "1", title: "甲", keywords: "" },
      { id: "2", title: "乙", keywords: "" },
    ];
    const both = [
      { id: "1", title: "章甲", keywords: "" },
      { id: "2", title: "章乙", keywords: "" },
    ];
    expect(ids("章", both)).toEqual(["1", "2"]);
    expect(list).toHaveLength(2);
  });

  it("无命中返回空数组", () => {
    expect(ids("完全不存在的词")).toEqual([]);
  });

  it("不修改传入数组", () => {
    const list = [...items];
    rank("楔子", list);
    expect(list).toEqual(items);
  });

  it("首尾空白与多个空格不影响结果", () => {
    expect(ids("  open   稿库  ")).toEqual(["d"]);
  });
});
