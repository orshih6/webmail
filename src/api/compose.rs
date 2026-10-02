use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};

use super::MAX_UPLOAD;
use crate::{
    AppState,
    compose::{self, FileData, InlineImage, Sender},
    error::{AppError, AppResult},
    imap::{Conn, ops},
    mime,
    session::Auth,
    settings,
    types::{ComposeRequest, DraftSaved, MessageRef, Special, Upload},
};

fn used_uploads(req: &ComposeRequest) -> Vec<String> {
    req.uploads
        .iter()
        .chain(&req.inline_uploads)
        .cloned()
        .collect()
}

fn new_id() -> String {
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        rand::random::<[u8; 16]>(),
    )
}

pub async fn upload(
    State(st): State<AppState>,
    auth: Auth,
    mut form: Multipart,
) -> AppResult<Json<Upload>> {
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field
            .file_name()
            .map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n).trim().to_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "attachment".to_owned());
        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_owned();
        let data = field
            .bytes()
            .await
            .map_err(|_| AppError::BadRequest("upload too large (25 MB max)".into()))?;
        if data.len() > MAX_UPLOAD {
            return Err(AppError::BadRequest("upload too large (25 MB max)".into()));
        }
        let id = new_id();
        sqlx::query(
            "INSERT INTO uploads (id, session_hash, filename, content_type, data) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&id)
        .bind(&auth.id_hash)
        .bind(&filename)
        .bind(&content_type)
        .bind(data.as_ref())
        .execute(&st.db)
        .await?;
        return Ok(Json(Upload {
            id,
            filename,
            content_type,
            size: data.len() as u32,
        }));
    }
    Err(AppError::BadRequest("no file".into()))
}

