//! 圖片 / 下載 列表與 bit-by-bit 上傳

use axum::{
    body::Bytes,
    extract::{Form, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::auth::{self, is_admin};
use crate::content::{escape_html, list_files, load_pages};
use crate::templates::render_public_page;
use crate::AppState;

fn require_admin(headers: &HeaderMap) -> Result<(), Redirect> {
    if is_admin(headers) {
        Ok(())
    } else {
        Err(Redirect::to("/login"))
    }
}

fn multi_chunk_ui(url: &str, kind: &str) -> String {
    format!(
        r#"
        <h3>Bit-by-bit 多檔上傳</h3>
        <input type="file" id="files-{kind}" multiple>
        <button type="button" class="btn" onclick="uploadAll('{url}','{kind}')">開始上傳</button>
        <pre id="log-{kind}" style="background:#f5f5f5;padding:10px;max-height:200px;overflow:auto"></pre>
        <script>
        async function uploadAll(url, kind) {{
          const input = document.getElementById('files-' + kind);
          const log = document.getElementById('log-' + kind);
          if (!input.files.length) {{ alert('請選擇檔案'); return; }}
          log.textContent = '';
          for (const file of input.files) {{
            log.textContent += '上傳 ' + file.name + ' ...\n';
            const chunkSize = 64 * 1024;
            let offset = 0;
            while (offset < file.size) {{
              const chunk = file.slice(offset, offset + chunkSize);
              const startFlag = offset === 0 ? '0' : '1';
              const qs = '?ax-file-name=' + encodeURIComponent(file.name) + '&start=' + startFlag;
              const res = await fetch(url + qs, {{ method:'POST', body: chunk, headers:{{'Content-Type':'application/octet-stream'}}, credentials:'same-origin' }});
              if (!res.ok) {{ log.textContent += '失敗 ' + file.name + '\n'; break; }}
              offset += chunkSize;
            }}
            if (offset >= file.size) log.textContent += '完成 ' + file.name + '\n';
          }}
          log.textContent += '全部結束\n';
        }}
        </script>"#
    )
}

#[derive(Deserialize)]
pub struct ListQuery {
    deleted: Option<String>,
    error: Option<String>,
}

pub async fn image_list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }
    let site_title = auth::load_sitetitle(&state.base_dir);
    let pages = load_pages(&state.content_path).unwrap_or_default();
    let files = list_files(
        &state.base_dir.join("images"),
        Some(&["jpg", "jpeg", "png", "gif", "webp", "svg"]),
    )
    .unwrap_or_default();
    let mut body = String::new();
    if let Some(name) = q.deleted.as_deref() {
        if !name.is_empty() {
            body.push_str(&format!(
                r#"<div class="notice" style="display:block">已刪除：{}</div>"#,
                escape_html(name)
            ));
        }
    }
    if let Some(err) = q.error.as_deref() {
        if !err.is_empty() {
            body.push_str(&format!(
                r#"<div class="notice" style="display:block;background:#fee;border-color:#c00">刪除失敗：{}</div>"#,
                escape_html(err)
            ));
        }
    }
    body.push_str("<h2>IList</h2><ul>");
    for f in &files {
        body.push_str(&format!(
            r#"<li><a href="/images/{0}" target="_blank">{0}</a>
            <form method="post" action="/delete_file" style="display:inline" onsubmit="return confirm('刪除？')">
            <input type="hidden" name="filename" value="{0}"><input type="hidden" name="dir" value="images">
            <button type="submit">刪</button></form></li>"#,
            escape_html(f)
        ));
    }
    body.push_str("</ul>");
    body.push_str(&multi_chunk_ui("/imageaxupload", "img"));
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

pub async fn download_list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }
    let site_title = auth::load_sitetitle(&state.base_dir);
    let pages = load_pages(&state.content_path).unwrap_or_default();
    let files = list_files(&state.base_dir.join("downloads"), None).unwrap_or_default();
    let mut body = String::new();
    if let Some(name) = q.deleted.as_deref() {
        if !name.is_empty() {
            body.push_str(&format!(
                r#"<div class="notice" style="display:block">已刪除：{}</div>"#,
                escape_html(name)
            ));
        }
    }
    if let Some(err) = q.error.as_deref() {
        if !err.is_empty() {
            body.push_str(&format!(
                r#"<div class="notice" style="display:block;background:#fee;border-color:#c00">刪除失敗：{}</div>"#,
                escape_html(err)
            ));
        }
    }
    body.push_str("<h2>FList</h2><ul>");
    for f in &files {
        body.push_str(&format!(
            r#"<li><a href="/downloads/{0}" target="_blank">{0}</a>
            <form method="post" action="/delete_file" style="display:inline" onsubmit="return confirm('刪除？')">
            <input type="hidden" name="filename" value="{0}"><input type="hidden" name="dir" value="downloads">
            <button type="submit">刪</button></form></li>"#,
            escape_html(f)
        ));
    }
    body.push_str("</ul>");
    body.push_str(&multi_chunk_ui("/fileaxupload", "file"));
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

