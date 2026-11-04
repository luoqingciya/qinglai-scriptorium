import { useEffect, useState } from "react";

import { readVersion } from "../../lib/ipc";
import { useWorkspace } from "../../store/workspace";

/** 把 RFC3339 转成「11-04 17:50」 */
function shortTime(at: string): string {
  const d = new Date(at);
  if (Number.isNaN(d.getTime())) return at;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

/**
 * 版本历史面板。
 *
 * 存的是「被覆盖的旧正文」，所以这里的每一条都代表「曾经存在过、后来被改掉的内容」。
 * 点条目先预览，确认后再恢复 —— 直接恢复太容易手滑。
 */
export function VersionPanel() {
  const versions = useWorkspace((s) => s.versions);
  const activePath = useWorkspace((s) => s.activePath);
  const close = useWorkspace((s) => s.toggleVersions);
  const restoreVersion = useWorkspace((s) => s.restoreVersion);

  const [selected, setSelected] = useState<string | null>(null);
  const [preview, setPreview] = useState<string>("");
  const [loading, setLoading] = useState(false);

  // 换章时清空选中，避免预览停留在上一章的内容上
  useEffect(() => {
    setSelected(null);
    setPreview("");
  }, [activePath]);

  async function pick(hash: string) {
    setSelected(hash);
    setLoading(true);
    try {
      const text = await readVersion(hash);
      setPreview(text);
    } catch {
      setPreview("");
    } finally {
      setLoading(false);
    }
  }

  async function onRestore() {
    if (!activePath || !selected) return;
    await restoreVersion(activePath, selected);
    setSelected(null);
    setPreview("");
  }

  return (
    <aside className="version-panel" aria-label="版本历史">
      <header className="version-head">
        <span className="version-title">版本历史</span>
        <button type="button" className="ghost-button" onClick={close}>
          收起
        </button>
      </header>

      {versions.length === 0 ? (
        <div className="version-empty">
          <p>还没有历史版本。</p>
          <p className="hint">
            每次保存会间隔 5 分钟记一档，改动多的章会先攒出记录。
          </p>
        </div>
      ) : (
        <ul className="version-list">
          {versions.map((v) => (
            <li key={v.hash}>
              <button
                type="button"
                className={`version-item${selected === v.hash ? " is-active" : ""}`}
                onClick={() => void pick(v.hash)}
              >
                <span className="version-time">{shortTime(v.at)}</span>
                <span className="version-size">{formatBytes(v.bytes)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}

      {selected && (
        <div className="version-preview">
          <div className="version-preview-head">
            <span>预览</span>
            <button
              type="button"
              className="ghost-button version-restore"
              disabled={loading}
              onClick={() => void onRestore()}
            >
              恢复此版本
            </button>
          </div>
          <div className="version-preview-body">
            {loading ? "读取中…" : preview || "（空内容）"}
          </div>
          <p className="version-preview-hint">
            恢复只替换正文，front matter 保留当前值；恢复前会把当前正文再存一档。
          </p>
        </div>
      )}
    </aside>
  );
}
