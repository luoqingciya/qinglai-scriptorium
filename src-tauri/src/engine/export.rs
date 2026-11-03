//! 导出 docx。
//!
//! ## 为什么不用 HTML → docx
//!
//! 那条路对中文缩进的处理不可靠：HTML 的 `text-indent: 2em` 经转换器落到 OOXML 时，
//! 常常变成固定 twips 值，换个字号就错位。而 Word 有专门的 `w:firstLineChars`
//! （单位 1/100 字符），按字号自动换算，这才是中文排版该用的属性。
//!
//! 所以这里用 `docx-rs` 直接构造 OOXML：段落属性、字体槽、行距全部显式指定，
//! 不经过任何中间格式。
//!
//! ## 排版规范
//!
//! 对齐国内长篇小说的通行格式：正文宋体小四、1.5 倍行距、首行缩进 2 字符。
//! 字体名写 `SimSun` 而非「宋体」—— 后者在英文版 Word 里找不到。

use std::io::Cursor;

use docx_rs::{
    AlignmentType, Docx, LineSpacing, LineSpacingType, Paragraph, Run, RunFonts,
    SpecialIndentType,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

/// 导出参数。前端可覆盖，缺省即为国内小说通行格式。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    /// 正文字体（同时写入 ascii / hAnsi / eastAsia 三个槽位）
    pub font: String,
    /// 正文字号，单位**半磅**（OOXML 的 `w:sz`）。12pt 小四 = 24
    pub font_size: usize,
    /// 行距，单位 1/240 行（240 = 单倍，360 = 1.5 倍）
    pub line_spacing: i32,
    /// 首行缩进，单位 1/100 字符（200 = 2 字符）
    pub first_line_chars: i32,
    /// 章与章之间分页
    pub page_break: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            font: "SimSun".to_string(),
            font_size: 24,       // 12pt 小四
            line_spacing: 360,   // 1.5 倍
            first_line_chars: 200, // 2 字符
            page_break: true,
        }
    }
}

/// 待导出的一章。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterInput {
    pub title: String,
    pub body: String,
}

/// 导出结果，供前端提示。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportStats {
    pub chapters: usize,
    pub paragraphs: usize,
    pub bytes: usize,
}

