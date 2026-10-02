//! IMAP commands the API needs. Every stream async-imap returns is drained to the end —
//! leaving one half-read desynchronises the connection.

use async_imap::{
    imap_proto::{
        MailboxDatum, NameAttribute, Response, ResponseCode, Status, rfc4315::UidSetMember,
    },
    types::Flag,
};
use futures::TryStreamExt;
use mail_parser::{MessageParser, MimeHeaders};

use super::{Session, utf7};
use crate::{
    error::{AppError, AppResult},
    mime,
    types::{FlagName, Flags, Folder, MessageSummary, Special},
};

/// Paths of the special-use folders, as LIST reported them.
#[derive(Default, Debug, Clone)]
pub struct Specials {
    pub sent: Option<String>,
    pub drafts: Option<String>,
    pub trash: Option<String>,
    pub junk: Option<String>,
    pub archive: Option<String>,
}

impl Specials {
    pub fn get(&self, s: Special) -> Option<&str> {
        match s {
            Special::Inbox => Some("INBOX"),
            Special::Sent => self.sent.as_deref(),
            Special::Drafts => self.drafts.as_deref(),
            Special::Trash => self.trash.as_deref(),
            Special::Junk => self.junk.as_deref(),
            Special::Archive => self.archive.as_deref(),
        }
    }

    pub fn require(&self, s: Special) -> AppResult<&str> {
        self.get(s)
            .ok_or_else(|| AppError::Mail(format!("the server has no {s:?} folder")))
    }
}

struct Listed {
    path: String,
    delimiter: Option<String>,
    special: Option<Special>,
    selectable: bool,
}

async fn list(s: &mut Session) -> AppResult<Vec<Listed>> {
    let names: Vec<_> = s.list(Some(""), Some("*")).await?.try_collect().await?;
    Ok(names
        .iter()
        .map(|n| {
            let attrs = n.attributes();
            let path = n.name().to_owned();
            let special = if path.eq_ignore_ascii_case("INBOX") {
                Some(Special::Inbox)
            } else {
                attrs.iter().find_map(|a| match a {
                    NameAttribute::Sent => Some(Special::Sent),
                    NameAttribute::Drafts => Some(Special::Drafts),
                    NameAttribute::Trash => Some(Special::Trash),
                    NameAttribute::Junk => Some(Special::Junk),
                    NameAttribute::Archive => Some(Special::Archive),
                    _ => None,
                })
            };
            Listed {
                path,
                delimiter: n.delimiter().map(str::to_owned),
                special,
                selectable: !attrs.iter().any(|a| matches!(a, NameAttribute::NoSelect)),
            }
        })
        .collect())
}

pub async fn specials(s: &mut Session) -> AppResult<Specials> {
    let mut sp = Specials::default();
    for l in list(s).await? {
        let slot = match l.special {
            Some(Special::Sent) => &mut sp.sent,
            Some(Special::Drafts) => &mut sp.drafts,
            Some(Special::Trash) => &mut sp.trash,
            Some(Special::Junk) => &mut sp.junk,
            Some(Special::Archive) => &mut sp.archive,
            _ => continue,
        };
        slot.get_or_insert(l.path);
    }
    Ok(sp)
}

pub async fn folders(s: &mut Session) -> AppResult<Vec<Folder>> {
    let mut out = Vec::new();
    for l in list(s).await? {
        let (total, unread) = if l.selectable {
            let st = s.status(&l.path, "(MESSAGES UNSEEN)").await?;
            (st.exists, st.unseen.unwrap_or(0))
        } else {
            (0, 0)
        };
        let leaf = match &l.delimiter {
            Some(d) => l.path.rsplit(d.as_str()).next().unwrap_or(&l.path),
            None => &l.path,
        };
        out.push(Folder {
            name: if l.special == Some(Special::Inbox) {
                "Inbox".to_owned()
            } else {
                utf7::decode(leaf)
            },
            path: l.path,
            delimiter: l.delimiter,
            special: l.special,
            selectable: l.selectable,
            total,
            unread,
        });
    }
    // Inbox, then the other special folders in a fixed order, then the rest by path.
    let rank = |f: &Folder| match f.special {
        Some(Special::Inbox) => 0,
        Some(Special::Drafts) => 1,
        Some(Special::Sent) => 2,
        Some(Special::Archive) => 3,
        Some(Special::Junk) => 4,
        Some(Special::Trash) => 5,
        None => 6,
    };
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.path.cmp(&b.path)));
    Ok(out)
}

