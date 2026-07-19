/**
 * The Recording primary view shell (Story 16.3, ⌘5; permission pre-flight Story
 * 16.5).
 *
 * A single non-chat utility surface living beside Bridges and Settings — no chat
 * list, no timeline, no composer, no live capture state (deferred to 16.6).
 * Story 16.5 adds the honest Screen Recording permission pre-flight above the
 * setup cards: a Permissions card hosting the live-detected tri-state row
 * (re-detected on focus/return via {@link useRecordingPermission}) and a Start
 * button gated on the grant — disabled with the blocking permission named until
 * it is green. Start's click is an inert placeholder; capture wiring is 16.6.
 * The whole surface is capability-gated at the app-shell / sidebar level so it
 * renders only when `recording` is on (desktop macOS ≥ 13.0), never a dead
 * affordance.
 *
 * It reuses the {@link BridgesPane} outer chrome (`<section>`/`<header>`/
 * `<ScrollArea>`) for visual consistency with the other primary views, but — per
 * UX-DR29 — centers its content at content-max-width (`mx-auto w-full
 * max-w-[720px]`, the conversation-pane realization) rather than going full-bleed.
 */
import { ActiveRecordingBanner } from "@/components/recording/active-recording-banner";
import { RecordingAudioControls } from "@/components/recording/recording-audio-controls";
import {
  RecordingPermissionRow,
  SCREEN_RECORDING_PERMISSION_NAME,
} from "@/components/recording/recording-permission-row";
import { RecordingSourcePicker } from "@/components/recording/recording-source-picker";
import { RecordingSettingsControls } from "@/components/settings/recording-settings-controls";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useRecordingPermission } from "@/hooks/use-recording-permission";
import { isLiveRecording, useRecordingSession } from "@/hooks/use-recording-session";
import { systemAudioEnabled } from "@/lib/stores/recording-audio";
import { micDeviceId, micEnabled } from "@/lib/stores/recording-mic";
import { selectedRecordingTarget } from "@/lib/stores/recording-source";

/** Honest local-only subtitle (recording voice: sentence case, no exclamation
 * marks). Recording adds zero network destinations. */
const RECORDING_SUBTITLE = "Recorded locally. Nothing uploads.";

/** The gated Start affordance's label (recording voice). */
export const START_RECORDING_LABEL = "Start recording";

/** The live-session stop affordance's label (recording voice). */
export const STOP_RECORDING_LABEL = "Stop";

/** The finalized-outcome note prefix (the saved file's path follows). */
export const FINALIZED_NOTE_PREFIX = "Saved to";

/** Names the blocking permission while Start is disabled (FR-67). */
export const START_BLOCKED_NOTE = `Start needs the ${SCREEN_RECORDING_PERMISSION_NAME} permission.`;

/** Placeholder copy for each not-yet-built setup card (recording voice). */
const PLACEHOLDER_COPY = "Configured in a later update.";

/** The setup cards this shell reserves. "Source" (Story 19.1 — the live
 * application/window/display picker), "Audio" (Story 19.2 — the system-audio
 * toggle), and "Segmenting" (Story 17.5, FR-72 — the shared segment-size +
 * duration-cap control) are live; the rest are later-story surfaces
 * ("Destination" is Story 19.5). */
const SETUP_CARDS: readonly string[] = [
  "Source",
  "Audio",
  "Webcam",
  "Destination",
  "Segmenting",
  "Advanced",
];

