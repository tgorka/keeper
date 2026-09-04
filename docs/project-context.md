---
project_name: "keeper"
user_name: "Dev"
date: "2026-07-03"
sections_completed:
  [
    "technology_stack",
    "language_rules",
    "framework_rules",
    "testing_rules",
    "quality_rules",
    "workflow_rules",
    "anti_patterns",
  ]
status: "complete"
rule_count: 57
optimized_for_llm: true
---

# Project Context for AI Agents

_This file contains critical rules and patterns that AI agents must follow when implementing code in this project. Focus on unobvious details that agents might otherwise miss._

Keeper is an open-source, Beeper-style Matrix messenger client (Apache-2.0). It is a **client only** — no server-side components. macOS-first Tauri desktop app; iOS/Android/Windows/Linux later from the same codebase.

---

## Technology Stack & Versions

| Layer          | Tech                                                                                       |
| -------------- | ------------------------------------------------------------------------------------------ |
| Shell          | Tauri 2 (tauri 2.x, Rust stable — see `rust-toolchain.toml`), backend in `src-tauri/`       |
| Matrix core    | `matrix-sdk` 0.18 (features: `e2e-encryption`, `sqlite`, `sso-login`) + `matrix-sdk-ui` 0.18 |
| Rust runtime   | tokio 1 (`macros`, `rt-multi-thread`, `sync`, `time`), tracing, thiserror 2, serde/serde_json |
| Frontend       | React 19 + TypeScript ~5.8 + Vite 7, in `src/`                                             |
| Styling/UI     | Tailwind CSS v4 (via `@tailwindcss/vite`, no tailwind.config) + shadcn/ui (radix-ui, lucide-react, cva) |
| Package manager| **bun** (bun.lock; never npm/yarn/pnpm)                                                     |
| Lint/format    | Biome 2 (TS/JS/JSON/CSS), rustfmt + clippy (Rust)                                          |
| Tests          | Vitest 4 + Testing Library (jsdom), cargo-nextest (Rust)                                    |
| Hooks/CI       | lefthook (installed by `bun install`), GitHub Actions on macos-latest                       |

## Architecture Invariant (most important rule)

- **All Matrix logic lives in Rust.** State, sync, crypto (E2EE), and persistence (SQLite) are owned by the Tauri backend via matrix-sdk / matrix-sdk-ui.
- The React frontend is a **pure renderer of view models** received over Tauri IPC:
  - **Commands** (`#[tauri::command]` + `invoke`) for one-shot actions.
  - **Channels** (`tauri::ipc::Channel<T>`) for streaming updates (room list / timeline / sync status diffs).
- **Never** put Matrix protocol logic, crypto, or message storage in TypeScript. Never add `matrix-js-sdk` (or any Matrix JS lib) to the frontend. One source of truth: Rust.
- Keep the full message DB and state in Rust; the webview receives only view models for visible ranges.

## Critical Implementation Rules

### Rust Rules (src-tauri/)

- `unsafe_code = "deny"` (workspace lint). In `keeper-core` and all business logic: no
  `unsafe`, ever. In the `keeper` shell crate ONLY, a narrowly-scoped, function-level
  `#[allow(unsafe_code)]` is permitted for platform FFI that has no safe binding (e.g.
  iOS `NSURLIsExcludedFromBackupKey` via objc2), under these conditions: one function per
  concern, behind the `Platform` port, with a `// SAFETY:` comment citing the API contract,
  and listed in the audit inventory in docs/constraints-and-limitations.md. (Coordinator
  policy amendment, 2026-07-11, story 14.7.)
