use axum::{
    Json,
    body::Body,
    extract::{Query, State},
    http::{StatusCode, header},
    response::Response,
};
use serde::Deserialize;

use crate::{
    AppState,
    error::{AppError, AppResult},
    imap::ops,
    mime,
    session::Auth,
    settings,
    types::{
        CreateFolderRequest, FlagRequest, Folder, FolderRequest, MessageDetail, MessagePage,
        MoveRequest, MoveResult, RemoteImages, RenameFolderRequest, SearchHit, SearchResults,
        Special, Thread, UidsRequest,
    },
};

pub async fn folders(State(st): State<AppState>, auth: Auth) -> AppResult<Json<Vec<Folder>>> {
    let mut c = st.pool.get(&auth).await?;
    let out = ops::folders(c.s()).await?;
    c.release();
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct ListQuery {
    folder: String,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    page_size: Option<u32>,
    #[serde(default)]
    q: Option<String>,
    /// "unread" or "flagged".
    #[serde(default)]
    filter: Option<String>,
}

pub async fn list(
    State(st): State<AppState>,
    auth: Auth,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<MessagePage>> {
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).clamp(10, 200);
    let criteria = filtered(q.q.as_deref(), q.filter.as_deref())?;

    let mut c = st.pool.get(&auth).await?;
    let uids = c.sorted(&q.folder, &criteria).await?;
    let start = ((page - 1) * page_size) as usize;
    let slice: Vec<u32> = uids
        .iter()
        .skip(start)
        .take(page_size as usize)
        .copied()
        .collect();
    let messages = ops::summaries(c.s(), &slice).await?;
    c.release();

    Ok(Json(MessagePage {
        folder: q.folder,
        total: uids.len() as u32,
        page,
        page_size,
        messages,
    }))
}

/// At most this many hits from a search across all folders.
const SEARCH_ALL_LIMIT: usize = 200;

fn filtered(q: Option<&str>, filter: Option<&str>) -> AppResult<String> {
    let criteria = ops::search_criteria(q.unwrap_or(""))?;
    Ok(match filter {
        Some("unread") => format!("UNSEEN {criteria}"),
        Some("flagged") => format!("FLAGGED {criteria}"),
        _ => criteria,
    })
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
    #[serde(default)]
    filter: Option<String>,
}

/// Search every folder. Each folder is searched in turn (newest first within it); the
/// newest `SEARCH_ALL_LIMIT` hits overall are returned with their folder.
pub async fn search_all(
    State(st): State<AppState>,
    auth: Auth,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<SearchResults>> {
    if q.q.trim().is_empty() {
        return Err(AppError::BadRequest("enter something to search for".into()));
    }
    let criteria = filtered(Some(&q.q), q.filter.as_deref())?;
    let mut c = st.pool.get(&auth).await?;
    let folders = ops::folders(c.s()).await?;
    let mut hits = Vec::new();
    let mut matched = 0usize;
    for f in folders.iter().filter(|f| f.selectable && f.total > 0) {
        let uids = c.sorted(&f.path, &criteria).await?;
        matched += uids.len();
        // Only the newest few per folder can make the overall cut.
        let top: Vec<u32> = uids.iter().take(SEARCH_ALL_LIMIT).copied().collect();
        for m in ops::summaries(c.s(), &top).await? {
            hits.push(SearchHit {
                folder: f.path.clone(),
                message: m,
            });
        }
    }
    c.release();
    // Dates keep the sender's offset, so compare instants, not strings. Undated mail last.
    let at = |h: &SearchHit| {
        h.message
            .date
            .as_deref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|d| d.timestamp())
    };
    hits.sort_by_key(|h| std::cmp::Reverse(at(h)));
    hits.truncate(SEARCH_ALL_LIMIT);
    Ok(Json(SearchResults {
        truncated: matched > hits.len(),
        hits,
    }))
}

#[derive(Deserialize)]
pub struct ThreadQuery {
    folder: String,
    uid: u32,
}

/// Most messages a conversation view will show.
const THREAD_LIMIT: usize = 100;

/// The conversation the message belongs to, gathered from its folder, Inbox and Sent (never
/// Trash/Junk), oldest first, de-duplicated by Message-ID (mail to yourself exists twice).
pub async fn thread(
    State(st): State<AppState>,
    auth: Auth,
    Query(q): Query<ThreadQuery>,
) -> AppResult<Json<Thread>> {
    let mut c = st.pool.get(&auth).await?;
    ops::select(c.s(), &q.folder).await?;
    let (raw, _) = ops::raw(c.s(), q.uid, true).await?;
    let (root, known) = {
        let m = mail_parser::MessageParser::new()
            .parse_headers(&raw)
            .ok_or(AppError::NotFound)?;
        let mut known: Vec<String> = mime::id_list(m.references());
        known.extend(mime::id_list(m.in_reply_to()));
        known.extend(m.message_id().map(str::to_owned));
        (mime::thread_key(&m, q.uid), known)
    };
    if root.starts_with("uid:") {
        c.release();
        return Ok(Json(Thread { items: vec![] })); // no Message-ID at all: nothing to match
    }
    let criteria = ops::thread_criteria(&root, &known)?;
    let sp = ops::specials(c.s()).await?;
    let mut folders = vec![q.folder.clone()];
    for f in [Some("INBOX"), sp.sent.as_deref()].into_iter().flatten() {
        if !folders.iter().any(|x| x == f) {
            folders.push(f.to_owned());
        }
    }
    let mut items: Vec<SearchHit> = Vec::new();
    for f in &folders {
        let uids = c.sorted(f, &criteria).await?;
        let top: Vec<u32> = uids.iter().take(THREAD_LIMIT).copied().collect();
        for m in ops::summaries(c.s(), &top).await? {
            let dup = m.message_id.is_some()
                && items.iter().any(|i| i.message.message_id == m.message_id);
            if !dup {
                items.push(SearchHit {
                    folder: f.clone(),
                    message: m,
                });
            }
        }
    }
    c.release();
    let at = |h: &SearchHit| {
        h.message
            .date
            .as_deref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|d| d.timestamp())
    };
    // Same second (fast exchanges, automated mail): fall back to thread depth.
    items.sort_by_key(|h| (at(h), h.message.depth));
    items.truncate(THREAD_LIMIT);
    Ok(Json(Thread { items }))
}

