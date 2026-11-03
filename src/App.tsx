import { useEffect } from "react";

import { BacklinkPanel } from "./features/backlinks/BacklinkPanel";
import { EditorPane } from "./features/editor/EditorPane";
import { Explorer } from "./features/explorer/Explorer";
import { StatusBar } from "./features/statusbar/StatusBar";
import { useWorkspace } from "./store/workspace";

function formatTime(at: number): string {
  return new Date(at).toLocaleString("zh-CN", { hour12: false });
}

export default function App() {
  const doc = useWorkspace((s) => s.doc);
  const docEpoch = useWorkspace((s) => s.docEpoch);
  const snapshot = useWorkspace((s) => s.snapshot);
  const error = useWorkspace((s) => s.error);
  const dismissError = useWorkspace((s) => s.dismissError);
  const notice = useWorkspace((s) => s.notice);
  const dismissNotice = useWorkspace((s) => s.dismissNotice);
  const flush = useWorkspace((s) => s.flush);
  const restoreLastVault = useWorkspace((s) => s.restoreLastVault);
  const recovery = useWorkspace((s) => s.pendingRecovery);
  const applyRecovery = useWorkspace((s) => s.applyRecovery);
  const discardRecovery = useWorkspace((s) => s.discardRecovery);
  const focusMode = useWorkspace((s) => s.focusMode);
  const backlinksOpen = useWorkspace((s) => s.backlinksOpen);

  // 启动时恢复上次打开的稿库，并检查有无崩溃残留
  useEffect(() => {
    void restoreLastVault();
  }, [restoreLastVault]);

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

  // F11 切换专注模式
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "F11") {
        e.preventDefault();
        useWorkspace.getState().toggleFocus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const recoveryTitle = recovery
    ? (snapshot?.chapters.find((c) => c.path === recovery.path)?.title ?? recovery.path)
    : "";

  return (
    <div
      className={`app${focusMode ? " is-focus" : ""}${backlinksOpen && doc ? " has-backlinks" : ""}`}
    >
      {!focusMode && <Explorer />}

      <main className="workspace">
        {recovery && (
          <div className="banner banner-recover" role="alert">
            <span>
              上次退出时「{recoveryTitle}」有未保存的改动（{formatTime(recovery.at)}），是否恢复？
            </span>
            <button type="button" className="ghost-button" onClick={() => void applyRecovery()}>
              恢复
            </button>
            <button type="button" className="ghost-button" onClick={discardRecovery}>
              丢弃
            </button>
          </div>
        )}

        {error && (
          <div className="banner banner-error" role="alert">
            <span>{error}</span>
            <button type="button" className="ghost-button" onClick={dismissError}>
              知道了
            </button>
          </div>
        )}

        {notice && (
          <div className="banner banner-notice" role="status">
            <span>{notice}</span>
            <button type="button" className="ghost-button" onClick={dismissNotice}>
              知道了
            </button>
          </div>
        )}

        {doc ? (
          <EditorPane key={`${doc.path}#${docEpoch}`} />
        ) : (
          <div className="empty-state">
            <h1>清籁斋</h1>
            <p>左侧选择一章开始写，或先打开一个稿库目录。</p>
          </div>
        )}
      </main>

      {backlinksOpen && doc && <BacklinkPanel />}

      <StatusBar />
    </div>
  );
}
