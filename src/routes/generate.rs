use axum::{
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect},
};
use std::error::Error;
use std::fs;

use crate::auth::{self, is_admin};
use crate::content::{
    escape_html, load_pages, normalize_preformatted_html, strip_tags, NavNode, Page, SearchPage,
};
use crate::templates::render_public_page;
use crate::AppState;

fn fix_static_paths(html: &str) -> String {
    html.replace("src=\"/images/", "src=\"./../images/")
        .replace("href=\"/images/", "href=\"./../images/")
        .replace("data=\"/images/", "data=\"./../images/")
        .replace("src=\"/downloads/", "src=\"./../downloads/")
        .replace("href=\"/downloads/", "href=\"./../downloads/")
        .replace("src=\"/static/", "src=\"./../static/")
        .replace("href=\"/static/", "href=\"./../static/")
        .replace("src=/downloads", "src=./../downloads")
        .replace("pythonpath:['/static/'", "pythonpath:['./../static/'")
        .replace("data-dirname=\"/static\"", "data-dirname=\"./../static\"")
}

fn html_header(title: &str, site_title: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="zh-Hant">
<head>
<meta charset="utf-8">
<meta property="head" content="H1">
<meta name="viewport" content="width=device-width, initial-scale=1, shrink-to-fit=no">
<title>{title}</title>

<link rel="stylesheet" href="./../static/chimper/fonts/icomoon/style.css">
<link rel="stylesheet" href="./../static/chimper/css/bootstrap.min.css">
<link rel="stylesheet" href="./../static/chimper/css/magnific-popup.css">
<link rel="stylesheet" href="./../static/chimper/css/jquery-ui.css">
<link rel="stylesheet" href="./../static/chimper/css/owl.carousel.min.css">
<link rel="stylesheet" href="./../static/chimper/css/owl.theme.default.min.css">
<link rel="stylesheet" href="./../static/chimper/css/bootstrap-datepicker.css">
<link rel="stylesheet" href="./../static/chimper/fonts/flaticon/font/flaticon.css">
<link rel="stylesheet" href="./../static/chimper/css/aos.css">
<link rel="stylesheet" href="./../static/chimper/css/style.css">
<link rel="shortcut icon" href="./../static/favicons.png">

<style type="text/css">
.site-section {{ background-color: #FFFF; padding: 40px 40px; }}
body > div > div.dropdown.open {{ display: block; }}
img.add_border {{ border: 3px solid blue; }}

.page-navigation {{
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    box-sizing: border-box;
    gap: 20px;
    margin: 24px 0;
    padding: 12px 0;
    border-top: 1px solid #ddd;
    border-bottom: 1px solid #ddd;
}}

.page-navigation .previous {{
    flex: 1 1 50%;
    min-width: 0;
    margin-right: auto;
    text-align: left;
}}

.page-navigation .next {{
    flex: 1 1 50%;
    min-width: 0;
    margin-left: auto;
    text-align: right;
}}

.page-navigation a {{
    display: inline-block;
    text-decoration: none;
    overflow-wrap: anywhere;
}}

.page-navigation a:hover {{
    text-decoration: underline;
}}
</style>

<script src="./../static/chimper/js/jquery-3.3.1.min.js"></script>
<link rel="stylesheet" href="./../static/tipuesearch/css/normalize.min.css">
<script src="./../static/tipuesearch/tipuesearch_set.js"></script>
<script src="tipuesearch_content.js"></script>
<link rel="stylesheet" href="./../static/tipuesearch/css/tipuesearch.css">
<script src="./../static/tipuesearch/tipuesearch.js"></script>

<script>
function doSearch() {{
    if (window.jQuery && jQuery.fn.tipuesearch) {{
        jQuery('#tipue_search_input').tipuesearch({{
            newWindow: true,
            minimumLength: 2,
            wholeWords: false
        }});
    }}
}}

if (window.jQuery) {{
    jQuery(document).ready(doSearch);
}}
</script>

<script type="text/javascript" src="./../static/syntaxhighlighter/shCore.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushBash.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushDiff.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushJScript.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushJava.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushPython.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushSql.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushHaxe.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushXml.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushPhp.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushPowerShell.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushLua.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushMojo.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushWbt.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushCpp.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushCss.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushCSharp.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushDart.js"></script>
<script type="text/javascript" src="./../static/syntaxhighlighter/shBrushRust.js"></script>
<link type="text/css" rel="stylesheet" href="./../static/syntaxhighlighter/css/shCoreDefault.css">
<style>
pre, pre code, .syntaxhighlighter .line, .syntaxhighlighter table td.code .line {{
  white-space: pre !important; word-wrap: normal !important; word-break: normal !important;
}}
</style>
</head>

<body>
<div class="container">
<div class="site-wrap">

<div class="site-mobile-menu">
  <div class="site-mobile-menu-header">
    <div class="site-mobile-menu-close mt-3">
      <span class="icon-close2 js-menu-toggle"></span>
    </div>
  </div>
  <div class="site-mobile-menu-body"></div>
</div>

<header class="site-navbar py-4 bg-white" role="banner">
  <div class="container">
    <div class="row align-items-center">
      <h1>{site_title}</h1>

      <div class="pl-4">
        <form action="#" onsubmit="return false;">
          <input type="text"
                 placeholder="Search"
                 name="q"
                 id="tipue_search_input"
                 pattern=".{{2,}}"
                 title="At least 2 characters">
        </form>
      </div>

      <div class="col-12 col-md-10 d-none d-xl-block">
        <nav class="site-navigation position-relative text-right" role="navigation">
          <!-- NAVIGATION_MENU_INSERTED_BY_CONVERTER -->
        </nav>
      </div>

      <div class="d-inline-block d-xl-none ml-md-0 mr-auto py-3"
           style="position: relative; top: 3px;">
        <a href="#" class="site-menu-toggle js-menu-toggle text-black">
          <span class="icon-menu h3"></span>
        </a>
      </div>
    </div>
  </div>
</header>

<div id="tipue_search_content">
"##,
        title = escape_html(title)
    )
}
fn html_footer() -> &'static str {
    r#"
</div><!-- /tipue_search_content -->

<div class="container">
  <div class="row pt-3 mx-auto">
    <p>
      Copyright &copy;<script>document.write(new Date().getFullYear());</script>
      All rights reserved |
      <i class="icon-heart" aria-hidden="true"></i>
      by <a href="https://colorlib.com" target="_blank">Colorlib</a>
    </p>
  </div>
</div>

</div><!-- /site-wrap -->

<script src="./../static/chimper/js/jquery-migrate-3.0.1.min.js"></script>
<script src="./../static/chimper/js/jquery-ui.js"></script>
<script src="./../static/chimper/js/popper.min.js"></script>
<script src="./../static/chimper/js/bootstrap.min.js"></script>
<script src="./../static/chimper/js/owl.carousel.min.js"></script>
<script src="./../static/chimper/js/jquery.stellar.min.js"></script>
<script src="./../static/chimper/js/jquery.countdown.min.js"></script>
<script src="./../static/chimper/js/jquery.magnific-popup.min.js"></script>
<script src="./../static/chimper/js/bootstrap-datepicker.min.js"></script>
<script src="./../static/chimper/js/aos.js"></script>
<script src="./../static/chimper/js/main.js"></script>
</body>
</html>
"#
}

