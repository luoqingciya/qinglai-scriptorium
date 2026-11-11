import { useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";

import { useTheme } from "../../lib/theme-store";
import { useWorkspace } from "../../store/workspace";
import { rank, type Rankable } from "./rank";

interface CommandItem extends Rankable {
  id: string;
  title: string;
  keywords: string;
  category: string;
  subtitle?: string;
  run: () => void | Promise<void>;
}

/** 一次最多显示多少条。再多就用键盘继续筛，而不是把面板撑满屏。 */
const VISIBLE_LIMIT = 12;

/**
 * 命令面板。
 *
 * 存在的理由是「功能可达性」：十几个功能散在侧栏 tab、状态栏按钮、F11 快捷键里，
 * 用久了记不住哪个在哪。这里给一个统一入口，并且把「按章跳转」也纳入同一套检索。
 *
 * 排序与匹配在 rank.ts 里做，是纯函数，可以单测；这里只负责取数据和执行。
 */
export function CommandPalette() {
  const close = () => useWorkspace.getState().setPaletteOpen(false);

  const snapshot = useWorkspace((s) => s.snapshot);
  const activePath = useWorkspace((s) => s.activePath);
  const focusMode = useWorkspace((s) => s.focusMode);
  const outlineOpen = useWorkspace((s) => s.outlineOpen);
  const statsOpen = useWorkspace((s) => s.statsOpen);
  const backlinksOpen = useWorkspace((s) => s.backlinksOpen);
  const versionsOpen = useWorkspace((s) => s.versionsOpen);
  const cycleTheme = useTheme((s) => s.cycle);

  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const commands = useMemo<CommandItem[]>(() => {
    const list: CommandItem[] = [
      {
        id: "vault.open",
        title: snapshot ? "换一个稿库" : "打开稿库",
        keywords: "open vault qinglai 目录",
        category: "稿库",
        run: async () => {
          close();
          const picked = await open({ directory: true, multiple: false, title: "选择稿库目录" });
          if (typeof picked === "string") {
            await useWorkspace.getState().openVault(picked);
          }
        },
      },
      {
        id: "vault.refresh",
        title: "刷新稿库",
        keywords: "refresh reload 重新扫描",
        category: "稿库",
        run: () => void useWorkspace.getState().refresh(),
      },
      {
        id: "view.outline",
        title: outlineOpen ? "关闭大纲" : "打开大纲",
        keywords: "outline summary 摘要 全书",
        category: "视图",
        run: () => useWorkspace.getState().toggleOutline(),
      },
      {
        id: "view.stats",
        title: statsOpen ? "关闭写作统计" : "写作统计",
        keywords: "stats trend words 字数 趋势",
        category: "视图",
        run: () => useWorkspace.getState().toggleStats(),
      },
      {
        id: "view.versions",
        title: versionsOpen ? "收起版本历史" : "查看版本历史",
        keywords: "version history snapshot 快照 恢复 回退 历史",
        category: "视图",
        run: () => useWorkspace.getState().toggleVersions(),
      },
      {
        id: "view.backlinks",
        title: backlinksOpen ? "收起反链面板" : "展开反链面板",
        keywords: "backlinks link 双链 引用",
        category: "视图",
        run: () => useWorkspace.getState().toggleBacklinks(),
      },
      {
        id: "view.focus",
        title: focusMode ? "退出专注模式" : "进入专注模式",
        keywords: "focus zen F11 沉浸",
        category: "视图",
        run: () => useWorkspace.getState().toggleFocus(),
      },
      {
        id: "view.theme",
        title: "切换主题",
        keywords: "theme dark light 护眼 暗色 亮色",
        category: "视图",
        run: cycleTheme,
      },
      {
        id: "action.export",
        title: "导出全库为 docx",
        keywords: "export word docx 导出",
        category: "操作",
        run: async () => {
          close();
          const dest = await save({
            title: "导出全库为 Word 文档",
            defaultPath: `${useWorkspace.getState().snapshot?.title ?? "未命名"}.docx`,
            filters: [{ name: "Word 文档", extensions: ["docx"] }],
          });
          if (typeof dest === "string") {
            await useWorkspace.getState().exportBook(dest);
          }
        },
      },
      {
        id: "action.exportEpub",
        title: "导出全库为 epub",
        keywords: "export epub ebook 电子书 阅读器",
        category: "操作",
        run: async () => {
          close();
          const dest = await save({
            title: "导出全库为 EPUB 电子书",
            defaultPath: `${useWorkspace.getState().snapshot?.title ?? "未命名"}.epub`,
            filters: [{ name: "EPUB 电子书", extensions: ["epub"] }],
          });
          if (typeof dest === "string") {
            await useWorkspace.getState().exportBook(dest, "epub");
          }
        },
      },
      {
        id: "action.search",
        title: "搜索全库",
        keywords: "search find 全文 检索",
        category: "操作",
        subtitle: "聚焦侧栏搜索框",
        run: () => {
          close();
          // 侧栏搜索框在章节视图里；先切回去，再聚焦
          window.setTimeout(() => {
            document.querySelector<HTMLInputElement>(".search-input")?.focus();
          }, 0);
        },
      },
    ];

    // 章节作为「跳转」类命令加入同一套检索
    if (snapshot) {
      for (const chapter of snapshot.chapters) {
        list.push({
          id: `doc.${chapter.path}`,
          title: chapter.title,
          keywords: `${chapter.path} ${chapter.dir}`,
          category: "章节",
          subtitle: chapter.dir || "根目录",
          run: () => void useWorkspace.getState().openDoc(chapter.path),
        });
      }
    }

    return list;
  }, [snapshot, activePath, focusMode, outlineOpen, statsOpen, backlinksOpen, versionsOpen, cycleTheme]);

  const results = useMemo(
    () => rank(query, commands).slice(0, VISIBLE_LIMIT),
    [query, commands],
  );

  // 查询结果变化后把选中项收回到第一项，避免 ↑↓ 越过边界
  useEffect(() => {
    setSelected(0);
  }, [query]);

  function execute(item: CommandItem) {
    close();
    void item.run();
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelected((i) => (results.length === 0 ? 0 : (i + 1) % results.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelected((i) =>
        results.length === 0 ? 0 : (i - 1 + results.length) % results.length,
      );
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = results[selected];
      if (item) execute(item);
    }
  }

  const totalMatches = rank(query, commands).length;

  return (
    <div className="palette-backdrop" onMouseDown={close}>
      <div
        className="palette"
        role="dialog"
        aria-label="命令面板"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="palette-input-row">
          <span className="palette-prompt">›</span>
          <input
            ref={inputRef}
            className="palette-input"
            value={query}
            placeholder="输入命令或章节名，空格分隔多个词"
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
          />
          <kbd className="palette-kbd">Esc</kbd>
        </div>

        <div className="palette-results">
          {results.length === 0 ? (
            <div className="palette-empty">没有匹配的命令或章节</div>
          ) : (
            results.map((item, index) => (
              <button
                key={item.id}
                type="button"
                className={`palette-item${index === selected ? " is-active" : ""}`}
                onMouseEnter={() => setSelected(index)}
                onClick={() => execute(item)}
              >
                <span className="palette-item-title">{item.title}</span>
                {item.subtitle && (
                  <span className="palette-item-subtitle">{item.subtitle}</span>
                )}
                <span className={`palette-tag palette-tag-${item.category}`}>
                  {item.category}
                </span>
              </button>
            ))
          )}
        </div>

        <div className="palette-foot">
          <span>↑↓ 选择 · Enter 执行 · Esc 关闭</span>
          <span>{query.trim() ? `${totalMatches} 条匹配` : `${commands.length} 项可用`}</span>
        </div>
      </div>
    </div>
  );
}
