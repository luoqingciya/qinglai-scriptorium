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

/** 一条反链：某章引用了当前章 */
export interface BacklinkHit {
  path: string;
  title: string;
  dir: string;
  /** 该章引用当前章的次数 */
  count: number;
  /** 每处引用的上下文片段 */
  contexts: string[];
}

/** 悬空链接：指向了稿库中不存在的标题 */
export interface DanglingLink {
  target: string;
  sourcePath: string;
  sourceTitle: string;
}

/** docx 导出参数。缺省即国内小说通行格式（宋体小四、1.5 倍行距、首行缩进 2 字符）。 */
export interface ExportOptions {
  /** 正文字体（同时写入 ascii / hAnsi / eastAsia 槽位） */
  font: string;
  /** 字号，单位半磅（12pt 小四 = 24） */
  fontSize: number;
  /** 行距，240 = 单倍，360 = 1.5 倍 */
  lineSpacing: number;
  /** 首行缩进，单位 1/100 字符（200 = 2 字符） */
  firstLineChars: number;
  /** 章与章之间分页 */
  pageBreak: boolean;
}

export interface ExportStats {
  chapters: number;
  paragraphs: number;
  bytes: number;
}
