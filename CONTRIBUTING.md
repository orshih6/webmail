# Contributing

Thanks for helping. The short version:

1. **Open an issue first** for anything bigger than a small fix, so we can agree on the
   approach before you spend time on it. [ROADMAP.md](ROADMAP.md) lists what is wanted.
2. **Run the dev stack and the tests** (see the README's Development section). CI runs the
   same things against a real mail server, so a green local run is a green CI run.
3. **Keep the invariants** in [CLAUDE.md](CLAUDE.md) — especially: message HTML is
   hostile, the From address is always the signed-in account, every per-account query is
   scoped by owner, and the CSP is never weakened.
4. **Update the end-to-end walkthrough** (`e2e/*.mjs`) when you change a user-visible flow.

## Style

- Rust: `cargo fmt`, `cargo clippy --all-targets -- -D warnings` clean.
- Web: `pnpm lint` (Biome) and `pnpm check` (svelte-check) clean. Svelte 5 runes, plain
  CSS with the existing design tokens, no UI kits — the whole UI ships in ~60 KB gzipped
  and should stay small.
- API shapes live in `src/types.rs`; `cargo test` regenerates the TypeScript types in
  `web/src/lib/api/types/`. Commit them; never edit them by hand.
- Comments explain *why*, not *what*.

## License

By contributing you agree that your contributions are licensed under the
[AGPL-3.0](LICENSE), like the rest of the project.
