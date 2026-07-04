/**
 * Shared, tested reducer for eyeball-im `VectorDiff`-style ops (AD-8, AD-9,
 * AD-20).
 *
 * Both the room-list and timeline mirror stores fold Rust-streamed index-based
 * ops onto a plain array using this one implementation. It is pure and
 * immutable, applies ops strictly by index, guards every index against its
 * range, and — critically — **never sorts, re-sorts, filters, or re-indexes**.
 * Ordering is authoritative from the Rust SDK; the frontend only mirrors it.
 */

/**
 * The canonical union of index-based op variants shared by every store
 * (room-list `RoomListOp`, timeline `TimelineOp`, …). Each is an internally
 * tagged union whose `op` discriminant names the operation. The single-element
 * payload arrives under `item` (timeline) or `room` (room list); the list
 * payload under `items` or `rooms`. `applyDiffOp` reads whichever is present, so
 * every concrete op type is assignable here and the reducer stays store-agnostic.
 */
export type DiffOp<T> =
  | { op: "reset"; items?: T[]; rooms?: T[] }
  | { op: "append"; items?: T[]; rooms?: T[] }
  | { op: "clear" }
  | { op: "pushFront"; item?: T; room?: T }
  | { op: "pushBack"; item?: T; room?: T }
  | { op: "popFront" }
  | { op: "popBack" }
  | { op: "insert"; index: number; item?: T; room?: T }
  | { op: "set"; index: number; item?: T; room?: T }
  | { op: "remove"; index: number }
  | { op: "truncate"; length: number };

/** Read the single-element payload (`item` or `room`), or `undefined` if absent. */
function one<T>(op: { item?: T; room?: T }): T | undefined {
  return op.item ?? op.room;
}

/** Read the list payload (`items` or `rooms`), or `[]` if absent. */
function many<T>(op: { items?: T[]; rooms?: T[] }): T[] {
  return op.items ?? op.rooms ?? [];
}

/**
 * Fold a single index-based op onto `arr`, returning a new array (immutable).
 * Pure: no network, no derivation of truth, and — critically — no sorting.
 * Out-of-range `insert`/`set`/`remove`/`truncate` indices and single-element ops
 * with a missing payload are ignored (the op is dropped, leaving `arr` intact).
 */
export function applyDiffOp<T>(arr: T[], op: DiffOp<T>): T[] {
  switch (op.op) {
    case "reset":
      return [...many(op)];
    case "append":
      return [...arr, ...many(op)];
    case "clear":
      return [];
    case "pushFront": {
      const value = one(op);
      return value === undefined ? arr : [value, ...arr];
    }
    case "pushBack": {
      const value = one(op);
      return value === undefined ? arr : [...arr, value];
    }
    case "popFront":
      return arr.slice(1);
    case "popBack":
      return arr.slice(0, -1);
    case "insert": {
      const value = one(op);
      if (value === undefined || op.index < 0 || op.index > arr.length) {
        return arr;
      }
      const next = [...arr];
      next.splice(op.index, 0, value);
      return next;
    }
    case "set": {
      const value = one(op);
      if (value === undefined || op.index < 0 || op.index >= arr.length) {
        return arr;
      }
      const next = [...arr];
      next[op.index] = value;
      return next;
    }
    case "remove": {
      if (op.index < 0 || op.index >= arr.length) {
        return arr;
      }
      const next = [...arr];
      next.splice(op.index, 1);
      return next;
    }
    case "truncate":
      // A negative length would make `slice(0, length)` drop from the end and
      // silently corrupt the mirror; guard it (u32 on the wire, defensive here).
      return op.length < 0 ? arr : arr.slice(0, op.length);
    default: {
      // Exhaustiveness guard: a new `DiffOp` variant added without a case above
      // is a compile error, not a silent no-op — protecting the AD-8/AD-20
      // "never desync from Rust ordering" invariant. Runtime-safe fallback.
      const _exhaustive: never = op;
      void _exhaustive;
      return arr;
    }
  }
}
