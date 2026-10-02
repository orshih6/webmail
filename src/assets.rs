//! Serves the Svelte build embedded at compile time. Any path that is not a real file gets
//! `index.html` so client-side routes (`/mail/INBOX/42`) survive a reload.

use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/build/"]
struct Web;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    match Web::get(path) {
        Some(file) if !path.is_empty() => file_response(path, file),
        _ => match Web::get("index.html") {
            Some(file) => file_response("index.html", file),
            None => (
                StatusCode::NOT_FOUND,
                "web/build is missing — run `pnpm build` in web/",
            )
                .into_response(),
        },
    }
}

fn file_response(path: &str, file: rust_embed::EmbeddedFile) -> Response {
    // Vite fingerprints everything under _app/immutable, so those never change for a URL.
    let cache = if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .header(header::CONTENT_TYPE, file.metadata.mimetype())
        .header(header::CACHE_CONTROL, cache)
        .body(Body::from(file.data))
        .expect("static headers are valid")
}
