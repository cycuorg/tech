
mod auth;
mod content;
mod routes;
mod templates;

use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use axum_server::tls_rustls::RustlsConfig;

use rcgen::{
    BasicConstraints,
    CertificateParams,
    DistinguishedName,
    DnType,
    ExtendedKeyUsagePurpose,
    IsCa,
    Issuer,
    KeyPair,
    KeyUsagePurpose,
    SanType,
};

use time::{Duration, OffsetDateTime};

use tower_http::cors::CorsLayer;

use tracing_subscriber::{
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

#[derive(Clone)]
pub struct AppState {
    pub base_dir: PathBuf,
    pub ip: String,
    pub port: u16,
    pub content_path: PathBuf,
}

// =====================================================
// 1. Read setup.ini
// =====================================================

fn load_setup(
    base_dir: &Path,
) -> (String, u16, Vec<String>) {
    let path = base_dir.join("config").join("setup.ini");

    let mut ip = "127.0.0.1".to_string();
    let mut port: u16 = 5000;

    let mut san_list = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ];

    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            let line = line.trim();

            if line.is_empty()
                || line.starts_with('#')
                || line.starts_with(';')
            {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_lowercase();

                let value = value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .trim();

                match key.as_str() {
                    "ip" if !value.is_empty() => {
                        ip = value.to_string();
                    }

                    "port" => {
                        if let Ok(p) = value.parse::<u16>() {
                            if p != 0 {
                                port = p;
                            }
                        }
                    }

                    "san" => {
                        san_list = value
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }

                    _ => {}
                }
            }
        }
    } else {
        std::fs::create_dir_all(
            path.parent().unwrap()
        ).ok();

        let default = r#"ip = "127.0.0.1"
port = 5000
san = "localhost,127.0.0.1"
"#;

        std::fs::write(&path, default).ok();

        tracing::info!("Created default setup.ini");
    }

    if let Ok(value) = std::env::var("CMS_IP") {
        if !value.trim().is_empty() {
            ip = value.trim().to_string();
        }
    }

    if let Ok(value) = std::env::var("CMS_PORT") {
        if let Ok(p) = value.parse::<u16>() {
            if p != 0 {
                port = p;
            }
        }
    }

    if let Ok(value) = std::env::var("CMS_SAN") {
        san_list = value
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    tracing::info!(
        "CMS configuration: ip={}, port={}, san={:?}",
        ip,
        port,
        san_list
    );

    (ip, port, san_list)
}

// =====================================================
// 2. Application root directory
// =====================================================

fn get_application_root()
    -> Result<PathBuf, Box<dyn std::error::Error>>
{
    let exe_path = std::env::current_exe()?;

    let exe_dir = exe_path
        .parent()
        .ok_or("Cannot determine executable directory")?;

    let app_root = exe_dir
        .parent()
        .ok_or("Cannot determine parent directory")?;

    Ok(app_root.to_path_buf())
}

// =====================================================
// 3. Certificate directory
// =====================================================

fn get_certificate_dir(
    app_root: &Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {

    let cert_dir = app_root.join("certs");

    std::fs::create_dir_all(&cert_dir)?;

    Ok(cert_dir)
}

// =====================================================
// 4. Private key permissions
// =====================================================

#[cfg(unix)]
fn restrict_private_key(
    path: &Path,
) -> std::io::Result<()> {

    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(0o600),
    )
}

#[cfg(not(unix))]
fn restrict_private_key(
    _path: &Path,
) -> std::io::Result<()> {
    Ok(())
}

// =====================================================
// 5. Create Root CA
// =====================================================

fn create_root_ca()
    -> Result<(String, String), Box<dyn std::error::Error>>
{
    let mut params = CertificateParams::default();

    params.distinguished_name = DistinguishedName::new();

    params.distinguished_name.push(
        DnType::CommonName,
        "CMS-Rust Root CA",
    );

    params.distinguished_name.push(
        DnType::OrganizationName,
        "CMS-Rust",
    );

    params.distinguished_name.push(
        DnType::CountryName,
        "TW",
    );

    params.is_ca = IsCa::Ca(
        BasicConstraints::Unconstrained,
    );

    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];

    let now = OffsetDateTime::now_utc();

    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(3650);

    let ca_key = KeyPair::generate()?;

    let ca_cert = params.self_signed(&ca_key)?;

    Ok((
        ca_cert.pem(),
        ca_key.serialize_pem(),
    ))
}

