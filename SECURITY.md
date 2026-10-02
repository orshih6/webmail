# Security

A webmail client handles hostile input (email) and holds the keys to people's mailboxes,
so security reports are very welcome.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through GitHub:
**Security → Report a vulnerability** on this repository
(<https://github.com/orshih6/webmail/security/advisories/new>).

Include what you found, how to reproduce it, and what an attacker could do with it. You
will get an acknowledgement within a few days; fixes are released as soon as they are
ready, and you will be credited in the advisory unless you prefer otherwise.

## Scope

In scope: anything that lets message content run script or reach the app, session or
credential exposure, CSRF, access to another user's mail, settings or uploads, and
bypasses of the remote-content blocking.

Out of scope: the security of the IMAP/SMTP server you connect it to, and deployments
that disable `COOKIE_SECURE` or enable `TLS_ACCEPT_INVALID_CERTS` (both are for local
development only).

The design is described under "Security model" in the [README](README.md).
