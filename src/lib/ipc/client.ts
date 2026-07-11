/**
 * Thin typed IPC client (AD-7, AD-8).
 *
 * The only hand-written TypeScript in `src/lib/ipc/`: wrappers around the Tauri
 * `invoke`/`Channel` primitives that carry the generated view-model types and
 * surface the {@link IpcError} envelope on rejection. All view-model types are
 * generated into `./gen/` by the Rust ts-rs export step — never hand-edited.
 */
import { Channel, invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ChatNotifyMode } from "./gen/ChatNotifyMode";
import type { DockBadgeMode } from "./gen/DockBadgeMode";
import type { EgressEndpointVm } from "./gen/EgressEndpointVm";
import type { IpcError } from "./gen/IpcError";
import type { LifecyclePhase } from "./gen/LifecyclePhase";
import type { NotificationPermission } from "./gen/NotificationPermission";
import type { NotifyTarget } from "./gen/NotifyTarget";

export type { AccountVm } from "./gen/AccountVm";
export type { ApprovalDraftVm } from "./gen/ApprovalDraftVm";
export type { BackupStatus } from "./gen/BackupStatus";
export type { BadgeStyle } from "./gen/BadgeStyle";
export type { BbctlAvailabilityVm } from "./gen/BbctlAvailabilityVm";
export type { BbctlInstallVm } from "./gen/BbctlInstallVm";
export type { BbctlNetworkVm } from "./gen/BbctlNetworkVm";
export type { BbctlPhase } from "./gen/BbctlPhase";
export type { BbctlProgressVm } from "./gen/BbctlProgressVm";
export type { BridgeDiscoveryVm } from "./gen/BridgeDiscoveryVm";
export type { BridgeHealth } from "./gen/BridgeHealth";
export type { BridgeHealthSnapshot } from "./gen/BridgeHealthSnapshot";
export type { BridgeLoginInput } from "./gen/BridgeLoginInput";
export type { BridgeLoginPhase } from "./gen/BridgeLoginPhase";
export type { BridgeLoginVm } from "./gen/BridgeLoginVm";
export type { BridgeNetworkVm } from "./gen/BridgeNetworkVm";
export type { BridgeSessionHealthVm } from "./gen/BridgeSessionHealthVm";
export type { BridgeStatus } from "./gen/BridgeStatus";
export type { CapabilitiesVm } from "./gen/CapabilitiesVm";
export type { ChatNotifyMode } from "./gen/ChatNotifyMode";
export type { ConnectionStatus } from "./gen/ConnectionStatus";
export type { ConnectionStatusBatch } from "./gen/ConnectionStatusBatch";
export type { CouplingCaveatVm } from "./gen/CouplingCaveatVm";
export type { DemoBatch } from "./gen/DemoBatch";
export type { DemoItem } from "./gen/DemoItem";
export type { DiscoveredBridgeVm } from "./gen/DiscoveredBridgeVm";
export type { DockBadgeMode } from "./gen/DockBadgeMode";
export type { DraftMirrorBatch } from "./gen/DraftMirrorBatch";
export type { EditVersionVm } from "./gen/EditVersionVm";
export type { EgressEndpointVm } from "./gen/EgressEndpointVm";
export type { EgressKind } from "./gen/EgressKind";
export type { EncryptionStatus } from "./gen/EncryptionStatus";
export type { EncryptionStatusBatch } from "./gen/EncryptionStatusBatch";
export type { ExportPhase } from "./gen/ExportPhase";
export type { ExportProgressVm } from "./gen/ExportProgressVm";
export type { ExportRequestVm } from "./gen/ExportRequestVm";
export type { ExportScopeKind } from "./gen/ExportScopeKind";
export type { HeldSendVm } from "./gen/HeldSendVm";
export type { HotkeyVm } from "./gen/HotkeyVm";
export type { InboxBatch } from "./gen/InboxBatch";
export type { InboxOp } from "./gen/InboxOp";
export type { InboxRoomVm } from "./gen/InboxRoomVm";
export type { IncognitoScope } from "./gen/IncognitoScope";
export type { IncognitoVm } from "./gen/IncognitoVm";
export type { IpcError } from "./gen/IpcError";
export type { IpcErrorCode } from "./gen/IpcErrorCode";
export type { LifecyclePhase } from "./gen/LifecyclePhase";
export type { LoginFieldVm } from "./gen/LoginFieldVm";
export type { LoginFlowVm } from "./gen/LoginFlowVm";
export type { MediaKindVm } from "./gen/MediaKindVm";
export type { MediaVm } from "./gen/MediaVm";
export type { MenuItemVm } from "./gen/MenuItemVm";
export type { MenuSectionVm } from "./gen/MenuSectionVm";
export type { MuteState } from "./gen/MuteState";
export type { NetworksSnapshot } from "./gen/NetworksSnapshot";
export type { NetworkVm } from "./gen/NetworkVm";
export type { NewChatResolutionVm } from "./gen/NewChatResolutionVm";
export type { NotificationPermission } from "./gen/NotificationPermission";
export type { NotifyTarget } from "./gen/NotifyTarget";
export type { OutboxVm } from "./gen/OutboxVm";
export type { PaginationState } from "./gen/PaginationState";
export type { PaginationStatusBatch } from "./gen/PaginationStatusBatch";
export type { PaletteActionVm } from "./gen/PaletteActionVm";
export type { PaletteChatVm } from "./gen/PaletteChatVm";
export type { PaletteMode } from "./gen/PaletteMode";
export type { PaletteResultsVm } from "./gen/PaletteResultsVm";
export type { PingVm } from "./gen/PingVm";
export type { Provider } from "./gen/Provider";
export type { ReactionGroupVm } from "./gen/ReactionGroupVm";
export type { RemoteDraftVm } from "./gen/RemoteDraftVm";
export type { ReplyPreviewVm } from "./gen/ReplyPreviewVm";
export type { ResolveSupportVm } from "./gen/ResolveSupportVm";
export type { RiskTier } from "./gen/RiskTier";
export type { RoomListBatch } from "./gen/RoomListBatch";
export type { RoomListOp } from "./gen/RoomListOp";
export type { RoomVm } from "./gen/RoomVm";
export type { SasEmojiVm } from "./gen/SasEmojiVm";
export type { SearchFilterVm } from "./gen/SearchFilterVm";
export type { SearchHitVm } from "./gen/SearchHitVm";
export type { SendState } from "./gen/SendState";
export type { SpacesSnapshot } from "./gen/SpacesSnapshot";
export type { SpaceVm } from "./gen/SpaceVm";
export type { TimelineBatch } from "./gen/TimelineBatch";
export type { TimelineItemVm } from "./gen/TimelineItemVm";
export type { TimelineOp } from "./gen/TimelineOp";
export type { TypingBatch } from "./gen/TypingBatch";
export type { TypistVm } from "./gen/TypistVm";
export type { VerificationFlowVm } from "./gen/VerificationFlowVm";
export type { VerificationPhase } from "./gen/VerificationPhase";

import type { AccountVm } from "./gen/AccountVm";
import type { ApprovalDraftVm } from "./gen/ApprovalDraftVm";
import type { BackupStatus } from "./gen/BackupStatus";
import type { BbctlAvailabilityVm } from "./gen/BbctlAvailabilityVm";
import type { BbctlProgressVm } from "./gen/BbctlProgressVm";
import type { BridgeDiscoveryVm } from "./gen/BridgeDiscoveryVm";
import type { BridgeHealthSnapshot } from "./gen/BridgeHealthSnapshot";
import type { BridgeLoginInput } from "./gen/BridgeLoginInput";
import type { BridgeLoginVm } from "./gen/BridgeLoginVm";
import type { BridgeNetworkVm } from "./gen/BridgeNetworkVm";
import type { CapabilitiesVm } from "./gen/CapabilitiesVm";
import type { ConnectionStatusBatch } from "./gen/ConnectionStatusBatch";
import type { CouplingCaveatVm } from "./gen/CouplingCaveatVm";
import type { DraftMirrorBatch } from "./gen/DraftMirrorBatch";
import type { EditVersionVm } from "./gen/EditVersionVm";
import type { EncryptionStatusBatch } from "./gen/EncryptionStatusBatch";
import type { ExportProgressVm } from "./gen/ExportProgressVm";
import type { ExportRequestVm } from "./gen/ExportRequestVm";
import type { HotkeyVm } from "./gen/HotkeyVm";
import type { InboxBatch } from "./gen/InboxBatch";
import type { IncognitoVm } from "./gen/IncognitoVm";
import type { MenuSectionVm } from "./gen/MenuSectionVm";
import type { NetworksSnapshot } from "./gen/NetworksSnapshot";
import type { NewChatResolutionVm } from "./gen/NewChatResolutionVm";
import type { OutboxVm } from "./gen/OutboxVm";
import type { PaginationStatusBatch } from "./gen/PaginationStatusBatch";
import type { PaletteMode } from "./gen/PaletteMode";
import type { PaletteResultsVm } from "./gen/PaletteResultsVm";
import type { RemoteDraftVm } from "./gen/RemoteDraftVm";
import type { ResolveSupportVm } from "./gen/ResolveSupportVm";
import type { RoomListBatch } from "./gen/RoomListBatch";
import type { SearchFilterVm } from "./gen/SearchFilterVm";
import type { SearchHitVm } from "./gen/SearchHitVm";
import type { SpacesSnapshot } from "./gen/SpacesSnapshot";
import type { TimelineBatch } from "./gen/TimelineBatch";
import type { TypingBatch } from "./gen/TypingBatch";
import type { VerificationFlowVm } from "./gen/VerificationFlowVm";

/**
 * Structural guard for the {@link IpcError} envelope so we can rethrow it
 * faithfully rather than as an opaque value.
 */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    typeof candidate.retriable === "boolean"
  );
}

/**
 * Typed one-shot command invocation. Resolves with the command's view model or
 * rejects with the {@link IpcError} envelope (never a raw string).
 */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (raw) {
    if (isIpcError(raw)) {
      throw raw;
    }
    // Backend contract guarantees an IpcError; anything else is unexpected.
    throw {
      code: "internal",
      message: typeof raw === "string" ? raw : "unexpected IPC failure",
      accountId: null,
      retriable: false,
    } satisfies IpcError;
  }
}

/**
 * Fetch the per-platform capability handshake (Story 12.2). A one-shot read of the
 * Rust-authored {@link CapabilitiesVm}: one boolean per optional platform surface
 * (tray icon, global hotkey, launch-at-login, in-app updater, native menu bar,
 * bridge sidecar, reveal-in-file-manager), where `false` means the surface is
 * absent on this build. The frontend mirrors this into the capabilities store at
 * startup and NEVER derives platform facts from user agents or build flags —
 * Rust is the only authority. Rejects with the {@link IpcError} envelope.
 */
export async function capabilities(): Promise<CapabilitiesVm> {
  return await invoke<CapabilitiesVm>("capabilities");
}

/**
 * Fetch the data-driven bridge catalog (FR-42, Story 6.1). A one-shot read of the
 * embedded, versioned risk-tier data, projected in the Rust core into the flat set
 * of surfaced {@link BridgeNetworkVm}s (the out-of-scope tier is excluded). Every
 * risk tier, tier label, badge style, and acknowledgment copy is authored in the
 * backend data files — never hardcoded here. Resolves with the catalog; rejects
 * with the {@link IpcError} envelope (`code: "internal"`) only if the embedded data
 * fails to parse or validate, so the Bridges view can show an error state.
 */
