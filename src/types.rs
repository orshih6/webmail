//! Everything the JSON API sends or accepts. `cargo test` exports these to
//! `web/src/lib/api/types/` (ts-rs), so the Svelte app never hand-copies a shape.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[ts(export)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Special {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
}

#[derive(Serialize, TS, Clone, Debug)]
#[ts(export)]
pub struct Folder {
    /// The IMAP name, exactly as the server spells it (modified UTF-7). Send this back.
    pub path: String,
    /// Human-readable leaf name.
    pub name: String,
    pub delimiter: Option<String>,
    pub special: Option<Special>,
    pub selectable: bool,
    pub total: u32,
    pub unread: u32,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, Default, PartialEq)]
#[ts(export)]
pub struct Flags {
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub forwarded: bool,
    pub draft: bool,
}

#[derive(Serialize, TS, Clone, Debug)]
#[ts(export)]
pub struct MessageSummary {
    pub uid: u32,
    pub subject: String,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    /// RFC 3339.
    pub date: Option<String>,
    pub size: u32,
    pub flags: Flags,
    pub has_attachments: bool,
    /// Messages with the same key belong to one conversation (root of References).
    pub thread_key: String,
    pub message_id: Option<String>,
    /// How many ancestors the message names (References, else In-Reply-To): a reply is
    /// always deeper than what it answers, which orders a thread when dates tie.
    pub depth: u32,
}

#[derive(Serialize, TS, Debug)]
#[ts(export)]
pub struct MessagePage {
    pub folder: String,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
    pub messages: Vec<MessageSummary>,
}

#[derive(Serialize, TS, Clone, Debug)]
#[ts(export)]
pub struct Attachment {
    pub index: u32,
    pub filename: String,
    pub content_type: String,
    pub size: u32,
}

#[derive(Serialize, TS, Debug)]
#[ts(export)]
pub struct MessageDetail {
    pub folder: String,
    pub uid: u32,
    pub subject: String,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// Only present on your own stored copies (Sent, Drafts).
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub date: Option<String>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub flags: Flags,
    /// A complete, sanitized HTML document for a sandboxed `<iframe srcdoc>`. Never insert
    /// it into the app's own DOM.
    pub html: Option<String>,
    pub text: Option<String>,
    /// True when remote images were stripped; re-request with `images=1` to allow them.
    pub remote_images_blocked: bool,
    pub attachments: Vec<Attachment>,
}

#[derive(Serialize, TS, Debug)]
#[ts(export)]
pub struct SessionInfo {
    pub email: String,
    pub csrf: String,
}

#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[ts(export)]
pub struct MessageRef {
    pub folder: String,
    pub uid: u32,
}

#[derive(Deserialize, TS, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum FlagName {
    Seen,
    Flagged,
}

#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct FlagRequest {
    pub folder: String,
    pub uids: Vec<u32>,
    pub flag: FlagName,
    pub value: bool,
}

#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct MoveRequest {
    pub folder: String,
    pub uids: Vec<u32>,
    pub to: String,
}

#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct UidsRequest {
    pub folder: String,
    pub uids: Vec<u32>,
}

#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct FolderRequest {
    pub folder: String,
}

#[derive(Serialize, TS, Debug)]
#[ts(export)]
pub struct Upload {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: u32,
}

/// Attachments to carry over from an existing message (forwarding, or reopening a draft).
#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct KeepAttachments {
    pub source: MessageRef,
    pub indices: Vec<u32>,
}

#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct ComposeRequest {
    /// Comma-separated, `Name <addr>` or bare addresses.
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub text: String,
    pub uploads: Vec<String>,
    pub keep: Option<KeepAttachments>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// Gets \Answered once sent.
    pub reply_of: Option<MessageRef>,
    /// Gets $Forwarded once sent.
    pub forward_of: Option<MessageRef>,
    /// The draft this compose was saved as; replaced on save, removed on send.
    pub draft: Option<MessageRef>,
    /// Rich-text body. When present, `text` is its plain-text alternative.
    #[serde(default)]
    pub html: Option<String>,
    /// Uploads the HTML references as `cid:<upload id>` (pasted / dropped images).
    #[serde(default)]
    pub inline_uploads: Vec<String>,
    /// Sender name / Reply-To come from this identity; `None` uses the default.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub identity: Option<i64>,
}

#[derive(Serialize, TS, Debug)]
#[ts(export)]
pub struct DraftSaved {
    pub draft: Option<MessageRef>,
}

#[derive(Serialize, TS, Clone, Debug)]
#[ts(export)]
pub struct LiveEvent {
    /// The folder whose contents changed.
    pub folder: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// When remote images in HTML mail load without asking.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum RemoteImages {
    #[default]
    Never,
    /// Only for senders in the address book.
    Contacts,
    Always,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(default)]
#[ts(export)]
pub struct Prefs {
    pub page_size: u32,
    pub conversations: bool,
    /// Start new messages in the rich-text editor.
    pub compose_html: bool,
    pub theme: Theme,
    pub remote_images: RemoteImages,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            page_size: 50,
            conversations: true,
            compose_html: false,
            theme: Theme::System,
            remote_images: RemoteImages::Never,
        }
    }
}

#[derive(Serialize, TS, Clone, Debug)]
#[ts(export)]
pub struct Identity {
    /// Absent for the built-in identity of an account that has not saved any.
    #[ts(type = "number | null")]
    pub id: Option<i64>,
    /// Always the login address.
    pub email: String,
    pub name: String,
    pub reply_to: String,
    pub signature: String,
    pub is_default: bool,
}

#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct IdentityInput {
    #[ts(type = "number | null")]
    pub id: Option<i64>,
    pub name: String,
    pub reply_to: String,
    pub signature: String,
    pub is_default: bool,
}

#[derive(Serialize, TS, Clone, Debug)]
#[ts(export)]
pub struct Contact {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub email: String,
    /// False for addresses only collected from sent mail.
    pub saved: bool,
}

#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct ContactInput {
    #[ts(type = "number | null")]
    pub id: Option<i64>,
    pub name: String,
    pub email: String,
}

#[derive(Serialize, TS, Debug)]
#[ts(export)]
pub struct PublicConfig {
    pub app_name: String,
    pub version: String,
    pub license: String,
    /// Where to get the source of the version running here (AGPL-3.0 §13).
    pub source_url: String,
}

#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct CreateFolderRequest {
    /// Wire path of the parent, or `None` for top level.
    pub parent: Option<String>,
    /// Display name; the server encodes it.
    pub name: String,
}

#[derive(Deserialize, TS, Debug)]
#[ts(export)]
pub struct RenameFolderRequest {
    pub folder: String,
    /// New display name for the last path segment.
    pub name: String,
}

/// Where messages went, so the UI can offer Undo (a move back from `to`).
#[derive(Serialize, TS, Debug)]
#[ts(export)]
pub struct MoveResult {
    /// Destination folder; `None` when the messages were deleted permanently.
    pub to: Option<String>,
    /// Their UIDs in `to`.
    pub uids: Vec<u32>,
}

#[derive(Serialize, TS, Debug)]
#[ts(export)]
pub struct SearchHit {
    /// Wire path of the folder the message is in.
    pub folder: String,
    pub message: MessageSummary,
}

#[derive(Serialize, TS, Debug)]
#[ts(export)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    /// True when more messages matched than are returned.
    pub truncated: bool,
}

/// A conversation: every message sharing the opened one's thread, across its folder,
/// Inbox and Sent, oldest first, one entry per Message-ID.
#[derive(Serialize, TS, Debug)]
#[ts(export)]
pub struct Thread {
    pub items: Vec<SearchHit>,
}
