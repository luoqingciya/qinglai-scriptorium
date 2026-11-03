//! 稿库全文检索。
//!
//! 刻意不引入 tantivy / SQLite FTS5：FTS5 的 `unicode61` 分词器不切分中文，
//! 要可用就得挂 jieba 做自定义 tokenizer；tantivy 同理。个人稿库上限通常
//! 千万字以内，**字符二元组倒排**足够快，且完全不需要分词词典。
//!
//! 查询流程：二元组交集缩小候选 → 子串校验消除假阳性 → 按命中次数排序。
//! 第二步不可省略：二元组只保证「这两个字都出现过」，不保证相邻，
//! 「风在」会把「风过了，字还在」也捞进来。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::md;
use super::vault::strip_order_prefix;

/// 摘要片段前后保留的字符数
const SNIPPET_BEFORE: usize = 24;
const SNIPPET_AFTER: usize = 48;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    pub dir: String,
    /// 所有关键词出现次数之和，用于排序
    pub count: u32,
    /// 首个命中位置附近的片段，两端截断处带省略号
    pub snippet: String,
}

/// 建索引用的一篇文档。
///
/// 同时被检索索引、双链索引、人物索引消费，所以除正文外还带着 front matter 里
/// 与索引相关的字段。一次扫描喂三个索引，避免各扫一遍盘。
#[derive(Debug, Clone)]
pub struct DocInput {
    pub path: String,
    pub title: String,
    pub dir: String,
    pub body: String,
    /// 出场人物（front matter `characters`）
    pub characters: Vec<String>,
    /// 视角人物（front matter `pov`）
    pub pov: Option<String>,
}

impl DocInput {
    /// 供测试构造，省略索引无关的元数据。
    #[cfg(test)]
    pub fn new(path: &str, title: &str, dir: &str, body: &str) -> Self {
        Self {
            path: path.to_string(),
            title: title.to_string(),
            dir: dir.to_string(),
            body: body.to_string(),
            characters: Vec::new(),
            pov: None,
        }
    }

    /// 从磁盘原文构造，供保存后增量更新索引使用。
    pub fn from_text(path: String, text: &str) -> Self {
        let split = md::split(text);
        let front = split
            .yaml
            .and_then(md::parse_front_matter)
            .unwrap_or_default();

        let stem = Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let title = front.title.unwrap_or_else(|| strip_order_prefix(&stem));

        let dir = path
            .rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default();

        Self {
            path,
            title,
            dir,
            body: split.body.to_string(),
            characters: front.characters,
            pov: front.pov,
        }
    }
}

struct Doc {
    path: String,
    title: String,
    dir: String,
    /// 小写正文，用于匹配
    lower: String,
    /// 原样正文，用于摘要（保留大小写）
    body: String,
}

#[derive(Default)]
pub struct SearchIndex {
    docs: Vec<Doc>,
    /// 二元组 → 文档编号
    postings: HashMap<u64, Vec<u32>>,
    by_path: HashMap<String, u32>,
}

/// 把两个字符压进一个 u64 作为键。字符值域 21 位，不会撞。
fn pack(a: char, b: char) -> u64 {
    ((a as u64) << 32) | (b as u64)
}

/// 相邻字符二元组。纯空白的二元组没有检索价值，跳过。
fn bigrams(text: &str) -> Vec<u64> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    for pair in chars.windows(2) {
        if pair[0].is_whitespace() && pair[1].is_whitespace() {
            continue;
        }
        out.push(pack(pair[0], pair[1]));
    }
    out
}

impl SearchIndex {
    pub fn from_docs(docs: Vec<DocInput>) -> Self {
        let mut index = Self::default();
        for doc in docs {
            index.insert(doc);
        }
        index
    }

    /// 索引中的文档数。供测试与诊断使用。
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    fn insert(&mut self, input: DocInput) {
        let id = self.docs.len() as u32;
        let lower = input.body.to_lowercase();

        // 同一文档内重复的二元组只记一次，倒排表才不会膨胀
        let mut seen = HashSet::new();
        for key in bigrams(&lower) {
            if seen.insert(key) {
                self.postings.entry(key).or_default().push(id);
            }
        }

        self.by_path.insert(input.path.clone(), id);
        self.docs.push(Doc {
            path: input.path,
            title: input.title,
            dir: input.dir,
            lower,
            body: input.body,
        });
    }

    /// 替换一篇文档（保存后用）。旧文档的倒排项要按旧正文重算才能摘除。
    pub fn upsert(&mut self, input: DocInput) {
        let Some(&id) = self.by_path.get(&input.path) else {
            self.insert(input);
            return;
        };

        let old_lower = self.docs[id as usize].lower.clone();
        let mut seen = HashSet::new();
        for key in bigrams(&old_lower) {
            if !seen.insert(key) {
                continue;
            }
            if let Some(list) = self.postings.get_mut(&key) {
                list.retain(|&x| x != id);
            }
        }

        let lower = input.body.to_lowercase();
        let mut seen = HashSet::new();
        for key in bigrams(&lower) {
            if seen.insert(key) {
                self.postings.entry(key).or_default().push(id);
            }
        }

        self.docs[id as usize] = Doc {
            path: input.path,
            title: input.title,
            dir: input.dir,
            lower,
            body: input.body,
        };
    }