export async function bridgeCatalog(): Promise<BridgeNetworkVm[]> {
  return await invoke<BridgeNetworkVm[]>("bridge_catalog");
}

/**
 * Run zero-config, per-Account bridge discovery (FR-25, AD-16, Story 6.2). A
 * one-shot pass in the Rust core that merges three sources — `thirdparty/protocols`,
 * a known-bot MXID probe, and a joined-room `m.bridge` portal / bot-DM scan — into a
 * per-Network status, catalog-gated to the surfaced 6.1 networks. Resolves with a
 * {@link BridgeDiscoveryVm} (the account's `homeserver` server name + the discovered
 * networks; an empty `networks` array is the honest "no bridges found" state, not an
 * error). A homeserver lacking `thirdparty/protocols` degrades to the other sources
 * rather than erroring. Rejects with the {@link IpcError} envelope: an unknown account
 * → `code: "internal"` (non-retriable); a total transport failure → `code:
 * "syncUnavailable"` (`retriable: true`). No bot Matrix ID is ever named by the user or
 * returned.
 */
export async function bridgeDiscover(accountId: string): Promise<BridgeDiscoveryVm> {
  return await invoke<BridgeDiscoveryVm>("bridge_discover", { accountId });
}

/**
 * Start a native bridge login for `networkId` (FR-26, AD-16, Story 6.3). Opens a
 * streaming subscription: the Rust core connects the provisioning transport
 * (authenticated with the account's Matrix access token as Bearer, read in Rust
 * and never crossing IPC), drives the bridgev2 login state machine, and forwards
 * each {@link BridgeLoginVm} snapshot to `onState`. Resolves with the `sessionId`
 * used to {@link submitBridgeLogin} / {@link cancelBridgeLogin}. Rejects with the
 * {@link IpcError} envelope: an unreachable provisioning API → `syncUnavailable`
 * (retriable). Only rendered VM state crosses IPC — never the token or a cookie.
 */
export async function startBridgeLogin(
  accountId: string,
  networkId: string,
  onState: (vm: BridgeLoginVm) => void,
): Promise<number> {
  return await subscribe<BridgeLoginVm>("bridge_login_start", onState, {
    accountId,
    networkId,
  });
}

/**
 * Submit input into a running bridge login (Story 6.3): a flow choice (from the
 * choosing-method phase) or the entered field values (from the code-entry phase).
 * Entered values ride only inside the {@link BridgeLoginInput} and are never
 * logged. Rejects with the {@link IpcError} envelope when the session has ended.
 */
export async function submitBridgeLogin(
  accountId: string,
  sessionId: number,
  input: BridgeLoginInput,
): Promise<void> {
  await invoke<void>("bridge_login_submit", { accountId, sessionId, input });
}

/**
 * Cancel a running bridge login (Story 6.3) — the user closed the Sheet / pressed
 * Esc. Drops the session, best-effort POSTs `/login/cancel` on the bridge (when the
 * login id has resolved), then aborts the driver task. Idempotent — cancelling an
 * unknown session is a no-op.
 */
export async function cancelBridgeLogin(accountId: string, sessionId: number): Promise<void> {
  await invoke<void>("bridge_login_cancel", { accountId, sessionId });
}

/**
 * Resolve-or-create the Bridge Bot DM room for `networkId` (FR-27, UX-DR19, Story
 * 6.4) and resolve with its room id — the manual escape hatch to the raw Bridge Bot
 * chat, offered from the card Manage menu and a login failure. The frontend
 * navigates to it via `primaryViewStore.setView("inbox")` + `roomsStore.selectRoom`.
 * Rejects with the {@link IpcError} envelope: an unknown account → `internal`; an
 * unresolvable / uncreatable bot DM → `syncUnavailable` (retriable). No bot Matrix ID
 * or session material crosses IPC — only the room id.
 */
export async function bridgeBotRoom(accountId: string, networkId: string): Promise<string> {
  return await invoke<string>("bridge_bot_room", { accountId, networkId });
}

/**
 * Fetch the `bbctl` self-host capability for the "Run your own bridge" surface
 * (FR-29, Story 6.7). A one-shot read of the embedded `bbctl.json` (guided-install
 * steps + the supported self-hostable networks) plus the live sidecar availability
 * probe, projected into a {@link BbctlAvailabilityVm}. `available: false` renders the
 * guided-install branch and everything else in keeper keeps working. Rejects with the
 * {@link IpcError} envelope (`code: "internal"`) only if the embedded data fails to
 * parse/validate. No token or process material crosses IPC.
 */
export async function bbctlAvailability(): Promise<BbctlAvailabilityVm> {
  return await invoke<BbctlAvailabilityVm>("bbctl_availability");
}

/**
 * Start a `bbctl` self-hosted-bridge run for `networkId` (FR-29, AD-16, Story 6.7).
 * Opens a streaming subscription: the Rust core gates the account (Beeper-only, read
 * from the durable registry `provider` — never a token) and the network, then drives
 * `bbctl register`/`run` as a launch-on-demand sidecar and forwards each
 * {@link BbctlProgressVm} snapshot (checking → registering → starting → running →
 * success/failure) to `onState`. Resolves with the `sessionId` used to
 * {@link bbctlRunCancel}. Rejects with the {@link IpcError} envelope: a non-Beeper
 * gate / unsupported network / absent sidecar → `syncUnavailable` (retriable). Only
 * rendered VM state crosses IPC — never the token or a raw `bbctl` log line.
 */
export async function bbctlRunStart(
  accountId: string,
  networkId: string,
  onState: (vm: BbctlProgressVm) => void,
): Promise<number> {
  return await subscribe<BbctlProgressVm>("bbctl_run_start", onState, {
    accountId,
    networkId,
  });
}

/**
 * Cancel a running `bbctl` self-hosted-bridge run (Story 6.7) — the user closed the
 * run Sheet. Aborts keeper's streaming driver task and removes it from the runs
 * registry. Idempotent — cancelling an unknown session is a no-op. (The launched
 * `bbctl run` daemon is launch-and-leave, so this tears down only keeper's streaming
 * task, not the already-detached bridge process — supervision is out of scope, v1.x.)
 */
export async function bbctlRunCancel(sessionId: number): Promise<void> {
  await invoke<void>("bbctl_run_cancel", { sessionId });
}

/**
 * Fetch the data-driven new-chat resolve capability for `networkId` (FR-32, Story
 * 6.6). A one-shot, I/O-free read of the embedded `resolve-support.json`
 * (override-or-default), projected into a {@link ResolveSupportVm}. The new-chat
 * dialog disables the identifier field and shows "not supported on {Network}" upfront
 * when `supported` is `false`, before any resolve call. The `identifierHint` /
 * `placeholder` copy is authored in the backend data file — never hardcoded here.
 * Rejects with the {@link IpcError} envelope (`code: "internal"`) only if the embedded
 * data fails to parse or validate.
 */
export async function bridgeResolveSupport(networkId: string): Promise<ResolveSupportVm> {
  return await invoke<ResolveSupportVm>("bridge_resolve_support", { networkId });
}

/**
 * Resolve a new-chat `identifier` on `networkId` through the bridge's provisioning
 * API (FR-32, Story 6.6) and resolve with the portal {@link NewChatResolutionVm} to
 * open. The Rust core connects the provisioning transport (Matrix access token as
 * Bearer, read in Rust and never crossing IPC), calls `resolve_identifier` then
 * `create_dm` only if no DM exists yet, and returns only the non-secret room id —
 * opened verbatim via `roomsStore.selectRoom`. Rejects with the {@link IpcError}
 * envelope: an unknown account → `internal`; a bot-only account or an unresolvable
 * identifier → `syncUnavailable` (retriable) carrying the bridge's own verbatim
 * message, so the dialog can render "Not found on {Network}" and retain the input.
 */
export async function resolveBridgeIdentifier(
  accountId: string,
  networkId: string,
  identifier: string,
): Promise<NewChatResolutionVm> {
  return await invoke<NewChatResolutionVm>("resolve_bridge_identifier", {
    accountId,
    networkId,
    identifier,
  });
}

/**
 * Password login (FR-1, FR-5). Sends the homeserver, username, and password to
 * the Rust core, which runs the store-less SSS probe, logs in, persists the
 * session to the Keychain, and writes the account registry row. Resolves with
 * the non-secret {@link AccountVm}; rejects with the {@link IpcError} envelope
 * (whose `code` distinguishes bad credentials / unreachable / unsupported login
 * type / non-SSS). The password is transient — it is never returned or stored.
 */
export async function loginPassword(
  homeserver: string,
  username: string,
  password: string,
): Promise<AccountVm> {
  return await invoke<AccountVm>("login_password", { homeserver, username, password });
}

/**
 * OIDC (OAuth 2.0 / MSC3861) login (Story 2.2). Sends the homeserver to the Rust
 * core, which runs the store-less SSS probe, opens the system browser for OAuth
 * consent, awaits the `keeper://oauth/callback` deep link, finishes the token
 * exchange, persists the session to the Keychain, and writes the registry row.
 * Resolves with the non-secret {@link AccountVm}; rejects with the
 * {@link IpcError} envelope (whose `code` distinguishes non-SSS / OIDC
 * unsupported / timed out / cancelled / failed). No token or authorization
 * `code`/`state` ever crosses back to JavaScript.
 *
 * This call stays pending for the whole browser round-trip; use
 * {@link cancelOidc} to abort it.
 */
export async function loginOidc(homeserver: string): Promise<AccountVm> {
  return await invoke<AccountVm>("login_oidc", { homeserver });
}

/**
 * Cancel any in-progress OIDC flow (Story 2.2). The pending {@link loginOidc}
 * call then rejects with `code: "oauthCancelled"` and the Rust core rolls back
 * any partial state. Idempotent — a no-op when no flow is pending.
 */
export async function cancelOidc(): Promise<void> {
  await invoke<void>("cancel_oidc");
}

/**
 * Request a Beeper email login code (Story 2.3, step 1). Sends the email to the
 * Rust core, which runs Beeper's unofficial `POST /user/login` → `POST
 * /user/login/email` and stores the intermediate request id server-side (keyed
 * by email) so it never crosses IPC. Resolves once a code has been emailed;
 * rejects with the {@link IpcError} envelope (`code: "beeperUnavailable"`,
 * `retriable: true`) on any Beeper failure — a non-2xx, timeout, transport error,
 * or a private-API shape change. No bearer token or request id crosses IPC.
 */
export async function beeperRequestCode(email: string): Promise<void> {
  await invoke<void>("beeper_request_code", { email });
}

/**
 * Complete a Beeper email-code login (Story 2.3, step 2). Sends the email and the
 * emailed code to the Rust core, which takes the stored request id, runs `POST
 * /user/login/response` to obtain the Beeper JWT, then completes login via
 * `org.matrix.login.jwt` against `matrix.beeper.com` through the shared
 * add-account pipeline. Resolves with the non-secret {@link AccountVm}; rejects
 * with the {@link IpcError} envelope (`code: "beeperUnavailable"`, `retriable:
 * true`) on any Beeper failure (including an abandoned flow with no stored
 * request id). The emailed `code` is transient — never returned or stored.
 */
