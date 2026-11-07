mod auth;
mod content;
mod routes;
mod templates;

use std::net::SocketAddr;
use std::path::PathBuf;

use axum_server::tls_rustls::RustlsConfig;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use tower_http::cors::CorsLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
pub struct AppState {
    pub base_dir: PathBuf,
    pub ip: String,
    pub port: u16,
    pub content_path: PathBuf,
}

/// 從 config/setup.ini 讀取 ip 與 port。
/// 支援簡單格式：
///   ip = "127.0.0.1"
///   port = 5000
/// 若檔案不存在或解析失敗，回傳預設值。
fn load_setup(base_dir: &std::path::Path) -> (String, u16) {
    let path = base_dir.join("config").join("setup.ini");
    let mut ip = "127.0.0.1".to_string();
    let mut port: u16 = 5000;

    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_lowercase();
                let value = value.trim().trim_matches('"').trim_matches('\'').trim();
                match key.as_str() {
                    "ip" => {
                        if !value.is_empty() {
                            ip = value.to_string();
                        }
                    }
                    "port" => {
                        if let Ok(p) = value.parse::<u16>() {
                            port = p;
                        }
                    }
                    _ => {}
                }
            }
        }
        tracing::info!(
            "Loaded setup from {}: ip={}, port={}",
            path.display(),
            ip,
            port
        );
    } else {
        // 若檔案不存在則寫入預設值，方便之後修改
        let default = r#"ip = "127.0.0.1"
port = 5000
"#;
        let _ = std::fs::write(&path, default);
        tracing::info!(
            "Created default {} (ip=127.0.0.1, port=5000)",
            path.display()
        );
    }

    // 環境變數可覆寫 ip
    if let Ok(env_ip) = std::env::var("CMS_IP") {
        let env_ip = env_ip.trim();

        if !env_ip.is_empty() {
            ip = env_ip.to_string();
            tracing::info!("CMS_IP env overrides ip to {}", ip);
        }
    }

    // 環境變數仍可覆寫 port（向後相容）
    if let Ok(env_port) = std::env::var("CMS_PORT") {
        if let Ok(p) = env_port.parse::<u16>() {
            port = p;
            tracing::info!("CMS_PORT env overrides port to {}", port);
        }
    }

    (ip, port)
}

/// 動態產生 self-signed 憑證（含指定 IP 與 localhost 作為 SAN）
/// 使用 rcgen 0.13 API
fn generate_self_signed(ip: &str) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "CMS-Rust Self-Signed");

    let mut sans = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
    ];

    // 若綁定 IP 不是 0.0.0.0 / 127.0.0.1，額外加入
    if ip != "0.0.0.0" && ip != "127.0.0.1" {
        if let Ok(addr) = ip.parse::<std::net::IpAddr>() {
            sans.push(SanType::IpAddress(addr));
        }
    }

    params.subject_alt_names = sans;

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    let cert_pem = cert.pem().into_bytes();
    let key_pem = key_pair.serialize_pem().into_bytes();
    Ok((cert_pem, key_pem))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cms_rust=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let base_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let base_dir = if base_dir.join("config").exists() {
        base_dir
    } else if let Ok(exe) = std::env::current_exe() {
        let parent = exe
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(base_dir.clone());
        if parent.join("config").exists() {
            parent
        } else {
            base_dir
        }
    } else {
        base_dir
    };

    for dir in &["config", "content", "static", "images", "downloads"] {
        let _ = std::fs::create_dir_all(base_dir.join(dir));
    }

    let content_path = base_dir.join("config").join("content.htm");
    if !content_path.exists() {
        let _ = std::fs::write(
            &content_path,
            r#"<h1>About</h1>
<p>Welcome to CMS-Rust.</p>
<h2>Features</h2>
<p>Edit and generate static site.</p>
<h1>Second</h1>
<p>Another page.</p>
"#,
        );
    }
    if !base_dir.join("config").join("sitetitle").exists() {
        auth::save_sitetitle(&base_dir, "Dialog");
    }
    let _ = auth::load_password_hash(&base_dir);

    // 讀取 setup.ini 起始設定
    let (bind_ip, port) = load_setup(&base_dir);

    let state = AppState {
        base_dir: base_dir.clone(),
        content_path,
        ip: bind_ip.clone(),
        port,
    };
    let app = routes::create_router(state).layer(CorsLayer::permissive());

    // 解析綁定位址
    let ip_addr: std::net::IpAddr = bind_ip
        .parse()
        .unwrap_or_else(|_| "127.0.0.1".parse().unwrap());
    let addr = SocketAddr::from((ip_addr, port));

    // 動態產生 self-signed 憑證並以 HTTPS 啟動
    let (cert_pem, key_pem) = match generate_self_signed(&bind_ip) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to generate self-signed cert: {}", e);
            // fallback 再試一次只用 localhost
            generate_self_signed("127.0.0.1").expect("cert generation failed")
        }
    };

    let config = RustlsConfig::from_pem(cert_pem, key_pem)
        .await
        .expect("Failed to create RustlsConfig from PEM");

    tracing::info!("CMS-Rust https://{} (self-signed)", addr);
    tracing::info!(
        "content.htm => {}",
        base_dir.join("config").join("content.htm").display()
    );

    axum_server::bind_rustls(addr, config)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
