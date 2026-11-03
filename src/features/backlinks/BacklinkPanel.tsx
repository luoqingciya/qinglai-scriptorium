import { useWorkspace } from "../../store/workspace";

/**
 * 反链面板：显示哪些章节引用了当前章。
 *
 * 默认收起。写作时宽度是稀缺资源，反链是「偶尔查一下」的信息，
 * 不该常驻占据编辑区。
 */
export function BacklinkPanel() {
  const backlinks = useWorkspace((s) => s.backlinks);
  const openDoc = useWorkspace((s) => s.openDoc);
  const close = useWorkspace((s) => s.toggleBacklinks);
  // 空状态里给出可直接照抄的示例，用当前章自己的标题
  const currentTitle = useWorkspace((s) => s.doc?.front?.title ?? s.doc?.path ?? "标题");

  const total = backlinks.reduce((sum, hit) => sum + hit.count, 0);

  return (
    <aside className="backlinks">
      <header className="backlinks-head">
        <span className="backlinks-title">反向链接</span>
        <span className="backlinks-count">{total}</span>
        <button type="button" className="backlinks-close" title="收起面板" onClick={close}>
          ×
        </button>
      </header>

      <div className="backlinks-body">
        {backlinks.length === 0 ? (
          <div className="backlinks-empty">
            <p>还没有章节引用这一章。</p>
            <p className="hint">
              在任意章节里写 <code>[[{currentTitle}]]</code> 即可建立链接。
            </p>
          </div>
        ) : (
          backlinks.map((hit) => (
            <div key={hit.path} className="backlink-item">
              <button
                type="button"
                className="backlink-head"
                title={hit.path}
                onClick={() => void openDoc(hit.path)}
              >
                <span className="backlink-name">{hit.title}</span>
                <span className="backlink-num">{hit.count}</span>
              </button>
              {hit.dir && <div className="backlink-dir">{hit.dir}</div>}
              {hit.contexts.slice(0, 3).map((ctx, i) => (
                <div key={i} className="backlink-context">
                  {ctx}
                </div>
              ))}
              {hit.contexts.length > 3 && (
                <div className="backlink-more">还有 {hit.contexts.length - 3} 处</div>
              )}
            </div>
          ))
        )}
      </div>
    </aside>
  );
}