fn insert_nav_node(nodes: &mut Vec<NavNode>, page_index: usize, depth: usize) {
    if depth <= 1 || nodes.is_empty() {
        nodes.push(NavNode {
            page_index,
            children: Vec::new(),
        });
        return;
    }

    if let Some(last_node) = nodes.last_mut() {
        insert_nav_node(&mut last_node.children, page_index, depth - 1);
    }
}

fn build_navigation_tree(pages: &[Page]) -> Vec<NavNode> {
    let mut roots = Vec::new();

    for (index, page) in pages.iter().enumerate() {
        insert_nav_node(&mut roots, index, page.level.clamp(1, 3));
    }

    roots
}

fn render_nav_nodes(nodes: &[NavNode], pages: &[Page], output: &mut String) {
    for node in nodes {
        let page = &pages[node.page_index];

        if node.children.is_empty() {
            output.push_str("<li>");
        } else {
            output.push_str("<li class=\"has-children\">");
        }

        output.push_str(&format!(
            "<a href=\"{}\">{}</a>",
            escape_html(&page.filename),
            escape_html(&page.title)
        ));

        if !node.children.is_empty() {
            output.push_str("<ul class=\"dropdown\">\n");

            render_nav_nodes(&node.children, pages, output);

            output.push_str("</ul>\n");
        }

        output.push_str("</li>\n");
    }
}

