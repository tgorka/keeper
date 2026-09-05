/**
 * The spoken turn's stream, observed (Epic 67, Story 67.1, AD-205).
 *
 * When the voice turn hears a question, Rust sends it and Rust reads the
 * answer aloud; the pane is not in that loop. It may still be on screen,
 * though, and a conversation that is growing under a person's eyes must show
 * it — so the shell forwards every {@link BotStreamEvent} of a spoken turn on
 * `keeper://bots-spoken-stream`, and this hook hands each one to `onEvent`,
 * which the panes point at the same `onStreamEvent` their own channel uses.
 * The store then treats the answer exactly as a typed one: `opened` brings
 * the conversation (the target bot's, not whatever was open — AD-206 says
 * the target is chosen under Bots, never inferred from the screen), the
 * deltas append, `closed` replaces the row with what was stored.
 *
 * Capability-gated like {@link useVoiceStream}: where `capabilities.bots` is
 * off nothing is subscribed. Best-effort outside a Tauri webview (jsdom), the
 * hotkey hooks' idiom — a failed subscription leaves the pane exactly as it
 * was, which is the honest state for an environment with no turn to observe.
 */
import { useEffect, useRef } from "react";
import type { BotStreamEvent } from "@/lib/ipc/client";
import { listenSpokenStream } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";

export function useSpokenStream(onEvent: (event: BotStreamEvent) => void): void {
  const bots = useCapabilitiesStore((s) => s.capabilities.bots);
  // The latest handler, so a re-render never re-subscribes and a stale
  // closure never applies an event to a pane that has moved on.
  const handler = useRef(onEvent);
  handler.current = onEvent;

  useEffect(() => {
    if (!bots) {
      return;
    }
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void listenSpokenStream((event) => {
        if (!cancelled) {
          handler.current(event);
        }
      })
        .then((fn) => {
          if (cancelled) {
            fn();
          } else {
            unlisten = fn;
          }
        })
        .catch(() => {
          // No Tauri host — nothing to observe in this environment.
        });
    } catch {
      // `listen` can throw synchronously when the Tauri IPC internals are absent.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [bots]);
}
