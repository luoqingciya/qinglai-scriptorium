/**
 * 与 Rust 侧 serde 输出一一对应的类型。
 *
 * Rust 结构体统一加了 `#[serde(rename_all = "camelCase")]`，
 * 所以这里的字段名与 JSON 完全一致。改动任一侧都要同步改另一侧。
 */

export type DocStatus = "draft" | "revising" | "done";

/** 文件指纹：修改时间 + 字节长度，用于冲突检测 */
export interface FileStamp {
  mtimeMs: number;
  len: number;
}

export interface WordCount {
  /** 中日韩字符数 */
  cjk: number;
  /** 非 CJK 词组数 */
  latin: number;
  /** 标点符号数 */
  punctuation: number;
  /** 按统计口径计入的总字数 */
  total: number;
}

export interface FrontMatter {
  id?: string;
  title?: string;
  order?: number;
  status?: DocStatus;
  characters?: string[];
  pov?: string;
  words?: number;
}

export interface ChapterEntry {
  /** 相对稿库根，正斜杠分隔 */
  path: string;
  dir: string;
  title: string;
  order: number;
  status: DocStatus | null;
  words: number;
  id: string | null;
}

export interface VaultSnapshot {
  root: string;
  title: string;
  countPunctuation: boolean;
  chapters: ChapterEntry[];
  totalWords: number;
  scannedMs: number;
}

export interface DocPayload {
  path: string;
  body: string;
  front: FrontMatter | null;
  hasFrontMatter: boolean;
  stamp: FileStamp;
  words: WordCount;
}

export type SaveOutcome =
  | { kind: "saved"; stamp: FileStamp; words: WordCount }
  | { kind: "conflict"; stamp: FileStamp }
  | { kind: "unchanged"; stamp: FileStamp };

/** 章节重排方向 */
export type Direction = "up" | "down";

/** 一次重排中发生的路径变化，用于修正当前打开的文件 */
export interface PathRename {
  from: string;
  to: string;
}

/** 全文检索命中 */
export interface SearchHit {
  path: string;
  title: string;
  dir: string;
  /** 所有关键词出现次数之和 */
  count: number;
  /** 首个命中位置附近的片段，两端截断处带省略号 */
  snippet: string;
}
