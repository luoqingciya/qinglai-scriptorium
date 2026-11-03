//! 稿库扫描与路径安全。
//!
//! 稿库根目录由用户在运行时选定，所有来自前端的相对路径必须经 `safe_join`
//! 校验后才允许触碰文件系统，防止路径穿越读到库外文件。

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::atomic;
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

/// 扫描结果：章节清单 + 建索引所需的正文。
pub struct ScanOutcome {
    pub snapshot: VaultSnapshot,
    pub docs: Vec<super::search::DocInput>,
}

/// 扫描整个稿库，返回按 `order` 排序的章节清单。
///
/// 应用启动路径走 `scan_with_docs`（要顺带建索引），这个包装供测试与
/// 只关心章节清单的场景使用。
#[allow(dead_code)]
pub fn scan(root: &Path) -> Result<VaultSnapshot, String> {
    scan_with_docs(root).map(|outcome| outcome.snapshot)
}

/// 扫描稿库，顺带产出建搜索索引用的正文。
///
/// 与 `scan` 共用同一次磁盘遍历，避免为了建索引再读一遍所有文件。
pub fn scan_with_docs(root: &Path) -> Result<ScanOutcome, String> {
    let started = std::time::Instant::now();
    if !root.is_dir() {
        return Err(format!("不是有效目录: {}", root.display()));
    }

    let config = load_config(root);
    let mut chapters = Vec::new();
    let mut docs = Vec::new();

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
        if !is_markdown(path) {
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

        let rel = to_rel(root, path);
        let dir = path
            .parent()
            .map(|p| to_rel(root, p))
            .unwrap_or_default();

        chapters.push(ChapterEntry {
            path: rel.clone(),
            dir: dir.clone(),
            title: title.clone(),
            order: front.order.unwrap_or(i64::MAX),
            status: front.status,
            words: words.total,
            id: front.id,
        });

        docs.push(super::search::DocInput {
            path: rel,
            title,
            dir,
            body: split.body.to_string(),
            characters: front.characters,
            pov: front.pov,
        });
    }

    chapters.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.path.cmp(&b.path)));
    let total_words = chapters.iter().map(|c| c.words).sum();

    let title = config.title.clone().unwrap_or_else(|| {
        root.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "清籁斋".to_string())
    });

    Ok(ScanOutcome {
        snapshot: VaultSnapshot {
            root: root.to_string_lossy().to_string(),
            title,
            count_punctuation: config.count_punctuation,
            chapters,
            total_words,
            scanned_ms: started.elapsed().as_millis(),
        },
        docs,
    })
}

/// 从文件名去掉排序前缀：`001-楔子` → `楔子`。
pub(crate) fn strip_order_prefix(stem: &str) -> String {
    let without_digits = stem.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = without_digits.trim_start_matches(['-', '_', '.', ' ']);
    if rest.is_empty() || rest.len() == stem.len() {
        stem.to_string()
    } else {
        rest.to_string()
    }
}

// ---------------------------------------------------------------------------
// 章节增删改
// ---------------------------------------------------------------------------

/// Windows 保留设备名，用作文件名会直接失败。
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 把标题转成可安全落盘的文件名片段。
///
/// 中文原样保留，只替换路径分隔符与 Windows 非法字符，避免用户输入 `卷一/楔子`
/// 时把文件写到别的目录去。
pub fn sanitize_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '·',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();

    let trimmed = cleaned.trim().trim_matches('.').trim();
    let limited: String = trimmed.chars().take(60).collect();
    if limited.is_empty() {
        return "未命名".to_string();
    }
    if RESERVED_NAMES
        .iter()
        .any(|r| r.eq_ignore_ascii_case(&limited))
    {
        return format!("_{limited}");
    }
    limited
}

/// 取文件名开头的数字串，作为排序前缀。
fn leading_digits(stem: &str) -> &str {
    let end = stem
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(stem.len());
    &stem[..end]
}