#[derive(Deserialize)]
pub struct MessageQuery {
    folder: String,
    uid: u32,
    /// "1" to keep remote images.
    #[serde(default)]
    images: Option<String>,
}

pub async fn message(
    State(st): State<AppState>,
    auth: Auth,
    Query(q): Query<MessageQuery>,
) -> AppResult<Json<MessageDetail>> {
    use crate::cache::Key;
    let mut c = st.pool.get(&auth).await?;
    let mb = ops::select(c.s(), &q.folder).await?;
    let key = |images: bool| {
        mb.uid_validity.map(|validity| Key {
            user: auth.ukey.clone(),
            folder: q.folder.clone(),
            validity,
            uid: q.uid,
            images,
        })
    };
    let cached = |images: bool| key(images).and_then(|k| st.messages.get(&k));

    // Remote content: explicit request, else the preference. "From contacts" needs the
    // sender, which either rendering in the cache already knows.
    let pref = settings::prefs(&st.db, &auth.email).await?.remote_images;
    let explicit = q.images.as_deref() == Some("1");
    let known = cached(false).or_else(|| cached(true));
    let allow = match (explicit, pref, &known) {
        (true, _, _) | (_, RemoteImages::Always, _) => Some(true),
        (_, RemoteImages::Never, _) => Some(false),
        (_, RemoteImages::Contacts, Some(d)) => match &d.from {
            Some(a) => Some(settings::is_contact(&st.db, &auth.email, &a.email).await?),
            None => Some(false),
        },
        (_, RemoteImages::Contacts, None) => None, // decided from the fetched message below
    };

    // Hit: content from memory, flags fresh from the server (they change; content can't).
    if let Some(hit) = allow.and_then(cached) {
        let flags = ops::flags(c.s(), q.uid).await?;
        c.release();
        let mut d = (*hit).clone();
        d.flags = flags;
        return Ok(Json(d));
    }

    // Miss. Never marks it read: the reader does that explicitly once the message is on
    // screen (see MessageView) — an implicit \Seen here raced "mark unread" pressed while
    // the message was loading, and silently undid it.
    let (raw, flags) = ops::raw(c.s(), q.uid, true).await?;
    c.release();
    let allow = match allow {
        Some(a) => a,
        None => match mime::sender(&raw) {
            Some(from) => settings::is_contact(&st.db, &auth.email, &from).await?,
            None => false,
        },
    };
    // Parsing and sanitising big messages is CPU work; keep it off the async workers.
    let folder = q.folder.clone();
    let detail =
        tokio::task::spawn_blocking(move || mime::detail(&folder, q.uid, &raw, flags, allow))
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
    if let Some(k) = key(allow) {
        st.messages.put(k, std::sync::Arc::new(detail.clone()));
    }
    Ok(Json(detail))
}

