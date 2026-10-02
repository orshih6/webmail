//! End-to-end against the dev stack: `dev/up.sh && source dev/dev.env && WEBMAIL_IT=1 cargo test --test integration`.
//! Skipped (passes trivially) without WEBMAIL_IT=1 so plain `cargo test` needs no services.

use std::time::Duration;

use axum::{
    Router,
    body::Body,
    http::{Request, Response, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use webmail::{AppState, app, config::Config, db};

struct Client {
    app: Router,
    cookie: String,
    csrf: String,
}

impl Client {
    async fn login(app: &Router, email: &str, password: &str) -> Result<Client, u16> {
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"email": email, "password": password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        if !res.status().is_success() {
            return Err(res.status().as_u16());
        }
        let cookie = res.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let body = json_body(res).await;
        Ok(Client {
            app: app.clone(),
            cookie,
            csrf: body["csrf"].as_str().unwrap().to_owned(),
        })
    }

    async fn req(&self, method: &str, path: &str, body: Option<Value>) -> Response<Body> {
        let mut r = Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, &self.cookie)
            .header("x-csrf-token", &self.csrf);
        if body.is_some() {
            r = r.header(header::CONTENT_TYPE, "application/json");
        }
        let body = body.map_or(Body::empty(), |b| Body::from(b.to_string()));
        self.app
            .clone()
            .oneshot(r.body(body).unwrap())
            .await
            .unwrap()
    }

    async fn get(&self, path: &str) -> Value {
        let res = self.req("GET", path, None).await;
        assert!(res.status().is_success(), "GET {path}: {}", res.status());
        json_body(res).await
    }

    async fn post(&self, path: &str, body: Value) -> u16 {
        let res = self.req("POST", path, Some(body)).await;
        let status = res.status().as_u16();
        if status >= 400 {
            panic!(
                "POST {path}: {status} {}",
                String::from_utf8_lossy(&res.into_body().collect().await.unwrap().to_bytes())
            );
        }
        status
    }

    async fn list(&self, folder: &str, q: &str) -> Vec<Value> {
        let path = format!("/api/messages?folder={}&q={}", enc(folder), enc(q));
        self.get(&path).await["messages"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// Polls until a message with `subject` appears in `folder` (delivery is asynchronous).
    async fn wait_for(&self, folder: &str, subject: &str) -> Value {
        for _ in 0..60 {
            if let Some(m) = self
                .list(folder, subject)
                .await
                .into_iter()
                .find(|m| m["subject"] == subject)
            {
                return m;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("{subject} never arrived in {folder}");
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

async fn json_body(res: Response<Body>) -> Value {
    serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

fn compose(to: &str, subject: &str, text: &str) -> Value {
    json!({"to": to, "cc": "", "bcc": "", "subject": subject, "text": text, "uploads": [],
           "keep": null, "in_reply_to": null, "references": [], "reply_of": null,
           "forward_of": null, "draft": null})
}

#[tokio::test]
async fn full_mail_flow() {
    if std::env::var("WEBMAIL_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set WEBMAIL_IT=1 with the dev stack running");
        return;
    }
    let config = Config::from_env().unwrap();
    let pool = db::connect(&config.database_url).await.unwrap();
    let app = app(AppState::new(config, pool));
    let tag: u32 = rand::random();

    // Sign-in: wrong password is a 401, right one works, CSRF is enforced.
    assert_eq!(
        Client::login(&app, "alice@example.test", "wrong")
            .await
            .err(),
        Some(401)
    );
    let alice = Client::login(&app, "alice@example.test", "alicepass")
        .await
        .unwrap();
    let bob = Client::login(&app, "bob@example.test", "bobpass")
        .await
        .unwrap();
    let no_csrf = Client {
        app: app.clone(),
        cookie: alice.cookie.clone(),
        csrf: "x".into(),
    };
    assert_eq!(
        no_csrf
            .req("POST", "/api/messages/flag", Some(json!({})))
            .await
            .status(),
        403
    );

    // Folders with special-use roles.
    let folders = alice.get("/api/folders").await;
    let specials: Vec<&str> = folders
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["special"].as_str())
        .collect();
    for s in ["inbox", "sent", "drafts", "trash", "junk"] {
        assert!(specials.contains(&s), "missing {s}: {folders}");
    }

    // Live updates: subscribe before mail arrives.
    let events = bob.req("GET", "/api/events", None).await;
    assert_eq!(events.status(), 200);
    let mut events = events.into_body();
    tokio::time::sleep(Duration::from_secs(1)).await; // let IDLE start

    // Upload an attachment and send alice → bob (with a non-ASCII subject).
    let boundary = "XyZ";
    let form = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"notes.txt\"\r\nContent-Type: text/plain\r\n\r\nattached bytes\r\n--{boundary}--\r\n"
    );
    let res = alice
        .app
        .clone()
        .oneshot(
            Request::post("/api/uploads")
                .header(header::COOKIE, &alice.cookie)
                .header("x-csrf-token", &alice.csrf)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let upload = json_body(res).await;

    let subject = format!("Сайн уу {tag}");
    let mut msg = compose("Bob <bob@example.test>", &subject, "Hello Bob");
    msg["uploads"] = json!([upload["id"]]);
    alice.post("/api/send", msg).await;

    // Bob is told about it over SSE.
    let frame = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let f = events.frame().await.expect("stream open").unwrap();
            if let Some(d) = f.data_ref()
                && String::from_utf8_lossy(d).contains("event: change")
            {
                return String::from_utf8_lossy(d).into_owned();
            }
        }
    })
    .await
    .expect("change event within 30s");
    assert!(frame.contains("INBOX"));

    // Bob lists, opens (marks read), downloads the attachment.
    let row = bob.wait_for("INBOX", &subject).await;
    assert_eq!(row["from"]["email"], "alice@example.test");
    assert_eq!(row["has_attachments"], true);
    assert_eq!(row["flags"]["seen"], false);
    let uid = row["uid"].as_u64().unwrap();
    let detail = bob
        .get(&format!("/api/message?folder=INBOX&uid={uid}"))
        .await;
    assert_eq!(detail["text"].as_str().unwrap().trim(), "Hello Bob");
    assert_eq!(detail["attachments"][0]["filename"], "notes.txt");
    let res = bob
        .req(
            "GET",
            &format!(
                "/api/attachment?folder=INBOX&uid={uid}&index={}",
                detail["attachments"][0]["index"]
            ),
            None,
        )
        .await;
    assert!(
        res.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    assert_eq!(
        res.into_body().collect().await.unwrap().to_bytes().as_ref(),
        b"attached bytes"
    );
    // Fetching a message never marks it read; the reader marks it explicitly.
    assert_eq!(
        bob.wait_for("INBOX", &subject).await["flags"]["seen"],
        false
    );
    bob.post(
        "/api/messages/flag",
        json!({"folder": "INBOX", "uids": [uid], "flag": "seen", "value": true}),
    )
    .await;
    assert_eq!(bob.wait_for("INBOX", &subject).await["flags"]["seen"], true);

    // Search finds it by a body word; flags toggle.
    assert!(
        bob.list("INBOX", "Hello")
            .await
            .iter()
            .any(|m| m["uid"].as_u64() == Some(uid))
    );
    bob.post(
        "/api/messages/flag",
        json!({"folder": "INBOX", "uids": [uid], "flag": "flagged", "value": true}),
    )
    .await;
    assert_eq!(
        bob.wait_for("INBOX", &subject).await["flags"]["flagged"],
        true
    );

    // Reply: threads with the original, marks it answered, lands in alice's Sent and INBOX.
    let mid = detail["message_id"].as_str().unwrap();
    let mut reply = compose("alice@example.test", &format!("Re: {subject}"), "Hi Alice");
    reply["in_reply_to"] = json!(mid);
    reply["references"] = json!([mid]);
    reply["reply_of"] = json!({"folder": "INBOX", "uid": uid});
    bob.post("/api/send", reply).await;
    assert_eq!(
        bob.wait_for("INBOX", &subject).await["flags"]["answered"],
        true
    );
    let got = alice.wait_for("INBOX", &format!("Re: {subject}")).await;
    let sent = alice.wait_for("Sent", &subject).await;
    assert_eq!(
        got["thread_key"], sent["thread_key"],
        "reply threads with the original"
    );

    // Conversation view from the reply in alice's INBOX includes her original in Sent.
    let t = alice
        .get(&format!("/api/thread?folder=INBOX&uid={}", got["uid"]))
        .await;
    let items = t["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{t}");
    assert_eq!(items[0]["folder"], "Sent", "oldest first");
    assert_eq!(items[0]["message"]["subject"], subject);
    assert_eq!(items[1]["folder"], "INBOX");

    // Draft: save, re-save replaces it, send removes it.
    let draft_subject = format!("draft {tag}");
    let res = alice
        .req(
            "POST",
            "/api/drafts",
            Some(compose("", &draft_subject, "v1")),
        )
        .await;
    let d1 = json_body(res).await["draft"].clone();
    let mut v2 = compose("bob@example.test", &draft_subject, "v2");
    v2["draft"] = d1.clone();
    let res = alice.req("POST", "/api/drafts", Some(v2.clone())).await;
    let d2 = json_body(res).await["draft"].clone();
    assert_ne!(d1["uid"], d2["uid"]);
    assert_eq!(alice.list("Drafts", &draft_subject).await.len(), 1);
    v2["draft"] = d2;
    alice.post("/api/send", v2).await;
    assert_eq!(alice.list("Drafts", &draft_subject).await.len(), 0);

    // Move reports destination UIDs, so moving back (Undo) needs no searching.
    let res = bob
        .req(
            "POST",
            "/api/messages/delete",
            Some(json!({"folder": "INBOX", "uids": [uid]})),
        )
        .await;
    let moved = json_body(res).await;
    assert_eq!(moved["to"], "Trash");
    assert_eq!(moved["uids"].as_array().unwrap().len(), 1);
    let res = bob
        .req(
            "POST",
            "/api/messages/move",
            Some(json!({"folder": "Trash", "uids": moved["uids"], "to": "INBOX"})),
        )
        .await;
    let back = json_body(res).await;
    assert_eq!(back["to"], "INBOX");
    let uid = back["uids"][0].as_u64().unwrap();
    assert_eq!(
        bob.wait_for("INBOX", &subject).await["uid"].as_u64(),
        Some(uid),
        "undo restored it"
    );

    // Junk, then delete → Trash, then empty Trash.
    bob.post(
        "/api/messages/junk",
        json!({"folder": "INBOX", "uids": [uid]}),
    )
    .await;
    let in_junk = bob.wait_for("Junk", &subject).await;
    bob.post(
        "/api/messages/delete",
        json!({"folder": "Junk", "uids": [in_junk["uid"]]}),
    )
    .await;
    bob.wait_for("Trash", &subject).await;
    let res = bob
        .req(
            "POST",
            "/api/folders/empty",
            Some(json!({"folder": "INBOX"})),
        )
        .await;
    assert_eq!(res.status(), 400, "INBOX can never be emptied");
    bob.post("/api/folders/empty", json!({"folder": "Trash"}))
        .await;
    assert!(bob.list("Trash", "").await.is_empty());

    // Sign-out kills the session.
    bob.post("/api/logout", json!({})).await;
    assert_eq!(bob.req("GET", "/api/folders", None).await.status(), 401);
}

#[tokio::test]
async fn settings_identities_contacts() {
    if std::env::var("WEBMAIL_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set WEBMAIL_IT=1 with the dev stack running");
        return;
    }
    let config = Config::from_env().unwrap();
    let pool = db::connect(&config.database_url).await.unwrap();
    let app = app(AppState::new(config, pool));
    let tag: u32 = rand::random();
    let alice = Client::login(&app, "alice@example.test", "alicepass")
        .await
        .unwrap();
    let bob = Client::login(&app, "bob@example.test", "bobpass")
        .await
        .unwrap();

    // Prefs: an empty save restores the defaults; then round-trip and clamping.
    alice.post("/api/prefs", json!({})).await;
    // Start with an empty address book (other suites, e.g. the browser walkthrough, save to it).
    for c in alice.get("/api/contacts").await.as_array().unwrap() {
        alice
            .req("DELETE", &format!("/api/contacts/{}", c["id"]), None)
            .await;
    }
    let p = alice.get("/api/prefs").await;
    assert_eq!(p["page_size"], 50);
    let res = alice
        .req("POST", "/api/prefs", Some(json!({"page_size": 5000, "conversations": false, "theme": "dark", "remote_images": "contacts"})))
        .await;
    let saved = json_body(res).await;
    assert_eq!(saved["page_size"], 200, "clamped");
    assert_eq!(alice.get("/api/prefs").await["theme"], "dark");

    // Identity: name + Reply-To land in the sent message; From stays the login address.
    let res = alice
        .req("POST", "/api/identities", Some(json!({"id": null, "name": format!("Alice {tag}"), "reply_to": "help@example.test", "signature": "-- \nAlice", "is_default": true})))
        .await;
    let ids = json_body(res).await;
    let id = ids
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == format!("Alice {tag}"))
        .unwrap()["id"]
        .clone();
    assert_eq!(ids[0]["email"], "alice@example.test");
    let res = alice
        .req("POST", "/api/identities", Some(json!({"id": null, "name": "x", "reply_to": "not an address", "signature": "", "is_default": false})))
        .await;
    assert_eq!(res.status(), 400);

    let subject = format!("identity test {tag}");
    let mut msg = compose(&format!("Bobby {tag} <bob@example.test>"), &subject, "hi");
    msg["identity"] = id.clone();
    alice.post("/api/send", msg).await;
    let row = bob.wait_for("INBOX", &subject).await;
    assert_eq!(row["from"]["name"], format!("Alice {tag}"));
    assert_eq!(row["from"]["email"], "alice@example.test");
    let d = bob
        .get(&format!("/api/message?folder=INBOX&uid={}", row["uid"]))
        .await;
    assert_eq!(d["reply_to"][0]["email"], "help@example.test");

    // The recipient was collected and autocompletes; it is not in the saved book yet.
    let hits = alice.get("/api/contacts?q=bob@exam").await;
    let bobc = hits
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["email"] == "bob@example.test")
        .expect("collected")
        .clone();
    assert_eq!(bobc["saved"], false);
    assert!(
        !alice
            .get("/api/contacts")
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["email"] == "bob@example.test")
    );

    // Saving promotes it; it is per-account.
    let res = alice
        .req(
            "POST",
            "/api/contacts",
            Some(json!({"id": null, "name": "Bob B", "email": "bob@example.test"})),
        )
        .await;
    let saved = json_body(res).await;
    assert_eq!(saved["saved"], true);
    assert_eq!(saved["id"], bobc["id"], "same row, promoted");
    assert!(
        alice
            .get("/api/contacts")
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["email"] == "bob@example.test")
    );
    assert!(
        !bob.get("/api/contacts?q=bob@example")
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == saved["id"])
    );

    // Bob cannot modify or delete Alice's rows by id.
    let res = bob
        .req(
            "POST",
            "/api/contacts",
            Some(json!({"id": saved["id"], "name": "hacked", "email": "x@example.test"})),
        )
        .await;
    assert_eq!(res.status(), 404);
    let res = bob
        .req("DELETE", &format!("/api/contacts/{}", saved["id"]), None)
        .await;
    assert_eq!(res.status(), 204, "no-op for someone else's id");
    assert!(
        alice
            .get("/api/contacts")
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == saved["id"])
    );
    let res = bob
        .req("DELETE", &format!("/api/identities/{id}"), None)
        .await;
    let left = json_body(res).await;
    assert!(
        left.as_array()
            .unwrap()
            .iter()
            .all(|i| i["email"] == "bob@example.test")
    );
    assert!(
        alice
            .get("/api/identities")
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == id)
    );

    // Clean up so reruns start from the same state.
    alice
        .req("DELETE", &format!("/api/identities/{id}"), None)
        .await;
    alice
        .req("DELETE", &format!("/api/contacts/{}", saved["id"]), None)
        .await;
    alice.req("POST", "/api/prefs", Some(json!({}))).await;
}

