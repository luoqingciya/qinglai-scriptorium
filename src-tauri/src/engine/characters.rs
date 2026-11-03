//! 人物索引：谁在哪几章出场。
//!
//! 数据本来就写在每章的 front matter 里（`characters` / `pov`），这里只是把它
//! 倒过来排一遍。与检索、双链一样是**纯内存的派生结构**，随扫描重建、随保存更新。
//!
//! ## 视角人物自动算作出场
//!
//! 作者写 `pov: 阿箬` 却忘了在 `characters` 里列一遍，是很容易发生的事。
//! 视角人物必然出场，所以索引在聚合时会把 `pov` 并入 `characters`，并对该章
//! 标记 `is_pov`。
//!
//! 这不是"猜"，是定义使然：视角人物就是这一章的叙述者，不可能不在场。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::search::DocInput;

/// 某角色在某一章的出场记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    pub path: String,
    pub title: String,
    pub dir: String,
    /// 是否为该章的视角人物
    pub is_pov: bool,
}

/// 一个角色的完整档案。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterCard {
    pub name: String,
    /// 出场章节，按文件路径排序（即目录树顺序）
    pub appearances: Vec<Appearance>,
    /// 其中作为视角人物的章数
    pub pov_count: usize,
}

impl CharacterCard {
    pub fn chapter_count(&self) -> usize {
        self.appearances.len()
    }
}

/// 角色名 → 出场记录。
#[derive(Debug, Default)]
pub struct CharacterIndex {
    by_character: HashMap<String, Vec<Appearance>>,
    /// 路径 → 该章贡献的角色名，用于增量更新时摘旧边
    names_by_path: HashMap<String, Vec<String>>,
}

fn dir_of(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(d, _)| d.to_string())
        .unwrap_or_default()
}