fn navigation_links(pages: &[Page]) -> String {
    let mut output = String::from(
        r#"<ul class="site-menu js-clone-nav mr-auto d-none d-lg-block">
<li class="active has-children">
<a href="index.html">Home</a>
<ul class="dropdown">
<li><a href="sitemap.html">SMap</a></li>
</ul>
</li>
"#,
    );

    let tree = build_navigation_tree(pages);
    render_nav_nodes(&tree, pages, &mut output);
    output.push_str("</ul>\n");
    output
}
fn render_previous_next(index: usize, pages: &[Page]) -> String {
    let mut output =
        String::from("<nav class=\"page-navigation\" aria-label=\"Page navigation\">\n");

    if index > 0 {
        let previous = &pages[index - 1];

        output.push_str(&format!(
            "<div class=\"previous\">\
             &lt;&lt; <a href=\"{}\">Previous: {}</a>\
             </div>\n",
            escape_html(&previous.filename),
            escape_html(&previous.title)
        ));
    } else {
        output.push_str("<div class=\"previous\"></div>\n");
    }

    if index + 1 < pages.len() {
        let next = &pages[index + 1];

        output.push_str(&format!(
            "<div class=\"next\">\
             <a href=\"{}\">Next: {}</a> &gt;&gt;\
             </div>\n",
            escape_html(&next.filename),
            escape_html(&next.title)
        ));
    } else {
        output.push_str("<div class=\"next\"></div>\n");
    }

    output.push_str("</nav>\n");

    output
}

fn render_page(index: usize, pages: &[Page], site_title: &str) -> String {
    let page = &pages[index];

    let mut output = html_header(&page.title, site_title);

    output = output.replace(
        "<!-- NAVIGATION_MENU_INSERTED_BY_CONVERTER -->",
        &navigation_links(pages),
    );

    output.push_str(&format!("<h1>{}</h1>\n", escape_html(&page.title)));

    output.push_str(&render_previous_next(index, pages));
    // 靜態頁同樣還原 pre 內 <br>/CR，否則 SyntaxHighlighter 會整段變一行
    let body = normalize_preformatted_html(&page.body);
    output.push_str(&fix_static_paths(&body));
    output.push_str(&render_previous_next(index, pages));
    // 對齊動態頁：bloggerMode + 先把 <br> 還原再 all()
    output.push_str(
        r#"<script>
(function(){
  function restorePreNewlines(){
    var nodes=document.querySelectorAll('pre');
    for(var i=0;i<nodes.length;i++){
      var pre=nodes[i], html=pre.innerHTML;
      if(!html) continue;
      var fixed=html.replace(/<br\s*\/?\s*(?:\s[^>]*)?>/gi,'\n').replace(/\r\n/g,'\n').replace(/\r/g,'\n');
      if(fixed!==html) pre.innerHTML=fixed;
    }
  }
  function run(){
    restorePreNewlines();
    if(!window.SyntaxHighlighter) return;
    try{
      if(SyntaxHighlighter.config){ SyntaxHighlighter.config.bloggerMode=true; SyntaxHighlighter.config.stripBrs=false; }
      if(typeof SyntaxHighlighter.all==='function') SyntaxHighlighter.all();
    }catch(e){}
  }
  if(document.readyState==='loading') document.addEventListener('DOMContentLoaded',run,{once:true});
  else run();
})();
</script>"#,
    );
    output.push_str(html_footer());

    output
}

