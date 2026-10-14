# Epic 100 — Errors that reach someone, a model per agent, a default agent, avatars, a slash menu

created: '2026-10-14'
status: planned 2026-10-14 by the coordinator; built as five rungs on the stack `agents-100-*` off `main` 4915fd99 (release 0.10.3)
source: the owner's message of 2026-10-14 (verbatim below); facts from four read-only surveys of `main` 4915fd99, a live probe of CLIProxyAPI (below) and hesperia's `keeper.log`.
binds: rulings R381–R386 in `_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md`; deferred items DW-1386…DW-1439 (blocks per story below).

## The owner's ask

Verbatim (Polish, 2026-10-14):

> Miej pewnosc ze wszyskie rzeczy (bledy, statystyki, analityka, tracing i inne observability) sa na miejscu zarowno w nowych zmianach jak i w calej aplikacji (ostatnio nie widzialem bledow gdy chcialem ustawic konto marcie przy auth - miej pewnosc ze to tez jest naprawione).
> poza tym ustaw zebym mogl ustawiac rozne modele (boty) dla roznych agentow (np jeden z opus high, inny z opus xhigh, inny z haiku high a jeszcze inny z gtp astra) - teraz wydaje sie wszystkie moga miec ustawiony tylko ten sam.
> jak otwieram aplikacje to chce zeby byl otwarty domyslnie agents z otwartym domyslnym agentem (do ustawienia ktory ma byc domyslny - tylko jeden do wyboru)
> w agentach chce zeby byly mozliwe avatary (nie tylko litery) - avatar plik w folderze agenta (svg preferowany)
> w czacie daj rozwijane menu np po wpisaniu / zeby bylo wiadomo co mozna wybrac - bierz przydkald z notatek albo cursor, t3chat itp.

In English: (1) errors, statistics, analytics, tracing and the rest of observability in place in the new changes and the whole app, and the silent failure when setting up Marta's account at sign-in fixed; (2) a different model per agent, including a reasoning level (Opus high, Opus xhigh, Haiku high, GPT Astra); (3) at launch the Agents window opens with the default agent's conversation open, the default chosen in settings, exactly one; (4) an avatar per agent from a file in the agent's folder, SVG preferred; (5) in chat, a dropdown after typing `/` that shows what can be chosen, after the notes editor, Cursor, t3.chat.

## What the survey found

| Ask | Today on `main` 4915fd99 | Gap |
| --- | --- | --- |
| 1 errors | One front funnel (`src/lib/crash-report.ts` → `frontend_error_report` → `tracing::error!`), one React boundary (`app-crash-boundary.tsx`), `tracing` to `~/Library/Logs/keeper/keeper.log` (WARN+ always, INFO with debug mode, `crates/keeper/src/debug_log.rs`), opt-in PostHog (`keeper-core/src/telemetry`, five closed kinds). **No Rust panic hook.** | Marta: `org_account/oidc.rs` `authorize` maps a timeout, a failed round trip, a cancel **and** the IdP's `error=access_denied` to `AccountError::Cancelled`; `account_ipc.rs` `problem_of(Cancelled)` is `None`; converge's sign-in, forge and credential arms log nothing; the sheet then shows only "Sign in to {name} to use your settings on this device." hesperia's `keeper.log` (current and `.1`) holds **no** WARN/ERROR line about any account, OIDC or setup step — the failure left no trace anywhere. Swallowed `.ok()`s in `account_ipc.rs` (session identity at confirm, credential source, device slug, models.toml), `debug!`-only device-history reads, empty `catch` blocks on front `invoke`s (e.g. `chat-row.tsx`). Voice, push and the Android port warn into a log nobody reads and reach no sentence. |
| 2 models | `agent.toml` `[model]` = `bot` + `local_only` (`agents/home.rs`), per agent already — but *Set up agents* seeds one bot for all, keeper never edits an existing `agent.toml` (docs/agents.md "keeper never writes"), and **no reasoning parameter exists on the wire** (`bots/chat.rs` `build_body`). | A reasoning level per agent, on the wire, and a place to set bot + level per agent. |
| 3 launch | `primaryViewStore` defaults to `"inbox"`, not persisted; no default-agent notion anywhere. `proxy_rooms()` gives each agent's `main` DM. `bots.voice_target` (`agent:<room>`) is the precedent for a user-global key that names an agent room. | A key naming one agent and a launch rule. |
| 4 avatars | The mark is `SOUL.md` `icon` (≤ 4 chars) or the handle's first letter, drawn by `BotIdentityCell`; no image anywhere. `keeper-file://` already serves `.svg` as `image/svg+xml` with `nosniff`. | A file in the home, read on the desktop; the same picture on devices without the drive. |
| 5 slash menu | Notes (CodeMirror source) and the ⌘9 bot composer (Rust registry `bots::commands`, `bots_command_preview`, a plain `<ul>`) have one; the messenger `Composer` — used by chats **and** agent rooms — has none. `ui/command.tsx` (cmdk) is the shared surface. | A Rust-decided command registry for rooms and one menu component both composers use. |