    /// 全文检索。多个关键词按 AND 处理。
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let terms: Vec<String> = query
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if terms.is_empty() {
            return Vec::new();
        }

        // 单字关键词无法用二元组索引，退化为全量校验
        let mut candidates: Option<HashSet<u32>> = None;
        for term in &terms {
            if term.chars().count() < 2 {
                continue;
            }
            for key in bigrams(term) {
                let Some(list) = self.postings.get(&key) else {
                    return Vec::new();
                };
                let set: HashSet<u32> = list.iter().copied().collect();
                candidates = Some(match candidates {
                    None => set,
                    Some(acc) => acc.intersection(&set).copied().collect(),
                });
                if candidates.as_ref().is_some_and(|c| c.is_empty()) {
                    return Vec::new();
                }
            }
        }

        let ids: Vec<u32> = match candidates {
            Some(set) => set.into_iter().collect(),
            None => (0..self.docs.len() as u32).collect(),
        };

        let mut hits = Vec::new();
        for id in ids {
            let doc = &self.docs[id as usize];

            let mut count = 0u32;
            let mut first: Option<usize> = None;
            let mut all_terms_found = true;

            for term in &terms {
                let mut found = 0u32;
                for (offset, _) in doc.lower.match_indices(term.as_str()) {
                    found += 1;
                    if first.is_none_or(|f| offset < f) {
                        first = Some(offset);
                    }
                }
                if found == 0 {
                    all_terms_found = false;
                    break;
                }
                count += found;
            }

            if !all_terms_found {
                continue;
            }

            hits.push(SearchHit {
                path: doc.path.clone(),
                title: doc.title.clone(),
                dir: doc.dir.clone(),
                count,
                snippet: make_snippet(doc, first.unwrap_or(0)),
            });
        }

        hits.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.path.cmp(&b.path)));
        hits.truncate(limit);
        hits
    }
}

