/**
 * 双链扫描规则的测试。
 *
 * 只测纯函数层（`findWikilinks`）：ProseMirror 的文档树需要真实 DOM，
 * jsdom 里跑不起来。但装饰器本身不改变文档结构，扫描规则对了，
 * 高亮位置就对，序列化也不受影响。
 *
 * 这些用例与 Rust 侧 `links.rs` 的测试一一对应，两边规则必须同步。
 */
import { describe, expect, it } from "vitest";

import { findWikilinks } from "./wikilink";

/** 取第一处双链，顺带断言确实存在，省得每个用例都写 `!` */
function first(text: string) {
  const spans = findWikilinks(text);
  const span = spans[0];
  if (!span) throw new Error(`没有找到双链：${text}`);
  return span;
}

describe("findWikilinks", () => {
  it("找出单个双链及其范围", () => {
    const text = "他想起[[楔子]]里那句话。";
    const span = first(text);

    expect(span.target).toBe("楔子");
    // 范围要覆盖完整的 [[楔子]]
    expect(text.slice(span.start, span.end)).toBe("[[楔子]]");
  });

  it("一个段落里多处双链都能找出", () => {
    const spans = findWikilinks("[[甲]]与[[乙]]都提到了[[丙]]。");
    expect(spans.map((s) => s.target)).toEqual(["甲", "乙", "丙"]);
  });

  it("带别名的双链只取目标部分", () => {
    expect(first("见[[第一章|开篇]]。").target).toBe("第一章");
  });

  it("带锚点的双链只取标题部分", () => {
    expect(first("[[楔子#开头]]").target).toBe("楔子");
  });

  it("未闭合的括号不误报", () => {
    expect(findWikilinks("这里有个[[没闭合的括号")).toEqual([]);
  });

  it("跨行的双链不算链接", () => {
    expect(findWikilinks("前面[[这里\n换行了]]后面")).toEqual([]);
  });

  it("空目标被忽略", () => {
    expect(findWikilinks("[[]] 和 [[  ]] 都不算")).toEqual([]);
  });

  it("目标首尾空白被去掉", () => {
    expect(first("[[ 楔子 ]]").target).toBe("楔子");
  });

  it("相邻的两个双链互不干扰", () => {
    const spans = findWikilinks("[[甲]][[乙]]");
    expect(spans.map((s) => s.target)).toEqual(["甲", "乙"]);
  });
});
