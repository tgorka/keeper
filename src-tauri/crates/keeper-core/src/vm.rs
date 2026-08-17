//! IPC view models (AD-7, AD-8).
//!
//! Every type that crosses the Tauri IPC boundary lives here, derives
//! `serde` + [`ts_rs::TS`], is `#[ts(export)]`, and renames fields to
//! camelCase. Timestamps are `i64` milliseconds since the Unix epoch (UTC) —
//! never strings. Bindings are emitted to `src/lib/ipc/gen/` by the ts-rs
//! export test step (`cargo nextest run`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::notes::export::NoteExportPlan;
use crate::signals::IncognitoScope;

/// The resolved Incognito state for a chat, projected to the frontend (Story 8.1).
///
/// The frontend renders this VM only — it never resolves precedence itself. `effective`
/// is the resolved on/off; `source` names *which* scope decided it (Chat > Account >
/// Global) so the header chip can read "this chat overrides account" even when the
/// per-Chat value equals the account's. `global`/`account`/`chat` echo the raw scope
/// values so the toggles reflect their own tri-state (`account`/`chat` are
/// `bool | null`, `null` = inherit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IncognitoVm {
    /// The resolved effective on/off — drives the private-vs-public receipt path.
    pub effective: bool,
    /// The scope that decided the effective value (Chat > Account > Global).
    pub source: IncognitoScope,
    /// The global default (plain bool, off by default).
    pub global: bool,
    /// The per-Account override, or `None` to inherit the global scope.
    pub account: Option<bool>,
    /// The per-Chat override, or `None` to inherit the account/global scope.
    pub chat: Option<bool>,
}

/// The OS-global summon hotkey binding, projected to the Settings → Shortcuts
/// section (Story 9.4, FR-50).
///
/// `accelerator` is the current opaque binding (e.g. `"Control+Alt+Space"`);
/// `isDefault` is whether it equals the shipped default; `active` is whether that
/// binding is currently registered with the OS (`false` ⇒ the section explains what to
/// enable rather than showing nothing); `conflict` carries a *soft* warning when the
/// accelerator matches a curated common macOS system shortcut (assignment still
/// proceeds), else `null`. The frontend renders this VM only — it never derives
/// conflict or registration state itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HotkeyVm {
    /// The current accelerator string (opaque to the frontend; the shell parses it).
    pub accelerator: String,
    /// Whether the current accelerator equals the shipped default (`⌃⌥Space`).
    pub is_default: bool,
    /// Whether the accelerator is currently registered with the OS.
    pub active: bool,
    /// A soft conflict warning when the accelerator matches a curated macOS system
    /// shortcut; `None` for a novel combo.
    pub conflict: Option<String>,
}

/// Response of the `app_ping` liveness command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PingVm {
    /// Backend liveness marker, e.g. `"pong"`.
    pub message: String,
    /// Server-side timestamp: milliseconds since the Unix epoch (UTC).
    ///
    /// Emitted to TypeScript as `number`, not `bigint`: Tauri IPC delivers the
    /// `i64` as a JS number via `JSON.parse`, and ms-epoch values stay well
    /// within `Number.MAX_SAFE_INTEGER`. This keeps the binding matching the
    /// wire reality — the timestamp convention every later VM copies.
    #[ts(type = "number")]
    pub ts: i64,
}

/// The per-platform capability handshake (Story 12.2): a flat, data-driven set of
/// booleans, one per optional platform surface, served by the shell's
/// `capabilities` command at startup and mirrored by the frontend.
///
/// `false` means the surface is **absent** on this build — the UI hides it (Epic
/// 13) rather than offering an action that would fail. The struct lives here in
/// `keeper-core::vm` (the VM home) but is *populated* per-platform in the shell
/// crate, keeping the core free of `cfg(target_os)` (AD-26). A later target
/// (Android / Windows) reuses this same shape by reporting its own flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CapabilitiesVm {
    /// The opt-in menu-bar (tray) icon (Story 10.3) exists on this platform.
    pub tray_icon: bool,
    /// The OS-global summon/hide hotkey (Story 9.4) exists on this platform.
    pub global_hotkey: bool,
    /// The launch-at-login toggle (Story 10.3, AD-25) exists on this platform.
    pub launch_at_login: bool,
    /// The in-app updater flow (Story 11.2) exists on this platform (app-store
    /// distribution channels never get an in-app updater).
    pub in_app_updater: bool,
    /// The registry-derived native menu bar (Story 9.3) exists on this platform.
    pub native_menu_bar: bool,
    /// The `bbctl` bridge sidecar (Story 6.7) can exist on this platform (no
    /// child processes / sidecars on mobile).
    pub bridge_sidecar: bool,
    /// "Reveal in Finder"-style file-manager reveal (Story 5.5) exists on this
    /// platform.
    pub reveal_in_file_manager: bool,
    /// Screen recording (Story 16.3) exists on this platform: `true` only on
    /// desktop macOS ≥ 13.0 (the system-audio floor), `false` on older macOS,
    /// every non-macOS desktop, and iOS. Computed in the shell from a runtime
    /// OS-version probe, keeping `keeper-core` free of `cfg(target_os)` (AD-26).
    pub recording: bool,
    /// Folder sync (Story 23.5, AD-41/AD-51) can run here: `true` only on
    /// desktop **with a usable `git` binary**, which is why this is a runtime
    /// probe rather than a `cfg!`. gitoxide implements neither push, nor
    /// worktree mutation, nor sparse patterns, nor merge, so without `git` the
    /// engine cannot be constructed at all — and every sync surface stays
    /// absent rather than failing when pressed (AD-27, "no dead buttons").
    pub sync: bool,
    /// Notes (Phase 5, FR-122) can run here: `sync && desktop`, computed in the
    /// shell like every other flag in this struct. It sits beside `sync` because
    /// it is strictly narrower — a vault is a folder keeper already syncs plus a
    /// flag, so a build without folder sync has nowhere to put one, and iOS gets
    /// `false` for both. When it is `false` every notes affordance is **absent**
    /// from the DOM rather than disabled, which is the whole of FR-122.
    pub notes: bool,
    /// Sessions (Phase 7, FR-223) can run here: `sync && desktop`, computed in
    /// the shell like every other flag in this struct. It sits beside `notes`
    /// because it is the same construction — a sessions root is a folder keeper
    /// already syncs plus a flag (AD-107), so a build without folder sync has
    /// nowhere to put one, and iOS gets `false` for both. When it is `false`
    /// every sessions affordance is **absent** from the DOM rather than
    /// disabled, which is the whole of FR-223.
    pub sessions: bool,
    /// The window's title bar is a transparent overlay over the webview, so the
    /// native window controls float over page content (Story 34.2, AD-34-2):
    /// `true` only on desktop macOS, the only platform where `tauri.conf.json`'s
    /// `titleBarStyle: "Overlay"` + `hiddenTitle` apply at all. Everywhere else
    /// the OS draws a real title bar above the webview, so reserving an inset or
    /// painting a drag band there would be empty space under chrome the platform
    /// already owns. Those two config keys are the other half of this fact —
    /// changing them means changing this flag with them.
    pub overlay_title_bar: bool,
}

/// Stable, string-serialized error taxonomy for the IPC envelope.
///
/// Variants serialize to their camelCase names (e.g. `"unsupported"`) and are
/// part of the frontend contract — rename with care.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum IpcErrorCode {
    /// The requested capability is not supported on this platform/build.
    Unsupported,
    /// An unexpected internal error occurred in the backend.
    Internal,
    /// The homeserver does not support Simplified Sliding Sync (MSC4186).
    SlidingSyncUnsupported,
    /// The supplied username/password was rejected by the homeserver.
    InvalidCredentials,
    /// The homeserver could not be reached (DNS/connection/transport failure).
    ServerUnreachable,
    /// The homeserver does not offer password login (`m.login.password`).
    UnsupportedLoginType,
    /// The homeserver does not offer OIDC (OAuth 2.0 / MSC3861) login.
    /// Non-retriable — the user must pick a different login mechanism.
    /// Serializes as `"oauthUnsupported"`.
    OauthUnsupported,
    /// The OIDC browser round-trip did not complete before the timeout.
    /// Retriable — the sign-in may be started again. Serializes as
    /// `"oauthTimedOut"`.
    OauthTimedOut,
    /// The user cancelled the in-progress OIDC flow. Retriable — the sign-in may
    /// be started again; the UI returns quietly to the form. Serializes as
    /// `"oauthCancelled"`.
    OauthCancelled,
    /// The OIDC flow failed (a server `error=` callback or a token-exchange
    /// failure). Retriable — the sign-in may be started again. Serializes as
    /// `"oauthFailed"`.
    OauthFailed,
    /// The Beeper unofficial email-code login flow is unavailable (Story 2.3):
    /// a non-2xx / timeout / transport failure from `api.beeper.com`, a
    /// missing/renamed field (the private API changed shape), an abandoned flow,
    /// or a JWT / `org.matrix.login.jwt` rejection. Retriable — the UI returns to
    /// the email step to start a fresh flow. Serializes as `"beeperUnavailable"`.
    BeeperUnavailable,
    /// The account could not start (or continue) syncing: the persisted session
    /// was missing, session restore failed, or `SyncService` failed to start.
    /// Retriable — the subscribe may be attempted again.
    SyncUnavailable,
    /// A room's timeline could not be opened: the room was not found or the SDK
    /// `Timeline` failed to build. Retriable — the subscribe may be attempted
    /// again.
    TimelineUnavailable,
    /// An outgoing message could not be enqueued for send (room not found, no
    /// open timeline, the wedged echo was gone, or the SDK dispatch failed).
    /// Retriable — the send may be attempted again. Asynchronous delivery
    /// failures are *not* this code; they surface as the `Failed` send-state on
    /// the timeline item instead.
    SendFailed,
    /// An interactive device self-verification action failed (Story 3.2): crypto
    /// not ready, the flow id was not found, or an SDK action (accept / start_sas
    /// / confirm / mismatch / cancel / request) failed. Retriable — the user can
    /// restart verification. Serializes as `"verificationFailed"`.
    VerificationFailed,
    /// A recovery key pasted for key-backup restore could not be decoded — it is
    /// malformed (wrong length / not a valid base58 recovery key) (Story 3.3,
    /// FR-14). Named so the modal can say "that doesn't look like a recovery key"
    /// rather than a generic failure. Serializes as `"backupMalformedKey"`.
    BackupMalformedKey,
    /// A well-formed recovery key failed the MAC check for this account — it does
    /// not match (Story 3.3, FR-14). Named so the modal can say "recovery key
    /// didn't match this account" rather than a generic failure. Serializes as
    /// `"backupIncorrectKey"`.
    BackupIncorrectKey,
    /// Enabling key backup raced an existing server-side backup: a backup already
    /// exists on the homeserver (Story 3.3). Named so the modal can offer restore
    /// instead of a generic failure. Serializes as `"backupExists"`.
    BackupExists,
    /// A key-backup enable/restore action failed for another reason (crypto not
    /// ready, network, or another SDK error). Retriable — the user can try again.
    /// Serializes as `"backupFailed"`.
    BackupFailed,
    /// A best-effort receipt/typing signal dispatch failed (Story 3.9, AD-14).
    /// Non-retriable and best-effort: in practice receipts/typing are swallowed in
    /// the core (never surfaced to the UI), so this code exists only to keep the
    /// error funnel exhaustive. Serializes as `"signalDispatchFailed"`.
    SignalDispatchFailed,
    /// A notes operation was handed something malformed: unreadable frontmatter,
    /// a title that cannot become a filename, a space query that does not parse,
    /// or a template with a broken placeholder (Phase 5). Retriable in the only
    /// sense that matters — the input is what is wrong, so fixing the text fixes
    /// the call, which is why these do not funnel to `internal`. Serializes as
    /// `"notesInvalid"`.
    NotesInvalid,
    /// A submitted recording path template did not parse (Story 40.2, Epic 40):
    /// a `..` folder, a `:`, an unknown `{token}`, a last folder that can render
    /// to nothing. The same reasoning `NotesInvalid` carries applies exactly —
    /// the input is what is wrong, so fixing the text fixes the call, which is
    /// why this does not funnel to `internal`. Non-retriable: resubmitting the
    /// same template can only fail the same way. The message is 40.1's own
    /// rejection sentence, rendered inline beside the field. Serializes as
    /// `"recordingTemplateInvalid"`.
    RecordingTemplateInvalid,
    /// A recording session could not be retitled because its folder is claimed
    /// (Story 40.4, Epic 40): the shell holds every live session's folder in its
    /// reservation set, and a retitle takes a claim on the folder it is about to
    /// move, so the refusal covers both "it is still recording" and "another
    /// rename of it is already running" — the set holds paths, not reasons.
    /// Mapped to this code rather than letting it funnel to `internal`. Not
    /// retriable *while the recording runs*: the driver and the sidecar hold
    /// absolute paths into the folder, so nothing can move until the session
    /// stops, and the surface needs to say what is holding the folder rather
    /// than "internal error". Serializes as `"recordingSessionLive"`.
    RecordingSessionLive,
    /// A submitted recording destination was refused before anything was written
    /// (Story 41.2, Epic 41, UX-DR47): a sync profile id that names no profile,
    /// names a paused one, or names one that does not say it holds recordings;
    /// or a plain folder that sits inside a synced folder's tree without being
    /// that folder's recordings root — the ambiguous case that would otherwise
    /// sync by accident, with nothing anywhere saying so.
    ///
    /// Its own code rather than `internal` for `RecordingTemplateInvalid`'s
    /// reason: the surface that submitted the destination has a control to point
    /// at, and the message names the synced folder it would have collided with.
    /// Non-retriable in every case but one — resubmitting the same choice can
    /// only fail the same way — while "the synced folders could not be read at
    /// all" (no usable `git` on this machine) IS retriable, because installing
    /// one changes the answer. Serializes as `"recordingDestinationRefused"`.
    RecordingDestinationRefused,
}

/// The account's live server-side key-backup posture, mapped from the SDK
/// `client.encryption().recovery().state()` (Story 3.3, FR-14, AD-8).
///
/// A Rust-authoritative honest signal streamed over the backup-status channel:
/// `Unknown` before crypto has synced ("Checking…"), `Disabled` when no backup is
/// set up (offer "Set up backup"), `Enabled` once this device is connected to the
/// backup ("Backup on"), `Incomplete` when a backup exists on the server but this
/// device is not yet connected — the fresh-login restore case ("Needs your
/// recovery key"). The Settings backup row is a pure projection of this one
/// status. Only the enum tag crosses IPC — never any key or secret-storage
/// material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BackupStatus {
    /// The recovery state is not yet known — crypto has not synced. Renders
    /// "Checking…" (avoid a false claim before the OlmMachine reports).
    Unknown,
    /// No default secret-storage key exists / recovery is disabled — no backup is
    /// set up. The Settings row offers "Set up backup".
    Disabled,
    /// Secret storage is set up and this device has all the secrets locally —
    /// backup is on. The Settings row reads "Backup on".
    Enabled,
    /// A backup exists on the server but this device is missing some secrets — the
    /// fresh-login restore case. The Settings row offers "Restore".
    Incomplete,
}

/// The delivery state of an outgoing (local-echo) message (FR-9, AD-13, UX-DR10).
///
/// Derived from the SDK `EventSendState` of a local echo: a message being
/// enqueued or retried is `Sending`; a message the server acknowledged is
/// `Sent`; a message whose send failed unrecoverably is `Failed`. A remote
/// (received or reconciled) item has no send state and maps to `None` on the VM.
/// Only the enum tag crosses IPC — never the txn id, error object, or event id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SendState {
    /// The message is being enqueued or is in flight (including a transient,
    /// recoverable failure the send queue is still auto-retrying).
    Sending,
    /// The homeserver acknowledged the message.
    Sent,
    /// The message failed to send unrecoverably; it is actionable via Retry and
    /// its caption never auto-clears.
    Failed,
}

/// The account's live connectivity, as mapped from the SDK `SyncService` state
/// (FR-8/FR-9, UX-DR10, UX-DR18, AD-8).
///
/// A Rust-authoritative signal streamed over the connection-status channel:
/// `Online` when the `SyncService` is `Running`, `Offline` for every other state
/// (`Idle`, `Terminated`, `Error`, `Offline`). The frontend renders the offline
/// pill and the "Queued" send caption as pure projections of this one status —
/// no timeline item is invented or mutated. Only the enum tag crosses IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ConnectionStatus {
    /// The `SyncService` is `Running` — the account is connected and syncing.
    Online,
    /// The `SyncService` is not `Running` — the account is disconnected; sends
    /// queue in the SDK's persistent send queue until connectivity returns.
    Offline,
}

/// A batch delivered over the connection-status subscription's `Channel` (AD-8).
///
/// The status is a scalar snapshot, so each batch carries the full current
/// [`ConnectionStatus`] — inherently idempotent, safe to re-subscribe. The stream
/// opens with the current mapped status, then emits on change (deduped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConnectionStatusBatch {
    /// The current connectivity status.
    pub status: ConnectionStatus,
}

/// The remote (cross-device) draft read back from the account-data mirror for a
/// `(account, room)` (Story 7.2, AD-15). Returned by `load_remote_draft` and
/// carried in a [`DraftMirrorBatch`] on a live remote edit.
///
/// **Local always wins**: this is only ever read to *offer* adoption. `body` is
/// always non-empty here — an empty body reads back as "no remote draft"
/// (`None`), so a tombstone never surfaces as an adoptable draft. `updated_ts` is
/// informational/forward-scaffolding only; the winner rule is purely local-wins
/// and never consults a timestamp. The body is never logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoteDraftVm {
    /// The remote draft body (always non-empty; empty maps to `None`).
    pub body: String,
    /// Write time in milliseconds since the Unix epoch (UTC). Informational only.
    #[ts(type = "number")]
    pub updated_ts: i64,
}

/// A batch delivered over the app-wide draft-mirror subscription's `Channel`
/// (Story 7.2, AD-15). Each batch carries one account/room's live remote-draft
/// change observed via the `dev.keeper.draft` room-account-data event handler.
///
/// A tombstone (empty body) arrives with `body: None` so the frontend clears any
/// offered remote draft for that key. The body is never logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DraftMirrorBatch {
    /// The owning account id.
    pub account_id: String,
    /// The room id the remote draft belongs to.
    pub room_id: String,
    /// The remote draft body, or `None` for a tombstone (cleared remote draft).
    pub body: Option<String>,
    /// Write time in milliseconds since the Unix epoch (UTC). Informational only.
    #[ts(type = "number")]
    pub updated_ts: i64,
}

/// One pending draft row for the cross-account approval pane (Story 7.3), sourced
/// from a cross-account query over the `drafts` table enriched with the owning
/// account's identity/hue and the room's display name + bridge network.
///
/// Metadata resolution is best-effort: an offline account whose room cannot be
/// resolved still yields a row — `display_name` falls back to `room_id` and
/// `network` to `None`. A pending draft is never hidden. The body is authoritative
/// in Rust and never logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApprovalDraftVm {
    /// The owning account id.
    pub account_id: String,
    /// The owning account's Matrix user id (section header identity).
    pub account_user_id: String,
    /// The owning account's hue index (0..8) for the account-hue edge.
    pub hue_index: u8,
    /// The room the draft belongs to.
    pub room_id: String,
    /// The room's display name, or `room_id` when the room cannot be resolved.
    pub display_name: String,
    /// The bridge network the room belongs to, or `None` when unresolved / native.
    pub network: Option<String>,
    /// The authoritative draft body (from Rust).
    pub body: String,
    /// Last write time in milliseconds since the Unix epoch (UTC).
    #[ts(type = "number")]
    pub updated_ts: i64,
}

/// The account's live device-verification (encryption) posture, mapped from the
/// SDK `client.encryption().verification_state()` (Story 3.1, FR, AD-8).
///
/// A Rust-authoritative honest signal streamed over the encryption-status
/// channel: `Unknown` before crypto has synced (never nag), `Verified` once this
/// device's user identity has signed it, `Unverified` for a freshly-logged-in
/// device that cannot yet read encrypted history. The "verify this device" banner
/// and the Settings badge are pure projections of this one status. Only the enum
/// tag crosses IPC — never any key, session, or crypto material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum EncryptionStatus {
    /// The verification state is not yet known — crypto has not synced. No banner
    /// and no badge (avoid a false nag before the OlmMachine reports).
    Unknown,
    /// This device is verified — its user identity has signed it. The banner and
    /// badge both clear.
    Verified,
    /// This device is unverified — encrypted history is locked until the user
    /// verifies it (Story 3.2) or restores key backup (Story 3.3). Drives the
    /// banner / badge.
    Unverified,
}

/// A batch delivered over the encryption-status subscription's `Channel` (AD-8).
///
/// The status is a scalar snapshot, so each batch carries the full current
/// [`EncryptionStatus`] — inherently idempotent, safe to re-subscribe. The stream
/// opens with the current mapped status, then emits on change (deduped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EncryptionStatusBatch {
    /// The current device-verification status.
    pub status: EncryptionStatus,
}

/// One emoji of the SAS short-authentication string (Story 3.2, FR-14, NFR-9).
///
/// A rendered projection of the SDK `Emoji` — its Unicode `symbol` and the
/// human-readable `name` (the SDK's `description`). Both are non-secret display
/// strings; NO SAS key, decimal, or crypto material crosses IPC on this VM. The
/// webview renders the symbol with its `name` in `mono` type (epic typography).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SasEmojiVm {
    /// The emoji symbol (e.g. `"🐶"`).
    pub symbol: String,
    /// The emoji's human-readable name (e.g. `"Dog"`).
    pub name: String,
}

/// The phase of an interactive self-verification flow (Story 3.2, FR-14,
/// UX verification-flow states).
///
/// A Rust-authoritative projection of the SDK's native `VerificationRequestState`
/// / `SasState` machine. The webview renders each phase distinctly (waiting,
/// comparing, confirmed, done, cancelled, failed) using the SDK's own vocabulary —
/// it never invents crypto UX. Only the enum tag crosses IPC. `Cancelled` and
/// `Failed` are intentionally distinct: a clean user/peer cancel is `Cancelled`;
/// a mismatch / timeout / other terminal cancel code is `Failed` (with a reason).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VerificationPhase {
    /// A request exists but is not yet ready — waiting for the other device to
    /// accept (or for us to accept an incoming request).
    Requested,
    /// The request is ready; a QR code may be shown and SAS can be started.
    Ready,
    /// SAS keys are exchanged — the two sides compare the emoji.
    Comparing,
    /// We confirmed the emoji match; waiting for the other device to confirm.
    Confirmed,
    /// The verification completed successfully. Story 3.1's `verification_state()`
    /// stream then flips the account to `Verified`, clearing the banner/badge.
    Done,
    /// The flow was cleanly cancelled (by the user or the peer).
    Cancelled,
    /// The flow failed (emoji mismatch, timeout, or another terminal cancel
    /// code). Carries a human-readable `reason`.
    Failed,
}

/// A snapshot of an interactive self-verification flow, delivered over the
/// verification subscription's `Channel` (Story 3.2, FR-14, AD-1, NFR-9).
///
/// The single view model the webview renders for the whole flow. Carries **only**
/// non-secret render data: the opaque `flow_id`, the current [`VerificationPhase`],
/// the SAS emoji list (symbols + names) when comparing, a pre-rendered QR SVG
/// string when a QR is available, and a human `reason` on cancel/failure. NO
/// `Verification`/`Sas`/`QrVerification` object, SAS key, decimal, or plaintext
/// ever crosses IPC on this VM (AD-1). Actions reference the flow by `flow_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VerificationFlowVm {
    /// The SDK verification flow id (opaque, passed through verbatim). Actions
    /// (accept/start_sas/confirm/mismatch/cancel) reference the flow by this id.
    pub flow_id: String,
    /// The current flow phase.
    pub phase: VerificationPhase,
    /// The 7 SAS emoji to compare, present only in the `Comparing` phase.
    pub emojis: Option<Vec<SasEmojiVm>>,
    /// A pre-rendered QR-code SVG string (keeper's own QR for the peer to scan),
    /// present when a QR is available in the `Ready` phase.
    pub qr_code_svg: Option<String>,
    /// A human-readable reason, present on `Cancelled` / `Failed`.
    pub reason: Option<String>,
}

/// A single room row rendered in the chat list (FR-8, NFR-9, AD-20).
///
/// Carries **only** non-secret render data. `timestamp` is `i64` milliseconds
/// since the Unix epoch (UTC) — never an ISO string. `lastMessage` is the
/// plain-text body of the room's latest event when it is an `m.room.message`
/// (text/notice/emote); `null` for any other event kind. No tokens, session
/// material, or event ids cross IPC on this VM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoomVm {
    /// Opaque Matrix room id (passed through verbatim as a string).
    pub room_id: String,
    /// The SDK-computed room display name.
    pub display_name: String,
    /// Plain-text preview of the latest `m.room.message`, or `null`.
    pub last_message: Option<String>,
    /// Latest-event timestamp: ms since the Unix epoch (UTC), or `null`.
    #[ts(type = "number | null")]
    pub timestamp: Option<i64>,
    /// Optional room avatar URL (an `mxc://` URI), or `null`.
    pub avatar_url: Option<String>,
    /// Authoritative unread flag: `true` when the room has unread messages,
    /// unread mentions, or the manual `m.marked_unread` flag set (AD-20). The
    /// frontend renders this directly (bold name + dot/badge) and never
    /// re-derives it from events.
    pub is_unread: bool,
    /// Count of unread mentions (client-side, precise for E2EE). Drives the
    /// filled primary mention badge; a value of 0 shows a plain dot when
    /// `is_unread` is otherwise set.
    #[ts(type = "number")]
    pub mention_count: u32,
    /// Authoritative archive flag: `true` when the room carries the Matrix
    /// low-priority tag (`m.lowpriority`) (Story 4.2, AD-20). The inbox merge
    /// partitions on this to place the room in the Archive window unless it is
    /// unread (auto-return is a pure view rule); the frontend never re-derives it.
    pub is_archived: bool,
    /// Authoritative favourite flag: `true` when the room carries the Matrix
    /// favourite tag (`m.favourite`) (Story 4.4, AD-20). This is a *notable* tag,
    /// so a change re-emits the room-list stream live and syncs cross-client. The
    /// inbox merge partitions on this to place the room in the Favorites window
    /// (removed from Inbox/Archive); the frontend never re-derives it.
    pub is_favourite: bool,
    /// Whether the room is itself a Matrix Space (`Room::is_space()`, `m.space`
    /// room type) (Story 4.5, AD-20). Used only to *exclude* Space rooms from the
    /// four inbox chat windows in the merge — Spaces are containers, not chats, and
    /// are surfaced separately as filter views. Not copied to [`InboxRoomVm`]; the
    /// merge drops `is_space` rooms before partitioning.
    pub is_space: bool,
    /// The bridged-Network label for this room (Story 4.6, FR-24), resolved from
    /// the room's MSC2346 `m.bridge` (or legacy `uk.half-shot.bridge`) state via
    /// [`crate::bridge::room_bridge_network`] — "Telegram", "WhatsApp", "Signal", …
    /// `None` for a native Matrix room (no bridge state); it then shows no badge and
    /// is excluded from the distinct-Networks list. Copied through to
    /// [`InboxRoomVm`] and used both for the avatar Network badge and the ephemeral
    /// Network filter. Never fabricated — it is untrusted, length-capped state.
    pub network: Option<String>,
    /// The room's stable bridge `network_id` — the machine `protocol.id` (Story 6.5,
    /// FR-28), resolved from the room's MSC2346 `m.bridge` state via
    /// [`crate::bridge::room_bridge_protocol_id`] (e.g. `"whatsapp"`, `"telegram"`).
    /// Distinct from the display `network` label: this is the join key that matches a
    /// room to an unhealthy bridge session on `(account_id, network_id)`. `None` for a
    /// native Matrix room (no bridge state). Copied through to [`InboxRoomVm`]. Never
    /// fabricated — it is untrusted, server-controlled state used only as a map key.
    pub network_id: Option<String>,
    /// The durable per-Chat / per-Network mute intent for this room (Story 10.2,
    /// FR-52), resolved at projection time from the room's synced push-rule mode plus
    /// the keeper-local muted-Network set. Copied through to [`InboxRoomVm`] to render
    /// the mute glyph; never gates unread. Fail-open `None` on any read error.
    pub mute_state: MuteState,
}

/// One Matrix Space the user belongs to, surfaced as a filter view (Story 4.5,
/// FR-22, AD-20).
///
/// Carries **only** non-secret render data: the opaque keeper `account_id` that
/// owns the Space, the opaque Space room id, the SDK-resolved display name, and an
/// optional avatar `mxc://` URI. Enumerated locally from
/// `Client::joined_space_rooms()` (no `/hierarchy` network fetch); membership (the
/// Space's joined children) is computed alongside but stays in the merger — never
/// on this VM. The frontend renders a SPACES sidebar row per `SpaceVm` and, on
/// select, pokes the ephemeral Space filter identified by `(account_id, space_id)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpaceVm {
    /// Opaque keeper account id that owns this Space. Part of the selection key.
    pub account_id: String,
    /// Opaque Matrix room id of the Space (passed through verbatim as a string).
    pub space_id: String,
    /// The SDK-computed Space display name.
    pub name: String,
    /// Optional Space avatar URL (an `mxc://` URI), or `null`.
    pub avatar_url: Option<String>,
}

/// The full current Space list, streamed as a whole-snapshot batch on the inbox
/// subscription's fifth `Channel` (Story 4.5, AD-20).
///
/// Spaces are few, so there is no diff protocol: each batch carries the complete
/// aggregated list across every account (stable account-id order), and the
/// frontend replaces its list wholesale. Emitted on subscribe, then on every sync
/// batch that changes the Space list or its membership, and on account removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpacesSnapshot {
    /// Every joined Space across all accounts, in stable account-id order.
    pub spaces: Vec<SpaceVm>,
}

/// One bridged Network connected in the merged inbox, surfaced as a filter view
/// (Story 4.6, FR-24, AD-20).
///
/// Carries **only** the Network's display `name`, deduped by name across accounts —
/// a Network is identified cross-account by its label (a Telegram bridge on two
/// accounts is one Network row). Derived in the merger from the distinct non-`None`
/// [`RoomVm::network`] values of the unfiltered merged set (name-sorted, native
/// rooms excluded). The frontend renders a NETWORKS sidebar row per `NetworkVm`
/// and, on select, pokes the ephemeral Network filter identified by `name`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetworkVm {
    /// The bridged Network's display name (the filter selection key).
    pub name: String,
}

/// The full current distinct-Networks list, streamed as a whole-snapshot batch on
/// the inbox subscription's sixth `Channel` (Story 4.6, AD-20).
///
/// Networks are few, so there is no diff protocol: each batch carries the complete
/// deduped, name-sorted list derived from the *unfiltered* merged set, and the
/// frontend replaces its list wholesale. Emitted on every merge `emit` (so it stays
/// live with sync and stable regardless of an active Space/Network filter).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetworksSnapshot {
    /// Every distinct connected Network, deduped by name and name-sorted.
    pub networks: Vec<NetworkVm>,
}

/// One index-based room-list operation mirroring an eyeball-im `VectorDiff`
/// (AD-8, AD-20).
///
/// The SDK's `entries_with_dynamic_adapters` stream is recency-sorted; keeper
/// forwards its `VectorDiff` sequence verbatim as these ops. The frontend
/// applies them to a plain array by index and **never** re-sorts. Serialized as
/// an internally tagged enum so the frontend can switch on `op`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum RoomListOp {
    /// Full reset — replace the current contents with `rooms`.
    Reset {
        /// The complete current window, in order.
        rooms: Vec<RoomVm>,
    },
    /// Append `rooms` to the end, in order.
    Append {
        /// Rooms to append.
        rooms: Vec<RoomVm>,
    },
    /// Remove all rooms.
    Clear,
    /// Insert `room` at the front (index 0).
    PushFront {
        /// The room to prepend.
        room: RoomVm,
    },
    /// Append `room` to the end.
    PushBack {
        /// The room to append.
        room: RoomVm,
    },
    /// Remove the first room.
    PopFront,
    /// Remove the last room.
    PopBack,
    /// Insert `room` at `index`, shifting the tail right.
    Insert {
        /// The insertion index.
        #[ts(type = "number")]
        index: u32,
        /// The room to insert.
        room: RoomVm,
    },
    /// Replace the room at `index` in place.
    Set {
        /// The index to overwrite.
        #[ts(type = "number")]
        index: u32,
        /// The replacement room.
        room: RoomVm,
    },
    /// Remove the room at `index`, shifting the tail left.
    Remove {
        /// The index to remove.
        #[ts(type = "number")]
        index: u32,
    },
    /// Truncate the list to `length` rooms.
    Truncate {
        /// The new length.
        #[ts(type = "number")]
        length: u32,
    },
}

/// A batch of room-list ops delivered over the subscription's `Channel` (AD-8).
///
/// The stream always opens with a batch whose first op is a
/// [`RoomListOp::Reset`] carrying the current window, then diff batches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoomListBatch {
    /// The ordered ops to apply, in sequence.
    pub ops: Vec<RoomListOp>,
    /// The total number of rooms the server knows about, when known.
    #[ts(type = "number | null")]
    pub total: Option<u32>,
}

/// The quoted-original preview of a reply message (Story 3.4, FR-10, NFR-9).
///
/// Derived in the timeline producer from `content.in_reply_to()`. Carries
/// **only** non-secret render data: the resolved *original* item's opaque render
/// `key` when it is loaded in the timeline (so the frontend can scroll to it),
/// the original sender's Matrix user id, a resolved display name, and the decoded
/// plain-text body (empty when the original is non-text). NO event ids, txn ids,
/// or raw event JSON cross IPC on this VM (AD-1) — the jump target is the same
/// opaque `key` (unique_id) used everywhere, resolved in Rust via the producer's
/// `event_id → unique_id` index. When the original is not loaded, `in_reply_to_key`
/// is `null` and the quote renders honestly but is not clickable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReplyPreviewVm {
    /// The *original* (replied-to) item's opaque render key (its `unique_id`)
    /// when that original is currently loaded in the timeline, else `null`. The
    /// frontend uses it to scroll the original into view; never an event id.
    pub in_reply_to_key: Option<String>,
    /// The original sender's Matrix user id (opaque, passed through verbatim).
    pub sender: String,
    /// The original sender's resolved display name, or `null` when unavailable.
    pub sender_display_name: Option<String>,
    /// The decoded plain-text body of the original message, or an empty string
    /// when the original is non-text or its details are unavailable.
    pub body: String,
}

/// One version in a message's edit history, fed by the Local Archive (Story 5.2,
/// FR-11).
///
/// The archive-fed edit-history popover lists these newest-first for a message
/// whose "Edited" caption is clicked. Carries **only** non-secret render data: the
/// version's display text, its origin timestamp, and whether it is the current
/// (newest) version. NO event ids or relation logic cross IPC on this VM (AD-1) —
/// the frontend addresses the message by its opaque render `key` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EditVersionVm {
    /// The decoded plain-text body of this version (the original's top-level
    /// `body`, or an edit's `m.new_content.body`).
    pub body: String,
    /// This version's origin server timestamp: milliseconds since the Unix epoch.
    #[ts(type = "number")]
    pub timestamp: i64,
    /// `true` for the current (newest) version, `false` for a prior version.
    pub is_current: bool,
}

/// The archive search request crossing IPC into the `search_archive` command
/// (Story 5.3, FR-34).
///
/// A deserialize-only input VM: every filter is optional. Empty `account_ids` /
/// `room_ids` lists mean unrestricted (the boundary for both the "Chat" and
/// "Network" UI filters — Story 5.4 resolves a Network selection to its `room_ids`
/// set before calling). `sender` is a Matrix user id; `startTs`/`endTs` bound
/// `origin_ts` in ms since the Unix epoch; `limit` caps the hit count (the engine
/// clamps it to a sane maximum). The core maps this to its tauri-free
/// `SearchFilter` domain struct — no bridge/session state ever crosses here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchFilterVm {
    /// The user's query text (dispatched to trigram MATCH at ≥3 Unicode scalar
    /// values, else an accelerated `LIKE` scan).
    pub query: String,
    /// Restrict to these keeper account ids; empty ⇒ all accounts.
    #[serde(default)]
    pub account_ids: Vec<String>,
    /// Restrict to these room ids; empty ⇒ all rooms.
    #[serde(default)]
    pub room_ids: Vec<String>,
    /// Restrict to this sender (Matrix user id), or `null` for any sender.
    #[serde(default)]
    pub sender: Option<String>,
    /// Inclusive lower bound on `origin_ts` (ms since the Unix epoch), or `null`.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub start_ts: Option<i64>,
    /// Inclusive upper bound on `origin_ts` (ms since the Unix epoch), or `null`.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub end_ts: Option<i64>,
    /// Cap on the number of hits, or `null` for the engine's default. The engine
    /// clamps this to `[1, max]`.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub limit: Option<i64>,
}