/// 规范化角色名。建表与查表必须走同一函数，否则永远匹配不上。
fn normalize(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

impl CharacterIndex {
    pub fn from_docs(docs: &[DocInput]) -> Self {
        let mut index = Self::default();
        for doc in docs {
            index.insert(doc);
        }
        index
    }

    fn insert(&mut self, doc: &DocInput) {
        let pov = doc.pov.as_deref().and_then(normalize);

        let mut names: Vec<String> = doc
            .characters
            .iter()
            .filter_map(|c| normalize(c))
            .collect();

        // 视角人物必然出场，即使作者忘了列进 characters
        if let Some(p) = &pov {
            if !names.contains(p) {
                names.push(p.clone());
            }
        }

        if names.is_empty() {
            return;
        }

        let dir = dir_of(&doc.path);
        for name in &names {
            self.by_character
                .entry(name.clone())
                .or_default()
                .push(Appearance {
                    path: doc.path.clone(),
                    title: doc.title.clone(),
                    dir: dir.clone(),
                    is_pov: pov.as_deref() == Some(name.as_str()),
                });
        }

        self.names_by_path.insert(doc.path.clone(), names);
    }

    fn remove(&mut self, path: &str) {
        let Some(names) = self.names_by_path.remove(path) else {
            return;
        };

        for name in names {
            if let Some(list) = self.by_character.get_mut(&name) {
                list.retain(|a| a.path != path);
                if list.is_empty() {
                    self.by_character.remove(&name);
                }
            }
        }
    }

    /// 增量更新：先摘旧边，再插新边。
    pub fn upsert(&mut self, doc: &DocInput) {
        self.remove(&doc.path);
        self.insert(doc);
    }

    pub fn forget(&mut self, path: &str) {
        self.remove(path);
    }

    /// 全部角色，按出场章数降序；同数量按名字排序，保证顺序稳定。
    pub fn list(&self) -> Vec<CharacterCard> {
        let mut cards: Vec<CharacterCard> = self
            .by_character
            .iter()
            .map(|(name, appearances)| {
                let mut sorted = appearances.clone();
                sorted.sort_by(|a, b| a.path.cmp(&b.path));
                let pov_count = sorted.iter().filter(|a| a.is_pov).count();
                CharacterCard {
                    name: name.clone(),
                    appearances: sorted,
                    pov_count,
                }
            })
            .collect();

        cards.sort_by(|a, b| {
            b.chapter_count()
                .cmp(&a.chapter_count())
                .then_with(|| a.name.cmp(&b.name))
        });
        cards
    }

    /// 只出现在 front matter 里、从未在任何章节正文中出现的角色名，
    /// 用于「设定里写了但还没排上场」的提示。当前未在 UI 使用。
    #[allow(dead_code)]
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.by_character.keys().cloned().collect();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, title: &str, characters: &[&str], pov: Option<&str>) -> DocInput {
        DocInput {
            path: path.to_string(),
            title: title.to_string(),
            dir: dir_of(path),
            body: String::new(),
            characters: characters.iter().map(|s| s.to_string()).collect(),
            pov: pov.map(|s| s.to_string()),
        }
    }

    #[test]
    fn 提取基本角色() {
        let index = CharacterIndex::from_docs(&[doc("a.md", "甲", &["沈砚", "阿箬"], None)]);
        let cards = index.list();
        assert_eq!(cards.len(), 2);
    }

    #[test]
    fn 视角人物自动算作出场() {
        // 作者只写了 pov，忘了列进 characters
        let index = CharacterIndex::from_docs(&[doc("a.md", "甲", &[], Some("阿箬"))]);
        let cards = index.list();

        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].name, "阿箬");
        assert!(cards[0].appearances[0].is_pov);
    }

    #[test]
    fn 视角人物已在出场列表时不重复计数() {
        let index = CharacterIndex::from_docs(&[doc("a.md", "甲", &["阿箬"], Some("阿箬"))]);
        let cards = index.list();

        assert_eq!(cards[0].appearances.len(), 1);
        assert!(cards[0].appearances[0].is_pov);
        assert_eq!(cards[0].pov_count, 1);
    }

    #[test]
    fn 多章聚合且按章数降序() {
        let index = CharacterIndex::from_docs(&[
            doc("001.md", "一", &["沈砚"], None),
            doc("002.md", "二", &["沈砚", "阿箬"], None),
            doc("003.md", "三", &["沈砚"], None),
        ]);

        let cards = index.list();
        assert_eq!(cards[0].name, "沈砚");
        assert_eq!(cards[0].chapter_count(), 3);
        assert_eq!(cards[1].name, "阿箬");
        assert_eq!(cards[1].chapter_count(), 1);
    }

    #[test]
    fn 出场章节按路径排序() {
        let index = CharacterIndex::from_docs(&[
            doc("002.md", "二", &["沈砚"], None),
            doc("001.md", "一", &["沈砚"], None),
        ]);

        let cards = index.list();
        let paths: Vec<_> = cards[0]
            .appearances
            .iter()
            .map(|a| a.path.as_str())
            .collect();
        assert_eq!(paths, vec!["001.md", "002.md"]);
    }

    #[test]
    fn 空白角色名被忽略() {
        let index = CharacterIndex::from_docs(&[doc("a.md", "甲", &["", "  ", "沈砚"], None)]);
        assert_eq!(index.list().len(), 1);
    }

    #[test]
    fn 角色名首尾空白被去掉() {
        let index = CharacterIndex::from_docs(&[
            doc("a.md", "甲", &[" 沈砚 "], None),
            doc("b.md", "乙", &["沈砚"], None),
        ]);

        let cards = index.list();
        assert_eq!(cards.len(), 1, "带空格与不带空格应合并成同一角色");
        assert_eq!(cards[0].chapter_count(), 2);
    }

    #[test]
    fn 增量更新会摘除旧出场() {
        let mut index = CharacterIndex::from_docs(&[doc("a.md", "甲", &["沈砚"], None)]);
        assert_eq!(index.list().len(), 1);

        // 改成只有阿箬
        index.upsert(&doc("a.md", "甲", &["阿箬"], None));
        let cards = index.list();

        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].name, "阿箬");
    }

    #[test]
    fn forget_摘除整章贡献() {
        let mut index = CharacterIndex::from_docs(&[
            doc("a.md", "甲", &["沈砚"], None),
            doc("b.md", "乙", &["沈砚"], None),
        ]);
        assert_eq!(index.list()[0].chapter_count(), 2);

        index.forget("a.md");
        assert_eq!(index.list()[0].chapter_count(), 1);
        assert_eq!(index.list()[0].appearances[0].path, "b.md");
    }

    #[test]
    fn 没有角色的章节不产生条目() {
        let index = CharacterIndex::from_docs(&[doc("a.md", "甲", &[], None)]);
        assert!(index.list().is_empty());
    }

    #[test]
    fn 视角章数单独统计() {
        let index = CharacterIndex::from_docs(&[
            doc("a.md", "甲", &["沈砚"], Some("沈砚")),
            doc("b.md", "乙", &["沈砚"], Some("阿箬")),
            doc("c.md", "丙", &["沈砚"], None),
        ]);

        let card = &index.list()[0];
        assert_eq!(card.chapter_count(), 3);
        assert_eq!(card.pov_count, 1);
    }

    #[test]
    fn 子目录里的章节带目录信息() {
        let index = CharacterIndex::from_docs(&[doc("卷一/001-甲.md", "甲", &["沈砚"], None)]);
        assert_eq!(index.list()[0].appearances[0].dir, "卷一");
    }

    /// 用仓库示例稿库跑真实链路：扫描 → 读 front matter → 聚合。
    #[test]
    fn 导出仓库示例稿库的人物() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/sample-vault");
        if !root.is_dir() {
            return; // 示例库不在时跳过
        }

        let outcome = crate::engine::vault::scan_with_docs(&root).expect("扫描示例库失败");
        let index = CharacterIndex::from_docs(&outcome.docs);
        let cards = index.list();

        let shen = cards
            .iter()
            .find(|c| c.name == "沈砚")
            .expect("示例库里有沈砚");
        // 楔子（characters+pov）与夜航（characters）
        assert_eq!(shen.chapter_count(), 2);
        assert_eq!(shen.pov_count, 1, "只有楔子以沈砚为视角");

        let aruo = cards
            .iter()
            .find(|c| c.name == "阿箬")
            .expect("示例库里有阿箬");
        // 夜航里 characters 列了阿箬，pov 也是阿箬 —— 不能重复计数
        assert_eq!(aruo.chapter_count(), 1);
        assert_eq!(aruo.pov_count, 1);
    }
}
