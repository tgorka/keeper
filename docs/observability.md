# Observability — what keeper records, where, and what never leaves

One page for every question of the form "where did that error go?" (Epic 100,
ruling R381, amended by epic 102's R461/R462). The rule it describes:

> **An error reaches a person and the log, never only one of them.** Every
> failure a person can feel ends in a sentence where they are looking **and** a
> line in `keeper.log` at `WARN` or above. With diagnostics consent — one
> switch for the whole account — every such line, panics included, also
> reaches PostHog with its message redacted; an agent's line leaves without
> its message.

Destinations and consent are in [egress.md](egress.md#the-observability-consent-boundary);
this page is the inside view.

## The three places a failure can land

| Where | What lands there | Who sees it |
| --- | --- | --- |
| **The screen** | The sentence: an inline alert beside the control, the surface's status line, or a toast | The person, now |
| **`keeper.log`** | `WARN` and `ERROR` lines always; `INFO` only with debug mode on | The person, later; with an organisation account, also everyone with access to its config repository, which receives the newest 512 KB once a day and on *Send log now* (below) |
| **PostHog** (opt-in, per account) | Every `WARN`/`ERROR` line and every webview error as a `$exception`: level, target, file:line, fingerprint and the **redacted** message (an agent line: no message); plus closed categories and durations | The maintainers, as issues grouped by fingerprint |

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
500 characters; a longer one loses the token the cut went through, so the
redactor never sees a fragment of a secret too short to recognise — R511).
Under diagnostics consent that line leaves for PostHog like
every other `ERROR` (R461, replacing R387's "never counted"): redacted, with
the `keeper::panic` target. The hook does nothing that could itself panic
beyond formatting the line — no lock it waits on, no random number, no queue;
the export layer behind it only `try_lock`s. Its proof runs in a re-executed
copy of the test binary, because a panic hook is process-global
(`logfile::tests::a_panic_on_another_thread_…`).

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
  `frontend_error_report` command, which writes it at `ERROR` under the target
  `keeper::frontend` and, under diagnostics consent, sends it to PostHog as
  `keeper_frontend_error` with a `$exception` whose type is the `source` and
  whose value is the message, redacted (102.1 #3). The line is
  `describeFailure`'s (`src/lib/failure-envelope.ts`): a JavaScript `Error` as
  its name and message; keeper's `IpcError` envelope — what every command
  rejects with, an object, not an `Error` — as `IpcError <code>` (and
  `(retriable)`), plus its message only for the codes whose message is keeper's
  own fixed sentence, never a homeserver's or identity provider's words; all
  bounded and stripped of control characters. The same `source` with the same
  classification (an envelope's code) is written once, and at most 100 lines per
  page load. Uncaught exceptions, unhandled rejections, the React crash
  boundary and `onUncaughtError` go the same way.
- `console.error` and `console.warn` are hooked (`hookConsole`, installed with
  the global listeners): after the console prints, each distinct line is sent
  once per load (50 at most) under `source` `console.error` / `console.warn`,
  the latter written at `WARN`. A line the funnel writes about its own refusal
  goes to the unhooked console, so it cannot loop.
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

Off until the person turns a category on in Settings › Diagnostics & studies.
**Diagnostics** and **product statistics** are the account's settings
(`telemetry.diagnostics`, `telemetry.product_analytics`; R462): settings sync
carries them in `<login>/settings.toml`, so on (or off) on one device is on (or
off) on every device of the account. What is in force is the **effective**
answer — a settings file (`~/.keeper/keeper.toml`, the account's own
`keeper.toml`) pinning a key wins over the row — re-read at boot, after every
synced apply of either key, and after every account-layer install or clear
(a pull that changed the account's files, sign-out, forget, a refused
sign-in; `config::set_account_layers_observer`) (R496). A change discards
what was queued and cancels what is in flight. A local save takes the answer
read back after its write; a pinned key keeps the file's value and Settings
says the save was overruled (`TelemetryStatusVm.consentPinned`). Every
transition reads and installs under one lock (`Telemetry::settling`), never
held across a write, so the last install answers the last write: a
revocation that finishes is in force, whichever save began first (R511). An
installation that had switched a category on before epic 102
(`telemetry-consent-v1.json`) keeps that "on" pending — never in force by
itself — until a settings sync establishes the key (`Telemetry::account_settled`
with `AccountConsentKnown`): adopted where the reconciled file has no value
and nothing about the key failed (or there is no account at all), dropped
where the account has a value, and kept pending where the key's apply failed
or was overtaken (R495 07, R511). Adoption writes its row only if none is
there, in one statement (`registry::insert_setting_if_absent`, R515), so a
choice saved after adoption decided — an "off" included — is never
overwritten, and the settings sync hears of the adopted row only once it is
written. Remote
configuration and the random installation id stay per installation.
Fail-closed: anything but `"1"` is off. What can leave, by category, is the
table in [egress.md](egress.md#the-observability-consent-boundary).

### Problems: every warning and error (R461)

The shell's one subscriber (`debug_log::init`, built by
`keeper_core::telemetry::log_subscriber`) carries
`keeper_core::telemetry::LogExport` beside the `keeper.log` writer, each under
its own filter: `keeper.log` keeps its `EnvFilter` (and `RUST_LOG`), while the
export layer forwards every `WARN` and `ERROR` on every target whatever those
say (`export_filter`; R495 08) — `keeper::panic` and the gitoxide targets
`keeper.log` holds down included. Each becomes one `$exception` event:

| Property | What it holds |
| --- | --- |
| `$exception_list[0].type` | The target (`keeper::sync`, `matrix_sdk::…`) |
| `$exception_list[0].value` | The message and its fields as `keeper.log` prints them, each part redacted from its raw value, then capped |
| `$exception_fingerprint` | FNV-1a of target + file + line: one callsite is one issue on every build and device |
| `$exception_level` | `warning` or `error` (`log_level`: `WARN` / `ERROR`) |
| `log_target`, `log_file`, `log_line` | The callsite; a workspace file as compiled, a dependency's file from its crate directory, never the builder's home |
| `os`, `app_version` | Which platform and release |

**The redactor** (`keeper_core::telemetry::redact`, R461 as fixed by R495)
fails closed: where it cannot tell where a secret ends, everything after its
start goes. It runs on each field's RAW value before anything quotes or
escapes it (`redact_field`), on the message, and only then is the line joined
and cut at 1,000 characters. Text cut before it gets here — a webview report
past 8,000 characters, a panic message past 500 — loses the token the cut
went through. It
replaces: `syt_`/`phc_`/`phx_`/`phs_` tokens and the word after `Bearer`
(`<token>`); a credential assignment — a name ending in `password`, `passwd`,
`passphrase`, `secret`, `token`, `key`, `authorization`, `cookie` or
`credential(s)`, then `=` or `:`, in any quoting or escaping — from the
assignment to the end of the text, and a field so named or holding one whole
(`<redacted>`); a URL's user information and its path, query and fragment
(scheme and host stay); email addresses; Matrix user, room, alias and event
ids with a server, and room and event ids without one (`$…`, `!…`;
`<matrix-id>`); home paths — `~/`, `/Users/…`, `/home/…`, `/root/…`, the
iPhone and Android containers, `C:\Users\…` with any escaping, `C:/Users/…`,
with `Users` in any letter case (`/users/…` is `/Users/…` on a case-insensitive
Mac volume), and the same roots inside a `file://` URL (`file:///Users/…`,
`file:///C:/Users/…`; R518) — **and everything after them**: from the home
root to the end of its field value, or of the message, in every quoting. Text
after a home path is not sent — a closing quote, the rest of the sentence, a
second path, a quoted value that follows. A filename may hold quotes, spaces
and backslashes, so no character after the root says for certain where the
path ends (R515) (`<path>`); runs of 16+ hex digits (`<hex>`); every run of
24+ base64 characters, words and identifiers included (`<base64>`). What
survives: the sentence around them, numbers, short ids, and — DW-1512, the
owner's open question — relative paths (a drive's `40-media/…`), note file
names and device names. It is a lexical redactor, not a content classifier.

**Agent lines** (targets `keeper_agent…` and `keeper_core::agents…`) leave with
level, target, file:line and fingerprint and the fixed value "An agent-host
line; its message stays on this device." — their fields are never even
formatted, and `Telemetry::capture_log` withholds an agent target's message
whatever it is handed (S-19's intent kept for content;
`agent_target_lines_leave_without_their_message`).

**Webview errors** cross to Rust uncut up to 8,000 characters
(`FAILURE_TRANSPORT_MAX` / `FRONTEND_INPUT_CAP`); a longer one is cut, and the
cut drops the token it went through, so no fragment of a secret is left too
short to recognise (`redact::bound`). Rust redacts, then caps. `keeper.log`'s
own line keeps its shorter caps.

**Limits.** At most 10 per fingerprint per minute and 500 per run of keeper;
the queue holds 128. A log call never waits: consent is an atomic read and the
queue a `try_lock`; a refused problem is counted, and the count leaves at most
once a minute as one `keeper_problems_dropped` event (`dropped`). Records older
than a minute are not sent; there is no disk spool and no retry. A noisy target
is bounded by its fingerprint's budget, not excluded.

**What is never forwarded** (R498, R511): what the exporter itself produces —
this module's lines (`keeper_core::telemetry`), and whatever its HTTP stack
logs on the exporter's own threads: every export and remote-config request
runs on a one-worker runtime (`layer::Transport`) whose threads are marked,
so a task the HTTP client spawns there (an HTTP/2 connection's dispatcher)
or a blocking lookup it starts is the exporter's too, and an export failure
cannot export itself; `rmcp`, which may carry an MCP server's words whole, at
any level; and `keeper::frontend`, whose report leaves as its own record. The
same HTTP crates' lines from any other runtime (a sync's connection error)
are problems like any other. Lines from before the layer is attached at boot
(the first lines of `setup`) are not forwarded.

**Where it runs.** The Mac, the iPhone and Android — every build that embeds
the public PostHog pair: release CI, `install-macos.sh`, and since 102.1
`install-ios.sh` and `install-android.sh` (`scripts/lib/posthog-client.sh`;
the phone builds also stamp `crates/keeper/posthog-client.env`, which
`build.rs` reads when the environment is silent; `install-ios.sh` removes the
local copy once the rsync has carried it and the Mac's copy once the Rust build
has read it, by a trap that covers success, failure and interruption, R499).
keeper-agentd and
keeper-nse install no such layer and link no exporter crate
(`check:agentd-lean`, `check:nse-lean`); electra's agent host logs to
journald only.

### Closed events

With **diagnostics** consent: app-ready and interaction durations and their
content-free OTLP records; with **product statistics**: opening the command
palette or Settings. `keeper_interaction` may carry one closed `surface`:
`slash_menu_opened`, `slash_menu_chosen`, `launch_default_agent` (Epic 100;
the front's `captureSurface(surface)` in `src/hooks/use-telemetry.ts`).

One closed event comes from the shell rather than the webview:
`keeper_recording_mic_lost`, under **diagnostics** consent, with one closed
`cause` — `removed` (`micLost`), `stalled` (`micStalled`) or `silent`
(`micSilent`) — and `event_source` `shell`. The recording sink raises it when
the session accepts that warning, at most once per cause per session, because
the sidecar repeats a loss (a failed fallback sends a second `micLost`, silence
and loss re-arm when sound returns). It has no duration and never the device
name or the warning's text (`keeper_core::telemetry::MicLossCause`,
`Telemetry::capture_mic_lost`). It is not part of the webview's
`telemetry_capture` contract.

**Never sent on purpose:** unredacted text, stack traces, chat messages,
notes, recordings, bot prompts or answers, an agent line's message or fields,
and nothing at all without diagnostics consent. A warning's own words can
still name a file, folder or device (DW-1512): the redactor is lexical.

### Reading it

Project 605256 (`https://us.posthog.com`). Problems are Error tracking issues;
by HogQL: `SELECT timestamp, properties.log_target,
properties.$exception_list[1].value FROM events WHERE event = '$exception'
ORDER BY timestamp DESC LIMIT 50` with the maintainer's personal key
(`op://tg/keeper-posthog-personal-api-key/credential`, never printed).

## Agents

Agents (`keeper-agent`, `keeper-agentd`, `keeper_core::agents`) send PostHog
no content: a desktop or phone agent line leaves with its level, target,
file:line and fingerprint only (R461; `telemetry/tests.rs`
`agent_target_lines_leave_without_their_message`), and the daemon on a server
links no exporter at all (`bun run check:agentd-lean`). An agent-host failure
on the desktop is a `WARN`/`ERROR` line in `keeper.log` and the agent room's
own status line; the daemon on a server writes its own log beside its
sessions ([agents.md](agents.md)).