#[derive(Deserialize)]
pub struct AttachmentQuery {
    folder: String,
    uid: u32,
    index: u32,
    /// "1" to display a previewable image instead of downloading it.
    #[serde(default)]
    inline: Option<String>,
}

/// Every response that carries message bytes gets this: even if a browser decided to render
/// one, it would be in an opaque origin with no script.
const SANDBOX_CSP: &str =
    "sandbox; default-src 'none'; img-src data:; style-src 'unsafe-inline'; frame-ancestors 'none'";

/// RFC 6266 `filename*` value.
fn disposition(filename: &str) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut enc = String::new();
    for b in filename.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            enc.push(b as char);
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{enc}")
}

pub async fn attachment(
    State(st): State<AppState>,
    auth: Auth,
    Query(q): Query<AttachmentQuery>,
) -> AppResult<Response> {
    let mut c = st.pool.get(&auth).await?;
    ops::select(c.s(), &q.folder).await?;
    let (raw, _) = ops::raw(c.s(), q.uid, true).await?;
    c.release();
    let a = tokio::task::spawn_blocking(move || mime::attachment(&raw, q.index))
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .ok_or(AppError::NotFound)?;
    let inline = q.inline.as_deref() == Some("1") && mime::previewable(&a.content_type);
    let disp = if inline {
        disposition(&a.filename).replacen("attachment", "inline", 1)
    } else {
        disposition(&a.filename)
    };
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, a.content_type)
        .header(header::CONTENT_DISPOSITION, disp)
        .header(header::CONTENT_SECURITY_POLICY, SANDBOX_CSP)
        .header(header::CACHE_CONTROL, "private, no-store")
        .body(Body::from(a.data))
        .map_err(|e| anyhow::anyhow!(e))?)
}

#[derive(Deserialize)]
pub struct RawQuery {
    folder: String,
    uid: u32,
    /// "1" to download as .eml; otherwise shown as plain text (view source).
    #[serde(default)]
    download: Option<String>,
}

/// The message exactly as stored: "view source" or a .eml download.
pub async fn raw(
    State(st): State<AppState>,
    auth: Auth,
    Query(q): Query<RawQuery>,
) -> AppResult<Response> {
    let mut c = st.pool.get(&auth).await?;
    ops::select(c.s(), &q.folder).await?;
    let (raw, _) = ops::raw(c.s(), q.uid, true).await?;
    c.release();
    let b = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_SECURITY_POLICY, SANDBOX_CSP)
        .header(header::CACHE_CONTROL, "private, no-store");
    let b = if q.download.as_deref() == Some("1") {
        let subject = mail_parser::MessageParser::new()
            .parse_headers(&raw)
            .and_then(|m| m.subject().map(str::to_owned))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("message-{}", q.uid));
        let name: String = subject
            .chars()
            .map(|c| {
                if c.is_control() || "/\\:*?\"<>|".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .take(80)
            .collect();
        b.header(header::CONTENT_TYPE, "message/rfc822").header(
            header::CONTENT_DISPOSITION,
            disposition(&format!("{}.eml", name.trim())),
        )
    } else {
        // text/plain + nosniff: the browser shows the bytes, never interprets them.
        b.header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
    };
    Ok(b.body(Body::from(raw)).map_err(|e| anyhow::anyhow!(e))?)
}