- `clippy::unwrap_used = "warn"` and clippy runs with `-D warnings`: **never use `.unwrap()` (or bare `.expect()`) in production paths.** Use `?` with `thiserror` error types; `expect` is tolerated only in tests and startup code that cannot proceed (e.g. `tauri::Builder::run`).
- `clippy --all-targets -- -D warnings` must pass — treat every clippy lint as an error.
- Use `tracing` for logging (not `println!`/`eprintln!`).
- Library crate is `keeper_lib` (see `[lib]` in Cargo.toml); app entry logic goes in `src-tauri/src/lib.rs`, not `main.rs`.
- New dependencies must pass the **cargo-deny license firewall** (`cargo deny check` from `src-tauri/`): permissive licenses only (Apache-2.0/MIT/BSD/ISC/Zlib/MPL-2.0…). **AGPL/GPL code must never be linked** — study AGPL projects (Element X, gomuks) for patterns only, never copy code.
- Bots (Epic 61): every decision — URL grammar, wire, discovery, grants, tool policy, caps — lives in `keeper-core::bots`; the `keeper` shell is a call site and decides nothing (AD-55/AD-56).
- A bots tool call contains a path with `keeper-sync`'s existing `browse::resolve` / `plain_segments` / `WriteScope::route`, carried verbatim across the verb boundary. **Never** new path arithmetic (AD-65, AD-159).
- A model capability keeper could not read is `None` (`BotModelVm.vision/tools/reasoning: Option<bool>`), never `Some(false)` — an unknown capability offers the affordance with a warning; a `false` one hides it (AD-151).
- File content reaching a model — tool results and `AGENTS.md`-style context files alike — is **data**, entered under the not-instructions sentence; it never becomes a directive and never widens a grant (AD-159, NFR-48).
- A write is never auto-approved by a grant alone: `bots::grant::decide` runs at every tool call, the first write and every write outside an approved subtree asks, and the audit row precedes the effect (AD-158, NFR-47).
- Voice (Epic 62, Epic 63): every decision — the turn state machine, the phrase grammar, when to ask for a permission, what armed listening costs, whether the port may record while it speaks (`may_record`), which platform noun and remedy a refusal names (`VoicePlatform`) — lives in `keeper_core::voice`; `voice_ios.rs` and `voice_macos.rs` in the shell are port implementations and decide nothing (AD-165, AD-175; D-5). `keeper-core` gains no platform `cfg` for it (AD-55).
- Speech recognition is on-device only: every `SFSpeech…RecognitionRequest` sets `requiresOnDeviceRecognition(true)` after `supportsOnDeviceRecognition` is checked, and a server fallback is a **refusal** (`VoiceUnavailable::NoOnDeviceModel` names the language to download; `NoOnDeviceRecognition` says the OS has no on-device asset for it, that the download *may* add one, and names the languages that run here), never a TODO — `voice_on_device` in `keeper-core/tests` scans `keeper-core/src/voice/**` and every `voice*.rs` in the shell crate (picked up by prefix, so a new port is scanned without editing the test) and fails on a `false` flag, a request without one, or any network token (NFR-50, AD-166). The language itself is `keeper_core::voice::locale::choose`'s answer over `bots.voice_locale`, the system locale and the port's cached enumeration of `supportedLocales()`: an explicit choice that cannot run is refused, never silently replaced (Epic 63).
- Every voice surface exists on `voice_availability`'s answer alone — `unsupported` renders it away, any other refusal renders it with its sentence, unanswered renders nothing — and `CapabilitiesVm` has no voice field and gains none (AD-179; D-8). Never gate a voice control on a capability flag.
- The wake phrase is armed by a person while keeper is in front and never by keeper itself; iOS refuses to start the microphone from the background, so no code may pretend to (AD-168; D-5). What stops listening and what it costs is `LISTENING_LIMITS`, one `const`, rendered beside the switch and nowhere else (AD-169).
- `CapabilitiesVm.bots` is *this build can talk to a model* (every tier); `botTools` is *this build can reach the drive* (desktop **and** `sync`). Grant and tool affordances read `botTools` and are absent, not disabled, where it is false — `bots_ipc` is sync-free, `bots_drive_ipc` is `#[cfg(desktop)]` (AD-161, AD-162).

### The shell crate cannot be compiled on the Linux dev host

- `cargo build -p keeper` (and `bun run check:rust`, which runs clippy over the workspace) **fails on this Linux container before any keeper code is reached**: there is no `pkg-config` and no glib development headers, so `gio-sys`/`glib-sys` fail in their build scripts (`scripts/install-macos.sh:10-16`, `scripts/check-macos.sh:5-10`). Do not read a clean `cargo nextest run -p keeper-core -p keeper-sync` as a clean workspace.
- **The whole local gate is:** `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync` (and `cargo clippy` over those two crates), plus the frontend (`bun run check`). **Everything under `crates/keeper/**` — every `#[tauri::command]`, every port implementation, every `cfg(target_os = "macos")` block — is gated only by CI's `Rust (fmt, clippy, test)` job on `macos-latest` (`.github/workflows/ci.yml:26-28`) or by `bun run check:rust:macos` against a Mac (`scripts/check-macos.sh`).** A story that edits the shell crate and reports "tests green" from Linux has reported the core crates' tests, not its own change.
- Therefore, when a story changes a `keeper-core` signature that the shell crate calls — a removed `From` impl, a renamed variant, a new required trait method — **grep `crates/keeper/src` for every caller and update it in the same change**, and say in the report that the shell crate was updated by inspection, not by compilation. Epic 63 shipped a real compile break this way: story 63.3 removed a `From` impl and `voice_authorize` in `crates/keeper/src/voice_ipc.rs` kept calling it; the local gate stayed green and a later story found it by reading, not by building. The macOS voice port (`crates/keeper/src/voice_macos.rs`) was written on this host and has never been compiled anywhere — DW-228 names the commands that close that.
- Before claiming a shell-crate change is verified, either run `bun run check:rust:macos` (needs a reachable Mac with Xcode, Rust and rsync) or say plainly that the change awaits CI's macOS job. Never describe a Linux build of the shell crate; there is none.

### TypeScript / React Rules (src/)

