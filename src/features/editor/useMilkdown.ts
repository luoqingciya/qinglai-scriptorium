/**
 * Milkdown 的 React 包装。
 *
 * 刻意不使用 `@milkdown/react`：该包在 7.23 内部锁定了 `@milkdown/kit@7.22.3`
 * 与 `@milkdown/crepe`，版本耦合会在升级时连锁失效。这里直接用 vanilla
 * `Editor`，生命周期由我们自己掌握。
 */
import { useEffect, useRef } from "react";
import type { RefObject } from "react";

import { Editor, defaultValueCtx, rootCtx } from "@milkdown/core";
import { history } from "@milkdown/plugin-history";
import { listener, listenerCtx } from "@milkdown/plugin-listener";
import { commonmark } from "@milkdown/preset-commonmark";
import { getMarkdown } from "@milkdown/utils";

interface Options {
  containerRef: RefObject<HTMLDivElement | null>;
  initialMarkdown: string;
  onChange: (markdown: string) => void;
  /** 编辑器就绪后回传其自身序列化基线 */
  onBaseline: (markdown: string) => void;
}

export function useMilkdown({ containerRef, initialMarkdown, onChange, onBaseline }: Options) {
  // 用 ref 持有回调，避免回调变化触发编辑器重建
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onBaselineRef = useRef(onBaseline);
  onBaselineRef.current = onBaseline;

  useEffect(() => {
    const host = containerRef.current;
    if (!host) return;

    let disposed = false;
    let created = false;
    // 就绪前不派发变更：初始化过程本身会触发一次 markdownUpdated
    let armed = false;

    const editor = Editor.make()
      .config((ctx) => {
        ctx.set(rootCtx, host);
        ctx.set(defaultValueCtx, initialMarkdown);
        ctx.get(listenerCtx).markdownUpdated((_ctx, markdown) => {
          if (!armed) return;
          onChangeRef.current(markdown);
        });
      })
      .use(commonmark)
      .use(history)
      .use(listener);

    editor
      .create()
      .then(() => {
        if (disposed) {
          void editor.destroy();
          return;
        }
        created = true;
        const baseline = editor.action(getMarkdown());
        onBaselineRef.current(baseline);
        armed = true;
      })
      .catch((err: unknown) => {
        console.error("编辑器初始化失败", err);
      });

    return () => {
      disposed = true;
      if (created) void editor.destroy();
    };
  }, [containerRef, initialMarkdown]);
}
