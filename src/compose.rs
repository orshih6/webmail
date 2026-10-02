//! Building outgoing messages and handing them to SMTP.

use std::{str::FromStr, time::Duration};

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    address::Envelope,
    message::{
        Attachment as LAttachment, Mailbox, Mailboxes, MultiPart, SinglePart, header::ContentType,
    },
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
};

use crate::{
    config::Config,
    error::{AppError, AppResult},
    types::ComposeRequest,
};

pub struct FileData {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// An image the HTML body shows inline, referenced as `cid:<cid>`.
pub struct InlineImage {
    pub cid: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// Outgoing HTML is cleaned too: the user's own editor produced it, but nothing guarantees
/// the request did, and we never want to be the server that sent script to someone.
pub fn clean_outgoing_html(html: &str) -> String {
    use std::{borrow::Cow, collections::HashSet};
    let mut b = ammonia::Builder::default();
    b.add_generic_attributes(&["align"])
        .url_schemes(HashSet::from(["http", "https", "mailto", "cid", "data"]))
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("img", "src") => {
                let v = value.trim().to_ascii_lowercase();
                (v.starts_with("cid:") || v.starts_with("https:") || v.starts_with("data:image/"))
                    .then_some(Cow::Borrowed(value))
            }
            ("a", "href") => {
                let v = value.trim().to_ascii_lowercase();
                (v.starts_with("http:") || v.starts_with("https:") || v.starts_with("mailto:"))
                    .then_some(Cow::Borrowed(value))
            }
            _ => Some(Cow::Borrowed(value)),
        });
    b.clean(html).to_string()
}

pub struct Outgoing {
    pub message_id: String,
    /// (name, address) of every To/Cc/Bcc recipient, for the address book.
    pub recipients: Vec<(String, String)>,
    /// What goes over SMTP: no Bcc header.
    pub wire: Message,
    /// What is stored in Sent/Drafts: keeps Bcc so the sender can see it.
    pub stored: Vec<u8>,
}

fn mailboxes(field: &str, s: &str) -> AppResult<Vec<Mailbox>> {
    let s = s.trim().trim_end_matches(',').trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    Mailboxes::from_str(s)
        .map(|m| m.into_iter().collect())
        .map_err(|_| AppError::BadRequest(format!("{field}: invalid address list")))
}

fn ids(refs: &[String]) -> String {
    refs.iter()
        .map(|r| format!("<{}>", r.trim_matches(|c| c == '<' || c == '>')))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The sender as the message will show it: the login address, with an identity's name.
pub struct Sender<'a> {
    pub app_name: &'a str,
    pub email: &'a str,
    pub name: &'a str,
    pub reply_to: &'a str,
}