/// One archive search result crossing IPC out of the `search_archive` command
/// (Story 5.3, FR-34).
///
/// Carries the `(account_id, room_id, event_id)` deep-link identifiers the epic AC
/// mandates for jumping into a timeline at the matched message, plus render data:
/// sender, the matched display body, its timestamp, and whether the row is
/// redacted. `eventId` is the chain root (the edit target when the match was on a
/// prior version, else the row's own event id), so every version deep-links to the
/// same timeline item. This `eventId` is the epic-authorized search-scoped
/// exception to the no-ids rule (see the Story 5.3 design notes) — no tokens,
/// session material, or full event content beyond the display body crosses here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchHitVm {
    /// Opaque keeper account id the matched message belongs to.
    pub account_id: String,
    /// Matrix room id the matched message was sent to.
    pub room_id: String,
    /// The chain-root Matrix event id — the sanctioned deep-link handle.
    pub event_id: String,
    /// Matrix user id of the sender.
    pub sender: String,
    /// The matched display body (an edit's `m.new_content.body`, else the
    /// original's top-level `body`).
    pub body: String,
    /// The matched row's origin server timestamp: ms since the Unix epoch (UTC).
    #[ts(type = "number")]
    pub timestamp: i64,
    /// `true` when the matched row has been marked remotely redacted. Only ever
    /// `true` in results when the honor-deletions setting is off (when on, redacted
    /// rows are excluded entirely).
    pub redacted: bool,
}

/// Which slice of the archive an export covers (Story 5.5, FR-35, AD-11).
///
/// The scope discriminant for [`ExportRequestVm`]: `Chat` restricts to one
/// `(accountId, roomId)`, `Account` to one account across all its rooms, and
/// `Everything` to every account. Serializes to its camelCase name — the frontend
/// wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ExportScopeKind {
    /// A single Chat: `accountId` + `roomId` both required.
    Chat,
    /// A single Account: `accountId` required, all its rooms.
    Account,
    /// Every Account and every room in the archive.
    Everything,
}

/// The archive-export request crossing IPC into the `export_start` command
/// (Story 5.5, FR-35, AD-11).
///
/// A deserialize-only input VM. `scope` picks the archive slice; `accountId` is
/// required for `Chat`/`Account` scope and ignored for `Everything`; `roomId` is
/// required for `Chat` scope only. `json`/`markdown` are the two output formats
/// (at least one must be true — the dialog enforces it). `includeMedia` governs a
/// best-effort media byte copy (skipped-and-counted when unresolvable — never
/// fatal). `destinationDir` is the OS folder the user picked (a scope subfolder is
/// created under it). No bridge/session state ever crosses here — the export reads
/// `archive.db` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportRequestVm {
    /// Which archive slice to export (chat / account / everything).
    pub scope: ExportScopeKind,
    /// The keeper account id for `Chat`/`Account` scope, else `null`.
    #[serde(default)]
    pub account_id: Option<String>,
    /// The Matrix room id for `Chat` scope, else `null`.
    #[serde(default)]
    pub room_id: Option<String>,
    /// Emit the lossless JSON array (every archived row in scope).
    pub json: bool,
    /// Emit the chronological Markdown transcript.
    pub markdown: bool,
    /// Best-effort copy of media bytes into `<export>/media/` when resolvable.
    pub include_media: bool,
    /// The OS destination folder the user picked (the scope subfolder lands here).
    pub destination_dir: String,
}

/// The terminal (or in-flight) phase of a running export job (Story 5.5).
///
/// Streamed on [`ExportProgressVm::phase`]: `Running` for every progress batch,
/// then exactly one terminal batch — `Completed` on success, `Cancelled` when the
/// user cancelled (partial output cleaned), or `Failed` on an error (partial
/// output cleaned, `error` set). Serializes to its camelCase name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ExportPhase {
    /// The job is still writing rows — a progress batch with live counts.
    Running,
    /// The job finished successfully; `outputPaths` are the written files.
    Completed,
    /// The user cancelled; partial output was deleted before this batch.
    Cancelled,
    /// The job failed; partial output was deleted and `error` describes it.
    Failed,
}

/// A progress (or terminal) batch streamed over the export subscription's
/// `Channel` (Story 5.5, FR-35, UX-DR11).
///
/// Carries **only** non-secret progress data: the job's `exportId`, its current
/// [`ExportPhase`], the running message/media counts, the written `outputPaths`
/// (populated on `Completed`), and a human `error` string on `Failed`. No message
/// content, media bytes, or session material ever cross IPC on this VM — the
/// archive stays on disk and only file paths + counts are reported. The stream
/// emits `Running` batches as rows are written, then exactly one terminal batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportProgressVm {
    /// The job id (also the cancel handle for `export_cancel`).
    #[ts(type = "number")]
    pub export_id: u64,
    /// The current phase (`Running` until exactly one terminal batch).
    pub phase: ExportPhase,
    /// How many logical messages (Markdown transcript entries) have been written
    /// so far — the transcript-progress counter the UI shows.
    #[ts(type = "number")]
    pub messages_written: u64,
    /// The total logical messages in scope when known (the scoped root count), or
    /// `null` before it has been computed. Drives the progress bar's determinacy.
    #[ts(type = "number | null")]
    pub total_messages: Option<u64>,
    /// How many media items had their bytes copied into `media/` (best-effort).
    #[ts(type = "number")]
    pub media_copied: u64,
    /// How many media items were skipped (unresolvable / uncached / no resolver) —
    /// counted, never fatal; the link + metadata are still emitted.
    #[ts(type = "number")]
    pub media_skipped: u64,
    /// The written output file paths, populated on the `Completed` batch (the JSON
    /// and/or Markdown files under the scope subfolder). Empty on non-terminal /
    /// cleaned-up batches.
    pub output_paths: Vec<String>,
    /// A human-readable failure description on `Failed` (never content/secrets), or
    /// `null` otherwise.
    #[serde(default)]
    pub error: Option<String>,
}

/// One aggregated emoji-reaction group on a timeline message (Story 3.5, FR-12,
/// NFR-9).
///
/// Derived in the timeline producer from `content.reactions()` — one group per
/// distinct emoji key, in the SDK's per-key insertion order. Carries **only**
/// non-secret render data: the emoji string, the count of distinct reactors, and
/// whether the current account is one of them. NO per-sender user ids, reaction
/// event ids, or relation logic ever cross IPC on this VM (AD-1) — those stay
/// inside `keeper-core`. The frontend renders a click-to-toggle pill from these
/// three fields alone and dispatches a toggle by the message's opaque render key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReactionGroupVm {
    /// The reaction emoji / key (an arbitrary Matrix reaction string, passed
    /// through verbatim).
    pub emoji: String,
    /// The number of distinct reactors for this emoji (per-sender uniqueness is
    /// guaranteed by the SDK, so this is the inner sender-map length).
    #[ts(type = "number")]
    pub count: u32,
    /// Whether the current account has reacted with this emoji (its own user id
    /// is present in the emoji's inner sender map). Drives the own-highlight pill.
    pub is_own: bool,
}

/// The media class of an attached message (Story 3.6, FR-13, AD-4, NFR-9).
///
/// A Rust-authoritative projection of the media `MessageType` (`Image`/`Video`/
/// `Audio`/`File`) — the only render-facing discriminant the frontend needs to
/// pick a renderer (thumbnail image / video poster / inline audio / file chip).
/// Serializes to its camelCase name. NO `mxc`/`EncryptedFile`/key material is ever
/// implied by this tag — the bytes travel only over the `keeper-media://` protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MediaKindVm {
    /// An image attachment (`m.image`). Renders a thumbnail; opens full-res in the
    /// preview overlay.
    Image,
    /// A video attachment (`m.video`). Renders a poster; plays via `<video>` over
    /// the Range protocol in the overlay.
    Video,
    /// An audio attachment (`m.audio`). Plays inline via `<audio controls>` over
    /// the protocol.
    Audio,
    /// An arbitrary file attachment (`m.file`). Renders a file chip (icon + name +
    /// size); no auto-download of bytes over IPC.
    File,
}

/// The render-facing metadata of a media attachment on a message (Story 3.6,
/// FR-13, AD-4, NFR-9).
///
/// Carries **only** opaque `keeper-media://` URL strings plus display metadata —
/// never a `MediaSource`, `EncryptedFile`, `mxc://` URI, decryption key, or event
/// id (those stay inside `keeper-core`). `url` is the full-content protocol URL;
/// `thumbnail_url` is the thumbnail-variant protocol URL when a thumbnail is
/// available. The decrypted bytes are served exclusively over the
/// `keeper-media://` custom protocol (AD-4) — never as base64/JSON over IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaVm {
    /// The media class (image/video/audio/file), driving the renderer choice.
    pub kind: MediaKindVm,
    /// The opaque `keeper-media://…/full` protocol URL for the full content. The
    /// preview overlay and inline audio/video load from this; the SDK decrypts
    /// E2EE bytes behind the protocol handler. Never an `mxc` URI.
    pub url: String,
    /// The opaque `keeper-media://…/thumb` protocol URL for the thumbnail variant,
    /// present when a thumbnail is renderable (image/video), else `null`. The
    /// bubble renders this before the full content loads. Never an `mxc` URI.
    pub thumbnail_url: Option<String>,
    /// The attachment's display filename (from `.filename()`, falling back to the
    /// message body). Rendered in the file chip and as the media alt text.
    pub filename: String,
    /// The attachment's MIME type from `info.mimetype` (e.g. `"image/png"`), or
    /// `null` when the sender omitted it.
    pub mimetype: Option<String>,
    /// The attachment size in bytes from `info.size`, or `null` when omitted. The
    /// file chip renders a human-readable size from this.
    #[ts(type = "number | null")]
    pub size: Option<u32>,
    /// The intrinsic width in pixels (image/video `info.w`), or `null`. Used to
    /// reserve layout so the thumbnail does not reflow on load.
    #[ts(type = "number | null")]
    pub width: Option<u32>,
    /// The intrinsic height in pixels (image/video `info.h`), or `null`. Used to
    /// reserve layout so the thumbnail does not reflow on load.
    #[ts(type = "number | null")]
    pub height: Option<u32>,
    /// The media caption (the message `body` when it differs from the filename),
    /// or `null`. Rendered under the attachment.
    pub caption: Option<String>,
}

/// A single timeline item rendered in the conversation pane (FR-8, NFR-9,
/// AD-8/AD-9/AD-20).
///
/// Carries **only** non-secret render data. `timestamp` is `i64` milliseconds
/// since the Unix epoch (UTC) — never an ISO string. Exactly one VM is produced
/// per SDK `TimelineItem` so diff indices stay aligned; virtual, state,
/// redacted, undecryptable, and non-text items become an [`TimelineItemVm::Other`]
/// carrying only a stable opaque `key`. No tokens, session material, event raw
/// JSON, or crypto state cross IPC on this VM. Serialized as an internally
/// tagged enum so the frontend can switch on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum TimelineItemVm {
    /// A renderable text message (`m.room.message` of msgtype text/notice/emote).
    Message {
        /// Stable opaque render key (the item's `unique_id`).
        key: String,
        /// The sender's Matrix user id (opaque, passed through verbatim).
        sender: String,
        /// The resolved sender display name, or `null` when unavailable.
        sender_display_name: Option<String>,
        /// The decoded plain-text body of the already-decrypted message
        /// (defensively truncated before crossing IPC).
        body: String,
        /// The message origin timestamp: ms since the Unix epoch (UTC).
        #[ts(type = "number")]
        timestamp: i64,
        /// Whether the current account sent this message.
        is_own: bool,
        /// The delivery state of an outgoing local echo, or `null` for a remote
        /// (received or reconciled) message that carries no send state.
        send_state: Option<SendState>,
        /// Whether this message has been edited (`message.is_edited()`). The
        /// bubble renders an "Edited" caption when `true` (Story 3.4, FR-11).
        is_edited: bool,
        /// The quoted-original preview when this message is a reply
        /// (`content.in_reply_to()`), else `null` (Story 3.4, FR-10).
        reply: Option<ReplyPreviewVm>,
        /// The aggregated emoji-reaction groups on this message, in the SDK's
        /// per-key insertion order (empty when none) (Story 3.5, FR-12). Each
        /// group carries only `{ emoji, count, is_own }` — never a per-sender
        /// user id or reaction event id.
        reactions: Vec<ReactionGroupVm>,
        /// The media attachment when this message is an image/video/audio/file
        /// msgtype (Story 3.6, FR-13), else `null` for a text message. Carries only
        /// opaque `keeper-media://` URLs + display metadata — never a `MediaSource`,
        /// key, `mxc` URI, or event id (AD-4, NFR-9). `body` remains the caption.
        ///
        /// Boxed so the (media-less) text-message case does not pay the full
        /// [`MediaVm`] size on every timeline item (`clippy::large_enum_variant`);
        /// `Box` is serde/ts-rs-transparent, so the wire shape and the generated
        /// binding stay `MediaVm | null`.
        media: Option<Box<MediaVm>>,
        /// The *other* members whose latest read receipt sits on this item, as
        /// opaque Matrix user ids (Story 3.9, receipts). Populated from
        /// `EventTimelineItem::read_receipts()` keys with the account's own user id
        /// excluded (never render self as a reader), in the SDK's receipt-map
        /// order. Empty when no other member has read up to here. Only opaque ids
        /// cross IPC — no avatars, receipt event ids, or timestamps (NFR-9, AD-1);
        /// the frontend renders deterministic initials micro-avatars. An own
        /// message with a non-empty `readers` additionally shows a read tick.
        readers: Vec<String>,
    },
    /// An event that could not be decrypted yet (`MsgLikeKind::UnableToDecrypt`).
    /// Renders an explicit honest stub instead of a blank row (Story 3.1). Carries
    /// **only** non-secret render data — a stable opaque render key, the sender
    /// user id, a resolved display name, and the timestamp. NO ciphertext, session
    /// id, or any crypto/key material ever crosses IPC on this VM (NFR-9, AD-1).
    /// When room keys arrive later, the SDK re-maps this item to a
    /// [`TimelineItemVm::Message`] via a `Set` diff — no extra code needed.
    Utd {
        /// Stable opaque render key (the item's `unique_id`).
        key: String,
        /// The sender's Matrix user id (opaque, passed through verbatim).
        sender: String,
        /// The resolved sender display name, or `null` when unavailable.
        sender_display_name: Option<String>,
        /// The event origin timestamp: ms since the Unix epoch (UTC).
        #[ts(type = "number")]
        timestamp: i64,
    },
    /// A message that has been redacted — deleted for everyone (Story 3.8, FR-15).
    /// Renders an explicit honest "Message deleted" stub instead of a blank row or
    /// a silent removal (the same honesty principle as [`TimelineItemVm::Utd`]).
    /// Carries **only** non-secret render data — a stable opaque render key, the
    /// sender user id, a resolved display name, and the timestamp. The redacted
    /// event has no body/content to read, and no tombstone/redaction reason crosses
    /// IPC (NFR-9, AD-1). The SDK turns a live message into this in place via a
    /// `Set` diff, so diff indices stay aligned — keeper never removes or re-indexes
    /// a redacted item (local archive retention is Story 5.2).
    Redacted {
        /// Stable opaque render key (the item's `unique_id`).
        key: String,
        /// The sender's Matrix user id (opaque, passed through verbatim).
        sender: String,
        /// The resolved sender display name, or `null` when unavailable.
        sender_display_name: Option<String>,
        /// The event origin timestamp: ms since the Unix epoch (UTC).
        #[ts(type = "number")]
        timestamp: i64,
    },
    /// Any non-text item (non-text msgtype, state/membership/profile change, or a
    /// virtual date-divider/read-marker item).
    /// Carried only to keep diff indices aligned; the frontend renders nothing.
    Other {
        /// Stable opaque render key (the item's `unique_id`).
        key: String,
    },
}

/// One index-based timeline operation mirroring an eyeball-im `VectorDiff`
/// (AD-8, AD-9, AD-20).
///
/// The SDK `Timeline`'s `subscribe` stream yields a `VectorDiff` sequence;
/// keeper forwards it verbatim as these ops (one VM per SDK item). The frontend
/// applies them to a plain array by index and **never** re-sorts, filters, or
/// re-indexes. Serialized as an internally tagged enum so the frontend can
/// switch on `op`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum TimelineOp {
    /// Full reset — replace the current contents with `items`.
    Reset {
        /// The complete current timeline, in order.
        items: Vec<TimelineItemVm>,
    },
    /// Append `items` to the end, in order.
    Append {
        /// Items to append.
        items: Vec<TimelineItemVm>,
    },
    /// Remove all items.
    Clear,
    /// Insert `item` at the front (index 0).
    PushFront {
        /// The item to prepend.
        item: TimelineItemVm,
    },
    /// Append `item` to the end.
    PushBack {
        /// The item to append.
        item: TimelineItemVm,
    },
    /// Remove the first item.
    PopFront,
    /// Remove the last item.
    PopBack,
    /// Insert `item` at `index`, shifting the tail right.
    Insert {
        /// The insertion index.
        #[ts(type = "number")]
        index: u32,
        /// The item to insert.
        item: TimelineItemVm,
    },
    /// Replace the item at `index` in place.
    Set {
        /// The index to overwrite.
        #[ts(type = "number")]
        index: u32,
        /// The replacement item.
        item: TimelineItemVm,
    },
    /// Remove the item at `index`, shifting the tail left.
    Remove {
        /// The index to remove.
        #[ts(type = "number")]
        index: u32,
    },
    /// Truncate the timeline to `length` items.
    Truncate {
        /// The new length.
        #[ts(type = "number")]
        length: u32,
    },
}

/// A batch of timeline ops delivered over the subscription's `Channel` (AD-8).
///
/// The stream always opens with a batch whose first op is a
/// [`TimelineOp::Reset`] carrying the cached snapshot, then diff batches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TimelineBatch {
    /// The ordered ops to apply, in sequence.
    pub ops: Vec<TimelineOp>,
}

/// One member currently typing in the open room (Story 3.9, typing, AD-14,
/// NFR-9).
///
/// Carries **only** the opaque Matrix `user_id` and a resolved `display_name`
/// (best-effort, `null` when the member can't be resolved) so the typing row can
/// render "<name> is typing…" honestly. No presence, avatars, or crypto material
/// cross IPC on this VM (AD-1). The SDK already filters the account's own user id
/// out of the typing stream, so a typist is always another member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TypistVm {
    /// The typing member's Matrix user id (opaque, passed through verbatim).
    pub user_id: String,
    /// The member's resolved display name for the "… is typing" copy, or `null`
    /// when it can't be resolved (the frontend then falls back to the user id).
    pub display_name: Option<String>,
}

/// A batch delivered over the typing subscription's `Channel` (Story 3.9, AD-8,
/// AD-14).
///
/// The full current set of *other* members typing in the open room — inherently
/// idempotent, safe to re-subscribe. An empty `typists` means nobody is typing
/// (the frontend renders nothing). The stream opens with the current set, then
/// emits on every change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TypingBatch {
    /// The members currently typing (other than the account's own user).
    pub typists: Vec<TypistVm>,
}

/// Whether back-pagination is currently running (Story 3.9, pagination, AD-8).
///
/// A Rust-authoritative projection of the SDK `PaginationStatus`:  `Paginating`
/// while a back-pagination request is in flight (the boundary shows a spinner),
/// `Idle` otherwise. Serializes to its camelCase name. The homeserver-start signal
/// is carried separately on [`PaginationStatusBatch::hit_start`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PaginationState {
    /// A back-pagination request is in flight — the boundary shows a spinner.
    Paginating,
    /// No back-pagination is running.
    Idle,
}

/// A batch delivered over the pagination-status subscription's `Channel` (Story
/// 3.9, AD-8).
///
/// A scalar snapshot of the live back-pagination status, mapped from the SDK
/// `PaginationStatus`: `state` drives the boundary spinner, and `hit_start` is
/// `true` once the homeserver has no older history (the boundary then states the
/// conversation start and no further pagination is attempted). Inherently
/// idempotent — each batch carries the full current status. Older events
/// themselves arrive over the existing timeline diff stream (`PushFront`/`Insert`),
/// never here; this channel carries only the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaginationStatusBatch {
    /// Whether back-pagination is currently in flight.
    pub state: PaginationState,
    /// Whether the homeserver start of the room has been reached (no more older
    /// history). `true` only alongside an `Idle` state.
    pub hit_start: bool,
}

/// The durable login-mechanism discriminant of an account (Story 2.5, AD-17).
///
/// Set once at add time by the authenticating [`AuthProvider`] and persisted in
/// the non-secret `keeper.db` registry row (never in the Keychain session blob,
/// never a secret). Surfaced on [`AccountVm::provider`] so the frontend can key
/// provider-specific UI (e.g. the Beeper coverage disclosure) off a stable tag
/// rather than the resolved homeserver host. Serializes to its lowercase name
/// (`"password" | "oidc" | "beeper"`) — the frontend wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Provider {
    /// A native Matrix password (`m.login.password`) login.
    Password,
    /// An OIDC (OAuth 2.0 / MSC3861) login.
    Oidc,
    /// A Beeper unofficial email-code (JWT) login against `matrix.beeper.com`.
    Beeper,
}

impl Provider {
    /// The lowercase string persisted in the `keeper.db` `provider` column and
    /// serialized over IPC (`"password" | "oidc" | "beeper"`).
    pub fn as_registry_str(&self) -> &'static str {
        match self {
            Provider::Password => "password",
            Provider::Oidc => "oidc",
            Provider::Beeper => "beeper",
        }
    }

    /// Parse a registry `provider` column value back into a [`Provider`], or
    /// `None` for an unrecognized / absent tag (a legacy NULL row).
    pub fn from_registry_str(value: &str) -> Option<Self> {
        match value {
            "password" => Some(Provider::Password),
            "oidc" => Some(Provider::Oidc),
            "beeper" => Some(Provider::Beeper),
            _ => None,
        }
    }
}

/// The kind of a network egress destination (Story 11.2, NFR-11; Story 23.7).
///
/// Classifies each entry in the [`EgressEndpointVm`] list the Settings → About
/// surface renders so the frontend can label it honestly without re-deriving the
/// classification. `Homeserver` is an account's Matrix homeserver; `Beeper` is the
/// `api.beeper.com` login/service endpoint present exactly when a Beeper account
/// exists; `GitRemote` is the host of one folder-sync profile's remote repository;
/// `Update` is the signed-update endpoint the app checks. Serializes to
/// `"homeserver" | "beeper" | "gitRemote" | "update"`.
///
/// The container rename is `camelCase` rather than `lowercase` (Story 23.7) so a
/// multi-word variant reads as `gitRemote` on the wire, matching every other
/// multi-word IPC enum in this module (e.g. [`IpcErrorCode`]). The three
/// pre-existing single-word variants serialize identically under either rule, so
/// this changed no existing wire value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum EgressKind {
    /// A Matrix homeserver an account is signed into.
    Homeserver,
    /// Beeper's `api.beeper.com` service endpoint (present iff a Beeper account exists).
    Beeper,
    /// The remote *host* of a folder-sync profile's repository (Story 23.7).
    ///
    /// The host alone, never the full remote URL: a remote URL carries a path, can
    /// carry a username, and — in a profile whose token was stored in the URL
    /// instead of the keychain — a credential. An egress disclosure answers "where
    /// do bytes go", and the host is the whole of that answer; the rest is only
    /// material to leak onto a screen the user may be sharing.
    GitRemote,
    /// The signed auto-update endpoint (`plugins.updater.endpoints`).
    Update,
}

/// One network destination keeper contacts, derived from live app state (Story
/// 11.2, NFR-11, UX-DR17).
///
/// The Settings → About surface renders the full set of these — computed by
/// `egress::compute_egress` from the accounts registry (each homeserver, plus
/// `api.beeper.com` when a Beeper account exists), the live folder-sync profile
/// set (each distinct remote host, Story 23.7) and the shared update endpoint —
/// so keeper's egress claim is verifiable rather than asserted. Never fabricated,
/// never stale-cached: both the registry rows and the profile rows are read on
/// each open, from the same stores the session-restore and sync paths use, so
/// adding or removing an account or a profile changes this list immediately.
/// `url` is the destination shown; `label` is a short honest caption; `kind`
/// classifies it for rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EgressEndpointVm {
    /// The destination shown: a homeserver URL (the raw stored string when it is
    /// unparseable), a fixed service endpoint, or — for [`EgressKind::GitRemote`] —
    /// the bare host of a sync remote, never its full URL.
    pub url: String,
    /// The classification of this destination.
    pub kind: EgressKind,
    /// A short, honest human-readable caption for the destination.
    pub label: String,
}

/// The durable per-Chat / per-Network mute intent stamped on a room row (Story
/// 10.2, FR-52, AD-18).
///
/// A pure render signal computed at inbox emit time from the room's synced Matrix
/// push-rule mode plus the keeper-local muted-Network set: `Muted` when the room
/// mode is `RoomNotificationMode::Mute` **or** the room's bridged Network is in the
/// muted set; `MentionOnly` when the mode is `MentionsAndKeywordsOnly`; otherwise
/// `None`. It reflects *durable* mute intent only — it deliberately does **not**
/// reflect the global Do-Not-Disturb switch (shown once in the footer, never stamped
/// per row) and never gates unread. Fail-open: any read error resolves to `None`.
/// Serializes to `"none" | "muted" | "mention_only"` (the frontend wire contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MuteState {
    /// No durable mute intent — the row notifies per the 10.1 pipeline.
    None,
    /// Muted: the room's push-rule mode is `Mute`, or its Network is muted.
    Muted,
    /// Mention-only: the room's push-rule mode is `MentionsAndKeywordsOnly`.
    MentionOnly,
}

/// The per-Chat notification mode the IPC boundary sets/reads (Story 10.2, AD-18),
/// a one-to-one mirror of matrix-sdk `RoomNotificationMode` mapped onto keeper's
/// wire vocabulary.
///
/// `All` clears any per-Chat rule back to "notify for all messages" (the effective
/// "unmute"); `MentionOnly` notifies only for mentions/keywords/replies; `Mute`
/// silences the Chat entirely. Persisted as a synced Matrix push rule via
/// `client.notification_settings().set_room_notification_mode(...)`, so it survives
/// restart and syncs across devices. Serializes to `"all" | "mention_only" | "mute"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChatNotifyMode {
    /// Notify for every message (clears any per-Chat rule — the "unmute" target).
    All,
    /// Notify for mentions / keywords / replies only.
    MentionOnly,
    /// Silence the Chat entirely.
    Mute,
}

/// The dock-badge mode the IPC boundary sets/reads (Story 10.3, FR-53, AD-18).
///
/// Drives the Rust-computed dock badge from the full cross-account unread/mention
/// state so the count stays correct while the window is hidden (the badge is never
/// computed in the webview). `All` shows the count of unread rooms; `Mentions` shows
/// the total unread-mention count; `Off` shows no badge. A zero total clears the
/// badge in every mode. Persisted in `keeper.db` `settings` under
/// `notify.dock_badge_mode`; default `All`. Serializes to `"all" | "mentions" | "off"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DockBadgeMode {
    /// Badge the count of unread rooms across all accounts.
    All,
    /// Badge the total unread-mention count across all accounts.
    Mentions,
    /// No dock badge.
    Off,
}

impl DockBadgeMode {
    /// The stable registry string this mode persists as under `notify.dock_badge_mode`.
    pub fn as_registry_str(self) -> &'static str {
        match self {
            DockBadgeMode::All => "all",
            DockBadgeMode::Mentions => "mentions",
            DockBadgeMode::Off => "off",
        }
    }

    /// Parse a persisted registry string back into a mode; an unknown/absent value
    /// resolves to the default [`DockBadgeMode::All`] (honest default, never fails).
    pub fn from_registry_str(value: &str) -> Self {
        match value {
            "mentions" => DockBadgeMode::Mentions,
            "off" => DockBadgeMode::Off,
            // "all" and any unrecognized value default to All.
            _ => DockBadgeMode::All,
        }
    }
}

/// The OS notification-permission state the iOS Settings surface reads (Story 14.3).
///
/// Read in Rust from `tauri-plugin-notification`'s `permission_state()` and mapped to
/// this typed enum so the notification-permission concern stays in one place and is
/// testable. `Granted`/`Denied` mirror the plugin's states; every other plugin state
/// (prompt / prompt-with-rationale), an unset app handle, or a read error maps to
/// `Unknown` — the UI then hides the persistent "off" surface rather than guessing.
/// Never drives a re-prompt (UX-DR28). Serializes to `"granted" | "denied" | "unknown"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum NotificationPermission {
    /// OS notification permission is granted — notifications (and the app-icon badge) post.
    Granted,
    /// OS notification permission is denied — the persistent iOS "off" surface renders.
    Denied,
    /// Prompt / not-yet-decided / unreadable — the UI shows no persistent "off" state.
    Unknown,
}

/// The last phone-stack navigation level, held in Rust so it survives a jettisoned
/// WKWebView web-content process (Story 14.4, tauri#14371).
///
/// Nav *selection* only — never message/room data (AD-8: the streams re-deliver a
/// full snapshot on re-subscribe after any reload). Reported by the iOS shell on the
/// reduced tier whenever a Chat is open (`detail_open` marks the level-2 Detail);
/// cleared on return to the Inbox. Ephemeral process state, never persisted: a true
/// app kill restarts Rust fresh, so a cold launch honestly starts at the Inbox.
/// Deliberately independent from 14.3's `NotifyConfig.active_room`
/// (notification-suppression state) — the two concerns never share a slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NavState {
    /// The opaque keeper account id of the open Chat.
    pub account_id: String,
    /// The Matrix room id of the open Chat.
    pub room_id: String,
    /// Whether the level-2 Detail panel was open on top of the Room.
    pub detail_open: bool,
}

/// The click-through target carried by every native notification (Story 10.4, FR-51).
///
/// Attached at the notification dispatch site so a click can land the user in the
/// right place. Under the **Option B** MVP scope (coordinator decision 2026-07-06) the
/// kept `tauri-plugin-notification` desktop backend has no per-notification click
/// callback, so the payload is recorded app-side as the "last notification target" at
/// dispatch and drives only a **coarse** view landing on app activation — Message
/// targets land on the Inbox, Bridge targets on the Bridges view. The full
/// `(account_id, room_id, event_id)` payload ships now even though MVP click handling is
/// coarse; exact-message / exact-re-login deep landing via a click-capable backend is
/// deferred to Epic 11 (see `deferred-work.md`). This is NEVER exact-message routing.
///
/// Serialized as an internally tagged enum so the frontend can switch on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum NotifyTarget {
    /// A message notification: the exact `(account_id, room_id, event_id)` that raised it.
    /// Coarse landing routes to the Inbox view; exact Chat/Account/message landing is
    /// deferred to Epic 11.
    Message {
        /// The opaque keeper account id the message belongs to.
        account_id: String,
        /// The Matrix room id the message was sent in.
        room_id: String,
        /// The message's Matrix event id.
        event_id: String,
    },
    /// A bridge-health notification: the `(account_id, network_id)` of the disconnected
    /// session. Coarse landing routes to the Bridges view; the persistent Story 6.5
    /// surfaces route the user into the exact re-login. Exact re-login deep-landing is
    /// deferred to Epic 11.
    Bridge {
        /// The opaque keeper account id owning the bridge session.
        account_id: String,
        /// The stable machine `network_id` (the `protocol.id`) of the bridge.
        network_id: String,
    },
    /// No specific target (a notification with nothing to land on). Coarse landing is a
    /// no-op — the window is still summoned+focused by the OS default activation.
    None,
}

/// Non-secret account registry projection returned to the frontend on a
/// successful login (FR-1, NFR-9).
///
/// Carries **only** the opaque keeper account id, the Matrix user id, the
/// resolved homeserver URL, the per-account hue index, and the durable
/// login-mechanism [`Provider`] tag. Tokens, refresh tokens, device/crypto keys,
/// and any `MatrixSession` material never appear here — they live only in the
/// macOS Keychain and never cross IPC back to TypeScript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountVm {
    /// Opaque keeper-generated account id (a ULID). Used in paths, rows, VMs,
    /// and Keychain entries.
    pub account_id: String,
    /// The Matrix user id this account signed in as (e.g. `@alice:example.org`).
    pub user_id: String,
    /// The resolved homeserver base URL (after well-known discovery).
    pub homeserver_url: String,
    /// The account's hue index (0–7) on the 8-hue wheel, assigned at add time
    /// and persisted in `keeper.db`. The frontend maps it to a CSS hue rendered
    /// as a 3 px chat-row edge bar and (later) a switcher dot.
    #[ts(type = "number")]
    pub hue_index: u8,
    /// The durable login-mechanism tag, stamped at add time and persisted in
    /// `keeper.db`. Drives provider-specific UI (e.g. Beeper coverage) off a
    /// stable discriminant rather than the resolved homeserver host.
    pub provider: Provider,
}

/// A single merged-inbox room row, attributed to its owning account (AD-20).
///
/// The unified inbox merges every active account's room-list stream into one
/// recency-ordered list. Each row is a [`RoomVm`]'s render data plus the opaque
/// keeper `accountId` it belongs to and that account's persisted `hueIndex`
/// (0–7). Carries **only** non-secret render data — no tokens, session material,
/// or event ids cross IPC on this VM. The frontend renders the hue as a 3 px
/// left edge bar and opens the row's timeline on the row's `accountId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InboxRoomVm {
    /// Opaque keeper account id this room belongs to. Drives timeline/send.
    pub account_id: String,
    /// The account's hue index (0–7) for the row's edge bar.
    #[ts(type = "number")]
    pub hue_index: u8,
    /// Opaque Matrix room id (passed through verbatim as a string).
    pub room_id: String,
    /// The SDK-computed room display name.
    pub display_name: String,
    /// Plain-text preview of the latest `m.room.message`, or `null`.
    pub last_message: Option<String>,
    /// Latest-event timestamp: ms since the Unix epoch (UTC), or `null`.
    #[ts(type = "number | null")]
    pub timestamp: Option<i64>,
    /// Optional room avatar URL (an `mxc://` URI), or `null`.
    pub avatar_url: Option<String>,
    /// Authoritative unread flag: `true` when the room has unread messages,
    /// unread mentions, or the manual `m.marked_unread` flag set (AD-20). The
    /// frontend renders this directly (bold name + dot/badge) and never
    /// re-derives it from events.
    pub is_unread: bool,
    /// Count of unread mentions (client-side, precise for E2EE). Drives the
    /// filled primary mention badge; a value of 0 shows a plain dot when
    /// `is_unread` is otherwise set.
    #[ts(type = "number")]
    pub mention_count: u32,
    /// Authoritative archive flag: `true` when the room carries the Matrix
    /// low-priority tag (`m.lowpriority`) (Story 4.2, AD-20). The merge
    /// partitions on this to place the row in the Archive window unless it is
    /// unread (auto-return is a pure view rule); the frontend never re-derives it.
    pub is_archived: bool,
    /// Authoritative favourite flag: `true` when the room carries the Matrix
    /// favourite tag (`m.favourite`) (Story 4.4, AD-20). A *notable* tag, so a
    /// change re-emits the room-list stream live and syncs cross-client (SDK-
    /// sourced, copied through like `is_archived` — not merger-owned like
    /// `is_pinned`). The merge partitions on this to place the row in the
    /// Favorites window (removed from Inbox/Archive), behind Pins in precedence;
    /// the frontend renders this directly (Favorite/Unfavorite gating) and never
    /// re-derives it.
    pub is_favourite: bool,
    /// Authoritative pin flag: `true` when the room is pinned in keeper-local
    /// state (Story 4.3, AD-20). Pins are keeper-local (no Matrix tag), owned by
    /// the merger, which places a pinned room in the Pins window (removed from
    /// Inbox/Archive). The frontend renders this directly (Pin/Unpin gating) and
    /// never re-derives it.
    pub is_pinned: bool,
    /// The bridged-Network label for this row (Story 4.6, FR-24), copied straight
    /// through from [`RoomVm::network`]. `None` for a native Matrix room (no badge).
    /// Drives the avatar Network badge and the ephemeral Network filter's retain;
    /// the frontend renders the badge directly and never re-derives or re-filters it.
    pub network: Option<String>,
    /// The room's stable bridge `network_id` — the machine `protocol.id` (Story 6.5,
    /// FR-28), copied straight through from [`RoomVm::network_id`]. Distinct from the
    /// display `network` label: this is the join key the frontend matches against an
    /// unhealthy bridge session on `(account_id, network_id)` to show the affected-row
    /// health dot and the in-conversation re-link banner. `None` for a native Matrix
    /// room. Never re-derived on the frontend — it mirrors the Rust stream.
    pub network_id: Option<String>,
    /// The durable per-Chat / per-Network mute intent (Story 10.2, FR-52), copied
    /// straight through from [`RoomVm::mute_state`]. Drives the row's mute glyph
    /// (`Muted` → bell-off, `MentionOnly` → at-sign); `None` shows no glyph. Reflects
    /// durable mute only — never the global DND switch — and never gates unread.
    pub mute_state: MuteState,
}