export async function loginBeeper(email: string, code: string): Promise<AccountVm> {
  return await invoke<AccountVm>("login_beeper", { email, code });
}

/**
 * Cancel any in-progress Beeper login flow (Story 2.3). The Rust core clears the
 * registry so no pending request id lingers; nothing is persisted. Idempotent —
 * a no-op when no flow is pending.
 */
export async function cancelBeeper(): Promise<void> {
  await invoke<void>("cancel_beeper");
}

/**
 * Persist the app-wide at-rest encryption posture (Story 2.6, AD-22). Sends the
 * chosen posture (`true` = encrypt SDK stores with a per-account passphrase,
 * `false` = FileVault only) to the Rust core, which writes it to `keeper.db`. The
 * passphrase itself is generated and stored (Keychain only) later, inside the
 * next account add — nothing secret crosses IPC. Resolves once persisted.
 */
export async function setEncryptionPosture(enabled: boolean): Promise<void> {
  await invoke<void>("set_encryption_posture", { enabled });
}

/**
 * Read the app-wide at-rest encryption posture (Story 2.6). Resolves with `true`
 * (on), `false` (off), or `null` (unchosen — the fresh-install state that gates
 * the first-run choice). The Rust `Option<bool>` serializes to `boolean | null`.
 */
export async function encryptionPosture(): Promise<boolean | null> {
  return await invoke<boolean | null>("encryption_posture");
}

/**
 * Read a message's edit history from the Local Archive (FR-11, Story 5.2).
 * `itemKey` is the message's opaque render `key` (`unique_id`); the Rust core
 * resolves it to the original event id and reads the version chain from
 * `archive.db` — never a homeserver fetch. Resolves with an ordered
 * {@link EditVersionVm}[] (oldest→newest, the last flagged `isCurrent`), or an
 * empty array when the item is unresolvable or has no local history.
 */
export async function getEditHistory(
  accountId: string,
  roomId: string,
  itemKey: string,
): Promise<EditVersionVm[]> {
  return await invoke<EditVersionVm[]>("edit_history_get", { accountId, roomId, itemKey });
}

/**
 * Read the app-wide "honor remote deletions locally" policy (FR-36, Story 5.2).
 * Resolves with `true` only when explicitly enabled; absent/off ⇒ `false`
 * (preserve). Read-time policy only — flipping it is never retroactive.
 */
export async function honorRemoteDeletions(): Promise<boolean> {
  return await invoke<boolean>("honor_remote_deletions");
}

/**
 * Persist the app-wide "honor remote deletions locally" policy (FR-36, Story
 * 5.2). Affects subsequent reads only (not retroactive). Resolves once persisted.
 */
export async function setHonorRemoteDeletions(enabled: boolean): Promise<void> {
  await invoke<void>("set_honor_remote_deletions", { enabled });
}

/**
 * Persist the composer draft for `(accountId, roomId)` (Story 7.1, AD-15). Upserts
 * the trimmed `body` into the `drafts` table in `keeper.db`. Called fire-and-forget
 * on the debounced keystroke path, so callers `void` it and never await — a failure
 * must never block typing. Resolves once persisted. The body is never logged.
 */
export async function saveDraft(accountId: string, roomId: string, body: string): Promise<void> {
  await invoke<void>("set_draft", { accountId, roomId, body });
}

/**
 * Read the composer draft for `(accountId, roomId)` (Story 7.1). Resolves with the
 * stored body, or `null` when no draft exists (the Rust `Option<String>` serializes
 * to `string | null`). The composer seeds its local state from this on mount.
 */
export async function loadDraft(accountId: string, roomId: string): Promise<string | null> {
  return await invoke<string | null>("get_draft", { accountId, roomId });
}

/**
 * Delete the composer draft for `(accountId, roomId)` (Story 7.1). Idempotent — a
 * no-op when no draft exists (a successful send, or the body trimmed to empty).
 * Fired fire-and-forget alongside the keystroke path; callers `void` it.
 */
export async function clearDraft(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("delete_draft", { accountId, roomId });
}

/**
 * List every draft's `(accountId, roomId)` key (Story 7.1). Presence only — the body
 * is not returned. Seeds the inbox draft markers at startup, cross-account. The Rust
 * `Vec<(String, String)>` serializes to `[accountId, roomId][]`.
 */
export async function listDrafts(): Promise<Array<[string, string]>> {
  return await invoke<Array<[string, string]>>("list_drafts");
}

/**
 * Mirror the composer draft for `(accountId, roomId)` to the account (Story 7.2,
 * AD-15): the synced `dev.keeper.draft` account data plus a best-effort
 * `save_composer_draft` (Element interop). The Rust core dedupes by last-mirrored
 * body and generates the `updatedTs` at write time. Called fire-and-forget on a
 * looser debounce than the local save, so callers `void` it and never await — a
 * mirror failure must never block typing or local persistence. The body is never
 * logged.
 */
export async function mirrorDraft(accountId: string, roomId: string, body: string): Promise<void> {
  await invoke<void>("mirror_draft", { accountId, roomId, body });
}

/**
 * Clear the draft mirror for `(accountId, roomId)` (Story 7.2): tombstone the
 * `dev.keeper.draft` account data plus `clear_composer_draft`, so other devices stop
 * showing the draft. Fired fire-and-forget on the send/clear path; callers `void` it.
 * A failure never blocks the clear and can at worst transiently re-present a cleared
 * draft cross-device (never destroys text).
 */
export async function clearDraftMirror(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("clear_draft_mirror", { accountId, roomId });
}

/**
 * Read the remote (cross-device) draft for `(accountId, roomId)` from the account-data
 * mirror (Story 7.2), or `null` when there is no draft (an empty-body tombstone reads
 * back as `null`). Read only to *offer* adoption — local always wins; the composer
 * never auto-replaces non-empty local text. A failure rejects with the {@link IpcError}
 * envelope; the composer falls back to local.
 */
export async function loadRemoteDraft(
  accountId: string,
  roomId: string,
): Promise<RemoteDraftVm | null> {
  return await invoke<RemoteDraftVm | null>("load_remote_draft", { accountId, roomId });
}

/**
 * List every pending draft across all accounts for the approval pane (Story 7.3).
 * Resolves with one {@link ApprovalDraftVm} per pending draft, enriched with the
 * owning account's identity/hue and the room's display name + bridge network. A
 * draft whose room/account cannot be resolved (account offline) is STILL listed
 * (`displayName = roomId`, `network = null`) — the airlock never hides held text.
 * Bodies stay authoritative in Rust. Rejects with the {@link IpcError} envelope on
 * a backend failure.
 */
export async function listPendingDrafts(): Promise<ApprovalDraftVm[]> {
  return await invoke<ApprovalDraftVm[]>("list_pending_drafts");
}

/**
 * Approve (send) a pending draft's `body` to `(accountId, roomId)` through the
 * single dispatch gate with the `ApprovalPaneApprove` trigger (FR-41, AD-13, Story
 * 7.3). Resolves once enqueued; the local echo arrives over the existing timeline
 * subscription. Rejects with the {@link IpcError} envelope on an enqueue failure —
 * callers MUST retain the draft on rejection so a failed send never loses text.
 */
export async function approveDraft(accountId: string, roomId: string, body: string): Promise<void> {
  await invoke<void>("approve_draft", { accountId, roomId, body });
}

/**
 * Search the Local Archive with full-text search (FR-34, AD-12, Story 5.3).
 * Runs fully offline against `archive.db` — never a homeserver fetch, no live
 * session required. Queries of 3+ characters use the trigram FTS index; shorter
 * queries fall back to an accelerated `LIKE` scan. All {@link SearchFilterVm}
 * filters are optional (empty `accountIds`/`roomIds` mean unrestricted). Resolves
 * with at most one {@link SearchHitVm} per logical message (chain-root `eventId`
 * for deep-linking), ordered newest-first, or an empty array when nothing matches.
 */
export async function searchArchive(filter: SearchFilterVm): Promise<SearchHitVm[]> {
  return await invoke<SearchHitVm[]>("search_archive", { filter });
}

/**
 * Start a background archive export (FR-35, AD-11, Story 5.5). Opens a `Channel`,
 * forwards each {@link ExportProgressVm} to `onProgress` in arrival order
 * (`running` heartbeats with live counts, then exactly one terminal
 * `completed`/`cancelled`/`failed` batch), and resolves with the backend-assigned
 * `exportId` (the handle {@link cancelExport} sets the cancel flag for). The job
 * reads `archive.db` only and never blocks messaging; media bytes are best-effort
 * (unresolvable ones are skipped-and-counted). Rejects with the {@link IpcError}
 * envelope only on a setup failure (the archive path / a malformed request) — a
 * runtime export failure arrives as the `failed` batch, not a rejection.
 */
export async function startExport(
  request: ExportRequestVm,
  onProgress: (batch: ExportProgressVm) => void,
): Promise<number> {
  return await subscribe<ExportProgressVm>("export_start", onProgress, { request });
}

/**
 * Cancel a running archive export by id (FR-35, Story 5.5). Sets the job's shared
 * cancel flag; the synchronous export loop stops at its next check, deletes partial
 * output, and streams the `cancelled` terminal batch over the original progress
 * channel. Idempotent — a no-op for an unknown / already-finished id. Rejects with
 * the {@link IpcError} envelope only on an unexpected backend failure.
 */
export async function cancelExport(exportId: number): Promise<void> {
  await invoke<void>("export_cancel", { exportId });
}

/**
 * Reveal an exported file in the OS file manager (FR-35, Story 5.5). `path` is one
 * of the completed export's `outputPaths`; the Rust core delegates to
 * `reveal_item_in_dir` (Finder on macOS). Rejects with the {@link IpcError}
 * envelope (`code: "internal"`) on an invalid / non-existent path — never a panic.
 */
export async function revealPath(path: string): Promise<void> {
  await invoke<void>("reveal_path", { path });
}

/**
 * Report every persisted account that can be restored on launch (FR-8, AD-20).
 * Identity only — the Rust core lists the registry rows and returns each whose
 * Keychain session is present as a non-secret {@link AccountVm} (with hue).
 * Resolves with an array (empty on a cold install); a row whose session is gone
 * is skipped. No token or session material ever crosses IPC.
 */
export async function sessionRestore(): Promise<AccountVm[]> {
  return await invoke<AccountVm[]>("session_restore");
}

/**
 * Report the live set of network destinations keeper contacts (Story 11.2, NFR-11,
 * UX-DR17). The Rust core reads the accounts registry (the same path
 * {@link sessionRestore} uses) and computes, from live state, each homeserver
 * (deduplicated), `api.beeper.com` exactly when a Beeper account exists, and the
 * signed-update endpoint. The Settings → About surface renders the returned
 * {@link EgressEndpointVm} list directly so keeper's egress claim is verifiable
 * rather than asserted — never hardcoded, never stale. Rejects with the
 * {@link IpcError} envelope on a registry read failure.
 */
export async function egressList(): Promise<EgressEndpointVm[]> {
  return await invoke<EgressEndpointVm[]>("egress_list");
}

/**
 * Sign out an account locally (AD-10, Story 1.8). The Rust core tears down the
 * account's live supervision tasks then deletes exactly its SDK store dir,
 * Keychain session entry, and registry row — no server-side logout, works
 * offline, idempotent. Rejects with the {@link IpcError} envelope on a cleanup
 * failure.
 */
