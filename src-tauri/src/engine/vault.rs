//! 稿库扫描与路径安全。
//!
//! 稿库根目录由用户在运行时选定，所有来自前端的相对路径必须经 `safe_join`
//! 校验后才允许触碰文件系统，防止路径穿越读到库外文件。

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::md::{self, DocStatus};

/// 稿库级配置，来自 `<vault>/_vault.yaml`。缺失时全部使用默认值。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VaultConfig {
    #[serde(default)]
    pub title: Option<String>,
    /// 字数统计是否计入标点。默认不计，与 Word「中文字数」口径一致。
    #[serde(default)]
    pub count_punctuation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterEntry {
    /// 相对稿库根的路径，统一用 `/` 分隔
    pub path: String,
    /// 所在目录（相对路径），根目录为空串
    pub dir: String,
    pub title: String,
    pub order: i64,
    pub status: Option<DocStatus>,
    pub words: u64,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSnapshot {
    pub root: String,
    pub title: String,
    pub count_punctuation: bool,
    pub chapters: Vec<ChapterEntry>,
    pub total_words: u64,
    pub scanned_ms: u128,
}

/// 把前端传来的相对路径安全地拼到根目录下。
///
/// 拒绝绝对路径、`..`、根前缀与当前目录段。返回 `None` 表示路径非法。
pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    if rel.trim().is_empty() {
        return None;
    }
    let candidate = Path::new(rel);
    if candidate.is_absolute() {
        return None;
    }

    let mut out = root.to_path_buf();
    let mut pushed = false;
    for comp in candidate.components() {
        match comp {
            Component::Normal(seg) => {
                out.push(seg);
                pushed = true;
            }
            // ParentDir / RootDir / Prefix / CurDir 一律拒绝
            _ => return None,
        }
    }

    if !pushed {
        return None;
    }
    Some(out)
}

/// 把绝对路径转成相对稿库根、以 `/` 分隔的字符串。
pub fn to_rel(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// 读取稿库配置。文件不存在或解析失败时退化为默认配置，绝不阻断扫描。
pub fn load_config(root: &Path) -> VaultConfig {
    let path = root.join("_vault.yaml");
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_yaml::from_str(&text).ok())
        .unwrap_or_default()
}

/// 扫描整个稿库，返回按 `order` 排序的章节清单。
pub fn scan(root: &Path) -> Result<VaultSnapshot, String> {
    let started = std::time::Instant::now();
    if !root.is_dir() {
        return Err(format!("不是有效目录: {}", root.display()));
    }

    let config = load_config(root);
    let mut chapters = Vec::new();

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            // 根目录本身永不跳过：稿库目录名以 `.` 开头（如 .tmpXXXX）也必须能扫
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            // 跳过隐藏目录与依赖目录，避免误扫 .git / node_modules
            !(entry.file_type().is_dir()
                && (name.starts_with('.') || name == "node_modules" || name == "target"))
        });

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let is_markdown = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("md"))
            .unwrap_or(false);
        if !is_markdown {
            continue;
        }

        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            // 非 UTF-8 或权限不足的文件直接跳过，不让单个坏文件拖垮整个扫描
            Err(_) => continue,
        };

        let split = md::split(&text);
        let front = split.yaml.and_then(md::parse_front_matter).unwrap_or_default();
        let words = md::count_words(split.body, config.count_punctuation);

        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let title = front.title.clone().unwrap_or_else(|| strip_order_prefix(&stem));

        let dir = path
            .parent()
            .map(|p| to_rel(root, p))
            .unwrap_or_default();

        chapters.push(ChapterEntry {
            path: to_rel(root, path),
            dir,
            title,
            order: front.order.unwrap_or(i64::MAX),
            status: front.status,
            words: words.total,
            id: front.id,
        });
    }

    chapters.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.path.cmp(&b.path)));
    let total_words = chapters.iter().map(|c| c.words).sum();

    let title = config.title.clone().unwrap_or_else(|| {
        root.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "清籁斋".to_string())
    });

    Ok(VaultSnapshot {
        root: root.to_string_lossy().to_string(),
        title,
        count_punctuation: config.count_punctuation,
        chapters,
        total_words,
        scanned_ms: started.elapsed().as_millis(),
    })
}

