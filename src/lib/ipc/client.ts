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
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ChatNotifyMode } from "./gen/ChatNotifyMode";
import type { DockBadgeMode } from "./gen/DockBadgeMode";
import type { EgressEndpointVm } from "./gen/EgressEndpointVm";
import type { IpcError } from "./gen/IpcError";
import type { LifecyclePhase } from "./gen/LifecyclePhase";
import type { NavState } from "./gen/NavState";
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
export type { CopyEntryVm } from "./gen/CopyEntryVm";
export type { CopyJobState } from "./gen/CopyJobState";
export type { CopyJobVm } from "./gen/CopyJobVm";
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
export type { NavState } from "./gen/NavState";
export type { NetworksSnapshot } from "./gen/NetworksSnapshot";
export type { NetworkVm } from "./gen/NetworkVm";
export type { NewChatResolutionVm } from "./gen/NewChatResolutionVm";
export type { NoteAttachmentVm } from "./gen/NoteAttachmentVm";
export type { NoteBodyBatch } from "./gen/NoteBodyBatch";
export type { NoteCadenceVm } from "./gen/NoteCadenceVm";
export type { NoteChangeBatch } from "./gen/NoteChangeBatch";
export type { NoteConflictChoiceReq } from "./gen/NoteConflictChoiceReq";
export type { NoteConflictVm } from "./gen/NoteConflictVm";
export type { NoteCreateReq } from "./gen/NoteCreateReq";
export type { NoteDiffVm } from "./gen/NoteDiffVm";
export type { NoteFlag } from "./gen/NoteFlag";
export type { NoteFolderVm } from "./gen/NoteFolderVm";
export type { NoteHunkVm } from "./gen/NoteHunkVm";
export type { NoteIndexProgressVm } from "./gen/NoteIndexProgressVm";
export type { NoteLinkTargetVm } from "./gen/NoteLinkTargetVm";
export type { NoteListOp } from "./gen/NoteListOp";
export type { NoteListVm } from "./gen/NoteListVm";
export type { NoteQueryCheckVm } from "./gen/NoteQueryCheckVm";
export type { NoteQueryReq } from "./gen/NoteQueryReq";
export type { NoteRefVm } from "./gen/NoteRefVm";
export type { NoteRevisionVm } from "./gen/NoteRevisionVm";
export type { NoteRowVm } from "./gen/NoteRowVm";
export type { NoteSearchBatch } from "./gen/NoteSearchBatch";
export type { NoteSearchHitVm } from "./gen/NoteSearchHitVm";
export type { NoteSearchReq } from "./gen/NoteSearchReq";
export type { NoteSpaceReq } from "./gen/NoteSpaceReq";
export type { NoteSpaceVm } from "./gen/NoteSpaceVm";
export type { NoteTagNodeVm } from "./gen/NoteTagNodeVm";
export type { NoteTagTreeVm } from "./gen/NoteTagTreeVm";
export type { NoteTemplateVm } from "./gen/NoteTemplateVm";
export type { NoteVaultSettingsReq } from "./gen/NoteVaultSettingsReq";
export type { NoteVaultVm } from "./gen/NoteVaultVm";
export type { NoteWriteVm } from "./gen/NoteWriteVm";
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
export type { RecordingApplicationVm } from "./gen/RecordingApplicationVm";
export type { RecordingDestinationKind } from "./gen/RecordingDestinationKind";
export type { RecordingDisplayVm } from "./gen/RecordingDisplayVm";
export type { RecordingDurabilityState } from "./gen/RecordingDurabilityState";
export type { RecordingDurabilityVm } from "./gen/RecordingDurabilityVm";
export type { RecordingFilterVm } from "./gen/RecordingFilterVm";
export type { RecordingHitVm } from "./gen/RecordingHitVm";
export type { RecordingNoteStubVm } from "./gen/RecordingNoteStubVm";
export type { RecordingNoteTargetKind } from "./gen/RecordingNoteTargetKind";
export type { RecordingNoteTargetVm } from "./gen/RecordingNoteTargetVm";
export type { RecordingPathPreviewVm } from "./gen/RecordingPathPreviewVm";
export type { RecordingPermissionVm } from "./gen/RecordingPermissionVm";
export type { RecordingProfileVm } from "./gen/RecordingProfileVm";
export type { RecordingSettingsVm } from "./gen/RecordingSettingsVm";
export type { RecordingSourcesVm } from "./gen/RecordingSourcesVm";
export type { RecordingStatusVm } from "./gen/RecordingStatusVm";
export type { RecordingTargetVm } from "./gen/RecordingTargetVm";
export type { RecordingUiState } from "./gen/RecordingUiState";
export type { RecordingVolumeState } from "./gen/RecordingVolumeState";
export type { RecordingVolumeVm } from "./gen/RecordingVolumeVm";
export type { RemoteDraftVm } from "./gen/RemoteDraftVm";
export type { ReplyPreviewVm } from "./gen/ReplyPreviewVm";
export type { ResolveSupportVm } from "./gen/ResolveSupportVm";
export type { RiskTier } from "./gen/RiskTier";
export type { RoomListBatch } from "./gen/RoomListBatch";
export type { RoomListOp } from "./gen/RoomListOp";
export type { RoomVm } from "./gen/RoomVm";
export type { SasEmojiVm } from "./gen/SasEmojiVm";
export type { ScreenRecordingAccess } from "./gen/ScreenRecordingAccess";
export type { SearchFilterVm } from "./gen/SearchFilterVm";
export type { SearchHitVm } from "./gen/SearchHitVm";
export type { SendState } from "./gen/SendState";
export type { SpacesSnapshot } from "./gen/SpacesSnapshot";
export type { SpaceVm } from "./gen/SpaceVm";
export type { SyncActivityVm } from "./gen/SyncActivityVm";
export type { SyncDeviceVm } from "./gen/SyncDeviceVm";
export type { SyncGitState } from "./gen/SyncGitState";
export type { SyncGitVm } from "./gen/SyncGitVm";
export type { SyncListSettingsVm } from "./gen/SyncListSettingsVm";
export type { SyncOutcomeVm } from "./gen/SyncOutcomeVm";
export type { SyncParkedVm } from "./gen/SyncParkedVm";
export type { SyncPendingVm } from "./gen/SyncPendingVm";
export type { SyncProblemsVm } from "./gen/SyncProblemsVm";
export type { SyncProfileReq } from "./gen/SyncProfileReq";
export type { SyncProfileVm } from "./gen/SyncProfileVm";
export type { SyncProgressVm } from "./gen/SyncProgressVm";
export type { SyncStatusVm } from "./gen/SyncStatusVm";
export type { TagVocabularyEntryVm } from "./gen/TagVocabularyEntryVm";
export type { TagVocabularyVm } from "./gen/TagVocabularyVm";
export type { TccPermission } from "./gen/TccPermission";
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
import type { CopyJobVm } from "./gen/CopyJobVm";
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
import type { NoteAttachmentVm } from "./gen/NoteAttachmentVm";
import type { NoteBodyBatch } from "./gen/NoteBodyBatch";
import type { NoteChangeBatch } from "./gen/NoteChangeBatch";
import type { NoteConflictChoiceReq } from "./gen/NoteConflictChoiceReq";
import type { NoteConflictVm } from "./gen/NoteConflictVm";
import type { NoteCreateReq } from "./gen/NoteCreateReq";
import type { NoteDiffVm } from "./gen/NoteDiffVm";
import type { NoteFlag } from "./gen/NoteFlag";
import type { NoteFolderVm } from "./gen/NoteFolderVm";
import type { NoteIndexProgressVm } from "./gen/NoteIndexProgressVm";
import type { NoteLinkTargetVm } from "./gen/NoteLinkTargetVm";
import type { NoteListVm } from "./gen/NoteListVm";
import type { NoteQueryCheckVm } from "./gen/NoteQueryCheckVm";
import type { NoteQueryReq } from "./gen/NoteQueryReq";
import type { NoteRefVm } from "./gen/NoteRefVm";
import type { NoteRevisionVm } from "./gen/NoteRevisionVm";
import type { NoteRowVm } from "./gen/NoteRowVm";
import type { NoteSearchBatch } from "./gen/NoteSearchBatch";
import type { NoteSearchReq } from "./gen/NoteSearchReq";
import type { NoteSpaceReq } from "./gen/NoteSpaceReq";
import type { NoteSpaceVm } from "./gen/NoteSpaceVm";
import type { NoteTagTreeVm } from "./gen/NoteTagTreeVm";
import type { NoteTemplateVm } from "./gen/NoteTemplateVm";
import type { NoteVaultSettingsReq } from "./gen/NoteVaultSettingsReq";
import type { NoteVaultVm } from "./gen/NoteVaultVm";
import type { NoteWriteVm } from "./gen/NoteWriteVm";
import type { OutboxVm } from "./gen/OutboxVm";
import type { PaginationStatusBatch } from "./gen/PaginationStatusBatch";
import type { PaletteMode } from "./gen/PaletteMode";
import type { PaletteResultsVm } from "./gen/PaletteResultsVm";
import type { RecordingFilterVm } from "./gen/RecordingFilterVm";
import type { RecordingHitVm } from "./gen/RecordingHitVm";
import type { RecordingNoteStubVm } from "./gen/RecordingNoteStubVm";
import type { RecordingNoteTargetVm } from "./gen/RecordingNoteTargetVm";
import type { RecordingPathPreviewVm } from "./gen/RecordingPathPreviewVm";
import type { RecordingPermissionVm } from "./gen/RecordingPermissionVm";
import type { RecordingProfileVm } from "./gen/RecordingProfileVm";
import type { RecordingSettingsVm } from "./gen/RecordingSettingsVm";
import type { RecordingSourcesVm } from "./gen/RecordingSourcesVm";
import type { RecordingStatusVm } from "./gen/RecordingStatusVm";
import type { RecordingTargetVm } from "./gen/RecordingTargetVm";
import type { RemoteDraftVm } from "./gen/RemoteDraftVm";
import type { ResolveSupportVm } from "./gen/ResolveSupportVm";
import type { RoomListBatch } from "./gen/RoomListBatch";
import type { SearchFilterVm } from "./gen/SearchFilterVm";
import type { SearchHitVm } from "./gen/SearchHitVm";
import type { SpacesSnapshot } from "./gen/SpacesSnapshot";
import type { SyncActivityVm } from "./gen/SyncActivityVm";
import type { SyncDeviceVm } from "./gen/SyncDeviceVm";
import type { SyncGitVm } from "./gen/SyncGitVm";
import type { SyncListSettingsVm } from "./gen/SyncListSettingsVm";
import type { SyncOutcomeVm } from "./gen/SyncOutcomeVm";
import type { SyncPendingVm } from "./gen/SyncPendingVm";
import type { SyncProblemsVm } from "./gen/SyncProblemsVm";
import type { SyncProfileReq } from "./gen/SyncProfileReq";
import type { SyncProfileVm } from "./gen/SyncProfileVm";
import type { SyncProgressVm } from "./gen/SyncProgressVm";
import type { SyncStatusVm } from "./gen/SyncStatusVm";
import type { TagVocabularyVm } from "./gen/TagVocabularyVm";
import type { TccPermission } from "./gen/TccPermission";
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
 * Cancel the in-progress Beeper login flow for `email` (Story 2.3). The Rust core
 * drops that flow's pending request id so nothing lingers; other in-flight Beeper
 * logins keep running and nothing is persisted. Idempotent — a no-op when no flow
 * is pending for `email`.
 */