/// One index-based merged-inbox operation mirroring an eyeball-im `VectorDiff`
/// (AD-8, AD-20).
///
/// The merged inbox is computed in `keeper-core::inbox`; keeper streams its
/// recency-ordered result as these ops. The frontend applies them to a plain
/// array by index and **never** re-sorts. Serialized as an internally tagged
/// enum so the frontend can switch on `op`. The variants mirror [`RoomListOp`]
/// so the shared frontend diff reducer applies both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum InboxOp {
    /// Full reset — replace the current contents with `rooms`.
    Reset {
        /// The complete current merged window, in recency order.
        rooms: Vec<InboxRoomVm>,
    },
    /// Append `rooms` to the end, in order.
    Append {
        /// Rooms to append.
        rooms: Vec<InboxRoomVm>,
    },
    /// Remove all rooms.
    Clear,
    /// Insert `room` at `index`, shifting the tail right.
    Insert {
        /// The insertion index.
        #[ts(type = "number")]
        index: u32,
        /// The room to insert.
        room: InboxRoomVm,
    },
    /// Replace the room at `index` in place.
    Set {
        /// The index to overwrite.
        #[ts(type = "number")]
        index: u32,
        /// The replacement room.
        room: InboxRoomVm,
    },
    /// Remove the room at `index`, shifting the tail left.
    Remove {
        /// The index to remove.
        #[ts(type = "number")]
        index: u32,
    },
}

/// A batch of merged-inbox ops delivered over the subscription's `Channel`
/// (AD-8, AD-20).
///
/// The stream always opens with a batch whose first op is an [`InboxOp::Reset`]
/// carrying the current merged window, then further batches as accounts sync or
/// are added/removed. The merge is partitioned into an Inbox and an Archive
/// window (Story 4.2), and `total` is the length of *this* window's partition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InboxBatch {
    /// The ordered ops to apply, in sequence.
    pub ops: Vec<InboxOp>,
    /// The number of rooms in this streamed window (the partition's own length),
    /// when known. Since Story 4.2 the merge is split into an Inbox and an
    /// Archive window, so this is per-window, not a cross-account server total.
    #[ts(type = "number | null")]
    pub total: Option<u32>,
}

/// The single error envelope every fallible command rejects with (AD-8, AD-21).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IpcError {
    /// Stable machine-readable error code.
    pub code: IpcErrorCode,
    /// Human-readable message (never contains secrets or plaintext).
    pub message: String,
    /// Opaque keeper account id this error pertains to, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Whether retrying the same operation may succeed.
    pub retriable: bool,
}

/// A single demo item carried in snapshot/diff batches. Placeholder payload
/// that exercises the snapshot-then-diff channel pattern end-to-end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DemoItem {
    /// Stable item id.
    pub id: String,
    /// Display label.
    pub label: String,
}

/// A batch delivered over a demo subscription's `Channel` (AD-8).
///
/// The stream always opens with a [`DemoBatch::Snapshot`] (full reset) before
/// any [`DemoBatch::Diff`]. Serialized as an internally tagged enum so the
/// frontend can switch on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub enum DemoBatch {
    /// Full state reset — the complete current set of items.
    Snapshot {
        /// Every item currently present.
        items: Vec<DemoItem>,
    },
    /// Incremental change relative to the last delivered state.
    Diff {
        /// Items added or updated in this batch.
        added: Vec<DemoItem>,
        /// Ids removed in this batch.
        removed: Vec<String>,
    },
}

/// The data-driven risk tier of a bridged Network (Story 6.1, Epic 6 addendum
/// §2).
///
/// Sourced from `risk-tiers.json` — never hardcoded in TypeScript. Only the four
/// *surfaced* tiers cross IPC: the out-of-scope tier stays in the data file for
/// completeness but is excluded from the catalog and has no enum variant.
/// Serializes to its camelCase name — the frontend wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RiskTier {
    /// Low risk — recommended by default, no warning beyond the label.
    Low,
    /// Maintenance-heavy — default-on with clear disclosure; expect session churn.
    Maintenance,
    /// Volatile / opt-in — connecting may violate ToS and risks a ban; gated by an
    /// acknowledgment dialog.
    Volatile,
    /// Conditional / advanced — e.g. macOS-only iMessage; gated by an
    /// acknowledgment dialog.
    Conditional,
}

/// The visual badge style for a risk tier (Story 6.1, Epic 6 addendum §2).
///
/// Sourced from the `badge` field of `risk-tiers.json` — the tier→badge mapping is
/// data, never hardcoded in TypeScript. The card maps this to the shadcn `Badge`
/// variant plus the `--bridge-*` colour tokens: `secondary` (Low), `outlineDegraded`
/// (Maintenance, amber), `filledDisconnected` (Volatile, red), `outline`
/// (Conditional). Serializes to its camelCase name — the frontend wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BadgeStyle {
    /// A plain secondary badge (Low risk).
    Secondary,
    /// An outlined badge tinted with the degraded (amber) token (Maintenance-heavy).
    OutlineDegraded,
    /// A filled badge tinted with the disconnected (red) token (Volatile / opt-in).
    FilledDisconnected,
    /// A plain outlined badge (Conditional / advanced).
    Outline,
}

/// One connectable bridged Network in the data-driven Bridges catalog (Story 6.1,
/// FR-42, Epic 6 addendum §2).
///
/// A pure projection of a *surfaced* tier's network entry from `risk-tiers.json`:
/// the stable `network_id`, display `name`, `glyph` initials, the resolved
/// [`RiskTier`], its display `tier_label`, the [`BadgeStyle`], whether connecting
/// `requires_ack`, and the acknowledgment `ack_copy` (present iff `requires_ack`).
/// The catalog is account-agnostic — the frontend keys a card per Network × Account
/// — and carries no health, session, or bridge state (health is Story 6.5; discovery
/// is Story 6.2). All risk/badge/ack copy is data, never hardcoded in TypeScript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BridgeNetworkVm {
    /// The stable network identifier (e.g. `"whatsapp"`), from the data file.
    pub network_id: String,
    /// The Network's display name (e.g. `"WhatsApp"`).
    pub name: String,
    /// The glyph initials rendered in the card avatar (e.g. `"WA"`).
    pub glyph: String,
    /// The resolved risk tier.
    pub tier: RiskTier,
    /// The tier's display label (e.g. `"Maintenance-heavy"`), from the data file.
    pub tier_label: String,
    /// The badge style driving the card's risk-tier Badge.
    pub badge_style: BadgeStyle,
    /// Whether connecting this Network requires an explicit acknowledgment (the
    /// volatile / conditional gate).
    pub requires_ack: bool,
    /// The acknowledgment copy shown in the connect gate, present iff
    /// `requires_ack`, else `null`. Sourced from the tier's `acknowledgment` field.
    pub ack_copy: Option<String>,
}

/// One per-Network coupling caveat — a behavior that connecting a Network couples
/// in (Story 8.2, FR-44). A pure read-only projection of `coupling-caveats.json`:
/// the stable `network_id` this caveat applies to, the human-readable `text` shown
/// inline at the per-Chat Incognito toggle, and `applies_to`, a machine tag naming
/// the coupled surface (e.g. `"read-receipts"`). All caveat copy is data — none is
/// authored in TypeScript. Joined to the open room's Network by `network_id` on the
/// frontend; an uncoupled or native (null-network) room shows no caveat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CouplingCaveatVm {
    /// The stable network identifier this caveat applies to (e.g. `"whatsapp"`).
    pub network_id: String,
    /// The human-readable caveat text, from the data file.
    pub text: String,
    /// A machine tag naming the coupled surface (e.g. `"read-receipts"`).
    pub applies_to: String,
}

/// The discovered setup/login status of a bridged Network on an Account's
/// homeserver (Story 6.2, FR-25, AD-16).
///
/// Derived once, per Account, from the merged three-source discovery pass
/// (`thirdparty/protocols` + known-bot MXID probe + bot-DM/portal room scan) by
/// the pure `merge_discovery` function. It is the *setup* state, not live
/// connection health — live health (degraded / disconnected, 60 s surfacing) is
/// Story 6.5 and stays a separate placeholder. Serializes to its camelCase name —
/// the frontend wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BridgeStatus {
    /// A portal room (`m.bridge` with the Network's `protocol.id`) exists — the
    /// Network is bridged and logged in.
    LoggedIn,
    /// A bot management DM with a known bot exists but no portal room — the bridge
    /// is present but the user has not logged into the Network yet.
    NotLoggedIn,
    /// The Network is present only via the homeserver's `thirdparty/protocols`
    /// list or a resolving known-bot MXID — configured on the server, no DM/portal
    /// yet.
    Configured,
}

/// One discovered bridged Network for an Account (Story 6.2, FR-25, AD-16).
///
/// Carries only the stable `network_id` (joined to the 6.1 [`BridgeNetworkVm`]
/// catalog on the frontend for glyph/name/tier badge/ack copy) and the derived
/// [`BridgeStatus`]. Only catalog-gated Networks appear here — a discovered
/// protocol with no catalog entry is logged and dropped, never surfaced.
/// Serializes camelCase — the frontend wire contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiscoveredBridgeVm {
    /// The stable network identifier (e.g. `"whatsapp"`), joined to the 6.1
    /// catalog by the frontend for presentation.
    pub network_id: String,
    /// The Network's derived setup/login status.
    pub status: BridgeStatus,
}

/// The result of a per-Account bridge discovery pass (Story 6.2, FR-25, AD-16).
///
/// `homeserver` is the account's server name (e.g. `"example.org"`), used verbatim
/// in the empty-state copy ("No bridges found on {homeserver}."). `networks` are the
/// catalog-gated discovered Networks with their derived statuses; an empty list is
/// the honest "no bridges found" state, not an error. Carries no bot MXID, token, or
/// session material — only non-secret network ids and statuses cross IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BridgeDiscoveryVm {
    /// The account's homeserver server name, for the empty-state copy.
    pub homeserver: String,
    /// The catalog-gated discovered Networks with their derived statuses.
    pub networks: Vec<DiscoveredBridgeVm>,
}

/// The data-driven new-chat resolve capability for one Network (Story 6.6, FR-32).
///
/// A pure projection of `resolve-support.json` (override-or-default) for a selected
/// network: whether starting a chat by resolving an identifier is `supported`, the
/// identifier-field `identifier_hint`, and its `placeholder`. `supported: false`
/// disables the identifier field and shows the "not supported on {Network}" copy
/// **before** any network I/O. All capability/hint copy is data, never hardcoded in
/// TypeScript or Rust. Carries no session material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResolveSupportVm {
    /// The stable network identifier this capability was resolved for.
    pub network_id: String,
    /// Whether resolving an identifier to start a chat is supported here.
    pub supported: bool,
    /// The identifier-field hint copy (also carries the "not supported" copy when
    /// `supported` is `false`).
    pub identifier_hint: String,
    /// The identifier-field placeholder copy (empty for an unsupported network).
    pub placeholder: String,
}

/// The result of resolving a new-chat identifier through the bridge (Story 6.6,
/// FR-32).
///
/// Carries only the non-secret portal `room_id` the frontend opens verbatim via
/// `roomsStore.selectRoom`. The account's Matrix access token is used only as an HTTP
/// Bearer header inside the provisioning transport and **never** appears here — no
/// token, cookie, or session material crosses IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewChatResolutionVm {
    /// The resolved portal room id to open (opened verbatim, never inferred).
    pub room_id: String,
}

/// The live connection health of a bridged session (Story 6.5, FR-28, NFR-6,
/// UX-DR8/UX-DR11).
///
/// A pure, per-session state — keyed by `(account_id, network_id)` — driven by the
/// bridge's management-room notices (real-time via the running sync) with a bounded
/// bot-ping liveness fallback. Distinct from the *setup* [`BridgeStatus`] (which is a
/// one-shot discovery result): this is the live signal that a logged-in session went
/// silent (device unlinked, token expired) or recovered. Serializes to its camelCase
/// name — the frontend wire contract. The frontend renders the dot / state-word / red
/// edge / roll-up / banner as pure projections of this one enum and never re-derives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BridgeHealth {
    /// The session is connected — the bridge is delivering. Renders "Connected" + a
    /// healthy dot; no banner.
    Healthy,
    /// The session is impaired but not dead — the bridge reported a transient
    /// reconnect. Renders "Action needed" + an amber dot.
    Degraded,
    /// The session is dead — the bridge posted a logged-out notice or the liveness
    /// tick timed out past the debounce threshold. Renders "Disconnected" + a red dot,
    /// a red left edge, an affected-row dot, and the non-dismissible re-link banner.
    Disconnected,
}

/// One bridged session's live health, keyed by `(account_id, network_id)` (Story
/// 6.5, FR-28).
///
/// Carries **only** non-secret render data: the opaque keeper `account_id`, the stable
/// machine `network_id` (the `protocol.id`, the row/conversation join key — never the
/// display label), the resolved display `network_name` for banner/card copy, the live
/// [`BridgeHealth`], the `last_checked_ms` timestamp (ms since the Unix epoch), and an
/// optional `detail` carrying the bot's verbatim reason (trimmed, length-capped, no
/// tokens or session material). Never a bot MXID, token, or session material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BridgeSessionHealthVm {
    /// Opaque keeper account id this session belongs to (part of the join key).
    pub account_id: String,
    /// The stable machine `network_id` (`protocol.id`), the room/conversation join
    /// key — never the display label.
    pub network_id: String,
    /// The Network's display name for the card / banner copy (e.g. `"WhatsApp"`).
    pub network_name: String,
    /// The live connection health.
    pub health: BridgeHealth,
    /// When the session was last checked: ms since the Unix epoch (UTC).
    #[ts(type = "number")]
    pub last_checked_ms: i64,
    /// The bot's verbatim reason (trimmed, length-capped, no tokens/session material),
    /// or `null` — populated on a disconnected/degraded notice, cleared on recovery.
    #[serde(default)]
    pub detail: Option<String>,
}

/// The full current bridge-session health snapshot, streamed as a whole-snapshot
/// batch over the health subscription's `Channel` (Story 6.5, FR-28, AD-8).
///
/// Sessions are few, so there is no diff protocol: each batch carries the complete
/// set of monitored (logged-in) sessions across every account, and the frontend
/// replaces its keyed map wholesale. Emitted on subscribe (the bootstrap snapshot),
/// then **only on a real per-session state change** (diffed) — no periodic re-emit
/// noise, matching the `NetworksSnapshot` cadence contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BridgeHealthSnapshot {
    /// Every monitored (logged-in) session's live health, across all accounts.
    pub sessions: Vec<BridgeSessionHealthVm>,
}

/// The phase of a native bridge login flow (Story 6.3, FR-26, AD-16).
///
/// A transport-agnostic projection of the bridgev2 provisioning login state
/// machine, rendered as a distinct native stepper state. The frontend switches on
/// this phase; the same set must render identically whichever [`BridgeTransport`]
/// (provisioning today, bot-driver in 6.4) powered the login. Serializes to its
/// camelCase name — the frontend wire contract.
///
/// [`BridgeTransport`]: crate::bridges::transport::BridgeTransport
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BridgeLoginPhase {
    /// The bridge exposes more than one login flow — the user must pick one before
    /// the login can start. `flows` carries the choices.
    ChoosingMethod,
    /// The login is in flight and there is nothing yet for the user to do (a step
    /// is being started or a `display_and_wait` with no visual is long-polling).
    Waiting,
    /// A QR code is displayed for the user to scan; `qrSvg` carries the pre-rendered
    /// SVG. A fresh QR while already in this phase sets `qrRefreshed`.
    Qr,
    /// The bridge asked for typed input (a phone number, a 2FA code, a password, …);
    /// `fields` carries the non-secret field descriptors to render.
    CodeEntry,
    /// The login completed — the Network is linked. Terminal.
    Success,
    /// The login failed. `error` carries the bridge's own message verbatim (or
    /// keeper's honest reason for an unsupported step / unreachable API). Terminal
    /// but retriable — the stepper offers Retry.
    Failure,
}

/// One labeled input field the bridge asked for during a code-entry login step
/// (Story 6.3, FR-26).
///
/// A non-secret projection of a bridgev2 `user_input` field descriptor: the field
/// `id` the submit body is keyed by, its provisioning `field_type` (so the Sheet
/// can pick an input treatment — a segmented code input, a masked password, …), a
/// human `name`/`description`, an optional client-side validation `pattern`, and an
/// optional prefilled `default_value`. NO entered value or secret ever rides on
/// this VM — values travel only inside a [`BridgeLoginInput::Fields`] submit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoginFieldVm {
    /// The field id the submit body is keyed by (opaque, passed through verbatim).
    pub id: String,
    /// The provisioning field type (e.g. `"phone_number"`, `"2fa_code"`,
    /// `"password"`, `"token"`, `"username"`), driving the input treatment.
    pub field_type: String,
    /// The human-readable field label (e.g. `"Phone number"`).
    pub name: String,
    /// An optional longer description / helper text, or `null`.
    pub description: Option<String>,
    /// An optional regex the entered value must match before submit (client-side
    /// validated), or `null`.
    pub pattern: Option<String>,
    /// An optional prefilled default value (non-secret), or `null`.
    pub default_value: Option<String>,
}

/// One selectable login method the bridge offers (Story 6.3, FR-26).
///
/// A non-secret projection of a bridgev2 login flow descriptor: the stable `id`
/// used to start the flow and a human `name`/`description` for the RadioGroup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoginFlowVm {
    /// The stable flow id used to start this login method (opaque, verbatim).
    pub id: String,
    /// The flow's human-readable name (e.g. `"QR code"`).
    pub name: String,
    /// An optional longer description of the method, or `null`.
    pub description: Option<String>,
}

/// A snapshot of a native bridge login flow, streamed over the login `Channel`
/// (Story 6.3, FR-26, AD-16, NFR secret containment).
///
/// The single view model the webview renders for the whole login, mirroring
/// [`VerificationFlowVm`]'s phase-plus-optional-payload shape. Carries **only**
/// non-secret render data: the `network_id` being linked, the current
/// [`BridgeLoginPhase`], a per-phase `instruction` line, a pre-rendered `qr_svg`
/// (QR phase), the `qr_refreshed` flag (a fresh QR during an active QR phase), the
/// `fields` to render (code-entry phase), the `flows` to pick from (choosing-method
/// phase), and the bridge's verbatim `error` (failure phase). The account's Matrix
/// access token is used only as an HTTP Bearer header inside the transport and
/// **never** appears here — no token, cookie, or session material crosses IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BridgeLoginVm {
    /// The stable network id being linked (e.g. `"whatsapp"`), joined to the 6.1
    /// catalog by the frontend for glyph/name.
    pub network_id: String,
    /// The current login phase.
    pub phase: BridgeLoginPhase,
    /// A per-phase instruction line (e.g. "Scan this QR with WhatsApp on your
    /// phone."), or `null`.
    pub instruction: Option<String>,
    /// The pre-rendered QR-code SVG string, present in the `Qr` phase, else `null`.
    pub qr_svg: Option<String>,
    /// `true` when a fresh QR replaced an earlier one during an active `Qr` phase
    /// (drives the subtle "QR refreshed" note); `false` otherwise.
    pub qr_refreshed: bool,
    /// The non-secret field descriptors to render, populated in the `CodeEntry`
    /// phase (empty otherwise).
    pub fields: Vec<LoginFieldVm>,
    /// The selectable login methods, populated in the `ChoosingMethod` phase (empty
    /// otherwise).
    pub flows: Vec<LoginFlowVm>,
    /// The bridge's verbatim error message (or keeper's honest reason), present in
    /// the `Failure` phase, else `null`.
    pub error: Option<String>,
}

/// User input submitted into a running bridge login (Story 6.3, FR-26).
///
/// A deserialize-in input VM pushed into the driver by `bridge_login_submit`: a
/// flow choice (from the `ChoosingMethod` phase) or a map of field id → entered
/// value (from the `CodeEntry` phase). Entered values are carried straight into the
/// transport's submit body and never logged. Serialized as an internally tagged
/// enum so the frontend can switch on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum BridgeLoginInput {
    /// The user picked a login flow in the `ChoosingMethod` phase.
    ChooseFlow {
        /// The chosen flow id (matches a [`LoginFlowVm::id`]).
        flow_id: String,
    },
    /// The user submitted the code-entry fields: a map of field id → entered value.
    Fields {
        /// The entered values, keyed by [`LoginFieldVm::id`].
        values: std::collections::BTreeMap<String, String>,
    },
}

/// The phase of a `bbctl` self-hosted-bridge run (Story 6.7, FR-29).
///
/// A log-free projection of the `bbctl register`/`run` progression, rendered as a
/// distinct stepper state. The frontend switches on this phase; only recognized
/// prose markers ever produce a phase (unrecognized `bbctl` output is dropped —
/// there is no path from a raw log line to the UI). `run` is launch-and-leave: on
/// the started marker the run resolves at [`BbctlPhase::Success`] leaving the
/// daemon alive and unsupervised (v1.x — no restart policy, no log viewer).
/// Serializes to its camelCase name — the frontend wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BbctlPhase {
    /// keeper is checking whether the `bbctl` sidecar is available.
    Checking,
    /// `bbctl register` is running (registering the self-hosted bridge appservice).
    Registering,
    /// `bbctl run` is starting the bridge daemon.
    Starting,
    /// The bridge daemon is coming up (post-start, pre-ready markers).
    Running,
    /// The bridge started successfully — it now surfaces through the existing
    /// discovery + health machinery. Terminal.
    Success,
    /// The run failed. `error` carries `bbctl`'s own message verbatim (or keeper's
    /// honest reason for an absent sidecar / non-Beeper gate). Terminal but
    /// retriable — the stepper offers Retry.
    Failure,
}

/// The `bbctl` self-host capability for the "Run your own bridge" surface (Story
/// 6.7, FR-29).
///
/// A one-shot projection of the embedded `bbctl.json` plus the live sidecar
/// availability probe: whether the `bbctl` binary can be resolved (`available`), the
/// guided-install instructions to render when it cannot, and the self-hostable
/// networks offered in the picker. Carries **only** non-secret static data — no
/// token, session, or process material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BbctlAvailabilityVm {
    /// Whether the `bbctl` sidecar resolved on this host/build. `false` renders the
    /// guided-install branch and everything else in keeper keeps working.
    pub available: bool,
    /// The guided-install instructions (rendered when `available` is `false`).
    pub install: BbctlInstallVm,
    /// The self-hostable networks offered in the run picker (supported only).
    pub networks: Vec<BbctlNetworkVm>,
}

/// The guided-install block of the bbctl availability VM (Story 6.7): ordered human
/// `steps` and a `docs_url` to the Beeper self-host documentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BbctlInstallVm {
    /// The ordered install steps (rendered as a numbered list — may repeat prose,
    /// so the frontend keys them by index).
    pub steps: Vec<String>,
    /// The Beeper self-host docs URL.
    pub docs_url: String,
}

/// One self-hostable network offered in the run-your-own-bridge picker (Story 6.7).
///
/// A non-secret projection of a supported `bbctl.json` network: the keeper
/// `network_id` (joined to the 6.1 catalog for glyph/badge), a display `name`, and
/// the `bbctl_name` the run uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BbctlNetworkVm {
    /// The keeper network id (e.g. `"signal"`).
    pub network_id: String,
    /// The network's display name (e.g. `"Signal"`), joined from the 6.1 catalog.
    pub name: String,
    /// The name `bbctl` uses for this self-hosted bridge (e.g. `"sh-signal"`).
    pub bbctl_name: String,
}

/// A snapshot of a `bbctl` self-hosted-bridge run, streamed over the run `Channel`
/// (Story 6.7, FR-29, NFR secret containment).
///
/// The single view model the webview renders for the whole run: the `network_id`
/// being run, the current [`BbctlPhase`], an optional per-phase `message`, and the
/// verbatim `error` (failure phase). Carries **only** non-secret render data — the
/// account's Beeper token is never read into a VM, and no raw `bbctl` log line
/// reaches the UI (only recognized phase markers project a snapshot).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BbctlProgressVm {
    /// The stable network id being run (e.g. `"signal"`).
    pub network_id: String,
    /// The current run phase.
    pub phase: BbctlPhase,
    /// An optional per-phase message line, or `null`.
    pub message: Option<String>,
    /// `bbctl`'s verbatim error message (capped, non-secret), present in the
    /// `Failure` phase, else `null`.
    pub error: Option<String>,
}

/// One held send awaiting the elapse of its Undo-Send window (Story 8.3).
///
/// A held send is a message the user approved (composer or Approval Pane) while the
/// Undo-Send window was positive: it has NOT been enqueued to the SDK send queue and
/// is durable in the `outbox` table until either its window elapses (the scheduler
/// dispatches it) or the user undoes it (the row is deleted, its body restored to the
/// composer). It is deliberately NOT an SDK timeline item — the frontend renders it
/// from this VM at the timeline tail, distinct from a real local echo. The body is
/// authoritative in Rust and never logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HeldSendVm {
    /// The opaque unique row id (a `TransactionId`), used to address the row for
    /// cancel / dispatch.
    pub id: String,
    /// The owning account id.
    pub account_id: String,
    /// The target room id.
    pub room_id: String,
    /// The held message body (authoritative, from Rust; never logged).
    pub body: String,
    /// When the send was held, in milliseconds since the Unix epoch (UTC).
    #[ts(type = "number")]
    pub held_at_ms: i64,
    /// When the hold elapses and the row dispatches, in ms since the Unix epoch —
    /// the frontend computes its countdown from this so a resumed Chat picks up the
    /// correct remaining time after a restart.
    #[ts(type = "number")]
    pub dispatch_at_ms: i64,
}

/// A full snapshot of the held sends streamed to the frontend for one open Chat
/// (Story 8.3). The outbox stream is low-churn, so each change emits a fresh, complete
/// snapshot (oldest-first) that REPLACES the room's mirrored rows — the frontend store
/// never folds ops. Empty `rows` means the Chat currently has no held sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OutboxVm {
    /// The held sends for the subscribed Chat, oldest-first.
    pub rows: Vec<HeldSendVm>,
}

/// Which palette query mode the frontend requested (Story 9.1).
///
/// `Default` filters chats + contacts (at ≥2 chars) plus matching actions;
/// `Action` (the `>`-prefix mode) returns only actions with open-chat-context
/// ranking. Serializes to its camelCase name — the `mode` argument of
/// `palette_query`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PaletteMode {
    /// The default finder: chats + contacts + actions.
    Default,
    /// Action mode (the `>` prefix): actions only, context-ranked.
    Action,
}

/// One chat- or contact-kind result row from the command palette (Story 9.1).
///
/// Projected from the in-memory `keeper_core::palette::PaletteIndex` for a query,
/// carrying **only** non-secret render data: the `(accountId, roomId)` selection
/// key, the resolved display name, the owning account's `hueIndex` (0–7) for the
/// hue dot, the bridged-`network` label (`None` for a native room, no badge), and
/// the `isDirect` DM flag. A DM room is surfaced under **Contacts** and excluded
/// from **Chats** so a person is never listed twice — the frontend groups on
/// `isDirect` alone and never re-classifies. No tokens, message bodies, or event
/// ids cross IPC on this VM. The `id` is a stable `"accountId|roomId"` composite
/// so the frontend can key rows without deriving it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaletteChatVm {
    /// Stable composite key `"accountId|roomId"` for the frontend row key.
    pub id: String,
    /// Opaque keeper account id this room belongs to. Drives select/peek.
    pub account_id: String,
    /// Opaque Matrix room id (passed through verbatim as a string).
    pub room_id: String,
    /// The resolved room display name.
    pub display_name: String,
    /// The owning account's hue index (0–7) for the hue dot.
    #[ts(type = "number")]
    pub hue_index: u8,
    /// The bridged-Network label, or `None` for a native Matrix room (no badge).
    pub network: Option<String>,
    /// `true` when the room is a direct/DM room — surfaced under Contacts.
    pub is_direct: bool,
}

/// One action-kind result row from the command palette (Story 9.1).
///
/// A projection of a single entry in the Rust action registry
/// (`keeper_core::palette::palette_actions`) — the sole source of palette actions,
/// reused by the cheat sheet + native menu bar (Story 9.3). Carries the stable
/// `id` (dispatched by the frontend `actions.ts` map), the human `title`, its
/// `category` group label, the `keywords` it also matches on, an optional
/// `shortcut` chip string (e.g. `"⌘K"`), and `requiresOpenChat` — `true` for an
/// action that operates on the currently open chat (Archive, Pin, …), which the
/// frontend disables when no chat is open and which ranks first in action mode
/// when a chat is open. Static, non-secret render data only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaletteActionVm {
    /// Stable action id — the dispatch key the frontend maps to a handler.
    pub id: String,
    /// The human-readable action title (the row label).
    pub title: String,
    /// The action's category / group label (e.g. `"Navigation"`, `"Chat"`).
    pub category: String,
    /// Extra keywords the action also matches on (never rendered directly).
    pub keywords: Vec<String>,
    /// An optional shortcut-chip string (e.g. `"⌘K"`), or `None` when unbound.
    pub shortcut: Option<String>,
    /// `true` when the action operates on the currently open chat.
    pub requires_open_chat: bool,
    /// `true` when the action requires the `recording` capability (Story 16.3):
    /// the shell filters it out of the palette, cheat sheet, and native menu when
    /// screen recording is unavailable, so a recording action is absent (never a
    /// dead button) on platforms that cannot record.
    pub requires_recording: bool,
    /// The toggle-pair group this action belongs to (Story 9.3), e.g. `"archive"`
    /// for both `archive-chat` and `unarchive-chat`. `None` for a non-toggle action.
    /// The palette ignores this (backward-safe); the cheat sheet + native menu
    /// collapse each group's two actions into a single unambiguous entry.
    pub toggle_group: Option<String>,
}

/// One category submenu in the derived menu/cheat-sheet projection (Story 9.3).
///
/// A projection of the action registry (`keeper_core::palette::registry_sections`),
/// grouping `palette_actions` by `category` in a stable order and collapsing each
/// toggle pair (archive/unarchive, pin/unpin, …) into a single [`MenuItemVm`]. Both
/// the native macOS menu bar and the ⌘? cheat sheet render this same projection — no
/// hand-maintained shortcut list, so adding/removing a registry action changes both
/// surfaces automatically (UX-DR15). Static, non-secret render data only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MenuSectionVm {
    /// The category / group label (e.g. `"Navigation"`, `"Chat"`).
    pub category: String,
    /// The category's items, in registry order, toggle pairs collapsed.
    pub items: Vec<MenuItemVm>,
}

/// One item in a derived menu/cheat-sheet section (Story 9.3).
///
/// The stable `id` is the canonical dispatch key the frontend `actions.ts` map
/// resolves — for a collapsed toggle pair it is the canonical (positive) direction
/// (e.g. `archive-chat`), which `use-menu-actions` flips to the opposite direction
/// from the open room's current flag at click time. `title` is the display label (a
/// combined "Archive / Unarchive Chat" for a collapsed pair), `shortcut` the shared
/// chip string, and `requires_open_chat` gates it to an open conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MenuItemVm {
    /// Canonical dispatch id (the positive direction for a collapsed toggle pair).
    pub id: String,
    /// The display label (combined for a collapsed toggle pair).
    pub title: String,
    /// The shared shortcut-chip string (e.g. `"⌘1"`, `"E"`), or `None` when unbound.
    pub shortcut: Option<String>,
    /// The toggle-pair group (e.g. `"archive"`) for a collapsed item, else `None`.
    pub toggle_group: Option<String>,
    /// `true` when the item operates on the currently open chat.
    pub requires_open_chat: bool,
}

/// The grouped, ranked, bounded result of one `palette_query` (Story 9.1).
///
/// The single view model the palette renders. `contacts` holds matched direct/DM
/// rooms, `chats` holds matched non-DM rooms (a person is never in both), and
/// `actions` holds matched (or, on an empty/short/no-match query, top) registered
/// actions. All filtering, fuzzy scoring, and ranking happen in Rust — the
/// frontend only renders these three lists and dispatches by id; it never filters
/// or re-orders. Each list is capped to a bounded top-N so the render stays cheap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaletteResultsVm {
    /// Matched direct/DM contact rows (empty for a short/no-match query).
    pub contacts: Vec<PaletteChatVm>,
    /// Matched non-DM chat rows (empty for a short/no-match query).
    pub chats: Vec<PaletteChatVm>,
    /// Matched actions, or the top registered actions on a short/no-match query.
    pub actions: Vec<PaletteActionVm>,
}

/// A macOS TCC (privacy database) permission state as reported by the `keeper-rec`
/// sidecar (Story 16.4, Epic 16, AD-34).
///
/// The sidecar is the process that will capture (16.6), so *its* grant is the one
/// that matters. In this story only `screenRecording` is probed live (a
/// non-prompting CoreGraphics preflight in the sidecar); `microphone`/`camera` stay
/// provisional `NotDetermined` until AVFoundation detection lands (16.6/19). The
/// preflight is two-valued, so it only reports `Granted` vs `NotDetermined` — it
/// cannot distinguish an explicit `Denied` from a never-requested state. The
/// authoritative granted / not-yet-requested / denied tri-state and its live
/// pre-flight UI (request, deep-link, re-detection) are Story 16.5's. Serializes
/// to `"granted" | "denied" | "notDetermined"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TccPermission {
    /// The permission is granted to the capturing process.
    Granted,
    /// The permission is not currently granted (denied or revoked).
    Denied,
    /// The permission has not been decided yet (or detection is deferred).
    NotDetermined,
}

/// The capture feature flags the `keeper-rec` sidecar reports via `getCapabilities`
/// (Story 16.4, AD-34). Shape-locked; values are code-owned and honest about what
/// the sidecar build actually supports today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingFeaturesVm {
    /// Whether system-audio capture is supported (true on the macOS 13+ sidecar).
    pub system_audio: bool,
    /// Whether microphone capture is supported (live since Story 19.3).
    pub microphone: bool,
    /// Whether camera/webcam capture is supported (live since Story 20.1 — a
    /// separate `camera-####.mp4` per segment, never a track inside the
    /// screen file).
    pub camera: bool,
}

/// The `getCapabilities` handshake result (Story 16.4, AD-34): the sidecar's
/// protocol version, macOS version, feature flags, and per-TCC permission states.
///
/// `protocol_version` carries the handshake — the host compares it against
/// `keeper_core::recording::PROTOCOL_VERSION` and surfaces a mismatch as an honest
/// `Unsupported`, never a crash. Consumed by 16.5's permission pre-flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingCapabilitiesVm {
    /// The NDJSON-RPC protocol version the sidecar speaks.
    pub protocol_version: u32,
    /// The sidecar host's macOS version, e.g. `"15.5.0"` (display-only, never parsed
    /// for gating — the `recording` capability flag owns the version gate).
    pub macos_version: String,
    /// What this sidecar build can capture.
    pub features: RecordingFeaturesVm,
    /// The Screen Recording state of the sidecar process from a non-prompting
    /// preflight — `Granted` or `NotDetermined` only (the preflight cannot confirm
    /// an explicit `Denied`; 16.5's live pre-flight resolves the full tri-state).
    pub screen_recording: TccPermission,
    /// The Microphone TCC state — the real, non-prompting AVFoundation
    /// tri-state (Story 19.3).
    pub microphone: TccPermission,
    /// The Camera TCC state — the real, non-prompting AVFoundation tri-state
    /// (Story 20.1).
    pub camera: TccPermission,
}

/// One recordable display reported by `listSources` (Story 16.4, AD-34) — real
/// values from the sidecar's active-display enumeration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingDisplayVm {
    /// The CoreGraphics display id (stable for the session, not across reboots).
    pub id: u32,
    /// The display width in points.
    pub width: u32,
    /// The display height in points.
    pub height: u32,
    /// Whether this is the main display (menu-bar display).
    pub is_main: bool,
    /// The display's true pixel width (Story 22.1; additive — 0 when an older
    /// sidecar omits it). Backs the live effective-resolution hint.
    #[serde(default)]
    pub pixel_width: u32,
    /// The display's true pixel height (Story 22.1; additive — 0 when absent).
    #[serde(default)]
    pub pixel_height: u32,
}

/// One recordable application reported by `listSources` (Story 16.4/19.1). Real
/// enumeration lands in Story 19.1 via the sidecar's `SCShareableContent`
/// enumeration: keeper's own bundle id is excluded (it can never be a target),
/// apps that own no on-screen window are dropped, and the list is name-sorted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingApplicationVm {
    /// The application bundle identifier.
    pub bundle_id: String,
    /// The human-readable application name.
    pub name: String,
    /// The running process id.
    pub pid: i32,
    /// The app's icon as a bounded (≤64×64px) PNG `data:image/png;base64,…`
    /// URI (Story 19.1), or `None` when an icon can't be produced — the picker
    /// then falls back to a generic app glyph. Kept small so the polled list
    /// never becomes a large-payload-over-IPC violation.
    pub icon: Option<String>,
}