#[tokio::test]
async fn folder_management() {
    if std::env::var("WEBMAIL_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set WEBMAIL_IT=1 with the dev stack running");
        return;
    }
    let config = Config::from_env().unwrap();
    let pool = db::connect(&config.database_url).await.unwrap();
    let app = app(AppState::new(config, pool));
    let tag: u32 = rand::random();
    let bob = Client::login(&app, "bob@example.test", "bobpass")
        .await
        .unwrap();
    let names = |v: Value| -> Vec<(String, String)> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap().to_owned(),
                    f["name"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };

    // Create a non-ASCII top-level folder and a child.
    let top = format!("Төсөл {tag}");
    bob.post("/api/folders/create", json!({"parent": null, "name": top}))
        .await;
    let fs = names(bob.get("/api/folders").await);
    let (top_path, _) = fs.iter().find(|(_, n)| *n == top).expect("created").clone();
    assert_ne!(top_path, top, "stored in modified UTF-7 on the wire");
    bob.post(
        "/api/folders/create",
        json!({"parent": top_path, "name": "Child"}),
    )
    .await;
    let child_path = format!("{top_path}.Child");
    assert!(
        names(bob.get("/api/folders").await)
            .iter()
            .any(|(p, _)| *p == child_path)
    );

    // Duplicates and bad names are refused with a reason.
    let res = bob
        .req(
            "POST",
            "/api/folders/create",
            Some(json!({"parent": null, "name": top})),
        )
        .await;
    assert_eq!(res.status(), 400);
    let res = bob
        .req(
            "POST",
            "/api/folders/create",
            Some(json!({"parent": null, "name": "a.b"})),
        )
        .await;
    assert_eq!(res.status(), 400);

    // System folders are protected; a parent with children cannot be deleted.
    for f in ["INBOX", "Sent", "Trash"] {
        let res = bob
            .req("POST", "/api/folders/delete", Some(json!({"folder": f})))
            .await;
        assert_eq!(res.status(), 400, "{f}");
        let res = bob
            .req(
                "POST",
                "/api/folders/rename",
                Some(json!({"folder": f, "name": "x"})),
            )
            .await;
        assert_eq!(res.status(), 400, "{f}");
    }
    let res = bob
        .req(
            "POST",
            "/api/folders/delete",
            Some(json!({"folder": top_path})),
        )
        .await;
    assert_eq!(res.status(), 400, "has a child");

    // Move mail into the child, mark it all read.
    let subject = format!("for folders {tag}");
    bob.post("/api/send", compose("bob@example.test", &subject, "x"))
        .await;
    let m = bob.wait_for("INBOX", &subject).await;
    assert_eq!(m["flags"]["seen"], false);
    bob.post(
        "/api/messages/move",
        json!({"folder": "INBOX", "uids": [m["uid"]], "to": child_path}),
    )
    .await;
    bob.post("/api/folders/read", json!({"folder": child_path}))
        .await;
    assert_eq!(
        bob.wait_for(&child_path, &subject).await["flags"]["seen"],
        true
    );

    // Renaming the parent carries the child (and its mail) along.
    bob.post(
        "/api/folders/rename",
        json!({"folder": top_path, "name": format!("Renamed {tag}")}),
    )
    .await;
    let fs = names(bob.get("/api/folders").await);
    let (new_top, _) = fs
        .iter()
        .find(|(_, n)| *n == format!("Renamed {tag}"))
        .expect("renamed")
        .clone();
    let new_child = format!("{new_top}.Child");
    assert!(fs.iter().any(|(p, _)| *p == new_child));
    bob.wait_for(&new_child, &subject).await;

    // Search across all folders finds it in the subfolder, tagged with that folder.
    let res = bob.get(&format!("/api/search?q={}", enc(&subject))).await;
    let hits = res["hits"].as_array().unwrap();
    assert!(
        hits.iter()
            .any(|h| h["folder"] == new_child && h["message"]["subject"] == subject),
        "{res}"
    );
    let unread = bob
        .get(&format!("/api/search?q={}&filter=unread", enc(&subject)))
        .await;
    assert!(
        unread["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["folder"] != new_child),
        "it was marked read"
    );
    assert_eq!(bob.req("GET", "/api/search?q=", None).await.status(), 400);

    // Delete child, then parent.
    bob.post("/api/folders/delete", json!({"folder": new_child}))
        .await;
    bob.post("/api/folders/delete", json!({"folder": new_top}))
        .await;
    assert!(
        !names(bob.get("/api/folders").await)
            .iter()
            .any(|(p, _)| p.starts_with(&new_top))
    );
}