/// SELECT with CONDSTORE where the server has it, so the result carries HIGHESTMODSEQ —
/// a counter that moves on *any* change (arrivals, expunges, flags). The sort cache keys on it.
pub async fn select(s: &mut Session, folder: &str) -> AppResult<async_imap::types::Mailbox> {
    use async_imap::error::Error;
    let not_found = |e: Error| match e {
        Error::No(_) => AppError::NotFound,
        e => e.into(),
    };
    match s.select_condstore(folder).await {
        Ok(mb) => Ok(mb),
        Err(Error::Bad(_)) => s.select(folder).await.map_err(not_found),
        Err(e) => Err(not_found(e)),
    }
}

/// The hierarchy delimiter (LIST "" "") — "." on Dovecot's default layout, "/" elsewhere.
pub async fn delimiter(s: &mut Session) -> AppResult<Option<String>> {
    let names: Vec<_> = s.list(Some(""), Some("\"\"")).await?.try_collect().await?;
    Ok(names.first().and_then(|n| n.delimiter()).map(str::to_owned))
}

/// Validates a user-typed folder name and encodes it for the wire.
pub fn folder_leaf(name: &str, delimiter: Option<&str>) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::BadRequest(
            "folder names are 1–100 characters".into(),
        ));
    }
    if name.contains(['*', '%', '"', '\\'])
        || name.chars().any(char::is_control)
        || delimiter.is_some_and(|d| name.contains(d))
    {
        return Err(AppError::BadRequest(format!(
            "folder names cannot contain {}* % \" \\",
            delimiter.map(|d| format!("{d} ")).unwrap_or_default()
        )));
    }
    if name.eq_ignore_ascii_case("INBOX") {
        return Err(AppError::BadRequest("that name is reserved".into()));
    }
    Ok(utf7::encode(name))
}

pub async fn create_folder(s: &mut Session, path: &str) -> AppResult<()> {
    s.create(path).await.map_err(already_exists)?;
    // Unsubscribed folders are invisible to some clients; keep them in step.
    let _ = s.subscribe(path).await;
    Ok(())
}

pub async fn rename_folder(s: &mut Session, from: &str, to: &str) -> AppResult<()> {
    s.rename(from, to).await.map_err(already_exists)?;
    let _ = s.unsubscribe(from).await;
    let _ = s.subscribe(to).await;
    Ok(())
}

pub async fn delete_folder(s: &mut Session, path: &str) -> AppResult<()> {
    // Never delete the selected mailbox from under ourselves.
    let _ = s.run_command_and_check_ok("UNSELECT").await;
    let _ = s.unsubscribe(path).await;
    s.delete(path).await?;
    Ok(())
}

fn already_exists(e: async_imap::error::Error) -> AppError {
    match e {
        async_imap::error::Error::No(m) if m.to_ascii_lowercase().contains("exists") => {
            AppError::BadRequest("a folder with that name already exists".into())
        }
        e => e.into(),
    }
}

/// Marks every message in the folder read.
pub async fn mark_all_read(s: &mut Session, folder: &str) -> AppResult<()> {
    let mb = select(s, folder).await?;
    if mb.exists == 0 {
        return Ok(());
    }
    let _: Vec<_> = s
        .uid_store("1:*", "+FLAGS.SILENT (\\Seen)")
        .await?
        .try_collect()
        .await?;
    Ok(())
}