/// One recordable audio/video device (microphone or camera) reported by
/// `listSources` (Story 16.4): a flat `{id, name}` row — the `localizedName`
/// already distinguishes built-in / external / Continuity devices, so there
/// is deliberately no device-class field. Microphones enumerate live since
/// Story 19.3, cameras since Story 20.1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingDeviceVm {
    /// The device's unique identifier.
    pub id: String,
    /// The human-readable device name.
    pub name: String,
}

/// The `listSources` result (Story 16.4, AD-34): everything the sidecar can
/// currently offer as a capture source — real displays (CoreGraphics), real
/// applications (SCShareableContent, Story 19.1), real microphones (Story
/// 19.3) and real cameras (Story 20.1) via AVFoundation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingSourcesVm {
    /// The active displays (real, from the sidecar's display enumeration).
    pub displays: Vec<RecordingDisplayVm>,
    /// Recordable applications (real since Story 19.1). Empty means "not
    /// enumerated or none available" — NEVER a permission verdict. The sidecar
    /// skips this leg entirely while Screen Recording is ungranted, because
    /// enumerating it prompts; the honest verdict is [`ScreenRecordingAccess`],
    /// which the picker's surface already holds.
    pub applications: Vec<RecordingApplicationVm>,
    /// Microphone devices (real since Story 19.3).
    pub microphones: Vec<RecordingDeviceVm>,
    /// Camera devices (real since Story 20.1) — a flat name list for the
    /// Webcam card's picker.
    pub cameras: Vec<RecordingDeviceVm>,
}

/// The single capture target a Recording Session records (Story 19.1) — the
/// picker's selection and the `recording_start` input.
///
/// An internally-tagged union (`{kind:"display"|"application", …}`) so invalid
/// combinations are unrepresentable: a display target carries only a
/// `displayId`, an application target only a `pid`+`bundleId`. `Display` with a
/// `None`/absent `displayId` means the main display (the unchanged 16.6
/// default). The shell maps this into a `keeper_core::recording::CaptureTarget`
/// (the manifest) and a `SessionParams.application`/`display_id` (the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum RecordingTargetVm {
    /// Capture a whole display (`None`/absent `displayId` = the main display).
    Display {
        /// The CoreGraphics display id, or `None` for the main display.
        #[serde(default)]
        display_id: Option<u32>,
    },
    /// Capture a single application's windows (exclusionary: keeper, other apps,
    /// and notification banners stay out of the file).
    Application {
        /// The application's running process id (re-resolved live at Start).
        pid: i32,
        /// The application's bundle identifier (for the manifest + disclosure).
        bundle_id: String,
    },
    /// Audio-only session (Story 21.3): system audio and/or the microphone —
    /// no video track, no screen pixels, `audio-####.m4a` segments.
    AudioOnly,
}

/// The honest Screen Recording tri-state the permission pre-flight resolves
/// (Story 16.5, Epic 16, FR-67, AD-36).
///
/// The sidecar's non-prompting preflight is two-valued ([`TccPermission`]:
/// `Granted` vs `NotDetermined`) — it cannot tell an explicit denial from a
/// never-requested state. `keeper_core::recording::resolve_screen_recording_access`
/// lifts it into this tri-state with a host *session* "already requested this app
/// lifetime" flag (one real OS prompt per app lifetime; the flag never persists
/// across sessions, so a grant is never cached optimistically). Serializes to
/// `"granted" | "notYetRequested" | "denied"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ScreenRecordingAccess {
    /// The permission is granted — capture can start.
    Granted,
    /// The permission has never been requested this app lifetime — the OS prompt
    /// is still available (`CGRequestScreenCaptureAccess` will show it).
    NotYetRequested,
    /// The permission is not granted and a prompt will not help (an explicit
    /// denial, or a request already spent this session) — the fix path is the
    /// System Settings deep link.
    Denied,
}

/// The recording permission pre-flight result the Recording view renders
/// (Story 16.5, FR-67, AD-36; mic/camera legs Story 20.2).
///
/// Live-detected through the `Recorder` port on every fetch (render,
/// focus/return re-detection, and every enabled-source change) — never cached.
/// All three legs resolve from the *same* `getCapabilities` probe: screen via
/// `keeper_core::recording::resolve_screen_recording_access` (the two-valued
/// preflight lifted with the session flag), mic/camera via
/// `keeper_core::recording::resolve_source_access` (the AVFoundation tri-state
/// mapped directly, no flag needed). `can_start` is the single Start gate:
/// `true` only when every required permission — Screen Recording plus each
/// *enabled* source leg — is granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingPermissionVm {
    /// The resolved Screen Recording tri-state.
    pub screen_recording: ScreenRecordingAccess,
    /// The Microphone leg (Story 20.2) — `Some` iff the mic source is enabled;
    /// `None` (disabled) renders no row and never gates Start.
    pub microphone: Option<ScreenRecordingAccess>,
    /// The Camera leg (Story 20.2) — `Some` iff the webcam is enabled;
    /// `None` (disabled) renders no row and never gates Start.
    pub camera: Option<ScreenRecordingAccess>,
    /// Whether Start may be enabled (`true` only when every required grant is
    /// green).
    pub can_start: bool,
}

/// The UI-facing state of the (at most one) live recording session (Story 16.6,
/// AD-33). A plain string projection of `keeper_core::recording::SessionState`
/// plus `idle` for "no session yet this app lifetime". Serializes to
/// `"idle" | "preflight" | "recording" | "rotating" | "stopping" | "finalized" |
/// "recovered" | "failed"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RecordingUiState {
    /// No session has run yet (or the last one's outcome was acknowledged).
    Idle,
    /// The sidecar is pre-flighting (permission / source checks).
    Preflight,
    /// Capture is live.
    Recording,
    /// A segment rotation is in progress (Epic 17; unreachable in 16.6).
    Rotating,
    /// A stop was requested; the output is finalizing.
    Stopping,
    /// Terminal — the recording finalized into a playable file.
    Finalized,
    /// Terminal — a partial recording was salvaged (Epic 17; unreachable in 16.6).
    Recovered,
    /// Terminal — the session failed (`error` carries the honest message).
    Failed,
}

impl RecordingUiState {
    /// Whether a session in this state is live — capture running or winding
    /// down (Story 18.2): the states where the tray must be present and a quit
    /// must warn first. Exhaustive by design: a new variant forces a decision
    /// here.
    pub fn is_live(self) -> bool {
        match self {
            Self::Preflight | Self::Recording | Self::Rotating | Self::Stopping => true,
            Self::Idle | Self::Finalized | Self::Recovered | Self::Failed => false,
        }
    }

    /// Whether this state is settled — no session yet, or a terminal outcome
    /// (Story 18.2). The exact complement of [`Self::is_live`], spelled out as
    /// its own exhaustive `match` so a new variant forces both decisions.
    pub fn is_terminal(self) -> bool {
        match self {
            Self::Idle | Self::Finalized | Self::Recovered | Self::Failed => true,
            Self::Preflight | Self::Recording | Self::Rotating | Self::Stopping => false,
        }
    }
}

/// How far this recording session's bytes have travelled beyond this Mac
/// (Story 41.6, FR-138, UX-DR48).
///
/// The variants are declared least- to most-durable ON PURPOSE, and the derived
/// `Ord` is load-bearing: the state a session reports is a FLOOR, and a floor is
/// a `max` over everything the session has observed. Deriving the order from the
/// declaration means the ranking cannot drift from a hand-written table that
/// someone later forgets to extend.
///
/// The words a surface prints for these are the RECORDER's, never git's
/// (UX-DR48): `Local` is "on this Mac", `Committed` is "committed",
/// `Pushed`/`Verified` are "on the drive". A commit and a push are HOW it
/// happened; what the person asked was whether the recording would survive the
/// laptop being dropped. Only the enum tag crosses IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RecordingDurabilityState {
    /// The bytes exist here and nowhere else. Either the destination is a plain
    /// folder — which makes no further promise, and says so plainly — or a
    /// profile destination has not committed anything yet.
    Local,
    /// The session's segments are in a commit in the destination profile. They
    /// survive the recorder, the app and a power cut; they do not yet survive
    /// the machine.
    Committed,
    /// That commit is on the remote. Branch-level, deliberately: whether THIS
    /// path's objects are on the far side is a question only the network can
    /// answer, and the poll path may not ask it — "the branch holding it has
    /// nothing the remote lacks" is the strongest local truth there is.
    Pushed,
    /// The engine has verified the pushed objects.
    ///
    /// Reserved rather than reachable today: nothing the engine records locally
    /// distinguishes a verified push from a push, so the derivation cannot
    /// synthesise this — it is what the verification pass will report when it
    /// records per-path results. The variant exists because the surface must not
    /// have to change shape the day it does.
    Verified,
}

/// The one durability reading a recording session carries (Story 41.6, FR-138,
/// UX-DR48/UX-DR49).
///
/// **DERIVED, never stored.** It is computed on the `recording_status` poll from
/// the destination and the engine's own local knowledge, so it cannot go stale
/// or disagree with the thing it describes; nothing about durability is written
/// to disk, and there is no second copy to reconcile. It rides the status the
/// recording surface ALREADY polls at ~1 Hz rather than a stream of its own,
/// because a second channel for a scalar that changes a handful of times per
/// session would be a second thing that can be out of date.
///
/// `detail` is the Rust-authored reason a push has not happened — `"push
/// rejected: non-fast-forward"`, a protected branch, an unreachable remote —
/// present ONLY when there is such a problem. It exists so the surface can print
/// the reason VERBATIM instead of inventing sync language of its own: a rejected
/// push is not a failed recording, and "recorded, not pushed" plus the remote's
/// own words is the honest reading. A healthy session carries `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingDurabilityVm {
    /// How far this session's bytes have got — the session's floor, so it never
    /// walks backwards while the session runs.
    pub state: RecordingDurabilityState,
    /// Why publication has not happened, in Rust's words, when it has not.
    pub detail: Option<String>,
}

impl RecordingDurabilityVm {
    /// The on-this-Mac reading: the honest answer for a plain-folder
    /// destination, for a session that has committed nothing yet, and for the
    /// idle snapshot. No problem is named because none has happened — nothing
    /// was promised.
    pub fn local() -> Self {
        Self {
            state: RecordingDurabilityState::Local,
            detail: None,
        }
    }
}

/// The recording-session status snapshot the Recording view polls (Story 16.6,
/// FR-68/FR-69/FR-71, UX-DR30).
///
/// The single source of truth for the active-session UI: the state drives the
/// record dot + Stop affordance, `started_at_epoch_ms` anchors the ticking
/// elapsed line (computed client-side from the host-reported start instant, so a
/// slow poll never freezes the clock), `output_path` is the session **folder**
/// holding the `screen-####.mp4` segments (Story 17.2 — not a single file; the
/// tray sums it live and "Open Recordings Folder" reveals it), and `error` is
/// the honest failure message on `failed` — never a silent reset.
///
/// `durability` (Story 41.6) is the one field here that is DERIVED rather than
/// folded from a sidecar event: it is computed on each read of this snapshot
/// from the session's destination and the engine's local knowledge, which is
/// why it needs no new command and no new stream — the surface that asks
/// "would what I have recorded survive?" is already asking this snapshot
/// everything else it shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingStatusVm {
    /// The session state driving the active-recording UI.
    pub state: RecordingUiState,
    /// Segments closed so far (0 in 16.6 — rotation is Epic 17).
    pub segments_closed: u32,
    /// When capture started (Unix epoch ms), for the ticking elapsed line.
    /// Emitted to TypeScript as `number` (Tauri IPC delivers JSON numbers, and
    /// epoch milliseconds sit far inside `Number.MAX_SAFE_INTEGER`).
    #[ts(type = "number | null")]
    pub started_at_epoch_ms: Option<u64>,
    /// The absolute path of the session **folder** being (or last) written —
    /// the directory holding the `screen-####.mp4` segments (Story 17.2).
    pub output_path: Option<String>,
    /// The honest failure message when `state == failed`.
    pub error: Option<String>,
    /// The sticky, non-fatal session warning (Story 19.4) — e.g. a microphone
    /// unplugged mid-recording. Set on the first sidecar `warning` event
    /// (last-write-wins message, NOT gated on any state — the session stays
    /// live) and never cleared for the rest of the session; it resets to
    /// `None` only when a new session starts. Drives the tray's
    /// warning-marked status line and the banner's amber variant.
    pub warning: Option<String>,
    /// Total on-disk bytes of this session's `screen-####.mp4` segments (Story
    /// 18.3) — the banner's and tray's `size` line. **Read-time**, not
    /// driver-maintained: `recording_snapshot` fills it best-effort from disk
    /// each read (0 when there is no session/folder, so the *stored* snapshot
    /// the driver keeps carries 0).
    ///
    /// Emitted as `number` (like `started_at_epoch_ms`): a byte count sits far
    /// inside `Number.MAX_SAFE_INTEGER`, and the banner does plain numeric math.
    #[ts(type = "number")]
    pub on_disk_bytes: u64,
    /// On-disk bytes of the **current** (highest-index, open) segment (Story
    /// 18.3) — the segment meter's numerator, which falls back toward ~0 at each
    /// gapless rotation. Read-time (see [`Self::on_disk_bytes`]); 0 with no
    /// session/segment. Emitted as `number` (see [`Self::on_disk_bytes`]).
    #[ts(type = "number")]
    pub current_segment_bytes: u64,
    /// The **session-captured** segment-size cap in decimal MB (Story 18.3) —
    /// the meter's denominator, read from settings once at `recording_start` and
    /// carried on the live run. Session-captured (never re-read from the mutable
    /// settings store, so a mid-session cap edit cannot skew a running meter); 0
    /// when there is no session.
    pub segment_cap_mb: u32,
    /// How far this session's bytes have got beyond this Mac (Story 41.6,
    /// FR-138) — the banner's one honest line and, when it names a problem,
    /// the tray's warning-marked status line. DERIVED on every read (see the
    /// type docs) and never stored, so it cannot disagree with the engine; a
    /// session with no destination profile, and the idle snapshot, both read
    /// [`RecordingDurabilityVm::local`].
    pub durability: RecordingDurabilityVm,
}

/// The read-only end-of-session summary the completion / recovery cards render
/// (Story 20.3, FR-71/FR-73). Derived on demand from a session's authoritative
/// on-disk `manifest.json` (never the live `RecordingStatusVm` snapshot): the
/// screen-track segment count backs "Saved N segments", the total on-disk bytes
/// back "{size}", and the folder path backs the mono line + Reveal in Finder.
///
/// Not a live-poll VM and not `ts_rs`-exported — the frontend declares the twin
/// `RecordingSummaryVm` type in `client.ts`; this struct only fixes the camelCase
/// wire shape the summary/list commands return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSummaryVm {
    /// The session **folder** path (holding the `screen-####.mp4` segments) —
    /// the mono line and the Reveal-in-Finder target.
    pub session_folder: String,
    /// The number of screen-track segments the session saved ("Saved N
    /// segments") — never the track-agnostic live `segments_closed` counter.
    pub screen_segment_count: u32,
    /// The total on-disk bytes across every segment (screen + camera) — the
    /// card's `{size}` line. Emitted as `number` (a byte count sits far inside
    /// `Number.MAX_SAFE_INTEGER`).
    pub total_bytes: u64,
    /// The user session title when one was set (Story 21.5) — rendered above
    /// the folder path on the completion card and the recovery notice.
    pub title: Option<String>,
}

/// A finished session's `meta` block, shaped for the "Next session" form
/// (Story 45.19, FR-197).
///
/// The same five fields the form collects, and in the form's own units — which
/// is the point: this VM is what the editor on the last recording opens with,
/// and what "record another like this" fills a fresh form from. A surface
/// receiving it never has to know how the manifest stores anything.
///
/// The one shape change from [`crate::recording::SessionMeta`] is [`Self::tags`]:
/// stored as a list, presented as the single comma-separated line the field
/// holds, joined by [`crate::recording::SessionMeta::tags_line`] so the join and
/// the split that undoes it stay one decision.
///
/// Every field is a plain `String` rather than an `Option`, because an absent
/// manifest field and an empty form field are the same fact to a form and
/// giving them two representations would only invite a `?? ""` at the other end.
/// "There is no session here at all" is a different fact and is carried by the
/// command answering `None`.
///
/// Not `ts_rs`-exported, for [`RecordingSummaryVm`]'s reason — the frontend
/// declares the twin in `client.ts`; this fixes the camelCase wire shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSessionMetaVm {
    /// The session's human title, `""` when it has none.
    pub title: String,
    /// Who the recording is with, `""` when unset.
    pub participants: String,
    /// The program/session note, `""` when unset.
    pub note: String,
    /// The tags as one comma-separated line, `""` when there are none.
    pub tags: String,
    /// The repeatable custom rows, in the order the manifest holds them.
    pub custom: Vec<RecordingSessionMetaFieldVm>,
}

/// One custom name/value row of [`RecordingSessionMetaVm`] — the wire twin of
/// [`crate::recording::SessionMetaField`], which is not itself a VM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSessionMetaFieldVm {
    /// The row's user-chosen name.
    pub name: String,
    /// The row's value.
    pub value: String,
}

/// The note stub the stop surface presents (Story 42.4, FR-142).
///
/// Composed by [`crate::notes::recording_note::compose`] at finalize and written
/// to disk by the shell, so by the time this VM exists there is a real file at
/// [`Self::path`]. `None` on the summary side means only "no stub file for this
/// session" — never written, or already dismissed — and the surface renders
/// nothing. It is never an error: a stub that could not be written is logged,
/// because finalize already succeeded and the recording is safe.
///
/// **`contents` is the file, not the composition.** After the user saves, this
/// carries what is on disk, with [`Self::body_offset`] re-derived from that
/// file's own frontmatter. Re-seeding an untouched draft from it can therefore
/// never resurrect text the user deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingNoteStubVm {
    /// The stub's absolute path on this machine: the save target, and what
    /// Reveal opens. Absolute is correct *here* and forbidden inside the note
    /// itself — FR-145 is a rule about what gets persisted and synced, and this
    /// VM is neither (the sibling `sessionFolder` on the summary is absolute for
    /// the same reason).
    pub path: String,
    /// `2026-08-08-quarterly-review.md` — the name, for display.
    pub filename: String,
    /// The whole file: frontmatter block, a blank separator line, then the body.
    pub contents: String,
    /// Where the body starts in [`Self::contents`], in **UTF-16 code units**.
    ///
    /// Not bytes. The surface splits `contents` at this index with
    /// `String.prototype.slice`, renders only the tail in the textarea and saves
    /// `head + draft` — so the user can never type inside the frontmatter, and
    /// the block keeper authored comes back byte-identical. JavaScript indexes
    /// strings in UTF-16 code units, so a byte offset would land mid-character
    /// on any non-ASCII title and silently split the block in the wrong place.
    /// Converted once, in Rust, from the composer's byte offset.
    pub body_offset: u32,
    /// `true` when the stub went into a notes vault subtree, `false` when it was
    /// written beside the session folder. Both are real files at real paths;
    /// this says whether the vault's index will pick it up.
    pub in_vault: bool,
    /// The session's immutable `<device ULID>-<session ULID>` (Story 40.3) — the
    /// same id the stub's own `session:` field carries.
    pub session_id: String,
    /// The stub's path relative to the root it was written under, for display.
    pub relative_path: String,
}

impl RecordingStatusVm {
    /// The boot/default snapshot: no session yet.
    pub fn idle() -> Self {
        Self {
            state: RecordingUiState::Idle,
            segments_closed: 0,
            started_at_epoch_ms: None,
            output_path: None,
            error: None,
            warning: None,
            on_disk_bytes: 0,
            current_segment_bytes: 0,
            segment_cap_mb: 0,
            durability: RecordingDurabilityVm::local(),
        }
    }
}

/// Which kind of place the recordings destination is (Story 41.2, FR-131,
/// UX-DR47).
///
/// The destination is a resolved DECISION, not a path, and this is the decision:
/// a plain folder on this machine, or a sync profile that says it holds
/// recordings. Exactly one of `recording.destination_dir` and
/// `recording.destination_profile_id` is in force, and this is which — carried
/// on the VM so a surface can state the CONSEQUENCE of the choice ("recordings
/// here are committed and pushed by that folder") instead of offering sync as a
/// second checkbox beside it. A second "also sync my recordings" toggle would be
/// a second source of truth about something the destination already decides,
/// which is exactly what UX-DR47 refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RecordingDestinationKind {
    /// A plain folder: `recording.destination_dir` (or the shell's default) is in
    /// force, and nothing publishes what is recorded there.
    Folder,
    /// A recordings-flagged sync profile: `recording.destination_profile_id` is
    /// in force, and that profile commits and pushes what is recorded there on
    /// its own policy.
    Profile,
}

/// One recordings-flagged sync profile, as the destination picker needs it
/// (Story 41.2, FR-131; Story 46.10 added the fourth field).
///
/// `recordings_root` is RESOLVED here — `local_path` joined with the profile's
/// recordings subfolder — for the same reason
/// [`RecordingSettingsVm::destination_dir`] is: no surface anywhere joins a
/// local path and a subfolder itself, so there is one definition of "where this
/// profile's recordings live" and it lives in Rust.
///
/// Only flagged, enabled profiles are ever listed: a folder that has not said it
/// holds recordings is not a destination, and a paused one is not one either
/// (the resolution degrades to the plain-path answer for both, so offering them
/// would be offering a choice keeper would then ignore).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingProfileVm {
    /// The profile's opaque ULID — what the destination choice persists, so the
    /// resolved root survives a rename of the folder or of the profile.
    pub id: String,
    /// The profile's human label, as the tray and commit subjects show it.
    pub name: String,
    /// The profile's RESOLVED recordings root, as an absolute path.
    pub recordings_root: String,
    /// The profile-relative recordings subfolder `recordings_root` was composed
    /// FROM — the "head" of the path a recording takes (Story 46.10).
    ///
    /// Carried beside the resolved root rather than sliced back out of it by the
    /// surface, and read in the same breath as it, so the two can never disagree:
    /// a card that showed a head from one read and a root from another would be
    /// describing two different profiles a fraction of a second apart. It is also
    /// not recoverable by string surgery — `local_path` is not on this row, and
    /// the join normalised nothing, so `20-media//sessions` and `20-media/sessions`
    /// resolve to one root and are two different stored values, only one of which
    /// an edit box may echo back to `sync_profile_save`.
    ///
    /// May be more than one component (`40-media/recordings`): a nested subfolder
    /// is valid and always has been (`RecordingsConfig::validate` refuses empty,
    /// absolute, escaping and vault-overlapping values, and nothing else).
    ///
    /// This is the half of the destination path that TRAVELS. It lives in the
    /// profile row in `sync.db`, so every machine syncing this folder records
    /// into it; the other half — `recording.path_template` — is a per-machine
    /// settings key and does not.
    pub subfolder: String,
}

/// Whether the recordings destination's volume is here right now (Story 41.7,
/// AD-48).
///
/// The three answers `keeper-sync`'s `VolumeStatus` gives, reduced to what a
/// surface can say a sentence about. Deliberately NOT a boolean: "a different
/// stick is mounted where yours lives" and "no stick at all" take different
/// actions from the person holding the drive, and collapsing them would make
/// the card tell one of the two a lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RecordingVolumeState {
    /// The volume the profile is bound to is attached: recording into it works.
    Attached,
    /// No volume marker at or above the folder — the media is not attached.
    /// A first-class state, not a fault (AD-48), and the one that makes
    /// `recording_start` refuse rather than quietly record somewhere else.
    Absent,
    /// Something is mounted where this profile's volume lives, but it is not
    /// provably that volume — a foreign marker, or one that could not be read.
    /// Refused for the same reason `Absent` is, with a different sentence.
    Unexpected,
}

/// The recordings destination's removable media, when it has any (Story 41.7).
///
/// Present ⇒ the destination's synced folder is on removable media; absent ⇒ it
/// is on a disk that is always there, and no surface says anything about drives.
/// Modelling removability as the OPTION rather than as a `removable: bool`
/// beside a state is what makes "not removable, but the volume is absent"
/// unrepresentable instead of merely unlikely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingVolumeVm {
    /// What the volume calls itself: its marker's label — the mount point's own
    /// name, `"merope"`, recorded when the volume was adopted. Never derived by
    /// slicing the local path apart: a stick re-mounted somewhere else is the
    /// same volume with the same name, and the path is the one thing about it
    /// that moves.
    ///
    /// `null` when the shell has never had this volume's marker in front of it —
    /// a drive that has been out since the app launched carries its own name
    /// away with it. The card has an unnamed phrasing for that; it does not
    /// invent a name.
    pub name: Option<String>,
    /// Whether that volume is attached right now.
    pub state: RecordingVolumeState,
}

/// The user-configurable recording settings (Story 17.5 + 19.5 + 40.2 + 41.2,
/// FR-72, FR-131): the segment size, the duration-cap rotation fallback, the
/// destination, the path template, the frame rate, the codec, the capture scale
/// and echo cancellation, as persisted in the `settings` k/v table
/// (`recording.segment_mb` / `recording.duration_cap_minutes` /
/// `recording.destination_dir` / `recording.destination_profile_id` /
/// `recording.path_template` / `recording.fps` / `recording.codec` /
/// `recording.scale_percent` / `recording.echo_cancellation`).
///
/// All settings surfaces (Settings → Recording and the pre-record setup cards)
/// render exactly this VM. The setter command normalizes (segment `100..=5000`
/// MB, duration cap `1..=600` min, fps {10, 15, 30, 60}) and returns the effective VM,
/// so the UI never displays an unsaved value. The path template is the one
/// field that is REJECTED rather than normalized when it is wrong — a template
/// is a specification, and rewriting one silently would hand the user a path
/// they did not ask for. Read again at every `recording_start` — edits apply to
/// the next Recording Session only.
///
/// **The destination is a decision, not a path** (Story 41.2, UX-DR47). Exactly
/// one of the two destination keys is in force;
/// [`Self::destination_kind`] says which, [`Self::destination_dir`] is the
/// RESOLVED root either way, and on the way IN the kind is the discriminator:
/// `Profile` reads [`Self::destination_profile_id`] and ignores the folder,
/// `Folder` reads the folder and ignores the id, and every write clears the key
/// that lost. Submitting a profile that does not hold recordings, or a plain
/// folder inside a synced folder's tree, is refused with
/// `IpcErrorCode::RecordingDestinationRefused` and writes nothing — except the
/// one unambiguous case: a plain folder that IS a synced folder's recordings
/// root is normalised to the PROFILE choice, because they are the same place and
/// only one of them carries the consequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingSettingsVm {
    /// Segment size in decimal MB (the sidecar's `segmentMB`; default 500).
    pub segment_mb: u32,
    /// Duration-cap rotation fallback in whole minutes (default 30; sent to the
    /// sidecar as `maxSegmentSeconds = minutes × 60`).
    pub duration_cap_minutes: u16,
    /// The EFFECTIVE destination root (Story 19.5, Story 41.2), whichever choice
    /// is in force: the chosen profile's resolved recordings root under
    /// [`RecordingDestinationKind::Profile`], the persisted folder (or the
    /// shell-resolved `~/Movies/keeper` default) under
    /// [`RecordingDestinationKind::Folder`]. Always a concrete absolute path —
    /// the "unset vs default" ambiguity never reaches the UI.
    ///
    /// It keeps this meaning across both kinds on purpose: `<local_path>` joined
    /// with a profile's recordings subfolder is computed in ONE place, in Rust,
    /// so no surface anywhere derives a destination and none of them can disagree
    /// with where `recording_start` actually writes.
    ///
    /// On the way IN this is an input only under `Folder` — a folder the user
    /// picked, or blank to clear the key and fall back to the default. Under
    /// `Profile` it is ignored, because it is an answer rather than a question.
    pub destination_dir: String,
    /// Which destination choice is in force (Story 41.2, UX-DR47), and the
    /// discriminator the setter reads.
    ///
    /// This is what lets a surface state the CONSEQUENCE of the choice —
    /// recordings in a synced folder are committed and pushed by that folder's
    /// own policy — rather than offering sync as a checkbox beside the
    /// destination. `Folder` and `Profile` are exhaustive because exactly one of
    /// the two settings keys is ever in force.
    pub destination_kind: RecordingDestinationKind,
    /// The chosen sync profile's opaque id under
    /// [`RecordingDestinationKind::Profile`], `None` under `Folder`.
    ///
    /// The ID is what persists (never the resolved path), so the destination
    /// survives a rename of the profile or a move of its folder. A stored id that
    /// no longer names a usable profile — deleted, paused, or no longer saying it
    /// holds recordings — degrades to the folder answer on READ with a `warn`
    /// line, and never fails the settings read: a machine with no `git` still
    /// records.
    pub destination_profile_id: Option<String>,
    /// The chosen profile's human name under [`RecordingDestinationKind::Profile`],
    /// `None` under `Folder`.
    ///
    /// Resolved from the id on every read rather than cached beside it, which is
    /// what makes a rename show up here immediately with the same resolved root.
    pub destination_profile_name: Option<String>,
    /// The chosen profile's removable media (Story 41.7), `None` under `Folder`
    /// and for a synced folder on a disk that is always there.
    ///
    /// Output only, and re-scanned on every read for the same reason the name
    /// is re-resolved: the answer changes when someone plugs a drive in, and a
    /// value cached beside the choice would say "not attached" about a stick
    /// that is sitting in the port. This is what lets the card say a
    /// destination is on removable media BEFORE Record is pressed, instead of
    /// letting the person find out from a failure (AD-48).
    pub destination_volume: Option<RecordingVolumeVm>,
    /// Capture frame rate (Story 19.5): 10, 15, 30 (default), or 60,
    /// normalized on read/write; the sidecar's `fps`.
    pub fps: u32,
    /// Video codec (Story 21.1): `"h264"` (maximum-compatibility default) or
    /// `"hevc"` (VideoToolbox hardware encode on Apple Silicon; markedly
    /// smaller files). Normalized on read/write; the sidecar's `codec`.
    pub codec: String,
    /// Capture scale percent (Story 21.2): 100 (default), 75, or 50 of the
    /// native pixel resolution, normalized on read/write; the sidecar's
    /// `scalePercent` (dimensions rounded to even pixels Swift-side).
    pub scale_percent: u32,
    /// Acoustic echo cancellation on the microphone feed (Story 22.7): `true`
    /// (the default) runs the mic through macOS's voice-processing unit, whose
    /// echo reference is the OUTPUT DEVICE's mix — so what the speakers play
    /// stops being re-recorded by the microphone. Costs a mono mic track and
    /// non-defeatable voice-band noise suppression. Read at every
    /// `recording_start`; the sidecar's `echoCancellation`, emitted only when
    /// the mic is on.
    pub echo_cancellation: bool,
    /// The EFFECTIVE recording path template (Story 40.2, AD-65): the persisted
    /// user choice when one exists and still parses, otherwise
    /// [`DEFAULT_TEMPLATE`](crate::recording::path_template::DEFAULT_TEMPLATE).
    /// Always a concrete, parseable template — never empty and never the unset
    /// sentinel, so the "unset vs default" ambiguity never reaches the UI, and
    /// a hand-edited `config.json` row that does not parse degrades here rather
    /// than failing the whole settings read. Submitting one that does not parse
    /// is rejected with `IpcErrorCode::RecordingTemplateInvalid` and writes
    /// nothing; submitting a blank one clears the key, which reads back as the
    /// default.
    pub path_template: String,
}

/// What a path template would name the next recording — or why it would not
/// name anything (Story 40.2, UX-DR45/UX-DR46).
///
/// The live preview under the template field IS the documentation for the
/// template language, so everything printed there is composed in Rust and
/// rendered verbatim: the path comes from the one renderer
/// ([`PathTemplate::render`](crate::recording::path_template::PathTemplate::render))
/// and the refusal comes from the one parser, so what the preview promises and
/// what `recording_start` will do cannot drift. A second renderer in TypeScript
/// is exactly what AD-65 forbids, and it could not produce these sentences.
///
/// The `summary`-or-`problem` shape is `SyncGitVm`'s: exactly one side is
/// populated. `problem` present ⇒ the template did not parse, both paths are
/// `None` (the preview never shows a path the template could not produce), and
/// the surface disables its save. `problem` absent ⇒ both paths are present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingPathPreviewVm {
    /// The rendered path beneath the destination root, e.g.
    /// `2026/2026-08-05 1432 standup`. `None` when the template did not parse.
    pub relative_path: Option<String>,
    /// The absolute folder the next recording would use — the destination
    /// surface's one line of truth (UX-DR46), resolved against the EFFECTIVE
    /// destination root. `None` when the template did not parse.
    pub absolute_path: Option<String>,
    /// Why the template was refused, as a standalone sentence to print inline
    /// beside the field. `None` when it parsed.
    pub problem: Option<String>,
}

/// What the Recordings browser is looking for, crossing IPC into the
/// `search_recordings` command (Story 42.3, FR-141, UX-DR50).
///
/// A deserialize-only input VM that mirrors Story 42.2's tauri-free
/// `RecordingFilter` field for field — the command maps this INTO it, exactly
/// as [`SearchFilterVm`] maps into `SearchFilter`, so the engine never learns
/// what a `Vm` is. Every predicate is optional and every one of them ANDs:
/// empty `query` is no text predicate at all, an empty `tags` list is
/// unrestricted, and a `null` bound is unbounded. The default value of every
/// field together is "every session, newest first", which is what the filter
/// row means before anyone touches it.
///
/// `#[serde(default)]` on each optional so the frontend may omit what it is not
/// filtering by, rather than being obliged to spell out seven `null`s to ask
/// the broadest question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingFilterVm {
    /// The user's free text over a session's title, participants, note, tags
    /// and custom-field values (trigram `MATCH` at ≥3 Unicode scalar values, an
    /// accelerated `LIKE` scan below that).
    pub query: String,
    /// Tags the session must carry, each matched hierarchically at the segment
    /// boundary (`client/acme` matches `client/acme/renewal`, never
    /// `client/acmecorp`). Several tags narrow; they never widen.
    #[serde(default)]
    pub tags: Vec<String>,
    /// A case-insensitive substring of the session's participants, or `null`
    /// for any.
    #[serde(default)]
    pub participant: Option<String>,
    /// Inclusive lower bound on the session's start (ms since the Unix epoch),
    /// or `null` for unbounded below.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub start_ts: Option<i64>,
    /// Inclusive upper bound on the session's start (ms since the Unix epoch),
    /// or `null` for unbounded above.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub end_ts: Option<i64>,
    /// Restrict to one durability state, as the wire word
    /// [`RecordingDurabilityState`] serialises to, or `null` for any state.
    #[serde(default)]
    pub durability: Option<String>,
    /// Restrict to sessions recorded under one destination profile, or `null`
    /// for any (including sessions recorded to a plain folder).
    #[serde(default)]
    pub profile_id: Option<String>,
    /// Cap on the number of hits, or `null` for the engine's default. The
    /// engine clamps this to `[1, 200]`, so a caller can never ask for an
    /// unbounded scan.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub limit: Option<i64>,
}

