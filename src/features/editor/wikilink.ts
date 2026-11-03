/**
 * 编辑器内 `[[双链]]` 的高亮与点击。
 *
 * **刻意用 Decoration 而不是自定义 node/mark。**
 *
 * 自定义 node 需要同时写 schema、parser、serializer 三处规则，任何一处偏差都会
 * 让 Markdown 往返失真 —— 这是本项目最不能接受的风险（见 architecture.md §3）。
 * Decoration 只是在已有文本上叠一层视觉样式，**不改变文档结构**，
 * 因此 `getMarkdown()` 的输出与没有这个插件时完全一致。
 *
 * 代价是 `[[...]]` 在文档里始终是纯文本，用户不能用光标「选中一个链接节点」，
 * 但对写作场景来说这不重要 —— 作者要的是看见并点得动。
 */
import { Plugin, PluginKey } from "@milkdown/prose/state";
import type { Node as ProseNode } from "@milkdown/prose/model";
import { Decoration, DecorationSet } from "@milkdown/prose/view";
import { $prose } from "@milkdown/utils";

export const wikilinkKey = new PluginKey("qinglai-wikilink");

/** 与 Rust 侧 `links::normalize` 保持一致：取 `#` 之前、去首尾空白 */
function normalizeTarget(raw: string): string {
  const head = raw.split("#")[0] ?? raw;
  return head.trim();
}

/** 一处双链在文本里的位置（相对该文本节点） */
export interface WikilinkSpan {
  start: number;
  end: number;
  target: string;
}

/**
 * 在一段纯文本里找出所有 `[[目标]]`。
 *
 * 抽成纯函数是为了可测试 —— ProseMirror 的文档树需要真实 DOM，
 * 在 jsdom 里跑不起来。扫描规则与 Rust 侧 `links::extract` 保持一致：
 * 不跨行、空目标忽略、`#锚点` 只取标题部分。
 */
export function findWikilinks(text: string): WikilinkSpan[] {
  const spans: WikilinkSpan[] = [];
  let cursor = 0;

  while (cursor < text.length - 1) {
    const open = text.indexOf("[[", cursor);
    if (open < 0) break;

    const close = text.indexOf("]]", open + 2);
    if (close < 0) break;

    const inner = text.slice(open + 2, close);
    // 跨行不算链接，与 Rust 侧规则一致
    if (!inner.includes("\n")) {
      const target = normalizeTarget(inner.split("|")[0] ?? inner);
      if (target) spans.push({ start: open, end: close + 2, target });
    }

    cursor = close + 2;
  }

  return spans;
}

function buildDecorations(doc: ProseNode): DecorationSet {
  const decos: Decoration[] = [];

  doc.descendants((node, pos) => {
    if (!node.isText || !node.text) return;

    for (const span of findWikilinks(node.text)) {
      decos.push(
        Decoration.inline(pos + span.start, pos + span.end, {
          class: "wikilink",
          "data-wikilink": span.target,
          title: `跳转到「${span.target}」`,
        }),
      );
    }
  });

  return DecorationSet.create(doc, decos);
}

export const wikilinkPlugin = $prose(
  () =>
    new Plugin({
      key: wikilinkKey,
      state: {
        init: (_config, state) => buildDecorations(state.doc),
        apply: (tr, previous) => (tr.docChanged ? buildDecorations(tr.doc) : previous),
      },
      props: {
        decorations(state) {
          return wikilinkKey.getState(state);
        },
      },
    }),
);

/**
 * 从点击事件里取出被点的 `[[目标]]`。
 *
 * 用 `closest` 而不是坐标命中测试：Decoration 生成的是带 `data-wikilink`
 * 的 `<span>`，浏览器已经帮我们做了命中判断。
 */
export function targetFromEvent(event: MouseEvent): string | null {
  const el = event.target;
  if (!(el instanceof HTMLElement)) return null;
  const link = el.closest<HTMLElement>("[data-wikilink]");
  return link?.dataset.wikilink ?? null;
}
