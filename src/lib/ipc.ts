/**
 * command 的类型化封装。业务逻辑一律不写在这里。
 *
 * 前端不持有任何文件系统权限，所有读写都经这些 command，
 * 路径在 Rust 侧经 `safe_join` 校验必须落在稿库根内。
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  BacklinkHit,
  CharacterCard,
  DanglingLink,
  Direction,
  DocPayload,
  ExportOptions,
  ExportStats,
  FileStamp,
  PathRename,
  SaveOutcome,
  SearchHit,
  Snapshot,
  TodayStat,
  TrendPoint,
  VaultSnapshot,
  WordCount,
} from "./types";

export function openVault(path: string): Promise<VaultSnapshot> {
  return invoke<VaultSnapshot>("open_vault", { path });
}

export function scanVault(): Promise<VaultSnapshot> {
  return invoke<VaultSnapshot>("scan_vault");
}

/** 在指定目录（相对稿库根，空串表示根目录）新建一章，返回新文件相对路径 */
export function createDoc(dir: string, title: string): Promise<string> {
  return invoke<string>("create_doc", { dir, title });
}

/** 重命名一章，返回重命名后的相对路径（文件名可能随之变化） */
export function renameDoc(path: string, title: string): Promise<string> {
  return invoke<string>("rename_doc", { path, title });
}

/** 删除一章（移入稿库内 .trash/），返回回收站中的相对路径 */
export function deleteDoc(path: string): Promise<string> {
  return invoke<string>("delete_doc", { path });
}

/** 同目录内上移或下移一章，返回发生的路径变化 */
export function reorderDoc(path: string, direction: Direction): Promise<PathRename[]> {
  return invoke<PathRename[]>("reorder_doc", { path, direction });
}

/** 检索当前稿库。多个关键词按 AND 处理。 */
export function searchVault(query: string, limit = 50): Promise<SearchHit[]> {
  return invoke<SearchHit[]>("search_vault", { query, limit });
}

export function readDoc(path: string): Promise<DocPayload> {
  return invoke<DocPayload>("read_doc", { path });
}

export function saveDoc(
  path: string,
  body: string,
  expected: FileStamp | null,
): Promise<SaveOutcome> {
  return invoke<SaveOutcome>("save_doc", { path, body, expected });
}

export function countWords(text: string, countPunctuation = false): Promise<WordCount> {
  return invoke<WordCount>("count_words", { text, countPunctuation });
}

/** 查询某章的反链：哪些章节引用了它 */
export function backlinks(path: string): Promise<BacklinkHit[]> {
  return invoke<BacklinkHit[]>("backlinks", { path });
}

/** 查询某章指向的标题（去重，保持首次出现顺序） */
export function outlinks(path: string): Promise<string[]> {
  return invoke<string[]>("outlinks", { path });
}

/** 全库悬空链接：指向了不存在的章节 */
export function danglingLinks(): Promise<DanglingLink[]> {
  return invoke<DanglingLink[]>("dangling_links");
}

/** 把 `[[标题]]` 解析成章节路径，找不到返回 null */
export function resolveLink(target: string): Promise<string | null> {
  return invoke<string | null>("resolve_link", { target });
}

/**
 * 导出 docx。
 *
 * `paths` 为空表示导出全库；`dest` 是绝对路径，由系统保存对话框得到。
 */
export function exportDocx(
  paths: string[],
  dest: string,
  options?: ExportOptions,
): Promise<ExportStats> {
  return invoke<ExportStats>("export_docx", { paths, dest, options });
}

/** 导出全库为 EPUB 3.0 电子书（章节顺序与目录树一致） */
export function exportEpub(dest: string): Promise<ExportStats> {
  return invoke<ExportStats>("export_epub", { dest });
}

/** 记录一次当前全库总字数，返回今日累计 */
export function recordStats(totalWords: number): Promise<TodayStat> {
  return invoke<TodayStat>("record_stats", { totalWords });
}

/** 最近若干天的写作趋势，按日期正序 */
export function statsHistory(limit = 30): Promise<TrendPoint[]> {
  return invoke<TrendPoint[]>("stats_history", { limit });
}

/** 全部角色及其出场章节，按出场章数降序 */
export function listCharacters(): Promise<CharacterCard[]> {
  return invoke<CharacterCard[]>("list_characters");
}

/** 改写一章的情节摘要（只动 front matter 的 summary 行） */
export function updateSummary(path: string, summary: string): Promise<void> {
  return invoke<void>("update_summary", { path, summary });
}

/** 某章的历史版本，最新的在前 */
export function listVersions(path: string): Promise<Snapshot[]> {
  return invoke<Snapshot[]>("list_versions", { path });
}

/** 读取某个版本的内容，供预览 */
export function readVersion(hash: string): Promise<string> {
  return invoke<string>("read_version", { hash });
}

/** 恢复到某个历史版本，返回恢复后的正文 */
export function restoreVersion(path: string, hash: string): Promise<string> {
  return invoke<string>("restore_version", { path, hash });
}