/// One session the Recordings browser lists (Story 42.3, FR-141, UX-DR50).
///
/// Story 42.2's `RecordingHit` is the engine's answer; this is the ROW, and the
/// two differ deliberately in three ways, each of which exists because the row
/// renders it and the engine does not carry it:
///
/// - **Paths, both of them.** `relative_path` is what the index stores and what
///   the surface prints as inert text where there is no file manager to reveal
///   into; `absolute_path` is the same session folder joined onto the EFFECTIVE
///   destination root by the command. No frontend surface ever joins a root and
///   a subfolder itself (AD-65) — Rust composes the destination in one place, so
///   Reveal cannot open a folder the recorder would not have written to.
/// - **Duration and size**, because the row shows them. `duration_ms` is derived
///   from the two stamps and `total_bytes` is summed from the session's
///   `recording_segments` rows; neither is a column on the session row, and
///   neither is worth a second copy that could go stale.
/// - **Tags decoded.** The engine hands back the stored JSON text (the column is
///   the truth, Story 42.1); a chip list is `string[]`, and decoding it once in
///   Rust means the frontend never parses a database column.
///
/// Not carried: the note, the participants and the custom fields. All three are
/// SEARCHABLE — a user remembers what they typed — but no row displays them,
/// and a hit that carried every column would be a `RecordingRow` wearing a
/// different name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingHitVm {
    /// The session's immutable identity — the id the row's Copy action puts on
    /// the clipboard, and the only handle that survives a Story 40.4 retitle.
    pub session_id: String,
    /// The session folder relative to the destination root, `/`-joined. What
    /// the row prints when there is no file manager to reveal into.
    pub relative_path: String,
    /// The absolute session folder — [`Self::relative_path`] resolved against
    /// the effective recordings destination by the command, and the Reveal
    /// target. Current by construction: Story 42.1's row follows the session
    /// through a retitle, so this is where the folder is NOW.
    pub absolute_path: String,
    /// The user's title for the session, or `null` for an untitled one (the row
    /// then leads with its date and folder, never a blank line).
    pub title: Option<String>,
    /// Session start, ms since the Unix epoch; `null` for a pre-21.5 manifest
    /// that carries no stamp.
    #[ts(type = "number | null")]
    pub started_ts: Option<i64>,
    /// Session end, ms since the Unix epoch; `null` while the session runs or
    /// when it was interrupted.
    #[ts(type = "number | null")]
    pub ended_ts: Option<i64>,
    /// How long the session ran, in milliseconds — `ended_ts - started_ts`, and
    /// `null` unless BOTH stamps are present. A session that has no end has no
    /// duration yet, and "now minus the start" would be a different fact
    /// (elapsed time) wearing this one's name, computed against a clock
    /// `keeper-core` deliberately does not read.
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    /// The session's total on-disk size in bytes, summed over its
    /// `recording_segments` rows. `0` for a session with no closed segment —
    /// which is the honest reading, not a missing value: nothing was written.
    #[ts(type = "number")]
    pub total_bytes: i64,
    /// How far the session's bytes have travelled, as epic 41's wire word — the
    /// row's durability glyph.
    pub durability: String,
    /// The session's tags, canonical (Story 42.5): what
    /// [`crate::notes::tags::normalise`] made of what the user typed, which is
    /// also exactly what the sidebar's tag node is called. Empty when the
    /// session has none, or when the column holds something that is not a JSON
    /// array of strings. The session's `manifest.json` still holds the user's
    /// own text — this is the index's reading of it.
    pub tags: Vec<String>,
    /// The absolute path of the file Play hands to the system handler: the
    /// session's first screen segment, or its first segment of any track when
    /// it captured no screen (an audio-only session still has something to
    /// play). `null` when the session has no segment row at all — nothing was
    /// written, so there is nothing to play, and the surface omits the action
    /// rather than opening the folder and calling that playback.
    pub playable_path: Option<String>,
}

/// One page of the Recordings browser, with the count behind it (Story 44.11,
/// FR-166).
///
/// **The page and the count are different numbers and this is where they stop
/// being confusable.** `search_recordings` has always stopped at
/// `recordings_fts::DEFAULT_LIMIT`, so `rows.len()` is 200 for an archive of
/// two hundred sessions and 200 for an archive of nine thousand. Until Story
/// 44.10 that at least LOOKED like a list that ended; windowed, a list that
/// stops at row 200 is indistinguishable on screen from a complete one. A
/// surface that counted the vector would have said "200 sessions" to somebody
/// with nine thousand of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingSearchVm {
    /// The sessions in this page, newest first.
    pub rows: Vec<RecordingHitVm>,
    /// How many sessions the filter matches in the whole archive, counted by
    /// SQL over the same predicates that selected `rows` — never `rows.len()`,
    /// and never a count of what a viewport rendered.
    pub total: u32,
}

/// What one [`RecordingNoteTargetVm`] is (Story 42.4, FR-142; widened by Story
/// 43.5, FR-150, AD-73).
///
/// **One vocabulary, because there is one question.** A surface holding a
/// target asks *what is this file and how should it be shown*, and every
/// answer keeper has — offer Preview, put a `<video>` in the note, put an
/// `<img>` there, serve the bytes over `keeper-recording://` — is a reading of
/// that one answer. Story 42.4 could answer it with `video` versus everything
/// else because the only consumer was a Preview item. Story 43.5 renders four
/// different elements, and the alternative to widening this was each surface
/// growing a private extension table: three tables that drift, so a file plays
/// in the note and offers no Preview in the panel.
///
/// Decided by extension in exactly one place —
/// [`crate::archive::recordings_fts::kind_for_file_name`] — and never by
/// reading the file. See that function for why sniffing is the wrong cost.
///
/// The order below is the vocabulary from most specific claim to least: the
/// three kinds keeper renders inline, then the two it can only act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RecordingNoteTargetKind {
    /// A file a `<video>` can play, and the one kind Preview is offered for.
    Video,
    /// A file an `<img>` can show.
    Image,
    /// A file an `<audio>` can play.
    Audio,
    /// Any file keeper does not render inline: the session's `manifest.json`, a
    /// PDF, an archive, a `.partial` from a rotation in flight, an
    /// extensionless dotfile. Reveal and Copy path, and nothing that claims to
    /// play it.
    ///
    /// Named for what keeper is claiming — that it is a file — and not for what
    /// it might contain. It is the catch-all, so an extension nobody
    /// anticipated is an attachment with working actions rather than a broken
    /// player: Story 42.6's rule that a dead player is worse than a plain link,
    /// applied to every file the tables above do not name.
    File,
    /// The session folder itself — the target of the note's `recording:` line.
    /// Never produced by extension: the caller knows a directory when it lists
    /// one.
    Folder,
}

/// One thing the reader of a recording note can act on: the session's folder,
/// or one file inside it (Story 42.4, FR-142, FR-145, AD-65).
///
/// **Why this VM exists at all.** A recording note names its recording only in
/// relative terms — `recording:` and each entry of `files:` — because FR-145
/// forbids an absolute path from ever being written into a file the user
/// syncs to their other machines. Relative text cannot be opened, and the
/// frontend is not allowed to make it openable by joining a destination root
/// onto it (AD-65). This is that join, done once in Rust, for every path a
/// note can name.
///
/// **The answer follows a retitle.** The list is composed from the session's
/// CURRENT folder — Story 42.1's row follows the session through a Story 40.4
/// rename — so a note written before a rename still opens the right thing,
/// while its own text goes on saying where the recording was when it was made.
/// That is the division of labour between the two: the note is the durable
/// human-readable record, and the index is the answer to "where is it now".
///
/// **Every entry existed a moment ago.** The list is read off the session
/// folder, so a surface that renders an action only for a target it was handed
/// has no path by which it can offer to open something that is not there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingNoteTargetVm {
    /// The target relative to the recordings destination root, `/`-joined —
    /// the same frame the note's own `recording:` and `files:` lines are
    /// written in, which is what lets a surface match one to the other without
    /// composing anything (FR-145).
    pub relative_path: String,
    /// The same target resolved against the EFFECTIVE recordings destination
    /// (Story 41.2). Only ever the argument of an action — never rendered as
    /// the note's text, and never written back into a note.
    pub absolute_path: String,
    /// What the target is: which element a note embeds it as, whether the
    /// panel offers Preview, and whether `keeper-recording://` will serve its
    /// bytes. One answer, so those three cannot disagree (AD-73).
    pub kind: RecordingNoteTargetKind,
}

/// The tag vocabulary a completion surface offers: every known tag, flat, with
/// its count (Story 42.5, FR-143).
///
/// **Flat, where [`crate::notes::vm::NoteTagTreeVm`] is nested, and that is the
/// only difference between them.** Both are projected from the same posting map
/// in the same snapshot, so they can never disagree about what a tag is or how
/// many things carry it. The tree exists because the sidebar renders a
/// hierarchy; this exists because the recording metadata card's tag field is a
/// plain text input, and the notes surface's existing affordance is a CodeMirror
/// `CompletionSource` that a plain input cannot consume. Forking the vocabulary
/// to give the card a completion is the one thing this story is about not doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TagVocabularyVm {
    /// Every tag path, ascending, each ancestor prefix its own entry.
    pub entries: Vec<TagVocabularyEntryVm>,
}

/// One tag in the vocabulary (Story 42.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TagVocabularyEntryVm {
    /// The full canonical tag path — `client/acme` — which is both what a
    /// completion inserts and what a `tag:` query matches.
    pub path: String,
    /// How many things carry this tag or anything under it, summed over BOTH
    /// producers: notes and recording sessions. The same number the sidebar
    /// node shows, because it is the same number.
    pub count: u32,
}

/// How many rows a folder card's lists show, folded and unfolded.
///
/// One pair for every list on the card rather than one pair per list: a user
/// setting this is answering "how much of this card do I want to read", and three
/// separate controls for one intent is how a settings pane stops being usable.
///
/// The setter clamps (folded `1..=50`, unfolded `10..=1000`) and returns the
/// effective VM, so the UI never shows an unsaved value; `unfolded` is also read
/// as never less than `folded`, because two independent rows can otherwise store
/// an "unfold" that reveals fewer rows than the fold it opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SyncListSettingsVm {
    /// Rows visible before unfolding (default 10).
    pub folded: u32,
    /// Rows visible after unfolding (default 100). Also the `LIMIT` the activity
    /// query runs with, so it bounds what the card can show at all.
    pub unfolded: u32,
}

/// Why a browsed directory has no entries — or that it has them (Story 43.8,
/// FR-153, AD-75).
///
/// **An empty folder and an absent drive are different facts, and the whole of
/// a user's trust in a file browser is that it never confuses them.** To
/// `read_dir` they are identical: an unplugged pendrive simply has no directory
/// there. A browser that reported both as "nothing here" would be telling
/// someone their recordings are gone every time they close their laptop lid,
/// which is the single fastest way to make a surface unusable.
///
/// The three failure answers are separate because their next steps are
/// different: reattach the drive, work out whose disk is mounted there, or
/// re-point the profile at a folder that moved. That is the same reasoning
/// [`RecordingVolumeState`] follows, and this is deliberately NOT that enum:
/// this one also has to carry "the folder is gone on a disk that is present",
/// which a volume state has no way to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FilesListingState {
    /// The directory was read. This is the only state whose entry list means
    /// anything, and an empty list under it is the honest "this folder is
    /// empty".
    Listed,
    /// The profile is on removable media (AD-48) and the media is not attached.
    /// A pause, not a fault: nothing on disk is missing.
    MediaAbsent,
    /// Something is mounted where this profile's volume lives and it is not
    /// provably that volume. Never listed — those files belong to a different
    /// disk, and showing them under this profile's name would misattribute
    /// somebody else's folder.
    MediaUnexpected,
    /// The directory is not on disk, on media that is attached. Moved, renamed
    /// or deleted outside keeper.
    Missing,
}

/// What sync knows about one browsed entry (Story 44.17, FR-173).
///
/// **The distinction this enum exists for is `excluded` against `waiting`.**
/// They look the same on screen if you collapse them — a file that is not
/// there yet — and they are opposite facts: one is arriving, the other never
/// will. A user watching an excluded file "sync" is a user waiting forever,
/// and that is the failure Story 44.17 was written from.
///
/// `notInRepository` is the third way a file fails to arrive, and it has its
/// own next step: the folder has no repository, so the first sync has to adopt
/// it before anything in it can travel. `unknown` exists for the same reason
/// [`FilesListingState`] separates an absent drive from an empty folder — when
/// the engine could not answer, every other value is a claim with nothing
/// behind it, and the two available guesses are "your work is safe" and "keep
/// waiting". Neither is honest.
///
/// Deliberately keeper's own vocabulary and not git's. `staged`, `untracked`
/// and `ahead` are answers to a question nobody browsing a folder is asking;
/// the sentence in [`FilesEntrySyncVm::detail`] is where the specific reason
/// goes, composed in Rust like every other sentence this surface renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FilesSyncStatusVm {
    /// In a repository, not excluded, and the engine has no outstanding work
    /// about it. For a folder, the same is true of everything inside it.
    Synced,
    /// The engine still has something to do about this entry before it is on
    /// the remote.
    Waiting,
    /// A pattern in this folder's own sync settings excludes it. It is listed
    /// *only* so it can say so; keeper's built-in noise corpus is not listed at
    /// all, because nobody chose those patterns and nobody is waiting on them.
    Excluded,
    /// The folder is not a git repository yet, so nothing in it is going
    /// anywhere until the first sync adopts it.
    NotInRepository,
    /// The engine was asked and could not say.
    Unknown,
}

/// One entry's sync mark, with the sentence that explains it.
///
/// A struct rather than two loose fields on [`FilesEntryVm`] because they are
/// one fact: the status decides the glyph, the detail is the words, and a row
/// that showed one without the other would be a row saying "waiting" with no
/// way to learn what for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilesEntrySyncVm {
    /// Which mark to render.
    pub status: FilesSyncStatusVm,
    /// The one sentence naming *why*, composed in Rust so the browser and the
    /// Pending list cannot come to word the same engine state differently.
    /// `None` when the status says everything there is to say — a synced file
    /// has no story.
    pub detail: Option<String>,
}

impl FilesEntrySyncVm {
    /// A mark with no sentence behind it.
    pub fn plain(status: FilesSyncStatusVm) -> Self {
        Self {
            status,
            detail: None,
        }
    }

    /// A mark and the sentence that explains it.
    pub fn explained(status: FilesSyncStatusVm, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: Some(detail.into()),
        }
    }
}

/// One entry's size, formatted once in Rust (Story 45.5, FR-178).
///
/// **Both halves, and the label is not derivable from the bytes by the
/// caller.** `bytes` is the exact count, for a tooltip, a sort, or a threshold
/// a viewer applies. `label` is [`crate::size::format_file_size`]'s answer,
/// computed here so that the Files pane, 45.2's unknown viewer, 45.3's delete
/// confirmation and any later surface all show the same string for the same
/// file. A frontend handed only `bytes` would divide by something, and the six
/// formatters this product had before this story are what that looks like after
/// a few years.
///
/// This type is only ever reached through an [`Option`]: see
/// [`FilesEntryVm::size`] for why a directory must not have one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileSizeVm {
    /// The exact byte count. `u64` on the wire is a JSON number, which loses
    /// precision above 2^53 — irrelevant for a file (9 PB) and stated here so
    /// nobody reaches for this field to carry something that is not a size.
    #[ts(type = "number")]
    pub bytes: u64,
    /// The rendered size, decimal, from [`crate::size::format_file_size`]. The
    /// only string any surface shows for this file's size.
    pub label: String,
}

impl FileSizeVm {
    /// Wrap a byte count with its one rendering.
    pub fn new(bytes: u64) -> Self {
        Self {
            bytes,
            label: crate::size::format_file_size(bytes),
        }
    }
}

/// A folder keeper itself put somewhere, so the pane can point at it (Story
/// 45.5, FR-178).
///
/// "Which of these forty folders is my vault" is a question the pane has all
/// the information to answer and, before this story, did not. The answer is
/// always **configuration**: the profile's own `notes.subfolder` and
/// `recordings.subfolder`. It is never a name. A user whose vault is called
/// `brain` or `Second Brain` or `zk` gets the same icon as one who kept the
/// default, and a user who names an ordinary folder `10-notes` does not get a
/// vault icon for it — which is the failure a hardcoded name produces on the
/// day somebody renames their vault and quietly loses the marker.
///
/// Deliberately NOT a member of [`RecordingNoteTargetKind`]. That vocabulary
/// answers "what is this thing" from the dirent and the file name (AD-73), and
/// it is the same answer on every machine. A folder's role is an answer about
/// *this installation's settings*, true of one folder on one profile and false
/// of a byte-identical copy of it elsewhere. Folding the two together would put
/// a machine-local fact into the classifier that 45.2's registry keys on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FilesFolderRoleVm {
    /// This folder is the profile's notes vault (`notes.subfolder`).
    NotesVault,
    /// This folder is where the profile's recordings are written
    /// (`recordings.subfolder`).
    Recordings,
}

/// The configured folder roles of one profile, as [`FilesEntryVm::new`] needs
/// them (Story 45.5).
///
/// Borrowed rather than owned, and passed per listing rather than per entry: a
/// directory of a thousand rows resolves its roles against two `&str` and
/// allocates nothing. `None` for either means the profile carries no such
/// configuration — not a vault, or not a recordings root — and no entry in it
/// can take that role.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FilesFolderRoles<'a> {
    /// The profile's `notes.subfolder`, profile-relative, exactly as stored.
    pub notes_subfolder: Option<&'a str>,
    /// The profile's `recordings.subfolder`, profile-relative, exactly as
    /// stored.
    pub recordings_subfolder: Option<&'a str>,
}

impl FilesFolderRoles<'_> {
    /// Which role, if any, the folder at `relative_path` plays.
    ///
    /// **Only a directory can hold a role**, so a *file* the user happens to
    /// name `notes` never takes the vault icon.
    ///
    /// **Exact match on the whole path, not a prefix.** A folder *inside* the
    /// vault is an ordinary folder; marking every descendant would make the
    /// marker useless at exactly the depth a person is scanning.
    ///
    /// **Compared case-insensitively, and with separators and edge slashes
    /// normalised.** The stored subfolder is whatever the user typed into the
    /// settings form — `Notes/`, `\Notes`, `NOTES` — while `relative_path` is
    /// what the dirent says, and on APFS and HFS+ those two differ in case for
    /// the same folder as a matter of course. A case-sensitive compare would
    /// silently drop the icon for the user who typed a capital, which is the
    /// same invisible failure as hardcoding the name.
    ///
    /// Notes wins a tie. A profile configured with the same subfolder for both
    /// is a misconfiguration the settings form should refuse, and if one ever
    /// reaches here it must produce one deterministic answer rather than
    /// whichever branch was written first.
    pub fn role_of(&self, relative_path: &str, is_dir: bool) -> Option<FilesFolderRoleVm> {
        if !is_dir {
            return None;
        }
        let matches = |configured: Option<&str>| {
            configured.is_some_and(|configured| same_folder_path(configured, relative_path))
        };
        if matches(self.notes_subfolder) {
            Some(FilesFolderRoleVm::NotesVault)
        } else if matches(self.recordings_subfolder) {
            Some(FilesFolderRoleVm::Recordings)
        } else {
            None
        }
    }
}

/// Whether two profile-relative folder paths name the same folder.
///
/// Split out so the normalisation rule is one function with one set of tests
/// rather than a chain of `trim`s inlined at a comparison — the shape that
/// grows a fifth `trim_matches` nobody notices is missing on the other side.
/// An empty configured subfolder matches nothing: the profile root is not a
/// vault, `NotesConfig::validate` refuses an empty subfolder, and returning
/// `true` for it here would mark every entry of an empty-string listing.
fn same_folder_path(left: &str, right: &str) -> bool {
    let normalise = |path: &str| {
        path.replace('\\', "/")
            .trim_matches('/')
            .to_ascii_lowercase()
    };
    let left = normalise(left);
    !left.is_empty() && left == normalise(right)
}

/// One entry in a browsed synced folder (Story 43.8, FR-153, FR-145, AD-65,
/// AD-73).
///
/// **Both paths, for two different jobs.** `relative_path` is what the surface
/// shows and what it hands back to list this entry's children — the frontend
/// never composes it, it echoes one this VM already carried, which is AD-65
/// applied to a tree that is expanded a node at a time. `absolute_path` is only
/// ever the argument of an action (reveal, copy path, open with the system
/// handler) and is composed in Rust from the profile's own root, so no synced
/// folder's location is ever assembled in TypeScript.
///
/// **The kind is the one attachment vocabulary** (AD-73). A `.mov` in a synced
/// folder is the same kind of thing as a `.mov` a note embeds, and giving the
/// browser a private extension table is how the two surfaces would come to
/// disagree about what a file is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilesEntryVm {
    /// The entry's own name, with no path in it — what the row renders.
    pub name: String,
    /// The entry's path relative to the profile root, `/`-joined. Never
    /// absolute: this is the string that appears on screen, and FR-145's rule
    /// against writing an absolute path into a synced artefact is the same rule
    /// that keeps a home-directory name out of a screenshot.
    pub relative_path: String,
    /// The same entry resolved against the profile's local path, composed in
    /// Rust. Only ever an action's argument.
    pub absolute_path: String,
    /// What this entry is. [`RecordingNoteTargetKind::Folder`] exactly when the
    /// dirent said directory; every other value is decided by extension in
    /// [`crate::archive::recordings_fts::kind_for_file_name`].
    pub kind: RecordingNoteTargetKind,
    /// Whether sync has this entry, and why not (Story 44.17, FR-173).
    pub sync: FilesEntrySyncVm,
    /// How big this entry is, rendered once in Rust (Story 45.5, FR-178).
    ///
    /// **`None` for a directory, and that is the point of the `Option`.** A
    /// folder has no size keeper is willing to claim — computing one means
    /// walking the tree, which a listing must never do — and a folder rendered
    /// as "0 bytes" says something false about every folder that has anything
    /// in it. Modelled as an absence rather than a zero so the natural
    /// refactor, making this field non-optional and defaulting it, has to
    /// delete a documented contract to happen.
    ///
    /// Also `None` when the entry's metadata could not be read at all — a
    /// broken symlink, a file removed between the `read_dir` and the `stat`.
    /// An unknown size and an absent size render identically, which is correct:
    /// in both cases keeper does not know and says nothing rather than
    /// guessing.
    pub size: Option<FileSizeVm>,
    /// Whether keeper itself put something here (Story 45.5, FR-178).
    ///
    /// `Some` only for the folder the profile's configuration names as its
    /// notes vault or its recordings root, and only ever from that
    /// configuration — never from the folder's name. See
    /// [`FilesFolderRoleVm`].
    pub folder_role: Option<FilesFolderRoleVm>,
    /// Whether the Files surface may change or remove this entry, and why not
    /// (Story 45.3, FR-175, AD-89).
    ///
    /// **This is the LOCATION question and only that.** It answers "is this
    /// path somewhere keeper writes" — inside a reachable notes vault, not the
    /// vault directory itself, not a folder. Whether the *format* can be
    /// written is a separate question the viewer registry answers (Story 45.2),
    /// and an edit needs both to say yes: a PDF in a writable folder is not
    /// editable, and a Markdown file outside a vault is not either.
    ///
    /// Carried on the entry rather than probed for, because the surface must
    /// never offer an action that will fail. The pane renders the reason where
    /// the action would have been.
    pub write: FilesWriteVm,
}

/// Everything [`FilesEntryVm::new`] needs, named at the call site.
///
/// A struct rather than eight positional parameters, and not only because
/// clippy counts: three of them are strings in a row (`name`,
/// `relative_path`, `absolute_path`), so transposing two of them compiles,
/// passes every type check, and produces a row that renders one file's name
/// over another file's path. A field name is the cheapest defence there is
/// against that, and this constructor is called from a loop over a whole
/// directory where the mistake would be uniform and therefore plausible.
pub struct FilesEntryFacts<'a> {
    /// The dirent's own name, which is what the row shows.
    pub name: String,
    /// Profile-relative, and the only path that may reach a note (FR-145).
    pub relative_path: String,
    /// An action argument for Reveal and Open With; never rendered (AD-65).
    pub absolute_path: String,
    pub is_dir: bool,
    pub sync: FilesEntrySyncVm,
    /// `None` when the metadata could not be read. A directory's is discarded.
    pub size_bytes: Option<u64>,
    /// The profile's configuration, not the folder's name (Story 45.5).
    pub roles: FilesFolderRoles<'a>,
    /// The location verdict `keeper_sync::files_write` already reached.
    pub write: FilesWriteVm,
}

impl FilesEntryVm {
    /// Project one listed entry, applying the one attachment vocabulary.
    ///
    /// `is_dir` comes from the dirent rather than from the name, because a
    /// directory called `notes.md` exists and an extension table would call it
    /// a document and offer to open it in a text editor.
    ///
    /// `size_bytes` is the dirent's own `len()` for a regular file and `None`
    /// for everything else. A directory's is DISCARDED here rather than
    /// trusted: `std::fs::metadata` reports a nonzero length for a directory on
    /// most filesystems (the size of its own dirent block), which is a number
    /// about the folder's bookkeeping and not about its contents. Showing it
    /// would be worse than showing nothing, so this constructor drops it
    /// unconditionally — a caller that passes one for a directory cannot leak
    /// it (Story 45.5).
    ///
    /// `roles` is the profile's configuration, not the folder's name.
    ///
    /// `write` is the location verdict `keeper_sync::files_write` already
    /// reached for this path. Passed in rather than derived: this crate is
    /// deliberately `keeper-sync`-free (AD-40), so it cannot see a profile's
    /// vault configuration, and a second opinion about where keeper may write
    /// is the last thing this projection should hold.
    pub fn new(facts: FilesEntryFacts<'_>) -> Self {
        let FilesEntryFacts {
            name,
            relative_path,
            absolute_path,
            is_dir,
            sync,
            size_bytes,
            roles,
            write,
        } = facts;
        let kind = if is_dir {
            RecordingNoteTargetKind::Folder
        } else {
            crate::archive::recordings_fts::kind_for_file_name(&name)
        };
        Self {
            name,
            kind,
            absolute_path,
            size: if is_dir {
                None
            } else {
                size_bytes.map(FileSizeVm::new)
            },
            folder_role: roles.role_of(&relative_path, is_dir),
            relative_path,
            sync,
            write,
        }
    }
}

/// One directory of one synced folder, as the Files tab renders it (Story 43.8,
/// FR-153).
///
/// **`entries` is `None` for every state but [`FilesListingState::Listed`], and
/// that is a contract rather than a convenience.** An empty array and a null
/// are different in TypeScript, so a surface that renders `entries.length === 0`
/// as "this folder is empty" cannot accidentally say it about a drive that is
/// out — it has to unwrap first, and unwrapping is where it meets the state.
/// Carrying `[]` for an unreadable folder would make the wrong rendering the
/// path of least resistance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilesListingVm {
    /// The profile this listing came from, echoed back so a surface with
    /// several expansions in flight can attribute a late answer.
    pub profile_id: String,
    /// The profile-relative directory that was listed; `""` is the profile
    /// root. Echoed for the same reason.
    pub subpath: String,
    /// Whether the directory was read, and if not, why.
    pub state: FilesListingState,
    /// The directory's children, `Some` exactly when `state` is
    /// [`FilesListingState::Listed`].
    pub entries: Option<Vec<FilesEntryVm>>,
    /// The one sentence to show alongside this listing, composed in Rust: why a
    /// non-`listed` state has no entries, or — under `listed` — that the list
    /// was capped. One field rather than two because the surface has one place
    /// to put a sentence, and because the folder-open action words "this folder
    /// is not reachable" from the same function, so the two can never disagree.
    /// `None` when there is nothing to explain.
    pub detail: Option<String>,
    /// Whether the listing was cut short at the shell's cap. `false` for every
    /// state that has no entries — there was nothing to cut.
    pub truncated: bool,
    /// Whether keeper may create a file in the directory that was listed, and
    /// why not (Story 45.3, FR-176, AD-89).
    ///
    /// The directory's own answer, which is a different question from any
    /// entry's: the vault root can be created in and cannot be deleted, and a
    /// folder outside the vault can be listed and cannot be created in. Carried
    /// on the listing so the "New file" control is absent-with-a-reason rather
    /// than present-and-failing, and so nothing has to ask a second time.
    ///
    /// Refused for every state but [`FilesListingState::Listed`]: a folder
    /// keeper could not read is not a folder keeper will write into.
    pub write: FilesWriteVm,
}

/// Whether keeper may write at one place, and the sentence saying why not
/// (Story 45.3, FR-175, FR-176, AD-89).
///
/// **A field on the listing rather than an error from an attempt.** The rule
/// the story turns on is that a file outside a vault "can be listed and viewed
/// but not written: the surface says why rather than offering an action that
/// will fail". That is only expressible if the surface knows before it renders
/// the control, so the verdict rides with the data.
///
/// `reason` is a whole sentence composed by
/// `keeper_sync::files_write::WriteRefusal`, rendered verbatim and never
/// paraphrased in TypeScript — the same rule [`FilesEntrySyncVm::detail`]
/// follows, for the same reason: a second copy of these words is a second copy
/// that will be edited once.
///
/// `reason` is `Some` exactly when `writable` is false. A refusal with no
/// reason would be a control that vanished with no explanation, which is the
/// failure this whole field exists to prevent.
///
/// `caveat` is the third state Story 46.14 introduced, and it is not a second
/// refusal: AD-102 gave keeper a second writer for files no vault holds, so a
/// location can now be writable *and* unmanaged. The caveat is what the surface
/// shows standing, BEFORE the first keystroke — an edit that quietly does less
/// than the vault path does is strictly worse than the refusal it replaces.
/// `Some` only when `writable`; the two are never both set.
///
/// `caveat_short` is the same fact in one sentence (Story 53.3, FR-318), and it
/// is a SECOND field rather than a replacement because the surface shows one and
/// then the other: the short form stands before the first keystroke and the full
/// one is a press away. Composed in Rust for the same reason the full one is —
/// the webview renders both verbatim, and a webview that clipped the long one to
/// fit would be paraphrasing exactly the clause that names what is missing.
/// `Some` exactly when `caveat` is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilesWriteVm {
    /// Whether keeper will write here.
    pub writable: bool,
    /// Why not, as a whole sentence. `None` exactly when `writable`.
    pub reason: Option<String>,
    /// What keeper will not do for this file even though it will write it, as
    /// a whole sentence. `None` when keeper manages the file, and always
    /// `None` when `writable` is false.
    pub caveat: Option<String>,
    /// The same fact in one sentence, for a surface that folds the caveat away
    /// (Story 53.3). `Some` exactly when `caveat` is.
    pub caveat_short: Option<String>,
}

impl FilesWriteVm {
    /// keeper writes here and manages what it writes.
    pub fn allowed() -> Self {
        Self {
            writable: true,
            reason: None,
            caveat: None,
            caveat_short: None,
        }
    }

    /// keeper writes here through AD-102's second writer, and these sentences
    /// say what that costs — the whole of it, and the one line a folded surface
    /// shows in its place (Story 53.3).
    ///
    /// Composed by `keeper_sync::files_write::WriteScope::unmanaged_caveat` and
    /// `unmanaged_caveat_short`, rendered verbatim, exactly as `reason` is. Both
    /// at once and from one call, so a row cannot carry the standing fact in a
    /// form the surface it reaches has folded away.
    pub fn unmanaged(caveat: impl Into<String>, short: impl Into<String>) -> Self {
        Self {
            writable: true,
            reason: None,
            caveat: Some(caveat.into()),
            caveat_short: Some(short.into()),
        }
    }

    /// keeper does not write here, and this is why.
    pub fn refused(reason: impl Into<String>) -> Self {
        Self {
            writable: false,
            reason: Some(reason.into()),
            caveat: None,
            caveat_short: None,
        }
    }

    /// Project a `Result` from the write-scope decision, which is how the
    /// create path holds this fact.
    ///
    /// Deliberately has no unmanaged arm: a `Result` carries two states and
    /// the third one is a different question. The entry path answers it with
    /// `keeper_sync::files_write::WriteOwner` and picks the constructor
    /// itself, so a caller cannot reach [`Self::allowed`] for a file AD-102
    /// says needs a caveat by routing through this.
    pub fn from_verdict<T, E: std::fmt::Display>(verdict: &Result<T, E>) -> Self {
        match verdict {
            Ok(_) => Self::allowed(),
            Err(refusal) => Self::refused(refusal.to_string()),
        }
    }
}

/// Which trash one file in a delete selection is bound for (Story 46.14,
/// AD-102, NFR-30).
///
/// **An input to [`FilesDeletePlanVm::compose`] and never a field on the VM**,
/// so it is deliberately not `TS`-exported: the frontend renders the sentence
/// this decides, and a second reading of the same fact in TypeScript is a
/// second reading that will eventually disagree. `keeper_sync`'s
/// `WriteOwner` is where the fact is actually decided; this is that decision
/// crossing into the crate that words it, since `keeper-core` must never name
/// `keeper-sync` (AD-40).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesDeleteDestinationVm {
    /// `<vault>/.keeper/trash/`, plus a recorded removal in this folder's
    /// history.
    VaultTrash,
    /// The operating system's own trash. No vault trash exists to reach and no
    /// note history records it.
    SystemTrash,
}

/// One thing in a delete selection that keeper will not delete, and why
/// (Story 45.3, FR-175).
///
/// Named, because a selection that silently shrinks between the click and the
/// confirmation is a selection that lied about what it was going to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilesDeleteRefusalVm {
    /// The entry's profile-relative path — what the surface already shows.
    pub relative_path: String,
    /// The whole sentence, composed by `keeper_sync::files_write`.
    pub reason: String,
}

/// What a delete would do, worded before it is done (Story 45.3, FR-175,
/// UX-DR66).
///
/// **Composed in Rust, and that is the story's requirement rather than a house
/// habit.** The confirmation has to name the file and say whether it syncs, and
/// both facts live here: the file list is the one the command will act on, and
/// the sync consequence is derived from the same [`FilesSyncStatusVm`] the row
/// already shows. A confirmation assembled in TypeScript from a count and a
/// glyph would be a second, unverified reading of the engine's answer, and the
/// one place a wrong reading costs a file.
///
/// Built by [`FilesDeletePlanVm::compose`], which is pure — so every sentence
/// below is asserted on any machine, not only on the one where the Tauri shell
/// builds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilesDeletePlanVm {
    /// The profile-relative paths that would go, in the order they were asked
    /// for. Empty means there is nothing to confirm.
    pub files: Vec<String>,
    /// The heading: names the one file, counts the many.
    pub question: String,
    /// What deleting these means for sync — the story's "says whether it
    /// syncs", worded for this exact set rather than in general.
    pub consequence: String,
    /// Where the bytes go. A destructive confirmation that does not say a copy
    /// is kept reads as an erasure, and this one is not.
    pub recovery: String,
    /// What was asked for and will not go, each named.
    pub refusals: Vec<FilesDeleteRefusalVm>,
}

impl FilesDeletePlanVm {
    /// Word a delete over a set of entries and the reasons the rest were
    /// dropped.
    ///
    /// `files` gives, for each deletable profile-relative path, what sync says
    /// about it right now and where its bytes are about to go.
    ///
    /// **The destination is per file and not per call, because a selection can
    /// hold both.** A person can select a note and the `AGENTS.md` beside the
    /// vault in one drag, and after Story 46.14 those two go to two different
    /// trashes with two different ways of getting them back. Wording the
    /// commoner one and hoping is how a confirmation becomes a lie.
    ///
    /// **[`FilesSyncStatusVm::Unknown`] counts as syncing, and says so.** The
    /// two available guesses are "this deletion stays on this machine" and
    /// "this deletion travels", and only one of them is safe to be wrong
    /// about. Silently picking the quiet one would be the same lie
    /// [`FilesSyncStatusVm::Unknown`] was introduced to refuse.
    pub fn compose(
        profile_name: &str,
        files: Vec<(String, FilesSyncStatusVm, FilesDeleteDestinationVm)>,
        refusals: Vec<FilesDeleteRefusalVm>,
    ) -> Self {
        let total = files.len();
        let unclear = files
            .iter()
            .filter(|(_, status, _)| *status == FilesSyncStatusVm::Unknown)
            .count();
        let travels = files
            .iter()
            .filter(|(_, status, _)| {
                matches!(
                    status,
                    FilesSyncStatusVm::Synced
                        | FilesSyncStatusVm::Waiting
                        | FilesSyncStatusVm::Unknown
                )
            })
            .count();
        let local = total - travels;

        let question = match files.first() {
            _ if total == 0 => "There is nothing here keeper can delete.".to_owned(),
            Some((path, _, _)) if total == 1 => format!("Delete {path}?"),
            _ => format!("Delete {total} files?"),
        };

        let consequence = if total == 0 {
            String::new()
        } else if unclear == total {
            let (subject, claim, object) = if total == 1 {
                ("this file's", "it syncs", "it")
            } else {
                ("these files'", "they sync", "them")
            };
            format!(
                "keeper could not read {subject} sync state, so it has assumed {claim} and \
                 that deleting removes {object} from every machine that syncs {profile_name}."
            )
        } else if local == 0 {
            let head = if total == 1 {
                "This file syncs".to_owned()
            } else {
                format!("These {total} files sync")
            };
            let caveat = unclear_caveat(unclear);
            format!(
                "{head}, so deleting {} here removes {} from every machine that syncs \
                 {profile_name}.{caveat}",
                if total == 1 { "it" } else { "them" },
                if total == 1 { "it" } else { "them" },
            )
        } else if travels == 0 {
            if total == 1 {
                "This file does not sync, so this removes it from this machine only.".to_owned()
            } else {
                format!(
                    "None of these {total} files sync, so this removes them from this \
                     machine only."
                )
            }
        } else {
            let caveat = unclear_caveat(unclear);
            format!(
                "{travels} of these {total} files sync, so deleting them removes them from \
                 every machine that syncs {profile_name}; the other {local} do not and go \
                 from this machine only.{caveat}"
            )
        };

        // Where the bytes go, and Story 46.14's correction: until AD-102 there
        // was one trash and one sentence, and that sentence promised the
        // vault's trash and this folder's history. For a file no vault holds,
        // both halves of it are lies — there is no vault trash to reach and no
        // note history to record in. So the destination is counted the same way
        // the sync consequence is, and a mixed selection says both.
        let to_system = files
            .iter()
            .filter(|(_, _, destination)| *destination == FilesDeleteDestinationVm::SystemTrash)
            .count();
        let recovery = match (total, to_system) {
            (0, _) => String::new(),
            // Every file is a note.
            (1, 0) => "keeper moves it into the vault's trash rather than erasing it, and the \
                       removal is recorded in this folder's history."
                .to_owned(),
            (_, 0) => "keeper moves them into the vault's trash rather than erasing them, and \
                       the removals are recorded in this folder's history."
                .to_owned(),
            // Nothing here is a note: there is no vault trash and no note
            // history, and saying otherwise is the defect this arm exists for.
            (1, 1) => "keeper moves it to this computer's trash rather than erasing it. It is \
                       not a note, so there is no vault trash and no note history for it — \
                       putting it back is your file manager's Put Back."
                .to_owned(),
            (total, system) if system == total => {
                "keeper moves them to this computer's trash rather than erasing them. None of \
                 them are notes, so there is no vault trash and no note history for them — \
                 putting them back is your file manager's Put Back."
                    .to_owned()
            }
            // Both, which one drag over a vault and the folder beside it
            // produces.
            (total, system) => format!(
                "Nothing is erased: {} of these {total} go to the vault's trash and are \
                 recorded in this folder's history, and the other {system} go to this \
                 computer's trash, because they are not notes.",
                total - system
            ),
        };

        Self {
            files: files.into_iter().map(|(path, _, _)| path).collect(),
            question,
            consequence,
            recovery,
            refusals,
        }
    }
}