static PRINT_CSP: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    use base64::Engine;
    use sha2::Digest;
    let hash = base64::engine::general_purpose::STANDARD
        .encode(sha2::Sha256::digest(mime::PRINT_SCRIPT.as_bytes()));
    format!(
        "default-src 'none'; img-src data: https:; style-src 'unsafe-inline'; \
         script-src 'sha256-{hash}'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    )
});

/// A print-friendly page that opens the print dialog itself. Its CSP permits one script —
/// the print call, by hash — so nothing from the message can run even if the sanitizer
/// missed something.
pub async fn print(
    State(st): State<AppState>,
    auth: Auth,
    Query(q): Query<MessageQuery>,
) -> AppResult<Response> {
    let mut c = st.pool.get(&auth).await?;
    ops::select(c.s(), &q.folder).await?;
    let (raw, _) = ops::raw(c.s(), q.uid, true).await?;
    c.release();
    let allow = q.images.as_deref() == Some("1");
    let doc = tokio::task::spawn_blocking(move || mime::print_document(&raw, allow))
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CONTENT_SECURITY_POLICY, PRINT_CSP.as_str())
        .header(header::CACHE_CONTROL, "private, no-store")
        .body(Body::from(doc))
        .map_err(|e| anyhow::anyhow!(e))?)
}

pub async fn flag(
    State(st): State<AppState>,
    auth: Auth,
    Json(r): Json<FlagRequest>,
) -> AppResult<StatusCode> {
    let mut c = st.pool.get(&auth).await?;
    ops::select(c.s(), &r.folder).await?;
    ops::set_flag(c.s(), &r.uids, ops::flag_atom(r.flag), r.value).await?;
    c.release();
    Ok(StatusCode::NO_CONTENT)
}

pub async fn move_messages(
    State(st): State<AppState>,
    auth: Auth,
    Json(r): Json<MoveRequest>,
) -> AppResult<Json<MoveResult>> {
    if r.to == r.folder {
        return Ok(Json(MoveResult {
            to: Some(r.to),
            uids: r.uids,
        }));
    }
    let mut c = st.pool.get(&auth).await?;
    ops::select(c.s(), &r.folder).await?;
    let moved = ops::move_tracked(c.s(), &r.uids, &r.to).await?;
    c.release();
    Ok(Json(MoveResult {
        to: Some(r.to),
        uids: moved.into_iter().map(|(_, dst)| dst).collect(),
    }))
}

/// To Trash; from Trash (or with no Trash folder) it is permanent.
pub async fn delete(
    State(st): State<AppState>,
    auth: Auth,
    Json(r): Json<UidsRequest>,
) -> AppResult<Json<MoveResult>> {
    let mut c = st.pool.get(&auth).await?;
    let sp = ops::specials(c.s()).await?;
    ops::select(c.s(), &r.folder).await?;
    let result = match sp.trash.as_deref() {
        Some(trash) if trash != r.folder => {
            let moved = ops::move_tracked(c.s(), &r.uids, trash).await?;
            MoveResult {
                to: Some(trash.to_owned()),
                uids: moved.into_iter().map(|(_, d)| d).collect(),
            }
        }
        _ => {
            ops::expunge(c.s(), &r.uids).await?;
            MoveResult {
                to: None,
                uids: vec![],
            }
        }
    };
    c.release();
    Ok(Json(result))
}

pub async fn junk(
    State(st): State<AppState>,
    auth: Auth,
    Json(r): Json<UidsRequest>,
) -> AppResult<Json<MoveResult>> {
    let mut c = st.pool.get(&auth).await?;
    let sp = ops::specials(c.s()).await?;
    let junk = sp.require(Special::Junk)?.to_owned();
    let mut uids = vec![];
    if junk != r.folder {
        ops::select(c.s(), &r.folder).await?;
        uids = ops::move_tracked(c.s(), &r.uids, &junk)
            .await?
            .into_iter()
            .map(|(_, d)| d)
            .collect();
    }
    c.release();
    Ok(Json(MoveResult {
        to: Some(junk),
        uids,
    }))
}