// =====================================================
// 6. Load or create Root CA
// =====================================================

fn load_or_create_root_ca(
    cert_dir: &Path,
) -> Result<Issuer<'static, KeyPair>, Box<dyn std::error::Error>>
{
    let ca_cert_path = cert_dir.join("ca_cert.pem");
    let ca_key_path = cert_dir.join("ca_key.pem");

    let cert_exists = ca_cert_path.exists();
    let key_exists = ca_key_path.exists();

    let (cert_pem, key_pem) = match (cert_exists, key_exists) {

        (true, true) => {
            tracing::info!("Loading existing Root CA");

            (
                std::fs::read_to_string(&ca_cert_path)?,
                std::fs::read_to_string(&ca_key_path)?,
            )
        }

        (false, false) => {
            tracing::info!("Creating new Root CA");

            let (cert, key) = create_root_ca()?;

            std::fs::write(&ca_cert_path, &cert)?;
            std::fs::write(&ca_key_path, &key)?;

            restrict_private_key(&ca_key_path)?;

            (cert, key)
        }

        _ => {
            return Err(
                "Root CA certificate or private key is missing. \
                 Restore the matching CA files before continuing."
                    .into()
            );
        }
    };

    let ca_key = KeyPair::from_pem(&key_pem)?;

    // Verify that the certificate and private key match.
    let ca_cert_params =
        CertificateParams::from_ca_cert_pem(&cert_pem)?;

    let test_key = KeyPair::generate()?;

    let test_cert = ca_cert_params
        .clone()
        .signed_by(
            &test_key,
            &Issuer::from_ca_cert_pem(
                &cert_pem,
                KeyPair::from_pem(&key_pem)?,
            )?,
        );

    // A successful signing operation verifies that the CA
    // private key can be used by rcgen with this CA certificate.
    test_cert?;

    // Reconstruct issuer for use by server certificate generation.
    let issuer = Issuer::from_ca_cert_pem(
        &cert_pem,
        ca_key,
    )?;

    tracing::info!("Root CA loaded and signing verified");

    Ok(issuer)
}

// =====================================================
// 7. Build Subject Alternative Names
// =====================================================

fn build_server_sans(
    bind_ip: &str,
    configured_sans: &[String],
) -> Result<Vec<SanType>, Box<dyn std::error::Error>> {

    let mut names = BTreeSet::<String>::new();

    names.insert("localhost".to_string());
    names.insert("127.0.0.1".to_string());

    if let Ok(addr) = bind_ip.parse::<IpAddr>() {

        if !addr.is_unspecified() {
            names.insert(addr.to_string());
        }

    } else {
        names.insert(bind_ip.to_string());
    }

    for item in configured_sans {

        let name = item.trim();

        if name.is_empty() {
            continue;
        }

        if let Ok(addr) = name.parse::<IpAddr>() {

            if addr.is_unspecified() {
                return Err(
                    format!(
                        "Unspecified IP cannot be used as SAN: {}",
                        name
                    ).into()
                );
            }

            names.insert(addr.to_string());

        } else {

            if name.contains("://")
                || name.contains('/')
                || name.contains(':')
                || name.contains('*')
            {
                return Err(
                    format!("Invalid DNS SAN: {}", name).into()
                );
            }

            names.insert(name.to_lowercase());
        }
    }

    let mut sans = Vec::new();

    for name in names {

        if let Ok(addr) = name.parse::<IpAddr>() {

            sans.push(SanType::IpAddress(addr));

        } else {

            sans.push(
                SanType::DnsName(name.try_into()?)
            );
        }
    }

    tracing::info!(
        "Generated SAN entries: {:?}",
        sans
    );

    Ok(sans)
}

// =====================================================
// 8. Generate HTTPS Server Certificate
// =====================================================

