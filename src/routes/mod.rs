pub mod admin;
pub mod editor;
pub mod files;
pub mod generate;
pub mod page;

use crate::AppState;

use axum::{
    routing::{get, post},
    Router,
};

use tower_http::services::ServeDir;

/// 建立 RustCMS 主 Router。
pub fn create_router(state: AppState) -> Router {
    let static_path = state.base_dir.join("static");
    let images_path = state.base_dir.join("images");
    let downloads_path = state.base_dir.join("downloads");
    let content_static = state.base_dir.join("content");

    // --------------------------------------------------
    // CMS 主 Router
    // --------------------------------------------------

    Router::new()
        // ----------------------------------------------
        // CMS 首頁及頁面路由
        // ----------------------------------------------
        .route("/", get(page::index))
        .route("/get_page/:heading", get(page::get_page))
        .route("/get_page/:heading/:edit", get(page::get_page_edit))
        .route("/sitemap", get(page::sitemap))
        .route("/search", get(page::search))
        .route("/search_form", get(page::search))
        .route("/login", get(page::login_page))
        .route("/check_login", post(page::check_login))
        .route("/logout", get(page::logout))
        // ----------------------------------------------
        // CMS 編輯器
        // ----------------------------------------------
        .route("/edit_all", get(editor::edit_all))
        .route("/edit_page", get(editor::edit_all))
        .route("/save_single_page", post(editor::save_single_page))
        .route("/file_selector", get(editor::file_selector))
        // ----------------------------------------------
        // CMS 管理功能
        // ----------------------------------------------
        .route("/save_page", post(admin::save_page))
        .route("/edit_config", get(admin::edit_config))
        .route("/save_config", post(admin::save_config))
        .route("/acpform", get(admin::acpform))
        .route("/do_acp", post(admin::do_acp))
        .route("/start_static", get(admin::start_static))
        // ----------------------------------------------
        // CMS 靜態頁面產生
        // ----------------------------------------------
        .route("/generate_pages", get(generate::generate_pages))
        // ----------------------------------------------
        // CMS 檔案管理
        // ----------------------------------------------
        .route("/image_list", get(files::image_list))
        .route("/download_list", get(files::download_list))
        .route("/image_upload_form", get(files::image_upload_form))
        .route("/file_upload_form", get(files::file_upload_form))
        .route("/imageaxupload", post(files::imageaxupload))
        .route("/fileaxupload", post(files::fileaxupload))
        .route("/imageuploadform", get(files::image_upload_form))
        .route("/fileuploadform", get(files::file_upload_form))
        .route("/delete_file", post(files::delete_file))
        // ----------------------------------------------
        // CMS 原有靜態資源
        // ----------------------------------------------
        .nest_service("/static", ServeDir::new(static_path))
        .nest_service("/images", ServeDir::new(images_path))
        .nest_service("/downloads", ServeDir::new(downloads_path))
        .nest_service("/content", ServeDir::new(content_static))
        // ----------------------------------------------
        // 設定 AppState
        // ----------------------------------------------
        .with_state(state)
}
