//! 內容解析、導覽樹、搜尋

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub title: String,
    pub filename: String,
    pub level: usize,
    pub body: String,
    pub index: usize,
}

#[derive(Debug, Clone)]
pub struct NavNode {
    pub page_index: usize,
    pub children: Vec<NavNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchPage {
    pub title: String,
    pub text: String,
    pub tags: String,
    pub url: String,
}

pub fn strip_tags(input: &str) -> Result<String, Box<dyn Error>> {
    let re = Regex::new(r"(?s)<[^>]*>")?;
    let text = re.replace_all(input, "");
    Ok(text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'"))
}

pub fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn slug_filename(title: &str) -> String {
    let invalid = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let result: String = title
        .trim()
        .chars()
        .map(|c| {
            if invalid.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let result = result.trim_end_matches([' ', '.']);
    if result.is_empty() {
        "untitled".to_string()
    } else {
        result.to_string()
    }
}

pub fn file_get_contents(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(path)?)
}

/// 解析 content.htm；重複標題：第一個原名，其後 -1, -2...
pub fn parse_pages(input: &str) -> Result<Vec<Page>, Box<dyn Error>> {
    let heading_re = Regex::new(r"(?is)<h([1-3])\b[^>]*>.*?</h[1-3]\s*>")?;
    let title_re = Regex::new(r"(?is)^<h([1-3])\b[^>]*>(.*?)</h[1-3]\s*>$")?;

    let mut headings: Vec<(usize, usize, usize, String)> = Vec::new();
    for found in heading_re.find_iter(input) {
        let tag = found.as_str();
        let Some(captures) = title_re.captures(tag) else {
            continue;
        };
        let level: usize = captures[1].parse()?;
        let title = strip_tags(&captures[2])?.trim().to_string();
        if title.is_empty() {
            continue;
        }
        headings.push((found.start(), found.end(), level, title));
    }
    if headings.is_empty() {
        return Err("content.htm 中找不到 h1、h2 或 h3 標題".into());
    }

    let mut total_counts: HashMap<String, usize> = HashMap::new();
    for h in &headings {
        *total_counts.entry(h.3.clone()).or_insert(0) += 1;
    }

    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut filename_counts: HashMap<String, usize> = HashMap::new();
    let mut pages = Vec::new();

    for i in 0..headings.len() {
        let (_start, end, level, title) = &headings[i];
        let body_end = headings.get(i + 1).map(|n| n.0).unwrap_or(input.len());
        let body = input[*end..body_end].to_string();

        let count = seen.entry(title.clone()).or_insert(0);
        *count += 1;
        let unique_title = if total_counts.get(title).copied().unwrap_or(0) > 1 {
            if *count == 1 {
                title.clone()
            } else {
                format!("{}-{}", title, *count - 1)
            }
        } else {
            title.clone()
        };

        let base_name = slug_filename(&unique_title);
        let key = base_name.to_lowercase();
        let fc = filename_counts.entry(key).or_insert(0);
        *fc += 1;
        let filename = if *fc == 1 {
            format!("{}.html", base_name)
        } else {
            format!("{}-{}.html", base_name, fc)
        };

        pages.push(Page {
            title: unique_title,
            filename,
            level: *level,
            body,
            index: i,
        });
    }
    Ok(pages)
}

pub fn load_pages(content_path: &Path) -> Result<Vec<Page>, Box<dyn Error>> {
    parse_pages(&file_get_contents(content_path)?)
}

/// 修復 <pre> / 程式碼區塊內的換行，讓 ViewPage 的 SyntaxHighlighter 能正確分行。
///
/// TinyMCE 與舊版 Rust 存檔常把 pre 內的換行變成 `<br>`、`&#10;` 或 `\r`。
/// SyntaxHighlighter 需要的是真正的 LF 換行字元；若只剩空白或 br 標籤，
/// 整段程式碼會被畫成一行。此函式只改 pre（與 pre 內的 code）區塊，
/// 不碰一般段落的 `<br>`，也不改已跳脫的 `&lt;br&gt;` 文字。
pub fn normalize_preformatted_html(input: &str) -> String {
    // 非貪婪匹配整段 <pre>...</pre>（含屬性、大小寫、跨行）
    let pre_re = Regex::new(r"(?is)(<pre\b[^>]*>)(.*?)(</pre\s*>)")
        .expect("valid pre block regex");
    // <br>、<br/>、<br />、<BR class="...">，後面可接一組既有換行
    let br_re = Regex::new(r"(?i)<br\s*/?\s*(?:\s[^>]*)?>\s*(?:\r\n|\n|\r)?")
        .expect("valid br regex");
    // HTML 換行實體（TinyMCE entity_encoding 有時會產生）
    let entity_nl_re = Regex::new(r"(?i)(?:&#10;|&#x0a;|&#13;|&#x0d;|&NewLine;)")
        .expect("valid newline entity regex");

    pre_re
        .replace_all(input, |caps: &regex::Captures<'_>| {
            let open = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            let inner = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
            let close = caps.get(3).map(|m| m.as_str()).unwrap_or_default();

            // 1) <br> → 真正換行（不要留下多餘空白行以外的殘渣）
            let inner = br_re.replace_all(inner, "\n");
            // 2) 換行實體 → LF
            let inner = entity_nl_re.replace_all(&inner, "\n");
            // 3) 統一 CRLF / CR → LF
            let inner = inner.replace("\r\n", "\n").replace('\r', "\n");
            // 4) 不要把 &nbsp; 留在程式碼縮排位置（改成一般空白，避免 SH 把行黏在一起）
            let inner = inner.replace('\u{00a0}', " ").replace("&nbsp;", " ");

            format!("{}{}{}", open, inner, close)
        })
        .into_owned()
}

/// 移除 pre 上 TinyMCE 留下的 contenteditable，避免 ViewPage 行為異常
/// （regex crate 不支援 backreference，故不使用 \2）
pub fn strip_pre_contenteditable(input: &str) -> String {
    let re_dq = Regex::new(r#"(?i)\s*contenteditable\s*=\s*"[^"]*""#)
        .expect("valid contenteditable dq regex");
    let re_sq = Regex::new(r#"(?i)\s*contenteditable\s*=\s*'[^']*'"#)
        .expect("valid contenteditable sq regex");
    let re_uq = Regex::new(r"(?i)\s*contenteditable\s*=\s*[^\s>]+")
        .expect("valid contenteditable uq regex");
    let pre_open = Regex::new(r"(?is)<pre\b[^>]*>").expect("valid pre open regex");
    pre_open
        .replace_all(input, |caps: &regex::Captures<'_>| {
            let tag = caps.get(0).map(|m| m.as_str()).unwrap_or_default();
            let tag = re_dq.replace_all(tag, "");
            let tag = re_sq.replace_all(&tag, "");
            re_uq.replace_all(&tag, "").into_owned()
        })
        .into_owned()
}

/// 建立巢狀導覽樹（有效 HTML 結構，供左拉選單 CSS）
pub fn build_navigation_tree(pages: &[Page]) -> Vec<NavNode> {
    fn insert(nodes: &mut Vec<NavNode>, page_index: usize, depth: usize) {
        if depth <= 1 || nodes.is_empty() {
            nodes.push(NavNode {
                page_index,
                children: Vec::new(),
            });
            return;
        }
        if let Some(last) = nodes.last_mut() {
            insert(&mut last.children, page_index, depth - 1);
        }
    }
    let mut roots = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        insert(&mut roots, index, page.level.clamp(1, 3));
    }
    roots
}

/// 左側 css3menu1：正確巢狀 ul/li，連結一律 /get_page/標題
pub fn render_side_menu(pages: &[Page]) -> String {
    fn render_nodes(nodes: &[NavNode], pages: &[Page], out: &mut String) {
        for node in nodes {
            let page = &pages[node.page_index];
            let href = format!("/get_page/{}", encode_url(&page.title));
            out.push_str("<li>");
            out.push_str(&format!(
                "<a href=\"{}\">{}</a>",
                href,
                escape_html(&page.title)
            ));
            if !node.children.is_empty() {
                out.push_str("<ul>");
                render_nodes(&node.children, pages, out);
                out.push_str("</ul>");
            }
            out.push_str("</li>");
        }
    }
    let tree = build_navigation_tree(pages);
    let mut html = String::from("<ul id=\"css3menu1\" class=\"topmenu\">");
    render_nodes(&tree, pages, &mut html);
    html.push_str("</ul>");
    html
}

pub fn search_content(pages: &[Page], keyword: &str) -> Vec<(String, String)> {
    let kw = keyword.trim().to_lowercase();
    if kw.is_empty() {
        return vec![];
    }
    let mut results = Vec::new();
    for page in pages {
        let title_l = page.title.to_lowercase();
        let body_plain = strip_tags(&page.body).unwrap_or_default().to_lowercase();
        if title_l.contains(&kw) || body_plain.contains(&kw) {
            let snippet: String = body_plain.chars().take(160).collect();
            let snippet = if body_plain.chars().count() > 160 {
                format!("{}...", snippet)
            } else {
                snippet
            };
            results.push((page.title.clone(), snippet));
        }
    }
    results
}

pub fn list_files(dir: &Path, extensions: Option<&[&str]>) -> Result<Vec<String>, Box<dyn Error>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if let Some(exts) = extensions {
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if exts.iter().any(|e| *e == ext) {
                    files.push(name);
                }
            }
        } else {
            files.push(name);
        }
    }
    files.sort();
    Ok(files)
}