pub fn build(
    sender: &Sender,
    req: &ComposeRequest,
    files: &[FileData],
    inline: &[InlineImage],
    need_recipients: bool,
) -> AppResult<Outgoing> {
    let html = req
        .html
        .as_deref()
        .filter(|h| !h.trim().is_empty())
        .map(clean_outgoing_html);
    let email: lettre::Address = sender
        .email
        .parse()
        .map_err(|_| AppError::BadRequest("invalid sender".into()))?;
    let from = Mailbox::new(
        Some(sender.name.trim().to_owned()).filter(|n| !n.is_empty()),
        email,
    );
    let reply_to: Option<Mailbox> = match sender.reply_to.trim() {
        "" => None,
        r => Some(
            r.parse()
                .map_err(|_| AppError::BadRequest("invalid Reply-To".into()))?,
        ),
    };
    let to = mailboxes("To", &req.to)?;
    let cc = mailboxes("Cc", &req.cc)?;
    let bcc = mailboxes("Bcc", &req.bcc)?;
    if need_recipients && to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(AppError::BadRequest("add at least one recipient".into()));
    }

    let domain = from.email.domain().to_owned();
    let message_id = format!(
        "{}@{domain}",
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            rand::random::<[u8; 18]>()
        )
    );

    let make = |keep_bcc: bool| -> AppResult<Message> {
        let mut b = Message::builder()
            .from(from.clone())
            .subject(req.subject.trim())
            .message_id(Some(format!("<{message_id}>")))
            .date_now()
            .user_agent(sender.app_name.to_owned());
        if let Some(r) = &reply_to {
            b = b.reply_to(r.clone());
        }
        for m in &to {
            b = b.to(m.clone());
        }
        for m in &cc {
            b = b.cc(m.clone());
        }
        for m in &bcc {
            b = b.bcc(m.clone());
        }
        if keep_bcc {
            b = b.keep_bcc();
        }
        if to.is_empty() && cc.is_empty() && bcc.is_empty() {
            // Only reachable for drafts. lettre derives the SMTP envelope from the recipients
            // and refuses an empty one; a draft is never sent, so any envelope will do.
            let me = from.email.clone();
            b = b.envelope(
                Envelope::new(Some(me.clone()), vec![me])
                    .map_err(|e| AppError::BadRequest(e.to_string()))?,
            );
        }
        if let Some(irt) = req.in_reply_to.as_deref().filter(|s| !s.is_empty()) {
            b = b.in_reply_to(ids(&[irt.to_owned()]));
        }
        if !req.references.is_empty() {
            b = b.references(ids(&req.references));
        }
        let text = SinglePart::plain(req.text.replace("\r\n", "\n"));
        // text | alternative(text, html | related(html, images...)), then mixed with files.
        let body = match &html {
            None => None,
            Some(h) => {
                let used: Vec<&InlineImage> = inline
                    .iter()
                    .filter(|i| h.contains(&format!("cid:{}", i.cid)))
                    .collect();
                let html_part = SinglePart::html(h.clone());
                let alt = MultiPart::alternative().singlepart(text.clone());
                Some(if used.is_empty() {
                    alt.singlepart(html_part)
                } else {
                    let mut rel = MultiPart::related().singlepart(html_part);
                    for i in used {
                        let ct = ContentType::parse(&i.content_type)
                            .map_err(|_| AppError::BadRequest("unsupported inline image".into()))?;
                        rel = rel.singlepart(
                            LAttachment::new_inline(i.cid.clone()).body(i.data.clone(), ct),
                        );
                    }
                    alt.multipart(rel)
                })
            }
        };
        let msg = match (body, files.is_empty()) {
            (None, true) => b.singlepart(text),
            (Some(alt), true) => b.multipart(alt),
            (body, false) => {
                let mut mp = match body {
                    Some(alt) => MultiPart::mixed().multipart(alt),
                    None => MultiPart::mixed().singlepart(text),
                };
                for f in files {
                    let ct = ContentType::parse(&f.content_type).unwrap_or_else(|_| {
                        ContentType::parse("application/octet-stream").expect("valid")
                    });
                    mp = mp
                        .singlepart(LAttachment::new(f.filename.clone()).body(f.data.clone(), ct));
                }
                b.multipart(mp)
            }
        };
        msg.map_err(|e| AppError::BadRequest(e.to_string()))
    };

    let recipients = to
        .iter()
        .chain(&cc)
        .chain(&bcc)
        .map(|m| (m.name.clone().unwrap_or_default(), m.email.to_string()))
        .collect();
    Ok(Outgoing {
        recipients,
        wire: make(false)?,
        stored: make(true)?.formatted(),
        message_id,
    })
}

