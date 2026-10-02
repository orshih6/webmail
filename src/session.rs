//! Sessions. The cookie carries `id ‖ key` (16 + 32 random bytes). Postgres stores only
//! SHA-256(id) and the IMAP password encrypted under `key`, so neither a database dump nor
//! the cookie name alone is enough to recover a password or hijack a session.

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, Method, request::Parts},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::{
    AppState,
    error::{AppError, AppResult},
};

pub const COOKIE: &str = "wm_session";
pub const CSRF_HEADER: &str = "x-csrf-token";
/// A session idle for longer than this is gone.
pub const IDLE_HOURS: i32 = 12;

/// A signed-in user, extracted from the cookie. Mutating requests are CSRF-checked during
/// extraction, so a handler that takes `Auth` never has to think about it.
#[derive(Clone)]
pub struct Auth {
    /// Hex of SHA-256(id): identifies this one web session.
    pub sid: String,
    /// Hex of SHA-256(email ‖ password): the key for shared IMAP connections. Every session
    /// of a user with the same credentials shares one cached connection and one IDLE
    /// watcher — Dovecot caps connections per user and IP (mail_max_userip_connections,
    /// default 10), and all webmail traffic arrives from one IP. A session holding an old
    /// password never gets a connection opened with the new one.
    pub ukey: String,
    pub id_hash: Vec<u8>,
    pub email: String,
    pub password: String,
    pub csrf: String,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth")
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

pub struct NewSession {
    pub cookie: Cookie<'static>,
    pub auth: Auth,
}

pub async fn create(
    db: &PgPool,
    email: &str,
    password: &str,
    secure: bool,
) -> AppResult<NewSession> {
    let id: [u8; 16] = rand::random();
    let key: [u8; 32] = rand::random();
    let nonce: [u8; 12] = rand::random();
    let csrf = B64.encode(rand::random::<[u8; 24]>());
    let id_hash = Sha256::digest(id).to_vec();

    let secret = cipher(&key)
        .encrypt(&Nonce::from(nonce), password.as_bytes())
        .map_err(|_| anyhow::anyhow!("encrypt session secret"))?;

    sqlx::query(
        "INSERT INTO sessions (id_hash, email, nonce, secret, csrf) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&id_hash)
    .bind(email)
    .bind(&nonce[..])
    .bind(&secret)
    .bind(&csrf)
    .execute(db)
    .await?;

    let mut token = id.to_vec();
    token.extend_from_slice(&key);
    let cookie = Cookie::build((COOKIE, B64.encode(token)))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Strict)
        .build();

    Ok(NewSession {
        cookie,
        auth: Auth {
            sid: hex(&id_hash),
            ukey: user_key(email, password),
            id_hash,
            email: email.to_owned(),
            password: password.to_owned(),
            csrf,
        },
    })
}

pub async fn destroy(db: &PgPool, auth: &Auth) -> AppResult<()> {
    sqlx::query("DELETE FROM sessions WHERE id_hash = $1")
        .bind(&auth.id_hash)
        .execute(db)
        .await?;
    Ok(())
}

pub fn removal_cookie() -> Cookie<'static> {
    let mut c = Cookie::build((COOKIE, "")).path("/").build();
    c.make_removal();
    c
}

/// Deletes idle sessions (their uploads cascade). Run periodically.
pub async fn sweep(db: &PgPool) -> AppResult<u64> {
    let r =
        sqlx::query("DELETE FROM sessions WHERE last_seen_at < now() - make_interval(hours => $1)")
            .bind(IDLE_HOURS)
            .execute(db)
            .await?;
    Ok(r.rows_affected())
}

async fn load(db: &PgPool, token: &str) -> AppResult<Auth> {
    let raw = B64.decode(token).map_err(|_| AppError::Unauthorized)?;
    if raw.len() != 48 {
        return Err(AppError::Unauthorized);
    }
    let (id, key) = raw.split_at(16);
    let id_hash = Sha256::digest(id).to_vec();

    let row: Option<(String, Vec<u8>, Vec<u8>, String)> = sqlx::query_as(
        "UPDATE sessions SET last_seen_at = now()
         WHERE id_hash = $1 AND last_seen_at > now() - make_interval(hours => $2)
         RETURNING email, nonce, secret, csrf",
    )
    .bind(&id_hash)
    .bind(IDLE_HOURS)
    .fetch_optional(db)
    .await?;
    let (email, nonce, secret, csrf) = row.ok_or(AppError::Unauthorized)?;

    let nonce: [u8; 12] = nonce.try_into().map_err(|_| AppError::Unauthorized)?;
    let password = cipher(key)
        .decrypt(&Nonce::from(nonce), secret.as_slice())
        .ok()
        .and_then(|p| String::from_utf8(p).ok())
        .ok_or(AppError::Unauthorized)?;

    Ok(Auth {
        sid: hex(&id_hash),
        ukey: user_key(&email, &password),
        id_hash,
        email,
        password,
        csrf,
    })
}

fn cipher(key: &[u8]) -> Aes256Gcm {
    Aes256Gcm::new_from_slice(key).expect("key is 32 bytes")
}

fn user_key(email: &str, password: &str) -> String {
    let mut h = Sha256::new();
    h.update(email.as_bytes());
    h.update([0]);
    h.update(password.as_bytes());
    hex(&h.finalize())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn csrf_ok(method: &Method, headers: &HeaderMap, expected: &str) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|got| constant_time_eq(got.as_bytes(), expected.as_bytes()))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> AppResult<Self> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar.get(COOKIE).ok_or(AppError::Unauthorized)?;
        let auth = load(&state.db, token.value()).await?;
        if !csrf_ok(&parts.method, &parts.headers, &auth.csrf) {
            return Err(AppError::Csrf);
        }
        Ok(auth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csrf_required_for_mutations_only() {
        let mut h = HeaderMap::new();
        assert!(csrf_ok(&Method::GET, &h, "tok"));
        assert!(!csrf_ok(&Method::POST, &h, "tok"));
        h.insert(CSRF_HEADER, "nope".parse().unwrap());
        assert!(!csrf_ok(&Method::POST, &h, "tok"));
        h.insert(CSRF_HEADER, "tok".parse().unwrap());
        assert!(csrf_ok(&Method::POST, &h, "tok"));
        assert!(csrf_ok(&Method::DELETE, &h, "tok"));
    }

    #[test]
    fn secret_roundtrip_and_wrong_key_fails() {
        let key: [u8; 32] = rand::random();
        let nonce: [u8; 12] = rand::random();
        let ct = cipher(&key)
            .encrypt(&Nonce::from(nonce), b"hunter2".as_ref())
            .unwrap();
        let pt = cipher(&key)
            .decrypt(&Nonce::from(nonce), ct.as_slice())
            .unwrap();
        assert_eq!(pt, b"hunter2");
        let other: [u8; 32] = rand::random();
        assert!(
            cipher(&other)
                .decrypt(&Nonce::from(nonce), ct.as_slice())
                .is_err()
        );
    }
}