pub async fn image_upload_form(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }
    let site_title = auth::load_sitetitle(&state.base_dir);
    let pages = load_pages(&state.content_path).unwrap_or_default();
    let body = format!(
        "<h2>IUpload</h2>{}",
        multi_chunk_ui("/imageaxupload", "img")
    );
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

pub async fn file_upload_form(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }
    let site_title = auth::load_sitetitle(&state.base_dir);
    let pages = load_pages(&state.content_path).unwrap_or_default();
    let body = format!(
        "<h2>FUpload</h2>{}",
        multi_chunk_ui("/fileaxupload", "file")
    );
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

#[derive(Deserialize)]
pub struct ChunkQuery {
    #[serde(rename = "ax-file-name")]
    ax_file_name: Option<String>,
    start: Option<String>,
}

fn write_chunk(
    state: &AppState,
    subdir: &str,
    q: &ChunkQuery,
    data: &Bytes,
) -> Result<String, String> {
    let filename = q.ax_file_name.as_ref().ok_or("missing ax-file-name")?;
    let safe_name = PathBuf::from(filename)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .ok_or("bad filename")?;
    let dir = state.base_dir.join(subdir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&safe_name);
    let start = q.start.as_deref().unwrap_or("0");
    let mut file = if start == "0" {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| e.to_string())?
    } else {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| e.to_string())?
    };
    file.write_all(data).map_err(|e| e.to_string())?;
    Ok(format!("{} ok", safe_name))
}

pub async fn imageaxupload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ChunkQuery>,
    body: Bytes,
) -> impl IntoResponse {
    if !is_admin(&headers) {
        return (StatusCode::UNAUTHORIZED, "login required").into_response();
    }
    match write_chunk(&state, "images", &q, &body) {
        Ok(m) => (StatusCode::OK, m).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn fileaxupload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ChunkQuery>,
    body: Bytes,
) -> impl IntoResponse {
    if !is_admin(&headers) {
        return (StatusCode::UNAUTHORIZED, "login required").into_response();
    }
    match write_chunk(&state, "downloads", &q, &body) {
        Ok(m) => (StatusCode::OK, m).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct DeleteForm {
    filename: String,
    dir: String,
}

pub async fn delete_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<DeleteForm>,
) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }
    let allowed = form.dir == "images" || form.dir == "downloads";
    let safe = PathBuf::from(&form.filename)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    // 回傳編輯模式列表頁（與其他功能一致，不再另開訊息頁）
    let list_path = if form.dir == "images" {
        "/image_list"
    } else {
        "/download_list"
    };

    if !allowed || safe.is_empty() {
        let loc = format!("{}?error={}", list_path, urlencoding_encode("不合法參數"));
        return Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, loc)
            .body(axum::body::Body::empty())
            .unwrap();
    }

    match fs::remove_file(state.base_dir.join(&form.dir).join(&safe)) {
        Ok(_) => {
            let loc = format!("{}?deleted={}", list_path, urlencoding_encode(&safe));
            Response::builder()
                .status(StatusCode::FOUND)
                .header(header::LOCATION, loc)
                .body(axum::body::Body::empty())
                .unwrap()
        }
        Err(e) => {
            let loc = format!("{}?error={}", list_path, urlencoding_encode(&e.to_string()));
            Response::builder()
                .status(StatusCode::FOUND)
                .header(header::LOCATION, loc)
                .body(axum::body::Body::empty())
                .unwrap()
        }
    }
}

/// 簡易 URL encode（僅處理查詢字串中常見需要編碼的字元）
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}