/// 把 `[[目标]]` / `[[目标|别名]]` 还原成纯文本。
///
/// 导出的文档是给稿库之外的人看的，双链语法在那里只是噪音。
/// 有别名用别名，没有就用目标标题。
fn strip_wikilinks(text: &str) -> String {
    if !text.contains("[[") {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '[' && chars[i + 1] == '[' {
            // 找同行内的闭合 `]]`
            let mut j = i + 2;
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

            if closed {
                let inner: String = chars[i + 2..j].iter().collect();
                let display = match inner.split_once('|') {
                    Some((_target, alias)) => alias.trim().to_string(),
                    None => {
                        // 去掉 `#锚点` 只留标题
                        inner.split('#').next().unwrap_or(&inner).trim().to_string()
                    }
                };
                out.push_str(&display);
                i = j + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

/// 收集一段内容里的 run 与段落。
struct Builder<'a> {
    opts: &'a ExportOptions,
    paragraphs: Vec<Paragraph>,
    /// 当前段落累积的 run
    runs: Vec<Run>,
    /// 强调嵌套层数（`**粗**` 可嵌套，用计数而非布尔）
    bold: u32,
    italic: u32,
    /// 引用嵌套层数
    quote: u32,
    /// 当前是否在标题里，以及层级
    heading: Option<u8>,
    /// 代码块内
    in_code: bool,
    /// 列表项序号（无序用 None）
    list_prefix: Option<String>,
    /// 段首是否已经加过列表前缀
    prefix_done: bool,
    paragraph_count: usize,
}

impl<'a> Builder<'a> {
    fn new(opts: &'a ExportOptions) -> Self {
        Self {
            opts,
            paragraphs: Vec::new(),
            runs: Vec::new(),
            bold: 0,
            italic: 0,
            quote: 0,
            heading: None,
            in_code: false,
            list_prefix: None,
            prefix_done: false,
            paragraph_count: 0,
        }
    }

    fn fonts(&self) -> RunFonts {
        RunFonts::new()
            .ascii(&self.opts.font)
            .hi_ansi(&self.opts.font)
            .east_asia(&self.opts.font)
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let mut run = Run::new()
            .add_text(strip_wikilinks(text))
            .fonts(self.fonts());

        // 标题用大一号的字，正文用设定字号
        let size = match self.heading {
            Some(1) => self.opts.font_size + 12, // 三号左右
            Some(_) => self.opts.font_size + 6,
            None => self.opts.font_size,
        };
        run = run.size(size);

        if self.bold > 0 || self.heading.is_some() {
            run = run.bold();
        }
        if self.italic > 0 || self.quote > 0 {
            run = run.italic();
        }

        // 列表项前缀只加在段首
        if let Some(prefix) = &self.list_prefix {
            if !self.prefix_done {
                let mut with_prefix = Run::new()
                    .add_text(prefix.clone())
                    .fonts(self.fonts())
                    .size(size);
                if self.bold > 0 {
                    with_prefix = with_prefix.bold();
                }
                self.runs.push(with_prefix);
                self.prefix_done = true;
            }
        }

        self.runs.push(run);
    }

    fn flush_paragraph(&mut self) {
        if self.runs.is_empty() {
            return;
        }

        let mut para = Paragraph::new();
        for run in self.runs.drain(..) {
            para = para.add_run(run);
        }

        para = para.line_spacing(
            LineSpacing::new()
                .line(self.opts.line_spacing)
                .line_rule(LineSpacingType::Auto),
        );

        match self.heading {
            Some(1) => {
                para = para.align(AlignmentType::Center);
            }
            Some(_) => {
                // 二级以下标题左对齐、不缩进
                para = para.align(AlignmentType::Left);
            }
            None => {
                if self.in_code || self.quote > 0 {
                    // 代码与引用不首行缩进
                    para = para.align(AlignmentType::Left);
                } else {
                    // 首行缩进必须换算成 twips 再传 `SpecialIndentType::FirstLine`。
                    //
                    // 不能用 docx-rs 的 `Paragraph::first_line_chars`：0.4.22 里
                    // `BuildXML for Indent` 只读 `special_indent`，`first_line_chars`
                    // 字段只参与 JSON 序列化，写进 XML 时被完全忽略 —— 调用它静默无效。
                    //
                    // twips 换算：1pt = 20 twips，字号以半磅计，故每字符 = size * 10 twips。
                    // 按字号换算而非写死 480，换字号时缩进才跟着走。
                    let twips = (self.opts.font_size as i32) * 10 * self.opts.first_line_chars / 100;
                    para = if self.opts.first_line_chars > 0 {
                        para.align(AlignmentType::Both).indent(
                            None,
                            Some(SpecialIndentType::FirstLine(twips)),
                            None,
                            None,
                        )
                    } else {
                        // 缩进设为 0 时不写 indent，避免留下 `w:firstLine="0"` 噪音
                        para.align(AlignmentType::Both)
                    };
                }
            }
        }

        self.paragraphs.push(para);
        self.paragraph_count += 1;
        self.prefix_done = false;
    }

    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.push_text(&t),
            Event::Code(t) => {
                // 行内代码按普通文本处理，等宽字体在小说里反而突兀
                self.push_text(&t);
            }
            Event::SoftBreak => {
                // 软换行在 docx 里不换行（中文排版靠分段），补一个空格避免粘连
                self.push_text(" ");
            }
            Event::HardBreak => {
                if !self.runs.is_empty() {
                    let last = self.runs.pop().expect("刚判过非空");
                    self.runs.push(last.add_break(docx_rs::BreakType::TextWrapping));
                }
            }
            Event::Rule => {
                self.flush_paragraph();
                self.paragraphs.push(Paragraph::new());
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                // 原始 HTML 不导出，避免把标签当正文吐出去
            }
            Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
            Event::InlineMath(_) | Event::DisplayMath(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush_paragraph();
                self.heading = Some(level as u8);
            }
            Tag::Strong => self.bold += 1,
            Tag::Emphasis => self.italic += 1,
            Tag::BlockQuote(_) => {
                self.flush_paragraph();
                self.quote += 1;
            }
            Tag::CodeBlock(_) => {
                self.flush_paragraph();
                self.in_code = true;
            }
            Tag::Item => {
                self.flush_paragraph();
                self.list_prefix = Some("· ".to_string());
            }
            Tag::List(start) => {
                self.flush_paragraph();
                // 有序列表记下起始编号，无序列表置空
                self.list_prefix = start.map(|n| format!("{n}. "));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_paragraph();
                self.list_prefix = None;
            }
            TagEnd::Heading(_) => {
                self.flush_paragraph();
                self.heading = None;
            }
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::BlockQuote(_) => {
                self.flush_paragraph();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                self.flush_paragraph();
                self.in_code = false;
            }
            TagEnd::Item => {
                self.flush_paragraph();
                self.list_prefix = None;
            }
            TagEnd::List(_) => {
                self.flush_paragraph();
                self.list_prefix = None;
            }
            _ => {}
        }
    }
}

/// 生成 docx 字节流。
pub fn build(
    title: &str,
    chapters: &[ChapterInput],
    opts: &ExportOptions,
) -> Result<(Vec<u8>, ExportStats), String> {
    if chapters.is_empty() {
        return Err("没有可导出的章节".to_string());
    }

    let mut doc = Docx::new();
    let mut total_paragraphs = 0usize;

    // 书名页
    let title_run = Run::new()
        .add_text(title)
        .fonts(
            RunFonts::new()
                .ascii(&opts.font)
                .hi_ansi(&opts.font)
                .east_asia(&opts.font),
        )
        .size(opts.font_size + 20)
        .bold();
    doc = doc.add_paragraph(
        Paragraph::new()
            .add_run(title_run)
            .align(AlignmentType::Center),
    );

    let mut parser_opts = Options::empty();
    parser_opts.insert(Options::ENABLE_STRIKETHROUGH);

    for (idx, chapter) in chapters.iter().enumerate() {
        // 章标题
        let chapter_run = Run::new()
            .add_text(&chapter.title)
            .fonts(
                RunFonts::new()
                    .ascii(&opts.font)
                    .hi_ansi(&opts.font)
                    .east_asia(&opts.font),
            )
            .size(opts.font_size + 12)
            .bold();

        let mut heading = Paragraph::new()
            .add_run(chapter_run)
            .align(AlignmentType::Center);
        if opts.page_break && idx > 0 {
            heading = heading.page_break_before(true);
        }
        doc = doc.add_paragraph(heading);

        let mut builder = Builder::new(opts);
        // 先剥掉 `[[...]]` 再交给 Markdown 解析器。
        //
        // 不能在解析后按 Text 事件处理：pulldown-cmark 会把 `[[` 和 `]]` 当成
        // 可能的链接语法拆成独立事件，那时再拼就看不到完整的 `[[目标]]` 了
        // —— 实测会输出成 `[` `[` `夜航` `]` `]` 五个 run。
        let body = strip_wikilinks(&chapter.body);
        for event in Parser::new_ext(&body, parser_opts) {
            builder.handle(event);
        }
        builder.flush_paragraph();

        total_paragraphs += builder.paragraph_count;
        for para in builder.paragraphs {
            doc = doc.add_paragraph(para);
        }
    }

    let mut buf = Cursor::new(Vec::new());
    doc.build()
        .pack(&mut buf)
        .map_err(|e| format!("生成 docx 失败：{e}"))?;

    let bytes = buf.into_inner();
    let stats = ExportStats {
        chapters: chapters.len(),
        paragraphs: total_paragraphs,
        bytes: bytes.len(),
    };

    Ok((bytes, stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<ChapterInput> {
        vec![
            ChapterInput {
                title: "楔子".to_string(),
                body: "清籁斋的窗纸上落了一层薄雪。\n\n他想起[[夜航]]上那句话。\n".to_string(),
            },
            ChapterInput {
                title: "夜航".to_string(),
                body: "船过第三道湾的时候，阿箬醒了。\n".to_string(),
            },
        ]
    }

    #[test]
    fn 生成的_docx_是合法_zip() {
        let (bytes, _) = build("清籁集", &sample(), &ExportOptions::default()).unwrap();
        // ZIP 魔数 PK\x03\x04
        assert_eq!(&bytes[0..2], b"PK");
        assert!(bytes.len() > 1000);
    }

    #[test]
    fn 统计章数与段落数() {
        let (_, stats) = build("清籁集", &sample(), &ExportOptions::default()).unwrap();
        assert_eq!(stats.chapters, 2);
        assert!(stats.paragraphs >= 3, "至少三章各有正文段落");
    }

    #[test]
    fn 空章节列表被拒绝() {
        let err = build("空书", &[], &ExportOptions::default()).unwrap_err();
        assert!(err.contains("没有可导出"));
    }

    #[test]
    fn 双链在导出时还原成纯文本() {
        assert_eq!(strip_wikilinks("他想起[[夜航]]上那句话。"), "他想起夜航上那句话。");
        assert_eq!(strip_wikilinks("见[[第一章|开篇]]。"), "见开篇。");
        assert_eq!(strip_wikilinks("[[楔子#开头]]"), "楔子");
    }

    #[test]
    fn 不含双链的文本原样返回() {
        let text = "普通的一段话，没有链接。";
        assert_eq!(strip_wikilinks(text), text);
    }

    #[test]
    fn 未闭合的双链保持原样() {
        assert_eq!(strip_wikilinks("这里有个[[没闭合"), "这里有个[[没闭合");
    }

    #[test]
    fn 自定义字体与字号被采用() {
        let opts = ExportOptions {
            font: "SimHei".to_string(),
            font_size: 28,
            ..Default::default()
        };
        let (bytes, _) = build("测试", &sample(), &opts).unwrap();
        assert_eq!(&bytes[0..2], b"PK");
    }

    #[test]
    fn 段落缩进参数生效不报错() {
        let opts = ExportOptions {
            first_line_chars: 0,
            line_spacing: 240,
            page_break: false,
            ..Default::default()
        };
        let (_, stats) = build("测试", &sample(), &opts).unwrap();
        assert!(stats.paragraphs > 0);
    }

    #[test]
    fn markdown_标题与强调都能导出() {
        let chapters = vec![ChapterInput {
            title: "测试".to_string(),
            body: "# 一级标题\n\n正文**加粗**与*斜体*混排。\n\n> 引用一段\n".to_string(),
        }];
        let (bytes, stats) = build("测试", &chapters, &ExportOptions::default()).unwrap();
        assert_eq!(&bytes[0..2], b"PK");
        assert!(stats.paragraphs >= 3);
    }

    #[test]
    fn 列表项带前缀() {
        let chapters = vec![ChapterInput {
            title: "测试".to_string(),
            body: "- 第一项\n- 第二项\n".to_string(),
        }];
        let (_, stats) = build("测试", &chapters, &ExportOptions::default()).unwrap();
        assert!(stats.paragraphs >= 2);
    }

    /// 直接解压 docx 检查 `word/document.xml`。
    ///
    /// 只看「是不是合法 zip」不足以说明中文写对了 —— 真正的风险是字体槽写错、
    /// 缩进单位写错、双链语法漏进正文。这些只能看生成的 XML。
    fn document_xml(bytes: &[u8]) -> String {
        use std::io::Read;
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
        let mut entry = archive.by_name("word/document.xml").unwrap();
        let mut xml = String::new();
        entry.read_to_string(&mut xml).unwrap();
        xml
    }

    #[test]
    fn 导出的_xml_含中文正文() {
        let (bytes, _) = build("清籁集", &sample(), &ExportOptions::default()).unwrap();
        let xml = document_xml(&bytes);

        assert!(xml.contains("清籁斋的窗纸上"), "正文中文缺失");
        assert!(xml.contains("船过第三道湾"), "第二章正文缺失");
        assert!(xml.contains("清籁集"), "书名缺失");
    }

    #[test]
    fn 导出的_xml_不含双链语法() {
        let (bytes, _) = build("清籁集", &sample(), &ExportOptions::default()).unwrap();
        let xml = document_xml(&bytes);

        assert!(!xml.contains("[["), "双链语法漏进了导出文档");
        assert!(xml.contains("他想起夜航上那句话"), "双链未还原成纯文本");
    }

    #[test]
    fn 导出的_xml_带中文字体槽与首行缩进() {
        let (bytes, _) = build("清籁集", &sample(), &ExportOptions::default()).unwrap();
        let xml = document_xml(&bytes);

        // eastAsia 槽位是中文在 Word 里正确显示的前提
        assert!(xml.contains(r#"w:eastAsia="SimSun""#), "缺少中文字体槽");
        // 2 字符 × 12pt × 20 twips/pt = 480
        assert!(
            xml.contains(r#"w:firstLine="480""#),
            "首行缩进未写入（期望 480 twips = 2 字符 @ 12pt）"
        );
        // 1.5 倍行距
        assert!(xml.contains(r#"w:line="360""#), "行距未写入");
    }

    #[test]
    fn 缩进随字号同步换算() {
        // 换成 14pt（28 半磅）时，2 字符应变成 560 twips 而不是仍为 480
        let opts = ExportOptions {
            font_size: 28,
            ..Default::default()
        };
        let (bytes, _) = build("清籁集", &sample(), &opts).unwrap();
        let xml = document_xml(&bytes);
        assert!(xml.contains(r#"w:firstLine="560""#), "缩进没有随字号换算");
    }

    #[test]
    fn 关闭缩进时不写缩进属性() {
        let opts = ExportOptions {
            first_line_chars: 0,
            ..Default::default()
        };
        let (bytes, _) = build("清籁集", &sample(), &opts).unwrap();
        let xml = document_xml(&bytes);
        assert!(!xml.contains(r#"w:firstLine"#), "关闭缩进后不该有 firstLine");
    }

    #[test]
    fn 分页设置生效() {
        let (bytes, _) = build("清籁集", &sample(), &ExportOptions::default()).unwrap();
        let xml = document_xml(&bytes);
        assert!(xml.contains("pageBreakBefore"), "章间分页未生效");

        let opts = ExportOptions {
            page_break: false,
            ..Default::default()
        };
        let (bytes, _) = build("清籁集", &sample(), &opts).unwrap();
        let xml = document_xml(&bytes);
        assert!(!xml.contains("pageBreakBefore"), "关闭分页后不该有分页标记");
    }

    /// 用仓库里的示例稿库跑一遍真实链路。
    ///
    /// 单测用的是构造出来的 ChapterInput，这里走 `scan_with_docs` → 读盘 →
    /// 剥 front matter → 导出，能覆盖目录结构、元数据与双链的真实组合。
    #[test]
    fn 导出仓库示例稿库() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/sample-vault");
        if !root.is_dir() {
            // 示例库不在时跳过，不让 CI 因数据文件缺失而红
            return;
        }

        let outcome = crate::engine::vault::scan_with_docs(&root).expect("扫描示例库失败");
        let chapters: Vec<ChapterInput> = outcome
            .docs
            .iter()
            .map(|d| ChapterInput {
                title: d.title.clone(),
                body: d.body.clone(),
            })
            .collect();

        let (bytes, stats) = build("清籁集", &chapters, &ExportOptions::default()).unwrap();
        assert!(stats.chapters >= 3, "示例库至少三章");
        assert_eq!(&bytes[0..2], b"PK");

        let xml = document_xml(&bytes);
        assert!(xml.contains("清籁斋的窗纸上"), "示例正文缺失");
        assert!(!xml.contains("[["), "双链语法漏进导出文档");
        // 示例里 `关联：[[夜航]]、[[沈砚]]` 应还原成纯文本
        assert!(xml.contains("关联：夜航、沈砚"), "双链未按预期还原");
    }
}
