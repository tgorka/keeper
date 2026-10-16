# Observability — what keeper records, where, and what never leaves

One page for every question of the form "where did that error go?" (Epic 100,
ruling R381). The rule it describes:

> **An error reaches a person and the log, never only one of them.** Every
> failure a person can feel ends in a sentence where they are looking **and** a
> line in `keeper.log` at `WARN` or above. Telemetry is a separate, opt-in,
> category-only count; agents never reach it, and neither does a panic.

Destinations and consent are in [egress.md](egress.md#the-observability-consent-boundary);
this page is the inside view.

## The three places a failure can land

| Where | What lands there | Who sees it |
| --- | --- | --- |
| **The screen** | The sentence: an inline alert beside the control, the surface's status line, or a toast | The person, now |
| **`keeper.log`** | `WARN` and `ERROR` lines always; `INFO` only with debug mode on | The person, later; with an organisation account, also everyone with access to its config repository, which receives the newest 512 KB once a day and on *Send log now* (below) |
| **PostHog** (opt-in) | Closed categories and durations only — never a message, path, id or token | The maintainers, counted |

A failure that reaches only the log (nobody was told) or only the screen (no
trace to read back) is a defect, and the guards below fail on the common shapes.

## `keeper.log`

- **Where.** macOS: `~/Library/Logs/keeper/keeper.log` (it also shows in
  Console.app under Log Reports). iPhone: `<app container>/Library/Logs/keeper/keeper.log`.
  Settings › About names this device's own file under the Debug mode switch.
- **What.** Every `tracing` line from the Rust side, formatted once by
  `debug_log::init` (the shell's only subscriber). `WARN` and `ERROR` are written
  **always**: by the time anyone knows something went wrong the evidence has to
  be on disk already. `INFO` — the trail of what the person was doing — is
  written only while **Settings › About › Debug mode** is on; turning it on or
  off applies at once, without a restart.
- **Bounds.** The file rotates at 64 MiB and keeps two older generations
  (`keeper.log.1`, `keeper.log.2`), at most 192 MiB in all
  (`keeper_sync::logfile`). The gitoxide targets that once wrote millions of
  identical lines are held to `ERROR`; `rmcp` is off at every level, because it
  would log an MCP server's messages whole. `RUST_LOG`, when set, replaces the
  default filter (and still cannot turn `rmcp` on).
- **Reading it.** `tail -n 200 ~/Library/Logs/keeper/keeper.log`, or
  `grep -E ' (WARN|ERROR) '` for problems only. Lines are plain text, one event
  each: time, level, target, fields, message. Useful targets:
  - `keeper::panic` — a Rust panic: `location`, `thread`, `reason` (below).
  - `account: a step did not finish` — an organisation-account sign-in or sync
    step: `step` (`sign_in`, `forge_connect`, `credential`, `fetch`, `publish`,
    `settings`, …), `account` (the descriptor id) and `kind` (`closed`,
    `denied`, `timed_out`, `interrupted`, `refused`, `unreachable`, …). Never
    the identity provider's words, a token or a URL with a code.
  - `frontend error: the webview reported an error it could not handle` — the
    webview's funnel (below), with `source` naming where.
  - `a background failure; the person is told` — a failure where no command
    was waiting (voice, push), with the `surface`, the `operation`
    (`voice-rearm`, `voice-synced-settings`, `push-registration`), the
    `account` (push is per account; `-` for the operation as a whole) and the
    sentence shown; `a background failure recovered` when that operation
    itself next succeeds.
  - `keeper::recording` — a screen recording. `recording warning: …` (with its
    `code`: `micLost` a microphone removed, `micStalled` one that stopped
    sending sound, `micSilent` one recording digital silence) and
    `recording failed: …` for every warning and failure the capture sidecar
    reports, and `keeper-rec: …` for
    the sidecar's own lines: which microphone a session opened (`INFO`), and —
    at `WARN` — a microphone that was disconnected or stopped sending sound,
    which device the recording switched to, and a microphone delivering only
    digital silence. Device names and their unique ids appear here; paths and
    media never do. A `micLost`, `micStalled` or `micSilent` warning the session
    accepted is also counted in telemetry as its closed cause (below), once per
    cause per session.

### Your log in the config repository

With an organisation account, keeper also publishes the newest 512 KB of this
file (the newest rotated generation, then the live file, cut at a line) to
`<login>/logs/<device>.log` in the account's config repository, so a failure
on someone else's device can be read without asking them for the file:

- **When.** Once a day, as a step of the sync, while **Settings › About ›
  Send this device's log to your account daily** (`diagnostics.log_upload`) is
  on, which it is unless switched off; and at once on **Send log now** beside
  it, whatever the switch says. Only a device registered in the account
  uploads.
- **What changes on the way.** A two-line header names the device, the keeper
  release and its commit. The log is not secret-free by itself — a git
  diagnostic can quote a remote's URL, query string and all — so every line,
  the rotated generation's too, is scrubbed on the way out
  (`log_upload::scrub`): the person's home folder becomes `~`; URL user
  information (`https://***@host`), credential-named query and fragment
  parameters (`access_token`, `code`, `client_secret`, `X-Amz-Signature`, …;
  docs/account.md § *This device's log* has the exact rule) and the values of
  `Authorization`, `Bearer` and `Basic` become `***`. What stays: account and
  Matrix ids, device and microphone names, paths outside the home folder,
  error text. The file carries no upload time, so a log that gained no line
  commits nothing; a log with no line at all is not uploaded.
- **Who reads it.** Everyone with access to the repository.
- **When it fails.** The daily upload writes one `WARN` line —
  `account: this device's log could not be published`, `… could not be read`
  (a log file there that does not read: nothing is uploaded), or `… was not
  published: the repository records someone else in this directory` — and
  tries again at the next sync. It is never the account's problem, a refused
  token never disconnects the repository, and Settings › Account does not show
  it. *Send log now* answers under the button in Rust's words: where the log
  went, that nothing was new, that there is no log yet, that the device must
  be signed in first, that the directory is not this person's, that a
  sign-out stopped it, or why the repository did not take it.

The code is `keeper_core::org_account::log_upload` (what is sent) and the
shell's `account_ipc::publish_log` (the push); docs/account.md § *This
device's log* has the repository side.

### Rust panics

`debug_log::init` installs a panic hook once
(`keeper_sync::logfile::install_panic_hook`, chained before the previous hook,
so the default stderr report still happens). Every panic on any thread becomes
one `ERROR` line under `keeper::panic` with its `location`
(`file:line:column`), `thread` name and `reason` (the panic message, at most
500 characters). That line is all a panic becomes (R387): it is never counted
in telemetry, because a process whose runtime workers poll agent and non-agent
work alike has no boundary that could tell an agent's panic apart, and agents
stay out of telemetry fail-closed. The hook does nothing that could itself
panic beyond formatting the line — no lock it waits on, no random number, no
queue. Its proof runs in a re-executed copy of the test binary, because a panic
hook is process-global (`logfile::tests::a_panic_on_another_thread_…`).

The hook is installed by `setup()`. A process launched in the background that
never runs `setup()` (an iOS push wake, DW-1263) has no hook and no log file;
its panics reach stderr only.

## The webview's funnel

The React front decides nothing about a failure's meaning — Rust's `IpcError`
carries the sentence — but it must say it and log it:

- `sayFailure(source, fallback)` (`src/lib/failure.ts`): for a failure the
  person can feel — the room was not archived, the reaction did not land. A
  toast in Rust's words (or the fallback) **and** `reportFrontendError(source, …)`.
- `logFailure(source)`: for a failure nobody can feel — a cleanup whose surface
  is gone, a refresh whose last answer stands. The log line only.
- `reportFrontendError` (`src/lib/crash-report.ts`) sends a line to the
  `frontend_error_report` command, which writes it at `ERROR`. The line is
  `describeFailure`'s (`src/lib/failure-envelope.ts`): a JavaScript `Error` as
  its name and message; keeper's `IpcError` envelope — what every command
  rejects with, an object, not an `Error` — as `IpcError <code>` (and
  `(retriable)`), plus its message only for the codes whose message is keeper's
  own fixed sentence, never a homeserver's or identity provider's words; all
  bounded and stripped of control characters. The same `source` with the same
  classification (an envelope's code) is written once, and at most 100 lines per
  page load. Uncaught exceptions, unhandled rejections and the React crash
  boundary go the same way.
- **Guard:** `src/test/no-silent-catch.test.ts` reads every `src/` file outside
  `components/ui/` as a TypeScript syntax tree and fails on a promise
  `.catch(() => {})` (or one yielding only `undefined`/`null`) and on a `try`
  that awaits with an empty `catch`. It is a narrow structural guard, not the
  audit: catches that show an inline error or leave a state behind also report
  (`_bmad-output/implementation-artifacts/100-1-silent-catch-sweep.md` lists
  every site).

A failure Rust meets where no command is waiting — the wake phrase not
re-armed when keeper comes back to the front, a synced voice phrase refused, a
push registration the homeserver turned down — is logged at `WARN` and held as
Rust state (`keeper_core::failure::Failures`), keyed by the operation that
failed and, for push, by account, until that operation's own next success
clears it: a push registration that lands; the wake phrase actually armed again
(or set by the person, which is the remedy its sentence names); synced voice
settings applied. Another operation under the same surface never clears it — a
re-arm that had nothing to do (the phrase already listening) leaves a refused
synced stop phrase standing. The webview mirrors it through `failure_subscribe` — the standing set
first, then every change — so a failure from before the webview listened (an
Android push start at launch) or before a reload is still said. The app root
says each new one in a toast, and the surface's own switch shows the standing
sentence (Settings' voice and push sections) until Rust clears it.

## The organisation-account sign-in

The setup sheet and Settings › Account show Rust's sentence; when the account
VM's `problem` is true it is an alert. A browser sign-in ends one of five ways:

| How it ended | What the person reads | Log `kind` |
| --- | --- | --- |
| They closed the sign-in — the browser sheet, or Cancel sign-in in keeper | Quiet: the sheet says the sign-in was closed (and, for an account already signed in here, that it stays as it was — never "Up to date" or Done) | `closed`, once, for the step it interrupted |
| The identity provider refused (`access_denied`) | "*host* refused to sign you in to *account*: *the provider's reason*." | `denied` |
| No answer before the wait ran out | "Signing in to *account* timed out before the browser answered. Sign in again." | `timed_out` |
| The browser round trip broke | "Signing in to *account* did not come back from the browser (*why*). Sign in again." | `interrupted` |
| The sign-in server could not be reached | "keeper could not reach *account* to sign you in. …" | `unreachable` |
| Signed in, but connecting the repository (`oauth` mode) failed | "Connecting *account*'s repository failed (*why*). Sign in again." or "keeper could not reach *account*'s repository to connect it. …" — a problem, never the neutral "Reconnect the repository" | `internal` / `unreachable` at `forge_connect` |

The sheet reads the outcome of *this* attempt from the account VM's `attempt`
(`running`, `done`, `closed`, `failed`), which Rust sets when the attempt ends —
never from the account's state, which a re-setup of an account already signed
in here leaves at "Up to date". An account whose repository was connected
before and is simply offline keeps its quiet "Offline — using settings from …"
status.

## Telemetry (PostHog), opt-in

Off on every install until the person turns a category on in Settings ›
Diagnostics & studies; synchronized settings cannot turn it on. What can leave, by category, is
the table in [egress.md](egress.md#the-observability-consent-boundary). In
short, with **diagnostics** consent: app-ready and interaction durations, a
frontend-error category and their content-free OTLP records; with **product statistics**: opening the command
palette or Settings. `keeper_interaction` may carry one closed `surface`:
`slash_menu_opened`, `slash_menu_chosen`, `launch_default_agent` (Epic 100;
the front's `captureSurface(surface)` in `src/hooks/use-telemetry.ts`).

One event comes from the shell rather than the webview:
`keeper_recording_mic_lost`, under **diagnostics** consent, with one closed
`cause` — `removed` (`micLost`), `stalled` (`micStalled`) or `silent`
(`micSilent`) — and `event_source` `shell`. The recording sink raises it when
the session accepts that warning, at most once per cause per session, because
the sidecar repeats a loss (a failed fallback sends a second `micLost`, silence
and loss re-arm when sound returns). It has no duration and never the device
name or the warning's text (`keeper_core::telemetry::MicLossCause`,
`Telemetry::capture_mic_lost`). It is not part of the webview's
`telemetry_capture` contract.

**Never sent:** messages, error text, panics in any form, stack traces, file names, paths,
notes, recordings, bot prompts or answers, account or Matrix identifiers, URLs,
tokens, and nothing from an agent. The telemetry module installs no `tracing`
subscriber, so no log line can reach it.

## Agents

Agents (`keeper-agent`, `keeper-agentd`, `keeper_core::agents`) never reach
PostHog — not their spans, not their errors, not their panics
(S-19, NFR-121; `telemetry/tests.rs` `agent_host_records_are_never_exported`;
`bun run check:agentd-lean` forbids the exporter crates in the daemon). An
agent-host failure on the desktop is a `WARN`/`ERROR` line in `keeper.log` and
the agent room's own status line; the daemon on a server writes its own log
beside its sessions ([agents.md](agents.md)).