/// 目录内下一个可用的 order 值。
fn next_order_in_dir(dir: &Path) -> Result<i64, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("读取目录失败：{e}"))?;
    let mut max_order = 0i64;
    let mut count = 0i64;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("md")) != Some(true)
        {
            continue;
        }
        count += 1;
        if let Ok(text) = fs::read_to_string(&path) {
            let split = md::split(&text);
            if let Some(front) = split.yaml.and_then(md::parse_front_matter) {
                if let Some(order) = front.order {
                    max_order = max_order.max(order);
                }
            }
        }
    }

    Ok(max_order.max(count) + 1)
}

/// 在指定目录下新建一章，返回其相对路径。
pub fn create_document(root: &Path, rel_dir: &str, title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("标题不能为空".to_string());
    }

    let dir = if rel_dir.trim().is_empty() {
        root.to_path_buf()
    } else {
        safe_join(root, rel_dir).ok_or_else(|| format!("非法目录：{rel_dir}"))?
    };
    fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败：{e}"))?;

    let order = next_order_in_dir(&dir)?;
    let stem = sanitize_filename(title);

    let mut filename = format!("{order:03}-{stem}.md");
    let mut seq = 1;
    while dir.join(&filename).exists() {
        seq += 1;
        filename = format!("{order:03}-{stem}-{seq}.md");
    }

    let abs = dir.join(&filename);
    let front = md::FrontMatter {
        id: Some(ulid::Ulid::new().to_string()),
        title: Some(title.to_string()),
        order: Some(order),
        status: Some(DocStatus::Draft),
        ..Default::default()
    };
    let text = md::compose(&front, "");

    // 用 create_new 语义写入，避免并发或重名时静默覆盖已有稿件
    if abs.exists() {
        return Err(format!("文件已存在：{filename}"));
    }
    atomic::write_atomic(&abs, text.as_bytes()).map_err(|e| format!("写入失败：{e}"))?;

    Ok(to_rel(root, &abs))
}

/// 重命名一章：改 front matter 的 title，并让文件名跟随标题。
///
/// 排序前缀（`001-`）原样保留，避免重命名打乱目录顺序。
pub fn rename_document(root: &Path, rel: &str, new_title: &str) -> Result<String, String> {
    let new_title = new_title.trim();
    if new_title.is_empty() {
        return Err("标题不能为空".to_string());
    }

    let abs = safe_join(root, rel).ok_or_else(|| format!("非法路径：{rel}"))?;
    let text = fs::read_to_string(&abs).map_err(|e| format!("读取失败：{e}"))?;

    let next_text = md::set_title(&text, new_title);
    atomic::write_atomic(&abs, next_text.as_bytes()).map_err(|e| format!("写入失败：{e}"))?;

    let dir = abs.parent().ok_or_else(|| "目标没有父目录".to_string())?;
    let old_name = abs
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let old_stem = abs
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let prefix = leading_digits(&old_stem).to_string();
    let stem = sanitize_filename(new_title);

    let build_name = |seq: usize| -> String {
        let base = if prefix.is_empty() {
            stem.clone()
        } else {
            format!("{prefix}-{stem}")
        };
        if seq == 1 {
            format!("{base}.md")
        } else {
            format!("{base}-{seq}.md")
        }
    };

    let mut new_name = build_name(1);
    if new_name == old_name {
        return Ok(to_rel(root, &abs));
    }

    let mut seq = 1;
    let mut target = dir.join(&new_name);
    while target.exists() {
        seq += 1;
        new_name = build_name(seq);
        target = dir.join(&new_name);
    }

    fs::rename(&abs, &target).map_err(|e| format!("重命名文件失败：{e}"))?;
    Ok(to_rel(root, &target))
}

