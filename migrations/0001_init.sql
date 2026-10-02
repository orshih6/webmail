-- A session row never holds anything usable on its own: `id_hash` is SHA-256 of the random id
-- in the cookie, and `secret` (the IMAP password) is AES-256-GCM encrypted with a key that
-- exists only in the cookie. A database dump yields neither sessions nor passwords.
CREATE TABLE sessions (
    id_hash      BYTEA PRIMARY KEY,
    email        TEXT NOT NULL,
    nonce        BYTEA NOT NULL,
    secret       BYTEA NOT NULL,
    csrf         TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX sessions_last_seen_idx ON sessions (last_seen_at);

-- Compose attachments, staged between upload and send/save-draft.
CREATE TABLE uploads (
    id           TEXT PRIMARY KEY,
    session_hash BYTEA NOT NULL REFERENCES sessions (id_hash) ON DELETE CASCADE,
    filename     TEXT NOT NULL,
    content_type TEXT NOT NULL,
    data         BYTEA NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX uploads_session_idx ON uploads (session_hash);