/// The clause appended when some of the set's sync state could not be read.
///
/// Separate because it hangs off three different sentences and a fourth copy of
/// it would be the one that eventually says something different.
fn unclear_caveat(unclear: usize) -> String {
    if unclear == 0 {
        return String::new();
    }
    format!(
        " keeper could not read the sync state of {unclear} of them, and has counted \
         {} as syncing because that is the reading that assumes more rather than less.",
        if unclear == 1 { "it" } else { "them" }
    )
}

/// What a delete actually did (Story 45.3, FR-175).
///
/// **Partial success is a real outcome and is reported rather than thrown.** A
/// file can vanish between the confirmation and the command — a sync pull, the
/// user's own Finder window — and failing the whole call would leave the other
/// four deleted with an error on screen saying nothing happened. Each path
/// answers for itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilesDeleteReceiptVm {
    /// The profile-relative paths that are now in the trash.
    pub deleted: Vec<String>,
    /// What did not go, each named with the reason.
    pub refusals: Vec<FilesDeleteRefusalVm>,
}

/// The most files an export receipt names before it starts counting.
///
/// A receipt is read once, in a toast. Three names tell somebody which files
/// they are and a fourth line of filenames tells them nothing they will read —
/// but a bare count tells them nothing at all, which is why this is a cap and
/// not a switch to counting.
const NAMED_IN_A_RECEIPT: usize = 3;

/// `a, b and 2 more`, or `""` for nothing.
fn named_list(items: &[String]) -> String {
    let shown = items
        .iter()
        .take(NAMED_IN_A_RECEIPT)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    match items.len().checked_sub(NAMED_IN_A_RECEIPT) {
        Some(rest) if rest > 0 => format!("{shown} and {rest} more"),
        _ => shown,
    }
}

/// What an export actually put in the folder the user picked (Story 45.21,
/// FR-199).
///
/// **What did not go is as much of the receipt as what did.** A note whose
/// embed has been moved exports without that file, and an export that reported
/// only its successes would be one nobody could rely on for the thing an export
/// is for — handing the document to somebody outside keeper. So the two kinds
/// of not-carried are separate lists with separate remedies: a file keeper
/// looked for and could not find, and a note the export deliberately did not
/// follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportReceiptVm {
    /// Absolute path of the one thing that now exists in the destination: the
    /// copied file, or the folder a note's export was written into.
    ///
    /// The only absolute path here, and it is what Reveal points at. FR-145
    /// forbids an absolute path in a *synced artefact*; this is a receipt for
    /// something the user just picked a location for, and naming it is the
    /// whole point.
    pub path: String,
    /// Every file written, relative to the destination folder, in copy order.
    pub written: Vec<String>,
    /// Embed targets that named a file and resolved to nothing on disk,
    /// spelled as the note spells them.
    pub missing: Vec<String>,
    /// Embed targets that named another note. Not carried, because following
    /// one would make an export of a note an export of an unbounded set of
    /// them.
    pub notes: Vec<String>,
    /// The finished sentence the surface shows, worded here so the words are
    /// asserted rather than assembled in a component.
    pub summary: String,
}

impl ExportReceiptVm {
    /// The receipt for one file copied out as itself.
    pub fn file(destination: &str, path: String, name: &str) -> Self {
        Self {
            path,
            written: vec![name.to_owned()],
            missing: Vec::new(),
            notes: Vec::new(),
            summary: format!("Exported {name} to {destination}."),
        }
    }

    /// The receipt for a note and the files it embeds.
    ///
    /// `written` is the engine's own list, note first, so the attachment count
    /// is derived from what actually landed rather than from what was planned
    /// — a receipt that counted the plan would say "and 2 attachments" about an
    /// export that copied one.
    ///
    /// **The plan arrives whole rather than as two `Vec<String>`s**, and that is
    /// a deliberate defence rather than tidiness. This is called from
    /// `keeper/src/notes_ipc.rs`, which does not compile on Linux, so a call
    /// site that passed `missing` where `notes` belongs would type-check for
    /// nobody and be found by a user reading "keeper could not find 1 file this
    /// note embeds: Other Note". One argument cannot be swapped with itself.
    pub fn note(path: String, note_name: &str, written: Vec<String>, plan: NoteExportPlan) -> Self {
        let NoteExportPlan {
            attachments: _,
            missing,
            notes,
        } = plan;
        let carried = written.len().saturating_sub(1);
        let with = match carried {
            0 => String::new(),
            1 => " and 1 attachment".to_owned(),
            many => format!(" and {many} attachments"),
        };
        let mut summary = format!("Exported {note_name}{with} to {path}.");
        if !missing.is_empty() {
            let names = named_list(&missing);
            let count = missing.len();
            summary.push_str(&if count == 1 {
                format!(" keeper could not find 1 file this note embeds, so it was not carried: {names}.")
            } else {
                format!(" keeper could not find {count} files this note embeds, so they were not carried: {names}.")
            });
        }
        if !notes.is_empty() {
            let names = named_list(&notes);
            summary.push_str(&if notes.len() == 1 {
                format!(" One embedded note was not carried — export it separately: {names}.")
            } else {
                format!(" Embedded notes were not carried — export them separately: {names}.")
            });
        }
        Self {
            path,
            written,
            missing,
            notes,
            summary,
        }
    }
}

/// Which layer file a settings value came from (Epic 46, AD-98, AD-99),
/// projected for the surface that answers "where did this value come from?".
///
/// A separate enum from [`crate::config::LayerTier`] rather than a re-export,
/// and the mapping below is an exhaustive `match`. That is the point: the layer
/// order is a user-visible contract, so adding a tier must break this file —
/// which compiles on Linux — rather than silently reaching the frontend as a
/// variant no surface has a sentence for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ConfigTierVm {
    /// `~/.keeper/keeper.toml` — this user, every machine, every folder.
    UserGlobal,
    /// `~/.keeper/keeper.<host>.toml` — this user, this machine only.
    UserGlobalMachine,
    /// `<main>/.keeper/keeper.toml` — the designated main sync folder, shared
    /// with every machine that syncs it.
    MainShared,
    /// `<main>/.keeper/keeper.<host>.toml` — the main sync folder, this machine.
    MainMachine,
    /// `<folder>/.keeper/keeper.toml` — one folder's own settings, shared.
    FolderShared,
    /// `<folder>/.keeper/keeper.<host>.toml` — one folder, this machine.
    FolderMachine,
}

/// One settings key whose value is decided by a file rather than by the app
/// (Epic 46, AD-98).
///
/// The presence of an entry here is the whole promise AD-98 makes: a control
/// whose key appears in this list would be overridden, and says so, instead of
/// accepting an edit that the next read throws away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConfigOverrideVm {
    /// The settings-table key, spelled as the file and the registry spell it
    /// (`recording.fps`, `debug.mode`). This is what a control matches itself
    /// against, so it is the raw key and never a prettified label.
    pub key: String,
    /// Which layer won it.
    pub tier: ConfigTierVm,
    /// The absolute path of the file that set it. Shown verbatim: the user
    /// asked for a file they can edit, and the only useful answer to "where"
    /// is a path they can paste into an editor.
    pub path: String,
    /// The folder this layer belongs to, when it is a folder layer.
    pub folder: Option<String>,
    /// The finished phrase the surface renders after the key — "your settings
    /// file, for every machine and folder". Composed here so the wording is
    /// asserted in a test rather than assembled in a component.
    pub source: String,
}

/// One thing that is wrong with the layer files, named out loud (Epic 46).
///
/// **Faults are the reason this VM is not just a list of overrides.** Every
/// failure mode of a settings file is silent by nature: a malformed file sets
/// nothing, a `[settings]` block in a folder that may not carry one sets
/// nothing, and a `mainSyncFolder` with a typo in it disables an entire layer
/// of the stack while looking exactly like a file that works. Each of those
/// reaches the user as an entry here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConfigFaultVm {
    /// A stable machine name for what went wrong (`malformed`,
    /// `settingsInNonMainFolder`, `mainFolderNotAProfile`, …). Not branched on
    /// by any surface — it exists so a test can pin *which* fault was raised
    /// rather than matching on prose.
    pub kind: String,
    /// The file the fault is about.
    pub path: String,
    /// The whole line the surface renders, composed by the config layer itself
    /// (`<path>[:line]: <message>`). Rendered verbatim, the way
    /// `SyncGitVm.problem` is: one spelling of one fact.
    pub summary: String,
}

impl ConfigFaultVm {
    /// A fault the sync engine's folder tier raised about one folder's own
    /// `.keeper/*.toml` — including "the value you just set here was dropped
    /// because that file owns it", which is AD-98's promise applied to the
    /// settings that travel with a folder.
    ///
    /// **`&Path` and `String`, not two `String`s.** The one call site is in
    /// `keeper/src/ipc.rs`, which does not compile on Linux, so a swapped pair
    /// of same-typed arguments would type-check for nobody and be discovered by
    /// a user reading a path where a reason belongs. Two different types cannot
    /// be swapped. (`ExportReceiptVm::note` takes the same precaution for the
    /// same reason.)
    pub fn folder(path: &std::path::Path, message: String) -> Self {
        Self {
            kind: "folder".to_owned(),
            path: path.display().to_string(),
            summary: format!("{}: {message}", path.display()),
        }
    }
}

/// Everything the Settings surface needs to answer "where did this value come
/// from, and is anything about my settings files broken?" (Epic 46, AD-98).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConfigLayersVm {
    /// Every key a file decides, in key order.
    pub overrides: Vec<ConfigOverrideVm>,
    /// Every problem found while loading the layers, in the order they were
    /// found — load-time faults first, then the ones raised after the sync
    /// engine opened.
    pub faults: Vec<ConfigFaultVm>,
    /// The main sync folder `~/.keeper/keeper.toml` designates, if it named
    /// one. Present even when it turned out to be wrong — the accompanying
    /// fault says so, and a field that blanked itself on a bad value would hide
    /// the typo the user has to fix.
    pub main_folder: Option<String>,
    /// The sentence the section leads with, covering both counts.
    pub summary: String,
}

impl ConfigLayersVm {
    /// Project the installed layer stack.
    ///
    /// Pure, and takes its three inputs rather than reading the process-global
    /// config: the shell crate that calls this does not compile on Linux
    /// (AD-55/AD-56), so a projection that reached for global state would be
    /// testable on exactly one machine. Every sentence below is asserted in
    /// this crate's own test module instead.
    pub fn new(
        overrides: Vec<(String, crate::config::LayerSource)>,
        faults: Vec<crate::config::LayerFault>,
        main_folder: Option<std::path::PathBuf>,
    ) -> Self {
        let overrides: Vec<ConfigOverrideVm> = overrides
            .into_iter()
            .map(|(key, source)| {
                let tier = ConfigTierVm::of(source.tier);
                ConfigOverrideVm {
                    key,
                    tier,
                    path: source.path.display().to_string(),
                    source: tier.phrase(source.folder.as_deref()),
                    folder: source.folder,
                }
            })
            .collect();
        let faults: Vec<ConfigFaultVm> = faults
            .into_iter()
            .map(|fault| ConfigFaultVm {
                kind: fault_kind(&fault.kind).to_owned(),
                path: fault.path.display().to_string(),
                // `summary()`, never `Display`: the two are different forms on
                // purpose. `Display` is the log form and is deliberately
                // multi-line for a malformed file, because `toml`'s own error
                // carries the offending input and a caret and flattening it
                // throws away the only thing that locates the mistake. A
                // settings pane wants the one-line form.
                summary: fault.summary(),
            })
            .collect();
        let summary = layers_summary(overrides.len(), faults.len());
        Self {
            overrides,
            faults,
            main_folder: main_folder.map(|path| path.display().to_string()),
            summary,
        }
    }

    /// Fold in the faults the sync engine's own folder tier raised.
    ///
    /// A second entry point rather than a fourth argument to [`Self::new`],
    /// because the two fault sources cannot meet any earlier than this. AD-40
    /// makes `keeper-sync` deliberately `keeper-core`-free and `keeper-core`
    /// deliberately `keeper-sync`-free — `bun run check:core-sync-free` asserts
    /// both edges — so the only place that can see a `FolderFault` and a
    /// `LayerFault` at once is the shell. The join is one `map` there and the
    /// wording stays here, where it is tested.
    ///
    /// The summary is recomputed rather than appended to: a count that stopped
    /// counting halfway is worse than no count.
    pub fn with_folder_faults(mut self, mut faults: Vec<ConfigFaultVm>) -> Self {
        self.faults.append(&mut faults);
        self.summary = layers_summary(self.overrides.len(), self.faults.len());
        self
    }
}

impl ConfigTierVm {
    /// The wire tier for a loaded layer's tier.
    fn of(tier: crate::config::LayerTier) -> Self {
        use crate::config::LayerTier;
        match tier {
            LayerTier::UserGlobal => Self::UserGlobal,
            LayerTier::UserGlobalMachine => Self::UserGlobalMachine,
            LayerTier::MainShared => Self::MainShared,
            LayerTier::MainMachine => Self::MainMachine,
            LayerTier::FolderShared => Self::FolderShared,
            LayerTier::FolderMachine => Self::FolderMachine,
        }
    }

    /// How this layer is described to the person reading Settings.
    ///
    /// Two axes, both of which the user has to be able to tell apart to know
    /// which file to open: *whose* file it is (yours, the main folder's, this
    /// folder's) and *how far it reaches* (every machine, or only this one).
    /// A folder layer names its folder when the layer knew it, because "a
    /// folder's settings file" is not an instruction anyone can act on.
    fn phrase(self, folder: Option<&str>) -> String {
        match (self, folder) {
            (Self::UserGlobal, _) => "your settings file, for every machine and folder".to_owned(),
            (Self::UserGlobalMachine, _) => "your settings file for this machine".to_owned(),
            (Self::MainShared, Some(name)) => {
                format!("the shared settings file in {name}, for every machine")
            }
            (Self::MainShared, None) => {
                "the main folder's shared settings file, for every machine".to_owned()
            }
            (Self::MainMachine, Some(name)) => {
                format!("the shared settings file in {name}, for this machine")
            }
            (Self::MainMachine, None) => {
                "the main folder's shared settings file, for this machine".to_owned()
            }
            (Self::FolderShared, Some(name)) => {
                format!("{name}'s own settings file, for every machine")
            }
            (Self::FolderShared, None) => {
                "a folder's own settings file, for every machine".to_owned()
            }
            (Self::FolderMachine, Some(name)) => {
                format!("{name}'s own settings file, for this machine")
            }
            (Self::FolderMachine, None) => {
                "a folder's own settings file, for this machine".to_owned()
            }
        }
    }
}

/// The stable machine name of a load fault.
///
/// An exhaustive `match` rather than the serde rename, for the same reason
/// [`ConfigTierVm::of`] is one: a new fault kind should stop the build here,
/// where someone will notice, rather than reach a surface that shows it as a
/// string nobody wrote a test for.
fn fault_kind(kind: &crate::config::LayerFaultKind) -> &'static str {
    use crate::config::LayerFaultKind as K;
    match kind {
        K::Unreadable => "unreadable",
        K::Malformed => "malformed",
        K::NotATable => "notATable",
        K::ScalarExpected => "scalarExpected",
        K::ValueShape => "valueShape",
        K::KeyRefused => "keyRefused",
        K::SettingsInNonMainFolder => "settingsInNonMainFolder",
        K::MainFolderInFolderLayer => "mainFolderInFolderLayer",
        K::UnknownTable => "unknownTable",
        K::MainFolderMissing => "mainFolderMissing",
        K::MainFolderNotADirectory => "mainFolderNotADirectory",
        K::MainFolderNotAProfile => "mainFolderNotAProfile",
    }
}

