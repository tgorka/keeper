# Keeper

An open-source, Beeper-style universal messenger client built on [Matrix](https://matrix.org).
macOS-first, with iOS/iPadOS, Windows, Android, and Linux planned (single Tauri codebase).

Keeper is a **client only** — it connects to Matrix homeservers (including Beeper's) and to
[mautrix bridges](https://github.com/mautrix) you host yourself or run locally, in the same way
the Beeper apps do. No server-side components live in this repository.

## Features (planned / in progress)

- Full messaging: text, media, files, emoji & reactions, voice/video (MatrixRTC, later)
- Local message archive: everything synced from bridges stored on-device (SQLite)
- Bridge management: connect to remote bridges, Beeper cloud bridges, and local/self-hosted
  bridges (bbctl-style)
- Multi-account: several Matrix accounts (e.g. beeper.com + self-hosted) at once
- Beeper-style UX: unified inbox, Spaces as network/room filters, favorites, command palette
- Drafts with explicit confirm-to-send, undo send (delayed dispatch window)
- Incognito mode (no read receipts / typing indicators), hotkeys, native notifications

See [docs/](docs/) and the BMAD planning artifacts in
[_bmad-output/planning-artifacts/](_bmad-output/planning-artifacts/) for the full product plan.

## Stack

| Layer     | Tech                                                               |
| --------- | ------------------------------------------------------------------ |
| Shell     | [Tauri 2](https://tauri.app) (Rust)                                |
| Matrix    | [matrix-rust-sdk](https://github.com/matrix-org/matrix-rust-sdk)   |
| UI        | React 19 + TypeScript + Vite + Tailwind CSS v4 + shadcn/ui         |
| Lint/fmt  | [Biome](https://biomejs.dev) (TS/JS/JSON), rustfmt + clippy (Rust) |
| Tests     | Vitest (+ Testing Library), cargo-nextest                          |
| Hooks     | [lefthook](https://lefthook.dev)                                   |

## Development

Prerequisites: [Rust](https://rustup.rs) (stable), [Bun](https://bun.sh), Xcode CLT.

```sh
bun install          # installs deps + git hooks (lefthook)
bun run tauri:dev    # run the desktop app in dev mode
```

Quality gates (also run in CI and via git hooks):

```sh
bun run check        # biome + tsc + vitest
bun run check:rust   # cargo fmt --check + clippy -D warnings
bun run test:rust    # cargo nextest
bun run check:all    # everything
```

- `pre-commit`: Biome (auto-fix staged), rustfmt, secret scan
- `pre-push`: typecheck, clippy, frontend tests

## Credentials

This is an open-source repo — **never commit credentials**. Development credentials
(test Matrix accounts, etc.) live in 1Password and are read via the `op` CLI at runtime.
See [docs/credentials.md](docs/credentials.md).

## Process

The project is planned and driven with [BMAD](https://github.com/bmad-code-org/BMAD-METHOD):
research → product brief → PRD → UX → architecture → epics/stories → dev loop.
Artifacts live in `_bmad-output/`.

## License

[Apache-2.0](LICENSE)
