use anyhow::Result;
use tracing_subscriber::EnvFilter;
use webmail::{AppState, app, config::Config, db};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env()?;
    if config.tls_accept_invalid_certs {
        tracing::warn!("TLS_ACCEPT_INVALID_CERTS=1: mail server certificates are NOT verified");
    }
    let db = db::connect(&config.database_url).await?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    tracing::info!(addr = %config.listen, "listening");

    let state = AppState::new(config, db);
    state.spawn_sweeper();
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        _ = ctrl_c => {},
        _ = term.recv() => {},
    }
}
