# Roadmap

The core client is complete and covered by end-to-end tests. Roughly in order of value,
this is what a general-purpose webmail client still needs. Contributions welcome — open an
issue first for anything large.

## Next

- **Translations.** The UI is English-only. Extract strings and add a locale switch
  (browser default, overridable in Settings).
- **Filters and vacation replies** via ManageSieve (RFC 5804): a simple rule editor
  (from / to / subject → move, mark, forward) and an out-of-office reply.
- **Quota** display from `GETQUOTAROOT` ("2.1 GB of 5 GB used").
- **Desktop notifications** for new mail (the live-update stream already exists).
- **"Not junk"** and spam-training hooks; show calendar invitations (`text/calendar`)
  readably.

## Later

- Import/export: contacts as vCard, messages as `.eml` / mbox.
- Previews for PDFs and other common attachment types.
- Faster first-time search on very large mailboxes. Uncached body search costs ~400 ms on
  20,000 messages because the server scans bodies; enabling a full-text index on the
  server (e.g. Dovecot FTS with flatcurve) fixes it without changes here.
- Download all attachments as a zip; forward as attachment.
- Aliases as identities, once the server can say which addresses an account may send as.

## Done

- Core mail: folders, threaded list, reader, compose, attachments, drafts, search,
  flags, move/junk/delete, live updates.
- Identities and signatures, address book with autocomplete, preferences (theme, page
  size, conversations, remote images).
- Folder management, view source / `.eml` / print, image previews, all-folder search,
  conversation view, rich-text compose with inline images.
- Undo for delete/move, resilience to mail-server outages, session-expiry recovery,
  large-mailbox performance, accessibility (WCAG 2.1 AA).