**CLIProxyAPI probe (2026-10-14, `:8452/v1/chat/completions`).** `claude-opus-5-5` with `"reasoning_effort":"high"` → 200, answer, no reasoning text; with `"xhigh"` → 200 and `reasoning_content` present; without the field → 200. The OpenAI-compatible field is accepted and changes behaviour, so a level is a wire field, not a model name.

## Stories (rungs, bottom first)

### 100.1 — Every error reaches a person and the log; Marta's sign-in says why (rung `agents-100-errors`, DW-1386…DW-1399)

1. A Rust panic anywhere in the desktop or phone process is written to `keeper.log` at ERROR with its location and thread (a hook installed by `debug_log::init`, chaining the previous hook) and, with telemetry consent on, counted as a category-only diagnostic event (no message, no path). Proved by a test that panics in a spawned thread and reads the line back.
2. Org-account sign-in tells apart: the person closed the sign-in, the identity provider refused (`access_denied`, with the provider's `error_description` when it has one), the sign-in timed out, the browser round trip failed. Each but the first is an `AccountProblem` with its own sentence naming the account; "closed" stays quiet. Proved by `authorize` tests per outcome against a stub callback.
3. Every `converge` arm that ends in a problem or a cancel writes one `tracing::warn!` with the step, the account id and the error kind (never a token or a URL with a code). Proved by a test that captures the warning for a refused sign-in.
4. The setup sheet and Settings › Account render the problem sentence as an alert; a signed-out sentence is never the only words after a failed step (`account-setup-sheet.tsx`). Proved in vitest and in the browser through the mock shell.
5. The swallowed reads named in the survey either propagate as a problem or are logged at WARN with what was lost; `debug!`-only failures a person can feel move to WARN. Each changed site listed in the As built section.
6. Front: every `invoke` whose failure a person would notice ends in a visible sentence (toast or inline) **and** `reportFrontendError`; empty `catch` blocks that hide such failures are gone. A vitest sweep that fails on an empty `catch` around an IPC call in `src/` (outside `components/ui`) stands as the guard.
7. Voice, push (iOS/Android) and bot-turn failures that reach no sentence today reach one where a person is looking (the voice switch's line, the push setting's line, the bot pane), and the log at WARN.
8. Agents stay out of PostHog (S-19/NFR-121 stand, the `telemetry/tests.rs` exclusion test unchanged); agent-host failures on the desktop reach `keeper.log` at WARN and the agent's room status line, never telemetry.
9. Analytics for the new surfaces of epic 100: `keeper_interaction` gains closed, content-free surface values for the slash menu (opened, chosen) and the launch into the default agent; nothing else new is sent. `deploy/posthog/manifest.json` and `docs/egress.md` say so.
10. `docs/observability.md` (new): what is logged where, what reaches telemetry and what never does, how to read `keeper.log`, how to turn on debug mode; linked from `docs/egress.md`.

### 100.2 — A model and a reasoning level per agent (rung `agents-100-model`, DW-1400…DW-1409)

1. `agent.toml` `[model]` accepts `effort = "low" | "medium" | "high" | "xhigh"` (optional; absent = the provider's default). An unknown value refuses the home with a sentence naming the file and the four values.
2. On the wire: `openai`-kind providers get `reasoning_effort` verbatim; `ollama` gets `think` with `low|medium|high` and refuses `xhigh` with a sentence at load; `hermes` refuses any `effort` with a sentence. Absent `effort` sends nothing new (byte-identical body). Proved by `build_body` tests per kind and a live run against CLIProxyAPI (`#[ignore]`) showing `reasoning_content` for `xhigh`.
3. The session log line of every turn names the bot and the level actually sent.
4. Settings › Agents on the desktop lists every agent whose home is on this device with its bot (from the person's bots) and its level; changing either writes **only** the `[model]` table's `bot` and `effort` keys of that agent's `agent.toml` (every other byte kept — comments, order, other tables), through the drive's existing write path, as the person. This reverses the "keeper never writes an existing `agent.toml`" rule for those two keys only (R382).
5. The next turn of that agent uses the new model and level, without restarting keeper or the host.
6. *Set up agents* lets each seeded agent take its own bot and level (default: the folder's choice).
7. The agent room's header shows the agent's model and level where the home is on the device.
8. Errors per 100.1: a write that fails (drive read-only, merge conflict, file changed underneath) says so beside the control and logs WARN.

### 100.3 — Agents open at launch, on the default agent (rung `agents-100-launch`, DW-1410…DW-1419)

1. A user-global key `agents.default` holds one agent's Matrix user id (`@nixi:…`), settable in Settings › Agents (one choice, or none) and from the agent room header's menu ("Make default"). Regenerated `docs/settings-keys.md`.
2. At launch, when the signed-in account has at least one agent room, keeper opens the Agents window; when `agents.default` names an agent with a `main` room on this account, that room is open and focused. When the agent has no room here, the Agents window opens with nothing selected and the settings row says why. With no agent rooms, launch is unchanged (Inbox).
3. Phone: the same rule when the restored navigation has nothing to restore.
4. Rust decides which room (`proxy_rooms()` `kind: main` for that agent); the front only applies the answer once accounts are hydrated and agent rooms have arrived. Proved by Rust tests of the choice and a vitest of the boot effect; browser proof at 1280 px.

### 100.4 — An agent's picture from its folder (rung `agents-100-avatar`, DW-1420…DW-1429)

1. An agent home may hold `avatar.svg` (preferred), else `avatar.png`, `avatar.webp` or `avatar.jpg`, at most 256 KiB; anything else is ignored with a WARN naming the file.
2. Where the agent's zone is on the device, keeper draws that file wherever the agent's mark is drawn today (room header, Agents window rows, notes dock, sessions board, approval card, delegate lines — every call site of the agent mark, listed in As built), clipped to the identity cell; the letter/icon stays the fallback.
3. The host that serves the agent sets the agent's Matrix profile avatar from the same file when its content hash changes, so devices without the drive (the phone, other Macs, other clients) show it too; keeper draws a Matrix profile avatar of an agent the same way.
4. SVG is only ever an `<img>` source served with `image/svg+xml` and `nosniff`, never inlined into the DOM, never thumbnailed by the server (keeper fetches the original).
5. Errors per 100.1: an unreadable or oversized file and a failed upload are WARN lines and a sentence in the agent's settings row.
6. `docs/agents.md` documents the file; seeded agents keep their letter until a file is added.

### 100.5 — A slash menu in every chat (rung `agents-100-slash`, DW-1430…DW-1439)

1. Typing `/` at the start of the messenger composer (chats and agent rooms) opens a dropdown above the composer listing the commands that apply in that room, each with its name, aliases, a one-line description and its argument hint; it filters as the person types, ↑/↓ move, Enter/Tab complete, a second Enter runs, Escape dismisses until the token changes, `//` sends a literal slash.
2. The registry and the decision of what a line means live in Rust (`keeper_core::chat::commands`, AD-55 like `bots::commands`): a chat set (at least `/me`, `/shrug`, `/plain`, `/topic`, `/invite`, `/leave` with a confirmation) and, in an agent room, the actions keeper already offers there (e.g. new conversation, stop/pause, the agent's model per 100.2, help) — no new agent protocol.
3. The ⌘9 bot composer renders its menu with the same component (cutover; its Rust registry stays); the notes editor is untouched.
4. Unknown command: the line is refused in place with a sentence, never sent as text.
5. Browser proof at 1280 px and 420 px; keyboard proof in vitest; analytics per 100.1 #9.

## Order and dependencies

100.1 is the bottom rung and the rule every other rung follows (its sentence-and-log pattern). 100.2–100.5 are independent of each other and are built in parallel on `agents-100-plan`, then stacked 100.1 → 100.2 → 100.3 → 100.4 → 100.5; ledger conflicts are unions, lower rung first.

## Owed on devices (never claimed by a Linux rung)

hesperia: Marta's org-account setup end to end against ZITADEL; a panic line in the real log; the agents settings write on a real drive; launch into the default agent; avatar in the real app. kalypso: launch rule on the phone; profile avatar on the phone.