/// Only Trash and Junk can be emptied: anywhere else one click would destroy real mail.
pub async fn empty_folder(
    State(st): State<AppState>,
    auth: Auth,
    Json(r): Json<FolderRequest>,
) -> AppResult<StatusCode> {
    let mut c = st.pool.get(&auth).await?;
    let sp = ops::specials(c.s()).await?;
    if sp.trash.as_deref() != Some(&r.folder) && sp.junk.as_deref() != Some(&r.folder) {
        return Err(AppError::BadRequest(
            "only Trash and Junk can be emptied".into(),
        ));
    }
    ops::select(c.s(), &r.folder).await?;
    let all = ops::sorted_uids(c.s(), "ALL").await?;
    if !all.is_empty() {
        ops::expunge(c.s(), &all).await?;
    }
    c.release();
    Ok(StatusCode::NO_CONTENT)
}

/// INBOX and the special-use folders are structural: other features find mail through them.
async fn require_user_folder(c: &mut crate::imap::Conn, folder: &str) -> AppResult<()> {
    let sp = ops::specials(c.s()).await?;
    let special = [&sp.sent, &sp.drafts, &sp.trash, &sp.junk, &sp.archive]
        .iter()
        .any(|p| p.as_deref() == Some(folder));
    if special || folder.eq_ignore_ascii_case("INBOX") {
        return Err(AppError::BadRequest(
            "system folders cannot be renamed or deleted".into(),
        ));
    }
    Ok(())
}

fn parent_of<'a>(path: &'a str, delimiter: Option<&str>) -> Option<&'a str> {
    delimiter.and_then(|d| path.rsplit_once(d)).map(|(p, _)| p)
}

pub async fn create_folder(
    State(st): State<AppState>,
    auth: Auth,
    Json(r): Json<CreateFolderRequest>,
) -> AppResult<StatusCode> {
    let mut c = st.pool.get(&auth).await?;
    let delim = ops::delimiter(c.s()).await?;
    let leaf = ops::folder_leaf(&r.name, delim.as_deref())?;
    let path = match (&r.parent, &delim) {
        (Some(p), Some(d)) => format!("{p}{d}{leaf}"),
        (Some(_), None) => {
            return Err(AppError::BadRequest("this server has no subfolders".into()));
        }
        (None, _) => leaf,
    };
    ops::create_folder(c.s(), &path).await?;
    c.release();
    Ok(StatusCode::NO_CONTENT)
}

pub async fn rename_folder(
    State(st): State<AppState>,
    auth: Auth,
    Json(r): Json<RenameFolderRequest>,
) -> AppResult<StatusCode> {
    let mut c = st.pool.get(&auth).await?;
    require_user_folder(&mut c, &r.folder).await?;
    let delim = ops::delimiter(c.s()).await?;
    let leaf = ops::folder_leaf(&r.name, delim.as_deref())?;
    let to = match (parent_of(&r.folder, delim.as_deref()), &delim) {
        (Some(p), Some(d)) => format!("{p}{d}{leaf}"),
        _ => leaf,
    };
    if to != r.folder {
        ops::rename_folder(c.s(), &r.folder, &to).await?;
    }
    c.release();
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes a folder and the mail in it. Refuses while it still has subfolders, so one click
/// cannot take a whole tree.
pub async fn delete_folder(
    State(st): State<AppState>,
    auth: Auth,
    Json(r): Json<FolderRequest>,
) -> AppResult<StatusCode> {
    let mut c = st.pool.get(&auth).await?;
    require_user_folder(&mut c, &r.folder).await?;
    let delim = ops::delimiter(c.s()).await?;
    if let Some(d) = &delim {
        let prefix = format!("{}{d}", r.folder);
        if ops::folders(c.s())
            .await?
            .iter()
            .any(|f| f.path.starts_with(&prefix))
        {
            return Err(AppError::BadRequest(
                "delete or move its subfolders first".into(),
            ));
        }
    }
    ops::delete_folder(c.s(), &r.folder).await?;
    c.release();
    Ok(StatusCode::NO_CONTENT)
}

pub async fn mark_folder_read(
    State(st): State<AppState>,
    auth: Auth,
    Json(r): Json<FolderRequest>,
) -> AppResult<StatusCode> {
    let mut c = st.pool.get(&auth).await?;
    ops::mark_all_read(c.s(), &r.folder).await?;
    c.release();
    Ok(StatusCode::NO_CONTENT)
}
