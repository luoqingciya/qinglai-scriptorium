import { useEffect } from "react";

import { EditorPane } from "./features/editor/EditorPane";
import { Explorer } from "./features/explorer/Explorer";
import { StatusBar } from "./features/statusbar/StatusBar";
import { useWorkspace } from "./store/workspace";

export default function App() {
  const doc = useWorkspace((s) => s.doc);
  const error = useWorkspace((s) => s.error);
  const dismissError = useWorkspace((s) => s.dismissError);
  const flush = useWorkspace((s) => s.flush);

  // 关窗或切到后台前把待保存内容落盘
  useEffect(() => {
    const onHide = () => void flush();
    window.addEventListener("blur", onHide);
    window.addEventListener("beforeunload", onHide);
    return () => {
      window.removeEventListener("blur", onHide);
      window.removeEventListener("beforeunload", onHide);
    };
  }, [flush]);

  return (
    <div className="app">
      <Explorer />

      <main className="workspace">
        {error && (
          <div className="banner banner-error" role="alert">
            <span>{error}</span>
            <button type="button" className="ghost-button" onClick={dismissError}>
              知道了
            </button>
          </div>
        )}

        {doc ? (
          <EditorPane key={doc.path} doc={doc} />
        ) : (
          <div className="empty-state">
            <h1>清籁斋</h1>
            <p>左侧选择一章开始写，或先打开一个稿库目录。</p>
          </div>
        )}
      </main>

      <StatusBar />
    </div>
  );
}