pub async fn remove_upload(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    sqlx::query("DELETE FROM uploads WHERE id = $1 AND session_hash = $2")
        .bind(&id)
        .bind(&auth.id_hash)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Uploads plus any attachments kept from an existing message, in that message's order.
async fn gather_files(
    st: &AppState,
    auth: &Auth,
    c: &mut Conn,
    req: &ComposeRequest,
) -> AppResult<Vec<FileData>> {
    let mut files = Vec::new();
    if let Some(keep) = &req.keep
        && !keep.indices.is_empty()
    {
        ops::select(c.s(), &keep.source.folder).await?;
        let (raw, _) = ops::raw(c.s(), keep.source.uid, true).await?;
        for &i in &keep.indices {
            let a = mime::attachment(&raw, i).ok_or(AppError::NotFound)?;
            files.push(FileData {
                filename: a.filename,
                content_type: a.content_type,
                data: a.data,
            });
        }
    }
    if !req.uploads.is_empty() {
        let rows: Vec<(String, String, String, Vec<u8>)> = sqlx::query_as(
            "SELECT id, filename, content_type, data FROM uploads WHERE session_hash = $1 AND id = ANY($2)",
        )
        .bind(&auth.id_hash)
        .bind(&req.uploads)
        .fetch_all(&st.db)
        .await?;
        for id in &req.uploads {
            let (_, filename, content_type, data) =
                rows.iter().find(|r| &r.0 == id).cloned().ok_or_else(|| {
                    AppError::BadRequest("an attachment expired; add it again".into())
                })?;
            files.push(FileData {
                filename,
                content_type,
                data,
            });
        }
    }
    let total: usize = files.iter().map(|f| f.data.len()).sum();
    if total > MAX_UPLOAD {
        return Err(AppError::BadRequest(
            "attachments exceed 25 MB in total".into(),
        ));
    }
    Ok(files)
}

async fn build(
    st: &AppState,
    auth: &Auth,
    req: &ComposeRequest,
    files: &[FileData],
    need_recipients: bool,
) -> AppResult<compose::Outgoing> {
    let inline = gather_inline(st, auth, req).await?;
    let id = settings::identity_for_send(&st.db, &auth.email, req.identity).await?;
    let sender = Sender {
        app_name: &st.config.app_name,
        email: &auth.email,
        name: &id.name,
        reply_to: &id.reply_to,
    };
    compose::build(&sender, req, files, &inline, need_recipients)
}

pub async fn send(
    State(st): State<AppState>,
    auth: Auth,
    Json(req): Json<ComposeRequest>,
) -> AppResult<StatusCode> {
    let mut c = st.pool.get(&auth).await?;
    let files = gather_files(&st, &auth, &mut c, &req).await?;
    let out = build(&st, &auth, &req, &files, true).await?;
    // Hold no IMAP lock across SMTP; the connection is released first.
    c.release();

    compose::send(st.pool.config(), &auth.email, &auth.password, &out.wire).await?;

    // The message has left. Bookkeeping failures below are logged, not reported as a failed
    // send — retrying would send it twice.
    if let Err(e) = after_send(&st, &auth, &req, &out.stored).await {
        tracing::warn!(error = %e, "post-send bookkeeping failed");
    }
    settings::collect_recipients(&st.db, &auth.email, &out.recipients).await;
    sqlx::query("DELETE FROM uploads WHERE session_hash = $1 AND id = ANY($2)")
        .bind(&auth.id_hash)
        .bind(used_uploads(&req))
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn after_send(
    st: &AppState,
    auth: &Auth,
    req: &ComposeRequest,
    stored: &[u8],
) -> AppResult<()> {
    let mut c = st.pool.get(auth).await?;
    let sp = ops::specials(c.s()).await?;
    if let Some(sent) = sp.get(Special::Sent) {
        ops::append(c.s(), sent, "(\\Seen)", stored).await?;
    }
    for (r, flag) in [
        (&req.reply_of, "\\Answered"),
        (&req.forward_of, "$Forwarded"),
    ] {
        if let Some(r) = r {
            ops::select(c.s(), &r.folder).await?;
            ops::set_flag(c.s(), &[r.uid], flag, true).await?;
        }
    }
    if let Some(d) = &req.draft {
        ops::select(c.s(), &d.folder).await?;
        ops::expunge(c.s(), &[d.uid]).await?;
    }
    c.release();
    Ok(())
}

/// Saves (or replaces) the draft and returns where it now lives.
pub async fn save_draft(
    State(st): State<AppState>,
    auth: Auth,
    Json(req): Json<ComposeRequest>,
) -> AppResult<Json<DraftSaved>> {
    let mut c = st.pool.get(&auth).await?;
    let files = gather_files(&st, &auth, &mut c, &req).await?;
    let out = build(&st, &auth, &req, &files, false).await?;
    let sp = ops::specials(c.s()).await?;
    let drafts = sp.require(Special::Drafts)?.to_owned();

    ops::append(c.s(), &drafts, "(\\Seen \\Draft)", &out.stored).await?;
    ops::select(c.s(), &drafts).await?;
    let uid = ops::find_by_message_id(c.s(), &format!("<{}>", out.message_id)).await?;
    if let Some(old) = &req.draft
        && Some(old.uid) != uid
    {
        ops::select(c.s(), &old.folder).await?;
        ops::expunge(c.s(), &[old.uid]).await?;
    }
    c.release();
    // The draft now carries these files; the client refers to them through it from here on.
    sqlx::query("DELETE FROM uploads WHERE session_hash = $1 AND id = ANY($2)")
        .bind(&auth.id_hash)
        .bind(&req.uploads)
        .execute(&st.db)
        .await?;
    // Inline images stay: the editor still shows them from /api/uploads until send.
    Ok(Json(DraftSaved {
        draft: uid.map(|uid| MessageRef {
            folder: drafts,
            uid,
        }),
    }))
}

/// Pasted images for the HTML body. Only previewable raster images qualify.
async fn gather_inline(
    st: &AppState,
    auth: &Auth,
    req: &ComposeRequest,
) -> AppResult<Vec<InlineImage>> {
    if req.inline_uploads.is_empty() || req.html.is_none() {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT id, content_type, data FROM uploads WHERE session_hash = $1 AND id = ANY($2)",
    )
    .bind(&auth.id_hash)
    .bind(&req.inline_uploads)
    .fetch_all(&st.db)
    .await?;
    let total: usize = rows.iter().map(|r| r.2.len()).sum();
    if total > MAX_UPLOAD {
        return Err(AppError::BadRequest("images exceed 25 MB in total".into()));
    }
    Ok(rows
        .into_iter()
        .filter(|r| mime::previewable(&r.1))
        .map(|(cid, content_type, data)| InlineImage {
            cid,
            content_type,
            data,
        })
        .collect())
}

/// Lets the editor show an image the user just pasted. Own uploads only, images only.
pub async fn get_upload(
    State(st): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> AppResult<axum::response::Response> {
    let row: Option<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT content_type, data FROM uploads WHERE id = $1 AND session_hash = $2",
    )
    .bind(&id)
    .bind(&auth.id_hash)
    .fetch_optional(&st.db)
    .await?;
    let (ct, data) = row.ok_or(AppError::NotFound)?;
    if !mime::previewable(&ct) {
        return Err(AppError::NotFound);
    }
    axum::response::Response::builder()
        .header(axum::http::header::CONTENT_TYPE, ct)
        .header(axum::http::header::CACHE_CONTROL, "private, max-age=3600")
        .header(
            axum::http::header::CONTENT_SECURITY_POLICY,
            "sandbox; default-src 'none'",
        )
        .body(axum::body::Body::from(data))
        .map_err(|e| AppError::Internal(e.into()))
}
