use std::{net::SocketAddr, time::Duration};

use anyhow::{Context, Result};

/// Runtime configuration, read once from the environment at startup.
#[derive(Clone, Debug)]
pub struct Config {
    pub listen: SocketAddr,
    pub database_url: String,
    /// Shown in the UI and the User-Agent header of sent mail.
    pub app_name: String,
    /// Where users can get this program's source (AGPL-3.0 §13). Point it at your fork if
    /// you run a modified version.
    pub source_url: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    /// Dev only (dev/compose.yml uses a self-signed cert). Never set in the cluster.
    pub tls_accept_invalid_certs: bool,
    /// Off only for plain-http local development.
    pub cookie_secure: bool,
    /// How long an API call may wait on the mail server before giving up (MAIL_TIMEOUT_SECS).
    /// Sending and attachment transfers get four times this.
    pub mail_timeout: Duration,
    /// Memory for parsed, sanitized messages (MESSAGE_CACHE_MB; 0 disables).
    pub message_cache_bytes: usize,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            listen: var_or("LISTEN_ADDR", "0.0.0.0:8080")
                .parse()
                .context("LISTEN_ADDR")?,
            database_url: required("DATABASE_URL")?,
            app_name: var_or("APP_NAME", "Webmail"),
            source_url: var_or("SOURCE_URL", "https://github.com/orshih6/webmail"),
            // Implicit TLS only (IMAPS / SMTPS): credentials never cross a plaintext socket.
            imap_host: required("IMAP_HOST")?,
            imap_port: var_or("IMAP_PORT", "993").parse().context("IMAP_PORT")?,
            smtp_host: required("SMTP_HOST")?,
            smtp_port: var_or("SMTP_PORT", "465").parse().context("SMTP_PORT")?,
            tls_accept_invalid_certs: var_or("TLS_ACCEPT_INVALID_CERTS", "0") == "1",
            cookie_secure: var_or("COOKIE_SECURE", "1") != "0",
            message_cache_bytes: var_or("MESSAGE_CACHE_MB", "64")
                .parse::<usize>()
                .context("MESSAGE_CACHE_MB")?
                * 1024
                * 1024,
            mail_timeout: Duration::from_secs(
                var_or("MAIL_TIMEOUT_SECS", "30")
                    .parse()
                    .context("MAIL_TIMEOUT_SECS")?,
            ),
        })
    }
}

fn required(key: &str) -> Result<String> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .with_context(|| format!("{key} is required"))
}

fn var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}