pub async fn send(cfg: &Config, user: &str, pass: &str, msg: &Message) -> AppResult<()> {
    let tls = TlsParameters::builder(cfg.smtp_host.clone())
        .dangerous_accept_invalid_certs(cfg.tls_accept_invalid_certs)
        .build_rustls()
        .map_err(|e| AppError::Mail(format!("smtp tls: {e}")))?;
    let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(cfg.smtp_host.as_str())
        .port(cfg.smtp_port)
        .tls(Tls::Wrapper(tls))
        .credentials(Credentials::new(user.to_owned(), pass.to_owned()))
        .timeout(Some(Duration::from_secs(60)))
        .build();
    transport.send(msg.clone()).await.map_err(|e| {
        if e.is_permanent() {
            AppError::BadRequest(format!("rejected by the mail server: {e}"))
        } else {
            AppError::Mail(format!("send: {e}"))
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests_support {
    use super::*;

    pub fn req() -> ComposeRequest {
        ComposeRequest {
            to: "Bob <bob@example.test>, carol@example.test".into(),
            cc: "".into(),
            bcc: "secret@example.test".into(),
            subject: "Hi".into(),
            text: "hello\r\nworld".into(),
            uploads: vec![],
            keep: None,
            in_reply_to: Some("m1@example.test".into()),
            references: vec!["m0@example.test".into(), "<m1@example.test>".into()],
            reply_of: None,
            forward_of: None,
            draft: None,
            identity: None,
            html: None,
            inline_uploads: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{tests_support::req, *};

    const ME: Sender = Sender {
        app_name: "Webmail",
        email: "alice@example.test",
        name: "",
        reply_to: "",
    };

    #[test]
    fn bcc_only_in_stored_copy() {
        let o = build(&ME, &req(), &[], &[], true).unwrap();
        let wire = String::from_utf8(o.wire.formatted()).unwrap();
        let stored = String::from_utf8(o.stored).unwrap();
        assert!(!wire.contains("secret@example.test"));
        assert!(stored.contains("secret@example.test"));
        assert!(wire.contains(&format!("Message-ID: <{}>", o.message_id)));
        assert!(wire.contains("In-Reply-To: <m1@example.test>"));
        assert!(wire.contains("References: <m0@example.test> <m1@example.test>"));
        let env = o.wire.envelope();
        assert_eq!(env.to().len(), 3, "bcc still receives it");
    }

    #[test]
    fn attachments_and_validation() {
        let files = [FileData {
            filename: "a.txt".into(),
            content_type: "text/plain".into(),
            data: b"x".to_vec(),
        }];
        let o = build(&ME, &req(), &files, &[], true).unwrap();
        assert!(
            String::from_utf8(o.stored)
                .unwrap()
                .contains("multipart/mixed")
        );

        let mut r = req();
        r.to = "not an address".into();
        assert!(matches!(
            build(&ME, &r, &[], &[], true),
            Err(AppError::BadRequest(_))
        ));

        let mut r = req();
        (r.to, r.bcc) = (String::new(), String::new());
        assert!(build(&ME, &r, &[], &[], true).is_err());
        assert!(
            build(&ME, &r, &[], &[], false).is_ok(),
            "drafts may lack recipients"
        );
    }
}

#[cfg(test)]
mod identity_tests {
    use super::{tests_support::req, *};

    #[test]
    fn identity_sets_display_name_and_reply_to() {
        let s = Sender {
            app_name: "Webmail",
            email: "alice@example.test",
            name: "Alice Smith",
            reply_to: "support@example.test",
        };
        let o = build(&s, &req(), &[], &[], true).unwrap();
        let wire = String::from_utf8(o.wire.formatted()).unwrap();
        assert!(
            wire.contains(r#"From: "Alice Smith" <alice@example.test>"#),
            "{wire}"
        );
        assert!(wire.contains("Reply-To: support@example.test"));
        assert_eq!(o.recipients.len(), 3);
        assert!(
            o.recipients
                .contains(&("Bob".into(), "bob@example.test".into()))
        );
    }
}

#[cfg(test)]
mod html_tests {
    use super::{tests_support::req, *};

    const ME: Sender = Sender {
        app_name: "Webmail",
        email: "alice@example.test",
        name: "",
        reply_to: "",
    };

    #[test]
    fn html_goes_out_as_alternative_with_inline_images() {
        let mut r = req();
        r.html = Some(r#"<p>Hi <b>Bob</b><img src="cid:img1"><script>x()</script><a href="javascript:x()">j</a></p>"#.into());
        r.text = "Hi Bob".into();
        let img = InlineImage {
            cid: "img1".into(),
            content_type: "image/png".into(),
            data: b"PNG".to_vec(),
        };
        let unused = InlineImage {
            cid: "gone".into(),
            content_type: "image/png".into(),
            data: b"X".to_vec(),
        };
        let files = [FileData {
            filename: "a.txt".into(),
            content_type: "text/plain".into(),
            data: b"x".to_vec(),
        }];
        let o = build(&ME, &r, &files, &[img, unused], true).unwrap();
        let m = String::from_utf8(o.wire.formatted()).unwrap();
        assert!(m.contains("multipart/mixed"));
        assert!(m.contains("multipart/alternative"));
        assert!(m.contains("multipart/related"));
        assert!(m.contains("Content-ID: <img1>"));
        assert!(!m.contains("<gone>"), "unreferenced inline image dropped");
        assert!(m.contains("<b>Bob</b>"));
        assert!(!m.contains("x()"), "script and javascript: stripped");
        // And it parses back the way a recipient would see it.
        let d = crate::mime::detail("INBOX", 1, m.as_bytes(), Default::default(), false);
        assert!(d.html.unwrap().contains("data:image/png;base64,UE5H"));
        assert_eq!(d.text.unwrap().trim(), "Hi Bob");
        assert_eq!(d.attachments.len(), 1);
    }
}
