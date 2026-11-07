use axum::http::HeaderMap;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub fn load_sitetitle(base: &Path) -> String {
    match fs::read_to_string(base.join("config").join("sitetitle")) {
        Ok(s) => {
            let t = s.trim().to_string();
            if t.is_empty() {
                "CMS-Rust".into()
            } else {
                t
            }
        }
        Err(_) => "CMS-Rust".into(),
    }
}

pub fn save_sitetitle(base: &Path, title: &str) {
    let _ = fs::create_dir_all(base.join("config"));
    let _ = fs::write(base.join("config").join("sitetitle"), title.trim());
}

pub fn encode_password(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"cmsimde-salt-");
    hasher.update(password.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn load_password_hash(base: &Path) -> String {
    let path = base.join("config").join("config");
    if let Ok(s) = fs::read_to_string(&path) {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    let default = encode_password("admin");
    let _ = fs::create_dir_all(base.join("config"));
    let _ = fs::write(&path, &default);
    default
}

pub fn save_password(base: &Path, plain: &str) {
    let _ = fs::create_dir_all(base.join("config"));
    let _ = fs::write(base.join("config").join("config"), encode_password(plain));
}

pub fn check_password(base: &Path, input: &str) -> bool {
    encode_password(input) == load_password_hash(base)
}

pub fn is_admin(headers: &HeaderMap) -> bool {
    headers
        .get("cookie")
        .and_then(|c| c.to_str().ok())
        .map(|s| s.contains("cms_admin=1"))
        .unwrap_or(false)
}

pub fn admin_cookie() -> String {
    "cms_admin=1; Path=/; HttpOnly; SameSite=Lax".into()
}

pub fn clear_admin_cookie() -> String {
    "cms_admin=; Path=/; Max-Age=0; HttpOnly".into()
}