/// IMAP quoted string. Only for ASCII without CR/LF; anything else goes through `literal`.
pub fn quote(s: &str) -> AppResult<String> {
    if s.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
        return Err(AppError::BadRequest("invalid characters".into()));
    }
    Ok(format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// Non-synchronising literal (LITERAL+): safe for any UTF-8, no round-trip to the server.
fn literal(s: &str) -> AppResult<String> {
    if s.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
        return Err(AppError::BadRequest("invalid characters".into()));
    }
    Ok(format!("{{{}+}}\r\n{}", s.len(), s))
}

/// SEARCH criteria for the search box: every word must match somewhere.
pub fn search_criteria(q: &str) -> AppResult<String> {
    let words: Vec<&str> = q.split_whitespace().take(8).collect();
    if words.is_empty() {
        return Ok("ALL".to_owned());
    }
    let mut parts = Vec::new();
    for w in words {
        let l = literal(w)?;
        parts.push(format!(
            "OR OR OR FROM {l} TO {l} SUBJECT {l} BODY {l}",
            l = l
        ));
    }
    Ok(parts.join(" "))
}

/// Runs a command async-imap has no (or a lossy) wrapper for, handing every response —
/// untagged ones and the final tagged one — to `on`. Errors unless the tagged status is OK.
async fn command(s: &mut Session, cmd: &str, mut on: impl FnMut(&Response<'_>)) -> AppResult<()> {
    let id = s.run_command(cmd).await?;
    loop {
        let resp = s
            .read_response()
            .await
            .map_err(|_| AppError::Unavailable(crate::error::UNREACHABLE.into()))?
            .ok_or_else(|| AppError::Unavailable(crate::error::UNREACHABLE.into()))?;
        let parsed = resp.parsed();
        on(parsed);
        if let Response::Done {
            tag,
            status,
            outcome,
            ..
        } = parsed
            && *tag == id
        {
            return match status {
                Status::Ok => Ok(()),
                _ => Err(AppError::Mail(
                    outcome
                        .information
                        .as_deref()
                        .unwrap_or("command failed")
                        .to_owned(),
                )),
            };
        }
    }
}

/// `UID SORT (REVERSE ARRIVAL)` over the selected folder: newest first.
pub async fn sorted_uids(s: &mut Session, criteria: &str) -> AppResult<Vec<u32>> {
    let mut uids = Vec::new();
    command(
        s,
        &format!("UID SORT (REVERSE ARRIVAL) UTF-8 {criteria}"),
        |r| {
            if let Response::MailboxData(MailboxDatum::Sort(ids)) = r {
                uids.extend_from_slice(ids);
            }
        },
    )
    .await?;
    Ok(uids)
}

fn expand(set: &[UidSetMember]) -> Vec<u32> {
    set.iter()
        .flat_map(|m| match m {
            UidSetMember::Uid(u) => *u..=*u,
            UidSetMember::UidRange(r) => r.clone(),
        })
        .collect()
}

/// UID MOVE that reports where the messages landed: (source uid, destination uid) pairs from
/// the server's COPYUID (UIDPLUS). Empty if the server does not report it — then there is
/// simply nothing to undo with.
pub async fn move_tracked(s: &mut Session, uids: &[u32], to: &str) -> AppResult<Vec<(u32, u32)>> {
    let mut pairs = Vec::new();
    let cmd = format!("UID MOVE {} {}", uid_set(uids)?, quote_mailbox(to)?);
    command(s, &cmd, |r| {
        let outcome = match r {
            Response::Data { outcome, .. } | Response::Done { outcome, .. } => outcome,
            _ => return,
        };
        if let Some(ResponseCode::CopyUid(_, src, dst)) = &outcome.code {
            pairs.extend(expand(src).into_iter().zip(expand(dst)));
        }
    })
    .await?;
    Ok(pairs)
}

/// Mailbox names on the wire: quoted if plain ASCII, else a literal.
fn quote_mailbox(name: &str) -> AppResult<String> {
    if name.is_ascii() {
        quote(name)
    } else {
        literal(name)
    }
}

pub fn uid_set(uids: &[u32]) -> AppResult<String> {
    if uids.is_empty() {
        return Err(AppError::BadRequest("no messages selected".into()));
    }
    Ok(uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(","))
}

fn flags_of(f: &async_imap::types::Fetch) -> Flags {
    let mut out = Flags::default();
    for flag in f.flags() {
        match flag {
            Flag::Seen => out.seen = true,
            Flag::Flagged => out.flagged = true,
            Flag::Answered => out.answered = true,
            Flag::Draft => out.draft = true,
            Flag::Custom(c) if c.eq_ignore_ascii_case("$Forwarded") => out.forwarded = true,
            _ => {}
        }
    }
    out
}

/// Only the header fields a list row shows (a fraction of a full header block).
const LIST_FETCH: &str = "(UID FLAGS RFC822.SIZE BODY.PEEK[HEADER.FIELDS \
     (FROM TO SUBJECT DATE MESSAGE-ID IN-REPLY-TO REFERENCES CONTENT-TYPE)])";

/// List rows for `uids`, returned in the order given.
pub async fn summaries(s: &mut Session, uids: &[u32]) -> AppResult<Vec<MessageSummary>> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    let fetches: Vec<_> = s
        .uid_fetch(uid_set(uids)?, LIST_FETCH)
        .await?
        .try_collect()
        .await?;
    let parser = MessageParser::new();
    let mut rows: Vec<MessageSummary> = fetches
        .iter()
        .filter_map(|f| {
            let uid = f.uid?;
            let hdr = parser.parse_headers(f.header().unwrap_or_default())?;
            let ct = hdr.content_type();
            let has_attachments = ct.is_some_and(|c| {
                c.ctype().eq_ignore_ascii_case("multipart")
                    && c.subtype().is_some_and(|s| s.eq_ignore_ascii_case("mixed"))
            });
            Some(MessageSummary {
                uid,
                subject: hdr.subject().unwrap_or_default().to_owned(),
                from: hdr.from().and_then(|a| a.first()).map(mime::address),
                to: mime::addresses(hdr.to()),
                date: hdr.date().map(|d| d.to_rfc3339()),
                size: f.size.unwrap_or(0),
                flags: flags_of(f),
                has_attachments,
                thread_key: mime::thread_key(&hdr, uid),
                message_id: hdr.message_id().map(str::to_owned),
                depth: mime::depth(&hdr),
            })
        })
        .collect();
    let pos = |uid: u32| uids.iter().position(|u| *u == uid).unwrap_or(usize::MAX);
    rows.sort_by_key(|r| pos(r.uid));
    Ok(rows)
}

