use axum::{
    extract::{Form, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use std::fs;
use std::process::{Command, Output};

use crate::auth::{self, is_admin};
use crate::content::{escape_html, load_pages, normalize_preformatted_html};
use crate::templates::{render_public_page, render_simple_message};
use crate::AppState;

fn require_admin(headers: &HeaderMap) -> Result<(), Redirect> {
    if is_admin(headers) {
        Ok(())
    } else {
        Err(Redirect::to("/login"))
    }
}

// ============================================================
// Config
// ============================================================

#[derive(Deserialize)]
pub struct EditConfigQuery {
    saved: Option<String>,
}

pub async fn edit_config(
    State(state): State<AppState>,
    Query(q): Query<EditConfigQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }

    let site_title = auth::load_sitetitle(&state.base_dir);
    let pages = load_pages(&state.content_path).unwrap_or_default();

    let saved_banner = if q.saved.as_deref() == Some("1") {
        r#"<div class="notice" style="display:block;margin-bottom:12px;padding:10px;background:#e8f5e9;border:1px solid #4caf50;color:#2e7d32">設定已成功儲存</div>"#
    } else {
        ""
    };

    let body = format!(
        r#"{banner}<h2>Config</h2>
<form method="post" action="/save_config">
<p>網站標題：<br>
<input type="text" name="title" value="{title}"
style="width:320px;padding:6px"></p>
<p>新密碼（留空不改）：<br>
<input type="password" name="password"
style="width:320px;padding:6px"></p>
<p><button type="submit">儲存</button></p>
</form>"#,
        banner = saved_banner,
        title = crate::content::escape_html(&site_title),
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
pub struct SaveConfigForm {
    title: String,
    password: Option<String>,
}

pub async fn save_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<SaveConfigForm>,
) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }

    auth::save_sitetitle(&state.base_dir, &form.title);

    if let Some(pw) = form.password {
        if !pw.is_empty() {
            auth::save_password(&state.base_dir, &pw);
        }
    }

    // 儲存後導回編輯頁面並顯示成功訊息，而非另開簡單訊息頁
    Redirect::to("/edit_config?saved=1").into_response()
}

// ============================================================
// Save page
// ============================================================

#[derive(Deserialize)]
pub struct SaveAllForm {
    content: String,
}

pub async fn save_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<SaveAllForm>,
) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }

    if let Some(parent) = state.content_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let _ = fs::copy(
        &state.content_path,
        state.content_path.with_extension("htm.bak"),
    );

    // Edit All 與單頁儲存使用相同規則，避免舊版 <pre><br> 再次寫回檔案。
    let normalized_content = normalize_preformatted_html(&form.content);
    match fs::write(&state.content_path, normalized_content.as_bytes()) {
        Ok(_) => {
            eprintln!(
                "[cms-rust] EditAll saved {} bytes -> {}",
                normalized_content.len(),
                state.content_path.display()
            );

            // 與單頁儲存一致：導回編輯模式並顯示成功提示，不再另開訊息頁
            Response::builder()
                .status(StatusCode::FOUND)
                .header(header::LOCATION, "/edit_all?saved=1")
                .body(axum::body::Body::empty())
                .unwrap()
        }
        Err(e) => Html(render_simple_message(
            &auth::load_sitetitle(&state.base_dir),
            "存檔失敗",
            &e.to_string(),
        ))
        .into_response(),
    }
}

// ============================================================
// ACP Git form
// ============================================================

pub async fn acpform(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }

    let site_title = auth::load_sitetitle(&state.base_dir);
    let pages = load_pages(&state.content_path).unwrap_or_default();

    // 與原版 Flask CMS 相同：表單顯示於動態編輯區（section）
    let body = r#"
<h2>ACP — Git</h2>
<form method="post" action="/do_acp" id="cms-acp-form">
    <p>
        Commit message：<br>
        <textarea
            name="commit"
            rows="3"
            maxlength="200"
            required
            placeholder="請輸入 Git commit 訊息"
            style="width:100%;max-width:500px;box-sizing:border-box;padding:6px"
        ></textarea>
    </p>
    <p>
        <button type="submit">git add / commit / push</button>
    </p>
