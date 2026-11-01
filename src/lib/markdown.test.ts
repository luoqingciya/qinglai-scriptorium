/**
 * Markdown 往返保真测试。
 *
 * 这一层测的是 Milkdown 底下的 remark 管线（parse → stringify）。
 * 它守护的是「中文标点不被转义」「结构不丢」这类最容易被序列化器破坏的东西。
 *
 * 注意：ProseMirror 文档树那一层的保真需要真实 DOM 才能验证，
 * 属于 GUI 手工验收项，不在本文件的覆盖范围内。
 */
import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkStringify from "remark-stringify";
import { describe, expect, it } from "vitest";

const processor = unified().use(remarkParse).use(
  remarkStringify,
  {
    bullet: "-",
    emphasis: "*",
    strong: "*",
    fences: true,
    listItemIndent: "one",
  } as const,
);

const roundtrip = (markdown: string): string => processor.processSync(markdown).toString();

const 样章 = `# 楔子

清籁斋的窗纸上落了一层薄雪。沈砚把笔搁在砚台上，听见檐角的铁马响了一声。

他忽然想起师父说过的话：「写字如听风，风过了，字还在。」

- 第一件事
- 第二件事

> 引文一行。

**加粗**与*斜体*，还有 \`代码\`。

他走了……然后——
`;

describe("Markdown 往返", () => {
  it("二次序列化不再变化（幂等）", () => {
    const once = roundtrip(样章);
    const twice = roundtrip(once);
    expect(twice).toBe(once);
  });

  it("中文标点不被转义", () => {
    const out = roundtrip(样章);
    expect(out).toContain("「写字如听风，风过了，字还在。」");
    expect(out).toContain("他走了……然后——");
  });

  it("标题、列表、引用、强调结构保留", () => {
    const out = roundtrip(样章);
    expect(out).toContain("# 楔子");
    expect(out).toContain("- 第一件事");
    expect(out).toContain("> 引文一行。");
    expect(out).toContain("**加粗**");
    expect(out).toContain("*斜体*");
    expect(out).toContain("`代码`");
  });

  it("全角空格不被吞掉", () => {
    const src = "　　段首空两格的一句话。\n";
    expect(roundtrip(src)).toContain("　　段首空两格的一句话。");
  });

  it("行尾双空格的硬换行会被规范化为反斜杠，但结果稳定", () => {
    const src = "第一行  \n第二行\n";
    const once = roundtrip(src);
    // 这是已知的规范化行为：Milkdown 序列化后统一用反斜杠表示硬换行
    expect(once).toContain("第一行");
    expect(roundtrip(once)).toBe(once);
  });

  it("连续空行折叠为单个空行", () => {
    const src = "第一段。\n\n\n\n第二段。\n";
    const out = roundtrip(src);
    expect(out).not.toContain("\n\n\n");
    expect(out).toContain("第一段。");
    expect(out).toContain("第二段。");
  });

  it("空字符串往返后仍为空", () => {
    expect(roundtrip("")).toBe("");
  });

  it("纯中文长段落逐字保真", () => {
    const src = "风声穿过窗棂，像是有人在很远的地方翻动一页旧纸。\n";
    expect(roundtrip(src).trim()).toBe(src.trim());
  });
});