/// SEARCH criteria for one conversation: the root message, everything whose References
/// mention it, and replies (by In-Reply-To) to any message already known in it — for
/// clients that send In-Reply-To without References.
pub fn thread_criteria(root: &str, known: &[String]) -> AppResult<String> {
    let mut terms = vec![
        format!("HEADER Message-ID {}", quote(&format!("<{root}>"))?),
        format!("HEADER References {}", quote(&format!("<{root}>"))?),
    ];
    for id in known.iter().filter(|i| *i != root).take(10) {
        terms.push(format!("HEADER In-Reply-To {}", quote(&format!("<{id}>"))?));
    }
    // IMAP's OR is binary: OR a OR b OR c d.
    let mut out = terms.pop().expect("at least two terms");
    while let Some(t) = terms.pop() {
        out = format!("OR {t} {out}");
    }
    Ok(out)
}

/// The full RFC 822 message. `peek = false` marks it read, as opening a message should.
pub async fn raw(s: &mut Session, uid: u32, peek: bool) -> AppResult<(Vec<u8>, Flags)> {
    let query = if peek {
        "(UID FLAGS BODY.PEEK[])"
    } else {
        "(UID FLAGS BODY[])"
    };
    let fetches: Vec<_> = s
        .uid_fetch(uid.to_string(), query)
        .await?
        .try_collect()
        .await?;
    let f = fetches
        .iter()
        .find(|f| f.uid == Some(uid))
        .ok_or(AppError::NotFound)?;
    let body = f.body().ok_or(AppError::NotFound)?.to_vec();
    let mut flags = flags_of(f);
    if !peek {
        flags.seen = true;
    }
    Ok((body, flags))
}

