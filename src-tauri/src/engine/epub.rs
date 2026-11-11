//! EPUB 3.0 导出。
//!
//! 与 docx 导出平行，但产物是给电子书阅读器看的：一章一个 XHTML，
//! 目录带卷层级，正文按中文小说习惯排版（段首缩进两字符、1.8 行距）。
//!
//! ## 两个必须做对的地方
//!
//! 1. **`mimetype` 必须是 ZIP 的第一个条目，且不压缩（STORED）**。
//!    EPUB 阅读器靠「文件偏移 30 处是 `application/epub+zip`」来快速识别，
//!    压缩或排在后面都会让部分阅读器拒绝打开。
//! 2. **XHTML 必须 well-formed**。HTML 里合法的 `<br>`、未闭合 `<p>`
//!    在 XHTML 里是致命错误。这里不用 `pulldown-cmark` 的 HTML 输出，
//!    而是遍历 Event 自己生成标签，并用栈保证 Start/End 严格配对。
//!
//! 与 docx 一样，导出类功能的测试会**解压产物看真实内容**，
//! 而不是只断言「生成了一个合法 ZIP」。

use std::io::{Cursor, Write};

use chrono::{SecondsFormat, Utc};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::export::{strip_wikilinks, ExportStats};

/// 待导出的一章。比 docx 的 `ChapterInput` 多一个 `dir`，用于生成分卷目录。
pub struct EpubChapter {
    pub title: String,
    pub dir: String,
    pub body: String,
}

const MIMETYPE: &str = "application/epub+zip";

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn heading_tag(level: HeadingLevel) -> (&'static str, &'static str) {
    match level {
        HeadingLevel::H1 => ("<h1>", "</h1>"),
        HeadingLevel::H2 => ("<h2>", "</h2>"),
        HeadingLevel::H3 => ("<h3>", "</h3>"),
        HeadingLevel::H4 => ("<h4>", "</h4>"),
        HeadingLevel::H5 => ("<h5>", "</h5>"),
        HeadingLevel::H6 => ("<h6>", "</h6>"),
    }
}

/// Markdown → XHTML 片段。返回（片段，段落数）。
///
/// 用「闭合标签栈」而不是从 `TagEnd` 反推标签：pulldown-cmark 保证
/// Start/End 配对，栈就一定配对，输出必然 well-formed。
/// 遇到不打算支持的块（表格、图片）压入空占位，内部文字仍会输出。
fn markdown_to_xhtml(markdown: &str) -> (String, usize) {
    // 双链在电子书里只是噪音，还原成纯文本（与 docx 导出一致）
    let cleaned = strip_wikilinks(markdown);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(&cleaned, opts);

    let mut out = String::new();
    let mut closers: Vec<String> = Vec::new();
    let mut paragraphs = 0usize;

    for event in parser {
        match event {
            Event::Start(tag) => {
                let (open, close): (&str, String) = match tag {
                    Tag::Paragraph => {
                        paragraphs += 1;
                        ("<p>", "</p>".to_string())
                    }
                    Tag::Heading { level, .. } => {
                        let (o, c) = heading_tag(level);
                        (o, c.to_string())
                    }
                    Tag::Emphasis => ("<em>", "</em>".to_string()),
                    Tag::Strong => ("<strong>", "</strong>".to_string()),
                    Tag::Strikethrough => ("<s>", "</s>".to_string()),
                    Tag::BlockQuote(_) => ("<blockquote>", "</blockquote>".to_string()),
                    Tag::List(Some(_)) => ("<ol>", "</ol>".to_string()),
                    Tag::List(None) => ("<ul>", "</ul>".to_string()),
                    Tag::Item => ("<li>", "</li>".to_string()),
                    Tag::CodeBlock(_) => ("<pre><code>", "</code></pre>".to_string()),
                    Tag::Link { dest_url, .. } => {
                        let href = escape_xml(&dest_url);
                        // 用静态占位，open 部分单独拼
                        out.push_str(&format!("<a href=\"{href}\">"));
                        closers.push("</a>".to_string());
                        continue;
                    }
                    // 以下块不展开：内部文字照常输出，但外面不套标签，
                    // 保证 well-formed 且不会把表格结构吐成乱码标签
                    _ => ("", String::new()),
                };
                out.push_str(open);
                closers.push(close);
            }
            Event::End(_) => {
                if let Some(close) = closers.pop() {
                    out.push_str(&close);
                }
            }
            Event::Text(text) => out.push_str(&escape_xml(&text)),
            Event::Code(code) => {
                out.push_str("<code>");
                out.push_str(&escape_xml(&code));
                out.push_str("</code>");
            }
            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push_str("<br />"),
            Event::Rule => out.push_str("<hr />"),
            // 原始 HTML 不导出：既避免注入，也避免破坏 XHTML 结构
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
            Event::InlineMath(_) | Event::DisplayMath(_) => {}
        }
    }

    // 理论上栈已空；防御性补齐，绝不留下未闭合标签
    while let Some(close) = closers.pop() {
        out.push_str(&close);
    }

    (out, paragraphs)
}