fn render_index(pages: &[Page], site_title: &str) -> String {
    render_page(0, pages, site_title)
}

/// 以巢狀 unordered list 呈現 h1 / h2 / h3 階層（對齊動態站 SMap 的階層語意）
fn render_sitemap_nodes(nodes: &[NavNode], pages: &[Page], output: &mut String) {
    for node in nodes {
        let page = &pages[node.page_index];
        output.push_str("<li>");
        output.push_str(&format!(
            "<a href=\"{}\">{}</a>",
            escape_html(&page.filename),
            escape_html(&page.title)
        ));
        if !node.children.is_empty() {
            output.push_str("\n<ul>\n");
            render_sitemap_nodes(&node.children, pages, output);
            output.push_str("</ul>\n");
        }
        output.push_str("</li>\n");
    }
}

fn render_sitemap(pages: &[Page], site_title: &str) -> String {
    let mut output = html_header("網站地圖", site_title);

    output = output.replace(
        "<!-- NAVIGATION_MENU_INSERTED_BY_CONVERTER -->",
        &navigation_links(pages),
    );

    output.push_str("<h1>網站地圖</h1>\n<ul>\n");

    let tree = build_navigation_tree(pages);
    render_sitemap_nodes(&tree, pages, &mut output);

    output.push_str("</ul>\n");
    output.push_str(html_footer());

    output
}
fn do_generate(state: &AppState, site_title: &str) -> Result<String, Box<dyn Error>> {
    println!("讀取：{}", state.content_path.display());

    let pages = load_pages(&state.content_path)?;

    if pages.is_empty() {
        return Err("沒有可產生的分頁".into());
    }

    let output_dir = state.base_dir.join("content");
    fs::create_dir_all(&output_dir)?;

    let mut log = String::new();
    let mut search_pages: Vec<SearchPage> = Vec::new();

    for (index, page) in pages.iter().enumerate() {
        let output_path = output_dir.join(&page.filename);

        fs::write(&output_path, render_page(index, &pages, site_title))?;

        search_pages.push(SearchPage {
            title: page.title.clone(),
            text: strip_tags(&page.body)?,
            tags: String::new(),
            url: page.filename.clone(),
        });

        log.push_str(&format!("產生：{}\n", page.filename));
    }

    fs::write(output_dir.join("index.html"), render_index(&pages, site_title))?;

    log.push_str("產生：index.html\n");

    fs::write(output_dir.join("sitemap.html"), render_sitemap(&pages, site_title))?;

    log.push_str("產生：sitemap.html\n");

    let search_json = serde_json::to_string(&search_pages)?;

    let search_js = format!("var tipuesearch = {{\"pages\": {}}};\n", search_json);

    fs::write(output_dir.join("tipuesearch_content.js"), search_js)?;

    log.push_str("產生：tipuesearch_content.js\n");

    log.push_str(&format!(
        "\n轉檔完成！\n分頁數量：{}\n輸出目錄：{}\n",
        pages.len(),
        output_dir.display()
    ));

    Ok(log)
}

pub async fn generate_pages(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin(&headers) {
        return Redirect::to("/login").into_response();
    }

    let site_title = auth::load_sitetitle(&state.base_dir);
    let pages = load_pages(&state.content_path).unwrap_or_default();

    let body = match do_generate(&state, &site_title) {
        Ok(msg) => format!(
            r#"<h2>Convert 完成</h2>
<pre style="white-space:pre-wrap;background:#f5f5f5;padding:12px;border-radius:4px">{}</pre>"#,
            escape_html(&msg)
        ),
        Err(e) => format!(
            r#"<h2>Convert 失敗</h2>
<pre style="white-space:pre-wrap;background:#fdecea;padding:12px;border-radius:4px;color:#c00">{}</pre>"#,
            escape_html(&e.to_string())
        ),
    };

    Html(render_public_page(
        &site_title,
        &pages,
        &body,
        true,
        None,
        &state.ip,
        state.port,
    ))
    .into_response()
}
