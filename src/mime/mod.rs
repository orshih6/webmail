//! Turning RFC 822 bytes into API shapes.

pub mod sanitize;

use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::STANDARD};
use mail_parser::{HeaderValue, Message, MessageParser, MessagePart, MimeHeaders};

use crate::types::{Address, Attachment, Flags, MessageDetail};

/// Inline images larger than this are left out of the rendered HTML (still downloadable).
const MAX_INLINE_IMAGE: usize = 5 * 1024 * 1024;

pub fn address(a: &mail_parser::Addr) -> Address {
    Address {
        name: a
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned),
        email: a.address.as_deref().unwrap_or_default().to_owned(),
    }
}

pub fn addresses(a: Option<&mail_parser::Address>) -> Vec<Address> {
    a.map(|a| a.iter().map(address).collect())
        .unwrap_or_default()
}

pub fn id_list(v: &HeaderValue) -> Vec<String> {
    match v {
        HeaderValue::Text(t) => vec![t.to_string()],
        HeaderValue::TextList(l) => l.iter().map(|t| t.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Number of ancestors a message declares: its References, or 1 for a bare In-Reply-To.
pub fn depth(m: &Message) -> u32 {
    match id_list(m.references()).len() {
        0 if !id_list(m.in_reply_to()).is_empty() => 1,
        n => n as u32,
    }
}

/// Conversation key: the first Message-ID in References, else In-Reply-To, else its own.
pub fn thread_key(m: &Message, uid: u32) -> String {
    id_list(m.references())
        .into_iter()
        .next()
        .or_else(|| id_list(m.in_reply_to()).into_iter().next())
        .or_else(|| m.message_id().map(str::to_owned))
        .unwrap_or_else(|| format!("uid:{uid}"))
}

fn content_type(p: &MessagePart) -> String {
    let token = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&b))
    };
    match p.content_type() {
        Some(ct) => {
            let sub = ct.subtype().unwrap_or("octet-stream");
            if token(ct.ctype()) && token(sub) {
                format!("{}/{}", ct.ctype(), sub).to_ascii_lowercase()
            } else {
                "application/octet-stream".to_owned()
            }
        }
        None => "application/octet-stream".to_owned(),
    }
}

/// The bytes of an attachment; a forwarded message (message/rfc822) is its raw source.
fn part_bytes<'a>(p: &'a MessagePart) -> &'a [u8] {
    match p.message() {
        Some(m) => m.raw_message(),
        None => p.contents(),
    }
}

fn filename(p: &MessagePart, index: usize) -> String {
    match p.attachment_name() {
        Some(n) if !n.trim().is_empty() => n.trim().to_owned(),
        _ if p.is_message() => format!("message-{}.eml", index + 1),
        _ => format!("attachment-{}", index + 1),
    }
}

pub fn detail(
    folder: &str,
    uid: u32,
    raw: &[u8],
    flags: Flags,
    allow_remote: bool,
) -> MessageDetail {
    let parser = MessageParser::new();
    let Some(m) = parser.parse(raw) else {
        return MessageDetail {
            folder: folder.to_owned(),
            uid,
            subject: String::new(),
            from: None,
            to: vec![],
            cc: vec![],
            bcc: vec![],
            reply_to: vec![],
            date: None,
            message_id: None,
            in_reply_to: None,
            references: vec![],
            flags,
            html: None,
            text: Some(String::from_utf8_lossy(raw).into_owned()),
            remote_images_blocked: false,
            attachments: vec![],
        };
    };

    // Only a real text/html part counts: mail-parser synthesises HTML from text otherwise.
    let html_src = m
        .html_part(0)
        .filter(|p| p.is_text_html())
        .and_then(|_| m.body_html(0));

    let mut inline_cids: HashMap<String, String> = HashMap::new();
    let mut attachments = Vec::new();
    for (i, p) in m.attachments().enumerate() {
        let ct = content_type(p);
        let cid = p.content_id().map(|c| {
            c.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_owned()
        });
        let referenced = match (&cid, &html_src) {
            (Some(c), Some(h)) => h.contains(&format!("cid:{c}")),
            _ => false,
        };
        if referenced && ct.starts_with("image/") {
            let bytes = part_bytes(p);
            if bytes.len() <= MAX_INLINE_IMAGE {
                inline_cids.insert(
                    cid.clone().unwrap_or_default(),
                    format!("data:{ct};base64,{}", STANDARD.encode(bytes)),
                );
                continue;
            }
        }
        attachments.push(Attachment {
            index: i as u32,
            filename: filename(p, i),
            content_type: ct,
            size: part_bytes(p).len() as u32,
        });
    }

    let (html, remote_images_blocked) = match &html_src {
        Some(h) => {
            let (doc, blocked) = sanitize::document(h, &inline_cids, allow_remote);
            (Some(doc), blocked)
        }
        None => (None, false),
    };

    MessageDetail {
        folder: folder.to_owned(),
        uid,
        subject: m.subject().unwrap_or_default().to_owned(),
        from: m.from().and_then(|a| a.first()).map(address),
        to: addresses(m.to()),
        cc: addresses(m.cc()),
        bcc: addresses(m.bcc()),
        reply_to: addresses(m.reply_to()),
        date: m.date().map(|d| d.to_rfc3339()),
        message_id: m.message_id().map(str::to_owned),
        in_reply_to: id_list(m.in_reply_to()).into_iter().next(),
        references: id_list(m.references()),
        flags,
        html,
        text: m.body_text(0).map(|t| t.into_owned()),
        remote_images_blocked,
        attachments,
    }
}