async fn upload(c: &Client, name: &str, ctype: &str, bytes: &[u8]) -> Value {
    let boundary = "B0undary";
    let mut form = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: {ctype}\r\n\r\n"
    )
    .into_bytes();
    form.extend_from_slice(bytes);
    form.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let res = c
        .app
        .clone()
        .oneshot(
            Request::post("/api/uploads")
                .header(header::COOKIE, &c.cookie)
                .header("x-csrf-token", &c.csrf)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    json_body(res).await
}

#[tokio::test]
async fn rich_text_and_reading_essentials() {
    if std::env::var("WEBMAIL_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set WEBMAIL_IT=1 with the dev stack running");
        return;
    }
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")
        .unwrap();
    let config = Config::from_env().unwrap();
    let pool = db::connect(&config.database_url).await.unwrap();
    let app = app(AppState::new(config, pool));
    let tag: u32 = rand::random();
    let alice = Client::login(&app, "alice@example.test", "alicepass")
        .await
        .unwrap();
    let bob = Client::login(&app, "bob@example.test", "bobpass")
        .await
        .unwrap();

    // A pasted image: previewable by its uploader only.
    let img = upload(&alice, "pasted.png", "image/png", &png).await;
    let id = img["id"].as_str().unwrap();
    let res = alice.req("GET", &format!("/api/uploads/{id}"), None).await;
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        bob.req("GET", &format!("/api/uploads/{id}"), None)
            .await
            .status(),
        404
    );
    let photo = upload(&alice, "photo.png", "image/png", &png).await;

    // Rich-text send with the inline image and a regular image attachment.
    let subject = format!("rich {tag}");
    let mut msg = compose("bob@example.test", &subject, "Hello bold world");
    msg["html"] = json!(format!(
        "<p>Hello <b>bold</b> world</p><p><img src=\"cid:{id}\"></p><script>evil()</script>"
    ));
    msg["inline_uploads"] = json!([id]);
    msg["uploads"] = json!([photo["id"]]);
    alice.post("/api/send", msg).await;
    assert_eq!(
        alice
            .req("GET", &format!("/api/uploads/{id}"), None)
            .await
            .status(),
        404,
        "consumed by send"
    );

    let row = bob.wait_for("INBOX", &subject).await;
    let uid = row["uid"].as_u64().unwrap();
    let d = bob
        .get(&format!("/api/message?folder=INBOX&uid={uid}"))
        .await;
    let html = d["html"].as_str().unwrap();
    assert!(html.contains("<b>bold</b>"));
    assert!(
        html.contains("data:image/png;base64,"),
        "inline image rendered"
    );
    assert!(!html.contains("evil"));
    assert_eq!(d["text"].as_str().unwrap().trim(), "Hello bold world");
    assert_eq!(
        d["attachments"].as_array().unwrap().len(),
        1,
        "inline image not listed as attachment"
    );

    // Image attachment previews inline; with ?inline=1 only, and always sandboxed.
    let idx = d["attachments"][0]["index"].as_u64().unwrap();
    let res = bob
        .req(
            "GET",
            &format!("/api/attachment?folder=INBOX&uid={uid}&index={idx}&inline=1"),
            None,
        )
        .await;
    assert!(
        res.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("inline")
    );
    assert!(
        res.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .starts_with("sandbox")
    );
    let res = bob
        .req(
            "GET",
            &format!("/api/attachment?folder=INBOX&uid={uid}&index={idx}"),
            None,
        )
        .await;
    assert!(
        res.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );

    // View source is inert text; download is a .eml named after the subject.
    let res = bob
        .req(
            "GET",
            &format!("/api/message/raw?folder=INBOX&uid={uid}"),
            None,
        )
        .await;
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "text/plain; charset=utf-8"
    );
    let src =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(src.contains(&format!("Subject: {subject}")));
    let res = bob
        .req(
            "GET",
            &format!("/api/message/raw?folder=INBOX&uid={uid}&download=1"),
            None,
        )
        .await;
    assert_eq!(res.headers()[header::CONTENT_TYPE], "message/rfc822");
    assert!(
        res.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains(&format!("rich {tag}.eml"))
    );

    // Print view: its own document, one hash-allowed script, no message script.
    let res = bob
        .req(
            "GET",
            &format!("/api/message/print?folder=INBOX&uid={uid}"),
            None,
        )
        .await;
    let csp = res.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(csp.contains("script-src 'sha256-"));
    let page =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(page.contains(&subject) && page.contains("<b>bold</b>") && !page.contains("evil"));

    // Filters: unread / flagged.
    let unread = format!(
        "/api/messages?folder=INBOX&filter=unread&q={}",
        enc(&subject)
    );
    bob.post(
        "/api/messages/flag",
        json!({"folder": "INBOX", "uids": [uid], "flag": "seen", "value": true}),
    )
    .await;
    assert!(
        bob.get(&unread).await["messages"]
            .as_array()
            .unwrap()
            .is_empty(),
        "marked read"
    );
    bob.post(
        "/api/messages/flag",
        json!({"folder": "INBOX", "uids": [uid], "flag": "seen", "value": false}),
    )
    .await;
    assert_eq!(
        bob.get(&unread).await["messages"].as_array().unwrap().len(),
        1
    );
    let flagged = format!(
        "/api/messages?folder=INBOX&filter=flagged&q={}",
        enc(&subject)
    );
    assert!(
        bob.get(&flagged).await["messages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    bob.post(
        "/api/messages/flag",
        json!({"folder": "INBOX", "uids": [uid], "flag": "flagged", "value": true}),
    )
    .await;
    assert_eq!(
        bob.get(&flagged).await["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn message_cache_keeps_flags_fresh_and_users_apart() {
    if std::env::var("WEBMAIL_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set WEBMAIL_IT=1 with the dev stack running");
        return;
    }
    let config = Config::from_env().unwrap();
    let pool = db::connect(&config.database_url).await.unwrap();
    let app = app(AppState::new(config, pool));
    let tag: u32 = rand::random();
    let alice = Client::login(&app, "alice@example.test", "alicepass")
        .await
        .unwrap();
    let bob = Client::login(&app, "bob@example.test", "bobpass")
        .await
        .unwrap();

    let subject = format!("cache me {tag}");
    alice
        .post(
            "/api/send",
            compose("bob@example.test", &subject, "body to cache"),
        )
        .await;
    let uid = bob.wait_for("INBOX", &subject).await["uid"]
        .as_u64()
        .unwrap();
    let url = format!("/api/message?folder=INBOX&uid={uid}");

    // Miss, then a hit: same content, but flags always current.
    let first = bob.get(&url).await;
    assert_eq!(first["flags"]["flagged"], false);
    bob.post(
        "/api/messages/flag",
        json!({"folder": "INBOX", "uids": [uid], "flag": "flagged", "value": true}),
    )
    .await;
    let second = bob.get(&url).await;
    assert_eq!(
        second["flags"]["flagged"], true,
        "flags are never served from the cache"
    );
    assert_eq!(second["text"], first["text"]);
    assert_eq!(second["subject"], subject);

    // Same folder name + UID in another account is a different message: never shared.
    // Each user files one fresh message into a new folder of the same name, so both
    // folders hold exactly UID 1.
    let folder = format!("Iso{tag}");
    for (who, client) in [("alice", &alice), ("bob", &bob)] {
        let subj = format!("iso {who} {tag}");
        alice
            .post(
                "/api/send",
                compose(&format!("{who}@example.test"), &subj, "x"),
            )
            .await;
        let uid = client.wait_for("INBOX", &subj).await["uid"].clone();
        client
            .post(
                "/api/folders/create",
                json!({"parent": null, "name": folder}),
            )
            .await;
        client
            .post(
                "/api/messages/move",
                json!({"folder": "INBOX", "uids": [uid], "to": folder}),
            )
            .await;
    }
    let url = format!("/api/message?folder={folder}&uid=1");
    let bob_first = bob.get(&url).await; // warm bob's entry first
    assert_eq!(bob_first["subject"], format!("iso bob {tag}"));
    assert_eq!(
        alice.get(&url).await["subject"],
        format!("iso alice {tag}"),
        "alice sees her own"
    );
    assert_eq!(
        bob.get(&url).await["subject"],
        format!("iso bob {tag}"),
        "bob sees his own"
    );
    for client in [&alice, &bob] {
        client
            .post("/api/folders/delete", json!({"folder": folder}))
            .await;
    }
}
