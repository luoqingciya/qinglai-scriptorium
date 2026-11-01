import { useRef } from "react";

import { useWorkspace } from "../../store/workspace";
import type { DocPayload } from "../../lib/types";
import { useMilkdown } from "./useMilkdown";

/**
 * 单章编辑区。
 *
 * 调用方用 `key={doc.path}` 挂载，切换章节即重建编辑器 —— 这样无需处理
 * 异步 replaceAll 的时序问题，撤销历史也自然按章隔离。
 */
export function EditorPane({ doc }: { doc: DocPayload }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const setBody = useWorkspace((s) => s.setBody);
  const adoptBaseline = useWorkspace((s) => s.adoptBaseline);

  useMilkdown({
    containerRef,
    initialMarkdown: doc.body,
    onChange: setBody,
    onBaseline: adoptBaseline,
  });

  return (
    <div className="editor-scroll">
      <div className="editor-page">
        <div ref={containerRef} className="editor-host" />
      </div>
    </div>
  );
}