/// 中文保留在 URL 中（與 Python 版相同）
pub fn encode_url(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            '%' => out.push_str("%25"),
            '"' => out.push_str("%22"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '\\' => out.push_str("%5C"),
            _ => out.push(c),
        }
    }
    out
}

pub fn decode_url(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let raw = s.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let Ok(hex) = std::str::from_utf8(&raw[i + 1..i + 3]) {
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    bytes.push(v);
                    i += 3;
                    continue;
                }
            }
        } else if raw[i] == b'+' {
            bytes.push(b' ');
            i += 1;
            continue;
        }
        bytes.push(raw[i]);
        i += 1;
    }
    String::from_utf8(bytes).unwrap_or_else(|_| s.to_string())
}


#[cfg(test)]
mod preformatted_tests {
    use super::normalize_preformatted_html;

    #[test]
    fn converts_legacy_br_inside_pre_to_real_newlines() {
        let input = r#"<p>outside<br>still outside</p><pre class="brush: rust">fn main() {<br />
    println!("hello");<BR>
 }</pre>"#;
        let output = normalize_preformatted_html(input);
        assert_eq!(
            output,
            "<p>outside<br>still outside</p><pre class=\"brush: rust\">fn main() {\n    println!(\"hello\");\n }</pre>"
        );
    }

    #[test]
    fn keeps_escaped_br_text_and_pre_whitespace() {
        let input = "<pre>one\r\ntwo\rthree &lt;br&gt;  </pre>";
        let output = normalize_preformatted_html(input);
        assert_eq!(output, "<pre>one\ntwo\nthree &lt;br&gt;  </pre>");
    }

    #[test]
    fn leaves_content_without_pre_unchanged() {
        let input = "<p>one<br>two</p>";
        assert_eq!(normalize_preformatted_html(input), input);
    }

    #[test]
    fn converts_newline_entities_inside_pre() {
        let input = "<pre class=\"brush: python\">a&#10;b&#13;c</pre>";
        let output = normalize_preformatted_html(input);
        assert_eq!(output, "<pre class=\"brush: python\">a\nb\nc</pre>");
    }

    #[test]
    fn converts_nbsp_inside_pre_to_space() {
        let input = "<pre>a&nbsp;&nbsp;b</pre>";
        let output = normalize_preformatted_html(input);
        assert_eq!(output, "<pre>a  b</pre>");
    }
}
