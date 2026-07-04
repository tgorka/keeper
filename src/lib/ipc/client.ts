/**
 * Thin typed IPC client (AD-7, AD-8).
 *
 * The only hand-written TypeScript in `src/lib/ipc/`: wrappers around the Tauri
 * `invoke`/`Channel` primitives that carry the generated view-model types and
 * surface the {@link IpcError} envelope on rejection. All view-model types are
 * generated into `./gen/` by the Rust ts-rs export step — never hand-edited.
 */
import { Channel, invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { IpcError } from "./gen/IpcError";

export type { AccountVm } from "./gen/AccountVm";
export type { DemoBatch } from "./gen/DemoBatch";
export type { DemoItem } from "./gen/DemoItem";
export type { IpcError } from "./gen/IpcError";
export type { IpcErrorCode } from "./gen/IpcErrorCode";
export type { PingVm } from "./gen/PingVm";
export type { RoomListBatch } from "./gen/RoomListBatch";
export type { RoomListOp } from "./gen/RoomListOp";
export type { RoomVm } from "./gen/RoomVm";

import type { AccountVm } from "./gen/AccountVm";
import type { RoomListBatch } from "./gen/RoomListBatch";

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
