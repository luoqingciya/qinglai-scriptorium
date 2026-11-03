//! 章节之间的 `[[双链]]` 与反链。
//!
//! 语法：`[[目标标题]]`，可选别名 `[[目标标题|显示文字]]`。
//!
//! 与检索索引一样，这是一个**纯内存的派生结构**：随扫描重建、随保存增量更新，
//! 不落盘。稿件正文里只有 `[[标题]]` 这几个字符，没有 ID、没有私有格式。
//!
//! ## 为什么按标题而不是 ID 匹配
//!
//! `[[楔子]]` 是作者自然会写的形式，`[[01JCV...]]` 不是。代价是重命名后旧引用会
//! 变成悬空链接 —— 这由 `dangling()` 显式暴露，让用户自己决定改不改，
//! 而不是在重命名时静默批量替换（那样会误伤同名的不同章节）。
//!
//! ## 与编辑器的关系
//!
//! 编辑器里 `[[...]]` 的高亮走 ProseMirror 的 Decoration，**不注册自定义 node/mark**。
//! 因此文档结构完全不变，Markdown 序列化一个字节都不会多出来 —— 往返保真零风险。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::search::DocInput;

/// 上下文片段在目标两侧各保留的字符数
const CONTEXT_PAD: usize = 20;

/// 正文中的一处引用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkRef {
    /// 目标标题（已规范化：去首尾空白、去 `#锚点`）
    pub target: String,
    /// `[[目标|别名]]` 里的别名
    pub alias: Option<String>,
    /// 上下文片段，用于反链面板展示
    pub context: String,
}

/// 一条反链：某章引用了当前章。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacklinkHit {
    pub path: String,
    pub title: String,
    pub dir: String,
    /// 该章引用当前章的次数
    pub count: u32,
    /// 每处引用的上下文片段
    pub contexts: Vec<String>,
}

/// 悬空链接：指向了稿库中不存在的标题。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DanglingLink {
    pub target: String,
    pub source_path: String,
    pub source_title: String,
}

/// 标题规范化。查表与建表必须走同一个函数，否则永远匹配不上。
fn normalize(raw: &str) -> String {
    // `[[楔子#开头]]` 这种带锚点的写法只取标题部分
    let head = raw.split('#').next().unwrap_or(raw);
    head.trim().to_lowercase()
}

/// 从正文中提取所有 `[[...]]`。
///
/// 手写扫描而不引 regex：规则足够简单，且能精确控制「不跨行」这条约束 ——
/// 正则的 `.` 默认不匹配换行，但 `[^\]]` 会跨行，写起来反而更绕。
///
/// 不跨行是刻意的：中文正文里 `[[` 与 `]]` 若相隔一整段，几乎一定是误输入
/// （比如两个独立的方括号），不该被当成链接。
pub fn extract(body: &str) -> Vec<LinkRef> {
    let chars: Vec<char> = body.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i + 1 < chars.len() {
        if chars[i] != '[' || chars[i + 1] != '[' {
            i += 1;
            continue;
        }

        let start = i + 2;
        // 找闭合的 `]]`，遇到换行就放弃这次匹配
        let mut j = start;
        let mut closed = false;
        while j + 1 < chars.len() {
            if chars[j] == '\n' {
                break;
            }
            if chars[j] == ']' && chars[j + 1] == ']' {
                closed = true;
                break;
            }
            j += 1;
        }

        if !closed {
            // 从 `[[` 之后继续找，避免 `[[[a]]` 这类写法漏掉内层
            i = start;
            continue;
        }

        let inner: String = chars[start..j].iter().collect();
        let (target_raw, alias) = match inner.split_once('|') {
            Some((t, a)) => (t, Some(a.trim().to_string())),
            None => (inner.as_str(), None),
        };

        let target = normalize(target_raw);
        if !target.is_empty() {
            out.push(LinkRef {
                context: context_of(&chars, i, j + 2),
                target,
                alias: alias.filter(|a| !a.is_empty()),
            });
        }

        i = j + 2;
    }

    out
}

/// 取 `[[...]]` 两侧的上下文。按字符切片，不会切坏中文。
fn context_of(chars: &[char], open: usize, close: usize) -> String {
    let from = open.saturating_sub(CONTEXT_PAD);
    let to = (close + CONTEXT_PAD).min(chars.len());
    let mut s: String = chars[from..to].iter().collect();
    // 折叠换行，让片段在单行面板里可读
    s = s.replace(['\n', '\r'], " ");
    s.trim().to_string()
}

/// 一个来源章节的引用记录
#[derive(Debug, Clone)]
struct SourceRef {
    path: String,
    title: String,
    dir: String,
    contexts: Vec<String>,
}

/// 引用索引：目标标题 → 引用它的章节。
#[derive(Debug, Default)]
pub struct LinkIndex {
    /// 规范化目标标题 → 来源列表
    by_target: HashMap<String, Vec<SourceRef>>,
    /// 路径 → 该章的标题（规范化），用于「给我这一章的反链」
    title_by_path: HashMap<String, String>,
    /// 路径 → 该章指向的目标（规范化），用于悬空检测与增量更新时摘除旧边
    targets_by_path: HashMap<String, Vec<String>>,
    /// 全库已知标题（规范化），用于悬空检测
    known_titles: HashMap<String, usize>,
}