fn chapter_xhtml(title: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"zh-CN\" lang=\"zh-CN\">\n\
<head>\n\
<meta charset=\"utf-8\" />\n\
<title>{title}</title>\n\
<link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\" />\n\
</head>\n\
<body>\n\
<h1 class=\"chapter-title\">{title}</h1>\n\
{body}\n\
</body>\n\
</html>\n",
        title = escape_xml(title),
        body = body,
    )
}

fn container_xml() -> &'static str {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n\
  <rootfiles>\n\
    <rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\" />\n\
  </rootfiles>\n\
</container>\n"
}

fn style_css() -> &'static str {
    "/* 中文小说通行排版：段首缩进两字符，段间不空行 */\n\
body { font-family: serif; line-height: 1.8; margin: 1em 1.2em; }\n\
p { text-indent: 2em; margin: 0; }\n\
h1.chapter-title { text-align: center; font-size: 1.4em; font-weight: normal;\n\
  text-indent: 0; margin: 2em 0 1.5em; }\n\
blockquote { margin: 0.5em 2em; }\n\
pre { white-space: pre-wrap; }\n"
}

/// 生成目录。章节已按路径排序，同一 `dir` 的章必然连续，直接按连续段分组。
fn nav_xhtml(chapters: &[EpubChapter]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" \
xml:lang=\"zh-CN\" lang=\"zh-CN\">\n\
<head><meta charset=\"utf-8\" /><title>目录</title>\n\
<link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\" /></head>\n\
<body>\n\
<nav epub:type=\"toc\" id=\"toc\">\n\
<h1>目录</h1>\n<ol>\n",
    );

    let mut i = 0usize;
    while i < chapters.len() {
        let dir = &chapters[i].dir;
        let href = |index: usize| format!("chapter-{:04}.xhtml", index + 1);

        if dir.is_empty() {
            // 根目录下的章直接列
            out.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>\n",
                href(i),
                escape_xml(&chapters[i].title)
            ));
            i += 1;
            continue;
        }

        out.push_str(&format!(
            "<li><span>{}</span>\n<ol>\n",
            escape_xml(dir)
        ));
        while i < chapters.len() && chapters[i].dir == *dir {
            out.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>\n",
                href(i),
                escape_xml(&chapters[i].title)
            ));
            i += 1;
        }
        out.push_str("</ol>\n</li>\n");
    }

    out.push_str("</ol>\n</nav>\n</body>\n</html>\n");
    out
}

fn content_opf(book_title: &str, chapter_count: usize, identifier: &str) -> String {
    let modified = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\">\n\
  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
    <dc:identifier id=\"bookid\">{identifier}</dc:identifier>\n\
    <dc:title>{title}</dc:title>\n\
    <dc:language>zh-CN</dc:language>\n\
    <meta property=\"dcterms:modified\">{modified}</meta>\n\
  </metadata>\n\
  <manifest>\n\
    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\" />\n\
    <item id=\"css\" href=\"style.css\" media-type=\"text/css\" />\n",
        identifier = escape_xml(identifier),
        title = escape_xml(book_title),
        modified = modified,
    );

    for i in 0..chapter_count {
        out.push_str(&format!(
            "    <item id=\"ch{0}\" href=\"chapter-{1:04}.xhtml\" media-type=\"application/xhtml+xml\" />\n",
            i + 1,
            i + 1
        ));
    }

    out.push_str("  </manifest>\n  <spine>\n");
    for i in 0..chapter_count {
        out.push_str(&format!("    <itemref idref=\"ch{}\" />\n", i + 1));
    }
    out.push_str("  </spine>\n</package>\n");
    out
}