/// 截取命中位置附近的片段。
///
/// `byte_pos` 是小写正文里的字节偏移。小写转换对中文与 ASCII 不改变字符数，
/// 因此可以按字符数平移到原样正文上取窗口；极端脚本（如土耳其语 İ）会略有偏移，
/// 只影响摘要展示，不影响匹配结果。
fn make_snippet(doc: &Doc, byte_pos: usize) -> String {
    let start_char = doc.lower[..byte_pos].chars().count();
    let chars: Vec<char> = doc.body.chars().collect();
    let total = chars.len();

    let from = start_char.saturating_sub(SNIPPET_BEFORE);
    let to = (start_char + SNIPPET_AFTER).min(total);

    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    out.extend(&chars[from..to]);
    if to < total {
        out.push('…');
    }

    // 片段要显示在单行里，换行压成空格
    out.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, body: &str) -> DocInput {
        DocInput::new(path, path, "", body)
    }

    fn index_of(docs: Vec<(&str, &str)>) -> SearchIndex {
        SearchIndex::from_docs(docs.into_iter().map(|(p, b)| doc(p, b)).collect())
    }

    #[test]
    fn 能搜到中文短语() {
        let idx = index_of(vec![
            ("a.md", "檐角的铁马响了一声。"),
            ("b.md", "院子里的老槐树已经空了。"),
        ]);
        let hits = idx.search("铁马", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "a.md");
        assert_eq!(hits[0].count, 1);
    }

    #[test]
    fn 二元组不相邻时不误报() {
        // 「风」「在」都出现过但不相邻，不能被「风在」命中
        let idx = index_of(vec![("a.md", "风过了，字还在。")]);
        assert!(idx.search("风在", 10).is_empty());
        assert_eq!(idx.search("风过", 10).len(), 1);
    }

    #[test]
    fn 多关键词按_and_处理() {
        let idx = index_of(vec![
            ("a.md", "沈砚把笔搁在砚台上。"),
            ("b.md", "阿箬醒了。"),
        ]);
        assert_eq!(idx.search("沈砚 笔", 10).len(), 1);
        assert!(idx.search("沈砚 阿箬", 10).is_empty());
    }

    #[test]
    fn 单字也能搜() {
        let idx = index_of(vec![("a.md", "檐角的铁马响了一声。"), ("b.md", "老槐树。")]);
        let hits = idx.search("檐", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "a.md");
    }

    #[test]
    fn 按命中次数排序() {
        let idx = index_of(vec![
            ("a.md", "铁马。铁马。铁马。"),
            ("b.md", "铁马响了一声。"),
        ]);
        let hits = idx.search("铁马", 10);
        assert_eq!(hits[0].path, "a.md");
        assert_eq!(hits[0].count, 3);
        assert_eq!(hits[1].count, 1);
    }

    #[test]
    fn 英文大小写不敏感() {
        let idx = index_of(vec![("a.md", "Scriptorium 是缮写室的意思。")]);
        assert_eq!(idx.search("scriptorium", 10).len(), 1);
        assert_eq!(idx.search("SCRIPTORIUM", 10).len(), 1);
    }

    #[test]
    fn 摘要保留原样大小写并带省略号() {
        let body = format!("{}Scriptorium 在这里{}", "前".repeat(60), "后".repeat(80));
        let idx = index_of(vec![("a.md", &body)]);
        let hits = idx.search("scriptorium", 10);
        let snippet = &hits[0].snippet;
        assert!(snippet.contains("Scriptorium"), "实际: {snippet}");
        assert!(snippet.starts_with('…'));
        assert!(snippet.ends_with('…'));
    }

    #[test]
    fn 短正文的摘要不加省略号() {
        let idx = index_of(vec![("a.md", "铁马响了一声。")]);
        let snippet = &idx.search("铁马", 10)[0].snippet;
        assert_eq!(snippet, "铁马响了一声。");
    }

    #[test]
    fn 摘要里的换行被压成空格() {
        let idx = index_of(vec![("a.md", "第一行\n第二行 铁马\n第三行")]);
        let snippet = &idx.search("铁马", 10)[0].snippet;
        assert!(!snippet.contains('\n'), "实际: {snippet:?}");
        assert!(snippet.contains("铁马"));
    }

    #[test]
    fn 空查询返回空() {
        let idx = index_of(vec![("a.md", "正文")]);
        assert!(idx.search("", 10).is_empty());
        assert!(idx.search("   ", 10).is_empty());
    }

    #[test]
    fn 无匹配返回空() {
        let idx = index_of(vec![("a.md", "正文")]);
        assert!(idx.search("不存在", 10).is_empty());
    }

    #[test]
    fn 遵守结果数量上限() {
        let docs: Vec<(&str, &str)> = (0..10).map(|_| ("x.md", "铁马")).collect();
        let mut idx = SearchIndex::default();
        for (i, (_, body)) in docs.into_iter().enumerate() {
            idx.insert(doc(&format!("x{i}.md"), body));
        }
        assert_eq!(idx.search("铁马", 3).len(), 3);
    }

    #[test]
    fn upsert_替换旧内容后旧词搜不到() {
        let mut idx = index_of(vec![("a.md", "旧内容铁马。")]);
        assert_eq!(idx.search("铁马", 10).len(), 1);

        idx.upsert(doc("a.md", "新内容铜铃。"));
        assert!(idx.search("铁马", 10).is_empty(), "旧词应已摘除");
        assert_eq!(idx.search("铜铃", 10).len(), 1);
        assert_eq!(idx.len(), 1, "不应新增文档");
    }

    #[test]
    fn upsert_新路径走插入() {
        let mut idx = index_of(vec![("a.md", "铁马")]);
        idx.upsert(doc("b.md", "铜铃"));
        assert_eq!(idx.len(), 2);
        assert_eq!(idx.search("铜铃", 10).len(), 1);
    }

    #[test]
    fn 从磁盘原文构造时能取到标题与目录() {
        let text = "---\ntitle: 楔子\norder: 1\n---\n檐角的铁马。\n";
        let input = DocInput::from_text("小说/卷一/001-楔子.md".to_string(), text);
        assert_eq!(input.title, "楔子");
        assert_eq!(input.dir, "小说/卷一");
        assert_eq!(input.body, "檐角的铁马。\n");
    }

    #[test]
    fn 没有元数据时用文件名当标题() {
        let input = DocInput::from_text("卷一/003-夜航.md".to_string(), "正文\n");
        assert_eq!(input.title, "夜航");
        assert_eq!(input.dir, "卷一");
    }

    /// 规模基准，不在 CI 里跑。手动执行：
    /// `cargo test --lib 千章规模 -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn 千章规模的建索引与检索耗时() {
        const CHAPTERS: usize = 1000;
        const PARAGRAPH: &str = "清籁斋的窗纸上落了一层薄雪。沈砚把笔搁在砚台上，\
                                 听见檐角的铁马响了一声。他忽然想起师父说过的话。";

        let docs: Vec<DocInput> = (0..CHAPTERS)
            .map(|i| {
                let body = PARAGRAPH.repeat(10);
                DocInput::new(&format!("卷一/{i:03}-章.md"), &format!("第{i}章"), "卷一", &body)
            })
            .collect();

        let chars: usize = docs.iter().map(|d| d.body.chars().count()).sum();

        let t0 = std::time::Instant::now();
        let index = SearchIndex::from_docs(docs);
        let build = t0.elapsed();

        let t1 = std::time::Instant::now();
        let hits = index.search("铁马", 50);
        let query = t1.elapsed();

        println!(
            "{CHAPTERS} 章 / {chars} 字：建索引 {build:?}，检索 {query:?}，命中 {}",
            hits.len()
        );

        assert_eq!(hits.len(), 50, "应受 limit 限制");
        assert!(build.as_millis() < 2000, "建索引过慢: {build:?}");
        assert!(query.as_millis() < 200, "检索过慢: {query:?}");
    }
}