export async function cancelBeeper(email: string): Promise<void> {
  await invoke<void>("cancel_beeper", { email });
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
 * Search the recordings archive for the Recordings browser (FR-141, UX-DR50,
 * Story 42.3). Runs fully offline against `archive.db` over a fresh read-only
 * connection — never the recorder's writer, never a network call. Queries of 3+
 * characters use the trigram index over each session's title, participants,
 * note, tags and custom-field values; shorter ones fall back to an accelerated
 * `LIKE` scan. Every {@link RecordingFilterVm} field is optional and they all
 * narrow: an empty `query` is no text predicate, an empty `tags` list is
 * unrestricted, and several tags AND together (each matched hierarchically, so
 * `client/acme` matches `client/acme/renewal` and never `client/acmecorp`).
 *
 * Resolves with at most `limit` (default and maximum 200) {@link RecordingHitVm}
 * rows, newest first, each carrying its absolute folder (composed in Rust from
 * the effective recordings destination — never join one here), its duration, its
 * summed size and its decoded tags. An empty array means "nothing matched";
 * a machine that has never recorded has no `archive.db` and also resolves with
 * `[]`, so the caller distinguishes the two facts by the filter it sent, not by
 * an error. Rejects with the {@link IpcError} envelope only on a genuine archive
 * failure.
 */
export async function searchRecordings(filter: RecordingFilterVm): Promise<RecordingHitVm[]> {
  return await invoke<RecordingHitVm[]>("search_recordings", { filter });
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
 * Hand a recording's file to the system's default handler — the Recordings
 * browser's Play (FR-141, UX-DR50, Story 42.3). `path` MUST be absolute and is
 * normally a row's `playablePath` (its `absolutePath` opens the session folder
 * instead).
 *
 * The Rust core refuses anything that is not inside the recordings destination
 * root, lexically and after resolving symlinks, before the opener ever sees it —
 * a command that opened any path the webview named would be a file-disclosure
 * primitive. Rejects with the {@link IpcError} envelope (`code: "internal"`,
 * `retriable: false`) for a path outside the root, one that no longer resolves
 * on disk (a session moved or deleted outside keeper), or an opener failure; on
 * a build without recording support, `code: "unsupported"`.
 */
export async function recordingOpenPath(path: string): Promise<void> {
  await invoke<void>("recording_open_path", { path });
}

/**
 * Everything the reader of a recording note can act on, for the session the
 * note names by id (FR-142, FR-145, AD-65, Story 42.4). The session folder
 * first, then every file in it, each as a {@link RecordingNoteTargetVm}
 * carrying the relative path the note itself is written in, the absolute path
 * composed in Rust from the effective recordings destination, and whether it
 * is a video.
 *
 * `sessionId` is the note's `session:` value, not one of its paths: a note
 * records where the recording was when the stub was written, and a Story 40.4
 * retitle moves the folder afterwards. The id is the handle that survives, so
 * this is where the recording is NOW.
 *
 * Resolves `null` — never rejects — when no archive row knows the session,
 * when its folder is not on this machine, and on a machine with no archive at
 * all (a fresh install syncing an old vault). The surface renders the note's
 * relative text either way and attaches an action only to a target it was
 * handed. Rejects with the {@link IpcError} envelope only on an archive read
 * failure.
 */
export async function recordingNoteTargets(
  sessionId: string,
): Promise<RecordingNoteTargetVm[] | null> {
  return await invoke<RecordingNoteTargetVm[] | null>("recording_note_targets", { sessionId });
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
 * Read the optional OS-global Start/Stop Recording hotkey binding (Story 20.4,
 * FR-50) — a second, independent binding stored under `hotkey.recording`.
 * Absent = the empty accelerator, meaning **unset by default** (`isDefault:
 * true`, nothing registered). `conflict` carries the curated system-shortcut
 * warning or a clash with the summon binding. Rejects with the
 * {@link IpcError} envelope on a registry failure.
 */
export async function recordingHotkeyGet(): Promise<HotkeyVm> {
  return await invoke<HotkeyVm>("recording_hotkey_get");
}

/**
 * Assign the OS-global Start/Stop Recording hotkey (Story 20.4, FR-50) with the
 * summon hotkey's validate → register → persist → rollback discipline. An empty
 * accelerator is rejected (clearing is {@link recordingHotkeyClear}); a
 * malformed accelerator or an OS refusal keeps the previous binding and rejects
 * with the {@link IpcError} envelope (nothing is persisted).
 */
export async function recordingHotkeySet(accelerator: string): Promise<HotkeyVm> {
  return await invoke<HotkeyVm>("recording_hotkey_set", { accelerator });
}

/**
 * Clear the OS-global Start/Stop Recording hotkey back to unset (Story 20.4):
 * unregisters the current binding and persists the empty accelerator. Resolves
 * with the unset {@link HotkeyVm} (`accelerator: ""`, `active: false`).
 */
export async function recordingHotkeyClear(): Promise<HotkeyVm> {
  return await invoke<HotkeyVm>("recording_hotkey_clear");
}

/**
 * Reveal the effective recordings destination folder in the OS file manager
 * (Story 20.4, FR-48) — the palette "Open Recordings Folder" verb. Rust
 * resolves the same effective destination `recording_start` uses and reveals
 * it, or its nearest existing ancestor when the folder has not been created
 * yet. Rejects with the {@link IpcError} envelope on a reveal failure.
 */
export async function recordingRevealFolder(): Promise<void> {
  await invoke<void>("recording_reveal_folder");
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
 * Record the last phone-stack navigation level in Rust (Story 14.4). Reported by the
 * iOS shell on the reduced tier whenever a Chat is open (`detailOpen` marks the level-2
 * Detail), so a webview reload after a content-process jettison (tauri#14371) can land
 * the user exactly where they were. Nav *selection* only — never message/room data.
 * Best-effort: callers fire-and-forget and swallow rejections.
 */
export async function navStateSet(
  selection: { accountId: string; roomId: string },
  detailOpen: boolean,
): Promise<void> {
  await invoke<void>("nav_state_set", {
    accountId: selection.accountId,
    roomId: selection.roomId,
    detailOpen,
  });
}

/**
 * Clear the Rust-held navigation level (Story 14.4) — the user returned to the Inbox,
 * so a later reload honestly starts at level 0. Best-effort: callers fire-and-forget
 * and swallow rejections.
 */
export async function navStateClear(): Promise<void> {
  await invoke<void>("nav_state_clear");
}

/**
 * Read the Rust-held navigation level (Story 14.4), or `null` on a cold launch (a true
 * app kill restarts Rust fresh — no stored nav means the Inbox). A read, not a take:
 * the shell keeps reporting over it. A rejection is treated as "no stored nav" by
 * callers (start at the Inbox).
 */
export async function navStateGet(): Promise<NavState | null> {
  return await invoke<NavState | null>("nav_state_get");
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
 * Resolve the live recording permission pre-flight (Story 16.5, FR-67, AD-36;
 * mic/camera legs Story 20.2). The Rust command probes the `keeper-rec`
 * sidecar's non-prompting `getCapabilities` (a fresh child process per call —
 * live detection, never a cached grant, bounded by a shell timeout so a wedged
 * sidecar resolves a clean error) and resolves all three legs from that one
 * probe into the honest {@link RecordingPermissionVm}: the Screen Recording
 * tri-state plus a Microphone/Camera leg for each source the caller reports
 * enabled (`micEnabled`/`cameraEnabled` — a disabled source's leg is `null`,
 * renders no row, and never gates Start). Called at Recording-view render and
 * re-called on focus/return and on every enabled-source change. Rejects with
 * the {@link IpcError} envelope on a sidecar failure — callers swallow to a
 * safe default (Start disabled) rather than surfacing a spinner.
 */
export async function recordingPermission(
  micEnabled: boolean,
  cameraEnabled: boolean,
): Promise<RecordingPermissionVm> {
  return await invoke<RecordingPermissionVm>("recording_permission", { micEnabled, cameraEnabled });
}

/**
 * Request Screen Recording access (Story 16.5, FR-67, AD-36). The Rust command
 * marks the session "already requested" flag, runs the sidecar
 * `requestScreenRecording` round-trip (`CGRequestScreenCaptureAccess` — the OS
 * posts its one real prompt per app lifetime where allowed; a prior denial shows
 * no prompt at all), and resolves the re-resolved {@link RecordingPermissionVm}:
 * granted unlocks Start; not granted resolves denied-with-fix-path so the row
 * offers the System Settings deep link. Story 20.2: `micEnabled`/`cameraEnabled`
 * carry the enabled sources so the returned VM keeps their legs resolved (from
 * a non-prompting `getCapabilities` probe) — never blanking an enabled row.
 * Rejects with the {@link IpcError} envelope on a sidecar failure — callers
 * swallow to a safe default.
 */
export async function requestScreenRecordingPermission(
  micEnabled: boolean,
  cameraEnabled: boolean,
): Promise<RecordingPermissionVm> {
  return await invoke<RecordingPermissionVm>("request_screen_recording_permission", {
    micEnabled,
    cameraEnabled,
  });
}

/**
 * Open the macOS System Settings pane for Screen Recording (Story 16.5, FR-67) —
 * the fix path for a denied grant, where re-prompting is impossible. Routes the
 * `x-apple.systempreferences:…Privacy_ScreenCapture` deep link through the Rust
 * opener (`Platform::open_url`) so it bypasses the opener JS default scope.
 * Never re-prompts. Best-effort — callers swallow rejection.
 */
export async function openScreenRecordingSettings(): Promise<void> {
  await invoke<void>("open_screen_recording_settings");
}

/**
 * Open the macOS System Settings pane for Microphone (Story 20.2, FR-67) — the
 * Microphone pre-flight row's fix path for a denied grant, where re-prompting
 * is impossible. Routes the `x-apple.systempreferences:…Privacy_Microphone`
 * deep link through the Rust opener (`Platform::open_url`) so it bypasses the
 * opener JS default scope. Never re-prompts. Best-effort — callers swallow
 * rejection.
 */
export async function openMicrophoneSettings(): Promise<void> {
  await invoke<void>("open_microphone_settings");
}

/**
 * Open the macOS System Settings pane for Camera (Story 20.2, FR-67) — the
 * Camera pre-flight row's fix path for a denied grant, where re-prompting is
 * impossible. Routes the `x-apple.systempreferences:…Privacy_Camera` deep link
 * through the Rust opener (`Platform::open_url`) so it bypasses the opener JS
 * default scope. Never re-prompts. Best-effort — callers swallow rejection.
 */
export async function openCameraSettings(): Promise<void> {
  await invoke<void>("open_camera_settings");
}

/**
 * Start the (at most one) full-screen + system-audio recording session (Story
 * 16.6, FR-68/FR-69/FR-71). The Rust command resolves the output file
 * (`~/Movies/keeper/keeper-rec <local timestamp>.mp4`), spawns the capture
 * sidecar session, and resolves the initial {@link RecordingStatusVm} snapshot.
 * Progress is polled via {@link recordingStatus}; a mid-session failure surfaces
 * on the snapshot (`state: "failed"` + message), never a silent reset. Rejects
 * with the {@link IpcError} envelope when a session is already live or the
 * sidecar cannot spawn.
 */
export async function recordingStart(
  target?: RecordingTargetVm,
  systemAudio?: boolean,
  micEnabled?: boolean,
  micDeviceId?: string | null,
  cameraEnabled?: boolean,
  cameraDeviceId?: string | null,
  meta?: {
    title?: string;
    participants?: string;
    note?: string;
    tags?: string;
    custom?: { name: string; value: string }[];
  },
): Promise<RecordingStatusVm> {
  // Story 19.1: the picker's selected source/target (a display or an
  // application). Omitted (`undefined`) preserves the 16.6 main-display default.
  // Story 19.2: the Audio card's ephemeral per-session toggle. Omitted
  // preserves the 16.6 default-on path (`system_audio.unwrap_or(true)` in Rust).
  // Story 19.3: the Audio card's ephemeral mic selection. Omitted preserves the
  // mic-off default (`microphone_enabled.unwrap_or(false)` in Rust);
  // `microphoneDeviceId` null = the system default input.
  // Story 20.1: the Webcam card's ephemeral camera selection. Omitted
  // preserves the camera-off default (`camera_enabled.unwrap_or(false)` in
  // Rust — no camera file, no Camera-TCC touch); `cameraDeviceId` null = the
  // system default camera.
  return await invoke<RecordingStatusVm>("recording_start", {
    target: target ?? null,
    systemAudio: systemAudio ?? null,
    microphoneEnabled: micEnabled ?? null,
    microphoneDeviceId: micDeviceId ?? null,
    cameraEnabled: cameraEnabled ?? null,
    cameraDeviceId: cameraDeviceId ?? null,
    // Story 21.5: optional session metadata — absent fields ship as null and
    // land only in the local session manifest (zero egress).
    metaTitle: meta?.title ?? null,
    metaParticipants: meta?.participants ?? null,
    metaNote: meta?.note ?? null,
    metaTags: meta?.tags ?? null,
    metaCustom: meta?.custom ?? null,
  });
}

/**
 * Request microphone access (Story 19.3, FR-69, AD-36). The Rust command runs
 * the sidecar `requestMicrophone` round-trip (`AVCaptureDevice.requestAccess`
 * in the child sidecar — the OS posts its one real prompt per app lifetime
 * where the state is undetermined, attributed to keeper via
 * `NSMicrophoneUsageDescription`) and resolves the authoritative post-request
 * {@link TccPermission} tri-state. Called **only** when the user enables the
 * mic source on the Audio card or hits the Microphone pre-flight row's
 * "Request permission" (Story 20.2) — never preemptively, never on render.
 * Since Story 20.2 an enabled mic that is not granted blocks Start. Rejects
 * with the {@link IpcError} envelope on a sidecar failure — the caller
 * swallows it to a no-claim caption rather than crashing.
 */
export async function requestMicrophonePermission(): Promise<TccPermission> {
  return await invoke<TccPermission>("request_microphone_permission");
}

/**
 * Request camera access (Story 20.1, FR-70, AD-36). The Rust command runs the
 * sidecar `requestCamera` round-trip (`AVCaptureDevice.requestAccess` for
 * `.video` in the child sidecar — the OS posts its one real prompt per app
 * lifetime where the state is undetermined, attributed to keeper via
 * `NSCameraUsageDescription`) and resolves the authoritative post-request
 * {@link TccPermission} tri-state. Called **only** when the user enables the
 * Webcam switch or hits the Camera pre-flight row's "Request permission"
 * (Story 20.2) — never preemptively, never on render. Since Story 20.2 an
 * enabled webcam that is not granted blocks Start. Rejects with the
 * {@link IpcError} envelope on a sidecar failure — the caller swallows it to
 * a no-claim caption rather than crashing.
 */
export async function requestCameraPermission(): Promise<TccPermission> {
  return await invoke<TccPermission>("request_camera_permission");
}

/**
 * Enumerate the recordable sources — displays and applications — the source
 * picker polls (Story 19.1). The Rust command runs the `keeper-rec`
 * `listSources` round-trip (a fresh child process per call, bounded by a shell
 * timeout so a wedged sidecar resolves a clean error, never a hung poll) and
 * resolves the live {@link RecordingSourcesVm}: real displays, real
 * applications (name/pid/bundleId + an optional ≤64px PNG icon data-URI, keeper
 * excluded), and real microphones (Story 19.3 — `{id, name}` rows for the Audio
 * card's device picker). Called on a ~3s poll while the idle setup surface is visible and on
 * window focus. Rejects with the {@link IpcError} envelope on a sidecar failure —
 * the picker swallows it to the prior list rather than blanking.
 */
export async function listRecordingSources(): Promise<RecordingSourcesVm> {
  return await invoke<RecordingSourcesVm>("recording_list_sources");
}

/**
 * Request a graceful stop of the live recording session (Story 16.6): the
 * sidecar finalizes the file (`stopping` -> `finalized` on the polled snapshot)
 * and exits. Idempotent -- a second stop is a no-op, never an error.
 */
export async function recordingStop(): Promise<void> {
  await invoke<void>("recording_stop");
}

/**
 * Read the current recording-session status snapshot (Story 16.6) -- what the
 * Recording view's active-session UI polls and renders from. No session yet
 * this app lifetime resolves the honest idle snapshot.
 */
export async function recordingStatus(): Promise<RecordingStatusVm> {
  return await invoke<RecordingStatusVm>("recording_status");
}

/**
 * Acknowledge (dismiss) a settled recording session's outcome (Story 18.4): a
 * terminal session (finalized / recovered / failed) is cleared back to idle --
 * dropping `error`/`warning`, which releases the held tray error rendering and
 * hides the banner error variant -- while a LIVE session is a strict no-op
 * (acknowledge never silently stops a recording). Resolves the fresh snapshot
 * either way (the idle default after a clear; the untouched live snapshot on
 * the no-op).
 */
export async function recordingAcknowledge(): Promise<RecordingStatusVm> {
  return await invoke<RecordingStatusVm>("recording_acknowledge");
}

/**
 * The read-only end-of-session summary the completion / recovery cards render
 * (Story 20.3, FR-71/FR-73) — the TypeScript twin of the Rust
 * `keeper_core::vm::RecordingSummaryVm`. Derived from a session's authoritative
 * on-disk `manifest.json`, never the live {@link RecordingStatusVm} snapshot:
 * `screenSegmentCount` backs "Saved N segments", `totalBytes` backs "{size}",
 * and `sessionFolder` backs the mono line + Reveal in Finder.
 */
export interface RecordingSummaryVm {
  /** The session folder path — the mono line and the Reveal-in-Finder target. */
  sessionFolder: string;
  /** The number of screen-track segments the session saved (never the
   * track-agnostic live `segmentsClosed` counter). */
  screenSegmentCount: number;
  /** The user session title when one was set (Story 21.5), else null. */
  title: string | null;
  /** The total on-disk bytes across every segment (screen + camera). */
  totalBytes: number;
}

/**
 * Summarize one session folder for the completion / in-app-recovered card
 * (Story 20.3, FR-71): the Rust core loads `folder/manifest.json` and returns
 * the manifest-authoritative `{screenSegmentCount, totalBytes, sessionFolder}` —
 * the honest "Saved N segments · {size}" figures, never the live rotation
 * counter. `folder` is the session **folder** (`status.outputPath`). Rejects
 * with the {@link IpcError} envelope on a manifest load failure — the card then
 * falls back to folder + Reveal, omitting count/size.
 */
export async function recordingSessionSummary(folder: string): Promise<RecordingSummaryVm> {
  return await invoke<RecordingSummaryVm>("recording_session_summary", { folder });
}

/**
 * The note stub waiting for one session (Story 42.4, FR-142), or `null` when
 * there is none.
 *
 * `null` is an ordinary answer, never a failure: a stub that could not be
 * written was logged at finalize (the recording succeeded regardless), and a
 * dismissed one is gone on purpose. The stop surface renders nothing in either
 * case.
 *
 * `contents` is what the FILE holds, not what Rust would compose — so calling
 * this after a save returns the user's own text, and re-seeding an untouched
 * draft from it can never resurrect something they deleted. Split it at
 * `bodyOffset` (UTF-16 code units, converted in Rust) to get keeper's
 * frontmatter block and the editable body.
 */
export async function recordingNoteStub(folder: string): Promise<RecordingNoteStubVm | null> {
  return await invoke<RecordingNoteStubVm | null>("recording_note_stub", { folder });
}

/**
 * Save the note the user typed (Story 42.4).
 *
 * `contents` is the WHOLE file — the untouched `bodyOffset` head plus the edited
 * body — because the frontend never owns keeper's frontmatter and must not be
 * able to send back a mangled one.
 *
 * The one stub command whose errors are surfaced rather than logged: until this
 * resolves the words exist only in the textarea, so the caller must print the
 * {@link IpcError} sentence and keep the editor open rather than dismissing.
 */
export async function recordingNoteStubSave(folder: string, contents: string): Promise<void> {
  await invoke<void>("recording_note_stub_save", { folder, contents });
}

/**
 * Dismiss a stub, deleting it only if the user never touched it (Story 42.4).
 * Resolves `true` when the file was deleted, `false` when it was kept.
 *
 * Nothing the caller passes can widen what this deletes: Rust recomposes the
 * stub from the session's manifest and removes the file only when the bytes on
 * disk are byte-identical to it. `false` is therefore a legitimate outcome, not
 * an error — close the card and leave the file. Every uncertainty (unreadable
 * file, failed delete, a stub already gone) also resolves `false`, because
 * deleting a note somebody wrote is the one unrecoverable mistake here.
 */
export async function recordingNoteStubDismiss(folder: string): Promise<boolean> {
  return await invoke<boolean>("recording_note_stub_dismiss", { folder });
}

/**
 * Rename a finished session (Story 40.4) — the affordance on the completion /
 * recovery card. The title is the manifest's `meta.title` (Story 21.5, the only
 * title there has ever been), and setting it MOVES the session on disk: Rust
 * re-renders the effective path template against the session's OWN start
 * instant with the new title, `create_dir`s the rendered leaf, `fs::rename`s
 * the session onto it, and rewrites `manifest.json`'s title and its `session`
 * label. The identity does NOT move — `meta.sessionId` is byte-identical
 * afterwards, so everything latched on it (a recovery dismissal) stays attached
 * to the session it was about.
 *
 * `folder` is the session folder as it stands NOW; `title` is the new title, or
 * `null` to clear it (which moves the session back to its untitled path). A
 * rendered path that is already taken gets the template's next `{seq}` ordinal
 * — the existing folder is never touched — and a session that renders to the
 * folder it already occupies is rewritten in place, moving nothing.
 *
 * Resolves the summary of the session AT ITS NEW LOCATION: `sessionFolder` is
 * the folder it now occupies, and the caller MUST re-render from it. The path
 * it was called with no longer exists, so a Reveal in Finder still aimed at the
 * old one opens nothing.
 *
 * Rejects with the {@link IpcError} envelope, and these refusals are the user's
 * to read, not the caller's to swallow: a session that is still recording is
 * refused with code `recordingSessionLive` (the driver and the sidecar hold
 * absolute paths), and "stop the recording first" is the only way out of it.
 * An exhausted ordinal run, a folder with no loadable manifest, and a folder
 * outside the recordings destination are refused the same way — with nothing
 * moved either.
 */
export async function recordingRetitle(
  folder: string,
  title: string | null,
): Promise<RecordingSummaryVm> {
  return await invoke<RecordingSummaryVm>("recording_retitle", { folder, title });
}

/**
 * List the crash-recovered sessions still needing a one-time notice (Story 20.3,
 * FR-73). The Rust core walks the effective recordings destination (Story 40.3 —
 * the path template may nest sessions under it) for a loadable `manifest.json`
 * with `status:"recovered"` whose acknowledgement key is NOT in the persisted
 * seen-set, deterministically sorted. Best-effort: a missing destination dir
 * resolves an empty array; a per-entry failure is skipped, never thrown.
 * Resolves an array (empty when nothing is due).
 */
export async function recoveredSessionsList(): Promise<RecordingSummaryVm[]> {
  return await invoke<RecordingSummaryVm[]>("recovered_sessions_list");
}

/**
 * Acknowledge (dismiss) a surfaced recovery card (Story 20.3, FR-73): the Rust
 * core latches the session's acknowledgement key — its immutable `meta.sessionId`
 * since Story 40.3, or its destination-relative folder path for a session
 * recorded before that — into the persisted seen-set so
 * {@link recoveredSessionsList} never surfaces it again on a later scan/restart.
 * Keying on the identity is what keeps a dismissal attached to the session when
 * the folder is later moved or retitled. A one-way, idempotent registry write.
 * `folder` is the session folder path. Every read the latch needs (the
 * destination setting, the manifest) degrades to a logged no-op rather than a
 * rejection, so a dismiss the user cannot retry differently never fails on them;
 * rejects with the {@link IpcError} envelope only on a write failure (the card
 * may then reappear next scan).
 */
export async function recoveredSessionAcknowledge(folder: string): Promise<void> {
  await invoke<void>("recovered_session_acknowledge", { folder });
}

/**
 * Read the effective segmentation settings (Story 17.5, FR-72) — the segment
 * size (MB) and duration-cap fallback (minutes) persisted in the Rust `settings`
 * k/v table. The Rust getters default (500 MB / 30 min) and clamp defensively,
 * so the resolved VM always sits in the authored bounds. Both settings surfaces
 * hydrate their shared store from this.
 *
 * The read also carries the effective `pathTemplate` (Story 40.2), which is
 * always concrete: an absent, blank *or* no-longer-parsing stored template
 * degrades to the default rather than failing the read, so a hand-edited
 * `config.json` can never make the settings read error.
 */
export async function recordingSettingsGet(): Promise<RecordingSettingsVm> {
  return await invoke<RecordingSettingsVm>("recording_settings_get");
}

/**
 * Read how many rows a folder card's lists show, folded and unfolded.
 *
 * Rust defaults (10 / 100) and clamps, and reads `unfolded` as never less than
 * `folded`, so the resolved VM is always coherent even over a hand-edited row.
 */
export async function syncListSettingsGet(): Promise<SyncListSettingsVm> {
  return await invoke<SyncListSettingsVm>("sync_list_settings_get");
}

/**
 * Persist the folder-card list sizes, resolving the EFFECTIVE (clamped) VM.
 *
 * Clamp, not reject — the same contract the recording settings use: a number out
 * of bounds is pulled into range and returned, so the field never sits showing a
 * value the database does not hold.
 */
export async function syncListSettingsSet(
  settings: SyncListSettingsVm,
): Promise<SyncListSettingsVm> {
  return await invoke<SyncListSettingsVm>("sync_list_settings_set", { settings });
}

/**
 * Persist the segmentation settings (Story 17.5, FR-72). Rust clamps to the
 * authored bounds (segment 100–5000 MB, duration cap 1–600 min — clamp, not
 * reject), writes both values, and resolves the effective (clamped) VM so the
 * UI never displays an unsaved value. A running session is unaffected — edits
 * apply to the next Recording Session only.
 *
 * `pathTemplate` (Story 40.2) is the one field here that is REJECTED rather
 * than clamped: a template that does not parse rejects with the
 * {@link IpcError} envelope (`code: "recordingTemplateInvalid"`, `retriable:
 * false`) *before* any write, so not one settings row moves — including the
 * unrelated ones sent in the same request. A blank template is legal: it clears
 * the key, and the echoed VM carries the default.
 */
export async function recordingSettingsSet(
  settings: RecordingSettingsVm,
): Promise<RecordingSettingsVm> {
  return await invoke<RecordingSettingsVm>("recording_settings_set", { settings });
}

/**
 * Preview what a path template would name the next recording (Story 40.2,
 * UX-DR45/UX-DR46): it renders the TYPED template — not the stored one —
 * against the shell's clock and the EFFECTIVE destination root, so the line
 * under the field cannot disagree with the folder a recording started now
 * would actually create.
 *
 * Read-only in every sense: nothing is parsed into the settings table and
 * nothing is written, which is what makes it safe to call per keystroke.
 * Exactly one side of the VM is populated — `relativePath` + `absolutePath`
 * for a template that parses, `problem` for one that does not (an unparseable
 * template is the preview's most useful output, not a rejected promise).
 *
 * The sentences in `problem` are the Rust-authored 40.1 rejection reasons and
 * are meant to be rendered verbatim: a TypeScript re-implementation of the
 * render rules, the token vocabulary or their failure copy would be a second
 * renderer — the exact drift AD-65 forbids. One round trip per keystroke, so
 * the caller owns staleness and must drop every response but the newest typed
 * text's.
 */
export async function recordingPathPreview(
  template: string,
  title?: string | null,
): Promise<RecordingPathPreviewVm> {
  return await invoke<RecordingPathPreviewVm>("recording_path_preview", {
    template,
    title: title ?? null,
  });
}

/**
 * List the synced folders a recording destination may be pointed at (Story
 * 41.2) — the profiles that are ENABLED and recordings-flagged (their
 * `recordings` block is present, which only `keeper-syncd` writes), and
 * nothing else. A profile that merely exists is not offered here, and hiding
 * it in the picker is not the guard: `recording_settings_set` refuses an
 * unflagged id outright.
 *
 * Resolves an EMPTY list rather than rejecting whenever folder sync cannot
 * answer — no git on the machine, no engine, no profiles at all. That makes
 * "nothing to offer" and "sync is unavailable" one code path for the caller:
 * the destination card renders its plain folder chooser and says nothing new.
 *
 * `recordingsRoot` is the RESOLVED absolute root (`local_path` joined with the
 * profile's recordings subfolder), composed by Rust. The caller NEVER joins
 * paths: a second joiner in TypeScript would drift from the one that actually
 * decides where a segment lands, and the resolved root is also what
 * `RecordingSettingsVm.destinationDir` carries once a profile is chosen.
 */
export async function recordingDestinationProfiles(): Promise<RecordingProfileVm[]> {
  return await invoke<RecordingProfileVm[]>("recording_destination_profiles");
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
/** Read the live debug-mode toggle (Story 22.5, FR-79). */
export async function debugModeGet(): Promise<boolean> {
  return invoke<boolean>("debug_mode_get");
}

/**
 * Set the debug-mode toggle (Story 22.5, FR-79): persists `debug.mode` and
 * flips on-disk logging live — the app log (~/Library/Logs/keeper/keeper.log)
 * and the per-session events.log beside each manifest.
 */
export async function debugModeSet(enabled: boolean): Promise<void> {
  await invoke("debug_mode_set", { enabled });
}

/** Which stage of an app-driven title-bar drag is being reported (Story 34.3). */
export type TitlebarDragStage = "issued" | "accepted" | "refused";

/**
 * Start dragging the window from the current mouse-down (Story 34.3).
 *
 * The same `plugin:window|start_dragging` command Tauri's `data-tauri-drag-region`
 * shim invokes, called explicitly so its outcome is observable: the shim drops the
 * promise, which is why a refused drag is silent today. Issues the IPC message
 * synchronously — nothing is awaited before the call — because on macOS the window
 * layer only honours the drag for the mouse event it is currently processing.
 *
 * Rejects with whatever the window plugin rejects with: a bare string for an ACL
 * denial (`window.start_dragging not allowed. …`), never the {@link IpcError}
 * envelope, so this deliberately skips the {@link invoke} normalization.
 */
export async function startWindowDragging(): Promise<void> {
  await getCurrentWindow().startDragging();
}

/**
 * Record one stage of an app-driven title-bar drag in the app log (Story 34.3).
 *
 * Diagnostic-only, and the only frontend path into `~/Library/Logs/keeper/keeper.log`:
 * Rust authors the log text, `detail` carries a refusal message. Rejects with the
 * {@link IpcError} envelope; callers swallow it — a report must never be the thing
 * that breaks a drag.
 */
export async function titlebarDragReport(stage: TitlebarDragStage, detail?: string): Promise<void> {
  await invoke<void>("titlebar_drag_report", { stage, detail: detail ?? null });
}

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

// ---------------------------------------------------------------------------
// Folder sync (Epic 29, FR-77..FR-93)
//
// Every one of these rejects with `unsupported` when the machine has no usable
// `git` (AD-41), which is why the UI gates on `CapabilitiesVm.sync` and hides
// the surface entirely rather than offering an action that cannot work.
// ---------------------------------------------------------------------------

/**
 * List every configured sync profile.
 *
 * Crosses IPC: profile configuration only -- never a credential, which lives in
 * the OS keychain and is referenced by an opaque key the frontend never sees.
 *
 * Rejects with: `unsupported` (no usable git), `internal`.
 */
export async function syncProfiles(): Promise<SyncProfileVm[]> {
  return await invoke<SyncProfileVm[]>("sync_profiles");
}

/**
 * Read a status snapshot for every profile -- what the sync pane renders and
 * what the tray line is composed from.
 *
 * Polled rather than streamed on purpose: the tray must render correctly when
 * no webview is subscribed at all.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function syncStatuses(): Promise<SyncStatusVm[]> {
  return await invoke<SyncStatusVm[]>("sync_statuses");
}

/**
 * Create or update a profile, resolving the stored result.
 *
 * Omit `id` to create. The request is validated in Rust before it reaches the
 * engine, so an impossible profile (a relative path, a bidirectional review
 * lane) rejects here rather than half-applying.
 *
 * Rejects with: `unsupported`, `internal` (validation, naming the bad value).
 */
export async function syncProfileSave(req: SyncProfileReq): Promise<SyncProfileVm> {
  return await invoke<SyncProfileVm>("sync_profile_save", { req });
}

/**
 * Forget a profile.
 *
 * Configuration only: the folder and its git repository are left on disk
 * exactly as they are. Removing a profile never deletes content.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function syncProfileRemove(id: string): Promise<void> {
  await invoke<void>("sync_profile_remove", { id });
}

/**
 * Pause or resume a profile, resolving its resulting status.
 *
 * A paused profile keeps its journal: resuming re-drives whatever was queued
 * rather than starting over.
 *
 * Rejects with: `unsupported`, `internal` (no such profile).
 */
export async function syncProfileSetEnabled(id: string, enabled: boolean): Promise<SyncStatusVm> {
  return await invoke<SyncStatusVm>("sync_profile_set_enabled", { id, enabled });
}

/**
 * Sync one profile now, ignoring its schedule.
 *
 * Named for the folder, not the app: `syncNow` is the Matrix sync kick and the
 * two must never be confused.
 *
 * Rejects with: `unsupported`, `serverUnreachable` (retriable),
 * `invalidCredentials`, `internal`.
 */
export async function syncFolderNow(id: string): Promise<SyncOutcomeVm> {
  return await invoke<SyncOutcomeVm>("sync_folder_now", { id });
}

/**
 * Re-verify a profile's stored content against its recorded digests.
 *
 * Resolves the list of problems found, each as `"<path>: <reason>"`. An empty
 * array means everything checked out.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function syncVerify(id: string): Promise<string[]> {
  return await invoke<string[]>("sync_verify", { id });
}

/**
 * Forget what a profile remembers about its own tree, so the next walk sees it
 * fresh.
 *
 * The counterpart to {@link syncVerify}, and a different question: verify asks
 * "is what I have intact", this asks "is what I *think* I have still what is
 * there". A file copied in with its modification time preserved matches the
 * remembered row exactly, so no amount of re-scanning finds it — clearing the
 * memory is what changes the answer.
 */
export async function syncRescan(id: string): Promise<void> {
  await invoke<void>("sync_rescan", { id });
}

/**
 * Open a synced folder in the OS file manager (Finder on macOS).
 *
 * Takes the profile id, never a path: Rust reads the folder off the stored
 * profile, so this cannot be used to open an arbitrary location on disk the way
 * {@link revealPath} can. That one stays for a path the frontend already
 * legitimately holds -- an export it just produced -- and sync does not widen
 * it.
 *
 * Gate the affordance on `capabilities.revealInFileManager`: a platform with no
 * user-visible file manager rejects rather than doing nothing.
 *
 * Rejects with: `internal` (no such profile, the folder is gone or its volume
 * is not attached, the file manager refused). The message names the folder and
 * the next step, so it is worth showing verbatim.
 */
export async function syncOpenPath(id: string): Promise<void> {
  await invoke<void>("sync_open_path", { id });
}

/**
 * Read the newest recorded activity for a profile -- what sync has actually
 * done to which files, newest first.
 *
 * Crosses IPC: a repo-relative path, a kind and a timestamp per row, and
 * nothing else -- never file contents. The engine keeps only the newest rows
 * per profile, so this is recent history rather than an audit log; `limit`
 * narrows it further and defaults to whatever the engine considers a page.
 *
 * Rejects with: `unsupported`, `internal` (no such profile).
 */
export async function syncActivity(id: string, limit?: number): Promise<SyncActivityVm[]> {
  return await invoke<SyncActivityVm[]>("sync_activity", { id, limit });
}

/**
 * Read what a profile has seen but not yet carried.
 *
 * Computed at query time from the worktree and the quiescence gate rather than
 * stored, so it cannot disagree with what the next tick will do. A `settling`
 * row carries `sinceMs` (when the quiet window began) -- render it as how long
 * keeper has been waiting, never as a countdown: every new write restarts the
 * window, so a finish time would be a guess.
 *
 * Rejects with: `unsupported`, `internal` (no such profile). A rejection is
 * not an empty list -- an unknown profile rejects rather than reporting calm.
 */
export async function syncPending(id: string): Promise<SyncPendingVm[]> {
  return await invoke<SyncPendingVm[]>("sync_pending", { id });
}

/**
 * Read everything currently wrong with a profile: the live warning/error, the
 * journal units that failed permanently, and the conflict copies still on disk.
 *
 * A list rather than one string, so a single success cannot clear seven
 * distinct conditions. Conflict entries leave on their own once the user
 * deletes the copy they no longer need.
 *
 * Rejects with: `unsupported`, `internal` (no such profile).
 */
export async function syncProblems(id: string): Promise<SyncProblemsVm> {
  return await invoke<SyncProblemsVm>("sync_problems", { id });
}

/**
 * Return one parked journal unit to the pending queue.
 *
 * `unitId` is the `id` of a {@link SyncParkedVm} read from
 * {@link syncProblems}: parking is per unit of work, so retrying one failed
 * push never re-drives the rest.
 *
 * Rejects with: `unsupported`, `internal` (no such profile or unit).
 */
export async function syncRetryParked(id: string, unitId: number): Promise<void> {
  await invoke<void>("sync_retry_parked", { id, unitId });
}

/**
 * Which `git` folder sync resolved, or why there isn't one (Story 34.14).
 *
 * Answers on exactly the machines where the engine will not open, which is the
 * point: it resolves without opening one. `state` is `ok` precisely when
 * `CapabilitiesVm.sync` is true, so this is also the explanation for a missing
 * Sync section rather than a second opinion about it.
 *
 * Never rejects for want of a git -- an absent one is a `state`, not an error.
 */
export async function syncGitStatus(): Promise<SyncGitVm> {
  return await invoke<SyncGitVm>("sync_git_status");
}

/**
 * Name the `git` binary folder sync should use, and get the new report back.
 *
 * An empty string clears the setting and returns to searching `PATH`. The
 * returned {@link SyncGitVm} reflects the path that is now in force, including
 * when it was rejected -- a field that cleared itself on a bad value would be a
 * silent fallback to automatic, which is the defect this exists to end.
 *
 * Rejects with: `unsupported` (no folder sync in this build), `internal`.
 */
export async function syncGitPathSet(path: string): Promise<SyncGitVm> {
  return await invoke<SyncGitVm>("sync_git_path_set", { path });
}

/**
 * Store an access token for a profile in the OS keychain.
 *
 * The token goes straight to the keychain under a key derived from the profile
 * id -- never into the config file, never into `sync.db`. It can be read back
 * through {@link syncGetCredential}, which the edit form calls as it opens.
 *
 * Rejects with: `unsupported`, `internal` (no such profile, keychain refusal).
 */
export async function syncSetCredential(id: string, token: string): Promise<void> {
  await invoke<void>("sync_set_credential", { id, token });
}

/**
 * Read a profile's stored access token out of the OS keychain.
 *
 * Resolves `null` when the profile has no stored token, which is an ordinary
 * state rather than a failure -- a public remote needs none. The edit form
 * calls this as it opens, so the field arrives holding the stored token
 * (Story 34.12, overriding AD-34-7). {@link SyncProfileVm} still carries no
 * token: a profile list is read for every folder at once and on a poll, and a
 * secret has no business in it.
 *
 * Rejects with: `unsupported`, `internal` (no such profile, keychain refusal).
 */
export async function syncGetCredential(id: string): Promise<string | null> {
  return await invoke<string | null>("sync_get_credential", { id });
}

/**
 * Forget a profile's stored access token. Clearing a profile that has none is
 * a no-op, so this is safe to offer whether or not one was ever set.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function syncClearCredential(id: string): Promise<void> {
  await invoke<void>("sync_clear_credential", { id });
}

/**
 * Read this device's identity -- the name that rides every commit keeper makes.
 *
 * Minted once from the machine's hostname the first time sync opens and the
 * user's from then on, so it is read from the engine rather than re-derived
 * here: a renamed device must not answer with its hostname again.
 *
 * Rejects with: `unsupported` (no usable git), `internal`.
 */
export async function syncDevice(): Promise<SyncDeviceVm> {
  return await invoke<SyncDeviceVm>("sync_device");
}

/**
 * Rename this device, resolving the identity as stored.
 *
 * Takes effect on the next commit and rewrites nothing: a `Keeper-Device`
 * trailer already in a repository keeps the name the machine had when it made
 * that commit. The `id` never changes -- it is what a shared history tells two
 * machines apart by, and what the git author address is derived from.
 *
 * Use the resolved label rather than the argument: the store trims it.
 *
 * Rejects with: `unsupported`, `internal` (an empty label).
 */
export async function syncDeviceSetLabel(label: string): Promise<SyncDeviceVm> {
  return await invoke<SyncDeviceVm>("sync_device_set_label", { label });
}

/**
 * Stream sync progress, resolving the subscription id.
 *
 * Complements {@link syncStatuses} rather than replacing it: this gives a
 * subscribed window sub-second detail, while the polled snapshot is what lets
 * the tray render correctly with no webview subscribed at all.
 *
 * The subscription cleans itself up when the channel closes, so a reload cannot
 * accumulate dead sinks -- but call {@link syncUnsubscribeProgress} on unmount
 * anyway so the engine stops composing events nobody reads.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function syncSubscribeProgress(
  onProgress: (event: SyncProgressVm) => void,
): Promise<number> {
  const channel = new Channel<SyncProgressVm>();
  // Armed before invoking: the ordering is load-bearing, exactly as in
  // `subscribe()` above -- an event emitted between invoke and assignment
  // would otherwise be dropped.
  channel.onmessage = onProgress;
  return await invoke<number>("sync_subscribe_progress", { channel });
}

/**
 * Stop a progress subscription. Unsubscribing an unknown id is a no-op, so a
 * double-unsubscribe from a racing unmount is safe.
 */
export async function syncUnsubscribeProgress(id: number): Promise<void> {
  await invoke<void>("sync_unsubscribe_progress", { id });
}

// ---------------------------------------------------------------------------
// One-time verified copy (Epic 33, AD-C1..AD-C6)
//
// A copy is a job, never a relationship: it is keyed by an opaque id, lives in
// app memory for the length of the run, and finishing it changes nothing about
// either folder. Nothing here is written into `profiles` and nothing here joins
// the sync journal.
// ---------------------------------------------------------------------------

/**
 * Start copying `source` into `destination`, resolving the job id to poll.
 *
 * `source` may be a file or a directory; `destination` is always a directory
 * the copy lands inside. Resolves as soon as the job is registered — the work
 * itself runs on a blocking thread, because every byte is hashed twice.
 *
 * Rejects with the {@link IpcError} envelope before any job exists when
 * `source` does not exist, or when `destination` sits inside `source` (which
 * would copy the tree into itself); the message names which one it was.
 *
 * `replaceExisting` defaults to `false` in Rust: an existing destination file
 * with identical content is reported `identical`, a differing one is reported
 * `collision` and left untouched. With it set, the old bytes are replaced only
 * after the new ones have passed verification.
 */
export async function copyStart(
  source: string,
  destination: string,
  replaceExisting?: boolean,
): Promise<string> {
  return await invoke<string>("copy_start", { source, destination, replaceExisting });
}

/**
 * Read one job's snapshot -- what the copy card polls and renders from.
 *
 * `entries` is empty until the job reaches a terminal state, so a partial
 * report can never be rendered as a finished one, and `error` is the job
 * failing to run at all: one unreadable file is an entry with outcome
 * `failed`, not a failed job.
 *
 * Rejects with the {@link IpcError} envelope for an id nobody minted -- a
 * caller polling an unknown job is a bug worth seeing, not a job that quietly
 * never finishes.
 */
export async function copyStatus(id: string): Promise<CopyJobVm> {
  return await invoke<CopyJobVm>("copy_status", { id });
}

/**
 * Ask a job to stop. Idempotent, and safe at any moment: the engine checks the
 * flag between files and between chunks, leaves no temp file behind, and
 * settles the job `cancelled` with the report of everything that had already
 * finished.
 */
export async function copyCancel(id: string): Promise<void> {
  await invoke<void>("copy_cancel", { id });
}

// ---------------------------------------------------------------------------
// Notes (Phase 5, FR-94..FR-124)
//
// A vault is a notes-flagged sync profile plus a subfolder (AD-54), so every one
// of these rejects with `unsupported` where folder sync cannot run, and the UI
// gates the whole surface on `CapabilitiesVm.notes` rather than offering an
// action that cannot work. Nothing large crosses here (AD-58): the list carries
// row view models windowed to what is on screen, a note BODY arrives only over a
// `Channel`, and attachment bytes never cross in either direction — paste and
// drop are payload-free because Rust reads the clipboard and Tauri hands Rust
// the dropped paths.
//
// Notes subscriptions resolve with a `String` id rather than the `number` the
// older streams use, so they route through `subscribeWithStringId` below instead
// of `subscribe`.
// ---------------------------------------------------------------------------

/**
 * Open a `Channel` for a notes stream and resolve with its Rust subscription id.
 *
 * The twin of {@link subscribe} for the notes commands, which key their
 * subscriptions by string. The `onmessage`-before-`invoke` ordering is the same
 * load-bearing rule and for the same reason: every notes stream opens with a
 * snapshot emitted from a spawned task, and a handler armed after the
 * id-returning command resolves would miss it.
 */
async function subscribeWithStringId<TBatch>(
  cmd: string,
  onBatch: (batch: TBatch) => void,
  args?: Record<string, unknown>,
): Promise<string> {
  const channel = new Channel<TBatch>();
  channel.onmessage = onBatch;
  return await invoke<string>(cmd, { ...args, channel });
}

/**
 * Every notes-flagged sync profile, with its index state and unread count
 * (FR-94, FR-95, AD-54). The vault list IS a filter over the profile list, so
 * there is nothing else to read and no second registry to keep in step.
 *
 * Rejects with: `unsupported` (no folder sync on this build), `internal`.
 */
export async function notesVaults(): Promise<NoteVaultVm[]> {
  return await invoke<NoteVaultVm[]>("notes_vaults");
}

/**
 * Flag a synced folder as a notes vault, or unflag it (FR-94).
 *
 * `config` absent unflags; unflagging removes no files and moves nothing —
 * keeper only forgets that the folder held a vault.
 *
 * Rejects with: `invalidInput` (a subfolder that is empty, absolute, escapes
 * with `..`, or is `.obsidian`), `unsupported`, `internal`.
 */
export async function notesVaultFlag(
  profileId: string,
  config: NoteVaultSettingsReq | null,
): Promise<NoteVaultVm> {
  return await invoke<NoteVaultVm>("notes_vault_flag", { profileId, config });
}

/**
 * Save one vault's four knobs — subfolder, journal template, default template
 * and sync cadence (FR-120). Cadence values below the engine's floors are
 * clamped in Rust, so the returned VM is what is actually in force.
 *
 * Rejects with: `invalidInput`, `unsupported`, `internal`.
 */
export async function notesVaultSettingsSave(
  vaultId: string,
  settings: NoteVaultSettingsReq,
): Promise<NoteVaultVm> {
  return await invoke<NoteVaultVm>("notes_vault_settings_save", { vaultId, settings });
}

/**
 * The vault everything vault-scoped resolves against, or `null` when nothing is
 * selected or the stored id no longer names a flagged profile.
 *
 * Rust owns this rather than the webview because the tray's New Note, Today's
 * Journal and recent slots, and the capture window's commit, all run with no
 * main window open at all. A second selection held only in a store would send
 * those writes into a different vault than the one on screen.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesVaultActive(): Promise<string | null> {
  return await invoke<string | null>("notes_vault_active");
}

/**
 * Switch the active vault (FR-95). A filter change, not a navigation (UX-DR41):
 * the note open in the editor stays open when it belongs to the new vault.
 *
 * Rejects with: `invalidInput` (unknown vault), `unsupported`, `internal`.
 */
export async function notesVaultSetActive(vaultId: string): Promise<void> {
  await invoke<void>("notes_vault_set_active", { vaultId });
}

/**
 * Drop a vault's `.keeper/index.json` cache and cold-scan it again (FR-96,
 * AD-57). The index is a cache, never a database: rebuilding it is a supported
 * repair and loses nothing, because every field in it is derived from files the
 * user owns. Progress streams on the index channel.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesIndexRebuild(vaultId: string): Promise<void> {
  await invoke<void>("notes_index_rebuild", { vaultId });
}

/**
 * One window of the filtered note list (FR-103, AD-58). `query` carries every
 * filter axis — text, tag chips, space, origin, flags — plus `offset`/`limit`,
 * and the returned {@link NoteListVm} carries `total` alongside the window so a
 * scrollbar can be honest about 10 000 rows without shipping them.
 *
 * Rows are view models. A row never carries a body; a body is a `Channel`
 * (see {@link notesOpen}).
 *
 * Rejects with: `invalidInput` (a malformed space query), `unsupported`,
 * `internal`.
 */
export async function notesList(vaultId: string, query: NoteQueryReq): Promise<NoteListVm> {
  return await invoke<NoteListVm>("notes_list", { vaultId, query });
}

/**
 * The vault's hierarchical tag tree with per-node counts (FR-104). Counts are of
 * the UNFILTERED vault, so a node never appears to shrink as chips are added — a
 * count that changes meaning mid-interaction is a lie.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesTagTree(vaultId: string): Promise<NoteTagTreeVm> {
  return await invoke<NoteTagTreeVm>("notes_tag_tree", { vaultId });
}

/**
 * The flat tag vocabulary — every known tag with its count (Story 42.5,
 * FR-143). One list, both producers: a tag that exists only on notes and a tag
 * that exists only on recordings are both in it, and a count is the sum of
 * everything carrying that tag or anything under it.
 *
 * For the surfaces that cannot consume {@link notesTagTree}: the recording
 * metadata card's tag field is a plain `<input>`, and the notes editor's
 * existing affordance is a CodeMirror `CompletionSource`. This is the same
 * vocabulary both of them offer, not a second one shaped for a text box.
 *
 * `vaultId` is optional — omit it on a surface that is not inside a vault (the
 * recording card) and the active vault answers. An unknown vault, or no vault at
 * all, resolves with `{ entries: [] }` rather than rejecting: a completion with
 * nothing to offer is a usable outcome, an error in a tag field is not.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function tagsVocabulary(vaultId?: string): Promise<TagVocabularyVm> {
  return await invoke<TagVocabularyVm>("tags_vocabulary", { vaultId: vaultId ?? null });
}

/**
 * One level of the physical folder tree (FR-106, UX-DR38) — the lens that is
 * always one click away, so a virtual row can always be traced to a real path.
 *
 * Rejects with: `invalidInput` (a directory outside the vault), `unsupported`,
 * `internal`.
 */
export async function notesTree(vaultId: string, relDir: string): Promise<NoteFolderVm> {
  return await invoke<NoteFolderVm>("notes_tree", { vaultId, relDir });
}

/**
 * Every space in the vault (FR-105) — ordinary notes under `spaces/`, each
 * carrying a saved query. A space whose query does not parse comes back with its
 * `error` set rather than being dropped: it is an agent-writable plain note, so
 * a broken one is expected and must not break the sidebar.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesSpaces(vaultId: string): Promise<NoteSpaceVm[]> {
  return await invoke<NoteSpaceVm[]>("notes_spaces", { vaultId });
}

/**
 * Write or update a space note (FR-105). This is what "save this filter as a
 * space" produces, and it produces an ordinary markdown note — so the
 * organisation syncs, diffs and can be edited by hand or by an agent.
 *
 * Rejects with: `invalidInput` (an unparseable query), `unsupported`, `internal`.
 */
export async function notesSpaceSave(vaultId: string, space: NoteSpaceReq): Promise<NoteRefVm> {
  return await invoke<NoteRefVm>("notes_space_save", { vaultId, space });
}

/**
 * Parse a space query without running it — what underlines the offending token
 * while a query is being typed. Never rejects on a bad query: an unparseable
 * query is a {@link NoteQueryCheckVm} with `ok: false` and a located message,
 * because a parse failure is a state of the field, not a failed command.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesSpaceValidate(query: string): Promise<NoteQueryCheckVm> {
  return await invoke<NoteQueryCheckVm>("notes_space_validate", { query });
}

/**
 * Create a note (FR-98). Every field of `req` is optional-shaped because there
 * is no dialog anywhere in this path (UX-DR35): a title comes from the first
 * line if it is not supplied, and the destination is a rule rather than a
 * question.
 *
 * Rejects with: `invalidInput` (an illegal name), `unsupported`, `internal`.
 */
export async function notesCreate(vaultId: string, req: NoteCreateReq): Promise<NoteRefVm> {
  return await invoke<NoteRefVm>("notes_create", { vaultId, req });
}

/**
 * Open today's journal entry, creating it from the vault's journal template if
 * it does not exist yet (FR-99). Idempotent — twice in a day is one note.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesJournalToday(vaultId: string): Promise<NoteRefVm> {
  return await invoke<NoteRefVm>("notes_journal_today", { vaultId });
}

/**
 * Every `templates/*.md` in the vault (FR-100). An empty vault answers with an
 * empty list, never an error — keeper creates `templates/` lazily, on first use,
 * because an empty scaffold in someone's existing vault is exactly the "keeper
 * moved my stuff" failure FR-121 forbids.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesTemplates(vaultId: string): Promise<NoteTemplateVm[]> {
  return await invoke<NoteTemplateVm[]>("notes_templates", { vaultId });
}

/**
 * Open a note's body as a stream (AD-58) and resolve with the subscription id.
 *
 * A body is never a command return value. The stream opens with a full
 * `Reset { rev, text }` snapshot and then delivers what happened to the document
 * — an external write merged in, a divergence to review, a rename, a deletion —
 * so an agent writing into the open note is a diff applied to a live buffer
 * rather than a poll that destroys the cursor.
 *
 * Rejects with: `invalidInput` (unknown note), `unsupported`, `internal`.
 */
export async function notesOpen(
  vaultId: string,
  noteId: string,
  onBatch: (batch: NoteBodyBatch) => void,
): Promise<string> {
  return await subscribeWithStringId<NoteBodyBatch>("notes_open", onBatch, { vaultId, noteId });
}

/**
 * Close one body subscription, aborting its backend producer. Idempotent — an
 * unknown id is a no-op.
 */
export async function notesClose(subscriptionId: string): Promise<void> {
  await invoke<void>("notes_close", { subscriptionId });
}

/**
 * Report the editor's current buffer text to Rust — the dirty-text heartbeat the
 * three-way merge needs.
 *
 * Rust holds `base` (what it last wrote or last delivered) and `theirs` (the new
 * disk bytes); this is what keeps `mine` current, so an external write arriving
 * mid-edit can be merged instead of refused. Sent after a typing pause and
 * immediately on blur, never per keystroke.
 *
 * Rejects with: `invalidInput` (unknown subscription), `unsupported`, `internal`.
 */
export async function notesBufferReport(
  subscriptionId: string,
  text: string,
  rev: string,
): Promise<void> {
  await invoke<void>("notes_buffer_report", { subscriptionId, text, rev });
}

/**
 * Flush the buffer to disk. `baseRev` is the revision the buffer opened at: when
 * it is older than what is on disk, Rust writes the disk bytes out as an
 * AD-43-shaped conflict copy FIRST and the buffer second, so saving over a
 * divergence loses neither side (NFR-30). The returned
 * {@link NoteWriteVm} names the copy when one was made.
 *
 * `text` is the **body** — the editor never holds the frontmatter block. Pass
 * `frontmatter` only to rewrite the block itself, which is the properties panel's
 * job; `null` keeps the block Rust last delivered, byte for byte.
 *
 * Rejects with: `invalidInput`, `unsupported`, `internal`.
 */
export async function notesSave(
  subscriptionId: string,
  text: string,
  baseRev: string,
  frontmatter: string | null = null,
): Promise<NoteWriteVm> {
  return await invoke<NoteWriteVm>("notes_save", {
    subscriptionId,
    text,
    baseRev,
    frontmatter,
  });
}

/**
 * Retitle a note and rename its file (FR-97). Links keep resolving because they
 * resolve through the note's ULID `id`, not its filename.
 *
 * Rejects with: `invalidInput` (an illegal name), `unsupported`, `internal`.
 */
export async function notesRename(
  vaultId: string,
  noteId: string,
  title: string,
): Promise<NoteRefVm> {
  return await invoke<NoteRefVm>("notes_rename", { vaultId, noteId, title });
}

/**
 * Set or clear `pinned` / `archived` in a note's frontmatter (FR-119). The write
 * touches that one key and leaves every other byte identical — the FR-121
 * guarantee is what makes editing someone's own file acceptable at all.
 *
 * Rejects with: `invalidInput`, `unsupported`, `internal`.
 */
export async function notesSetFlag(
  vaultId: string,
  noteId: string,
  flag: NoteFlag,
  on: boolean,
): Promise<void> {
  await invoke<void>("notes_set_flag", { vaultId, noteId, flag, on });
}

/**
 * Move a note to `.keeper/trash/` and stage the removal (NFR-30). Never an
 * unlink — a delete keeper cannot undo is a delete keeper should not offer.
 *
 * Rejects with: `invalidInput`, `unsupported`, `internal`.
 */
export async function notesDelete(vaultId: string, noteId: string): Promise<void> {
  await invoke<void>("notes_delete", { vaultId, noteId });
}

/**
 * Run a bounded parallel content scan over the vault, streaming hits as they are
 * found (FR-118), and resolve with the subscription id.
 *
 * It reads the files rather than an index, which is the whole argument for not
 * shipping a search engine at this size: a note written a millisecond ago is
 * matched, because there is nothing to invalidate.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesSearch(
  vaultId: string,
  req: NoteSearchReq,
  onBatch: (batch: NoteSearchBatch) => void,
): Promise<string> {
  return await subscribeWithStringId<NoteSearchBatch>("notes_search", onBatch, { vaultId, req });
}

/**
 * Wikilink autocomplete candidates for a `[[` prefix (FR-108), ranked in Rust.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesLinkTargets(
  vaultId: string,
  prefix: string,
): Promise<NoteLinkTargetVm[]> {
  return await invoke<NoteLinkTargetVm[]>("notes_link_targets", { vaultId, prefix });
}

/**
 * The notes that link TO this one (FR-108), projected from the link graph.
 * Derived, never stored.
 *
 * Rejects with: `invalidInput`, `unsupported`, `internal`.
 */
export async function notesBacklinks(vaultId: string, noteId: string): Promise<NoteRowVm[]> {
  return await invoke<NoteRowVm[]>("notes_backlinks", { vaultId, noteId });
}

/**
 * A note's revision history (FR-114, AD-63), projected from the commit trailers
 * `keeper-sync` already writes. keeper keeps no parallel history store, so a
 * vault whose profile has never committed answers with an honest empty list
 * rather than an error.
 *
 * Rejects with: `invalidInput`, `unsupported`, `internal`.
 */
export async function notesHistory(
  vaultId: string,
  noteId: string,
  limit: number,
): Promise<NoteRevisionVm[]> {
  return await invoke<NoteRevisionVm[]>("notes_history", { vaultId, noteId, limit });
}

/**
 * A unified diff between two revisions of one note. `toRev` absent diffs against
 * the working tree.
 *
 * Rejects with: `invalidInput` (unknown revision), `unsupported`, `internal`.
 */
export async function notesDiff(
  vaultId: string,
  noteId: string,
  fromRev: string,
  toRev: string | null,
): Promise<NoteDiffVm> {
  return await invoke<NoteDiffVm>("notes_diff", { vaultId, noteId, fromRev, toRev });
}

/**
 * Acknowledge a revision, clearing the note's unread mark and — when it was the
 * last one — the tray dot (FR-113).
 *
 * The mark is local state (the last revision the user accepted), compared
 * against the head revision that touched the path, so it survives a restart
 * without keeper writing read state into a file that would then sync.
 *
 * Rejects with: `invalidInput`, `unsupported`, `internal`.
 */
export async function notesMarkRead(vaultId: string, noteId: string, rev: string): Promise<void> {
  await invoke<void>("notes_mark_read", { vaultId, noteId, rev });
}

/**
 * Every unresolved conflict in the vault (FR-116) — a Syncthing-shaped conflict
 * copy recognised by name and bound back to its canonical note, so it is a row
 * in the list rather than litter to find on disk.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesConflicts(vaultId: string): Promise<NoteConflictVm[]> {
  return await invoke<NoteConflictVm[]>("notes_conflicts", { vaultId });
}

/**
 * Resolve a conflict by keeping one side or a merged body. The resolved body is
 * written as one new revision and the conflict copy is deleted only after that
 * write is acked (NFR-30) — there is no path by which either side is dropped
 * without the user choosing it.
 *
 * Rejects with: `invalidInput`, `unsupported`, `internal`.
 */
export async function notesResolveConflict(
  vaultId: string,
  noteId: string,
  choice: NoteConflictChoiceReq,
): Promise<NoteRefVm> {
  return await invoke<NoteRefVm>("notes_resolve_conflict", { vaultId, noteId, choice });
}

/**
 * Write the clipboard's image into `attachments/` and answer with the embed
 * (FR-110). No payload crosses IPC in either direction (AD-58): Rust reads the
 * system clipboard itself, so the webview never holds the bytes it is asking
 * keeper to write.
 *
 * Rejects with: `invalidInput` (no image on the clipboard), `unsupported`,
 * `internal`.
 */
export async function notesAttachmentPaste(
  vaultId: string,
  noteId: string,
): Promise<NoteAttachmentVm> {
  return await invoke<NoteAttachmentVm>("notes_attachment_paste", { vaultId, noteId });
}

/**
 * Import dropped files into `attachments/` and answer with their embeds
 * (FR-110). `paths` come from Tauri's own window drag-drop event, which hands
 * Rust real paths — they are never composed in JavaScript.
 *
 * Rejects with: `invalidInput`, `unsupported`, `internal`.
 */
export async function notesAttachmentDrop(
  vaultId: string,
  noteId: string,
  paths: string[],
): Promise<NoteAttachmentVm[]> {
  return await invoke<NoteAttachmentVm[]>("notes_attachment_drop", { vaultId, noteId, paths });
}

/**
 * The persisted quick-capture buffer (FR-101). Durable registry storage, not the
 * vault and not the index — a capture buffer is the one thing in this phase that
 * is not disposable, so it survives dismissal, a crash and a reboot.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesCaptureBuffer(): Promise<string> {
  return await invoke<string>("notes_capture_buffer");
}

/**
 * Persist the capture buffer. Debounced by the caller; cleared by exactly one
 * event, a write acknowledgement.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesCaptureBufferSave(text: string): Promise<void> {
  await invoke<void>("notes_capture_buffer_save", { text });
}

/**
 * Write the captured text into the active vault as an ordinary note and clear
 * the buffer (FR-101).
 *
 * Rejects with: `invalidInput` (no vault flagged), `unsupported`, `internal` —
 * and a rejection is what keeps the panel open with the text intact, because the
 * one thing capture may never do is swallow words.
 */
export async function notesCaptureCommit(text: string): Promise<NoteRefVm> {
  return await invoke<NoteRefVm>("notes_capture_commit", { text });
}

/**
 * Subscribe to the vault's list changes (AD-58) and resolve with the
 * subscription id. Opens with a `Reset` snapshot of the current window, then
 * diffs — coalesced in Rust to at most one message per 250 ms, so a 500-file
 * agent run is about four messages a second and not five hundred.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesSubscribeChanges(
  vaultId: string,
  onBatch: (batch: NoteChangeBatch) => void,
): Promise<string> {
  return await subscribeWithStringId<NoteChangeBatch>("notes_subscribe_changes", onBatch, {
    vaultId,
  });
}

/**
 * Unsubscribe exactly one changes subscription, aborting its backend producer.
 * Idempotent — an unknown id is a no-op.
 */
export async function notesUnsubscribeChanges(subscriptionId: string): Promise<void> {
  await invoke<void>("notes_unsubscribe_changes", { subscriptionId });
}

/**
 * Subscribe to cold-scan progress for one vault (FR-96) and resolve with the
 * subscription id. Opens with the current phase, so a subscriber that arrives
 * mid-scan is not left staring at nothing.
 *
 * Rejects with: `unsupported`, `internal`.
 */
export async function notesSubscribeIndex(
  vaultId: string,
  onProgress: (progress: NoteIndexProgressVm) => void,
): Promise<string> {
  return await subscribeWithStringId<NoteIndexProgressVm>("notes_subscribe_index", onProgress, {
    vaultId,
  });
}

/**
 * Show the quick-capture panel, positioned on the display holding the pointer
 * (FR-101, AD-60). Desktop only.
 *
 * The window is created hidden at startup with its textarea already focused, so
 * this focuses nothing and the first keystroke is never dropped (NFR-27).
 *
 * Rejects with: `unsupported` (non-desktop), `internal`.
 */
export async function notesCaptureShow(): Promise<void> {
  await invoke<void>("notes_capture_show");
}

/**
 * Hide the quick-capture panel. `commit` true is the Escape contract — write the
 * buffer, clear it, hide, and answer with the note written; `commit` false hides
 * and leaves the buffer intact.
 *
 * Resolves with `null` when nothing was written: an empty buffer (keeper never
 * creates an empty note) or no vault flagged (the text stays durable in the
 * buffer, per FR-101). Neither is an error. A write that FAILS rejects, and the
 * panel must then stay open with the text untouched.
 *
 * Rejects with: `unsupported` (non-desktop), `internal`.
 */
export async function notesCaptureHide(commit: boolean): Promise<NoteRefVm | null> {
  return await invoke<NoteRefVm | null>("notes_capture_hide", { commit });
}

/**
 * Reveal a note's real path in the OS file manager (UX-DR38). Desktop only.
 *
 * Every row in every lens can do this, and that is deliberate: the failure mode
 * of a virtual-folder system is that people stop believing they know where their
 * files are, and then stop trusting the tool with them.
 *
 * Rejects with: `invalidInput`, `unsupported` (non-desktop), `internal`.
 */
export async function notesReveal(vaultId: string, noteId: string): Promise<void> {
  await invoke<void>("notes_reveal", { vaultId, noteId });
}

/**
 * Open a file a note links to with the OS handler (FR-109). Desktop only. The
 * path may point anywhere inside the profile root, not only inside the vault —
 * and nowhere outside it.
 *
 * Rejects with: `invalidInput` (a path outside the profile root),
 * `unsupported` (non-desktop), `internal`.
 */
export async function notesOpenFile(vaultId: string, relPath: string): Promise<void> {
  await invoke<void>("notes_open_file", { vaultId, relPath });
}

/**
 * The Tauri event the shell emits when the tray asks the main window to open a
 * note — New Note, Today's Journal, or one of the five recent slots. Must match
 * the constant in `keeper/src/notes_ipc.rs`.
 */
export const NOTES_OPEN_NOTE_EVENT = "keeper://notes-open-note";

/**
 * The Tauri event the shell emits when the tray's unread item is chosen: open
 * the Notes view on that vault with the origin chip active.
 */
export const NOTES_SHOW_UNREAD_EVENT = "keeper://notes-show-unread";

/**
 * The Tauri event the shell emits after the capture panel is shown, so the
 * capture entry point can re-assert focus after a Linux compositor race.
 */
export const NOTES_CAPTURE_SHOWN_EVENT = "keeper://notes-capture-shown";

/**
 * Subscribe to the tray's open-a-note event. Resolves with an unlisten function;
 * registering is best-effort and graceful outside a Tauri webview (jsdom in
 * tests), so a failure leaves the bridge inert rather than crashing the shell.
 */
export async function listenNotesOpenNote(onOpen: (ref: NoteRefVm) => void): Promise<() => void> {
  return await listen<NoteRefVm>(NOTES_OPEN_NOTE_EVENT, (event) => {
    onOpen(event.payload);
  });
}

/** Subscribe to the tray's show-unread event, which carries the vault id. */
export async function listenNotesShowUnread(
  onShow: (vaultId: string) => void,
): Promise<() => void> {
  return await listen<string>(NOTES_SHOW_UNREAD_EVENT, (event) => {
    onShow(event.payload);
  });
}

/** Subscribe to the capture-shown event (payload-free). */
export async function listenNotesCaptureShown(onShown: () => void): Promise<() => void> {
  return await listen<null>(NOTES_CAPTURE_SHOWN_EVENT, () => {
    onShown();
  });
}