/// 生成 EPUB 字节流。
pub fn build(book_title: &str, chapters: &[EpubChapter]) -> Result<(Vec<u8>, ExportStats), String> {
    if chapters.is_empty() {
        return Err("没有可导出的章节".to_string());
    }

    // dc:identifier 只要求是一个唯一 URI，不必是 UUID；ULID 已足够且无需引入新依赖
    let identifier = format!("urn:qinglai:{}", ulid::Ulid::new());

    let mut files: Vec<(String, String)> = Vec::with_capacity(chapters.len() + 4);
    files.push(("META-INF/container.xml".to_string(), container_xml().to_string()));
    files.push((
        "OEBPS/content.opf".to_string(),
        content_opf(book_title, chapters.len(), &identifier),
    ));
    files.push(("OEBPS/nav.xhtml".to_string(), nav_xhtml(chapters)));
    files.push(("OEBPS/style.css".to_string(), style_css().to_string()));

    let mut total_paragraphs = 0usize;
    for (i, chapter) in chapters.iter().enumerate() {
        let (body, paragraphs) = markdown_to_xhtml(&chapter.body);
        total_paragraphs += paragraphs;
        files.push((
            format!("OEBPS/chapter-{:04}.xhtml", i + 1),
            chapter_xhtml(&chapter.title, &body),
        ));
    }

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);

    // 必须第一个写入，且 STORED（不压缩）
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("mimetype", stored)
        .map_err(|e| format!("写入 mimetype 失败：{e}"))?;
    zip.write_all(MIMETYPE.as_bytes())
        .map_err(|e| format!("写入 mimetype 内容失败：{e}"))?;

    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, content) in &files {
        zip.start_file(name.as_str(), deflated.clone())
            .map_err(|e| format!("写入 {name} 失败：{e}"))?;
        zip.write_all(content.as_bytes())
            .map_err(|e| format!("写入 {name} 内容失败：{e}"))?;
    }

    let bytes = zip
        .finish()
        .map_err(|e| format!("完成 EPUB 打包失败：{e}"))?
        .into_inner();

    Ok((
        bytes.clone(),
        ExportStats {
            chapters: chapters.len(),
            paragraphs: total_paragraphs,
            bytes: bytes.len(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use zip::ZipArchive;

    fn sample() -> Vec<EpubChapter> {
        vec![
            EpubChapter {
                title: "楔子".into(),
                dir: "小说/清籁集/卷一·听风".into(),
                body: "窗纸上落了一层薄雪。\n\n他想起 [[夜航]] 里的旧话。".into(),
            },
            EpubChapter {
                title: "夜航".into(),
                dir: "小说/清籁集/卷一·听风".into(),
                body: "船过第三道湾的时候，阿箬醒了。".into(),
            },
            EpubChapter {
                title: "沈砚".into(),
                dir: "设定/人物".into(),
                body: "话少，惯用 **旧砚**。".into(),
            },
        ]
    }

    fn read_entry(bytes: &[u8], name: &str) -> (String, CompressionMethod) {
        let mut archive = ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
        let mut file = archive.by_name(name).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        (text, file.compression())
    }

    #[test]
    fn mimetype_是首条目且不压缩() {
        let (bytes, _) = build("测试书", &sample()).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();

        let mut text = String::new();
        {
            let first = archive.by_index(0).unwrap();
            assert_eq!(first.name(), "mimetype");
            assert_eq!(first.compression(), CompressionMethod::Stored);
        }
        let mut f = archive.by_name("mimetype").unwrap();
        f.read_to_string(&mut text).unwrap();
        assert_eq!(text, "application/epub+zip");
    }

    #[test]
    fn 容器指向_opf() {
        let (bytes, _) = build("测试书", &sample()).unwrap();
        let (text, _) = read_entry(&bytes, "META-INF/container.xml");
        assert!(text.contains("full-path=\"OEBPS/content.opf\""));
        assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    }

    #[test]
    fn opf_的_manifest_与_spine_数量一致() {
        let (bytes, _) = build("测试书", &sample()).unwrap();
        let (text, _) = read_entry(&bytes, "OEBPS/content.opf");

        for i in 1..=3 {
            assert!(text.contains(&format!("chapter-{i:04}.xhtml")));
            assert!(text.contains(&format!("<itemref idref=\"ch{i}\" />")));
        }
        assert!(text.contains("<dc:title>测试书</dc:title>"));
        assert!(text.contains("<dc:language>zh-CN</dc:language>"));
        assert!(text.contains("properties=\"nav\""));
        assert!(text.contains("urn:qinglai:"));
    }

    #[test]
    fn 章节内容与双链还原() {
        let (bytes, _) = build("测试书", &sample()).unwrap();
        let (ch1, _) = read_entry(&bytes, "OEBPS/chapter-0001.xhtml");

        assert!(ch1.contains("窗纸上落了一层薄雪。"));
        assert!(ch1.contains("他想起 夜航 里的旧话。"), "双链应还原成纯文本");
        assert!(!ch1.contains("[["), "不应残留双链语法");
        assert!(ch1.contains("<h1 class=\"chapter-title\">楔子</h1>"));
    }

    #[test]
    fn markdown_标记被转成_xhtml() {
        let (bytes, _) = build("测试书", &sample()).unwrap();
        let (ch3, _) = read_entry(&bytes, "OEBPS/chapter-0003.xhtml");
        assert!(ch3.contains("<strong>旧砚</strong>"));
    }

    #[test]
    fn xhtml_不含未转义的和号() {
        let chapters = vec![EpubChapter {
            title: "A & B <C>".into(),
            dir: "".into(),
            body: "甲 & 乙 <丙> \"丁\"。\n\n链接 [外链](https://a.test/?x=1&y=2)。".into(),
        }];
        let (bytes, _) = build("测试书", &chapters).unwrap();
        let (ch, _) = read_entry(&bytes, "OEBPS/chapter-0001.xhtml");

        assert!(ch.contains("A &amp; B &lt;C&gt;"));
        assert!(ch.contains("甲 &amp; 乙 &lt;丙&gt; &quot;丁&quot;"));
        assert!(ch.contains("x=1&amp;y=2"), "链接属性也要转义");
        assert!(!ch.contains(" & "));
    }

    #[test]
    fn 目录按卷分组且根目录章直接列出() {
        let (bytes, _) = build("测试书", &sample()).unwrap();
        let (nav, _) = read_entry(&bytes, "OEBPS/nav.xhtml");

        assert!(nav.contains("<span>小说/清籁集/卷一·听风</span>"));
        assert!(nav.contains("<span>设定/人物</span>"));
        assert!(nav.contains("<a href=\"chapter-0001.xhtml\">楔子</a>"));
        assert!(nav.contains("<a href=\"chapter-0003.xhtml\">沈砚</a>"));
        assert_eq!(nav.matches("<a href=").count(), 3);
    }

    #[test]
    fn 空章节列表报错() {
        assert!(build("空", &[]).is_err());
    }

    #[test]
    fn 统计数与章节数一致() {
        let (_, stats) = build("测试书", &sample()).unwrap();
        assert_eq!(stats.chapters, 3);
        assert!(stats.paragraphs >= 4);
        assert!(stats.bytes > 0);
    }

    #[test]
    fn 需要的条目都在包里() {
        let (bytes, _) = build("测试书", &sample()).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();

        for expected in [
            "mimetype",
            "META-INF/container.xml",
            "OEBPS/content.opf",
            "OEBPS/nav.xhtml",
            "OEBPS/style.css",
            "OEBPS/chapter-0001.xhtml",
            "OEBPS/chapter-0002.xhtml",
            "OEBPS/chapter-0003.xhtml",
        ] {
            assert!(names.contains(&expected.to_string()), "缺少 {expected}");
        }
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn 扫描示例稿库能生成_epub() {
        let Some(root) = crate::engine::test_support::sample_vault() else {
            return;
        };
        let outcome = crate::engine::vault::scan_with_docs(&root).unwrap();
        let chapters: Vec<EpubChapter> = outcome
            .docs
            .iter()
            .map(|d| EpubChapter {
                title: d.title.clone(),
                dir: d.dir.clone(),
                body: d.body.clone(),
            })
            .collect();

        let (bytes, stats) = build("清籁斋示例库", &chapters).unwrap();
        assert_eq!(stats.chapters, 3);
        let (ch1, _) = read_entry(&bytes, "OEBPS/chapter-0001.xhtml");
        assert!(ch1.contains("清籁斋的窗纸上落了一层薄雪"));
    }
}