export async function signOut(accountId: string): Promise<void> {
  await invoke<void>("sign_out", { accountId });
}

/**
 * Deliberately delete one account's local archive (Story 5.7, FR-6). The Rust
 * core routes the purge through the single serialized archive writer so only this
 * account's `events` rows and `events_fts` entries are removed — every other
 * account's history stays intact. This is the destructive counterpart to the
 * keep-archive {@link signOut}; the caller signs out first, then invokes this.
 * Rejects with the {@link IpcError} envelope on a purge failure.
 */
export async function deleteAccountArchive(accountId: string): Promise<void> {
  await invoke<void>("delete_account_archive", { accountId });
}

/**
 * Query the command palette (Story 9.1). Serves grouped, ranked, bounded results
 * from the in-memory Rust index over every room across all accounts (chats + DM
 * contacts) plus the static action registry — the frontend only renders and
 * dispatches by id, never filters or re-orders (AD-20). `mode` is `"default"`
 * (chats + contacts at ≥2 chars + actions) or `"action"` (the `>` prefix: actions
 * only, open-chat actions first when `openChat` is set). Never rejects on an empty
 * index — global actions always come back. Resolves with the {@link PaletteResultsVm}.
 */
export async function paletteQuery(
  query: string,
  mode: PaletteMode,
  openChat: boolean,
): Promise<PaletteResultsVm> {
  return await invoke<PaletteResultsVm>("palette_query", { query, mode, openChat });
}

/**
 * Fetch the category-grouped, toggle-collapsed shortcut reference for the ⌘? cheat
 * sheet (Story 9.3). A pure projection of the same `palette_actions()` registry the
 * palette consumes (`registry_sections()` in Rust), grouped by category with each
 * toggle pair (archive/unarchive, …) collapsed to one unambiguous row — no
 * hand-maintained list, so it never drifts from the palette or the native menu bar
 * (UX-DR15). Stateless and never fails. Resolves with the {@link MenuSectionVm}[].
 */
export async function cheatSheetSections(): Promise<MenuSectionVm[]> {
  return await invoke<MenuSectionVm[]>("cheat_sheet_sections");
}

/**
 * Open a streaming subscription. Creates a `Channel`, forwards each delivered
 * batch to `onBatch` in arrival order (snapshot before any diff, per AD-8), and
 * resolves with the backend-assigned subscription id.
 */
export async function subscribe<TBatch>(
  cmd: string,
  onBatch: (batch: TBatch) => void,
  args?: Record<string, unknown>,
): Promise<number> {
  const channel = new Channel<TBatch>();
  // Arm `onmessage` BEFORE invoking: this ordering is load-bearing. The demo
  // command delivers synchronously, but real streams will emit asynchronously
  // from a spawned task after the id-returning command resolves — batches sent
  // before the handler is set would be dropped. Keep this order when copying.
  channel.onmessage = onBatch;
  return await invoke<number>(cmd, { ...args, channel });
}

/**
 * Subscribe to an account's sliding-sync room list (FR-8, AD-8). Opens a
 * `Channel`, forwards each {@link RoomListBatch} to `onBatch` in arrival order
 * (a `Reset` snapshot before any diff), and resolves with the subscription id.
 * Rejects with the {@link IpcError} envelope (`code: "syncUnavailable"`) if the
 * account cannot start syncing.
 */
export async function subscribeRoomList(
  accountId: string,
  onBatch: (batch: RoomListBatch) => void,
): Promise<number> {
  return await subscribe<RoomListBatch>("room_list_subscribe", onBatch, { accountId });
}

/**
 * Unsubscribe exactly one room-list subscription, aborting its backend producer
 * task (AD-19). Idempotent — unsubscribing an unknown id is a no-op.
 */
