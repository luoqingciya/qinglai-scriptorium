import { useRef } from "react";

import { useWorkspace } from "../../store/workspace";
import { useMilkdown } from "./useMilkdown";

/**
 * 编辑区。
 *
 * 调用方用 `key={path#epoch}` 挂载，切章或强制重载即重建编辑器 ——
 * 这样无需处理异步 replaceAll 的时序问题，撤销历史也自然按章隔离。
 */
export function EditorPane() {
  const containerRef = useRef<HTMLDivElement>(null);
  const setBody = useWorkspace((s) => s.setBody);
  const adoptBaseline = useWorkspace((s) => s.adoptBaseline);
  // 与 doc.body 分开：崩溃恢复时编辑器要以日志内容挂载，而 doc.body 仍是磁盘上的内容
  const editorContent = useWorkspace((s) => s.editorContent);

  useMilkdown({
    containerRef,
    initialMarkdown: editorContent,
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
