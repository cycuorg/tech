//! 前台瀏覽、搜尋、登入

use axum::{
    extract::{Form, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use crate::auth::{self, is_admin};
use crate::content::{
    decode_url, encode_url, escape_html, load_pages, normalize_preformatted_html, search_content,
};
use crate::templates::{
    render_edit_prev_next, render_editor_page, render_prev_next, render_public_page,
};
use crate::AppState;

pub async fn index(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let title = auth::load_sitetitle(&state.base_dir);
    let admin = is_admin(&headers);
    match load_pages(&state.content_path) {
        Ok(pages) if !pages.is_empty() => {
            let (top, bottom) = render_prev_next(0, &pages);
            let body = format!(
                "{}{}{}{}",
                top,
                format!("<h1>{}</h1>\n", escape_html(&pages[0].title)),
                pages[0].body,
                bottom
            );
            Html(render_public_page(
                &title,
                &pages,
                &body,
                admin,
                Some(&pages[0].title),
                &state.ip,
                state.port,
            ))
            .into_response()
        }
        Ok(_) => Html(render_public_page(
            &title,
            &[],
            "<p>尚無內容</p>",
            admin,
            None,
            &state.ip,
            state.port,
        ))
        .into_response(),
        Err(e) => Html(render_public_page(
            &title,
            &[],
            &format!("<pre>{}</pre>", e),
            admin,
            None,
            &state.ip,
            state.port,
        ))
        .into_response(),
    }
}

pub async fn get_page(
    State(state): State<AppState>,
    Path(heading): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    get_page_inner(&state, &heading, 0, &headers, false).await
}

#[derive(Deserialize)]
pub struct EditQuery {
    saved: Option<String>,
}

pub async fn get_page_edit(
    State(state): State<AppState>,
    Path((heading, edit)): Path<(String, u32)>,
    Query(q): Query<EditQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    get_page_inner(
        &state,
        &heading,
        edit,
        &headers,
        q.saved.as_deref() == Some("1"),
    )
    .await
}

async fn get_page_inner(
    state: &AppState,
    heading_raw: &str,
    edit: u32,
    headers: &HeaderMap,
    show_saved: bool,
) -> Response {
    let mut heading = decode_url(heading_raw);
    if heading.contains('%') {
        heading = decode_url(&heading);
    }
    let title = auth::load_sitetitle(&state.base_dir);
    let admin = is_admin(headers);

    let pages = match load_pages(&state.content_path) {
        Ok(p) => p,
        Err(e) => {
            return Html(render_public_page(
                &title,
                &[],
                &format!("<pre>{}</pre>", e),
                admin,
                None,
                &state.ip,
                state.port,
            ))
            .into_response();
        }
    };

    let found = pages.iter().enumerate().find(|(_, p)| {
        p.title == heading
            || p.title == heading_raw
            || encode_url(&p.title) == heading_raw
            || encode_url(&p.title) == heading
    });

    let Some((idx, page)) = found else {
        return (
            StatusCode::NOT_FOUND,
            Html(render_public_page(
                &title,
                &pages,
                &format!("<p>找不到：{}</p>", escape_html(&heading)),
                admin,
                None,
                &state.ip,
                state.port,
            )),
        )
            .into_response();
    };

    if edit == 1 {
        if !admin {
            return Redirect::to("/login").into_response();
        }
        // 載入編輯器前先修復 pre 內舊 <br>/實體換行，避免存回後仍是一行
        let fixed_body = normalize_preformatted_html(&page.body);
        let editor_content = format!(
            "<h{}>{}</h{}>\n{}",
            page.level, page.title, page.level, fixed_body
        );
        let saved_banner = if show_saved {
            r#"<div class="notice" style="display:block">已成功儲存至 config/content.htm</div>"#
        } else {
            ""
        };
        let edit_nav = render_edit_prev_next(idx, &pages);
        let editor_html = format!(
            r#"{banner}<h2>編輯：{ptitle}</h2>
<p style="color:#666;font-size:0.85em">寫入路徑：{path}</p>
{edit_nav}
<form id="cms-edit-form" action="/save_single_page" method="post" accept-charset="UTF-8">
<input type="hidden" name="page_order" value="{idx}">
<input type="hidden" name="action" value="save">
<textarea class="cms-editor" name="page_content">{content}</textarea>
</form>
<p style="margin-top:10px">
<button type="button" class="btn cms-save-outside" disabled style="opacity:.5;cursor:not-allowed" onclick="return cmsDoSave()">Save</button>
<a class="btn" style="background:#6c757d" href="/get_page/{enc}">ViewPage</a>
</p>
{edit_nav}"#,
            banner = saved_banner,
            ptitle = escape_html(&page.title),
            path = escape_html(&state.content_path.display().to_string()),
            edit_nav = edit_nav,
            idx = idx,
            content = escape_html(&editor_content),
            enc = encode_url(&page.title)
        );
        return Html(render_editor_page(
            &title,
            &pages,
            &page.title,
            &editor_html,
            &state.ip,
            state.port,
        ))
        .into_response();
    }

    let (top, bottom) = render_prev_next(idx, &pages);
    let body = format!(
        "{}{}{}{}",
        top,
        format!("<h1>{}</h1>\n", escape_html(&page.title)),
        page.body,
        bottom
    );
    Html(render_public_page(
        &title,
        &pages,
        &body,
        admin,
        Some(&page.title),
        &state.ip,
        state.port,
    ))
    .into_response()
}

pub async fn sitemap(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let title = auth::load_sitetitle(&state.base_dir);
    let admin = is_admin(&headers);
    match load_pages(&state.content_path) {
        Ok(pages) => {
            let mut body = String::from("<ul>");
            for page in &pages {
                body.push_str(&format!(
                    r#"<li style="margin-left:{}px"><a href="/get_page/{}">{}</a></li>"#,
                    (page.level.saturating_sub(1)) * 20,
                    encode_url(&page.title),
                    escape_html(&page.title)
                ));
            }
            body.push_str("</ul>");
            Html(render_public_page(
                &title, &pages, &body, admin, None, &state.ip, state.port,
            ))
            .into_response()
        }
        Err(e) => Html(render_public_page(
            &title,
            &[],
            &format!("<pre>{}</pre>", e),
            admin,
            None,
            &state.ip,
            state.port,
        ))
        .into_response(),
    }
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
}

pub async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let title = auth::load_sitetitle(&state.base_dir);
    let keyword = query.q.unwrap_or_default();
    let admin = is_admin(&headers);
    match load_pages(&state.content_path) {
        Ok(pages) => {
            let results = search_content(&pages, &keyword);
            let mut body = format!(
                r#"<h2>Search</h2>
<form method="get" action="/search">
<input type="text" name="q" value="{}" style="width:60%;padding:6px">
<button type="submit">Search</button></form>"#,
                escape_html(&keyword)
            );
            if !keyword.is_empty() {
                body.push_str(&format!("<h3>結果：{} 筆</h3><ul>", results.len()));
                for (t, snip) in &results {
                    body.push_str(&format!(
                        r#"<li><a href="/get_page/{}"><strong>{}</strong></a><br><small>{}</small></li>"#,
                        encode_url(t),
                        escape_html(t),
                        escape_html(snip)
                    ));
                }
                body.push_str("</ul>");
            }
            Html(render_public_page(
                &title, &pages, &body, admin, None, &state.ip, state.port,
            ))
            .into_response()
        }
        Err(e) => Html(render_public_page(
            &title,
            &[],
            &format!("<pre>{}</pre>", e),
            admin,
            None,
            &state.ip,
            state.port,
        ))
        .into_response(),
    }
}

/// Login 表單內容（顯示於動態編輯區，與 Search / ACP 相同版面）
fn login_form_body(error: Option<&str>) -> String {
    let err = error
        .map(|e| format!("<p style=\"color:red\">{}</p>", escape_html(e)))
        .unwrap_or_default();
    format!(
        r#"<h2>Login</h2>
{err}
<form method="post" action="/check_login">
<p>Password:<br>
<input type="password" name="password" required autofocus
       style="width:100%;max-width:320px;padding:8px;margin:8px 0"></p>
<p><button type="submit" style="padding:8px 20px;background:#2c5f8a;color:#fff;border:none;border-radius:4px">Login</button></p>
</form>"#,
        err = err
    )
}

pub async fn login_page(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if is_admin(&headers) {
        return Redirect::to("/").into_response();
    }
    let title = auth::load_sitetitle(&state.base_dir);
    let pages = load_pages(&state.content_path).unwrap_or_default();
    Html(render_public_page(
        &title,
        &pages,
        &login_form_body(None),
        false,
        None,
        &state.ip,
        state.port,
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct LoginForm {
    password: String,
}

pub async fn check_login(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    if auth::check_password(&state.base_dir, &form.password) {
        Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, "/")
            .header(header::SET_COOKIE, auth::admin_cookie())
            .body(axum::body::Body::empty())
            .unwrap()
    } else {
        let title = auth::load_sitetitle(&state.base_dir);
        let pages = load_pages(&state.content_path).unwrap_or_default();
        Html(render_public_page(
            &title,
            &pages,
            &login_form_body(Some("密碼錯誤")),
            false,
            None,
            &state.ip,
            state.port,
        ))
        .into_response()
    }
}

pub async fn logout() -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, "/")
        .header(header::SET_COOKIE, auth::clear_admin_cookie())
        .body(axum::body::Body::empty())
        .unwrap()
}
