mod auth;
mod compose;
mod events;
mod mail;
mod settings;

use std::time::Duration;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Serialize;

use crate::{AppState, error::AppError};

/// Per-upload and per-message attachment limit (most providers cap mail at ~25 MB).
pub const MAX_UPLOAD: usize = 25 * 1024 * 1024;

/// Gives up on a request that waits on the mail server for too long. The handler future is
/// dropped, which drops its IMAP connection unreleased, so the next request reconnects
/// instead of reusing a connection stuck mid-command.
async fn deadline(State(limit): State<Duration>, req: Request, next: Next) -> Response {
    let sending = req.uri().path().ends_with("/send");
    match tokio::time::timeout(limit, next.run(req)).await {
        Ok(res) => res,
        // A send cut off mid-SMTP may still have gone out: say so, so nobody blindly resends.
        Err(_) if sending => AppError::Unavailable(
            "the mail server stopped responding while sending; check Sent before trying again"
                .into(),
        )
        .into_response(),
        Err(_) => AppError::Unavailable("the mail server is not responding".into()).into_response(),
    }
}

pub fn router(state: &AppState) -> Router<AppState> {
    let quick = state.config.mail_timeout;
    let slow = quick * 4;

    // Everything that talks to the mail server, with the normal deadline.
    let mail = Router::new()
        .route("/folders", get(mail::folders))
        .route("/folders/empty", post(mail::empty_folder))
        .route("/folders/create", post(mail::create_folder))
        .route("/folders/rename", post(mail::rename_folder))
        .route("/folders/delete", post(mail::delete_folder))
        .route("/folders/read", post(mail::mark_folder_read))
        .route("/messages", get(mail::list))
        .route("/message", get(mail::message))
        .route("/thread", get(mail::thread))
        .route("/messages/flag", post(mail::flag))
        .route("/messages/move", post(mail::move_messages))
        .route("/messages/delete", post(mail::delete))
        .route("/messages/junk", post(mail::junk))
        .route("/login", post(auth::login))
        .route_layer(middleware::from_fn_with_state(quick, deadline));

    // Big transfers: sending, attachments, whole messages.
    let transfers = Router::new()
        .route("/attachment", get(mail::attachment))
        .route("/message/raw", get(mail::raw))
        .route("/message/print", get(mail::print))
        .route(
            "/uploads",
            post(compose::upload).layer(DefaultBodyLimit::max(MAX_UPLOAD + 64 * 1024)),
        )
        .route("/search", get(mail::search_all))
        .route("/send", post(compose::send))
        .route("/drafts", post(compose::save_draft))
        .route_layer(middleware::from_fn_with_state(slow, deadline));

    Router::new()
        .merge(mail)
        .merge(transfers)
        .route("/health", get(health))
        .route("/config", get(public_config))
        .route("/logout", post(auth::logout))
        .route("/session", get(auth::session))
        .route(
            "/uploads/{id}",
            delete(compose::remove_upload).get(compose::get_upload),
        )
        // Long-lived by design; reconnects on its own.
        .route("/events", get(events::events))
        .route("/prefs", get(settings::get_prefs).post(settings::set_prefs))
        .route(
            "/identities",
            get(settings::identities).post(settings::save_identity),
        )
        .route("/identities/{id}", delete(settings::delete_identity))
        .route(
            "/contacts",
            get(settings::contacts).post(settings::save_contact),
        )
        .route("/contacts/{id}", delete(settings::delete_contact))
        .fallback(|| async { StatusCode::NOT_FOUND })
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

/// What the UI needs before sign-in. Public: nothing here is sensitive.
async fn public_config(
    axum::extract::State(st): axum::extract::State<AppState>,
) -> Json<crate::types::PublicConfig> {
    Json(crate::types::PublicConfig {
        app_name: st.config.app_name.clone(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        license: env!("CARGO_PKG_LICENSE").to_owned(),
        source_url: st.config.source_url.clone(),
    })
}

async fn health() -> impl IntoResponse {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}
