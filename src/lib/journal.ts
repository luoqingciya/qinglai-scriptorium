/**
 * 崩溃恢复用的本地日志。
 *
 * 自动保存有 800ms 防抖，强杀进程或 WebView 崩溃会丢掉窗口内的输入。
 * 这里以更短的间隔把「当前章节 + 未落盘正文」写进 localStorage，
 * 成功落盘后立刻清掉，下次启动时若发现残留且与磁盘内容不同，就提示恢复。
 *
 * 日志只做短期兜底，不是归档数据 —— 一旦正文落盘，它就没有存在意义。
 */

export interface JournalEntry {
  /** 相对稿库根的路径 */
  path: string;
  body: string;
  /** 写入时刻（epoch 毫秒） */
  at: number;
}

/** 只依赖这三个方法，便于在测试里用内存实现替换 */
export interface KeyValueStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

const JOURNAL_KEY = "qinglai.journal.v1";
const VAULT_KEY = "qinglai.vault.v1";

function defaultStore(): KeyValueStore | null {
  try {
    // 某些沙箱上下文里访问 localStorage 本身就会抛错
    return typeof localStorage !== "undefined" ? localStorage : null;
  } catch {
    return null;
  }
}

function isEntry(value: unknown): value is JournalEntry {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.path === "string" &&
    typeof candidate.body === "string" &&
    typeof candidate.at === "number"
  );
}

export function readJournal(store: KeyValueStore | null = defaultStore()): JournalEntry | null {
  if (!store) return null;

  const raw = store.getItem(JOURNAL_KEY);
  if (!raw) return null;

  try {
    const parsed: unknown = JSON.parse(raw);
    if (!isEntry(parsed)) {
      store.removeItem(JOURNAL_KEY);
      return null;
    }
    return parsed;
  } catch {
    // 损坏的日志直接丢弃，不能让一条坏数据挡住启动
    store.removeItem(JOURNAL_KEY);
    return null;
  }
}

export function writeJournal(
  entry: JournalEntry,
  store: KeyValueStore | null = defaultStore(),
): void {
  if (!store) return;
  try {
    store.setItem(JOURNAL_KEY, JSON.stringify(entry));
  } catch {
    // 配额满或隐私模式：恢复能力降级，但不该影响正常写作
  }
}

export function clearJournal(store: KeyValueStore | null = defaultStore()): void {
  if (!store) return;
  try {
    store.removeItem(JOURNAL_KEY);
  } catch {
    /* 忽略 */
  }
}

/** 记住上次打开的稿库，下次启动直接恢复 */
export function readLastVault(store: KeyValueStore | null = defaultStore()): string | null {
  if (!store) return null;
  return store.getItem(VAULT_KEY);
}

export function writeLastVault(path: string, store: KeyValueStore | null = defaultStore()): void {
  if (!store) return;
  try {
    store.setItem(VAULT_KEY, path);
  } catch {
    /* 忽略 */
  }
}

export function clearLastVault(store: KeyValueStore | null = defaultStore()): void {
  if (!store) return;
  try {
    store.removeItem(VAULT_KEY);
  } catch {
    /* 忽略 */
  }
}
