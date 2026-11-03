import type { DocStatus } from "./types";

/** 章节状态的中文标签。目录树与大纲视图共用。 */
export const STATUS_LABEL: Record<DocStatus, string> = {
  draft: "草稿",
  revising: "修订",
  done: "完稿",
};