/// The section's opening sentence.
///
/// It says what a file-set value *costs the reader* rather than reporting a
/// count, because the count is already visible in the list underneath. The one
/// thing the list cannot say is why the switch they just flipped went back.
fn layers_summary(overrides: usize, faults: usize) -> String {
    let mut summary = match overrides {
        0 => "No setting is being set by a file. Everything here is stored by keeper.".to_owned(),
        1 => "1 setting is set by a file. Changing it here will not take effect while the file sets it.".to_owned(),
        many => format!(
            "{many} settings are set by a file. Changing them here will not take effect while the files set them."
        ),
    };
    match faults {
        0 => {}
        1 => summary.push_str(" keeper found 1 problem in your settings files."),
        many => summary.push_str(&format!(
            " keeper found {many} problems in your settings files."
        )),
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 45.21: the file receipt names the file and where it went, and
    /// carries none of a note's caveats — a file has no embeds keeper reads.
    #[test]
    fn a_file_export_receipt_names_the_file_and_the_folder() {
        let receipt = ExportReceiptVm::file(
            "/Users/alice/Desktop",
            "/Users/alice/Desktop/clip.mov".to_owned(),
            "clip.mov",
        );
        assert_eq!(
            receipt.summary,
            "Exported clip.mov to /Users/alice/Desktop."
        );
        assert_eq!(receipt.written, vec!["clip.mov"]);
        assert!(receipt.missing.is_empty() && receipt.notes.is_empty());
    }

    /// A plan carrying only the two not-carried lists — the attachment list is
    /// the copier's input, never the receipt's.
    fn caveats(missing: &[&str], notes: &[&str]) -> NoteExportPlan {
        NoteExportPlan {
            attachments: Vec::new(),
            missing: missing.iter().map(|s| (*s).to_owned()).collect(),
            notes: notes.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    /// Story 45.21: the attachment count comes off what landed, so a receipt
    /// cannot claim a file the copier never wrote.
    #[test]
    fn a_note_export_receipt_counts_the_files_that_actually_landed() {
        let receipt = ExportReceiptVm::note(
            "/out/Meeting".to_owned(),
            "Meeting.md",
            vec![
                "Meeting/Meeting.md".to_owned(),
                "Meeting/attachments/a.png".to_owned(),
                "Meeting/data/rows.csv".to_owned(),
            ],
            caveats(&[], &[]),
        );
        assert_eq!(
            receipt.summary,
            "Exported Meeting.md and 2 attachments to /out/Meeting."
        );
    }

    #[test]
    fn a_note_export_receipt_is_singular_about_one_attachment_and_silent_about_none() {
        let one = ExportReceiptVm::note(
            "/out/Meeting".to_owned(),
            "Meeting.md",
            vec!["Meeting/Meeting.md".to_owned(), "Meeting/a.png".to_owned()],
            caveats(&[], &[]),
        );
        assert_eq!(
            one.summary,
            "Exported Meeting.md and 1 attachment to /out/Meeting."
        );
        let none = ExportReceiptVm::note(
            "/out/Meeting".to_owned(),
            "Meeting.md",
            vec!["Meeting/Meeting.md".to_owned()],
            caveats(&[], &[]),
        );
        assert_eq!(none.summary, "Exported Meeting.md to /out/Meeting.");
    }

    /// Story 45.21: what did not go is named, not merely counted — a count
    /// sends somebody to compare two folders by hand.
    ///
    /// The two caveats are asserted TOGETHER and in order, because they are
    /// two lists of the same type: a call site that passed one where the other
    /// belongs would produce a grammatical sentence about the wrong files, and
    /// only their relative order in one string can see that.
    #[test]
    fn a_note_export_receipt_names_what_it_could_not_find_and_what_it_would_not_follow() {
        let receipt = ExportReceiptVm::note(
            "/out/Meeting".to_owned(),
            "Meeting.md",
            vec!["Meeting/Meeting.md".to_owned()],
            caveats(&["gone.png", "vanished.pdf"], &["Other Note"]),
        );
        assert_eq!(
            receipt.summary,
            "Exported Meeting.md to /out/Meeting. keeper could not find 2 files this note \
             embeds, so they were not carried: gone.png, vanished.pdf. One embedded note was \
             not carried — export it separately: Other Note."
        );
        // And the lists reach the wire intact, not only the sentence.
        assert_eq!(receipt.missing, vec!["gone.png", "vanished.pdf"]);
        assert_eq!(receipt.notes, vec!["Other Note"]);
    }

    #[test]
    fn a_single_missing_file_reads_as_one_rather_than_as_a_list() {
        let receipt = ExportReceiptVm::note(
            "/out/M".to_owned(),
            "M.md",
            vec!["M/M.md".to_owned()],
            caveats(&["gone.png"], &["A", "B"]),
        );
        assert!(
            receipt.summary.contains(
                "keeper could not find 1 file this note embeds, so it was not carried: gone.png."
            ),
            "{}",
            receipt.summary
        );
        assert!(
            receipt
                .summary
                .contains("Embedded notes were not carried — export them separately: A, B."),
            "{}",
            receipt.summary
        );
    }

    /// Story 45.21: a receipt is read once, in a toast, so a long list stops
    /// naming and starts counting — but it never stops naming entirely.
    ///
    /// Four entries as well as five: with three named, four is the smallest
    /// list that has a remainder, and a cap that only starts counting at two
    /// left over would pass every five-entry test.
    #[test]
    fn a_long_list_names_three_and_counts_the_rest() {
        let names = |n: usize| -> Vec<String> {
            ["a", "b", "c", "d", "e"][..n]
                .iter()
                .map(|s| (*s).to_owned())
                .collect()
        };
        assert_eq!(named_list(&names(0)), "");
        assert_eq!(named_list(&names(1)), "a");
        assert_eq!(named_list(&names(3)), "a, b, c");
        assert_eq!(named_list(&names(4)), "a, b, c and 1 more");
        assert_eq!(named_list(&names(5)), "a, b, c and 2 more");
    }

    /// Story 18.2: `is_live`/`is_terminal` partition every `RecordingUiState`
    /// variant — live = Preflight/Recording/Rotating/Stopping, terminal =
    /// Idle/Finalized/Recovered/Failed — and are exact complements.
    #[test]
    fn recording_ui_state_live_terminal_partition_covers_all_variants() {
        use RecordingUiState::*;
        for state in [
            Idle, Preflight, Recording, Rotating, Stopping, Finalized, Recovered, Failed,
        ] {
            let expected_live = matches!(state, Preflight | Recording | Rotating | Stopping);
            assert_eq!(state.is_live(), expected_live, "is_live({state:?})");
            assert_eq!(
                state.is_terminal(),
                !expected_live,
                "is_terminal({state:?})"
            );
        }
    }

    /// Story 18.3: the idle snapshot carries zeroed byte/cap fields — the
    /// stored/default snapshot never invents size or a cap (those are read-time /
    /// session-captured, filled only when a session exists).
    #[test]
    fn recording_status_idle_zeroes_bytes_and_cap() {
        let idle = RecordingStatusVm::idle();
        assert_eq!(idle.state, RecordingUiState::Idle);
        assert_eq!(idle.on_disk_bytes, 0);
        assert_eq!(idle.current_segment_bytes, 0);
        assert_eq!(idle.segment_cap_mb, 0);
        // Story 41.6: no session has promised anything, so the honest reading is
        // "on this Mac" with nothing to explain.
        assert_eq!(idle.durability, RecordingDurabilityVm::local());
    }

    /// Story 41.6: the four states are the frontend's string union, so their
    /// camelCase wire names are the contract — a rename here silently breaks a
    /// banner that switches on them.
    #[test]
    fn recording_durability_state_serializes_camel_case() {
        for (state, wire) in [
            (RecordingDurabilityState::Local, "\"local\""),
            (RecordingDurabilityState::Committed, "\"committed\""),
            (RecordingDurabilityState::Pushed, "\"pushed\""),
            (RecordingDurabilityState::Verified, "\"verified\""),
        ] {
            let json = serde_json::to_string(&state).expect("serialize durability state");
            assert_eq!(json, wire);
            let back: RecordingDurabilityState =
                serde_json::from_str(&json).expect("deserialize durability state");
            assert_eq!(back, state);
        }
    }

    /// The floor is a `max` over the declared order, so the order IS the
    /// ranking — anything that reorders these variants changes what "never
    /// regresses" means.
    #[test]
    fn recording_durability_state_orders_least_to_most_durable() {
        use RecordingDurabilityState::*;
        let mut states = [Verified, Local, Pushed, Committed];
        states.sort();
        assert_eq!(states, [Local, Committed, Pushed, Verified]);
    }

    /// The reason is a Rust-authored sentence carried verbatim, and it rides the
    /// VM as `detail` — present only when publication is actually stuck.
    #[test]
    fn recording_durability_vm_carries_the_reason_verbatim() {
        let stuck = RecordingDurabilityVm {
            state: RecordingDurabilityState::Committed,
            detail: Some("push rejected: non-fast-forward".to_owned()),
        };
        let json = serde_json::to_string(&stuck).expect("serialize durability");
        assert_eq!(
            json,
            "{\"state\":\"committed\",\"detail\":\"push rejected: non-fast-forward\"}"
        );
        let back: RecordingDurabilityVm =
            serde_json::from_str(&json).expect("deserialize durability");
        assert_eq!(back, stuck);
        assert_eq!(
            serde_json::to_string(&RecordingDurabilityVm::local()).expect("serialize local"),
            "{\"state\":\"local\",\"detail\":null}"
        );
    }

    #[test]
    fn ipc_error_code_serializes_camel_case() {
        let json = serde_json::to_string(&IpcErrorCode::Unsupported).expect("serialize code");
        assert_eq!(json, "\"unsupported\"");
        let back: IpcErrorCode = serde_json::from_str(&json).expect("deserialize code");
        assert_eq!(back, IpcErrorCode::Unsupported);
    }

    #[test]
    fn ipc_error_round_trips_camel_case_and_omits_none_account() {
        let err = IpcError {
            code: IpcErrorCode::Internal,
            message: "boom".to_owned(),
            account_id: None,
            retriable: true,
        };
        let json = serde_json::to_string(&err).expect("serialize error");
        // camelCase field name and absent account_id.
        assert!(json.contains("\"retriable\":true"), "json was: {json}");
        assert!(
            !json.contains("accountId"),
            "account_id should be omitted: {json}"
        );
        let back: IpcError = serde_json::from_str(&json).expect("deserialize error");
        assert_eq!(back, err);
    }

    #[test]
    fn ipc_error_serializes_account_id_camel_case_when_present() {
        let err = IpcError {
            code: IpcErrorCode::Internal,
            message: "boom".to_owned(),
            account_id: Some("01ABC".to_owned()),
            retriable: false,
        };
        let json = serde_json::to_string(&err).expect("serialize error");
        assert!(json.contains("\"accountId\":\"01ABC\""), "json was: {json}");
    }

    #[test]
    fn account_vm_round_trips_camel_case() {
        let vm = AccountVm {
            account_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            user_id: "@alice:example.org".to_owned(),
            homeserver_url: "https://matrix.example.org/".to_owned(),
            hue_index: 3,
            provider: Provider::Password,
        };
        let json = serde_json::to_string(&vm).expect("serialize account vm");
        assert!(json.contains("\"accountId\":"), "json was: {json}");
        assert!(json.contains("\"userId\":"), "json was: {json}");
        assert!(json.contains("\"homeserverUrl\":"), "json was: {json}");
        assert!(json.contains("\"hueIndex\":3"), "json was: {json}");
        assert!(
            json.contains("\"provider\":\"password\""),
            "json was: {json}"
        );
        // No token/session material is present on the VM.
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: AccountVm = serde_json::from_str(&json).expect("deserialize account vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn bridge_network_vm_round_trips_camel_case() {
        let vm = BridgeNetworkVm {
            network_id: "whatsapp".to_owned(),
            name: "WhatsApp".to_owned(),
            glyph: "WA".to_owned(),
            tier: RiskTier::Maintenance,
            tier_label: "Maintenance-heavy".to_owned(),
            badge_style: BadgeStyle::OutlineDegraded,
            requires_ack: false,
            ack_copy: None,
        };
        let json = serde_json::to_string(&vm).expect("serialize bridge network vm");
        assert!(json.contains("\"networkId\":"), "json was: {json}");
        assert!(json.contains("\"tierLabel\":"), "json was: {json}");
        assert!(
            json.contains("\"badgeStyle\":\"outlineDegraded\""),
            "json was: {json}"
        );
        assert!(
            json.contains("\"tier\":\"maintenance\""),
            "json was: {json}"
        );
        assert!(json.contains("\"requiresAck\":false"), "json was: {json}");
        assert!(json.contains("\"ackCopy\":null"), "json was: {json}");
        let back: BridgeNetworkVm =
            serde_json::from_str(&json).expect("deserialize bridge network vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn bridge_discovery_vm_round_trips_camel_case() {
        let vm = BridgeDiscoveryVm {
            homeserver: "example.org".to_owned(),
            networks: vec![
                DiscoveredBridgeVm {
                    network_id: "whatsapp".to_owned(),
                    status: BridgeStatus::LoggedIn,
                },
                DiscoveredBridgeVm {
                    network_id: "signal".to_owned(),
                    status: BridgeStatus::Configured,
                },
            ],
        };
        let json = serde_json::to_string(&vm).expect("serialize discovery vm");
        assert!(
            json.contains("\"homeserver\":\"example.org\""),
            "json was: {json}"
        );
        assert!(
            json.contains("\"networkId\":\"whatsapp\""),
            "json was: {json}"
        );
        assert!(json.contains("\"status\":\"loggedIn\""), "json was: {json}");
        assert!(
            json.contains("\"status\":\"configured\""),
            "json was: {json}"
        );
        // No bot MXID, token, or session material crosses the wire.
        assert!(!json.contains("@"), "json leaked an mxid: {json}");
        let back: BridgeDiscoveryVm =
            serde_json::from_str(&json).expect("deserialize discovery vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn resolve_support_vm_round_trips_camel_case() {
        let vm = ResolveSupportVm {
            network_id: "whatsapp".to_owned(),
            supported: true,
            identifier_hint: "Phone number in international format".to_owned(),
            placeholder: "+1 555 123 4567".to_owned(),
        };
        let json = serde_json::to_string(&vm).expect("serialize resolve support vm");
        assert!(
            json.contains("\"networkId\":\"whatsapp\""),
            "json was: {json}"
        );
        assert!(json.contains("\"supported\":true"), "json was: {json}");
        assert!(json.contains("\"identifierHint\":"), "json was: {json}");
        assert!(json.contains("\"placeholder\":"), "json was: {json}");
        // No token/session material is present on the VM.
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: ResolveSupportVm =
            serde_json::from_str(&json).expect("deserialize resolve support vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn new_chat_resolution_vm_round_trips_camel_case() {
        let vm = NewChatResolutionVm {
            room_id: "!portal:example.org".to_owned(),
        };
        let json = serde_json::to_string(&vm).expect("serialize new chat resolution vm");
        assert!(
            json.contains("\"roomId\":\"!portal:example.org\""),
            "json was: {json}"
        );
        // Only the room id crosses the wire — no token/session material.
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: NewChatResolutionVm =
            serde_json::from_str(&json).expect("deserialize new chat resolution vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn bridge_status_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&BridgeStatus::NotLoggedIn).expect("serialize status"),
            "\"notLoggedIn\""
        );
        assert_eq!(
            serde_json::to_string(&BridgeStatus::LoggedIn).expect("serialize status"),
            "\"loggedIn\""
        );
        assert_eq!(
            serde_json::to_string(&BridgeStatus::Configured).expect("serialize status"),
            "\"configured\""
        );
    }

    #[test]
    fn risk_tier_and_badge_style_serialize_camel_case() {
        assert_eq!(
            serde_json::to_string(&RiskTier::Volatile).expect("serialize tier"),
            "\"volatile\""
        );
        assert_eq!(
            serde_json::to_string(&BadgeStyle::FilledDisconnected).expect("serialize badge"),
            "\"filledDisconnected\""
        );
    }

    #[test]
    fn provider_serializes_lowercase_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&Provider::Password).expect("serialize password"),
            "\"password\""
        );
        assert_eq!(
            serde_json::to_string(&Provider::Oidc).expect("serialize oidc"),
            "\"oidc\""
        );
        assert_eq!(
            serde_json::to_string(&Provider::Beeper).expect("serialize beeper"),
            "\"beeper\""
        );
        for provider in [Provider::Password, Provider::Oidc, Provider::Beeper] {
            let json = serde_json::to_string(&provider).expect("serialize provider");
            let back: Provider = serde_json::from_str(&json).expect("deserialize provider");
            assert_eq!(back, provider);
        }
    }

    #[test]
    fn provider_registry_str_round_trips() {
        for provider in [Provider::Password, Provider::Oidc, Provider::Beeper] {
            assert_eq!(
                Provider::from_registry_str(provider.as_registry_str()),
                Some(provider)
            );
        }
        assert_eq!(Provider::from_registry_str("unknown"), None);
        assert_eq!(Provider::from_registry_str(""), None);
    }

    fn sample_inbox_room() -> InboxRoomVm {
        InboxRoomVm {
            account_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            hue_index: 2,
            room_id: "!abc:example.org".to_owned(),
            display_name: "Alice".to_owned(),
            last_message: Some("hi there".to_owned()),
            timestamp: Some(1_720_000_000_000),
            avatar_url: None,
            is_unread: false,
            mention_count: 0,
            is_archived: false,
            is_favourite: false,
            is_pinned: false,
            network: None,
            network_id: None,
            mute_state: MuteState::None,
        }
    }

    #[test]
    fn inbox_room_vm_round_trips_camel_case_with_account_and_hue() {
        let vm = sample_inbox_room();
        let json = serde_json::to_string(&vm).expect("serialize inbox room vm");
        assert!(json.contains("\"accountId\":"), "json was: {json}");
        assert!(json.contains("\"hueIndex\":2"), "json was: {json}");
        assert!(json.contains("\"roomId\":"), "json was: {json}");
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: InboxRoomVm = serde_json::from_str(&json).expect("deserialize inbox room vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn inbox_op_tags_and_round_trips() {
        let reset = InboxOp::Reset {
            rooms: vec![sample_inbox_room()],
        };
        let json = serde_json::to_string(&reset).expect("serialize reset");
        assert!(json.contains("\"op\":\"reset\""), "json was: {json}");
        let back: InboxOp = serde_json::from_str(&json).expect("deserialize reset");
        assert_eq!(back, reset);

        let remove = InboxOp::Remove { index: 2 };
        let json = serde_json::to_string(&remove).expect("serialize remove");
        assert!(json.contains("\"op\":\"remove\""), "json was: {json}");
        assert!(json.contains("\"index\":2"), "json was: {json}");
        let back: InboxOp = serde_json::from_str(&json).expect("deserialize remove");
        assert_eq!(back, remove);
    }

    #[test]
    fn inbox_batch_round_trips() {
        let batch = InboxBatch {
            ops: vec![InboxOp::Reset {
                rooms: vec![sample_inbox_room()],
            }],
            total: Some(11),
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        assert!(json.contains("\"total\":11"), "json was: {json}");
        let back: InboxBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn new_error_codes_serialize_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::SlidingSyncUnsupported)
                .expect("serialize sss code"),
            "\"slidingSyncUnsupported\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::InvalidCredentials).expect("serialize creds code"),
            "\"invalidCredentials\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::ServerUnreachable)
                .expect("serialize unreachable code"),
            "\"serverUnreachable\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::UnsupportedLoginType)
                .expect("serialize login-type code"),
            "\"unsupportedLoginType\""
        );
        // Story 2.2 OIDC codes — locked to the frontend wire contract.
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::OauthUnsupported).expect("serialize oauth-unsup"),
            "\"oauthUnsupported\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::OauthTimedOut).expect("serialize oauth-timeout"),
            "\"oauthTimedOut\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::OauthCancelled).expect("serialize oauth-cancel"),
            "\"oauthCancelled\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::OauthFailed).expect("serialize oauth-failed"),
            "\"oauthFailed\""
        );
        // Story 2.3 Beeper code — locked to the frontend wire contract.
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::BeeperUnavailable)
                .expect("serialize beeper-unavailable"),
            "\"beeperUnavailable\""
        );
    }

    #[test]
    fn verification_failed_code_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::VerificationFailed)
                .expect("serialize verification-failed code"),
            "\"verificationFailed\""
        );
    }

    #[test]
    fn backup_error_codes_serialize_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::BackupMalformedKey)
                .expect("serialize backup-malformed code"),
            "\"backupMalformedKey\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::BackupIncorrectKey)
                .expect("serialize backup-incorrect code"),
            "\"backupIncorrectKey\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::BackupExists)
                .expect("serialize backup-exists code"),
            "\"backupExists\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::BackupFailed)
                .expect("serialize backup-failed code"),
            "\"backupFailed\""
        );
    }

    #[test]
    fn backup_status_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&BackupStatus::Unknown).expect("serialize unknown"),
            "\"unknown\""
        );
        assert_eq!(
            serde_json::to_string(&BackupStatus::Disabled).expect("serialize disabled"),
            "\"disabled\""
        );
        assert_eq!(
            serde_json::to_string(&BackupStatus::Enabled).expect("serialize enabled"),
            "\"enabled\""
        );
        assert_eq!(
            serde_json::to_string(&BackupStatus::Incomplete).expect("serialize incomplete"),
            "\"incomplete\""
        );
    }

    #[test]
    fn backup_status_round_trips() {
        for status in [
            BackupStatus::Unknown,
            BackupStatus::Disabled,
            BackupStatus::Enabled,
            BackupStatus::Incomplete,
        ] {
            let json = serde_json::to_string(&status).expect("serialize status");
            let back: BackupStatus = serde_json::from_str(&json).expect("deserialize status");
            assert_eq!(back, status);
        }
    }

    #[test]
    fn sync_unavailable_code_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::SyncUnavailable).expect("serialize sync code"),
            "\"syncUnavailable\""
        );
    }

    fn sample_room() -> RoomVm {
        RoomVm {
            room_id: "!abc:example.org".to_owned(),
            display_name: "Alice".to_owned(),
            last_message: Some("hi there".to_owned()),
            timestamp: Some(1_720_000_000_000),
            avatar_url: Some("mxc://example.org/av".to_owned()),
            is_unread: false,
            mention_count: 0,
            is_archived: false,
            is_favourite: false,
            is_space: false,
            network: None,
            network_id: None,
            mute_state: MuteState::None,
        }
    }

    #[test]
    fn room_vm_round_trips_camel_case() {
        let vm = sample_room();
        let json = serde_json::to_string(&vm).expect("serialize room vm");
        assert!(json.contains("\"roomId\":"), "json was: {json}");
        assert!(json.contains("\"displayName\":"), "json was: {json}");
        assert!(json.contains("\"lastMessage\":"), "json was: {json}");
        assert!(json.contains("\"avatarUrl\":"), "json was: {json}");
        // No token/session material may appear on the VM.
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: RoomVm = serde_json::from_str(&json).expect("deserialize room vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn room_vm_null_fields_round_trip() {
        let vm = RoomVm {
            room_id: "!x:example.org".to_owned(),
            display_name: "Room".to_owned(),
            last_message: None,
            timestamp: None,
            avatar_url: None,
            is_unread: false,
            mention_count: 0,
            is_archived: false,
            is_favourite: false,
            is_space: false,
            network: None,
            network_id: None,
            mute_state: MuteState::None,
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(json.contains("\"lastMessage\":null"), "json was: {json}");
        assert!(json.contains("\"timestamp\":null"), "json was: {json}");
        let back: RoomVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn space_vm_round_trips_camel_case() {
        let vm = SpaceVm {
            account_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            space_id: "!space:example.org".to_owned(),
            name: "Design Team".to_owned(),
            avatar_url: Some("mxc://example.org/space".to_owned()),
        };
        let json = serde_json::to_string(&vm).expect("serialize space vm");
        assert!(json.contains("\"accountId\":"), "json was: {json}");
        assert!(json.contains("\"spaceId\":"), "json was: {json}");
        assert!(json.contains("\"name\":"), "json was: {json}");
        assert!(json.contains("\"avatarUrl\":"), "json was: {json}");
        let back: SpaceVm = serde_json::from_str(&json).expect("deserialize space vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn spaces_snapshot_round_trips() {
        let snapshot = SpacesSnapshot {
            spaces: vec![SpaceVm {
                account_id: "acctA".to_owned(),
                space_id: "!space:example.org".to_owned(),
                name: "Space".to_owned(),
                avatar_url: None,
            }],
        };
        let json = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(json.contains("\"spaces\":["), "json was: {json}");
        let back: SpacesSnapshot = serde_json::from_str(&json).expect("deserialize snapshot");
        assert_eq!(back, snapshot);
    }

    #[test]
    fn network_vm_round_trips_camel_case() {
        let vm = NetworkVm {
            name: "Telegram".to_owned(),
        };
        let json = serde_json::to_string(&vm).expect("serialize network vm");
        assert!(json.contains("\"name\":\"Telegram\""), "json was: {json}");
        let back: NetworkVm = serde_json::from_str(&json).expect("deserialize network vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn networks_snapshot_round_trips() {
        let snapshot = NetworksSnapshot {
            networks: vec![
                NetworkVm {
                    name: "Signal".to_owned(),
                },
                NetworkVm {
                    name: "Telegram".to_owned(),
                },
            ],
        };
        let json = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(json.contains("\"networks\":["), "json was: {json}");
        let back: NetworksSnapshot = serde_json::from_str(&json).expect("deserialize snapshot");
        assert_eq!(back, snapshot);
    }

    #[test]
    fn room_vm_network_round_trips() {
        let vm = RoomVm {
            network: Some("Telegram".to_owned()),
            ..sample_room()
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(
            json.contains("\"network\":\"Telegram\""),
            "json was: {json}"
        );
        let back: RoomVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn inbox_room_vm_network_round_trips() {
        let vm = InboxRoomVm {
            network: Some("Signal".to_owned()),
            ..sample_inbox_room()
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(json.contains("\"network\":\"Signal\""), "json was: {json}");
        let back: InboxRoomVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn room_vm_network_id_round_trips() {
        let vm = RoomVm {
            network: Some("WhatsApp".to_owned()),
            network_id: Some("whatsapp".to_owned()),
            ..sample_room()
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(
            json.contains("\"networkId\":\"whatsapp\""),
            "json was: {json}"
        );
        let back: RoomVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn inbox_room_vm_network_id_round_trips() {
        let vm = InboxRoomVm {
            network: Some("WhatsApp".to_owned()),
            network_id: Some("whatsapp".to_owned()),
            ..sample_inbox_room()
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(
            json.contains("\"networkId\":\"whatsapp\""),
            "json was: {json}"
        );
        let back: InboxRoomVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn mute_state_serializes_snake_case_wire_contract() {
        // The frontend union is `"none" | "muted" | "mention_only"` — assert the exact
        // wire tags so the ts-rs binding and the TS renderer never drift (Story 10.2).
        assert_eq!(
            serde_json::to_string(&MuteState::None).expect("serialize none"),
            "\"none\""
        );
        assert_eq!(
            serde_json::to_string(&MuteState::Muted).expect("serialize muted"),
            "\"muted\""
        );
        assert_eq!(
            serde_json::to_string(&MuteState::MentionOnly).expect("serialize mention"),
            "\"mention_only\""
        );
        let back: MuteState =
            serde_json::from_str("\"mention_only\"").expect("deserialize mention_only");
        assert_eq!(back, MuteState::MentionOnly);
    }

    #[test]
    fn inbox_room_vm_carries_mute_state() {
        let vm = InboxRoomVm {
            mute_state: MuteState::Muted,
            ..sample_inbox_room()
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(json.contains("\"muteState\":\"muted\""), "json was: {json}");
        let back: InboxRoomVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn chat_notify_mode_serializes_snake_case_wire_contract() {
        // The IPC command vocabulary is `"all" | "mention_only" | "mute"` (Story 10.2).
        assert_eq!(
            serde_json::to_string(&ChatNotifyMode::All).expect("serialize all"),
            "\"all\""
        );
        assert_eq!(
            serde_json::to_string(&ChatNotifyMode::MentionOnly).expect("serialize mention"),
            "\"mention_only\""
        );
        assert_eq!(
            serde_json::to_string(&ChatNotifyMode::Mute).expect("serialize mute"),
            "\"mute\""
        );
        let back: ChatNotifyMode = serde_json::from_str("\"mute\"").expect("deserialize mute");
        assert_eq!(back, ChatNotifyMode::Mute);
    }

    #[test]
    fn bridge_health_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&BridgeHealth::Healthy).expect("serialize health"),
            "\"healthy\""
        );
        assert_eq!(
            serde_json::to_string(&BridgeHealth::Degraded).expect("serialize health"),
            "\"degraded\""
        );
        assert_eq!(
            serde_json::to_string(&BridgeHealth::Disconnected).expect("serialize health"),
            "\"disconnected\""
        );
    }

    #[test]
    fn bridge_health_snapshot_round_trips_camel_case() {
        let snapshot = BridgeHealthSnapshot {
            sessions: vec![
                BridgeSessionHealthVm {
                    account_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
                    network_id: "whatsapp".to_owned(),
                    network_name: "WhatsApp".to_owned(),
                    health: BridgeHealth::Disconnected,
                    last_checked_ms: 1_720_000_000_000,
                    detail: Some("you have been logged out".to_owned()),
                },
                BridgeSessionHealthVm {
                    account_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
                    network_id: "telegram".to_owned(),
                    network_name: "Telegram".to_owned(),
                    health: BridgeHealth::Healthy,
                    last_checked_ms: 1_720_000_000_000,
                    detail: None,
                },
            ],
        };
        let json = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(
            json.contains("\"networkId\":\"whatsapp\""),
            "json was: {json}"
        );
        assert!(
            json.contains("\"networkName\":\"WhatsApp\""),
            "json was: {json}"
        );
        assert!(
            json.contains("\"health\":\"disconnected\""),
            "json was: {json}"
        );
        assert!(json.contains("\"lastCheckedMs\":"), "json was: {json}");
        // No bot MXID, token, or session material crosses the wire.
        assert!(!json.contains("@"), "json leaked an mxid: {json}");
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: BridgeHealthSnapshot = serde_json::from_str(&json).expect("deserialize snapshot");
        assert_eq!(back, snapshot);
    }

    #[test]
    fn room_list_op_tags_and_round_trips() {
        let reset = RoomListOp::Reset {
            rooms: vec![sample_room()],
        };
        let json = serde_json::to_string(&reset).expect("serialize reset");
        assert!(json.contains("\"op\":\"reset\""), "json was: {json}");
        let back: RoomListOp = serde_json::from_str(&json).expect("deserialize reset");
        assert_eq!(back, reset);

        let insert = RoomListOp::Insert {
            index: 3,
            room: sample_room(),
        };
        let json = serde_json::to_string(&insert).expect("serialize insert");
        assert!(json.contains("\"op\":\"insert\""), "json was: {json}");
        assert!(json.contains("\"index\":3"), "json was: {json}");
        let back: RoomListOp = serde_json::from_str(&json).expect("deserialize insert");
        assert_eq!(back, insert);

        let clear = RoomListOp::Clear;
        assert_eq!(
            serde_json::to_string(&clear).expect("serialize clear"),
            "{\"op\":\"clear\"}"
        );
    }

    #[test]
    fn room_list_batch_round_trips() {
        let batch = RoomListBatch {
            ops: vec![
                RoomListOp::Reset {
                    rooms: vec![sample_room()],
                },
                RoomListOp::PopFront,
            ],
            total: Some(7),
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        assert!(json.contains("\"total\":7"), "json was: {json}");
        let back: RoomListBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn timeline_unavailable_code_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::TimelineUnavailable)
                .expect("serialize timeline code"),
            "\"timelineUnavailable\""
        );
    }

    #[test]
    fn send_failed_code_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::SendFailed).expect("serialize send-failed code"),
            "\"sendFailed\""
        );
    }

    #[test]
    fn send_state_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&SendState::Sending).expect("serialize sending"),
            "\"sending\""
        );
        assert_eq!(
            serde_json::to_string(&SendState::Sent).expect("serialize sent"),
            "\"sent\""
        );
        assert_eq!(
            serde_json::to_string(&SendState::Failed).expect("serialize failed"),
            "\"failed\""
        );
    }

    #[test]
    fn send_state_round_trips() {
        for state in [SendState::Sending, SendState::Sent, SendState::Failed] {
            let json = serde_json::to_string(&state).expect("serialize send state");
            let back: SendState = serde_json::from_str(&json).expect("deserialize send state");
            assert_eq!(back, state);
        }
    }

    #[test]
    fn connection_status_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&ConnectionStatus::Online).expect("serialize online"),
            "\"online\""
        );
        assert_eq!(
            serde_json::to_string(&ConnectionStatus::Offline).expect("serialize offline"),
            "\"offline\""
        );
    }

    #[test]
    fn connection_status_round_trips() {
        for status in [ConnectionStatus::Online, ConnectionStatus::Offline] {
            let json = serde_json::to_string(&status).expect("serialize status");
            let back: ConnectionStatus = serde_json::from_str(&json).expect("deserialize status");
            assert_eq!(back, status);
        }
    }

    #[test]
    fn connection_status_batch_round_trips() {
        let batch = ConnectionStatusBatch {
            status: ConnectionStatus::Offline,
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        assert!(json.contains("\"status\":\"offline\""), "json was: {json}");
        let back: ConnectionStatusBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn encryption_status_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&EncryptionStatus::Unknown).expect("serialize unknown"),
            "\"unknown\""
        );
        assert_eq!(
            serde_json::to_string(&EncryptionStatus::Verified).expect("serialize verified"),
            "\"verified\""
        );
        assert_eq!(
            serde_json::to_string(&EncryptionStatus::Unverified).expect("serialize unverified"),
            "\"unverified\""
        );
    }

    #[test]
    fn encryption_status_round_trips() {
        for status in [
            EncryptionStatus::Unknown,
            EncryptionStatus::Verified,
            EncryptionStatus::Unverified,
        ] {
            let json = serde_json::to_string(&status).expect("serialize status");
            let back: EncryptionStatus = serde_json::from_str(&json).expect("deserialize status");
            assert_eq!(back, status);
        }
    }

    #[test]
    fn encryption_status_batch_round_trips() {
        let batch = EncryptionStatusBatch {
            status: EncryptionStatus::Unverified,
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        assert!(
            json.contains("\"status\":\"unverified\""),
            "json was: {json}"
        );
        let back: EncryptionStatusBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn sas_emoji_vm_round_trips_camel_case() {
        let vm = SasEmojiVm {
            symbol: "🐶".to_owned(),
            name: "Dog".to_owned(),
        };
        let json = serde_json::to_string(&vm).expect("serialize emoji vm");
        assert!(json.contains("\"symbol\":\"🐶\""), "json was: {json}");
        assert!(json.contains("\"name\":\"Dog\""), "json was: {json}");
        let back: SasEmojiVm = serde_json::from_str(&json).expect("deserialize emoji vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn verification_phase_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Requested).expect("serialize requested"),
            "\"requested\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Ready).expect("serialize ready"),
            "\"ready\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Comparing).expect("serialize comparing"),
            "\"comparing\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Confirmed).expect("serialize confirmed"),
            "\"confirmed\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Done).expect("serialize done"),
            "\"done\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Cancelled).expect("serialize cancelled"),
            "\"cancelled\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Failed).expect("serialize failed"),
            "\"failed\""
        );
    }

    #[test]
    fn verification_phase_round_trips() {
        for phase in [
            VerificationPhase::Requested,
            VerificationPhase::Ready,
            VerificationPhase::Comparing,
            VerificationPhase::Confirmed,
            VerificationPhase::Done,
            VerificationPhase::Cancelled,
            VerificationPhase::Failed,
        ] {
            let json = serde_json::to_string(&phase).expect("serialize phase");
            let back: VerificationPhase = serde_json::from_str(&json).expect("deserialize phase");
            assert_eq!(back, phase);
        }
    }

    #[test]
    fn verification_flow_vm_round_trips_camel_case() {
        let vm = VerificationFlowVm {
            flow_id: "$flow123".to_owned(),
            phase: VerificationPhase::Comparing,
            emojis: Some(vec![
                SasEmojiVm {
                    symbol: "🐶".to_owned(),
                    name: "Dog".to_owned(),
                },
                SasEmojiVm {
                    symbol: "🐱".to_owned(),
                    name: "Cat".to_owned(),
                },
            ]),
            qr_code_svg: None,
            reason: None,
        };
        let json = serde_json::to_string(&vm).expect("serialize flow vm");
        assert!(json.contains("\"flowId\":\"$flow123\""), "json was: {json}");
        assert!(json.contains("\"phase\":\"comparing\""), "json was: {json}");
        assert!(json.contains("\"qrCodeSvg\":null"), "json was: {json}");
        // No SAS key / decimal / crypto material may appear on the VM.
        assert!(!json.contains("key"), "json leaked a key field: {json}");
        assert!(
            !json.contains("decimal"),
            "json leaked a decimal field: {json}"
        );
        let back: VerificationFlowVm = serde_json::from_str(&json).expect("deserialize flow vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn verification_flow_vm_qr_and_reason_round_trip() {
        let vm = VerificationFlowVm {
            flow_id: "$flow456".to_owned(),
            phase: VerificationPhase::Failed,
            emojis: None,
            qr_code_svg: Some("<svg>…</svg>".to_owned()),
            reason: Some("The expected key did not match the verified one".to_owned()),
        };
        let json = serde_json::to_string(&vm).expect("serialize flow vm");
        assert!(json.contains("\"qrCodeSvg\":\"<svg>"), "json was: {json}");
        assert!(
            json.contains("\"reason\":\"The expected"),
            "json was: {json}"
        );
        let back: VerificationFlowVm = serde_json::from_str(&json).expect("deserialize flow vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn bridge_login_phase_round_trips() {
        for phase in [
            BridgeLoginPhase::ChoosingMethod,
            BridgeLoginPhase::Waiting,
            BridgeLoginPhase::Qr,
            BridgeLoginPhase::CodeEntry,
            BridgeLoginPhase::Success,
            BridgeLoginPhase::Failure,
        ] {
            let json = serde_json::to_string(&phase).expect("serialize phase");
            let back: BridgeLoginPhase = serde_json::from_str(&json).expect("deserialize phase");
            assert_eq!(back, phase);
        }
        // Spot-check the camelCase wire form for a multi-word variant.
        assert_eq!(
            serde_json::to_string(&BridgeLoginPhase::ChoosingMethod).expect("serialize"),
            "\"choosingMethod\""
        );
    }

    #[test]
    fn bridge_login_vm_qr_round_trips_camel_case_and_leaks_no_token() {
        let vm = BridgeLoginVm {
            network_id: "whatsapp".to_owned(),
            phase: BridgeLoginPhase::Qr,
            instruction: Some("Scan this QR with WhatsApp on your phone.".to_owned()),
            qr_svg: Some("<svg>…</svg>".to_owned()),
            qr_refreshed: true,
            fields: vec![],
            flows: vec![],
            error: None,
        };
        let json = serde_json::to_string(&vm).expect("serialize login vm");
        assert!(
            json.contains("\"networkId\":\"whatsapp\""),
            "json was: {json}"
        );
        assert!(json.contains("\"phase\":\"qr\""), "json was: {json}");
        assert!(json.contains("\"qrSvg\":\"<svg>"), "json was: {json}");
        assert!(json.contains("\"qrRefreshed\":true"), "json was: {json}");
        // No access token / bearer / cookie material may ride on the login VM.
        assert!(!json.contains("access_token"), "token leaked: {json}");
        assert!(!json.contains("Bearer"), "bearer leaked: {json}");
        let back: BridgeLoginVm = serde_json::from_str(&json).expect("deserialize login vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn bridge_login_vm_code_entry_and_flows_round_trip() {
        let vm = BridgeLoginVm {
            network_id: "signal".to_owned(),
            phase: BridgeLoginPhase::CodeEntry,
            instruction: Some("Enter the code sent to your device.".to_owned()),
            qr_svg: None,
            qr_refreshed: false,
            fields: vec![LoginFieldVm {
                id: "2fa_code".to_owned(),
                field_type: "2fa_code".to_owned(),
                name: "Verification code".to_owned(),
                description: Some("The 6-digit code".to_owned()),
                pattern: Some("^[0-9]{6}$".to_owned()),
                default_value: None,
            }],
            flows: vec![LoginFlowVm {
                id: "qr".to_owned(),
                name: "QR code".to_owned(),
                description: None,
            }],
            error: None,
        };
        let json = serde_json::to_string(&vm).expect("serialize login vm");
        assert!(
            json.contains("\"fieldType\":\"2fa_code\""),
            "json was: {json}"
        );
        assert!(json.contains("\"defaultValue\":null"), "json was: {json}");
        let back: BridgeLoginVm = serde_json::from_str(&json).expect("deserialize login vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn bridge_login_input_tags_and_round_trips() {
        let choose = BridgeLoginInput::ChooseFlow {
            flow_id: "qr".to_owned(),
        };
        let json = serde_json::to_string(&choose).expect("serialize input");
        assert!(json.contains("\"kind\":\"chooseFlow\""), "json was: {json}");
        assert!(json.contains("\"flowId\":\"qr\""), "json was: {json}");
        let back: BridgeLoginInput = serde_json::from_str(&json).expect("deserialize input");
        assert_eq!(back, choose);

        let mut values = std::collections::BTreeMap::new();
        values.insert("phone_number".to_owned(), "+15551234".to_owned());
        let fields = BridgeLoginInput::Fields { values };
        let json = serde_json::to_string(&fields).expect("serialize input");
        assert!(json.contains("\"kind\":\"fields\""), "json was: {json}");
        let back: BridgeLoginInput = serde_json::from_str(&json).expect("deserialize input");
        assert_eq!(back, fields);
    }

    #[test]
    fn bbctl_phase_serializes_camel_case_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&BbctlPhase::Checking).expect("serialize"),
            "\"checking\""
        );
        for phase in [
            BbctlPhase::Checking,
            BbctlPhase::Registering,
            BbctlPhase::Starting,
            BbctlPhase::Running,
            BbctlPhase::Success,
            BbctlPhase::Failure,
        ] {
            let json = serde_json::to_string(&phase).expect("serialize phase");
            let back: BbctlPhase = serde_json::from_str(&json).expect("deserialize phase");
            assert_eq!(back, phase);
        }
    }

    #[test]
    fn bbctl_availability_vm_round_trips_camel_case() {
        let vm = BbctlAvailabilityVm {
            available: false,
            install: BbctlInstallVm {
                steps: vec!["install bbctl".to_owned(), "run bbctl login".to_owned()],
                docs_url: "https://example.org/docs".to_owned(),
            },
            networks: vec![BbctlNetworkVm {
                network_id: "signal".to_owned(),
                name: "Signal".to_owned(),
                bbctl_name: "sh-signal".to_owned(),
            }],
        };
        let json = serde_json::to_string(&vm).expect("serialize availability vm");
        assert!(json.contains("\"docsUrl\":"), "json was: {json}");
        assert!(
            json.contains("\"bbctlName\":\"sh-signal\""),
            "json was: {json}"
        );
        let back: BbctlAvailabilityVm =
            serde_json::from_str(&json).expect("deserialize availability vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn bbctl_progress_vm_round_trips_and_leaks_no_token() {
        let vm = BbctlProgressVm {
            network_id: "signal".to_owned(),
            phase: BbctlPhase::Failure,
            message: None,
            error: Some("bbctl: could not reach the appservice".to_owned()),
        };
        let json = serde_json::to_string(&vm).expect("serialize progress vm");
        assert!(
            json.contains("\"networkId\":\"signal\""),
            "json was: {json}"
        );
        // No token / bearer / session material is ever carried on the VM.
        assert!(
            !json.to_lowercase().contains("token")
                && !json.to_lowercase().contains("bearer")
                && !json.to_lowercase().contains("access_token"),
            "progress VM must carry no token material: {json}"
        );
        let back: BbctlProgressVm = serde_json::from_str(&json).expect("deserialize progress vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_utd_tags_and_round_trips() {
        let vm = TimelineItemVm::Utd {
            key: "unique-3".to_owned(),
            sender: "@carol:example.org".to_owned(),
            sender_display_name: Some("Carol".to_owned()),
            timestamp: 1_720_000_000_000,
        };
        let json = serde_json::to_string(&vm).expect("serialize utd vm");
        assert!(json.contains("\"kind\":\"utd\""), "json was: {json}");
        assert!(json.contains("\"key\":\"unique-3\""), "json was: {json}");
        assert!(
            json.contains("\"senderDisplayName\":\"Carol\""),
            "json was: {json}"
        );
        // No ciphertext / session / key material may appear on the VM.
        assert!(
            !json.contains("session"),
            "json leaked a session field: {json}"
        );
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize utd vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_utd_null_display_name_round_trips() {
        let vm = TimelineItemVm::Utd {
            key: "k".to_owned(),
            sender: "@a:example.org".to_owned(),
            sender_display_name: None,
            timestamp: 1,
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(
            json.contains("\"senderDisplayName\":null"),
            "json was: {json}"
        );
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_message_with_send_state_round_trips() {
        let vm = TimelineItemVm::Message {
            key: "unique-1".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_display_name: Some("Alice".to_owned()),
            body: "outgoing".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: true,
            send_state: Some(SendState::Sending),
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: None,
            readers: Vec::new(),
        };
        let json = serde_json::to_string(&vm).expect("serialize message vm");
        assert!(
            json.contains("\"sendState\":\"sending\""),
            "json was: {json}"
        );
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize message vm");
        assert_eq!(back, vm);
    }

    fn sample_message() -> TimelineItemVm {
        TimelineItemVm::Message {
            key: "unique-1".to_owned(),
            sender: "@bob:example.org".to_owned(),
            sender_display_name: Some("Bob".to_owned()),
            body: "hello world".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: false,
            send_state: None,
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: None,
            readers: Vec::new(),
        }
    }

    #[test]
    fn reply_preview_vm_round_trips_camel_case() {
        let vm = ReplyPreviewVm {
            in_reply_to_key: Some("unique-orig".to_owned()),
            sender: "@carol:example.org".to_owned(),
            sender_display_name: Some("Carol".to_owned()),
            body: "original body".to_owned(),
        };
        let json = serde_json::to_string(&vm).expect("serialize reply preview vm");
        assert!(
            json.contains("\"inReplyToKey\":\"unique-orig\""),
            "json was: {json}"
        );
        assert!(
            json.contains("\"senderDisplayName\":\"Carol\""),
            "json was: {json}"
        );
        // No event-id / txn-id material may appear on the VM.
        assert!(
            !json.contains("eventId") && !json.contains("$"),
            "json leaked event-id material: {json}"
        );
        let back: ReplyPreviewVm =
            serde_json::from_str(&json).expect("deserialize reply preview vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn reply_preview_vm_null_key_round_trips() {
        let vm = ReplyPreviewVm {
            in_reply_to_key: None,
            sender: "@carol:example.org".to_owned(),
            sender_display_name: None,
            body: String::new(),
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(json.contains("\"inReplyToKey\":null"), "json was: {json}");
        let back: ReplyPreviewVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_message_with_reply_and_edited_round_trips() {
        let vm = TimelineItemVm::Message {
            key: "unique-9".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_display_name: Some("Alice".to_owned()),
            body: "a reply".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: true,
            send_state: None,
            is_edited: true,
            reply: Some(ReplyPreviewVm {
                in_reply_to_key: Some("unique-orig".to_owned()),
                sender: "@bob:example.org".to_owned(),
                sender_display_name: Some("Bob".to_owned()),
                body: "the original".to_owned(),
            }),
            reactions: vec![
                ReactionGroupVm {
                    emoji: "👍".to_owned(),
                    count: 3,
                    is_own: false,
                },
                ReactionGroupVm {
                    emoji: "❤️".to_owned(),
                    count: 1,
                    is_own: true,
                },
            ],
            media: None,
            readers: Vec::new(),
        };
        let json = serde_json::to_string(&vm).expect("serialize message vm");
        assert!(json.contains("\"isEdited\":true"), "json was: {json}");
        assert!(
            json.contains("\"inReplyToKey\":\"unique-orig\""),
            "json was: {json}"
        );
        // The reaction groups carry only emoji/count/is_own — no user-id or
        // event-id material.
        assert!(json.contains("\"emoji\":\"👍\""), "json was: {json}");
        assert!(json.contains("\"count\":3"), "json was: {json}");
        assert!(json.contains("\"isOwn\":true"), "json was: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize message vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_message_tags_and_round_trips() {
        let vm = sample_message();
        let json = serde_json::to_string(&vm).expect("serialize message vm");
        assert!(json.contains("\"kind\":\"message\""), "json was: {json}");
        assert!(
            json.contains("\"senderDisplayName\":\"Bob\""),
            "json was: {json}"
        );
        assert!(json.contains("\"isOwn\":false"), "json was: {json}");
        // No token/session/event-id material may appear on the VM.
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize message vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_other_tags_and_round_trips() {
        let vm = TimelineItemVm::Other {
            key: "unique-2".to_owned(),
        };
        let json = serde_json::to_string(&vm).expect("serialize other vm");
        assert!(json.contains("\"kind\":\"other\""), "json was: {json}");
        assert!(json.contains("\"key\":\"unique-2\""), "json was: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize other vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_null_display_name_round_trips() {
        let vm = TimelineItemVm::Message {
            key: "k".to_owned(),
            sender: "@a:example.org".to_owned(),
            sender_display_name: None,
            body: "hi".to_owned(),
            timestamp: 1,
            is_own: true,
            send_state: None,
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: None,
            readers: Vec::new(),
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(
            json.contains("\"senderDisplayName\":null"),
            "json was: {json}"
        );
        assert!(json.contains("\"sendState\":null"), "json was: {json}");
        assert!(json.contains("\"reply\":null"), "json was: {json}");
        assert!(json.contains("\"media\":null"), "json was: {json}");
        // An empty reaction set serializes as an empty array (no pill row).
        assert!(json.contains("\"reactions\":[]"), "json was: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn reaction_group_vm_round_trips_camel_case_and_carries_no_identity() {
        let vm = ReactionGroupVm {
            emoji: "🎉".to_owned(),
            count: 4,
            is_own: true,
        };
        let json = serde_json::to_string(&vm).expect("serialize reaction group vm");
        assert!(json.contains("\"emoji\":\"🎉\""), "json was: {json}");
        assert!(json.contains("\"count\":4"), "json was: {json}");
        assert!(json.contains("\"isOwn\":true"), "json was: {json}");
        // Only emoji/count/is_own cross IPC — never a per-sender user id or a
        // reaction event id.
        assert!(
            !json.contains("sender") && !json.contains("userId") && !json.contains("eventId"),
            "json leaked identity material: {json}"
        );
        assert!(
            !json.contains('@') && !json.contains('$'),
            "json leaked user-id/event-id material: {json}"
        );
        let back: ReactionGroupVm =
            serde_json::from_str(&json).expect("deserialize reaction group vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_op_tags_and_round_trips() {
        let reset = TimelineOp::Reset {
            items: vec![sample_message()],
        };
        let json = serde_json::to_string(&reset).expect("serialize reset");
        assert!(json.contains("\"op\":\"reset\""), "json was: {json}");
        let back: TimelineOp = serde_json::from_str(&json).expect("deserialize reset");
        assert_eq!(back, reset);

        let insert = TimelineOp::Insert {
            index: 4,
            item: sample_message(),
        };
        let json = serde_json::to_string(&insert).expect("serialize insert");
        assert!(json.contains("\"op\":\"insert\""), "json was: {json}");
        assert!(json.contains("\"index\":4"), "json was: {json}");
        let back: TimelineOp = serde_json::from_str(&json).expect("deserialize insert");
        assert_eq!(back, insert);

        let clear = TimelineOp::Clear;
        assert_eq!(
            serde_json::to_string(&clear).expect("serialize clear"),
            "{\"op\":\"clear\"}"
        );
    }

    #[test]
    fn timeline_batch_round_trips() {
        let batch = TimelineBatch {
            ops: vec![
                TimelineOp::Reset {
                    items: vec![sample_message()],
                },
                TimelineOp::PushBack {
                    item: TimelineItemVm::Other {
                        key: "k2".to_owned(),
                    },
                },
            ],
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        assert!(json.contains("\"ops\":"), "json was: {json}");
        let back: TimelineBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn media_kind_vm_serializes_camel_case_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&MediaKindVm::Image).expect("serialize image"),
            "\"image\""
        );
        assert_eq!(
            serde_json::to_string(&MediaKindVm::Video).expect("serialize video"),
            "\"video\""
        );
        assert_eq!(
            serde_json::to_string(&MediaKindVm::Audio).expect("serialize audio"),
            "\"audio\""
        );
        assert_eq!(
            serde_json::to_string(&MediaKindVm::File).expect("serialize file"),
            "\"file\""
        );
        for kind in [
            MediaKindVm::Image,
            MediaKindVm::Video,
            MediaKindVm::Audio,
            MediaKindVm::File,
        ] {
            let json = serde_json::to_string(&kind).expect("serialize kind");
            let back: MediaKindVm = serde_json::from_str(&json).expect("deserialize kind");
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn media_vm_round_trips_camel_case_and_carries_no_key_material() {
        let vm = MediaVm {
            kind: MediaKindVm::Image,
            url: "keeper-media://media/acct/room/item/full".to_owned(),
            thumbnail_url: Some("keeper-media://media/acct/room/item/thumb".to_owned()),
            filename: "photo.png".to_owned(),
            mimetype: Some("image/png".to_owned()),
            size: Some(12_345),
            width: Some(800),
            height: Some(600),
            caption: Some("a nice photo".to_owned()),
        };
        let json = serde_json::to_string(&vm).expect("serialize media vm");
        assert!(json.contains("\"kind\":\"image\""), "json was: {json}");
        assert!(
            json.contains("\"url\":\"keeper-media://"),
            "json was: {json}"
        );
        assert!(
            json.contains("\"thumbnailUrl\":\"keeper-media://"),
            "json was: {json}"
        );
        assert!(json.contains("\"size\":12345"), "json was: {json}");
        assert!(json.contains("\"width\":800"), "json was: {json}");
        // No mxc / EncryptedFile / key / event-id material may appear on the VM.
        assert!(!json.contains("mxc://"), "json leaked an mxc uri: {json}");
        assert!(!json.contains("mxc"), "json leaked mxc material: {json}");
        assert!(
            !json.contains("\"key\"") && !json.contains("iv") && !json.contains("hashes"),
            "json leaked EncryptedFile key material: {json}"
        );
        assert!(
            !json.contains("eventId") && !json.contains('$'),
            "json leaked event-id material: {json}"
        );
        let back: MediaVm = serde_json::from_str(&json).expect("deserialize media vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn media_vm_null_fields_round_trip() {
        let vm = MediaVm {
            kind: MediaKindVm::File,
            url: "keeper-media://media/a/r/i/full".to_owned(),
            thumbnail_url: None,
            filename: "report.pdf".to_owned(),
            mimetype: None,
            size: None,
            width: None,
            height: None,
            caption: None,
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(json.contains("\"thumbnailUrl\":null"), "json was: {json}");
        assert!(json.contains("\"mimetype\":null"), "json was: {json}");
        assert!(json.contains("\"size\":null"), "json was: {json}");
        let back: MediaVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_message_with_media_round_trips_no_key_material() {
        let vm = TimelineItemVm::Message {
            key: "unique-media".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_display_name: Some("Alice".to_owned()),
            body: "look at this".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: false,
            send_state: None,
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: Some(Box::new(MediaVm {
                kind: MediaKindVm::Video,
                url: "keeper-media://media/a/r/i/full".to_owned(),
                thumbnail_url: Some("keeper-media://media/a/r/i/thumb".to_owned()),
                filename: "clip.mp4".to_owned(),
                mimetype: Some("video/mp4".to_owned()),
                size: Some(999),
                width: Some(1280),
                height: Some(720),
                caption: None,
            })),
            readers: Vec::new(),
        };
        let json = serde_json::to_string(&vm).expect("serialize message vm");
        assert!(json.contains("\"media\":{"), "json was: {json}");
        assert!(json.contains("\"kind\":\"video\""), "json was: {json}");
        // No mxc / key / event-id material may cross on the media-carrying message.
        assert!(!json.contains("mxc"), "json leaked mxc material: {json}");
        assert!(!json.contains("eventId"), "json leaked event id: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize message vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn demo_batch_tags_variants() {
        let snap = DemoBatch::Snapshot {
            items: vec![DemoItem {
                id: "1".into(),
                label: "one".into(),
            }],
        };
        let json = serde_json::to_string(&snap).expect("serialize snapshot");
        assert!(json.contains("\"kind\":\"snapshot\""), "json was: {json}");
    }

    #[test]
    fn message_vm_carries_readers_as_opaque_ids() {
        // The receipts feature (Story 3.9): a message VM carries the *other*
        // members whose read receipt sits on it as opaque user ids under
        // `readers` — camelCase, an array of strings, no avatar/receipt-id fields.
        let vm = TimelineItemVm::Message {
            key: "unique-1".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_display_name: Some("Alice".to_owned()),
            body: "read by others".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: true,
            send_state: None,
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: None,
            readers: vec![
                "@bob:example.org".to_owned(),
                "@carol:example.org".to_owned(),
            ],
        };
        let json = serde_json::to_string(&vm).expect("serialize message vm");
        assert!(
            json.contains("\"readers\":[\"@bob:example.org\",\"@carol:example.org\"]"),
            "json was: {json}"
        );
        // No receipt event id crosses on a reader.
        assert!(
            !json.contains("receiptId"),
            "json leaked receipt id: {json}"
        );
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize message vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn typist_vm_round_trips_camel_case() {
        let vm = TypistVm {
            user_id: "@bob:example.org".to_owned(),
            display_name: Some("Bob".to_owned()),
        };
        let json = serde_json::to_string(&vm).expect("serialize typist");
        assert!(
            json.contains("\"userId\":\"@bob:example.org\""),
            "json was: {json}"
        );
        assert!(json.contains("\"displayName\":\"Bob\""), "json was: {json}");
        let back: TypistVm = serde_json::from_str(&json).expect("deserialize typist");
        assert_eq!(back, vm);
    }

    #[test]
    fn typing_batch_round_trips_and_empty_serializes() {
        let batch = TypingBatch {
            typists: vec![TypistVm {
                user_id: "@bob:example.org".to_owned(),
                display_name: None,
            }],
        };
        let json = serde_json::to_string(&batch).expect("serialize typing batch");
        assert!(json.contains("\"typists\":["), "json was: {json}");
        assert!(json.contains("\"displayName\":null"), "json was: {json}");
        let back: TypingBatch = serde_json::from_str(&json).expect("deserialize typing batch");
        assert_eq!(back, batch);

        let empty = TypingBatch { typists: vec![] };
        assert_eq!(
            serde_json::to_string(&empty).expect("serialize empty"),
            "{\"typists\":[]}"
        );
    }

    #[test]
    fn pagination_state_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&PaginationState::Paginating).expect("serialize paginating"),
            "\"paginating\""
        );
        assert_eq!(
            serde_json::to_string(&PaginationState::Idle).expect("serialize idle"),
            "\"idle\""
        );
    }

    #[test]
    fn pagination_status_batch_round_trips_camel_case() {
        let batch = PaginationStatusBatch {
            state: PaginationState::Idle,
            hit_start: true,
        };
        let json = serde_json::to_string(&batch).expect("serialize pagination status");
        assert!(json.contains("\"state\":\"idle\""), "json was: {json}");
        assert!(json.contains("\"hitStart\":true"), "json was: {json}");
        let back: PaginationStatusBatch =
            serde_json::from_str(&json).expect("deserialize pagination status");
        assert_eq!(back, batch);

        let paginating = PaginationStatusBatch {
            state: PaginationState::Paginating,
            hit_start: false,
        };
        let json = serde_json::to_string(&paginating).expect("serialize paginating");
        assert!(
            json.contains("\"state\":\"paginating\""),
            "json was: {json}"
        );
        assert!(json.contains("\"hitStart\":false"), "json was: {json}");
    }

    /// Story 43.8: the browser classifies through the one attachment
    /// vocabulary, so a `.mov` in a synced folder and a `.mov` a note embeds
    /// are the same kind of thing.
    #[test]
    fn a_files_entry_takes_its_kind_from_the_one_vocabulary() {
        for (name, expected) in [
            ("clip.mov", RecordingNoteTargetKind::Video),
            ("shot.PNG", RecordingNoteTargetKind::Image),
            ("voice.m4a", RecordingNoteTargetKind::Audio),
            ("manifest.json", RecordingNoteTargetKind::File),
            ("Makefile", RecordingNoteTargetKind::File),
        ] {
            let entry = FilesEntryVm::new(FilesEntryFacts {
                name: name.to_owned(),
                relative_path: format!("sub/{name}"),
                absolute_path: format!("/v/sub/{name}"),
                is_dir: false,
                sync: FilesEntrySyncVm::plain(FilesSyncStatusVm::Synced),
                size_bytes: Some(7),
                roles: FilesFolderRoles::default(),
                write: FilesWriteVm::allowed(),
            });
            assert_eq!(entry.kind, expected, "{name}");
        }
    }

    /// The dirent decides folder-ness, never the name: a directory called
    /// `notes.md` is a folder, and an extension table would offer to open it in
    /// a text editor.
    #[test]
    fn a_directory_is_a_folder_whatever_it_is_named() {
        let entry = FilesEntryVm::new(FilesEntryFacts {
            name: "notes.md".to_owned(),
            relative_path: "notes.md".to_owned(),
            absolute_path: "/v/notes.md".to_owned(),
            is_dir: true,
            sync: FilesEntrySyncVm::plain(FilesSyncStatusVm::Synced),
            size_bytes: None,
            roles: FilesFolderRoles::default(),
            write: FilesWriteVm::allowed(),
        });
        assert_eq!(entry.kind, RecordingNoteTargetKind::Folder);
    }

    /// The distinction the whole surface rests on, asserted on the wire: an
    /// empty folder serializes `"entries":[]` and an absent drive serializes
    /// `"entries":null`, so no frontend can read one as the other.
    #[test]
    fn an_empty_listing_and_an_absent_drive_are_different_on_the_wire() {
        let empty = FilesListingVm {
            profile_id: "01PROFILE".to_owned(),
            subpath: String::new(),
            state: FilesListingState::Listed,
            entries: Some(Vec::new()),
            detail: None,
            truncated: false,
            write: FilesWriteVm::allowed(),
        };
        let json = serde_json::to_string(&empty).expect("serialize empty listing");
        assert!(json.contains("\"state\":\"listed\""), "json was: {json}");
        assert!(json.contains("\"entries\":[]"), "json was: {json}");

        let absent = FilesListingVm {
            entries: None,
            state: FilesListingState::MediaAbsent,
            detail: Some("merope is not attached.".to_owned()),
            ..empty.clone()
        };
        let json = serde_json::to_string(&absent).expect("serialize absent media");
        assert!(
            json.contains("\"state\":\"mediaAbsent\""),
            "json was: {json}"
        );
        assert!(json.contains("\"entries\":null"), "json was: {json}");
        let back: FilesListingVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, absent);
    }

    /// Story 45.3: `writable` and `reason` are exact complements, so a control
    /// can never vanish without a sentence saying why.
    #[test]
    fn a_write_verdict_carries_a_reason_exactly_when_it_refuses() {
        assert_eq!(
            FilesWriteVm::allowed(),
            FilesWriteVm {
                writable: true,
                reason: None,
                caveat: None,
                caveat_short: None,
            }
        );
        let refused = FilesWriteVm::refused("nope, and here is why");
        assert!(!refused.writable);
        assert_eq!(refused.reason.as_deref(), Some("nope, and here is why"));
        assert_eq!(refused.caveat, None);
        assert_eq!(refused.caveat_short, None);

        // Story 46.14's third state: writable AND unmanaged. Never a refusal,
        // and never silent — the two fields are still exclusive.
        //
        // Story 53.3 made the caveat two forms of one fact, and they arrive
        // TOGETHER: a row carrying only the long one reaches a folded surface
        // with nothing to show, which is AD-102's fact off the screen.
        let unmanaged = FilesWriteVm::unmanaged(
            "AGENTS.md is not one of keeper's notes — it is outside Vault's notes vault",
            "AGENTS.md is not one of keeper's notes: no note history",
        );
        assert!(unmanaged.writable);
        assert_eq!(unmanaged.reason, None);
        assert_eq!(
            unmanaged.caveat.as_deref(),
            Some("AGENTS.md is not one of keeper's notes — it is outside Vault's notes vault")
        );
        assert_eq!(
            unmanaged.caveat_short.as_deref(),
            Some("AGENTS.md is not one of keeper's notes: no note history")
        );

        // The projection every call site actually uses: a `Result` from the
        // write-scope decision, whose `Err` already holds the whole sentence.
        let ok: Result<(), String> = Ok(());
        assert_eq!(FilesWriteVm::from_verdict(&ok), FilesWriteVm::allowed());
        let err: Result<(), String> = Err("outside the vault".to_owned());
        assert_eq!(
            FilesWriteVm::from_verdict(&err),
            FilesWriteVm::refused("outside the vault")
        );
    }

    /// One vault file in a delete selection — what every Story 45.3 case is.
    fn note(
        path: &str,
        status: FilesSyncStatusVm,
    ) -> (String, FilesSyncStatusVm, FilesDeleteDestinationVm) {
        (
            path.to_owned(),
            status,
            FilesDeleteDestinationVm::VaultTrash,
        )
    }

    /// One file no vault holds — AD-102's second writer's, bound for the
    /// operating system's trash.
    fn loose(
        path: &str,
        status: FilesSyncStatusVm,
    ) -> (String, FilesSyncStatusVm, FilesDeleteDestinationVm) {
        (
            path.to_owned(),
            status,
            FilesDeleteDestinationVm::SystemTrash,
        )
    }

    /// Story 45.3, the confirmation's first requirement: it NAMES one file and
    /// COUNTS many.
    #[test]
    fn a_delete_confirmation_names_one_file_and_counts_several() {
        let one = FilesDeletePlanVm::compose(
            "Vault",
            vec![note("10-notes/Report.md", FilesSyncStatusVm::Synced)],
            Vec::new(),
        );
        assert_eq!(one.question, "Delete 10-notes/Report.md?");
        assert_eq!(one.files, vec!["10-notes/Report.md".to_owned()]);

        let many = FilesDeletePlanVm::compose(
            "Vault",
            vec![
                note("10-notes/a.md", FilesSyncStatusVm::Synced),
                note("10-notes/b.md", FilesSyncStatusVm::Waiting),
                note("10-notes/c.md", FilesSyncStatusVm::Synced),
            ],
            Vec::new(),
        );
        assert_eq!(many.question, "Delete 3 files?");
        // The list is the one the command will act on, in the asked-for order.
        assert_eq!(
            many.files,
            vec![
                "10-notes/a.md".to_owned(),
                "10-notes/b.md".to_owned(),
                "10-notes/c.md".to_owned()
            ]
        );
    }

    /// Story 45.3, the confirmation's second requirement: it says whether the
    /// files sync, and the three answers are different sentences rather than
    /// one hedged one.
    #[test]
    fn a_delete_confirmation_says_whether_the_files_sync() {
        let synced = FilesDeletePlanVm::compose(
            "Vault",
            vec![note("10-notes/a.md", FilesSyncStatusVm::Synced)],
            Vec::new(),
        );
        assert_eq!(
            synced.consequence,
            "This file syncs, so deleting it here removes it from every machine that \
             syncs Vault."
        );

        // Excluded and not-in-a-repository are both "this stays here", and the
        // sentence must not promise a remote that has never heard of the file.
        for status in [
            FilesSyncStatusVm::Excluded,
            FilesSyncStatusVm::NotInRepository,
        ] {
            let local = FilesDeletePlanVm::compose("Vault", vec![note("a.md", status)], Vec::new());
            assert_eq!(
                local.consequence,
                "This file does not sync, so this removes it from this machine only.",
                "{status:?}"
            );
        }

        let none = FilesDeletePlanVm::compose(
            "Vault",
            vec![
                note("a.md", FilesSyncStatusVm::Excluded),
                note("b.md", FilesSyncStatusVm::NotInRepository),
            ],
            Vec::new(),
        );
        assert_eq!(
            none.consequence,
            "None of these 2 files sync, so this removes them from this machine only."
        );

        let mixed = FilesDeletePlanVm::compose(
            "Vault",
            vec![
                note("a.md", FilesSyncStatusVm::Synced),
                note("b.md", FilesSyncStatusVm::Excluded),
                note("c.md", FilesSyncStatusVm::Waiting),
            ],
            Vec::new(),
        );
        assert_eq!(
            mixed.consequence,
            "2 of these 3 files sync, so deleting them removes them from every machine \
             that syncs Vault; the other 1 do not and go from this machine only."
        );
    }

    /// An engine that could not answer is counted as syncing and SAYS so.
    ///
    /// The two guesses are "this deletion stays here" and "this deletion
    /// travels", and only one of them is safe to be wrong about. Picking the
    /// quiet one silently would be exactly the lie `FilesSyncStatusVm::Unknown`
    /// exists to refuse.
    #[test]
    fn an_unreadable_sync_state_is_counted_as_syncing_and_admitted() {
        let all = FilesDeletePlanVm::compose(
            "Vault",
            vec![note("a.md", FilesSyncStatusVm::Unknown)],
            Vec::new(),
        );
        assert_eq!(
            all.consequence,
            "keeper could not read this file's sync state, so it has assumed it syncs and \
             that deleting removes it from every machine that syncs Vault."
        );

        let some = FilesDeletePlanVm::compose(
            "Vault",
            vec![
                note("a.md", FilesSyncStatusVm::Synced),
                note("b.md", FilesSyncStatusVm::Unknown),
            ],
            Vec::new(),
        );
        assert!(
            some.consequence.starts_with("These 2 files sync"),
            "{}",
            some.consequence
        );
        assert!(
            some.consequence.contains(
                "keeper could not read the sync state of 1 of them, and has counted it as \
                 syncing"
            ),
            "{}",
            some.consequence
        );
    }

    /// A destructive confirmation that does not say a copy is kept reads as an
    /// erasure, and this one is not one.
    #[test]
    fn a_delete_confirmation_says_where_the_bytes_go() {
        let one = FilesDeletePlanVm::compose(
            "Vault",
            vec![note("a.md", FilesSyncStatusVm::Synced)],
            Vec::new(),
        );
        assert!(one.recovery.contains("vault's trash"), "{}", one.recovery);
        assert!(one.recovery.contains("rather than erasing it"));

        let many = FilesDeletePlanVm::compose(
            "Vault",
            vec![
                note("a.md", FilesSyncStatusVm::Synced),
                note("b.md", FilesSyncStatusVm::Synced),
            ],
            Vec::new(),
        );
        assert!(many.recovery.contains("rather than erasing them"));
    }

    /// **Story 46.14: the recovery sentence stops promising a trash that does
    /// not exist.** Before AD-102 there was one destination and one sentence;
    /// for a file no vault holds, both halves of that sentence — the vault's
    /// trash and this folder's history — were untrue.
    #[test]
    fn a_delete_out_of_the_vault_does_not_promise_the_vaults_trash() {
        let one = FilesDeletePlanVm::compose(
            "Vault",
            vec![loose("AGENTS.md", FilesSyncStatusVm::Synced)],
            Vec::new(),
        );
        assert!(
            one.recovery.contains("this computer's trash"),
            "{}",
            one.recovery
        );
        assert!(!one.recovery.contains("vault's trash"), "{}", one.recovery);
        assert!(
            !one.recovery.contains("folder's history"),
            "{}",
            one.recovery
        );

        let many = FilesDeletePlanVm::compose(
            "Vault",
            vec![
                loose("AGENTS.md", FilesSyncStatusVm::Synced),
                loose("README.md", FilesSyncStatusVm::Synced),
            ],
            Vec::new(),
        );
        assert!(
            many.recovery.contains("None of them are notes"),
            "{}",
            many.recovery
        );
        assert!(
            !many.recovery.contains("vault's trash"),
            "{}",
            many.recovery
        );

        // One drag over a vault and the folder beside it: both destinations
        // are named and counted, because wording the commoner one and hoping
        // is how a confirmation becomes a lie.
        let mixed = FilesDeletePlanVm::compose(
            "Vault",
            vec![
                note("10-notes/a.md", FilesSyncStatusVm::Synced),
                note("10-notes/b.md", FilesSyncStatusVm::Synced),
                loose("AGENTS.md", FilesSyncStatusVm::Synced),
            ],
            Vec::new(),
        );
        assert_eq!(
            mixed.recovery,
            "Nothing is erased: 2 of these 3 go to the vault's trash and are recorded in \
             this folder's history, and the other 1 go to this computer's trash, because \
             they are not notes."
        );
        // The sync consequence is a different question and is unchanged by the
        // destination: all three of these travel.
        assert!(
            mixed.consequence.starts_with("These 3 files sync"),
            "{}",
            mixed.consequence
        );
    }

    /// A selection keeper will only partly act on says so by name, and an
    /// empty plan is a plan that asks nothing.
    #[test]
    fn a_plan_that_can_delete_nothing_asks_no_question() {
        let refusals = vec![FilesDeleteRefusalVm {
            relative_path: "10-notes/daily".to_owned(),
            reason: "daily is a folder.".to_owned(),
        }];
        let plan = FilesDeletePlanVm::compose("Vault", Vec::new(), refusals.clone());
        assert!(plan.files.is_empty());
        assert_eq!(plan.question, "There is nothing here keeper can delete.");
        // No consequence and no recovery: there is nothing to word, and a
        // leftover sentence about syncing would describe a deletion that is
        // not going to happen.
        assert_eq!(plan.consequence, "");
        assert_eq!(plan.recovery, "");
        assert_eq!(plan.refusals, refusals);

        // A partial selection keeps both halves.
        let partial = FilesDeletePlanVm::compose(
            "Vault",
            vec![note("10-notes/a.md", FilesSyncStatusVm::Synced)],
            refusals.clone(),
        );
        assert_eq!(partial.question, "Delete 10-notes/a.md?");
        assert_eq!(partial.refusals, refusals);
    }

    #[test]
    fn files_entry_camel_cases_both_paths_and_never_leaks_one_into_the_other() {
        let entry = FilesEntryVm::new(FilesEntryFacts {
            name: "clip.mov".to_owned(),
            relative_path: "2026/clip.mov".to_owned(),
            absolute_path: "/Volumes/m/2026/clip.mov".to_owned(),
            is_dir: false,
            sync: FilesEntrySyncVm::explained(
                FilesSyncStatusVm::Waiting,
                "This file has changed and has not been committed yet.",
            ),
            size_bytes: Some(1_500_000),
            roles: FilesFolderRoles::default(),
            write: FilesWriteVm::allowed(),
        });
        let json = serde_json::to_string(&entry).expect("serialize files entry");
        assert!(
            json.contains("\"relativePath\":\"2026/clip.mov\""),
            "json was: {json}"
        );
        assert!(
            json.contains("\"absolutePath\":\"/Volumes/m/2026/clip.mov\""),
            "json was: {json}"
        );
        assert!(json.contains("\"kind\":\"video\""), "json was: {json}");
        // The mark crosses as one nested object, so a surface reads
        // `entry.sync.status` and cannot render a glyph with no sentence.
        assert!(
            json.contains(
                "\"sync\":{\"status\":\"waiting\",\"detail\":\"This file has changed and \
                 has not been committed yet.\"}"
            ),
            "json was: {json}"
        );
        // The size crosses as one nested object carrying both halves, so a
        // surface reads `entry.size.label` and never divides anything (45.5).
        assert!(
            json.contains("\"size\":{\"bytes\":1500000,\"label\":\"1.5 MB\"}"),
            "json was: {json}"
        );
        assert!(json.contains("\"folderRole\":null"), "json was: {json}");
    }

    /// A directory carries no size, even when the caller offers one (Story
    /// 45.5, FR-178).
    ///
    /// `std::fs::metadata` reports a nonzero length for a directory on every
    /// platform keeper runs on, so "the caller offers one" is the ordinary
    /// case rather than a contrived one. The constructor drops it, and the
    /// wire says `null` — not `0`, which would be a false claim about the
    /// folder's contents and the exact string this story exists to prevent.
    #[test]
    fn a_directory_has_no_size_even_when_one_is_offered() {
        let entry = FilesEntryVm::new(FilesEntryFacts {
            name: "Archive".to_owned(),
            relative_path: "Archive".to_owned(),
            absolute_path: "/v/Archive".to_owned(),
            is_dir: true,
            sync: FilesEntrySyncVm::plain(FilesSyncStatusVm::Synced),
            size_bytes: Some(4_096),
            roles: FilesFolderRoles::default(),
            write: FilesWriteVm::allowed(),
        });
        assert_eq!(entry.size, None, "a folder's size is absent, never zero");
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"size\":null"), "json was: {json}");
        assert!(
            !json.contains("0 B") && !json.contains("0 bytes"),
            "a folder must never carry a rendered zero: {json}"
        );
    }

    /// An unreadable file has an unknown size, and an empty one has a size of
    /// zero. They are different facts and must stay so.
    #[test]
    fn an_unknown_size_and_an_empty_file_are_different() {
        let unknown = FilesEntryVm::new(FilesEntryFacts {
            name: "broken.link".to_owned(),
            relative_path: "broken.link".to_owned(),
            absolute_path: "/v/broken.link".to_owned(),
            is_dir: false,
            sync: FilesEntrySyncVm::plain(FilesSyncStatusVm::Unknown),
            size_bytes: None,
            roles: FilesFolderRoles::default(),
            write: FilesWriteVm::allowed(),
        });
        assert_eq!(unknown.size, None);
        let empty = FilesEntryVm::new(FilesEntryFacts {
            name: "empty.md".to_owned(),
            relative_path: "empty.md".to_owned(),
            absolute_path: "/v/empty.md".to_owned(),
            is_dir: false,
            sync: FilesEntrySyncVm::plain(FilesSyncStatusVm::Synced),
            size_bytes: Some(0),
            roles: FilesFolderRoles::default(),
            write: FilesWriteVm::allowed(),
        });
        assert_eq!(
            empty.size.as_ref().map(|size| size.label.as_str()),
            Some("0 bytes"),
            "an empty FILE says so; it is a folder that says nothing"
        );
    }

    /// The vault and the recordings folder are found by CONFIGURATION, never by
    /// name (Story 45.5, FR-178).
    ///
    /// The fixture deliberately uses `Second Brain` and `Clips` rather than the
    /// defaults, and puts a decoy folder literally called `10-notes` beside
    /// them. An implementation that matches keeper's default subfolder names —
    /// the shortcut this story explicitly forbids — marks the decoy and misses
    /// both real ones, so it fails on three assertions at once.
    #[test]
    fn the_vault_and_the_recordings_folder_come_from_configuration_not_from_a_name() {
        let roles = FilesFolderRoles {
            notes_subfolder: Some("Second Brain"),
            recordings_subfolder: Some("Clips"),
        };
        let role_of = |name: &str, is_dir: bool| {
            FilesEntryVm::new(FilesEntryFacts {
                name: name.to_owned(),
                relative_path: name.to_owned(),
                absolute_path: format!("/v/{name}"),
                is_dir,
                sync: FilesEntrySyncVm::plain(FilesSyncStatusVm::Synced),
                size_bytes: None,
                roles,
                write: FilesWriteVm::allowed(),
            })
            .folder_role
        };
        assert_eq!(
            role_of("Second Brain", true),
            Some(FilesFolderRoleVm::NotesVault)
        );
        assert_eq!(role_of("Clips", true), Some(FilesFolderRoleVm::Recordings));
        assert_eq!(
            role_of("10-notes", true),
            None,
            "keeper's default vault name is not evidence of anything"
        );
        assert_eq!(role_of("recordings", true), None);
        assert_eq!(
            role_of("Second Brain", false),
            None,
            "a FILE named like the vault is not the vault"
        );
        // A profile with no vault and no recordings root marks nothing at all.
        let unconfigured = FilesEntryVm::new(FilesEntryFacts {
            name: "Second Brain".to_owned(),
            relative_path: "Second Brain".to_owned(),
            absolute_path: "/v/Second Brain".to_owned(),
            is_dir: true,
            sync: FilesEntrySyncVm::plain(FilesSyncStatusVm::Synced),
            size_bytes: None,
            roles: FilesFolderRoles::default(),
            write: FilesWriteVm::allowed(),
        });
        assert_eq!(unconfigured.folder_role, None);
    }

    /// The role matches the whole path, case-insensitively, however the
    /// subfolder was typed.
    ///
    /// Case-insensitivity is not politeness: APFS and HFS+ are case-insensitive
    /// by default, so the folder the user created as `Notes` and the subfolder
    /// they typed as `notes` are the same folder on disk, and a case-sensitive
    /// compare drops the marker with no way for the user to tell why. The
    /// nested case matters because a subfolder may be `work/notes`, and the
    /// descendant case matters because marking everything under the vault makes
    /// the marker useless.
    #[test]
    fn the_role_normalises_the_configured_subfolder_and_matches_only_the_folder_itself() {
        let role_at = |configured: &str, path: &str| {
            FilesFolderRoles {
                notes_subfolder: Some(configured),
                recordings_subfolder: None,
            }
            .role_of(path, true)
        };
        for configured in ["notes", "Notes", "NOTES", "/notes", "notes/", "\\notes"] {
            assert_eq!(
                role_at(configured, "Notes"),
                Some(FilesFolderRoleVm::NotesVault),
                "configured as {configured:?}"
            );
        }
        assert_eq!(
            role_at("work/notes", "work/notes"),
            Some(FilesFolderRoleVm::NotesVault),
            "a nested vault is still the vault"
        );
        assert_eq!(
            role_at("notes", "notes/daily"),
            None,
            "a folder INSIDE the vault is an ordinary folder"
        );
        assert_eq!(
            role_at("notes", "archive/notes"),
            None,
            "a folder with the vault's NAME elsewhere in the tree is not the vault"
        );
        assert_eq!(
            role_at("", ""),
            None,
            "an empty configured subfolder matches nothing: the profile root is not a vault"
        );
    }

    /// The role vocabulary crosses the wire camel-cased, so a surface can
    /// switch on it without a translation table.
    #[test]
    fn the_folder_role_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&FilesFolderRoleVm::NotesVault).expect("serialize"),
            "\"notesVault\""
        );
        assert_eq!(
            serde_json::to_string(&FilesFolderRoleVm::Recordings).expect("serialize"),
            "\"recordings\""
        );
    }

    /// Epic 46 / AD-98: the projection the "where did this value come from?"
    /// surface renders.
    ///
    /// These live here rather than in the shell because the shell does not
    /// compile on Linux. Every sentence a user reads about their settings files
    /// is composed by `ConfigLayersVm::new`, so asserting it here asserts it on
    /// every machine.
    mod config_layers {
        use std::path::PathBuf;

        use super::*;
        use crate::config::{LayerFault, LayerFaultKind, LayerSource, LayerTier};

        fn source(tier: LayerTier, path: &str, folder: Option<&str>) -> LayerSource {
            LayerSource {
                tier,
                path: PathBuf::from(path),
                folder: folder.map(str::to_owned),
            }
        }

        /// The empty case is the normal one, and it must not imply anything is
        /// wrong: a user with no settings file has a healthy install.
        #[test]
        fn no_layers_reads_as_a_healthy_install_rather_than_a_problem() {
            let vm = ConfigLayersVm::new(Vec::new(), Vec::new(), None);
            assert_eq!(
                vm.summary,
                "No setting is being set by a file. Everything here is stored by keeper."
            );
            assert!(vm.overrides.is_empty());
            assert!(vm.faults.is_empty());
            assert_eq!(vm.main_folder, None);
        }

        /// One override names its key, its file, and what that file reaches —
        /// the three things someone needs to go and change it.
        #[test]
        fn an_override_names_the_key_the_file_and_how_far_the_file_reaches() {
            let vm = ConfigLayersVm::new(
                vec![(
                    "recording.fps".to_owned(),
                    source(LayerTier::UserGlobal, "/Users/t/.keeper/keeper.toml", None),
                )],
                Vec::new(),
                None,
            );
            let [only] = &vm.overrides[..] else {
                panic!("expected exactly one override, got {:?}", vm.overrides);
            };
            assert_eq!(only.key, "recording.fps");
            assert_eq!(only.tier, ConfigTierVm::UserGlobal);
            assert_eq!(only.path, "/Users/t/.keeper/keeper.toml");
            assert_eq!(
                only.source,
                "your settings file, for every machine and folder"
            );
            assert_eq!(
                vm.summary,
                "1 setting is set by a file. Changing it here will not take effect while the file sets it."
            );
        }

        /// The singular/plural boundary, because "1 settings are set by a file"
        /// is the kind of sentence that makes a user distrust the rest of it.
        #[test]
        fn the_summary_counts_in_words_that_agree_with_the_number() {
            let two = ConfigLayersVm::new(
                vec![
                    (
                        "debug.mode".to_owned(),
                        source(LayerTier::UserGlobal, "/h/.keeper/keeper.toml", None),
                    ),
                    (
                        "recording.fps".to_owned(),
                        source(LayerTier::MainShared, "/m/.keeper/keeper.toml", None),
                    ),
                ],
                Vec::new(),
                None,
            );
            assert_eq!(
                two.summary,
                "2 settings are set by a file. Changing them here will not take effect while the files set them."
            );
        }

        /// A machine-scoped layer says "this machine". Two people syncing one
        /// folder need to be able to tell which of the two files they are
        /// looking at from the sentence alone.
        #[test]
        fn every_tier_gets_a_distinct_sentence_naming_its_reach() {
            let phrases: Vec<String> = [
                (LayerTier::UserGlobal, None),
                (LayerTier::UserGlobalMachine, None),
                (LayerTier::MainShared, Some("tgdrive")),
                (LayerTier::MainMachine, Some("tgdrive")),
                (LayerTier::FolderShared, Some("photos")),
                (LayerTier::FolderMachine, Some("photos")),
            ]
            .into_iter()
            .map(|(tier, folder)| {
                let vm = ConfigLayersVm::new(
                    vec![(
                        "k".to_owned(),
                        source(tier, "/f/.keeper/keeper.toml", folder),
                    )],
                    Vec::new(),
                    None,
                );
                vm.overrides[0].source.clone()
            })
            .collect();
            assert_eq!(
                phrases,
                vec![
                    "your settings file, for every machine and folder",
                    "your settings file for this machine",
                    "the shared settings file in tgdrive, for every machine",
                    "the shared settings file in tgdrive, for this machine",
                    "photos's own settings file, for every machine",
                    "photos's own settings file, for this machine",
                ]
            );
        }

        /// A folder layer that arrived without a folder name still produces a
        /// grammatical sentence rather than an empty slot.
        #[test]
        fn a_layer_with_no_folder_name_still_reads_as_a_sentence() {
            let vm = ConfigLayersVm::new(
                vec![(
                    "k".to_owned(),
                    source(LayerTier::FolderMachine, "/f/.keeper/keeper.h.toml", None),
                )],
                Vec::new(),
                None,
            );
            assert_eq!(
                vm.overrides[0].source,
                "a folder's own settings file, for this machine"
            );
            assert_eq!(vm.overrides[0].folder, None);
        }

        /// A fault crosses with a stable machine name AND the config layer's
        /// own finished line, rendered verbatim — the `SyncGitVm.problem`
        /// contract: one fact, one spelling, composed once in Rust.
        #[test]
        fn a_fault_carries_a_stable_kind_and_the_line_the_surface_renders() {
            let fault = LayerFault::late(
                LayerFaultKind::MainFolderNotAProfile,
                PathBuf::from("/Volumes/merope/tgdrive"),
                "mainSyncFolder names a folder that is not a sync folder".to_owned(),
            );
            let vm = ConfigLayersVm::new(Vec::new(), vec![fault], None);
            let [only] = &vm.faults[..] else {
                panic!("expected exactly one fault, got {:?}", vm.faults);
            };
            assert_eq!(only.kind, "mainFolderNotAProfile");
            assert_eq!(only.path, "/Volumes/merope/tgdrive");
            assert_eq!(
                only.summary,
                "/Volumes/merope/tgdrive: mainSyncFolder names a folder that is not a sync folder"
            );
        }

        /// The one that would otherwise be found by a user: a malformed TOML
        /// fault's `Display` is deliberately multi-line — it carries `toml`'s
        /// own caret diagram, which is the right thing in a log and wrecks a
        /// settings pane. The projection takes `summary()`, which is one line.
        #[test]
        fn a_multi_line_parser_fault_reaches_the_surface_as_one_line() {
            let mut fault = LayerFault::late(
                LayerFaultKind::Malformed,
                PathBuf::from("/h/.keeper/keeper.toml"),
                "expected `=`, found a newline\n  |\n3 | recording.fps\n  |              ^",
            );
            fault.line = Some(3);
            let vm = ConfigLayersVm::new(Vec::new(), vec![fault], None);
            assert_eq!(
                vm.faults[0].summary,
                "/h/.keeper/keeper.toml:3: expected `=`, found a newline"
            );
            assert!(
                !vm.faults[0].summary.contains('\n'),
                "a settings pane renders one line per fault"
            );
        }

        /// Faults are counted in the lead sentence, because the list below is
        /// scrollable and the one thing that must not be scrolled past is
        /// "part of your configuration did not load".
        #[test]
        fn the_summary_says_how_many_problems_were_found() {
            let one = ConfigLayersVm::new(
                Vec::new(),
                vec![LayerFault::late(
                    LayerFaultKind::Malformed,
                    PathBuf::from("/h/.keeper/keeper.toml"),
                    "expected `=`".to_owned(),
                )],
                None,
            );
            assert!(
                one.summary
                    .ends_with(" keeper found 1 problem in your settings files."),
                "got {}",
                one.summary
            );
            let two = ConfigLayersVm::new(
                Vec::new(),
                vec![
                    LayerFault::late(
                        LayerFaultKind::Malformed,
                        PathBuf::from("/h/.keeper/keeper.toml"),
                        "expected `=`".to_owned(),
                    ),
                    LayerFault::late(
                        LayerFaultKind::Unreadable,
                        PathBuf::from("/m/.keeper/keeper.toml"),
                        "permission denied".to_owned(),
                    ),
                ],
                None,
            );
            assert!(
                two.summary
                    .ends_with(" keeper found 2 problems in your settings files."),
                "got {}",
                two.summary
            );
        }

        /// A designated main folder that turned out to be wrong is still
        /// reported. Blanking the field on a bad value would hide the typo the
        /// user has to fix — which is the failure this story exists to make
        /// loud.
        #[test]
        fn a_rejected_main_folder_is_still_named_beside_its_fault() {
            let vm = ConfigLayersVm::new(
                Vec::new(),
                vec![LayerFault::late(
                    LayerFaultKind::MainFolderMissing,
                    PathBuf::from("/Volumes/merope/tgdrve"),
                    "no such folder".to_owned(),
                )],
                Some(PathBuf::from("/Volumes/merope/tgdrve")),
            );
            assert_eq!(vm.main_folder.as_deref(), Some("/Volumes/merope/tgdrve"));
            assert_eq!(vm.faults.len(), 1);
        }

        /// The tier vocabulary crosses the wire camel-cased, so a surface can
        /// switch on it without a translation table.
        #[test]
        fn the_tier_serializes_camel_case() {
            assert_eq!(
                serde_json::to_string(&ConfigTierVm::UserGlobalMachine).expect("serialize"),
                "\"userGlobalMachine\""
            );
            assert_eq!(
                serde_json::to_string(&ConfigTierVm::MainShared).expect("serialize"),
                "\"mainShared\""
            );
        }

        /// The engine's folder faults land in the same list as the loader's,
        /// because a user does not care which crate noticed that part of their
        /// configuration did not apply.
        #[test]
        fn folder_faults_join_the_same_list_and_are_counted_with_the_rest() {
            let vm = ConfigLayersVm::new(
                vec![(
                    "recording.fps".to_owned(),
                    source(LayerTier::UserGlobal, "/h/.keeper/keeper.toml", None),
                )],
                vec![LayerFault::late(
                    LayerFaultKind::Malformed,
                    PathBuf::from("/h/.keeper/keeper.toml"),
                    "expected `=`".to_owned(),
                )],
                None,
            )
            .with_folder_faults(vec![ConfigFaultVm::folder(
                &PathBuf::from("/Volumes/merope/photos/.keeper/keeper.toml"),
                "the tag you set was dropped because this file owns it".to_owned(),
            )]);
            assert_eq!(vm.faults.len(), 2);
            assert_eq!(vm.faults[1].kind, "folder");
            assert_eq!(
                vm.faults[1].summary,
                "/Volumes/merope/photos/.keeper/keeper.toml: the tag you set was dropped because this file owns it"
            );
            // Recomputed, not appended to: a count that stopped counting is the
            // failure this method exists to avoid.
            assert!(
                vm.summary
                    .ends_with(" keeper found 2 problems in your settings files."),
                "got {}",
                vm.summary
            );
        }

        /// Folding in nothing changes nothing — the common case, and the one
        /// that would otherwise quietly restate a stale count.
        #[test]
        fn folding_in_no_folder_faults_leaves_the_summary_alone() {
            let plain = ConfigLayersVm::new(Vec::new(), Vec::new(), None);
            let folded =
                ConfigLayersVm::new(Vec::new(), Vec::new(), None).with_folder_faults(Vec::new());
            assert_eq!(plain, folded);
        }
    }
}
