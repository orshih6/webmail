use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use tower::ServiceExt;
use webmail::{AppState, app, config::Config};

fn test_app() -> axum::Router {
    // These routes never touch the database, so a lazy pool to nowhere is fine.
    let db = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    let config = Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        database_url: String::new(),
        app_name: "Webmail".into(),
        source_url: "https://example.test/src".into(),
        imap_host: "127.0.0.1".into(),
        imap_port: 1,
        smtp_host: "127.0.0.1".into(),
        smtp_port: 1,
        tls_accept_invalid_certs: false,
        cookie_secure: true,
        mail_timeout: std::time::Duration::from_secs(30),
    };
    app(AppState::new(config, db))
}

async fn get(path: &str) -> (u16, String, String) {
    let res = test_app()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status().as_u16();
    let ctype = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let body =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    (status, ctype, body)
}

#[tokio::test]
async fn health_reports_ok() {
    let (status, _, body) = get("/api/health").await;
    assert_eq!(status, 200);
    assert!(body.contains(r#""status":"ok""#));
}

#[tokio::test]
async fn unknown_api_path_is_404_not_spa() {
    let (status, _, _) = get("/api/nope").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn client_routes_fall_back_to_index() {
    let (status, ctype, body) = get("/mail/INBOX/42").await;
    assert_eq!(status, 200);
    assert!(ctype.starts_with("text/html"));
    assert!(body.contains("<html"));
}

#[tokio::test]
async fn api_requires_a_session() {
    let (status, _, body) = get("/api/folders").await;
    assert_eq!(status, 401);
    assert!(body.contains("not signed in"));
}

#[tokio::test]
async fn pages_cannot_be_framed() {
    let res = test_app()
        .oneshot(Request::get("/mail").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        res.headers()["content-security-policy"],
        "frame-ancestors 'none'"
    );
}

#[tokio::test]
async fn manifest_and_config_carry_the_brand() {
    let (status, ctype, body) = get("/manifest.webmanifest").await;
    assert_eq!(status, 200);
    assert_eq!(ctype, "application/manifest+json");
    assert!(body.contains(r#""name":"Webmail""#));
    let (_, _, body) = get("/api/config").await;
    assert!(body.contains(r#""license":"AGPL-3.0-only""#), "{body}");
    assert!(body.contains("example.test/src"));
}