/// 把一章移入稿库内的回收站，返回它在回收站里的相对路径。
///
/// 刻意不做物理删除：写作工具误删一章的代价远高于留一点垃圾。
/// 回收站目录以 `.` 开头，扫描时会被自动跳过。
pub fn delete_document(root: &Path, rel: &str) -> Result<String, String> {
    let abs = safe_join(root, rel).ok_or_else(|| format!("非法路径：{rel}"))?;
    if !abs.is_file() {
        return Err(format!("文件不存在：{rel}"));
    }

    let trash = root.join(".trash");
    fs::create_dir_all(&trash).map_err(|e| format!("创建回收站失败：{e}"))?;

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let name = abs
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "doc.md".to_string());

    let mut target = trash.join(format!("{stamp}-{name}"));
    let mut seq = 1;
    while target.exists() {
        seq += 1;
        target = trash.join(format!("{stamp}-{seq}-{name}"));
    }

    fs::rename(&abs, &target).map_err(|e| format!("移入回收站失败：{e}"))?;
    Ok(to_rel(root, &target))
}

// ---------------------------------------------------------------------------
// 章节排序
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Up,
    Down,
}

/// 一次重排中发生的路径变化，前端据此修正当前打开的文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathRename {
    pub from: String,
    pub to: String,
}

struct ChapterSlot {
    path: PathBuf,
    order: i64,
    title: String,
    text: String,
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

fn collect_chapters(dir: &Path) -> Result<Vec<ChapterSlot>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("读取目录失败：{e}"))?;
    let mut slots = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_markdown(&path) {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        let (order, title) = {
            let split = md::split(&text);
            let front = split.yaml.and_then(md::parse_front_matter).unwrap_or_default();
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let title = front
                .title
                .clone()
                .unwrap_or_else(|| strip_order_prefix(&stem));
            (front.order.unwrap_or(i64::MAX), title)
        };
        slots.push(ChapterSlot {
            path,
            order,
            title,
            text,
        });
    }

    slots.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.path.cmp(&b.path)));
    Ok(slots)
}