fn generate_server_certificate(
    bind_ip: &str,
    configured_sans: &[String],
    ca_issuer: &Issuer<'_, KeyPair>,
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {

    let mut params = CertificateParams::default();

    params.distinguished_name = DistinguishedName::new();

    params.distinguished_name.push(
        DnType::CommonName,
        "CMS-Rust HTTPS Server",
    );

    params.distinguished_name.push(
        DnType::OrganizationName,
        "CMS-Rust",
    );

    params.is_ca = IsCa::NoCa;

    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
    ];

    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ServerAuth,
    ];

    params.use_authority_key_identifier_extension = true;

    params.subject_alt_names = build_server_sans(
        bind_ip,
        configured_sans,
    )?;

    let now = OffsetDateTime::now_utc();

    params.not_before = now - Duration::minutes(5);
    params.not_after = now + Duration::days(90);

    let server_key = KeyPair::generate()?;

    let server_cert = params.signed_by(
        &server_key,
        ca_issuer,
    )?;

    Ok((
        server_cert.pem().into_bytes(),
        server_key.serialize_pem().into_bytes(),
    ))
}

// =====================================================
// 9. Main
// =====================================================

#[tokio::main]
async fn main() {

    // Logging

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(
                    |_| "cms_rust=info,tower_http=info".into(),
                ),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Application root

    let app_root = get_application_root()
        .expect("Cannot determine application root directory");

    tracing::info!(
        "Application root: {}",
        app_root.display()
    );

    // Certificate directory

    let cert_dir = get_certificate_dir(&app_root)
        .expect("Cannot create certificate directory");

    // CMS base directory

    let current_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."));

    let base_dir = if current_dir.join("config").exists() {

        current_dir

    } else if let Ok(exe) = std::env::current_exe() {

        let parent = exe
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(current_dir.clone());

        if parent.join("config").exists() {
            parent
        } else {
            current_dir
        }

    } else {
        current_dir
    };

    for dir in &[
        "config",
        "content",
        "static",
        "images",
        "downloads",
    ] {
        std::fs::create_dir_all(
            base_dir.join(dir)
        ).ok();
    }

    let config_dir = base_dir.join("config");

    // Initialize content

    let content_path = config_dir.join("content.htm");

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

    if !config_dir.join("sitetitle").exists() {
        auth::save_sitetitle(
            &base_dir,
            "Dialog",
        );
    }

    let _ = auth::load_password_hash(&base_dir);

    // Setup

    let (bind_ip, port, san_list) =
        load_setup(&base_dir);

    // AppState

    let state = AppState {
        base_dir: base_dir.clone(),
        content_path,
        ip: bind_ip.clone(),
        port,
    };

    let app = routes::create_router(state)
        .layer(CorsLayer::permissive());

    // Bind address

    let ip_addr: IpAddr = bind_ip
        .parse()
        .unwrap_or_else(|_| {
            "127.0.0.1".parse().unwrap()
        });

    let addr = SocketAddr::from((
        ip_addr,
        port,
    ));

    // Root CA

    let ca_issuer = load_or_create_root_ca(
        &cert_dir
    ).unwrap_or_else(|e| {

        tracing::error!(
            "Root CA initialization failed: {}",
            e
        );

        panic!("Cannot initialize Root CA");
    });

    // Generate server certificate

    let (cert_pem, key_pem) =
        generate_server_certificate(
            &bind_ip,
            &san_list,
            &ca_issuer,
        )
        .unwrap_or_else(|e| {

            tracing::error!(
                "Server certificate generation failed: {}",
                e
            );

            panic!("Cannot generate HTTPS certificate");
        });

    // Save server certificate

    let server_cert_path =
        cert_dir.join("server_cert.pem");

    let server_key_path =
        cert_dir.join("server_key.pem");

    std::fs::write(
        &server_cert_path,
        &cert_pem,
    ).expect("Cannot save server certificate");

    std::fs::write(
        &server_key_path,
        &key_pem,
    ).expect("Cannot save server private key");

    restrict_private_key(
        &server_key_path
    ).expect("Cannot restrict private key permissions");

    // Rustls configuration

    let config = RustlsConfig::from_pem(
        cert_pem,
        key_pem,
    )
    .await
    .expect("Failed to create RustlsConfig");

    // Start server

    tracing::info!(
        "CMS-Rust HTTPS listening on: {}",
        addr
    );

    tracing::info!(
        "Root CA: {}",
        cert_dir.join("ca_cert.pem").display()
    );

    tracing::info!(
        "Server certificate: {}",
        server_cert_path.display()
    );

    tracing::info!(
        "Certificate SAN: {:?}",
        san_list
    );

    tracing::info!(
        "CMS config: {}",
        config_dir.display()
    );

    axum_server::bind_rustls(
        addr,
        config,
    )
    .serve(app.into_make_service())
    .await
    .unwrap();
}
