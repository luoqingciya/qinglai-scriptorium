/**
 * command 的类型化封装。业务逻辑一律不写在这里。
 *
 * 前端不持有任何文件系统权限，所有读写都经这些 command，
 * 路径在 Rust 侧经 `safe_join` 校验必须落在稿库根内。
 */
import { invoke } from "@tauri-apps/api/core";
import type { DocPayload, FileStamp, SaveOutcome, VaultSnapshot, WordCount } from "./types";

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