</form>
"#;

    Html(render_public_page(
        &site_title,
        &pages,
        body,
        true,
        None,
        &state.ip,
        state.port,
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct GitCommitForm {
    commit: String,
}

// ============================================================
// Git command helper
// 使用 std::process::Command 同步執行，並帶入基本 identity，
// 避免「Author identity unknown」導致 commit 失敗。
// ============================================================

fn run_git(base_dir: &std::path::Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .args(args)
        .current_dir(base_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "CMS-Rust")
        .env("GIT_AUTHOR_EMAIL", "cms-rust@localhost")
        .env("GIT_COMMITTER_NAME", "CMS-Rust")
        .env("GIT_COMMITTER_EMAIL", "cms-rust@localhost")
        .output()
        .map_err(|e| format!("無法執行 git {:?}: {}", args, e))
}

/// 確保 base_dir 是 git 倉庫；若尚無 .git 則執行 git init
fn ensure_git_repo(base_dir: &std::path::Path) -> Result<(), String> {
    if base_dir.join(".git").exists() {
        return Ok(());
    }
    let out = run_git(base_dir, &["init"])?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git init 失敗：\n{}", stderr));
    }
    Ok(())
}

// ============================================================
// ACP Git: add / commit / push
// ============================================================

/// 將 ACP 執行結果以動態編輯區（render_public_page）顯示
fn acp_result_page(state: &AppState, title: &str, msg: &str, is_error: bool) -> Response {
    let site_title = auth::load_sitetitle(&state.base_dir);
    let pages = load_pages(&state.content_path).unwrap_or_default();
    let style = if is_error {
        "background:#fdecea;color:#c00"
    } else {
        "background:#eef7ee;color:#060"
    };
    let body = format!(
        r#"<h2>{}</h2>
<pre style="white-space:pre-wrap;padding:12px;border-radius:4px;{}">{}</pre>
<p style="margin-top:12px"><a href="/acpform">返回 ACP</a> · <a href="/">Home</a></p>"#,
        escape_html(title),
        style,
        escape_html(msg)
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

pub async fn do_acp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<GitCommitForm>,
) -> Response {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }

    let commit = form.commit.trim();

    if commit.is_empty() {
        return acp_result_page(&state, "ACP 失敗", "Commit message 不可為空白", true);
    }

    if commit.chars().count() > 200 {
        return acp_result_page(
            &state,
            "ACP 失敗",
            "Commit message 不可超過 200 個字元",
            true,
        );
    }

    let base_dir = state.base_dir.as_path();
    let mut log = String::new();

    // 0. 若尚未是 git 倉庫，先 init（對齊原版腳本可在新目錄直接使用）
    if let Err(e) = ensure_git_repo(base_dir) {
        return acp_result_page(&state, "ACP 失敗", &e, true);
    }
    log.push_str("git 倉庫就緒\n");

    // 1. git add .
    let add = match run_git(base_dir, &["add", "."]) {
        Ok(output) => output,
        Err(e) => {
            return acp_result_page(&state, "ACP 失敗", &e, true);
        }
    };

    if !add.status.success() {
        let err = String::from_utf8_lossy(&add.stderr);
        return acp_result_page(
            &state,
            "ACP 失敗",
            &format!("git add 失敗：\n{}", err),
            true,
        );
    }
    log.push_str("git add . 完成\n");

    // 2. git commit -m <表單輸入>
    // 訊息以獨立參數傳遞，不經過 Shell，避免注入。
    let commit_output = match run_git(base_dir, &["commit", "-m", commit]) {
        Ok(output) => output,
        Err(e) => {
            return acp_result_page(
                &state,
                "ACP 失敗",
                &format!("無法執行 git commit：{}", e),
                true,
            );
        }
    };

    let stdout = String::from_utf8_lossy(&commit_output.stdout);
    let stderr = String::from_utf8_lossy(&commit_output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if !commit_output.status.success() {
        // 「nothing to commit」視為可接受的結果，繼續嘗試 push
        let nothing = combined.to_lowercase().contains("nothing to commit")
            || combined
                .to_lowercase()
                .contains("no changes added to commit");
        if nothing {
            log.push_str(&format!(
                "git commit：工作目錄無變更（nothing to commit）\n{}\n",
                combined
            ));
        } else {
            return acp_result_page(
                &state,
                "ACP 失敗",
                &format!("git commit 失敗：\n{}", combined),
                true,
            );
        }
    } else {
        log.push_str(&format!(
            "git commit -m \"{}\" 完成\n{}\n",
            commit, combined
        ));
    }

    // 3. git push
    let push = match run_git(base_dir, &["push"]) {
        Ok(output) => output,
        Err(e) => {
            return acp_result_page(
                &state,
                "ACP 部分完成",
                &format!("{}\n已完成 add/commit，但 git push 無法執行：{}", log, e),
                true,
            );
        }
    };

    let p_stdout = String::from_utf8_lossy(&push.stdout);
    let p_stderr = String::from_utf8_lossy(&push.stderr);

    if !push.status.success() {
        // 常見：尚未設定 upstream 或無遠端，仍回報部分完成而非完全失敗
        return acp_result_page(
            &state,
            "ACP 部分完成",
            &format!(
                "{}\n已完成 add/commit，但 git push 失敗：\n{}{}",
                log, p_stdout, p_stderr
            ),
            true,
        );
    }

    log.push_str(&format!("git push 完成\n{}{}", p_stdout, p_stderr));

    acp_result_page(
        &state,
        "ACP 完成",
        &format!(
            "Git 新增、提交與推送完成！\n\nCommit message：{}\n\n{}",
            commit, log
        ),
        false,
    )
}

// ============================================================
// SStatic
// ============================================================

pub async fn start_static(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = require_admin(&headers) {
        return r.into_response();
    }

    let content_dir = state.base_dir.join("content");
    let _ = fs::create_dir_all(&content_dir);

    let target_url = "/content/index.html";

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="zh-Hant">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>SStatic</title>
</head>
<body>
    <p>正在新分頁開啟靜態網站……</p>
    <p>
        <a href="{target_url}" target="_blank"
           rel="noopener noreferrer">
            如果沒有自動開啟，請點擊這裡
        </a>
    </p>
    <script>
        window.open("{target_url}", "_blank");
        window.location.href = "/";
    </script>
</body>
</html>"#
    ))
    .into_response()
}
