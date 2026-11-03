//! Markdown 文档的拆分、合成与字数统计。
//!
//! 核心设计：拆分结果保留 `head` 原文（含起止分隔行），因此「只改正文」的保存
//! 可以做到字节级保真 —— front matter 永远不会因为序列化器行为变化而漂移。
//! 这是本项目数据安全的底线，见 `docs/architecture.md` 第 3 节。

use serde::{Deserialize, Serialize};

/// 章节状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocStatus {
    Draft,
    Revising,
    Done,
}

impl Default for DocStatus {
    fn default() -> Self {
        DocStatus::Draft
    }
}

/// 章节 front matter。未知字段经 `extra` 原样保留，不丢用户自定义键。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrontMatter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DocStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub characters: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pov: Option<String>,
    /// 一句话情节摘要，供大纲视图使用。
    ///
    /// 放在 front matter 而不是独立大纲文件：摘要属于这一章，同文件存储不会
    /// 因重命名、移动、重排而失步。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// 派生缓存值。写入端不主动刷新，避免每次保存都改动 front matter 造成无谓 diff。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<u64>,
    #[serde(default, flatten)]
    pub extra: serde_yaml::Mapping,
}

/// 拆分结果。
///
/// - `head` 是从文件开头到闭合分隔行（含换行）的完整原文，重建时直接前置即可。
/// - `yaml` 是分隔行之间的 YAML 文本，仅供解析。
/// - `body` 是正文。
#[derive(Debug, Clone, Copy)]
pub struct SplitDoc<'a> {
    pub head: &'a str,
    pub yaml: Option<&'a str>,
    pub body: &'a str,
    /// YAML 段在原文中的起始字节偏移。改写单个字段时靠它做原位拼接，
    /// 分隔行的换行风格（LF / CRLF）因此得以保持。
    pub yaml_start: usize,
}

/// 拆分 front matter 与正文。
///
/// 未闭合的分隔符视为「没有 front matter」，整个文件都是正文 —— 宁可把分隔线当正文，
/// 也不能把用户的正文误吞成元数据。
pub fn split(text: &str) -> SplitDoc<'_> {
    let open_len = if text.starts_with("---\r\n") {
        5
    } else if text.starts_with("---\n") {
        4
    } else {
        return SplitDoc {
            head: "",
            yaml: None,
            body: text,
            yaml_start: 0,
        };
    };

    let rest = &text[open_len..];
    let mut offset = 0usize;

    loop {
        let line_end = match rest[offset..].find('\n') {
            Some(i) => offset + i,
            None => rest.len(),
        };
        let line = rest[offset..line_end].trim_end_matches('\r');

        if line == "---" {
            let close_end = if line_end < rest.len() {
                line_end + 1
            } else {
                rest.len()
            };
            let head_end = open_len + close_end;
            return SplitDoc {
                head: &text[..head_end],
                yaml: Some(&rest[..offset]),
                body: &text[head_end..],
                yaml_start: open_len,
            };
        }

        if line_end >= rest.len() {
            break;
        }
        offset = line_end + 1;
    }

    SplitDoc {
        head: "",
        yaml: None,
        body: text,
        yaml_start: 0,
    }
}

/// 解析 front matter。解析失败返回 `None`，调用方应退化为「无元数据」而非报错。
pub fn parse_front_matter(yaml: &str) -> Option<FrontMatter> {
    if yaml.trim().is_empty() {
        return Some(FrontMatter::default());
    }
    serde_yaml::from_str(yaml).ok()
}

/// 用新正文替换原文正文，front matter 字节不变。
pub fn replace_body(original: &str, new_body: &str) -> String {
    let s = split(original);
    let mut out = String::with_capacity(s.head.len() + new_body.len());
    out.push_str(s.head);
    out.push_str(new_body);
    out
}

/// 由 front matter 与正文合成一篇新文档。
pub fn compose(front: &FrontMatter, body: &str) -> String {
    match serde_yaml::to_string(front) {
        Ok(yaml) => format!("---\n{}---\n{}", yaml, body),
        Err(_) => body.to_string(),
    }
}

