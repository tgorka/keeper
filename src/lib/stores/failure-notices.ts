/**
 * The mirror of Rust's standing background failures (Epic 100, R381 as
 * amended by R387, 100.1 #7).
 *
 * A failure where no command was waiting — the wake phrase not re-armed, a
 * push registration refused — is logged at WARN in Rust and held there, per
 * operation (and per account where the operation is per account), until that
 * operation's own next success clears it (`keeper_core::failure::Failures`,
 * R410).
 * {@link useFailureNotices}, mounted once at the app root, subscribes: the
 * current set arrives first — a failure from before this webview listened is
 * in it — then every change. Each failure the mirror has not shown yet is
 * said once in a toast, and the surface's own switch reads the standing
 * sentence ({@link useFailureNotice}). Nothing here decides or clears: the
 * state is Rust's.
 */
import { useEffect } from "react";
import { toast } from "sonner";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { logFailure } from "@/lib/failure";
import {
  type FailureStateVm,
  type FailureSurface,
  failureSubscribe,
  failureUnsubscribe,
} from "@/lib/ipc/client";

export const failureNoticesStore = createStore<FailureStateVm>()(() => ({ notices: [] }));

/** Apply one state from Rust, saying in a toast each failure not shown before. */
function applyFailureState(next: FailureStateVm): void {
  const shown = failureNoticesStore.getState().notices;
  for (const notice of next.notices) {
    const known = shown.some(
      (was) =>
        was.surface === notice.surface &&
        was.account === notice.account &&
        was.sentence === notice.sentence,
    );
    if (!known) {
      toast.error(notice.sentence);
    }
  }
  failureNoticesStore.setState(next, true);
}

/** Mirror Rust's standing background failures for as long as the app is mounted. */
export function useFailureNotices(): void {
  useEffect(() => {
    let stopped = false;
    let subscription: string | undefined;
    void failureSubscribe((state) => {
      if (!stopped) {
        applyFailureState(state);
      }
    })
      .then((id) => {
        if (stopped) {
          void failureUnsubscribe(id).catch(logFailure("failure.unsubscribe"));
        } else {
          subscription = id;
        }
      })
      .catch(logFailure("failure.subscribe"));
    return () => {
      stopped = true;
      if (subscription !== undefined) {
        void failureUnsubscribe(subscription).catch(logFailure("failure.unsubscribe"));
      }
    };
  }, []);
}

/** The standing background failure of `surface` (the first, if several stand), or `null`. */
export function useFailureNotice(surface: FailureSurface): string | null {
  return useStore(
    failureNoticesStore,
    (state) => state.notices.find((notice) => notice.surface === surface)?.sentence ?? null,
  );
}