export async function unsubscribeRoomList(accountId: string, id: number): Promise<void> {
  await invoke<void>("room_list_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Subscribe to the merged unified inbox across every restorable account (FR-18,
 * AD-20, Story 4.2 + 4.3 + 4.4). Opens **four** `Channel`s over one subscription
 * and forwards the recency-ordered Inbox window to `onInbox`, the Archive window
 * to `onArchive`, the Pins window (pinned rooms, user order) to `onPins`, and the
 * Favorites window (favourited rooms, recency order) to `onFavourites` (each a
 * `Reset` window that updates as accounts sync or as archive/pin/favourite state
 * changes). Resolves with the inbox subscription id — one
 * {@link unsubscribeInbox} tears down all four. Ordering and the four-way split
 * are computed in Rust — never re-derived here. Rejects with the {@link IpcError}
 * envelope (`code: "syncUnavailable"`) on a stream-start failure.
 *
 * All channels arm their `onmessage` before `invoke` (the ordering is
 * load-bearing per AD-8, so no batch sent by a spawned task is dropped). The Rust
 * command's params are `channel` (inbox), `archive`, `pins`, `favourites`,
 * `spaces`, and `networks`. The fifth channel (Story 4.5) delivers the aggregated
 * Space list as a whole {@link SpacesSnapshot}; the sixth (Story 4.6) delivers the
 * distinct-Networks list as a whole {@link NetworksSnapshot} (no diff protocol for
 * either — the frontend replaces its list).
 */
export async function subscribeInbox(
  onInbox: (batch: InboxBatch) => void,
  onArchive: (batch: InboxBatch) => void,
  onPins: (batch: InboxBatch) => void,
  onFavourites: (batch: InboxBatch) => void,
  onSpaces: (snapshot: SpacesSnapshot) => void,
  onNetworks: (snapshot: NetworksSnapshot) => void,
): Promise<number> {
  const channel = new Channel<InboxBatch>();
  const archive = new Channel<InboxBatch>();
  const pins = new Channel<InboxBatch>();
  const favourites = new Channel<InboxBatch>();
  const spaces = new Channel<SpacesSnapshot>();
  const networks = new Channel<NetworksSnapshot>();
  channel.onmessage = onInbox;
  archive.onmessage = onArchive;
  pins.onmessage = onPins;
  favourites.onmessage = onFavourites;
  spaces.onmessage = onSpaces;
  networks.onmessage = onNetworks;
  return await invoke<number>("inbox_subscribe", {
    channel,
    archive,
    pins,
    favourites,
    spaces,
    networks,
  });
}

/**
 * Set (or clear) the ephemeral Space filter on the merged inbox (Story 4.5,
 * FR-22). Pass an `accountId` + `spaceId` to narrow every inbox window to that
 * Space's joined children (the Rust merger re-emits all four windows filtered);
 * pass `null`/`null` to clear and restore the full inbox. The selection is
 * ephemeral — never persisted, cleared on relaunch. Best-effort: callers may
 * fire-and-forget and swallow rejection (the stream is truth). Rejects with the
 * {@link IpcError} envelope only on an unexpected backend failure.
 */
export async function setSpaceFilter(
  accountId: string | null,
  spaceId: string | null,
): Promise<void> {
  await invoke<void>("set_space_filter", { accountId, spaceId });
}

/**
 * Set (or clear) the ephemeral Network filter on the merged inbox (Story 4.6,
 * FR-24). Pass a Network `name` to narrow every inbox window to rooms bridged to
 * that Network (the Rust merger re-emits all four windows filtered, across all
 * accounts — the selection is name-keyed); pass `null` to clear and restore the
 * full inbox. Composes AND with any active Space filter. The selection is ephemeral
 * — never persisted, cleared on relaunch. Best-effort: callers may fire-and-forget
 * and swallow rejection (the stream is truth). Rejects with the {@link IpcError}
 * envelope only on an unexpected backend failure.
 */
export async function setNetworkFilter(network: string | null): Promise<void> {
  await invoke<void>("set_network_filter", { network });
}

/**
 * Unsubscribe the merged inbox, aborting every per-account producer feeding it
 * (AD-20). Idempotent — a mismatched/unknown id is a no-op. Covers the Inbox,
 * Archive, Pins, and Favorites channels (Story 4.2 + 4.3 + 4.4).
 */
export async function unsubscribeInbox(id: number): Promise<void> {
  await invoke<void>("inbox_unsubscribe", { subscriptionId: id });
}

/**
 * Subscribe to live bridge-session health across every active account (Story 6.5,
 * FR-28, NFR-6, AD-16). Opens a `Channel` and forwards each whole-set
 * {@link BridgeHealthSnapshot} to `onSnapshot` — the bootstrap snapshot on subscribe,
 * then only on a per-session state change (diffed in Rust). Resolves with the
 * subscription id; {@link unsubscribeBridgeHealth} tears it down. Health is computed
 * entirely in Rust — the frontend mirrors the stream and never re-derives it. Never
 * rejects (a per-account discovery/monitor failure is skipped in the core).
 */
export async function subscribeBridgeHealth(
  onSnapshot: (snapshot: BridgeHealthSnapshot) => void,
): Promise<number> {
  return await subscribe<BridgeHealthSnapshot>("bridge_subscribe_health", onSnapshot);
}

/**
 * Unsubscribe the bridge-health subscription (Story 6.5), draining every per-account
 * monitor. Idempotent — a mismatched/unknown id is a no-op.
 */
export async function unsubscribeBridgeHealth(id: number): Promise<void> {
  await invoke<void>("bridge_unsubscribe_health", { subscriptionId: id });
}

/**
 * Subscribe to a room's timeline (FR-8, FR-9, AD-4/AD-8). Opens a `Channel`,
 * forwards each {@link TimelineBatch} to `onBatch` in arrival order (a `Reset`
 * snapshot before any diff), and resolves with the subscription id. Rejects with
 * the {@link IpcError} envelope (`code: "timelineUnavailable"`) if the room's
 * timeline cannot be opened.
 */
export async function subscribeTimeline(
  accountId: string,
  roomId: string,
  onBatch: (batch: TimelineBatch) => void,
): Promise<number> {
  return await subscribe<TimelineBatch>("timeline_subscribe", onBatch, { accountId, roomId });
}

/**
 * Unsubscribe exactly one timeline subscription, aborting its backend producer
 * task and dropping its `Timeline` (AD-19). Idempotent — unsubscribing an
 * unknown id is a no-op.
 */
export async function unsubscribeTimeline(accountId: string, id: number): Promise<void> {
  await invoke<void>("timeline_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Subscribe to an account's connection status (FR-8/FR-9, UX-DR18, AD-8). Opens a
 * `Channel`, forwards each {@link ConnectionStatusBatch} to `onBatch` in arrival
 * order (an initial snapshot before any change), and resolves with the
 * subscription id. Rejects with the {@link IpcError} envelope (`code:
 * "syncUnavailable"`) if the account cannot start syncing.
 */
export async function subscribeConnectionStatus(
  accountId: string,
  onBatch: (batch: ConnectionStatusBatch) => void,
): Promise<number> {
  return await subscribe<ConnectionStatusBatch>("connection_status_subscribe", onBatch, {
    accountId,
  });
}

/**
 * Unsubscribe exactly one connection-status subscription, aborting its backend
 * producer task (AD-19). Idempotent — unsubscribing an unknown id is a no-op.
 */
export async function unsubscribeConnectionStatus(accountId: string, id: number): Promise<void> {
  await invoke<void>("connection_status_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Subscribe to live remote draft edits across every account (Story 7.2, AD-15).
 * App-wide (not per account): opens a `Channel`, forwards each {@link DraftMirrorBatch}
 * to `onBatch` in arrival order, and resolves with the subscription id. The frontend
 * pumps these into the drafts store's `remote` map for local-wins conflict detection.
 * There is exactly one such subscription for the app's lifetime.
 */
export async function subscribeDraftMirror(
  onBatch: (batch: DraftMirrorBatch) => void,
): Promise<number> {
  return await subscribe<DraftMirrorBatch>("draft_mirror_subscribe", onBatch);
}

/**
 * Unsubscribe exactly one draft-mirror subscription, aborting its backend relay task
 * (Story 7.2). Idempotent — unsubscribing an unknown id is a no-op.
 */
export async function unsubscribeDraftMirror(id: number): Promise<void> {
  await invoke<void>("draft_mirror_unsubscribe", { subscriptionId: id });
}

/**
 * Subscribe to an account's encryption (device-verification) status (Story 3.1,
 * AD-8). Opens a `Channel`, forwards each {@link EncryptionStatusBatch} to
 * `onBatch` in arrival order (an initial snapshot before any change), and resolves
 * with the subscription id. Rejects with the {@link IpcError} envelope (`code:
 * "syncUnavailable"`) if the account cannot start syncing.
 */
export async function subscribeEncryptionStatus(
  accountId: string,
  onBatch: (batch: EncryptionStatusBatch) => void,
): Promise<number> {
  return await subscribe<EncryptionStatusBatch>("encryption_status_subscribe", onBatch, {
    accountId,
  });
}

/**
 * Unsubscribe exactly one encryption-status subscription, aborting its backend
 * producer task (AD-19). Idempotent — unsubscribing an unknown id is a no-op.
 */
export async function unsubscribeEncryptionStatus(accountId: string, id: number): Promise<void> {
  await invoke<void>("encryption_status_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Subscribe to an account's interactive device self-verification flow (Story 3.2,
 * FR-14, AD-1). Opens a `Channel`, forwards each {@link VerificationFlowVm}
 * snapshot to `onBatch` in arrival order (the flow's state machine: waiting →
 * compare emoji / show QR → confirmed → done/cancelled/failed), and resolves with
 * the subscription id. An *incoming* request the peer started surfaces here as a
 * `requested` snapshot so the UI can auto-open the modal. NO crypto/key/plaintext
 * crosses IPC — only the rendered VM. Rejects with the {@link IpcError} envelope
 * (`code: "syncUnavailable"`) if the account cannot start syncing.
 */
export async function subscribeVerification(
  accountId: string,
  onBatch: (batch: VerificationFlowVm) => void,
): Promise<number> {
  return await subscribe<VerificationFlowVm>("verification_subscribe", onBatch, { accountId });
}

/**
 * Unsubscribe exactly one verification subscription, aborting its backend producer
 * task and clearing the account's flow sender (AD-19). Idempotent — unsubscribing
 * an unknown id is a no-op.
 */
export async function unsubscribeVerification(accountId: string, id: number): Promise<void> {
  await invoke<void>("verification_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Start an interactive self-verification from keeper against the user's other
 * session (Story 3.2, FR-14). The Rust core requests the verification and streams
 * the resulting flow over the existing verification subscription. Requires an
 * active verification subscription. Rejects with the {@link IpcError} envelope
 * (`code: "verificationFailed"`) on failure.
 */
export async function verificationStart(accountId: string): Promise<void> {
  await invoke<void>("verification_start", { accountId });
}

/**
 * Accept an incoming verification request the peer started (Story 3.2). Moves the
 * flow from `requested` to `ready`. `flowId` is the flow's opaque id from the
 * streamed {@link VerificationFlowVm}. Rejects with the {@link IpcError} envelope
 * (`code: "verificationFailed"`) on failure.
 */
export async function verificationAccept(accountId: string, flowId: string): Promise<void> {
  await invoke<void>("verification_accept", { accountId, flowId });
}

/**
 * Start the emoji/SAS sub-flow on a ready request (Story 3.2). The SAS state
 * transition arrives over the verification stream. Rejects with the
 * {@link IpcError} envelope (`code: "verificationFailed"`) on failure.
 */
export async function verificationStartSas(accountId: string, flowId: string): Promise<void> {
  await invoke<void>("verification_start_sas", { accountId, flowId });
}

/**
 * Confirm the SAS emoji match on our side (Story 3.2). When both sides confirm,
 * the SDK completes verification and Story 3.1's encryption-status stream flips
 * the account to `verified`. Rejects with the {@link IpcError} envelope (`code:
 * "verificationFailed"`) on failure.
 */
export async function verificationConfirm(accountId: string, flowId: string): Promise<void> {
  await invoke<void>("verification_confirm", { accountId, flowId });
}

/**
 * Signal that the SAS emoji do NOT match (Story 3.2). Cancels the flow with the
 * SDK mismatch code, which surfaces as `failed`. Rejects with the {@link IpcError}
 * envelope (`code: "verificationFailed"`) on failure.
 */
export async function verificationMismatch(accountId: string, flowId: string): Promise<void> {
  await invoke<void>("verification_mismatch", { accountId, flowId });
}

/**
 * Cancel the verification flow (Story 3.2) — the user closed the modal or pressed
 * Esc. Cancels the active SAS or the request; a missing flow is a no-op. Rejects
 * with the {@link IpcError} envelope (`code: "verificationFailed"`) on failure.
 */
export async function verificationCancel(accountId: string, flowId: string): Promise<void> {
  await invoke<void>("verification_cancel", { accountId, flowId });
}

/**
 * Subscribe to an account's server-side key-backup status (Story 3.3, FR-14,
 * AD-8). Opens a `Channel`, forwards each {@link BackupStatus} to `onStatus` in
 * arrival order (an initial snapshot before any change), and resolves with the
 * subscription id. NO recovery key or secret-storage material crosses IPC — only
 * the enum tag. Rejects with the {@link IpcError} envelope (`code:
 * "syncUnavailable"`) if the account cannot start syncing.
 */
export async function subscribeBackupStatus(
  accountId: string,
  onStatus: (status: BackupStatus) => void,
): Promise<number> {
  return await subscribe<BackupStatus>("backup_status_subscribe", onStatus, { accountId });
}

/**
 * Unsubscribe exactly one backup-status subscription, aborting its backend
 * producer task (AD-19). Idempotent — unsubscribing an unknown id is a no-op.
 */
export async function unsubscribeBackupStatus(accountId: string, id: number): Promise<void> {
  await invoke<void>("backup_status_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Enable server-side key backup for the account (Story 3.3, FR-14). The Rust core
 * creates the backup + secret store and returns the base58 **recovery key** once —
 * the deliberate boundary exception, meant for the human to save (shown once in
 * `mono`, never persisted in a JS store beyond the modal's lifecycle). A race with
 * an existing server backup rejects with the {@link IpcError} envelope (`code:
 * "backupExists"`) so the modal can offer restore; any other failure rejects with
 * `code: "backupFailed"`.
 */
export async function backupEnable(accountId: string): Promise<string> {
  return await invoke<string>("backup_enable", { accountId });
}

/**
 * Restore from server-side key backup with a recovery key (Story 3.3, FR-14). The
 * Rust core opens the secret store and imports secrets; the SDK then downloads
 * room keys automatically, so Story 3.1's streams re-render previously
 * undecryptable rows with no extra code. An invalid key rejects with the
 * {@link IpcError} envelope carrying a *named* code — `"backupMalformedKey"` (not
 * decodable) vs `"backupIncorrectKey"` (well-formed but wrong) — never a generic
 * failure. `recoveryKey` is transient — never stored in a JS store beyond the
 * modal's lifecycle.
 */
export async function backupRestore(accountId: string, recoveryKey: string): Promise<void> {
  await invoke<void>("backup_restore", { accountId, recoveryKey });
}

/**
 * Save a recovery key to the OS Keychain (Story 3.3, FR-14) — the user's opt-in
 * after seeing the key once. The Rust core writes it at `recovery_key/<accountId>`
 * via the platform keychain port. Rejects with the {@link IpcError} envelope on a
 * write failure so the modal can keep the key visible for manual copy.
 */
export async function backupSaveRecoveryKey(accountId: string, recoveryKey: string): Promise<void> {
  await invoke<void>("backup_save_recovery_key", { accountId, recoveryKey });
}

/**
 * Read a previously-saved recovery key from the OS Keychain (Story 3.3) to prefill
 * the restore textarea, or `null` if none was saved. The Rust `Option<String>`
 * serializes to `string | null`.
 */
export async function backupSavedRecoveryKey(accountId: string): Promise<string | null> {
  return await invoke<string | null>("backup_saved_recovery_key", { accountId });
}

/**
 * Send a plain-text message to a room (FR-9, AD-13). Delegates to the single Rust
 * dispatch gate; the message's local echo and every send-state transition arrive
 * back over the room's existing timeline subscription (no echo is synthesized
 * here). Resolves on successful enqueue; rejects with the {@link IpcError}
 * envelope (`code: "sendFailed"`, `retriable: true`) on an enqueue-time failure.
 */
export async function sendText(accountId: string, roomId: string, body: string): Promise<void> {
  await invoke<void>("send_text", { accountId, roomId, body });
}

/**
 * Read the Undo-Send window in whole seconds (Story 8.3, FR-46). Absent/unparsable =
 * the default of 10; a stored value is clamped to `0..=60` (0 disables holding).
 * Rejects with the {@link IpcError} envelope on a registry failure.
 */
export async function undoSendWindow(): Promise<number> {
  return await invoke<number>("undo_send_window");
}

/**
 * Set the Undo-Send window in whole seconds (Story 8.3, FR-46). Clamped to `0..=60`
 * before persisting (0 disables holding). Resolves once persisted.
 */
export async function setUndoSendWindow(seconds: number): Promise<void> {
  await invoke<void>("set_undo_send_window", { seconds });
}

/**
 * Read the OS-global summon hotkey binding (Story 9.4, FR-50). Returns the persisted
 * accelerator (absent = the default `⌃⌥Space`), whether it equals the default, whether
 * it is currently registered with the OS (`active`), and any soft conflict warning.
 * Rejects with the {@link IpcError} envelope on a registry failure.
 */
export async function hotkeyGet(): Promise<HotkeyVm> {
  return await invoke<HotkeyVm>("hotkey_get");
}

/**
 * Reassign the OS-global summon hotkey (Story 9.4, FR-50). Validates the accelerator,
 * unregisters the old binding, registers the new one with the OS, and persists it on
 * success — resolving with the new {@link HotkeyVm} (including any soft `conflict`
 * warning). A malformed accelerator or an OS refusal keeps the previous binding and
 * rejects with the {@link IpcError} envelope (nothing is persisted).
 */
export async function hotkeySet(accelerator: string): Promise<HotkeyVm> {
  return await invoke<HotkeyVm>("hotkey_set", { accelerator });
}

/**
 * Cancel a held send by its `id` (Story 8.3, FR-46): deletes the durable `outbox`
 * row, persists its body as the Chat's Draft, and resolves with the restored body so
 * the composer can restore it. Performs **zero** network dispatch. Cancel of an
 * already-dispatched/absent row is an idempotent no-op resolving with an empty string.
 */
export async function cancelHeldSend(
  accountId: string,
  roomId: string,
  id: string,
): Promise<string> {
  return await invoke<string>("cancel_held_send", { accountId, roomId, id });
}

/**
 * Subscribe to the held sends for one open Chat (Story 8.3, FR-46). Opens a `Channel`,
 * forwards each {@link OutboxVm} snapshot to `onBatch` in arrival order (an initial
 * snapshot before any change; each snapshot is the full, oldest-first set that
 * REPLACES the room's mirrored rows), and resolves with the subscription id.
 */
export async function subscribeOutbox(
  accountId: string,
  roomId: string,
  onBatch: (batch: OutboxVm) => void,
): Promise<number> {
  return await subscribe<OutboxVm>("subscribe_outbox", onBatch, { accountId, roomId });
}

/**
 * Unsubscribe exactly one outbox subscription, aborting its backend producer task
 * (Story 8.3). Idempotent — unsubscribing an unknown id is a no-op.
 */
export async function unsubscribeOutbox(accountId: string, id: number): Promise<void> {
  await invoke<void>("unsubscribe_outbox", { accountId, subscriptionId: id });
}

/**
 * Send a plain-text reply to a message (FR-10, AD-13, Story 3.4). `inReplyToKey`
 * is the *original* message's opaque render `key` (`unique_id`); the Rust core
 * resolves it to the event id and enqueues the reply through the single dispatch
 * gate. The reply's local echo (with its own quoted-original preview) and every
 * send-state transition arrive back over the room's existing timeline
 * subscription (no echo is synthesized here). Resolves on successful enqueue;
 * rejects with the {@link IpcError} envelope (`code: "sendFailed"`) on failure —
 * `retriable: false` when the reply target is gone.
 */
export async function sendReply(
  accountId: string,
  roomId: string,
  inReplyToKey: string,
  body: string,
): Promise<void> {
  await invoke<void>("send_reply", { accountId, roomId, inReplyToKey, body });
}

/**
 * Edit an own text message in place (FR-11, AD-13, Story 3.4). `itemKey` is the
 * message's opaque render `key` (`unique_id`); the Rust core resolves it, gates on
 * editability (own + text), and enqueues the edit through the single dispatch
 * gate. The `Set` diff that updates the content in place (and flips `isEdited`)
 * arrives back over the room's existing timeline subscription. Resolves on
 * successful enqueue; rejects with the {@link IpcError} envelope (`code:
 * "sendFailed"`) on failure — `retriable: false` when the target is gone or not
 * editable.
 */
export async function editMessage(
  accountId: string,
  roomId: string,
  itemKey: string,
  body: string,
): Promise<void> {
  await invoke<void>("edit_message", { accountId, roomId, itemKey, body });
}

/**
 * Toggle the account's emoji reaction on a message (FR-12, AD-13, Story 3.5).
 * `itemKey` is the message's opaque render `key` (`unique_id`); the Rust core
 * resolves it and calls the SDK's `toggle_reaction` through the single dispatch
 * gate — adding the reaction if absent, retracting it if the account already
 * reacted with `emoji`. The updated pill state arrives back over the room's
 * existing timeline subscription as a `Set` diff (nothing is stored or synthesized
 * on the frontend). Resolves on successful dispatch; rejects with the
 * {@link IpcError} envelope (`code: "sendFailed"`) on failure — `retriable: false`
 * when the target is gone.
 */
export async function toggleReaction(
  accountId: string,
  roomId: string,
  itemKey: string,
  emoji: string,
): Promise<void> {
  await invoke<void>("toggle_reaction", { accountId, roomId, itemKey, emoji });
}

/**
 * Resolve a search hit's `eventId` to the open room's opaque timeline render key
 * so a search result can deep-link to the matched message (FR-34, Story 5.4).
 * `eventId` is the sanctioned deep-link handle from a {@link SearchHitVm}; the
 * Rust core parses it and scans the room's live timeline for the loaded item whose
 * event id matches, returning its opaque `key` (`unique_id`). It is an *input*
 * only — no event id is ever added to a streamed timeline VM. Resolves with the
 * render `key` when the event is a currently-loaded timeline item, or `null` when
 * it is not in the loaded window (the caller best-effort paginates and retries, or
 * degrades honestly). Rejects with the {@link IpcError} envelope (`code:
 * "timelineUnavailable"`) on an unparsable room/event id.
 */
export async function resolveTimelineEventKey(
  accountId: string,
  roomId: string,
  eventId: string,
): Promise<string | null> {
  return await invoke<string | null>("resolve_timeline_event_key", {
    accountId,
    roomId,
    eventId,
  });
}

/**
 * Retry a failed outgoing message by re-driving its wedged local echo through the
 * controlled send path (`unwedge`, not a new dispatch). `itemKey` is the timeline
 * item's opaque `key` (`unique_id`). Rejects with the {@link IpcError} envelope
 * (`code: "sendFailed"`) if the echo is gone or the room has no open timeline.
 */
export async function retrySend(accountId: string, roomId: string, itemKey: string): Promise<void> {
  await invoke<void>("send_retry", { accountId, roomId, itemKey });
}

/**
 * Delete an own message for everyone by issuing a Matrix redaction (FR-15, AD-13,
 * Story 3.8). `itemKey` is the message's opaque render `key` (`unique_id`); the
 * Rust core resolves it and calls the SDK's `redact` through the single dispatch
 * gate (no reason). The `Set` diff that turns the message into a "Message deleted"
 * stub in place arrives back over the room's existing timeline subscription
 * (nothing is synthesized on the frontend). Resolves on successful dispatch;
 * rejects with the {@link IpcError} envelope (`code: "sendFailed"`) on failure —
 * `retriable: false` when the target is gone, `retriable: true` on an SDK dispatch
 * error the dialog can retry.
 */
export async function deleteMessage(
  accountId: string,
  roomId: string,
  itemKey: string,
): Promise<void> {
  await invoke<void>("delete_message", { accountId, roomId, itemKey });
}

/**
 * Resolve the bridged Network label for the delete confirmation on demand (FR-15,
 * UX-DR17, Story 3.8). The Rust core reads the Room's MSC2346 `m.bridge` (and
 * legacy `uk.half-shot.bridge`) state event and returns the Network's display name
 * ("Telegram", "WhatsApp", …), or `null` for a native Matrix Room (no bridge
 * state). The Rust `Option<String>` serializes to `string | null` — only the
 * resolved, non-secret label crosses. Rejects with the {@link IpcError} envelope
 * (`code: "timelineUnavailable"`) on an unknown room/account.
 */
export async function roomNetworkLabel(accountId: string, roomId: string): Promise<string | null> {
  return await invoke<string | null>("room_network_label", { accountId, roomId });
}

/**
 * Send a media attachment from an OS file path (FR-13, AD-4, Story 3.7). The
 * composer attach button and native drag-drop both deliver a **path** — the Rust
 * core reads the file itself, so no media bytes cross IPC. `caption` is the trimmed
 * composer text (omit when empty). The local echo + every send-state transition
 * arrive back over the room's existing timeline subscription (no echo is
 * synthesized here). Resolves on successful enqueue; rejects with the
 * {@link IpcError} envelope (`code: "sendFailed"`) on an enqueue-time failure.
 */
export async function sendAttachmentPath(
  accountId: string,
  roomId: string,
  path: string,
  caption?: string,
): Promise<void> {
  await invoke<void>("send_attachment_path", {
    accountId,
    roomId,
    path,
    caption: caption ?? null,
  });
}

/**
 * Send a path-less pasted clipboard image (FR-13, AD-4, Story 3.7). The image
 * **bytes** ride as a **raw binary IPC body** (never base64/JSON — the sanctioned
 * exception for pastes with no OS path), with `accountId`/`roomId`/`filename`/
 * `mime`/`caption` in **request headers** (filename + caption percent-encoded so
 * non-ASCII survives an ASCII-only header). The Rust core reads the raw body,
 * decodes the headers, and enqueues the attachment through the single dispatch
 * gate; the local echo + send-state transitions arrive over the room's existing
 * timeline subscription. Resolves on successful enqueue; rejects with the
 * {@link IpcError} envelope (`code: "sendFailed"`) on failure.
 */
export async function sendAttachmentBytes(
  accountId: string,
  roomId: string,
  bytes: ArrayBuffer,
  filename: string,
  mime: string,
  caption?: string,
): Promise<void> {
  const headers: Record<string, string> = {
    "x-account-id": accountId,
    "x-room-id": roomId,
    // Percent-encode text that may contain non-ASCII (filename/caption); the Rust
    // side percent-decodes. ASCII-safe values (ids/mime) ride verbatim.
    "x-filename": encodeURIComponent(filename),
    "x-mime": mime,
  };
  if (caption != null && caption !== "") {
    headers["x-caption"] = encodeURIComponent(caption);
  }
  try {
    // Raw-body invoke: the `ArrayBuffer` becomes the `InvokeBody::Raw` payload;
    // metadata rides in headers. `invoke` in `@tauri-apps/api/core` maps a
    // rejection to a value, so mirror the shared client's IpcError normalization.
    await tauriInvoke<void>("send_attachment_bytes", bytes, { headers });
  } catch (raw) {
    if (isIpcError(raw)) {
      throw raw;
    }
    throw {
      code: "internal",
      message: typeof raw === "string" ? raw : "unexpected IPC failure",
      accountId: null,
      retriable: false,
    } satisfies IpcError;
  }
}

/**
 * Cancel an in-flight outgoing echo by aborting its SDK send handle (best-effort,
 * Story 3.7). `itemKey` is the echo's opaque render `key` (`unique_id`). If the
 * send already dispatched, the abort is a no-op and the message stays sent (the
 * echo's removal or its no-op arrives over the room's existing timeline
 * subscription). Rejects with the {@link IpcError} envelope (`code: "sendFailed"`)
 * if the echo is gone or the room has no open timeline.
 */
export async function cancelSend(
  accountId: string,
  roomId: string,
  itemKey: string,
): Promise<void> {
  await invoke<void>("cancel_send", { accountId, roomId, itemKey });
}

/**
 * Mark a room read (Story 3.9 receipts, Story 4.1, AD-14). The Rust core dispatches
 * a public `m.read` receipt on the room's latest event through the receipt/typing
 * signals seam — other Matrix clients observe the advance — and clears any manual
 * `m.marked_unread` flag. Works for any inbox row whether or not its timeline is
 * open. Best-effort: a dispatch failure is swallowed in the core (never a UI error),
 * so this resolves even then. Callers may fire-and-forget and swallow rejections.
 * Rejects with the {@link IpcError} envelope (`code: "timelineUnavailable"`) only on
 * an unknown room/inactive account.
 */
export async function markRoomRead(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("mark_room_read", { accountId, roomId });
}

/**
 * Kick every live account's sync loop (Story 13.6: pull-to-refresh + the
 * "Sync now" palette action). The Rust core resumes each already-active
 * account's `SyncService` via its idempotent `start()` — a no-op while the
 * loop is running, the same resume operation as a foreground wake (the Epic
 * 14-1 lifecycle seam). It never builds a second sync loop and never activates
 * signed-out accounts. Best-effort: callers may fire-and-forget and swallow
 * rejections — pull-to-refresh clears its spinner with no toast on an
 * {@link IpcError}.
 */
export async function syncNow(): Promise<void> {
  await invoke<void>("sync_now");
}

/**
 * Report an app-lifecycle transition to the single Rust lifecycle entry (Epic
 * 14-1). `"background"` gracefully pauses every live account's `SyncService`
 * (the sliding-sync long-poll ends cleanly, account state retained);
 * `"foreground"` routes through the same `AccountManager::sync_now()` sync-kick
 * pull-to-refresh uses, so the two cannot diverge.
 *
 * On iOS this is driven from the webview `visibilitychange` event (the
 * zero-native stopgap, {@link useAppLifecycle}); a future Swift `UIApplication`
 * plugin will call the same command. Never invoked on desktop, so Story 10.3
 * background operation is untouched. Best-effort: callers fire-and-forget and
 * swallow rejections (no toast).
 */
export async function appLifecycleChanged(phase: LifecyclePhase): Promise<void> {
  await invoke<void>("app_lifecycle_changed", { phase });
}

/**
 * Release a PUBLIC read receipt on a room — the explicit "Mark read publicly" action
 * (Story 8.2, AD-14, FR-45). The Rust core dispatches exactly one public `m.read` on
 * the room's latest event through the signals seam regardless of the effective
 * Incognito policy (the user chose to acknowledge), so own + remote clients see it
 * read. Best-effort: a dispatch failure is swallowed in the core (never a UI error),
 * so this resolves even then. Callers may fire-and-forget and swallow rejections.
 * Rejects with the {@link IpcError} envelope (`code: "timelineUnavailable"`) only on
 * an unknown room/inactive account.
 */
export async function releaseReceipt(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("release_receipt", { accountId, roomId });
}

/**
 * Read the data-driven per-Network coupling caveats (Story 8.2, FR-44). The Rust core
 * projects the embedded `coupling-caveats.json` into {@link CouplingCaveatVm}s the
 * frontend joins to the open room's Network by `networkId` to surface the caveat
 * inline at the Incognito toggle — no caveat copy is authored in TypeScript. Rejects
 * with the {@link IpcError} envelope on an embedded-data parse failure.
 */
export async function couplingCaveats(): Promise<CouplingCaveatVm[]> {
  return await invoke<CouplingCaveatVm[]>("coupling_caveats");
}

/**
 * Read the resolved Incognito state for `(accountId, roomId)` (Story 8.1). The Rust
 * core reads the three registry scopes and applies the Chat > Account > Global
 * resolver inside the `signals` seam, returning an {@link IncognitoVm} the frontend
 * renders directly — precedence is never resolved on the frontend. Rejects with the
 * {@link IpcError} envelope on a registry failure.
 */
export async function incognitoGet(accountId: string, roomId: string): Promise<IncognitoVm> {
  return await invoke<IncognitoVm>("incognito_get", { accountId, roomId });
}

/**
 * Read the "message previews" toggle (Story 10.1). Absent = on (previews enabled by
 * default). Resolves with the current in-memory config value. Rejects with the
 * {@link IpcError} envelope only on an unexpected failure.
 */
export async function notifyGetPreviewEnabled(): Promise<boolean> {
  return await invoke<boolean>("notify_get_preview_enabled");
}

/**
 * Set the "message previews" toggle (Story 10.1). Persists into the `settings` k/v
 * table in `keeper.db` and updates the in-memory config so every live notify handler
 * sees the change immediately. Resolves once persisted.
 */
export async function notifySetPreviewEnabled(enabled: boolean): Promise<void> {
  await invoke<void>("notify_set_preview_enabled", { enabled });
}

/**
 * Read the global Do-Not-Disturb switch (Story 10.2). Absent = off (DND off by default,
 * so notifications post normally). Resolves with the current in-memory config value.
 */
export async function dndGetGlobal(): Promise<boolean> {
  return await invoke<boolean>("dnd_get_global");
}

/**
 * Set the global Do-Not-Disturb switch (Story 10.2). Persists into the `settings` k/v
 * table under `notify.dnd_global` and updates the in-memory config so every live notify
 * handler sees the change immediately. Resolves once persisted.
 */
export async function dndSetGlobal(enabled: boolean): Promise<void> {
  await invoke<void>("dnd_set_global", { enabled });
}

/**
 * Read the dock-badge mode (Story 10.3, FR-53). Absent = `"all"` (badge all unreads by
 * default). The badge count itself is computed in Rust from the full cross-account
 * unread/mention state; this only reads the mode. Resolves with the current mode.
 */
export async function dockBadgeModeGet(): Promise<DockBadgeMode> {
  return await invoke<DockBadgeMode>("dock_badge_mode_get");
}

/**
 * Set the dock-badge mode (Story 10.3, FR-53). Persists into the `settings` k/v table
 * under `notify.dock_badge_mode` and re-pokes the Rust inbox merger so the dock badge is
 * recomputed and reapplied immediately. Resolves once persisted.
 */
export async function dockBadgeModeSet(mode: DockBadgeMode): Promise<void> {
  await invoke<void>("dock_badge_mode_set", { mode });
}

/**
 * Report the currently-visible Chat to the shared notify engine (Story 14.3, AD-18). A
 * `{ accountId, roomId }` selection sets the active Chat (a message for exactly it is
 * suppressed — its content is already on screen); `null` clears it. Reported by the iOS
 * shell from `roomsStore.selected` on the reduced tier only, so desktop notification
 * behavior is unchanged. Best-effort: callers fire-and-forget and swallow rejections.
 */
export async function activeChatSet(
  selection: { accountId: string; roomId: string } | null,
): Promise<void> {
  await invoke<void>("active_chat_set", {
    accountId: selection?.accountId ?? null,
    roomId: selection?.roomId ?? null,
  });
}

/**
 * Read the OS notification-permission state (Story 14.3). Maps the notification plugin's
 * `permission_state()` to `"granted" | "denied" | "unknown"` in Rust; a prompt state, an
 * unset handle, or a read error resolves to `"unknown"` (the UI then hides the persistent
 * "off" surface). Never re-prompts. Resolves with the current state; degrades to
 * `"unknown"` rather than rejecting.
 */
export async function notificationPermissionState(): Promise<NotificationPermission> {
  return await invoke<NotificationPermission>("notification_permission_state");
}

/**
 * Open this app's page in the iOS system Settings (Story 14.3). Routes `app-settings:`
 * through the Rust opener (`Platform::open_url`) so it bypasses the opener JS default
 * scope (which only permits `mailto`/`tel`/`http(s)`). Used by the permission-denied
 * "Open Settings" affordance; never re-prompts. Best-effort — callers swallow rejection.
 */
export async function iosOpenAppSettings(): Promise<void> {
  await invoke<void>("ios_open_app_settings");
}

/**
 * Read whether the one-time iOS no-background-sync disclosure has been shown
 * (Story 14.2, FR-61). Absent = `false` (not yet shown). The latch is device-global
 * and persisted in the Rust `settings` k/v table — never `localStorage`.
 */
export async function iosSyncDisclosureShownGet(): Promise<boolean> {
  return await invoke<boolean>("ios_sync_disclosure_shown_get");
}

/**
 * Latch the one-time iOS no-background-sync disclosure as shown (Story 14.2, FR-61).
 * One-way — once persisted the card never re-appears, including across relaunch.
 * Resolves once persisted.
 */
export async function iosSyncDisclosureShownSet(): Promise<void> {
  await invoke<void>("ios_sync_disclosure_shown_set");
}

/**
 * Read whether launch-at-login is enabled (Story 10.3, FR-53, AD-25). The autostart
 * plugin's LaunchAgent state is authoritative; off by default on a fresh install.
 * Rejects with the {@link IpcError} envelope on a plugin failure.
 */
export async function launchAtLoginGet(): Promise<boolean> {
  return await invoke<boolean>("launch_at_login_get");
}

/**
 * Set launch-at-login (Story 10.3, FR-53, AD-25). Enables or disables the macOS
 * LaunchAgent through the autostart plugin (the single source of truth). Only ever
 * called from an explicit user toggle. Rejects with the {@link IpcError} envelope on a
 * plugin failure.
 */
export async function launchAtLoginSet(enabled: boolean): Promise<void> {
  await invoke<void>("launch_at_login_set", { enabled });
}

/**
 * Read the menu-bar (tray) presence toggle (Story 10.3, FR-53). Reads the persisted
 * `system.menu_bar_presence` setting; off by default. Rejects with the {@link IpcError}
 * envelope on a registry failure.
 */
export async function menuBarPresenceGet(): Promise<boolean> {
  return await invoke<boolean>("menu_bar_presence_get");
}

/**
 * Set the menu-bar (tray) presence toggle (Story 10.3, FR-53). Persists the choice and
 * creates or destroys the tray icon live. Only ever called from an explicit user toggle.
 * Rejects with the {@link IpcError} envelope on a registry failure.
 */
export async function menuBarPresenceSet(enabled: boolean): Promise<void> {
  await invoke<void>("menu_bar_presence_set", { enabled });
}

/**
 * Read whether a Network label is currently muted (Story 10.2). Reads the persisted
 * `muted_networks` table. Rejects with the {@link IpcError} envelope on failure.
 */
export async function networkMuteGet(networkId: string): Promise<boolean> {
  return await invoke<boolean>("network_mute_get", { networkId });
}

/**
 * Set (or clear) the muted state for a Network label (Story 10.2). Persists into the
 * `muted_networks` table and updates the in-memory config so every live notify handler
 * and the inbox glyph see the change immediately. Resolves once persisted.
 */
export async function networkMuteSet(networkId: string, muted: boolean): Promise<void> {
  await invoke<void>("network_mute_set", { networkId, muted });
}

/**
 * Read the per-Chat notification mode for `(accountId, roomId)` (Story 10.2). Resolves
 * the account's live client and reads the synced Matrix push-rule mode
 * (`"all" | "mention_only" | "mute"`). Rejects with the {@link IpcError} envelope
 * (`timelineUnavailable`) for an unknown room / inactive account.
 */
export async function chatNotifyModeGet(
  accountId: string,
  roomId: string,
): Promise<ChatNotifyMode> {
  return await invoke<ChatNotifyMode>("chat_notify_mode_get", { accountId, roomId });
}

/**
 * Set the per-Chat notification mode for `(accountId, roomId)` (Story 10.2). Writes a
 * synced Matrix push rule so the mode survives restart and syncs across devices; `"all"`
 * clears any per-Chat rule (the "unmute" target). Rejects with the {@link IpcError}
 * envelope for an unknown room / inactive account or a push-rule dispatch failure.
 */
export async function chatNotifyModeSet(
  accountId: string,
  roomId: string,
  mode: ChatNotifyMode,
): Promise<void> {
  await invoke<void>("chat_notify_mode_set", { accountId, roomId, mode });
}

/**
 * Read the global Incognito default (Story 8.1). Absent = off (Incognito off by
 * default). Rejects with the {@link IpcError} envelope on a registry failure.
 */
export async function incognitoGetGlobal(): Promise<boolean> {
  return await invoke<boolean>("incognito_get_global");
}

/**
 * Set the global Incognito default (Story 8.1). Persists into the `settings` k/v
 * table in `keeper.db`; off by default. Resolves once persisted.
 */
export async function incognitoSetGlobal(enabled: boolean): Promise<void> {
  await invoke<void>("incognito_set_global", { enabled });
}

/**
 * Read the per-Account Incognito override (Story 8.1). Tri-state: `true`/`false` = an
 * explicit override, `null` = inherit the global scope (the Rust `Option<bool>`
 * serializes to `boolean | null`). Rejects with the {@link IpcError} envelope on a
 * registry failure.
 */
export async function incognitoGetAccount(accountId: string): Promise<boolean | null> {
  return await invoke<boolean | null>("incognito_get_account", { accountId });
}

/**
 * Set (or clear) the per-Account Incognito override (Story 8.1). `value` is tri-state:
 * `true`/`false` sets an explicit override; `null` clears it back to inherit the global
 * scope. Resolves once persisted.
 */
export async function incognitoSetAccount(accountId: string, value: boolean | null): Promise<void> {
  await invoke<void>("incognito_set_account", { accountId, value });
}

/**
 * Set (or clear) the per-Chat Incognito override for `(accountId, roomId)` (Story
 * 8.1). `enabled` is tri-state: `true`/`false` upserts an explicit override; `null`
 * clears it back to inherit the account/global scope. Resolves once persisted.
 */
export async function incognitoSetChat(
  accountId: string,
  roomId: string,
  enabled: boolean | null,
): Promise<void> {
  await invoke<void>("incognito_set_chat", { accountId, roomId, enabled });
}

/**
 * Manually mark a room unread (Story 4.1). The Rust core sets the `m.marked_unread`
 * account-data flag (`Room::set_unread_flag(true)`) so the row renders unread and the
 * flag syncs to the user's other Matrix clients. Best-effort: a dispatch failure is
 * swallowed in the core (never a UI error), so this resolves even then. Callers may
 * fire-and-forget and swallow rejections. Rejects with the {@link IpcError} envelope
 * (`code: "timelineUnavailable"`) only on an unknown room/inactive account.
 */
export async function markRoomUnread(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("mark_room_unread", { accountId, roomId });
}

/**
 * Archive a room (Story 4.2). The Rust core sets the Matrix low-priority tag
 * (`m.lowpriority`) via `Room::set_is_low_priority(true, None)` so the row moves into
 * the Archive window (unless it is unread) and the tag persists and syncs to the
 * user's other Matrix clients. Best-effort: a dispatch failure is swallowed in the
 * core (never a UI error), so this resolves even then. Callers may fire-and-forget
 * and swallow rejections. Rejects with the {@link IpcError} envelope (`code:
 * "timelineUnavailable"`) only on an unknown room/inactive account.
 */
export async function archiveRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("archive_room", { accountId, roomId });
}

/**
 * Unarchive a room (Story 4.2). The Rust core clears the Matrix low-priority tag
 * (`m.lowpriority`) via `Room::set_is_low_priority(false, None)` so the row returns to
 * its chronological Inbox position. Best-effort: a dispatch failure is swallowed in
 * the core (never a UI error), so this resolves even then. Callers may
 * fire-and-forget and swallow rejections. Rejects with the {@link IpcError} envelope
 * (`code: "timelineUnavailable"`) only on an unknown room/inactive account.
 */
export async function unarchiveRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("unarchive_room", { accountId, roomId });
}

/**
 * Favourite a room (Story 4.4, FR-21). The Rust core sets the Matrix favourite tag
 * (`m.favourite`) via `Room::set_is_favourite(true, None)`. Because `m.favourite`
 * is a *notable* tag, the row moves into the Favorites window on the SDK's live
 * re-emit and the tag persists and syncs to the user's other Matrix clients (no
 * out-of-band merger poke). Best-effort: a dispatch failure is swallowed in the
 * core (never a UI error), so this resolves even then. Callers may fire-and-forget
 * and swallow rejections. Rejects with the {@link IpcError} envelope (`code:
 * "timelineUnavailable"`) only on an unknown room/inactive account.
 */
export async function favoriteRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("favourite_room", { accountId, roomId });
}

/**
 * Unfavourite a room (Story 4.4). The Rust core clears the Matrix favourite tag
 * (`m.favourite`) via `Room::set_is_favourite(false, None)` so the row returns to
 * its chronological Inbox position on the SDK's live re-emit. Best-effort: a
 * dispatch failure is swallowed in the core (never a UI error), so this resolves
 * even then. Callers may fire-and-forget and swallow rejections. Rejects with the
 * {@link IpcError} envelope (`code: "timelineUnavailable"`) only on an unknown
 * room/inactive account.
 */
export async function unfavoriteRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("unfavourite_room", { accountId, roomId });
}

/**
 * Read the Favorites section's persisted collapse state (Story 4.4). Pure UI
 * chrome, stored in the app-level `settings` table in `keeper.db` (survives
 * restart and re-login). Resolves `false` (expanded) when unset. Rejects with the
 * {@link IpcError} envelope only on a registry read failure.
 */
export async function getFavoritesCollapsed(): Promise<boolean> {
  return await invoke<boolean>("get_favorites_collapsed");
}

/**
 * Persist the Favorites section's collapse state (Story 4.4). Stores the boolean
 * in the app-level `settings` table so it survives restart and re-login.
 * Best-effort: callers may fire-and-forget and swallow rejections. Rejects with
 * the {@link IpcError} envelope only on a registry write failure.
 */
export async function setFavoritesCollapsed(collapsed: boolean): Promise<void> {
  await invoke<void>("set_favorites_collapsed", { collapsed });
}

/**
 * Pin a room (Story 4.3, FR-22). The Rust core appends the pin at the end of the
 * keeper-local ordered list, persists it to `keeper.db` (pins have no Matrix
 * representation), and re-emits the Pins/Inbox/Archive windows so the strip
 * updates within one frame. Best-effort: callers fire-and-forget and swallow
 * rejection — the stream is truth. Rejects with the {@link IpcError} envelope
 * (`code: "internal"`) only on a registry write failure.
 */
export async function pinRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("pin_room", { accountId, roomId });
}

/**
 * Unpin a room (Story 4.3). The Rust core removes the keeper-local pin ref and
 * re-emits the windows so the row returns to its chronological Inbox (or Archive)
 * position. Best-effort: callers fire-and-forget and swallow rejection. Rejects
 * with the {@link IpcError} envelope only on a registry write failure.
 */
export async function unpinRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("unpin_room", { accountId, roomId });
}

/**
 * Reorder the pins to the exact `order` given (Story 4.3). Each entry is a
 * `{ accountId, roomId }` ref; the Rust core rewrites the keeper-local order to
 * contiguous `0..n` and re-emits the Pins window in that order (authoritative —
 * no optimistic TS overlay). Best-effort: callers fire-and-forget and swallow
 * rejection. Rejects with the {@link IpcError} envelope only on a registry write
 * failure.
 */
export async function reorderPins(order: { accountId: string; roomId: string }[]): Promise<void> {
  await invoke<void>("reorder_pins", { order });
}

/**
 * Set (or clear) the account's typing notice in the open room (Story 3.9, typing,
 * AD-14). The Rust core emits a normal (non-private) typing notification through
 * the receipt/typing signals seam. Best-effort: a dispatch failure is swallowed in
 * the core (typing is never a UI error). Callers fire-and-forget and swallow
 * rejections.
 */
export async function setTyping(accountId: string, roomId: string, typing: boolean): Promise<void> {
  await invoke<void>("set_typing", { accountId, roomId, typing });
}

/**
 * Back-paginate the open room's timeline (Story 3.9, pagination). The Rust core
 * fetches up to `numEvents` older events; they arrive back over the room's existing
 * timeline subscription (no second channel — the store applies the prepend ops).
 * Resolves with whether the homeserver start of the room was reached (no more older
 * history). Rejects with the {@link IpcError} envelope (`code:
 * "timelineUnavailable"`, `retriable: true`) on a pagination failure so the
 * boundary can show a retriable inline error, not an infinite spinner.
 */
export async function paginateBackwards(
  accountId: string,
  roomId: string,
  numEvents: number,
): Promise<boolean> {
  return await invoke<boolean>("paginate_backwards", { accountId, roomId, numEvents });
}

/**
 * Subscribe to the open room's typing notifications (Story 3.9, typing, AD-8,
 * AD-14). Opens a `Channel`, forwards each {@link TypingBatch} (the current set of
 * *other* members typing, each with a resolved display name) to `onBatch` in
 * arrival order (an initial empty snapshot before any change), and resolves with
 * the subscription id. Only opaque user ids + display names cross IPC. Rejects with
 * the {@link IpcError} envelope (`code: "timelineUnavailable"`) if the room isn't
 * open.
 */
export async function subscribeTyping(
  accountId: string,
  roomId: string,
  onBatch: (batch: TypingBatch) => void,
): Promise<number> {
  return await subscribe<TypingBatch>("typing_subscribe", onBatch, { accountId, roomId });
}

/**
 * Unsubscribe exactly one typing subscription, aborting its backend producer task
 * and dropping the SDK typing event handler (AD-19). Idempotent — an unknown id is
 * a no-op.
 */
export async function unsubscribeTyping(accountId: string, id: number): Promise<void> {
  await invoke<void>("typing_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Subscribe to the open room's live back-pagination status (Story 3.9, pagination,
 * AD-8). Opens a `Channel`, forwards each {@link PaginationStatusBatch} (a scalar
 * snapshot: `paginating`/`idle` + `hitStart`) to `onBatch` in arrival order (an
 * initial snapshot before any change), and resolves with the subscription id. The
 * status drives the honest history-boundary row; older events themselves arrive
 * over the timeline subscription, never here. Rejects with the {@link IpcError}
 * envelope (`code: "timelineUnavailable"`) if the room isn't open.
 */
export async function subscribePaginationStatus(
  accountId: string,
  roomId: string,
  onBatch: (batch: PaginationStatusBatch) => void,
): Promise<number> {
  return await subscribe<PaginationStatusBatch>("pagination_status_subscribe", onBatch, {
    accountId,
    roomId,
  });
}

/**
 * Unsubscribe exactly one pagination-status subscription, aborting its backend
 * producer task (AD-19). Idempotent — an unknown id is a no-op.
 */
export async function unsubscribePaginationStatus(accountId: string, id: number): Promise<void> {
  await invoke<void>("pagination_status_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * The Tauri event the Rust shell emits on app activation following a notification
 * (Story 10.4, Option B). Must match `NOTIFY_NAVIGATE_EVENT` in `keeper/src/ipc.rs`.
 */
export const NOTIFY_NAVIGATE_EVENT = "notify://navigate";

/**
 * Subscribe to the coarse notification-navigate event (Story 10.4, Option B). The kept
 * `tauri-plugin-notification` desktop backend has NO per-notification click callback, so
 * on app activation following a notification the Rust shell summons+focuses the window and
 * emits this event carrying the {@link NotifyTarget} recorded at dispatch. The frontend
 * routes its KIND to a **coarse** view (Message → Inbox, Bridge → Bridges) — this is NEVER
 * exact-message routing (deferred to Epic 11).
 *
 * Resolves with an unlisten function; registering is best-effort and graceful outside a
 * Tauri webview (jsdom in tests / a future non-desktop port) — a failure just leaves the
 * bridge inert and never crashes the shell.
 */
export async function listenNotifyNavigate(
  onNavigate: (target: NotifyTarget) => void,
): Promise<() => void> {
  return await listen<NotifyTarget>(NOTIFY_NAVIGATE_EVENT, (event) => {
    onNavigate(event.payload);
  });
}