/// 在 front matter 中就地改写单个标量字段，其余字节保持不动。
///
/// 只处理单行标量：找到行首的 `key:` 整行替换，找不到则插到元数据顶部。
/// 没有 front matter 时创建一段只含该字段的元数据，正文原样接在后面。
pub fn set_scalar(original: &str, key: &str, value: &str) -> String {
    let split = split(original);
    let Some(yaml) = split.yaml else {
        return format!(
            "---\n{}: {}\n---\n{}",
            key,
            escape_yaml_scalar(value),
            original
        );
    };

    let next_yaml = upsert_scalar_line(yaml, key, value);
    let mut out = String::with_capacity(original.len() + next_yaml.len());
    out.push_str(&original[..split.yaml_start]);
    out.push_str(&next_yaml);
    out.push_str(&original[split.yaml_start + yaml.len()..]);
    out
}

/// 改写标题。
pub fn set_title(original: &str, title: &str) -> String {
    set_scalar(original, "title", title)
}

/// 整行替换或插入一个标量字段，保留原文件的换行风格。
fn upsert_scalar_line(yaml: &str, key: &str, value: &str) -> String {
    let escaped = escape_yaml_scalar(value);
    let mut out = String::with_capacity(yaml.len() + escaped.len() + 8);
    let mut replaced = false;

    for line in yaml.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        let is_target = !replaced
            && content.len() > key.len()
            && content.starts_with(key)
            && content.as_bytes().get(key.len()) == Some(&b':');

        if is_target {
            let eol = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
            out.push_str(key);
            out.push_str(": ");
            out.push_str(&escaped);
            out.push_str(eol);
            replaced = true;
        } else {
            out.push_str(line);
        }
    }

    if replaced {
        return out;
    }

    let eol = if yaml.contains("\r\n") { "\r\n" } else { "\n" };
    let mut prefixed = String::with_capacity(out.len() + escaped.len() + 8);
    prefixed.push_str(key);
    prefixed.push_str(": ");
    prefixed.push_str(&escaped);
    prefixed.push_str(eol);
    prefixed.push_str(&out);
    prefixed
}

/// YAML 标量转义。只在必要时加双引号，避免把普通中文标题写成带引号的形式。
fn escape_yaml_scalar(value: &str) -> String {
    let needs_quote = value.is_empty()
        || value.trim() != value
        || value
            .chars()
            .any(|c| matches!(c, ':' | '#' | '"' | '\'' | '\\' | '\n' | '\r' | '\t'))
        || value.starts_with([
            '-', '?', '!', '&', '*', '|', '>', '%', '@', '`', '[', ']', '{', '}', ',', '\'',
            '"',
        ]);

    if !needs_quote {
        return value.to_string();
    }

    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

// ---------------------------------------------------------------------------
// 字数统计
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordCount {
    /// 中日韩表意文字与假名、谚文字符数
    pub cjk: u64,
    /// 非 CJK 的字母/数字词组数
    pub latin: u64,
    /// 标点与符号数
    pub punctuation: u64,
    /// 计入统计口径的总字数
    pub total: u64,
}

/// CJK 字符判定。覆盖基本区、扩展 A、兼容区、假名、谚文与扩展 B 以上。
pub fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
            | 0x20000..=0x2FA1F
    )
}

