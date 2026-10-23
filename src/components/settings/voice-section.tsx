/**
 * Settings › Voice: the wake phrase, the stop word, the language, who a
 * spoken question goes to, and a button to talk now (Story 108.3a,
 * UX-DR-E108-4; the controls are Epics 62–68's).
 *
 * Voice is not a bot feature (AD-179): its controls lived under Settings ›
 * Bots until that surface left keeper, and they keep their keys' values —
 * `voice.wake_enabled`, `voice.wake_phrase`, `voice.locale`,
 * `voice.stop_phrase`, `voice.target` — which `registry` moved from their
 * old names once.
 *
 * # Where the section exists at all
 *
 * On `voice_availability`'s answer alone (AD-179), read here through
 * {@link useVoiceFacts} because Settings is a dialog over whatever is open:
 * unanswered or `unsupported` renders nothing (AD-27), and every other
 * answer renders the section with the refusal's sentence beside the switch.
 * No capability flag gates it, and `CapabilitiesVm` gains none.
 *
 * # The phone's record
 *
 * On the phone tier the section also shows what the voice port did — the
 * ring Rust keeps in memory (Story 65.3, AD-192) — because the phone has no
 * pill or tray to say what its ear is doing.
 */
import { useEffect, useState } from "react";
import { VoiceMic, VoiceStatus } from "@/components/voice/voice-mic";
import { VoiceWake } from "@/components/voice/voice-wake";
import { useVoiceFacts } from "@/hooks/use-voice-facts";
import { useVoiceStream } from "@/hooks/use-voice-stream";
import { logFailure } from "@/lib/failure";
import type { VoiceEventVm } from "@/lib/ipc/client";
import { voiceEvents } from "@/lib/ipc/client";
import { useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { useVoiceStore } from "@/lib/stores/voice";

/** The section heading, so the dialog and its test cannot disagree about it. */
export const VOICE_SECTION_TITLE = "Voice";

/** The standing explanation. */
export const VOICE_SECTION_NOTE =
  "Ask a question aloud and hear the answer. Speech is recognised on this device and never sent to a server.";

/** The phone's record of what the voice port did (Story 65.3, AD-192). */
export const VOICE_EVENTS_TITLE = "What the voice port did";
export const VOICE_EVENTS_NOTE =
  "The last things the voice port did on this phone, newest first. Kept in memory only — nothing is written or sent.";
export const VOICE_EVENTS_EMPTY = "Nothing from the voice port yet.";
export const VOICE_EVENTS_READ_FAILED = "keeper couldn't read the voice port's record.";
/** The accessible name of the list. */
export const VOICE_EVENTS_LABEL = "Voice events";

/** How many of the ring's 200 the phone shows: a screenful of evidence. */
const VOICE_EVENTS_LIMIT = 50;

/**
 * How often the block re-reads while open. A roll is 45 s apart and a
 * refusal is one event; two seconds is live to the eye and cheap to memory.
 */
export const VOICE_EVENTS_REFRESH_MS = 2_000;

/**
 * How long ago an event was, to the second under a minute — a roll every
 * 45 s would read "just now" forever otherwise — and coarser above it. A
 * clock that went backwards reads as now, never as the future.
 */
export function formatVoiceEventAge(atMs: number, now: number = Date.now()): string {
  const elapsedMs = Math.max(0, now - atMs);
  const seconds = Math.floor(elapsedMs / 1000);
  if (seconds < 60) {
    return `${seconds}s ago`;
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes} min ago`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours} hr ago`;
  }
  const days = Math.floor(hours / 24);
  return days === 1 ? "1 day ago" : `${days} days ago`;
}

/**
 * The ring, on the phone: read on open and every {@link VOICE_EVENTS_REFRESH_MS}
 * while open, so a refusal at arming or a roll in the background is on the
 * screen without a reopen. Nothing is streamed and no watcher is opened.
 */
function VoiceEventsBlock({ open }: { open: boolean }) {
  // `undefined` = not read yet; `null` = the read failed; else the ring.
  const [events, setEvents] = useState<VoiceEventVm[] | null | undefined>(undefined);
  // The clock the ages are measured against, moved on every read so a row
  // that sits on the screen keeps ageing.
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!open) {
      return;
    }
    let cancelled = false;
    const read = () => {
      void voiceEvents(VOICE_EVENTS_LIMIT)
        .then((list) => {
          if (!cancelled) {
            setEvents(list);
            setNow(Date.now());
          }
        })
        .catch((raw: unknown) => {
          logFailure("voice.events")(raw);
          if (!cancelled) {
            setEvents(null);
          }
        });
    };
    read();
    const timer = setInterval(read, VOICE_EVENTS_REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [open]);

  return (
    <div className="flex flex-col gap-1 border-border border-t pt-2">
      <p className="font-medium">{VOICE_EVENTS_TITLE}</p>
      <p className="text-muted-foreground text-xs">{VOICE_EVENTS_NOTE}</p>
      {events === null ? (
        <p role="alert" className="text-destructive text-xs">
          {VOICE_EVENTS_READ_FAILED}
        </p>
      ) : events === undefined || events.length === 0 ? (
        <p className="text-muted-foreground text-xs">{VOICE_EVENTS_EMPTY}</p>
      ) : (
        <ul aria-label={VOICE_EVENTS_LABEL} className="flex flex-col gap-1">
          {events.map((event) => (
            <li
              // `seq` is the ring's own monotonic position, never reused: the
              // same kind at the same millisecond is possible, a seq is not.
              key={event.seq}
              className="flex flex-col"
            >
              <div className="flex items-baseline justify-between gap-2">
                <span className="font-mono text-xs">{event.kind}</span>
                <span className="shrink-0 text-muted-foreground text-xs">
                  {formatVoiceEventAge(event.atMs, now)}
                </span>
              </div>
              {event.detail !== null && (
                <span className="break-words text-muted-foreground text-xs">{event.detail}</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** The Voice settings section. */
export function VoiceSection({ open }: { open: boolean }) {
  useVoiceFacts(open);
  // The turn's live state for the talk button and the status line, only while
  // the section is showing (DW-4: no stream outlives its surface).
  useVoiceStream(open);
  const unavailable = useVoiceStore((s) => s.unavailable);
  const reducedPlatform = useIsReducedCapabilityPlatform();

  if (unavailable === undefined || unavailable?.kind === "unsupported") {
    return null;
  }

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="font-medium">{VOICE_SECTION_TITLE}</p>
        <VoiceMic />
      </div>
      <p className="text-muted-foreground">{VOICE_SECTION_NOTE}</p>
      <VoiceStatus />
      <VoiceWake className="pb-2" />
      {reducedPlatform && <VoiceEventsBlock open={open} />}
    </div>
  );
}