export function RecordingPane() {
  const { permission, request, openSettings } = useRecordingPermission();
  const { status, elapsed, start, stop } = useRecordingSession();
  const live = isLiveRecording(status);

  return (
    <section
      aria-label="Recording"
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background"
    >
      <header className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-6 py-4">
        <div className="min-w-0">
          <h1 className="font-heading font-medium text-lg">Recording</h1>
          <p className="text-muted-foreground text-sm">{RECORDING_SUBTITLE}</p>
        </div>
        <div className="flex shrink-0 flex-col items-end gap-1">
          {/* The live record dot / ticking elapsed / Stop cluster now lives in
              the pinned banner below the header (Story 18.3) — the header keeps
              only the idle Start affordance and the terminal notes. */}
          {!live && (
            <Button
              type="button"
              disabled={!permission.canStart}
              onClick={() => {
                // Story 19.1: start the session for the picker's selected target
                // (a display or an application; the main display by default).
                // Story 19.2: thread the Audio card's system-audio toggle
                // (default on) read imperatively at click time.
                // Story 19.3: thread the Audio card's mic selection (default
                // off; device null = system default input) the same way.
                void start(
                  selectedRecordingTarget(),
                  systemAudioEnabled(),
                  micEnabled(),
                  micDeviceId(),
                );
              }}
            >
              {START_RECORDING_LABEL}
            </Button>
          )}
          {!permission.canStart && !live && (
            <p className="text-muted-foreground text-xs">{START_BLOCKED_NOTE}</p>
          )}
          {status.state === "finalized" && status.outputPath !== null && (
            <p className="text-muted-foreground text-xs" role="status">
              {FINALIZED_NOTE_PREFIX} <span className="font-mono">{status.outputPath}</span>
            </p>
          )}
          {status.state === "failed" && (
            <p className="text-held text-xs" role="alert">
              Recording failed: {status.error ?? "unknown error"}
            </p>
          )}
        </div>
      </header>

      {/* The in-app active-recording banner + segment meter (Story 18.3):
          pinned between the header and the scrolling body, persistent while
          live, and a pure renderer of the enriched Rust snapshot. It renders
          `null` on any terminal/idle state. */}
      <ActiveRecordingBanner
        status={status}
        elapsed={elapsed}
        onStop={() => {
          void stop();
        }}
      />

      <ScrollArea className="min-h-0 flex-1">
        {/* Centered single column at content-max-width (UX-DR29), not a full-bleed
            body — unlike the Bridges pane. */}
        <div className="mx-auto flex w-full max-w-[720px] flex-col gap-6 p-6">
          {/* The permission pre-flight (Story 16.5) sits above the setup cards:
              live-detected at render, re-detected on focus/return. */}
          <Card size="sm">
            <CardHeader>
              <CardTitle>Permissions</CardTitle>
            </CardHeader>
            <CardContent>
              <RecordingPermissionRow
                access={permission.screenRecording}
                onRequest={() => {
                  void request();
                }}
                onOpenSettings={openSettings}
              />
            </CardContent>
          </Card>

          {SETUP_CARDS.map((title) =>
            title === "Source" ? (
              // The live source picker (Story 19.1): displays + applications,
              // polled ~3s while idle, single-select, app-scope disclosed.
              <Card key={title} size="sm">
                <CardHeader>
                  <CardTitle>{title}</CardTitle>
                </CardHeader>
                <CardContent>
                  {/* Pause the ~3s source poll while recording (Story 19.1):
                      the setup cards stay mounted during a live session, so the
                      picker must stop spawning enumeration children. */}
                  <RecordingSourcePicker active={!live} />
                </CardContent>
              </Card>
            ) : title === "Audio" ? (
              // The live Audio card (Story 19.2): the system-audio Switch,
              // default on, with the content-audio label and disclosure.
              <Card key={title} size="sm">
                <CardHeader>
                  <CardTitle>{title}</CardTitle>
                </CardHeader>
                <CardContent>
                  <RecordingAudioControls active={!live} />
                </CardContent>
              </Card>
            ) : title === "Segmenting" ? (
              // The live pre-record segmentation controls (Story 17.5): the
              // same shared control Settings → Recording mounts, bound to one
              // store so the two surfaces mirror each other.
              <Card key={title} size="sm">
                <CardHeader>
                  <CardTitle>{title}</CardTitle>
                </CardHeader>
                <CardContent>
                  <RecordingSettingsControls />
                </CardContent>
              </Card>
            ) : (
              <Card key={title} size="sm">
                <CardHeader>
                  <CardTitle>{title}</CardTitle>
                  <p className="text-muted-foreground text-sm">{PLACEHOLDER_COPY}</p>
                </CardHeader>
              </Card>
            ),
          )}
        </div>
      </ScrollArea>
    </section>
  );
}
