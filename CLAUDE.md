# CLAUDE.md

A general-purpose webmail client (a Roundcube-style rewrite) for any IMAP/SMTP server. One
binary: an axum server that talks IMAPS/SMTPS and serves a Svelte 5 SPA embedded at compile
time. Configured entirely by environment (`src/config.rs`, documented in README):
`IMAP_HOST`, `SMTP_HOST`, `DATABASE_URL` are required; `APP_NAME` brands the UI. Plans are in
`ROADMAP.md`. Licensed AGPL-3.0.

## Layout

- `src/` — Rust server. `api/` is the JSON API under `/api`, `assets.rs` serves the embedded
  `web/build/` with an `index.html` fallback for client routes.
- `web/` — SvelteKit (`adapter-static`, `ssr = false`), Svelte 5 runes, TypeScript, pnpm,
  Biome. No UI kit, no jQuery-style libraries: keep the bundle small.
- `docker-compose.yml` — webmail + Postgres for your own mail server (configured by `.env`,
  see `.env.example`); `docker-compose.demo.yml` — a self-contained trial with a bundled test
  mail server. `deploy/kubernetes/` — a generic Kubernetes example. None of these carry real
  hostnames or secrets.
- `dev/` — the docker-mailserver + Postgres stack for development and tests; `e2e/` — browser
  walkthroughs and the accessibility audit.

## Commands

```bash
cd web && pnpm install && pnpm build   # must run before any cargo build: rust-embed needs web/build
cargo run                              # :8080
cd web && pnpm dev                     # :5173, proxies /api → :8080 (in debug builds rust-embed
                                       # reads web/build from disk, so no rebuild needed for it)
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd web && pnpm lint && pnpm check

# against a real mail server (docker-mailserver + Postgres, self-signed TLS):
dev/up.sh && source dev/dev.env
cargo run                                          # sign in as alice@example.test / alicepass
WEBMAIL_IT=1 cargo test --test integration         # full API flow, incl. SMTP and IDLE
e2e/run.sh                                         # browser walkthroughs (needs `cargo run` up)
dev/seed-large.py 20000 && node e2e/bench.mjs      # big-mailbox timings (carol@example.test)
dev/seed-demo.py && node e2e/screenshots.mjs       # README screenshots (jane@example.test)
```

CI (`.github/workflows/ci.yml`) runs all of the above on every push, including the dev
stack, integration tests and browser walkthroughs; image builds reuse it. A UI change that
alters a flow should update the matching `e2e/*.mjs`, which reset their accounts first.

`cargo test` regenerates `web/src/lib/api/types/` from `src/types.rs` (ts-rs); commit the
result — CI fails if it is stale. Never edit those files by hand.

## Invariants

- **Auth is IMAP LOGIN**, as in Roundcube. There is no user table; a session holds the
  address and the password encrypted with a per-session key that exists only in the
  cookie (`src/session.rs`). Never log or return the password.
- **IMAP connections are per user, not per session** (`Auth::ukey`): Dovecot allows 10
  connections per user+IP and every webmail request comes from one IP. Never open a
  connection outside `imap::Pool` / the IDLE watcher.
- **Mail path**: implicit TLS only (IMAPS 993 / SMTPS 465); credentials never cross a
  plaintext socket.
- **Message HTML is hostile.** It is sanitized server-side (ammonia) *and* rendered only
  inside a `sandbox` iframe without `allow-scripts`/`allow-same-origin`. Remote images are
  blocked until the user opts in. Never `{@html}` message content into the app document.
- **CSP**: the page policy is a `<meta>` tag SvelteKit generates with the hash of its inline
  bootstrap (`web/vite.config.ts`, `csp`). The server's default header only adds
  `frame-ancestors`, and only when a handler set no CSP of its own (`if_not_present`):
  responses carrying message bytes (attachments, view source, print) set a stricter
  `sandbox` policy and must never be overridden.
- **Only raster images render inline** (`mime::previewable`: png/jpeg/gif/webp). SVG and
  everything else is download-only.
- Unknown `/api/*` paths are 404, never the SPA fallback.
- **Sorted UID lists are cached per user** (`Conn::sorted`), keyed on UIDVALIDITY +
  HIGHESTMODSEQ + EXISTS from `SELECT (CONDSTORE)`. Any change to the folder (arrival,
  expunge, flag) moves HIGHESTMODSEQ, so a hit is never stale; servers without CONDSTORE
  are simply not cached. Don't add a cache keyed on anything weaker.
- **Opened messages are cached** (`src/cache.rs`, server; `MessageView`, per tab) keyed on
  user + folder + UIDVALIDITY + UID (+ remote-images rendering): content can't change under
  that key. **Flags are never served from a cache** — always fetched fresh. Sign-out drops
  the user's entries on both sides. Prefetching must never mark anything read.
- **From is always the login address.** Identities change only the display name, Reply-To
  and signature. Many mail servers don't enforce sender ownership, so a user-chosen From
  address would let anyone send as anyone.
- **Per-account rows (prefs, identities, contacts) are keyed by `owner` = login address**
  and every query filters on it (`src/settings.rs`). Never act on a client-supplied id
  without the owner in the WHERE clause.
