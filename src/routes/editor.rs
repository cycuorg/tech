//! EditA、單頁儲存、檔案選擇

use axum::{
    extract::{Form, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use std::fs;

use crate::auth::{self, is_admin};
use crate::content::{
    encode_url, escape_html, file_get_contents, list_files, load_pages, normalize_preformatted_html,
};
use crate::templates::{render_edit_prev_next, render_editor_page, render_simple_message};
use crate::AppState;

#[derive(Deserialize)]
pub struct EditAllQuery {
    saved: Option<String>,
}

fn require_admin(headers: &HeaderMap) -> Result<(), Redirect> {
    if is_admin(headers) {
        Ok(())
    } else {
        Err(Redirect::to("/login"))
    }
}

fn write_content_htm(path: &std::path::Path, data: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir {}: {}", parent.display(), e))?;
    }
    // Windows 上 rename 覆蓋可能失敗，改用直接 write
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    // 備份
    if path.exists() {
        let bak = path.with_extension("htm.bak");
        let _ = fs::copy(path, &bak);
    }
    fs::write(path, data.as_bytes()).map_err(|e| {
        format!(
            "write {} failed: {} (abs={})",
            path.display(),
            e,
            abs.display()
        )
    })?;
    // 驗證
    let written = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    eprintln!(
        "[cms-rust] SAVED {} bytes -> {} (verify_meta={})",
        data.len(),
        path.display(),
        written
    );
    if written == 0 && !data.is_empty() {
        return Err("write reported ok but file size is 0".into());
    }
    Ok(())
}

pub async fn edit_all(
    State(state): State<AppState>,
    Query(q): Query<EditAllQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }
    let site_title = auth::load_sitetitle(&state.base_dir);
    let content = file_get_contents(&state.content_path).unwrap_or_default();
    // 進入 Edit All 前先修復 pre 內舊 <br>，避免再寫回一行
    let content = normalize_preformatted_html(&content);
    let pages = load_pages(&state.content_path).unwrap_or_default();
    // Edit All 也提供前後頁導覽（以第 0 頁為基準，可進入各單頁 Edit）
    let edit_nav = if pages.is_empty() {
        String::new()
    } else {
        render_edit_prev_next(0, &pages)
    };
    let saved_banner = if q.saved.as_deref() == Some("1") {
        r#"<div class="notice" style="display:block">已成功儲存至 config/content.htm</div>"#
    } else {
        ""
    };
    let editor_html = format!(
        r#"{banner}<h2>Edit All（content.htm）</h2>
<p style="color:#c00">儲存將覆蓋整個 content.htm</p>
<p style="color:#666;font-size:0.9em">路徑：{path}</p>
{edit_nav}
<form id="cms-edit-form" action="/save_page" method="post" accept-charset="UTF-8">
<textarea class="cms-editor" name="content">{content}</textarea>
</form>
<p style="margin-top:10px">
<button type="button" class="btn cms-save-outside" disabled style="opacity:.5;cursor:not-allowed" onclick="cmsDoSave()">Save All</button>
</p>
{edit_nav}"#,
        banner = saved_banner,
        path = escape_html(&state.content_path.display().to_string()),
        edit_nav = edit_nav,
        content = escape_html(&content),
    );
    let cur = pages.first().map(|p| p.title.as_str()).unwrap_or("");
    Html(render_editor_page(
        &site_title,
        &pages,
        cur,
        &editor_html,
        &state.ip,
        state.port,
    ))
    .into_response()
}

#[derive(Deserialize, Debug)]
pub struct SaveSingleForm {
    page_content: String,
    #[serde(default)]
    page_order: usize,
    #[allow(dead_code)]
    action: Option<String>,
}

