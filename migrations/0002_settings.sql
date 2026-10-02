-- Per-account data, keyed by the login address (lowercased at sign-in). Unlike sessions,
-- these outlive sign-out.

CREATE TABLE prefs (
    owner TEXT PRIMARY KEY,
    data  JSONB NOT NULL DEFAULT '{}'
);

-- Identities vary the display name, Reply-To and signature. The From address is always the
-- login address: the mail server does not enforce sender ownership (no SPOOF_PROTECTION),
-- so letting users type one would let anyone send as anyone.
CREATE TABLE identities (
    id         BIGSERIAL PRIMARY KEY,
    owner      TEXT NOT NULL,
    name       TEXT NOT NULL DEFAULT '',
    reply_to   TEXT NOT NULL DEFAULT '',
    signature  TEXT NOT NULL DEFAULT '',
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX identities_owner_idx ON identities (owner);
CREATE UNIQUE INDEX identities_one_default ON identities (owner) WHERE is_default;

-- Address book. Recipients of sent mail are collected automatically (use_count > 0, saved
-- = false) and rank first in autocomplete; contacts the user created or kept are saved = true.
CREATE TABLE contacts (
    id           BIGSERIAL PRIMARY KEY,
    owner        TEXT NOT NULL,
    name         TEXT NOT NULL DEFAULT '',
    email        TEXT NOT NULL,
    saved        BOOLEAN NOT NULL DEFAULT false,
    use_count    INTEGER NOT NULL DEFAULT 0,
    last_used_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX contacts_owner_email ON contacts (owner, lower(email));
