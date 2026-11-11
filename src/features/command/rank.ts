/**
 * 命令面板的检索排序，纯函数。
 *
 * 刻意不依赖 store 或 DOM：渲染层需要真实 DOM 才能测，而排序逻辑不需要，
 * 抽出来就能用单元测试锁住匹配行为。
 */

/** 可被检索的最小接口 */
export interface Rankable {
  title: string;
  /** 额外的检索词，空格分隔（如拼音首字母、分类名、英文别名） */
  keywords?: string;
}

interface Scored<T> {
  item: T;
  score: number;
  index: number;
}

/**
 * 按查询词过滤并排序。
 *
 * - 多个词按「且」组合，每个词都要在标题或关键词里命中
 * - 标题前缀命中得分最高，其次是标题中间，最后才是关键词命中
 * - 空查询原样返回，保持调用方给的顺序（章节按 order、命令按固定顺序）
 * - 同分保持原始顺序，避免每次敲键结果乱跳
 */
export function rank<T extends Rankable>(query: string, items: readonly T[]): T[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...items];

  const terms = q.split(/\s+/).filter(Boolean);
  const scored: Scored<T>[] = [];

  items.forEach((item, index) => {
    const title = item.title.toLowerCase();
    const haystack = `${title} ${(item.keywords ?? "").toLowerCase()}`;

    let total = 0;
    let matchedAll = true;

    for (const term of terms) {
      if (!haystack.includes(term)) {
        matchedAll = false;
        break;
      }

      const titlePos = title.indexOf(term);
      if (titlePos === 0) {
        total += 100;
      } else if (titlePos > 0) {
        // 命中位置越靠前越相关，最多扣 40 分
        total += 60 - Math.min(titlePos * 2, 40);
      } else {
        // 只在 keywords 里命中，权重最低
        total += 20;
      }
    }

    if (!matchedAll) return;

    // 短标题更可能是精确目标：搜「楔子」时「楔子」应排在「楔子·修订版」前面
    total += Math.max(0, 20 - title.length / 5);

    scored.push({ item, score: total, index });
  });

  scored.sort((a, b) => b.score - a.score || a.index - b.index);
  return scored.map((s) => s.item);
}