pub async fn save_single_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<SaveSingleForm>,
) -> Response {
    eprintln!(
        "[cms-rust] save_single_page order={} content_len={} path={}",
        form.page_order,
        form.page_content.len(),
        state.content_path.display()
    );

    if !is_admin(&headers) {
        eprintln!("[cms-rust] not admin");
        return Redirect::to("/login").into_response();
    }

    let pages = match load_pages(&state.content_path) {
        Ok(p) => p,
        Err(e) => {
            return Html(render_simple_message(
                &auth::load_sitetitle(&state.base_dir),
                "讀取失敗",
                &e.to_string(),
            ))
            .into_response();
        }
    };

    if form.page_order >= pages.len() {
        return Html(render_simple_message(
            &auth::load_sitetitle(&state.base_dir),
            "錯誤",
            &format!(
                "page_order {} 超出範圍（共 {} 頁）",
                form.page_order,
                pages.len()
            ),
        ))
        .into_response();
    }

    if form.page_content.trim().is_empty() {
        return Html(render_simple_message(
            &auth::load_sitetitle(&state.base_dir),
            "錯誤",
            "page_content 是空的（TinyMCE 可能沒有把內容寫回 textarea）",
        ))
        .into_response();
    }

    let mut new_content = String::new();
    for (i, page) in pages.iter().enumerate() {
        if i == form.page_order {
            // 與 Python ssavePage() action=="save" 一致：寫入 TinyMCE HTML。
            // 額外：pre 內 <br>/CR → LF（SH 需要真正換行；\r 會被 SH 換成空白）。
            let chunk = form
                .page_content
                .replace("// <![CDATA[", "")
                .replace("// ]]>", "");
            let chunk = normalize_preformatted_html(&chunk);
            new_content.push_str(&chunk);
            if !chunk.ends_with('\n') {
                new_content.push('\n');
            }
        } else {
            // 其他頁一併修復 pre，避免舊 <br> 殘留
            let body = normalize_preformatted_html(&page.body);
            new_content.push_str(&format!(
                "<h{lvl}>{title}</h{lvl}>\n{body}",
                lvl = page.level,
                title = page.title,
                body = body
            ));
            if !body.ends_with('\n') {
                new_content.push('\n');
            }
        }
    }

    match write_content_htm(&state.content_path, &new_content) {
        Ok(()) => {
            // 存完導回該頁編輯模式，並帶 saved=1
            let title = &pages[form.page_order].title;
            let loc = format!("/get_page/{}/1?saved=1", encode_url(title));
            Response::builder()
                .status(StatusCode::FOUND)
                .header(header::LOCATION, loc)
                .body(axum::body::Body::empty())
                .unwrap()
        }
        Err(e) => Html(render_simple_message(
            &auth::load_sitetitle(&state.base_dir),
            "存檔失敗",
            &e,
        ))
        .into_response(),
    }
}

#[derive(Deserialize)]
pub struct FileSelectorQuery {
    #[serde(rename = "type")]
    file_type: Option<String>,
}

pub async fn file_selector(
    State(state): State<AppState>,
    Query(q): Query<FileSelectorQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin(&headers) {
        return Redirect::to("/login").into_response();
    }
    let ftype = q.file_type.as_deref().unwrap_or("file");
    let (dir, prefix, exts) = if ftype == "image" {
        (
            state.base_dir.join("images"),
            "/images",
            Some(["jpg", "jpeg", "png", "gif", "webp", "svg"].as_slice()),
        )
    } else {
        (state.base_dir.join("downloads"), "/downloads", None)
    };
    let files = list_files(&dir, exts).unwrap_or_default();
    let mut list = String::new();
    for f in &files {
        list.push_str(&format!(
            r#"<div style="margin:6px 0"><a href="javascript:void(0)" onclick="selectFile('{}/{}')">{}</a></div>"#,
            prefix,
            escape_html(f),
            escape_html(f)
        ));
    }
    if list.is_empty() {
        list = "<p>尚無檔案</p>".into();
    }
    Html(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>選擇檔案</title></head>
<body style="font-family:sans-serif;padding:15px"><h3>選擇檔案</h3>{list}
<script>
function selectFile(url){{
  try{{
    var p=parent.tinymce.activeEditor.windowManager.getParams();
    if(p&&p.oninsert)p.oninsert(url,{{}});
    parent.tinymce.activeEditor.windowManager.close();
  }}catch(e){{alert(url);}}
}}
</script></body></html>"#,
        list = list
    ))
    .into_response()
}