pub async fn set_flag(s: &mut Session, uids: &[u32], flag: &str, value: bool) -> AppResult<()> {
    let op = if value {
        "+FLAGS.SILENT"
    } else {
        "-FLAGS.SILENT"
    };
    let _: Vec<_> = s
        .uid_store(uid_set(uids)?, format!("{op} ({flag})"))
        .await?
        .try_collect()
        .await?;
    Ok(())
}

pub fn flag_atom(f: FlagName) -> &'static str {
    match f {
        FlagName::Seen => "\\Seen",
        FlagName::Flagged => "\\Flagged",
    }
}

pub async fn move_to(s: &mut Session, uids: &[u32], to: &str) -> AppResult<()> {
    move_tracked(s, uids, to).await.map(|_| ())
}

/// Permanently removes `uids` from the selected folder (UIDPLUS: touches nothing else).
pub async fn expunge(s: &mut Session, uids: &[u32]) -> AppResult<()> {
    set_flag(s, uids, "\\Deleted", true).await?;
    let _: Vec<_> = s.uid_expunge(uid_set(uids)?).await?.try_collect().await?;
    Ok(())
}

pub async fn append(s: &mut Session, folder: &str, flags: &str, msg: &[u8]) -> AppResult<()> {
    s.append(folder, Some(flags), None, msg).await?;
    Ok(())
}

/// Finds a message in the selected folder by its Message-ID (to learn an APPENDed UID).
pub async fn find_by_message_id(s: &mut Session, message_id: &str) -> AppResult<Option<u32>> {
    let found = s
        .uid_search(format!("HEADER Message-ID {}", quote(message_id)?))
        .await?;
    Ok(found.into_iter().max())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_criteria_or_chain() {
        let c = thread_criteria("r@x", &["r@x".into(), "a@x".into()]).unwrap();
        assert_eq!(
            c,
            r#"OR HEADER Message-ID "<r@x>" OR HEADER References "<r@x>" HEADER In-Reply-To "<a@x>""#
        );
        assert!(thread_criteria("bad\r\nid", &[]).is_err());
    }

    #[test]
    fn folder_names() {
        assert_eq!(folder_leaf("  Projects ", Some(".")).unwrap(), "Projects");
        assert_eq!(folder_leaf("Төсөл", Some(".")).unwrap(), "&BCIE6QRBBOkEOw-");
        assert!(folder_leaf("a.b", Some(".")).is_err());
        assert!(folder_leaf("a/b", Some(".")).is_ok());
        assert!(folder_leaf("a/b", Some("/")).is_err());
        for bad in ["", "   ", "x*", "x%", "inbox", "a\tb"] {
            assert!(folder_leaf(bad, Some(".")).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn quoting() {
        assert_eq!(quote(r#"a "b" \c"#).unwrap(), r#""a \"b\" \\c""#);
        assert!(quote("a\r\nb").is_err());
    }

    #[test]
    fn search_uses_literals() {
        assert_eq!(search_criteria("  ").unwrap(), "ALL");
        let c = search_criteria("hé").unwrap();
        assert_eq!(
            c,
            "OR OR OR FROM {3+}\r\nhé TO {3+}\r\nhé SUBJECT {3+}\r\nhé BODY {3+}\r\nhé"
        );
        assert!(search_criteria("x\0y").is_err());
    }
}