/// 统计字数。
///
/// 口径与 Word「中文字数」对齐：CJK 逐字计数，非 CJK 按词计数，标点默认不计。
/// 由于标点默认排除，Markdown 语法符号（`#` `*` `-`）不会污染结果。
pub fn count_words(text: &str, count_punctuation: bool) -> WordCount {
    let mut cjk = 0u64;
    let mut latin = 0u64;
    let mut punctuation = 0u64;
    let mut in_word = false;

    for c in text.chars() {
        if is_cjk(c) {
            cjk += 1;
            in_word = false;
        } else if c.is_alphanumeric() {
            if !in_word {
                latin += 1;
                in_word = true;
            }
        } else if c.is_whitespace() {
            in_word = false;
        } else {
            punctuation += 1;
            in_word = false;
        }
    }

    let total = cjk + latin + if count_punctuation { punctuation } else { 0 };
    WordCount {
        cjk,
        latin,
        punctuation,
        total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 无_front_matter_时整篇都是正文() {
        let text = "第一段。\n\n第二段。\n";
        let s = split(text);
        assert_eq!(s.head, "");
        assert!(s.yaml.is_none());
        assert_eq!(s.body, text);
    }

    #[test]
    fn 拆分_lf_格式() {
        let text = "---\ntitle: 楔子\norder: 1\n---\n正文第一段。\n";
        let s = split(text);
        assert_eq!(s.head, "---\ntitle: 楔子\norder: 1\n---\n");
        assert_eq!(s.yaml, Some("title: 楔子\norder: 1\n"));
        assert_eq!(s.body, "正文第一段。\n");
    }

    #[test]
    fn 拆分_crlf_格式且保留原样() {
        let text = "---\r\ntitle: 楔子\r\n---\r\n正文。\r\n";
        let s = split(text);
        assert_eq!(s.head, "---\r\ntitle: 楔子\r\n---\r\n");
        assert_eq!(s.body, "正文。\r\n");
        // 关键：head 原样保留，重建时不会把 CRLF 洗成 LF
        assert_eq!(replace_body(text, "新正文。\r\n"), "---\r\ntitle: 楔子\r\n---\r\n新正文。\r\n");
    }

    #[test]
    fn 未闭合分隔符视为无_front_matter() {
        let text = "---\ntitle: 楔子\n正文没有闭合。\n";
        let s = split(text);
        assert!(s.yaml.is_none());
        assert_eq!(s.body, text);
    }

    #[test]
    fn 空正文也能正确拆分() {
        let text = "---\ntitle: 楔子\n---\n";
        let s = split(text);
        assert_eq!(s.body, "");
        assert_eq!(s.yaml, Some("title: 楔子\n"));
    }

    #[test]
    fn 正文未改动时替换是字节幂等的() {
        let cases = [
            "---\ntitle: 楔子\norder: 1\n---\n正文第一段。\n\n第二段。\n",
            "---\r\ntitle: 楔子\r\n---\r\n正文。\r\n",
            "纯正文，没有元数据。\n",
            "",
            "---\n---\n",
        ];
        for original in cases {
            let body = split(original).body;
            assert_eq!(replace_body(original, body), original, "往返失真: {original:?}");
        }
    }

    #[test]
    fn 只改正文不动_front_matter() {
        let original = "---\ntitle: 楔子\norder: 1\ncustom_key: 保留我\n---\n旧正文。\n";
        let next = replace_body(original, "新正文。\n");
        assert!(next.starts_with("---\ntitle: 楔子\norder: 1\ncustom_key: 保留我\n---\n"));
        assert!(next.ends_with("新正文。\n"));
    }

    #[test]
    fn 解析未知字段不丢失() {
        let yaml = "title: 楔子\norder: 3\nstatus: revising\nmy_flag: true\ncharacters:\n  - 沈砚\n";
        let f = parse_front_matter(yaml).expect("应能解析");
        assert_eq!(f.title.as_deref(), Some("楔子"));
        assert_eq!(f.order, Some(3));
        assert_eq!(f.status, Some(DocStatus::Revising));
        assert_eq!(f.characters, vec!["沈砚".to_string()]);
        // my_flag 未被结构化字段吸收，应完整落进 extra
        assert_eq!(f.extra.len(), 1);
    }

    #[test]
    fn 空_yaml_解析为默认值() {
        let f = parse_front_matter("").expect("空 YAML 应成功");
        assert!(f.title.is_none());
        assert!(f.extra.is_empty());
    }

    #[test]
    fn 合成新文档结构正确() {
        let mut f = FrontMatter::default();
        f.title = Some("楔子".into());
        f.order = Some(1);
        let doc = compose(&f, "正文。\n");
        assert!(doc.starts_with("---\n"));
        assert!(doc.contains("title: 楔子"));
        assert!(doc.ends_with("---\n正文。\n"));
        // 合成结果必须能被自己拆回来
        let s = split(&doc);
        assert_eq!(s.body, "正文。\n");
    }

    #[test]
    fn 中文字数按字计() {
        let wc = count_words("清籁斋里写小说。", false);
        assert_eq!(wc.cjk, 7);
        assert_eq!(wc.latin, 0);
        assert_eq!(wc.punctuation, 1);
        assert_eq!(wc.total, 7);
    }

    #[test]
    fn 中英混排按字加词计() {
        let wc = count_words("清籁斋 Scriptorium 2026", false);
        assert_eq!(wc.cjk, 3);
        assert_eq!(wc.latin, 2);
        assert_eq!(wc.total, 5);
    }

    #[test]
    fn 标点可选计入() {
        // 他 说 走 = 3 字；： 「 。 」 = 4 标点
        let text = "他说：「走。」";
        assert_eq!(count_words(text, false).total, 3);
        assert_eq!(count_words(text, true).total, 7);
    }

    #[test]
    fn markdown_语法符号不计入字数() {
        let wc = count_words("## 楔子\n\n**加粗**与*斜体*\n\n- 列表项\n", false);
        // 楔子 2 + 加粗 2 + 与 1 + 斜体 2 + 列表项 3 = 10
        assert_eq!(wc.cjk, 10);
        assert_eq!(wc.total, 10);
    }

    #[test]
    fn 省略号与破折号算标点() {
        // 他 走 了 然 后 = 5 字；…… 两字符 + —— 两字符 = 4 标点
        let wc = count_words("他走了……然后——", false);
        assert_eq!(wc.cjk, 5);
        assert_eq!(wc.punctuation, 4);
    }

    #[test]
    fn 空白与换行不计数() {
        let wc = count_words("  \n\n\t  ", false);
        assert_eq!(wc.total, 0);
    }

    #[test]
    fn 扩展区汉字可识别() {
        assert!(is_cjk('𠀋')); // U+2000B 扩展 B
        assert!(is_cjk('齋'));
        assert!(!is_cjk('A'));
        assert!(!is_cjk('，'));
    }

    #[test]
    fn 改标题不动其他字段与正文() {
        let original = "---\ntitle: 旧名\norder: 3\nstatus: draft\ncustom: 保留\n---\n正文。\n";
        let next = set_title(original, "新名");
        assert!(next.contains("title: 新名\n"));
        assert!(next.contains("order: 3\n"));
        assert!(next.contains("status: draft\n"));
        assert!(next.contains("custom: 保留\n"));
        assert!(next.ends_with("---\n正文。\n"));

        let front = parse_front_matter(split(&next).yaml.unwrap()).unwrap();
        assert_eq!(front.title.as_deref(), Some("新名"));
        assert_eq!(front.order, Some(3));
    }

    #[test]
    fn 标题含半角冒号时自动加引号且能解析回原值() {
        let original = "---\ntitle: 旧名\n---\n正文。\n";
        let next = set_title(original, "卷一: 听风");
        assert!(next.contains(r#"title: "卷一: 听风""#));

        let front = parse_front_matter(split(&next).yaml.unwrap()).unwrap();
        assert_eq!(front.title.as_deref(), Some("卷一: 听风"));
    }

    #[test]
    fn 全角冒号无需加引号也能解析() {
        let original = "---\ntitle: 旧名\n---\n正文。\n";
        let next = set_title(original, "卷一：听风");
        let front = parse_front_matter(split(&next).yaml.unwrap()).unwrap();
        assert_eq!(front.title.as_deref(), Some("卷一：听风"));
    }

    #[test]
    fn 标题含井号时不被当注释() {
        let original = "---\ntitle: 旧名\n---\n正文。\n";
        let next = set_title(original, "第三幕 #2");
        let front = parse_front_matter(split(&next).yaml.unwrap()).unwrap();
        assert_eq!(front.title.as_deref(), Some("第三幕 #2"));
    }

    #[test]
    fn 无_front_matter_时新建元数据并保留正文() {
        let original = "纯正文，没有元数据。\n";
        let next = set_title(original, "楔子");
        assert!(next.starts_with("---\ntitle: 楔子\n---\n"));
        assert!(next.ends_with("纯正文，没有元数据。\n"));
        assert_eq!(split(&next).body, original);
    }

    #[test]
    fn 字段不存在时插入到顶部() {
        let original = "---\norder: 1\n---\n正文。\n";
        let next = set_title(original, "楔子");
        assert!(next.starts_with("---\ntitle: 楔子\norder: 1\n---\n"));
    }

    #[test]
    fn 改写字段时保持_crlf() {
        let original = "---\r\ntitle: 旧名\r\norder: 2\r\n---\r\n正文。\r\n";
        let next = set_title(original, "新名");
        assert!(next.contains("title: 新名\r\n"));
        assert!(next.contains("order: 2\r\n"));
        assert!(next.ends_with("---\r\n正文。\r\n"));
        assert!(!next.contains("\n\r"), "不应混入孤立的 LF");
    }

    #[test]
    fn 只改数字字段时不动标题() {
        let original = "---\ntitle: 楔子\norder: 1\n---\n正文。\n";
        let next = set_scalar(original, "order", "7");
        assert!(next.contains("title: 楔子\n"));
        assert!(next.contains("order: 7\n"));
        let front = parse_front_matter(split(&next).yaml.unwrap()).unwrap();
        assert_eq!(front.order, Some(7));
    }
}