- TypeScript `strict` mode; `noUnusedLocals`/`noUnusedParameters` are errors.
- Biome enforces: `noExplicitAny` (error — no `any`), `useImportType` (use `import type` for type-only imports), `useConst`, no unused imports/variables.
- Formatting (Biome, not Prettier): 2-space indent, 100-char lines, double quotes, semicolons, trailing commas.
- Path alias `@/*` → `./src/*` — use `@/lib/utils`, `@/components/ui/...` instead of relative walks.
- React 19 function components only; shared hooks in `src/hooks/` (kebab-case filenames like `use-mobile.ts`), utilities in `src/lib/`.
- `src/components/ui/` is **shadcn-generated code**: add components via the shadcn CLI, don't hand-write them there; it has relaxed lint rules (biome overrides) and is excluded from test coverage. Do not import app business logic into it. `src/index.css` is excluded from Biome.
- Use `cn()` from `@/lib/utils` for conditional class names; Tailwind v4 CSS-variable theming (configured in `src/index.css`, no `tailwind.config.*`).

### Testing Rules

- Frontend: Vitest with globals + jsdom; tests **colocated** as `*.test.ts(x)` next to the source (e.g. `src/App.test.tsx`); setup in `src/test/setup.ts` (jest-dom). Use Testing Library queries, not DOM poking.
- Rust: unit tests in `#[cfg(test)]` modules; integration tests in `src-tauri/tests/`. Runner is **cargo-nextest** (`bun run test:rust`), not plain `cargo test`.
- Coverage excludes `src/components/ui/**` and `src/test/**` — don't write tests for generated shadcn components.

### Quality Gates (must pass before done)

- `bun run check` — biome lint + tsc typecheck + vitest.
- `bun run check:rust` — `cargo fmt --check` + `clippy --all-targets -- -D warnings`.
- `bun run test:rust` — cargo-nextest.
- `bun run check:all` — everything. Run the relevant gate after any change; CI runs all of them plus a `tauri build --no-bundle`.
- lefthook hooks enforce these locally: pre-commit (biome auto-fix on staged files, rustfmt check, secret scan) and pre-push (tsc, clippy, frontend tests). Never bypass with `--no-verify`.

### Development Workflow Rules

- **English everywhere**: code, comments, docs, commit messages, UI strings.
- Commit subjects: conventional-ish imperative mood ("add room list stream", "fix timeline pagination"), lowercase, no trailing period.
- **Never commit secrets.** Dev credentials live in 1Password, referenced via `op://` URIs in `.env.1p`; run with `op run --env-file=.env.1p -- <command>`. See `docs/credentials.md`. The pre-commit hook scans for private keys and Matrix access tokens (`syt_...`) — real values must never appear in the repo.
- Package management with bun only: `bun install`, `bun add`, `bun run <script>`.
- BMAD artifacts: planning documents go in `_bmad-output/planning-artifacts/`; implementation artifacts (stories, `sprint-status.yaml`) in `_bmad-output/implementation-artifacts/`; durable project docs in `docs/`.

### Critical Don't-Miss Rules (anti-patterns)

- ❌ No Matrix/crypto/persistence logic in TypeScript — Rust owns it all.
- ❌ No `.unwrap()` in Rust production code paths.
- ❌ No `any` in TypeScript; no plain `import` where `import type` fits.
- ❌ No AGPL/GPL dependencies (Rust or JS) — cargo-deny will fail the build; the same policy applies to npm deps.
- ❌ No hand-edits that fight Biome/rustfmt formatting — run the formatters instead.
- ❌ No secrets, tokens, or homeserver credentials in code, tests, fixtures, or docs.
- ❌ Don't shuttle large payloads (media, full timelines) through IPC as JSON/base64 — stream view models for visible ranges; media should use a custom URI scheme handler from the Rust cache.
- ❌ Don't hold message/room state in a JS store as the source of truth — frontend stores mirror Rust view-model streams only.
- ❌ Nothing from the bots surface lives in a recording path — no file under `src/components/recording/`, no `recording-*` lib/store, no `use-record*` hook, and no palette verb the zero-egress scan would read as an upload or a transcription (`zero-egress.test.ts`, `src/test/bots-surface-stays-out-of-recording.test.ts`).

---

## Usage Guidelines

**For AI Agents:**

- Read this file before implementing any code.
- Follow ALL rules exactly as documented.
- When in doubt, prefer the more restrictive option.
- Update this file if new patterns emerge.

**For Humans:**

- Keep this file lean and focused on agent needs.
- Update when the technology stack changes.
- Review periodically for outdated rules and remove rules that become obvious.

Last Updated: 2026-09-03

## Git workflow (automation sessions)

- Commit on the branch that is checked out when the session starts. Do NOT create new
  branches, switch branches, push, or rewrite history — the bmad-loop orchestrator and the
  human coordinator own branch topology and pushing.