/// 从文件名去掉排序前缀：`001-楔子` → `楔子`。
fn strip_order_prefix(stem: &str) -> String {
    let without_digits = stem.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = without_digits.trim_start_matches(['-', '_', '.', ' ']);
    if rest.is_empty() || rest.len() == stem.len() {
        stem.to_string()
    } else {
        rest.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn 拒绝路径穿越() {
        let root = Path::new("/vault");
        assert!(safe_join(root, "../etc/passwd").is_none());
        assert!(safe_join(root, "卷一/../../x.md").is_none());
        assert!(safe_join(root, "/abs/path.md").is_none());
        assert!(safe_join(root, "").is_none());
        assert!(safe_join(root, ".").is_none());
    }

    #[test]
    fn 接受正常相对路径() {
        let root = Path::new("/vault");
        let joined = safe_join(root, "卷一·听风/001-楔子.md").expect("应接受");
        assert!(joined.ends_with("001-楔子.md"));
        assert!(joined.starts_with(root));
    }

    #[test]
    fn 相对路径统一用正斜杠() {
        let root = Path::new("/vault");
        let p = Path::new("/vault").join("卷一").join("a.md");
        assert_eq!(to_rel(root, &p), "卷一/a.md");
    }

    #[test]
    fn 扫描统计章数与总字数() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "小说/卷一/001-楔子.md", "---\ntitle: 楔子\norder: 1\n---\n清籁斋里写小说。\n");
        write(root, "小说/卷一/002-开端.md", "---\ntitle: 开端\norder: 2\n---\n他说走。\n");
        write(root, "笔记/灵感.md", "三个字\n");

        let snap = scan(root).unwrap();
        assert_eq!(snap.chapters.len(), 3);
        // 7 + 3 + 3
        assert_eq!(snap.total_words, 13);
        // 有 order 的排前面，按 order 升序
        assert_eq!(snap.chapters[0].title, "楔子");
        assert_eq!(snap.chapters[1].title, "开端");
        assert_eq!(snap.chapters[2].title, "灵感");
    }

    #[test]
    fn 扫描跳过非_markdown_与隐藏目录() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "a.md", "正文");
        write(root, "b.txt", "不该被扫到");
        write(root, "_vault.yaml", "title: 测试库");
        write(root, ".git/config", "不该被扫到");
        write(root, "node_modules/x/readme.md", "不该被扫到");

        let snap = scan(root).unwrap();
        assert_eq!(snap.chapters.len(), 1);
        assert_eq!(snap.chapters[0].path, "a.md");
    }

    #[test]
    fn 读取_vault_配置生效() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "_vault.yaml", "title: 清籁斋\ncount_punctuation: true\n");
        write(root, "a.md", "他说：「走。」\n");

        let snap = scan(root).unwrap();
        assert_eq!(snap.title, "清籁斋");
        assert!(snap.count_punctuation);
        assert_eq!(snap.total_words, 7);
    }

    #[test]
    fn 配置缺失时退化不报错() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.md", "正文\n");
        let snap = scan(dir.path()).unwrap();
        assert!(!snap.count_punctuation);
        assert_eq!(snap.chapters.len(), 1);
    }

    #[test]
    fn 坏_front_matter_不影响扫描() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "a.md", "---\n: : 这不是合法 YAML\n---\n正文\n");
        let snap = scan(root).unwrap();
        assert_eq!(snap.chapters.len(), 1);
        // 退化用文件名当标题
        assert_eq!(snap.chapters[0].title, "a");
    }

    #[test]
    fn 文件名排序前缀被去掉() {
        assert_eq!(strip_order_prefix("001-楔子"), "楔子");
        assert_eq!(strip_order_prefix("012_开端"), "开端");
        assert_eq!(strip_order_prefix("楔子"), "楔子");
        assert_eq!(strip_order_prefix("2026"), "2026");
    }

    #[test]
    fn 端到端保存只改正文不碰元数据() {
        use crate::engine::atomic::write_atomic;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = "小说/卷一/001-楔子.md";
        let original = "---\ntitle: 楔子\norder: 1\ncustom: 保留我\n---\n旧正文。\n";
        write(root, rel, original);

        let path = safe_join(root, rel).expect("路径合法");
        let text = fs::read_to_string(&path).unwrap();

        // 打开后不做任何编辑就写回：文件必须字节不变（对应验收标准「git diff 为空」）
        let untouched = md::replace_body(&text, md::split(&text).body);
        write_atomic(&path, untouched.as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        // 改了正文再写回：front matter 一个字节都不能动
        let next = md::replace_body(&text, "新正文。\n");
        write_atomic(&path, next.as_bytes()).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert!(
            after.starts_with("---\ntitle: 楔子\norder: 1\ncustom: 保留我\n---\n"),
            "front matter 被改动了: {after:?}"
        );
        assert!(after.ends_with("新正文。\n"));

        // 重新扫描应反映新正文的字数（新 正 文 = 3）
        let snap = scan(root).unwrap();
        assert_eq!(snap.chapters[0].words, 3);
        assert_eq!(snap.chapters[0].title, "楔子");
    }
}