impl LinkIndex {
    pub fn from_docs(docs: &[DocInput]) -> Self {
        let mut index = Self::default();
        for doc in docs {
            index.insert(doc);
        }
        index
    }

    fn insert(&mut self, doc: &DocInput) {
        let title_key = normalize(&doc.title);
        self.title_by_path.insert(doc.path.clone(), title_key.clone());
        *self.known_titles.entry(title_key).or_insert(0) += 1;

        let dir = doc
            .path
            .rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default();

        let refs = extract(&doc.body);
        let mut targets = Vec::with_capacity(refs.len());

        // 同一章里对同一目标的多次引用合并成一条，避免反链面板里刷屏
        let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
        for r in refs {
            targets.push(r.target.clone());
            grouped.entry(r.target).or_default().push(r.context);
        }

        for (target, contexts) in grouped {
            let entry = SourceRef {
                path: doc.path.clone(),
                title: doc.title.clone(),
                dir: dir.clone(),
                contexts,
            };
            self.by_target.entry(target).or_default().push(entry);
        }

        self.targets_by_path.insert(doc.path.clone(), targets);
    }

    /// 摘除某路径贡献的所有边。增量更新与删除都必须先走这一步，
    /// 否则旧引用会永远留在倒排里（检索索引踩过同样的坑）。
    fn remove(&mut self, path: &str) {
        if let Some(targets) = self.targets_by_path.remove(path) {
            for target in targets {
                if let Some(list) = self.by_target.get_mut(&target) {
                    list.retain(|s| s.path != path);
                    if list.is_empty() {
                        self.by_target.remove(&target);
                    }
                }
            }
        }

        if let Some(title) = self.title_by_path.remove(path) {
            if let Some(count) = self.known_titles.get_mut(&title) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.known_titles.remove(&title);
                }
            }
        }
    }

    /// 增量更新一篇文档：先摘旧边，再插新边。
    pub fn upsert(&mut self, doc: &DocInput) {
        self.remove(&doc.path);
        self.insert(doc);
    }

    /// 删除一篇文档贡献的所有边。
    pub fn forget(&mut self, path: &str) {
        self.remove(path);
    }

    /// 查询某章的反链。`path` 是相对稿库根的路径。
    pub fn backlinks(&self, path: &str) -> Vec<BacklinkHit> {
        let Some(title_key) = self.title_by_path.get(path) else {
            return Vec::new();
        };

        let Some(sources) = self.by_target.get(title_key) else {
            return Vec::new();
        };

        let mut hits: Vec<BacklinkHit> = sources
            .iter()
            // 自己引用自己不算反链
            .filter(|s| s.path != path)
            .map(|s| BacklinkHit {
                path: s.path.clone(),
                title: s.title.clone(),
                dir: s.dir.clone(),
                count: s.contexts.len() as u32,
                contexts: s.contexts.clone(),
            })
            .collect();

        // 引用次数多的排前面，同数量按标题稳定排序
        hits.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.title.cmp(&b.title)));
        hits
    }

    /// 全库悬空链接：目标标题不存在于任何章节。
    pub fn dangling(&self) -> Vec<DanglingLink> {
        let mut out = Vec::new();
        for (target, sources) in &self.by_target {
            if self.known_titles.contains_key(target) {
                continue;
            }
            for source in sources {
                out.push(DanglingLink {
                    target: target.clone(),
                    source_path: source.path.clone(),
                    source_title: source.title.clone(),
                });
            }
        }
        out.sort_by(|a, b| {
            a.target
                .cmp(&b.target)
                .then_with(|| a.source_path.cmp(&b.source_path))
        });
        out
    }

    /// 某章指向的所有目标（去重，保持首次出现顺序）。
    pub fn outlinks(&self, path: &str) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        self.targets_by_path
            .get(path)
            .map(|targets| {
                targets
                    .iter()
                    .filter(|t| seen.insert((*t).clone()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 按标题找章节路径。同名时返回第一条，供编辑器内点击跳转使用。
    pub fn resolve(&self, target: &str) -> Option<String> {
        let key = normalize(target);
        self.title_by_path
            .iter()
            .find(|(_, t)| **t == key)
            .map(|(p, _)| p.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, title: &str, body: &str) -> DocInput {
        DocInput::new(path, title, "", body)
    }

    #[test]
    fn 提取基本双链() {
        let refs = extract("他想起[[楔子]]里那句话。");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target, "楔子");
        assert_eq!(refs[0].alias, None);
    }

    #[test]
    fn 提取带别名的双链() {
        let refs = extract("见[[第一章|开篇]]。");
        assert_eq!(refs[0].target, "第一章");
        assert_eq!(refs[0].alias.as_deref(), Some("开篇"));
    }

    #[test]
    fn 一个段落里多处引用都能提取() {
        let refs = extract("[[甲]]与[[乙]]都提到了[[丙]]。");
        let targets: Vec<_> = refs.iter().map(|r| r.target.as_str()).collect();
        assert_eq!(targets, vec!["甲", "乙", "丙"]);
    }

    #[test]
    fn 未闭合的双链不误报() {
        assert!(extract("这里有个[[没闭合的括号").is_empty());
    }

    #[test]
    fn 跨行的双链不算链接() {
        // `[[` 与 `]]` 相隔一整段，几乎一定是误输入
        assert!(extract("前面[[这里\n换行了]]后面").is_empty());
    }

    #[test]
    fn 空目标被忽略() {
        assert!(extract("[[]] 和 [[  ]] 都不算").is_empty());
    }

    #[test]
    fn 带锚点的目标只取标题部分() {
        let refs = extract("[[楔子#开头]]");
        assert_eq!(refs[0].target, "楔子");
    }

    #[test]
    fn 目标大小写与首尾空白被规范化() {
        let refs = extract("[[  Chapter One  ]]");
        assert_eq!(refs[0].target, "chapter one");
    }

    #[test]
    fn 上下文包含目标两侧文字() {
        let refs = extract("他想起[[楔子]]里那句话。");
        let ctx = &refs[0].context;
        assert!(ctx.contains("楔子"));
        assert!(ctx.contains("他想起"));
    }

    #[test]
    fn 反链按引用次数排序() {
        let index = LinkIndex::from_docs(&[
            doc("a.md", "甲", "[[目标]] 一次"),
            doc("b.md", "乙", "[[目标]] 两次 [[目标]]"),
            doc("c.md", "目标", "正文"),
        ]);

        let hits = index.backlinks("c.md");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, "b.md");
        assert_eq!(hits[0].count, 2);
        assert_eq!(hits[1].path, "a.md");
    }

    #[test]
    fn 自己引用自己不算反链() {
        let index = LinkIndex::from_docs(&[doc("a.md", "甲", "[[甲]] 自引")]);
        assert!(index.backlinks("a.md").is_empty());
    }

    #[test]
    fn 悬空链接被识别() {
        let index = LinkIndex::from_docs(&[doc("a.md", "甲", "[[不存在的章]]")]);
        let dangling = index.dangling();
        assert_eq!(dangling.len(), 1);
        assert_eq!(dangling[0].target, "不存在的章");
        assert_eq!(dangling[0].source_path, "a.md");
    }

    #[test]
    fn 有目标时不算悬空() {
        let index = LinkIndex::from_docs(&[
            doc("a.md", "甲", "[[乙]]"),
            doc("b.md", "乙", "正文"),
        ]);
        assert!(index.dangling().is_empty());
    }

    #[test]
    fn 增量更新会摘除旧引用() {
        let mut index = LinkIndex::from_docs(&[doc("a.md", "甲", "[[乙]]"), doc("b.md", "乙", "")]);
        assert_eq!(index.backlinks("b.md").len(), 1);

        // 甲不再引用乙
        index.upsert(&doc("a.md", "甲", "改成了别的内容"));
        assert!(index.backlinks("b.md").is_empty());
    }

    #[test]
    fn 增量更新会插入新引用() {
        let mut index = LinkIndex::from_docs(&[doc("a.md", "甲", ""), doc("b.md", "乙", "")]);
        assert!(index.backlinks("b.md").is_empty());

        index.upsert(&doc("a.md", "甲", "现在引用[[乙]]了"));
        assert_eq!(index.backlinks("b.md").len(), 1);
    }

    #[test]
    fn 改名后旧引用变成悬空() {
        let mut index = LinkIndex::from_docs(&[
            doc("a.md", "甲", "[[乙]]"),
            doc("b.md", "乙", ""),
        ]);
        assert!(index.dangling().is_empty());

        // 乙改名成「丙」，甲里的 [[乙]] 就悬空了
        index.upsert(&doc("b.md", "丙", ""));
        let dangling = index.dangling();
        assert_eq!(dangling.len(), 1);
        assert_eq!(dangling[0].target, "乙");
    }

    #[test]
    fn forget_摘除整章贡献的边() {
        let mut index = LinkIndex::from_docs(&[doc("a.md", "甲", "[[乙]]"), doc("b.md", "乙", "")]);
        index.forget("a.md");
        assert!(index.backlinks("b.md").is_empty());
        // 乙自己还在，所以不算悬空
        assert!(index.dangling().is_empty());
    }

    #[test]
    fn resolve_按标题找路径() {
        let index = LinkIndex::from_docs(&[
            doc("a.md", "甲", ""),
            doc("b.md", "乙", ""),
        ]);
        assert_eq!(index.resolve("乙").as_deref(), Some("b.md"));
        assert_eq!(index.resolve(" 乙 ").as_deref(), Some("b.md"));
        assert_eq!(index.resolve("不存在"), None);
    }

    #[test]
    fn outlinks_去重且保持顺序() {
        let index = LinkIndex::from_docs(&[doc("a.md", "甲", "[[乙]] [[丙]] [[乙]]")]);
        assert_eq!(index.outlinks("a.md"), vec!["乙", "丙"]);
    }
}