/// 把一章在同目录内上移或下移一位，返回发生的路径变化。
///
/// 排序以 front matter 的 `order` 为准，文件名前缀随之重算 —— 否则在文件管理器里
/// 看到的顺序会和应用内不一致。
///
/// 只改写 order 或文件名真正变化的文件，因此一次移动通常只动两个文件：
/// 按位置重编号时，未换位的章节其序号本就不变。这同时顺带修掉序号空洞与重复。
pub fn reorder_document(
    root: &Path,
    rel: &str,
    direction: Direction,
) -> Result<Vec<PathRename>, String> {
    let abs = safe_join(root, rel).ok_or_else(|| format!("非法路径：{rel}"))?;
    if !abs.is_file() {
        return Err(format!("文件不存在：{rel}"));
    }
    let dir = abs
        .parent()
        .ok_or_else(|| "目标没有父目录".to_string())?
        .to_path_buf();

    let mut slots = collect_chapters(&dir)?;
    let index = slots
        .iter()
        .position(|s| s.path == abs)
        .ok_or_else(|| format!("目标不在该目录：{rel}"))?;

    let neighbor = match direction {
        Direction::Up if index > 0 => index - 1,
        Direction::Down if index + 1 < slots.len() => index + 1,
        // 已在头尾，无需移动
        _ => return Ok(Vec::new()),
    };
    slots.swap(index, neighbor);

    // 1. 按新位置重编号，只写真正变化的文件
    for (i, slot) in slots.iter_mut().enumerate() {
        let new_order = (i + 1) as i64;
        if slot.order == new_order {
            continue;
        }
        slot.text = md::set_scalar(&slot.text, "order", &new_order.to_string());
        slot.order = new_order;
    }
    for slot in &slots {
        let current = fs::read_to_string(&slot.path).map_err(|e| format!("读取失败：{e}"))?;
        if current != slot.text {
            atomic::write_atomic(&slot.path, slot.text.as_bytes())
                .map_err(|e| format!("写入失败：{e}"))?;
        }
    }

    // 2. 两阶段改名：先把要动的文件挪到临时名，腾空所有目标位，
    //    否则两章互换文件名时会互相占用。
    let mut pending: Vec<(usize, PathBuf)> = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        let desired = dir.join(format!(
            "{:03}-{}.md",
            i + 1,
            sanitize_filename(&slot.title)
        ));
        if desired == slot.path {
            continue;
        }
        let temp = dir.join(format!(".qcl-reorder-{i}-{}.tmp", std::process::id()));
        fs::rename(&slot.path, &temp).map_err(|e| format!("重排改名失败：{e}"))?;
        pending.push((i, temp));
    }

    // 3. 依次落位。留在原位的文件先占住自己的名字，重名章节自动加后缀。
    let mut occupied: HashSet<PathBuf> = slots
        .iter()
        .enumerate()
        .filter(|(i, _)| !pending.iter().any(|(pi, _)| pi == i))
        .map(|(_, s)| s.path.clone())
        .collect();

    let mut renames = Vec::new();
    for (i, temp) in pending {
        let slot = &slots[i];
        let base = format!("{:03}-{}", i + 1, sanitize_filename(&slot.title));

        let mut name = format!("{base}.md");
        let mut seq = 1;
        while occupied.contains(&dir.join(&name)) {
            seq += 1;
            name = format!("{base}-{seq}.md");
        }

        let target = dir.join(&name);
        fs::rename(&temp, &target).map_err(|e| format!("重排落位失败：{e}"))?;
        occupied.insert(target.clone());
        renames.push(PathRename {
            from: to_rel(root, &slot.path),
            to: to_rel(root, &target),
        });
    }

    Ok(renames)
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

    #[test]
    fn 新建章节带序号与元数据() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let rel = create_document(root, "小说/卷一", "楔子").unwrap();
        assert_eq!(rel, "小说/卷一/001-楔子.md");

        let text = fs::read_to_string(root.join(&rel)).unwrap();
        let front = md::parse_front_matter(md::split(&text).yaml.unwrap()).unwrap();
        assert_eq!(front.title.as_deref(), Some("楔子"));
        assert_eq!(front.order, Some(1));
        assert_eq!(front.status, Some(DocStatus::Draft));
        assert!(front.id.is_some(), "应生成 ULID");
        assert_eq!(md::split(&text).body, "", "新章正文应为空");
    }

    #[test]
    fn 连续新建序号递增() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        assert_eq!(create_document(root, "卷一", "楔子").unwrap(), "卷一/001-楔子.md");
        assert_eq!(create_document(root, "卷一", "开端").unwrap(), "卷一/002-开端.md");
        assert_eq!(create_document(root, "卷一", "夜航").unwrap(), "卷一/003-夜航.md");
    }

    #[test]
    fn 新建到根目录() {
        let dir = tempfile::tempdir().unwrap();
        let rel = create_document(dir.path(), "", "灵感").unwrap();
        assert_eq!(rel, "001-灵感.md");
    }

    #[test]
    fn 标题里的路径分隔符不会造成目录穿越() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = create_document(root, "卷一", "../../越界/楔子").unwrap();
        // 分隔符被替换成中点，文件仍落在指定目录内
        assert!(rel.starts_with("卷一/"), "实际: {rel}");
        let abs = root.join(&rel);
        assert!(abs.is_file());
        assert_eq!(abs.parent().unwrap(), root.join("卷一"));
        // 库外不应出现任何文件
        assert!(!root.parent().unwrap().join("越界").exists());
    }

    #[test]
    fn 空标题被拒绝() {
        let dir = tempfile::tempdir().unwrap();
        assert!(create_document(dir.path(), "卷一", "   ").is_err());
    }

    #[test]
    fn 文件名清洗规则() {
        assert_eq!(sanitize_filename("楔子"), "楔子");
        assert_eq!(sanitize_filename("卷一/楔子"), "卷一·楔子");
        assert_eq!(sanitize_filename("a:b*c?d"), "a·b·c·d");
        assert_eq!(sanitize_filename("  "), "未命名");
        assert_eq!(sanitize_filename("..."), "未命名");
        assert_eq!(sanitize_filename("CON"), "_CON");
        // 超长标题被截断
        assert_eq!(sanitize_filename(&"字".repeat(100)).chars().count(), 60);
    }

    #[test]
    fn 重命名同时改标题与文件名() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = create_document(root, "卷一", "楔子").unwrap();
        fs::write(root.join(&rel), "---\ntitle: 楔子\norder: 1\ncustom: 保留\n---\n旧正文。\n")
            .unwrap();

        let next = rename_document(root, &rel, "开端").unwrap();
        assert_eq!(next, "卷一/001-开端.md");
        assert!(!root.join(&rel).exists(), "旧文件名应已不存在");

        let text = fs::read_to_string(root.join(&next)).unwrap();
        let front = md::parse_front_matter(md::split(&text).yaml.unwrap()).unwrap();
        assert_eq!(front.title.as_deref(), Some("开端"));
        assert_eq!(front.order, Some(1));
        // 正文与其他自定义字段都不受影响
        assert_eq!(md::split(&text).body, "旧正文。\n");
        assert!(text.contains("custom: 保留"));
    }

    #[test]
    fn 重命名为同名文件时原地不动() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = create_document(root, "卷一", "楔子").unwrap();
        let next = rename_document(root, &rel, "楔子").unwrap();
        assert_eq!(next, rel);
        assert!(root.join(&rel).is_file());
    }

    #[test]
    fn 重命名遇到同名文件自动加序号() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let a = create_document(root, "卷一", "甲").unwrap();
        assert_eq!(a, "卷一/001-甲.md");

        // 手工造一个同前缀同标题的文件，模拟历史遗留的同名稿件
        fs::create_dir_all(root.join("卷一")).unwrap();
        fs::write(root.join("卷一/001-乙.md"), "---\ntitle: 乙\n---\n").unwrap();

        let next = rename_document(root, &a, "乙").unwrap();
        assert_eq!(next, "卷一/001-乙-2.md");
        assert!(root.join(&next).is_file());
        assert!(root.join("卷一/001-乙.md").is_file(), "原有文件不能被覆盖");
    }

    #[test]
    fn 删除是移入回收站而非物理删除() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = create_document(root, "卷一", "楔子").unwrap();

        let trashed = delete_document(root, &rel).unwrap();
        assert!(trashed.starts_with(".trash/"), "实际: {trashed}");
        assert!(!root.join(&rel).exists());
        assert!(root.join(&trashed).is_file(), "回收站里应还能找到");

        // 回收站目录以 . 开头，扫描时被跳过
        let snap = scan(root).unwrap();
        assert_eq!(snap.chapters.len(), 0);
    }

    #[test]
    fn 删除不存在的文件报错() {
        let dir = tempfile::tempdir().unwrap();
        assert!(delete_document(dir.path(), "不存在.md").is_err());
    }

    #[test]
    fn 新建与删除都拒绝越界路径() {
        let dir = tempfile::tempdir().unwrap();
        assert!(create_document(dir.path(), "../外面", "楔子").is_err());
        assert!(delete_document(dir.path(), "../外面.md").is_err());
        assert!(rename_document(dir.path(), "../外面.md", "新名").is_err());
    }

    // ---------------------------------------------------------------- 排序

    fn 造三章(root: &Path) -> Vec<String> {
        vec![
            create_document(root, "卷一", "甲").unwrap(),
            create_document(root, "卷一", "乙").unwrap(),
            create_document(root, "卷一", "丙").unwrap(),
        ]
    }

    fn 目录顺序(root: &Path) -> Vec<String> {
        scan(root)
            .unwrap()
            .chapters
            .into_iter()
            .map(|c| c.title)
            .collect()
    }

    #[test]
    fn 上移一位并重算文件名前缀() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let chapters = 造三章(root);
        assert_eq!(目录顺序(root), vec!["甲", "乙", "丙"]);

        // 把「乙」上移
        let renames = reorder_document(root, &chapters[1], Direction::Up).unwrap();
        assert_eq!(目录顺序(root), vec!["乙", "甲", "丙"]);

        // 只动换位的两个文件
        assert_eq!(renames.len(), 2, "实际: {renames:?}");
        assert!(root.join("卷一/001-乙.md").is_file());
        assert!(root.join("卷一/002-甲.md").is_file());
        assert!(root.join("卷一/003-丙.md").is_file());

        // front matter 的 order 与文件名前缀一致
        let text = fs::read_to_string(root.join("卷一/001-乙.md")).unwrap();
        let front = md::parse_front_matter(md::split(&text).yaml.unwrap()).unwrap();
        assert_eq!(front.order, Some(1));
        assert_eq!(front.title.as_deref(), Some("乙"));
    }

    #[test]
    fn 下移一位() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let chapters = 造三章(root);

        reorder_document(root, &chapters[0], Direction::Down).unwrap();
        assert_eq!(目录顺序(root), vec!["乙", "甲", "丙"]);
    }

    #[test]
    fn 在头尾移动无变化() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let chapters = 造三章(root);

        assert!(reorder_document(root, &chapters[0], Direction::Up).unwrap().is_empty());
        assert!(reorder_document(root, &chapters[2], Direction::Down).unwrap().is_empty());
        assert_eq!(目录顺序(root), vec!["甲", "乙", "丙"]);
    }

    #[test]
    fn 重排后正文不受影响() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let chapters = 造三章(root);
        fs::write(
            root.join(&chapters[1]),
            "---\ntitle: 乙\norder: 2\ncustom: 保留\n---\n乙的正文。\n",
        )
        .unwrap();

        reorder_document(root, &chapters[1], Direction::Up).unwrap();

        let text = fs::read_to_string(root.join("卷一/001-乙.md")).unwrap();
        assert_eq!(md::split(&text).body, "乙的正文。\n");
        assert!(text.contains("custom: 保留"), "自定义字段不能丢");
        assert!(text.contains("title: 乙"));
    }

    #[test]
    fn 序号有空洞时重排会归一化() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "卷一/001-甲.md", "---\ntitle: 甲\norder: 1\n---\n");
        write(root, "卷一/002-乙.md", "---\ntitle: 乙\norder: 7\n---\n");
        write(root, "卷一/003-丙.md", "---\ntitle: 丙\norder: 9\n---\n");

        // 丙(9) 上移 → 顺序变为 甲, 丙, 乙，重编号为 1,2,3
        reorder_document(root, "卷一/003-丙.md", Direction::Up).unwrap();
        assert_eq!(目录顺序(root), vec!["甲", "丙", "乙"]);
        assert!(root.join("卷一/002-丙.md").is_file());
        assert!(root.join("卷一/003-乙.md").is_file());
    }

    #[test]
    fn 同名章节重排不会互相覆盖() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "卷一/001-楔子.md", "---\ntitle: 楔子\norder: 1\n---\n甲内容\n");
        write(root, "卷一/002-楔子.md", "---\ntitle: 楔子\norder: 2\n---\n乙内容\n");

        reorder_document(root, "卷一/002-楔子.md", Direction::Up).unwrap();

        // 两个文件都还在，内容没有互相覆盖
        let a = fs::read_to_string(root.join("卷一/001-楔子.md")).unwrap();
        let b = fs::read_to_string(root.join("卷一/002-楔子.md")).unwrap();
        assert!(a.contains("乙内容"), "实际: {a:?}");
        assert!(b.contains("甲内容"), "实际: {b:?}");
    }

    #[test]
    fn 无_front_matter_的章节也能参与重排() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "卷一/001-甲.md", "---\ntitle: 甲\norder: 1\n---\n");
        write(root, "卷一/乙.md", "没有元数据的正文。\n");

        // 无元数据的排在最后（order = i64::MAX）
        assert_eq!(目录顺序(root), vec!["甲", "乙"]);

        reorder_document(root, "卷一/乙.md", Direction::Up).unwrap();
        assert_eq!(目录顺序(root), vec!["乙", "甲"]);

        let text = fs::read_to_string(root.join("卷一/001-乙.md")).unwrap();
        assert!(text.starts_with("---\n"), "应补上元数据: {text:?}");
        assert_eq!(md::split(&text).body, "没有元数据的正文。\n");
    }

    #[test]
    fn 只影响同目录不波及其他目录() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        造三章(root);
        write(root, "卷二/001-丁.md", "---\ntitle: 丁\norder: 1\n---\n");

        reorder_document(root, "卷一/002-乙.md", Direction::Up).unwrap();
        assert!(root.join("卷二/001-丁.md").is_file(), "别的目录不该被动");
    }

    #[test]
    fn 重排拒绝越界路径与非法方向() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        造三章(root);
        assert!(reorder_document(root, "../外面.md", Direction::Up).is_err());
        assert!(reorder_document(root, "卷一/不存在.md", Direction::Up).is_err());
    }

    // -------------------------------------------------------------- 检索

    #[test]
    fn 扫描建索引后能搜到正文() {
        use crate::engine::search::SearchIndex;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "卷一/001-楔子.md", "---\ntitle: 楔子\n---\n檐角的铁马响了一声。\n");
        write(root, "卷一/002-夜航.md", "---\ntitle: 夜航\n---\n船过第三道湾。\n");

        let outcome = scan_with_docs(root).unwrap();
        assert_eq!(outcome.docs.len(), 2);
        let index = SearchIndex::from_docs(outcome.docs);

        let hits = index.search("铁马", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "楔子");
        assert_eq!(hits[0].dir, "卷一");
        assert_eq!(hits[0].path, "卷一/001-楔子.md");
        assert!(hits[0].snippet.contains("铁马"));
    }

    #[test]
    fn front_matter_不进索引() {
        use crate::engine::search::SearchIndex;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "001-楔子.md", "---\ntitle: 楔子\n---\n正文里没有那个词。\n");

        let index = SearchIndex::from_docs(scan_with_docs(root).unwrap().docs);
        // 「楔子」只出现在元数据里，不该被搜到
        assert!(index.search("楔子", 10).is_empty());
        assert_eq!(index.search("正文", 10).len(), 1);
    }

    #[test]
    fn 扫描产出的标题与章节清单一致() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "卷一/003-夜航.md", "---\ntitle: 夜航\norder: 3\n---\n正文\n");

        let outcome = scan_with_docs(root).unwrap();
        assert_eq!(outcome.docs.len(), outcome.snapshot.chapters.len());
        assert_eq!(outcome.docs[0].title, outcome.snapshot.chapters[0].title);
        assert_eq!(outcome.docs[0].dir, outcome.snapshot.chapters[0].dir);
    }

    #[test]
    fn 端到端双链反链() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "001-楔子.md",
            "---\ntitle: 楔子\norder: 1\n---\n\n他想起[[夜航]]上那句话。\n",
        );
        write(
            root,
            "002-夜航.md",
            "---\ntitle: 夜航\norder: 2\n---\n\n夜航的正文。\n",
        );

        let index = crate::engine::links::LinkIndex::from_docs(&scan_with_docs(root).unwrap().docs);
        let hits = index.backlinks("002-夜航.md");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "楔子");
        assert!(hits[0].contexts[0].contains("夜航"));
        assert!(index.dangling().is_empty());
    }

    #[test]
    fn 端到端_front_matter_里的双链不被索引() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 元数据里出现的 [[夜航]] 只是普通字符串，不是引用
        write(
            root,
            "001-楔子.md",
            "---\ntitle: 楔子\norder: 1\nnote: \"[[夜航]]\"\n---\n\n正文没有链接。\n",
        );
        write(
            root,
            "002-夜航.md",
            "---\ntitle: 夜航\norder: 2\n---\n\n正文。\n",
        );

        let index = crate::engine::links::LinkIndex::from_docs(&scan_with_docs(root).unwrap().docs);
        assert!(index.backlinks("002-夜航.md").is_empty());
    }
}
