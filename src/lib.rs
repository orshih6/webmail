pub mod api;
pub mod assets;
pub mod compose;
pub mod config;
pub mod db;
pub mod error;
pub mod imap;
pub mod mime;
pub mod ratelimit;
pub mod session;
pub mod settings;
pub mod types;

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    http::{HeaderValue, header},
};
use sqlx::PgPool;
use tower_http::{
    compression::CompressionLayer,
    set_header::SetResponseHeaderLayer,
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, TraceLayer},
};

use config::Config;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: PgPool,
    pub pool: Arc<imap::Pool>,
    pub live: Arc<imap::idle::Live>,
    pub limiter: Arc<ratelimit::LoginLimiter>,
}

impl AppState {
    pub fn new(config: Config, db: PgPool) -> Self {
        let config = Arc::new(config);
        Self {
            pool: Arc::new(imap::Pool::new(config.clone())),
            live: Arc::default(),
            limiter: Arc::default(),
            config,
            db,
        }
    }

    /// Periodic housekeeping: idle IMAP connections, expired sessions, limiter entries.
    pub fn spawn_sweeper(&self) {
        let st = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            loop {
                tick.tick().await;
                st.pool.reap().await;
                st.limiter.sweep();
                if let Err(e) = session::sweep(&st.db).await {
                    tracing::warn!(error = %e, "session sweep failed");
                }
            }
        });
    }
}

/// The web app manifest, generated so an installed app carries this deployment's name.
async fn manifest(
    axum::extract::State(st): axum::extract::State<AppState>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let body = serde_json::json!({
        "name": st.config.app_name,
        "short_name": st.config.app_name,
        "start_url": "/mail?f=INBOX",
        "scope": "/",
        "display": "standalone",
        "background_color": "#ffffff",
        "theme_color": "#3b5bdb",
        "icons": [
            { "src": "/icons/logo.svg", "sizes": "any", "type": "image/svg+xml" },
            { "src": "/icons/icon-192.png", "sizes": "192x192", "type": "image/png" },
            { "src": "/icons/icon-512.png", "sizes": "512x512", "type": "image/png" },
            { "src": "/icons/icon-maskable-512.png", "sizes": "512x512", "type": "image/png", "purpose": "maskable" }
        ]
    });
    (
        [(header::CONTENT_TYPE, "application/manifest+json")],
        body.to_string(),
    )
        .into_response()
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .nest("/api", api::router(&state))
        .route("/manifest.webmanifest", axum::routing::get(manifest))
        .fallback(assets::serve)
        .with_state(state)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            // The page policy (with the hash of SvelteKit's inline bootstrap) ships as a <meta>
            // tag in index.html, set in web/vite.config.ts. A <meta> policy cannot express
            // frame-ancestors, so that one lives here. `if_not_present`: responses that carry
            // message bytes (attachments, source, print) set a stricter policy of their own,
            // and overriding it would silently drop their sandbox.
            HeaderValue::from_static("frame-ancestors 'none'"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(300),
        ))
        // INFO-level request spans, so every log line (warnings included) says which request
        // it belongs to; the default DEBUG span is filtered out in production.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(tracing::Level::INFO)),
        )
}
