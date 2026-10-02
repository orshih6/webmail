use axum::{Json, extract::State, http::HeaderMap};
use axum_extra::extract::CookieJar;

use crate::{
    AppState,
    error::{AppError, AppResult},
    session::{self, Auth},
    types::{LoginRequest, SessionInfo},
};

/// The client address Envoy appended to X-Forwarded-For (the rightmost entry).
fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "direct".to_owned())
}

pub async fn login(
    State(st): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> AppResult<(CookieJar, Json<SessionInfo>)> {
    let email = req.email.trim().to_lowercase();
    if email.is_empty() || req.password.is_empty() || email.len() > 254 || req.password.len() > 1024
    {
        return Err(AppError::BadRequest("enter your email and password".into()));
    }
    let ip_key = format!("ip:{}", client_ip(&headers));
    let user_key = format!("user:{email}");
    if st.limiter.blocked(&ip_key) || st.limiter.blocked(&user_key) {
        return Err(AppError::RateLimited);
    }

    let imap = match st.pool.connect(&email, &req.password).await {
        Ok(s) => s,
        Err(AppError::BadLogin) => {
            st.limiter.fail(&ip_key);
            st.limiter.fail(&user_key);
            tracing::info!(ip = %ip_key, "failed login");
            return Err(AppError::BadLogin);
        }
        Err(e) => return Err(e),
    };
    st.limiter.clear(&user_key);

    let new = session::create(&st.db, &email, &req.password, st.config.cookie_secure).await?;
    st.pool.put(&new.auth.ukey, imap).await;
    Ok((
        jar.add(new.cookie),
        Json(SessionInfo {
            email: new.auth.email,
            csrf: new.auth.csrf,
        }),
    ))
}

pub async fn logout(
    State(st): State<AppState>,
    auth: Auth,
    jar: CookieJar,
) -> AppResult<CookieJar> {
    session::destroy(&st.db, &auth).await?;
    st.live.stop(&auth.ukey);
    st.pool.remove(&auth.ukey).await;
    Ok(jar.add(session::removal_cookie()))
}

pub async fn session(auth: Auth) -> Json<SessionInfo> {
    Json(SessionInfo {
        email: auth.email,
        csrf: auth.csrf,
    })
}
