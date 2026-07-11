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
rule_count: 42
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

Last Updated: 2026-07-03

## Git workflow (automation sessions)

- Commit on the branch that is checked out when the session starts. Do NOT create new
  branches, switch branches, push, or rewrite history — the bmad-loop orchestrator and the
  human coordinator own branch topology and pushing.
