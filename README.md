<p align="center">
  <img src="web/static/icons/logo.svg" width="88" height="88" alt="">
</p>

<h1 align="center">Webmail</h1>

<p align="center">
  A fast, secure, self-hosted webmail client for any IMAP/SMTP server.<br>
  One small binary — Rust on the server, Svelte in the browser.
</p>

<p align="center">
  <a href="https://github.com/orshih6/webmail/actions/workflows/ci.yml"><img src="https://github.com/orshih6/webmail/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue" alt="License: AGPL-3.0"></a>
</p>

<p align="center">
  <img src="docs/screenshots/inbox-light.png" alt="Inbox with a conversation open" width="860">
</p>

## Why

Roundcube showed that a webmail client can be a small piece of software you run next to
your mail server. This is that idea rebuilt with a modern stack: a single Rust binary
(a ~70 MB image using ~10 MB of memory) that serves a Svelte app of about 60 KB of gzipped
JavaScript, talks to your existing IMAP/SMTP server, and needs only Postgres beside it.
It has no user database of its own — people sign in with their mail account.

## Features

- **Mail** — threaded conversations across Inbox and Sent, search in one or all folders,
  unread/starred filters, star, move, junk, delete with **Undo**, drag messages onto
  folders, live updates (IMAP IDLE → server-sent events).
- **Writing** — plain text or rich text (formatting, links, pasted images sent inline),
  attachments up to 25 MB, drafts that autosave, reply/reply-all/forward, identities with
  signatures, address book with autocomplete.
- **Folders** — create, nest, rename, delete, mark all read; Unicode names.
- **Reading** — view source, download `.eml`, print, image attachment previews.
- **Built for real use** — fast on 20,000-message folders (~12 ms per page), a calm
  "reconnecting" banner when the mail server is slow or down, sign back in over the page
  when a session expires without losing what you were writing.
- **Accessible** — keyboard-first (`j`/`k`, `r`, `#`, `/`… see Settings → About), screen
  reader labels and announcements, WCAG 2.1 AA contrast in light and dark themes,
  reduced motion. Checked automatically in CI.
- **Mobile** — one-pane layout with a folder drawer; installable as an app.

<p align="center">
  <img src="docs/screenshots/compose-dark.png" alt="Rich-text compose in the dark theme" width="600">
  <img src="docs/screenshots/mobile.png" alt="Phone layout" width="200">
</p>

## Security model

Email is hostile input, and a webmail client holds the keys to the mailbox. The design
assumes both:

- **Message HTML never runs.** It is sanitized on the server (ammonia) *and* shown only in
  an `<iframe sandbox>` with neither scripts nor same-origin access. Remote images and CSS
  are stripped until the user asks (or per preference: never / from contacts / always).
- **Passwords are never stored in recoverable form.** Signing in is an IMAP LOGIN. The
  session row holds the password encrypted under a key that exists only in the user's
  cookie, so a database dump alone yields nothing usable.
- **Strict CSP** (hashed inline bootstrap, no third-party origins), CSRF tokens on every
  mutation, `SameSite=Strict` cookies, login rate limiting per IP and per account.
- **Implicit TLS only** to the mail server (IMAPS 993, SMTPS 465).
- **The From address is always the signed-in account.** Identities change only the
  display name, Reply-To and signature.
- Attachments, "view source" and print pages are served under a `sandbox` CSP; only
  raster images (never SVG) are displayed inline.

Found a problem? See [SECURITY.md](SECURITY.md).

## Quick start

You need an IMAP/SMTP server reachable over IMAPS (993) and SMTPS (465).

```bash
curl -O https://raw.githubusercontent.com/orshih6/webmail/main/deploy/docker-compose.yml
IMAP_HOST=mail.example.com POSTGRES_PASSWORD=$(openssl rand -hex 16) \
  docker compose up -d
```

Then put an HTTPS reverse proxy (Caddy, nginx, Traefik…) in front of port 8080 and open
it. Session cookies are `Secure`, so plain http only works for local testing with
`COOKIE_SECURE=0`. A Kubernetes example is in [`deploy/kubernetes/`](deploy/kubernetes/).

## Configuration

All configuration is environment variables.

| Variable | Default | |
|---|---|---|
| `IMAP_HOST` | — (required) | IMAPS server |
| `SMTP_HOST` | — (required) | SMTPS server |
| `DATABASE_URL` | — (required) | Postgres, e.g. `postgres://user:pass@host/webmail`; migrations run at startup |
| `IMAP_PORT` / `SMTP_PORT` | `993` / `465` | implicit TLS ports |
| `APP_NAME` | `Webmail` | name shown in the UI, page titles, installed app and `User-Agent` |
| `SOURCE_URL` | this repository | link to the source of the version you run (AGPL §13 — point it at your fork if you modify it) |
| `MAIL_TIMEOUT_SECS` | `30` | how long a request may wait on the mail server (sending gets 4×) |
| `LISTEN_ADDR` | `0.0.0.0:8080` | |
| `COOKIE_SECURE` | `1` | `0` only for local http testing |
| `TLS_ACCEPT_INVALID_CERTS` | `0` | `1` only for a dev server with a self-signed certificate |

Run a single instance: IMAP connections are pooled in memory (one per user, plus one for
live updates — Dovecot's default `mail_max_userip_connections` of 10 is plenty).

## Development

```bash
dev/up.sh                       # docker-mailserver + Postgres with test accounts
source dev/dev.env
(cd web && pnpm install && pnpm build)
cargo run                       # http://localhost:8080 — alice@example.test / alicepass
```

`cd web && pnpm dev` gives hot reload on :5173 (proxies `/api` to the Rust server).

Tests, all of which run in CI against a real mail server:

```bash
cargo test                                   # unit tests
WEBMAIL_IT=1 cargo test --test integration   # API against the dev mail server
e2e/run.sh                                   # browser walkthroughs + accessibility audit
```

Architecture notes and the invariants contributors must keep are in
[CLAUDE.md](CLAUDE.md); what's planned is in [ROADMAP.md](ROADMAP.md).

## License

[GNU Affero General Public License v3.0](LICENSE). You may run, study, change and share
it; if you offer a modified version to users over a network, you must offer them its
source too (set `SOURCE_URL`).