/// The From address, for the "load images from contacts" preference.
pub fn sender(raw: &[u8]) -> Option<String> {
    let m = MessageParser::new().parse_headers(raw)?;
    m.from()?.first()?.address.as_deref().map(str::to_owned)
}

/// The script that opens the print dialog, and the CSP hash that allows exactly it.
pub const PRINT_SCRIPT: &str = "addEventListener('load',function(){print()})";

/// A printable page: a header block (escaped) above the sanitized body. Served as its own
/// document under a CSP that allows only `PRINT_SCRIPT`.
pub fn print_document(raw: &[u8], allow_remote: bool) -> String {
    let d = detail("", 0, raw, Flags::default(), allow_remote);
    let list = |a: &[Address]| {
        a.iter()
            .map(|a| match &a.name {
                Some(n) => format!("{n} <{}>", a.email),
                None => a.email.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut rows = Vec::new();
    if let Some(f) = &d.from {
        rows.push(("From", list(std::slice::from_ref(f))));
    }
    for (k, v) in [("To", &d.to), ("Cc", &d.cc)] {
        if !v.is_empty() {
            rows.push((k, list(v)));
        }
    }
    if let Some(date) = &d.date {
        rows.push(("Date", date.clone()));
    }
    let header: String = rows
        .iter()
        .map(|(k, v)| format!("<tr><th>{k}</th><td>{}</td></tr>", sanitize::escape(v)))
        .collect();
    let body = match &d.html {
        Some(_) => {
            let cids = inline_images(raw);
            let src = MessageParser::new()
                .parse(raw)
                .and_then(|m| m.body_html(0).map(|h| h.into_owned()))
                .unwrap_or_default();
            sanitize::body(&src, &cids, allow_remote).0
        }
        None => format!(
            "<pre>{}</pre>",
            sanitize::escape(d.text.as_deref().unwrap_or(""))
        ),
    };
    let head = format!(
        "<title>{}</title><style>.wm-print{{border-bottom:1px solid #ccc;margin:0 0 16px;padding-bottom:8px}}\
         .wm-print h1{{font-size:18px;margin:0 0 8px}}.wm-print th{{text-align:left;padding-right:12px;\
         color:#666;font-weight:500;vertical-align:top}}</style><script>{PRINT_SCRIPT}</script>",
        sanitize::escape(&d.subject)
    );
    sanitize::wrap(
        &format!(
            "<div class=\"wm-print\"><h1>{}</h1><table>{header}</table></div>{body}",
            sanitize::escape(if d.subject.is_empty() {
                "(no subject)"
            } else {
                &d.subject
            })
        ),
        &head,
    )
}

/// cid → data: URI for the inline images an HTML body references.
fn inline_images(raw: &[u8]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(m) = MessageParser::new().parse(raw) {
        for p in m.attachments() {
            let ct = content_type(p);
            if let (Some(cid), true) = (p.content_id(), ct.starts_with("image/")) {
                let bytes = part_bytes(p);
                if bytes.len() <= MAX_INLINE_IMAGE {
                    out.insert(
                        cid.trim()
                            .trim_start_matches('<')
                            .trim_end_matches('>')
                            .to_owned(),
                        format!("data:{ct};base64,{}", STANDARD.encode(bytes)),
                    );
                }
            }
        }
    }
    out
}

/// Raster formats a browser can show without running anything. SVG is deliberately absent:
/// it can carry script, so it is only ever downloaded.
pub fn previewable(content_type: &str) -> bool {
    matches!(
        content_type,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    )
}

pub struct AttachmentData {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

pub fn attachment(raw: &[u8], index: u32) -> Option<AttachmentData> {
    let m = MessageParser::new().parse(raw)?;
    let p = m.attachments().nth(index as usize)?;
    Some(AttachmentData {
        filename: filename(p, index as usize),
        content_type: content_type(p),
        data: part_bytes(p).to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALT: &[u8] = b"From: \"Bob\" <bob@example.test>\r\n\
To: alice@example.test, Carol <carol@example.test>\r\n\
Subject: =?UTF-8?B?0J/RgNC40LLQtdGC?=\r\n\
Message-ID: <m2@example.test>\r\n\
In-Reply-To: <m1@example.test>\r\n\
References: <m0@example.test> <m1@example.test>\r\n\
Date: Thu, 1 Oct 2026 10:00:00 +0000\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"outer\"\r\n\
\r\n\
--outer\r\n\
Content-Type: multipart/related; boundary=\"rel\"\r\n\
\r\n\
--rel\r\n\
Content-Type: multipart/alternative; boundary=\"alt\"\r\n\
\r\n\
--alt\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
plain body\r\n\
--alt\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p onclick=\"x()\">html <b>body</b><img src=\"cid:logo@x\"><img src=\"https://t.example/p.gif\"></p><script>alert(1)</script>\r\n\
--alt--\r\n\
--rel\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo@x>\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--rel--\r\n\
--outer\r\n\
Content-Type: application/pdf; name=\"report.pdf\"\r\n\
Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQ=\r\n\
--outer--\r\n";

    #[test]
    fn parses_headers_bodies_and_attachments() {
        let d = detail("INBOX", 7, ALT, Flags::default(), false);
        assert_eq!(d.subject, "Привет");
        assert_eq!(d.from.as_ref().unwrap().email, "bob@example.test");
        assert_eq!(d.from.as_ref().unwrap().name.as_deref(), Some("Bob"));
        assert_eq!(d.to.len(), 2);
        assert_eq!(d.references, vec!["m0@example.test", "m1@example.test"]);
        assert_eq!(d.text.as_deref().map(str::trim), Some("plain body"));

        let html = d.html.unwrap();
        assert!(html.contains("<b>body</b>"));
        assert!(!html.contains("script") && !html.contains("onclick"));
        assert!(html.contains("data:image/png;base64,"), "cid image inlined");
        assert!(!html.contains("t.example"), "remote image blocked");
        assert!(d.remote_images_blocked);

        // The inline logo is not listed; the PDF is, with its parser index.
        assert_eq!(d.attachments.len(), 1);
        assert_eq!(d.attachments[0].filename, "report.pdf");
        assert_eq!(d.attachments[0].content_type, "application/pdf");
        let a = attachment(ALT, d.attachments[0].index).unwrap();
        assert_eq!(a.data, b"%PDF-1.4");
    }

    #[test]
    fn remote_images_allowed_on_request() {
        let d = detail("INBOX", 7, ALT, Flags::default(), true);
        assert!(d.html.unwrap().contains("https://t.example/p.gif"));
        assert!(!d.remote_images_blocked);
    }

    #[test]
    fn thread_key_prefers_references_root() {
        let m = MessageParser::new().parse_headers(ALT).unwrap();
        assert_eq!(thread_key(&m, 1), "m0@example.test");
        let m = MessageParser::new()
            .parse_headers(b"Message-ID: <solo@x>\r\n\r\n".as_slice())
            .unwrap();
        assert_eq!(thread_key(&m, 1), "solo@x");
    }

    #[test]
    fn print_document_escapes_headers_and_keeps_body_safe() {
        let raw = b"From: \"<script>x</script>\" <a@x>\r\nSubject: <b>hi</b>\r\nContent-Type: text/html\r\n\r\n<p>ok</p><script>bad()</script>\r\n";
        let doc = print_document(raw, false);
        assert!(doc.contains("&lt;b&gt;hi&lt;/b&gt;"));
        assert!(doc.contains("&lt;script&gt;x"));
        assert!(doc.contains("<p>ok</p>"));
        assert!(!doc.contains("bad()"));
        assert_eq!(doc.matches("<script>").count(), 1, "only the print script");
        assert!(doc.contains(PRINT_SCRIPT));
    }

    #[test]
    fn plain_text_message_has_no_html() {
        let raw = b"From: a@x\r\nSubject: hi\r\n\r\njust text <b>not html</b>\r\n";
        let d = detail("INBOX", 1, raw, Flags::default(), false);
        assert!(d.html.is_none());
        assert!(d.text.unwrap().contains("<b>not html</b>"));
    }
}
