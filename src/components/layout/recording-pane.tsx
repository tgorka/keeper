/**
 * The Recording primary view shell (Story 16.3, ⌘5; permission pre-flight Story
 * 16.5).
 *
 * A single non-chat utility surface living beside Bridges and Settings — no chat
 * list, no timeline, no composer, no live capture state (deferred to 16.6).
 * Story 16.5 adds the honest Screen Recording permission pre-flight above the
 * setup cards: a Permissions card hosting the live-detected tri-state rows
 * (re-detected on focus/return via {@link useRecordingPermission}; Story 20.2
 * adds the Microphone/Camera rows, present only while that source is enabled)
 * and a Start button gated on the grants — disabled with the highest-priority
 * blocking permission named until every required grant is green.
 * The whole surface is capability-gated at the app-shell / sidebar level so it
 * renders only when `recording` is on (desktop macOS ≥ 13.0), never a dead
 * affordance.
 *
 * It reuses the {@link BridgesPane} outer chrome (`<section>`/`<header>`/
 * `<ScrollArea>`) for visual consistency with the other primary views, but — per
 * UX-DR29 — centers its content at content-max-width (`mx-auto w-full
 * max-w-[720px]`, the conversation-pane realization) rather than going full-bleed.
 */
import { useEffect } from "react";
import { RecordingSummaryCard } from "@/components/layout/recording-summary-card";
import { ActiveRecordingBanner } from "@/components/recording/active-recording-banner";
import { RecordingAdvancedControls } from "@/components/recording/recording-advanced-controls";
import { RecordingAudioControls } from "@/components/recording/recording-audio-controls";
import { RecordingDestinationControls } from "@/components/recording/recording-destination-controls";
import { RecordingMetaCard } from "@/components/recording/recording-meta-card";
import {
  CAMERA_PERMISSION_NAME,
  CAMERA_ROW_NOTE,
  MICROPHONE_PERMISSION_NAME,
  MICROPHONE_ROW_NOTE,
  RecordingPermissionRow,
  SCREEN_RECORDING_NOTES,
  SCREEN_RECORDING_PERMISSION_NAME,
} from "@/components/recording/recording-permission-row";
import { RecordingSourcePicker } from "@/components/recording/recording-source-picker";
import { RecordingWebcamControls } from "@/components/recording/recording-webcam-controls";
import { RecordingSettingsControls } from "@/components/settings/recording-settings-controls";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useRecordedSessionSummary } from "@/hooks/use-recorded-session-summary";
import { useRecordingPermission } from "@/hooks/use-recording-permission";
import { isLiveRecording, useRecordingSession } from "@/hooks/use-recording-session";
import { useRecoveredSessions } from "@/hooks/use-recovered-sessions";
import type { RecordingPermissionVm } from "@/lib/ipc/client";
import { systemAudioEnabled } from "@/lib/stores/recording-audio";
import { consumeRecordingMeta } from "@/lib/stores/recording-meta";
import { micDeviceId, micEnabled } from "@/lib/stores/recording-mic";
import { selectedRecordingTarget } from "@/lib/stores/recording-source";
import { cameraDeviceId, webcamEnabled } from "@/lib/stores/recording-webcam";

/** Honest local-only subtitle (recording voice: sentence case, no exclamation
 * marks). Recording adds zero network destinations. */
const RECORDING_SUBTITLE = "Recorded locally. Nothing uploads.";

/** The gated Start affordance's label (recording voice). */
export const START_RECORDING_LABEL = "Start recording";

/** The live-session stop affordance's label (recording voice). */
export const STOP_RECORDING_LABEL = "Stop";

/**
 * The highest-priority blocking permission's name (Story 20.2, FR-67): Screen
 * Recording → Microphone → Camera. A source leg blocks only while enabled
 * (`Some`/non-null) and not granted; `null` when nothing blocks.
 */
export function blockingPermissionName(permission: RecordingPermissionVm): string | null {
  if (permission.screenRecording !== "granted") {
    return SCREEN_RECORDING_PERMISSION_NAME;
  }
  if (permission.microphone != null && permission.microphone !== "granted") {
    return MICROPHONE_PERMISSION_NAME;
  }
  if (permission.camera != null && permission.camera !== "granted") {
    return CAMERA_PERMISSION_NAME;
  }
  return null;
}

/** Names the blocking permission while Start is disabled (FR-67). */
export function startBlockedNote(permissionName: string): string {
  return `Start needs the ${permissionName} permission.`;
}

/** Placeholder copy for each not-yet-built setup card (recording voice). */
const PLACEHOLDER_COPY = "Configured in a later update.";

/** The setup cards this shell reserves. "Source" (Story 19.1 — the live
 * application/window/display picker), "Audio" (Story 19.2 — the system-audio
 * toggle), "Webcam" (Story 20.1, FR-70 — the separate-file camera switch +
 * picker), "Segmenting" (Story 17.5, FR-72 — the shared segment-size +
 * duration-cap control), "Destination" and "Advanced" (Story 19.5 — the folder
 * chooser and the collapsed fps group) are all live. */
const SETUP_CARDS: readonly string[] = [
  "Source",
  "Audio",
  "Webcam",
  "Destination",
  "Segmenting",
  "Advanced",
];

export function RecordingPane() {
  const {
    permission,
    request,
    openSettings,
    requestMicrophone,
    openMicrophoneSettings,
    requestCamera,
    openCameraSettings,
    refresh,
  } = useRecordingPermission();
  const { status, elapsed, start, stop, acknowledge } = useRecordingSession();
  const live = isLiveRecording(status);
  // The completion (finalized) and in-app recovery (recovered) terminals both
  // render the summary card from the on-disk manifest for the session folder
  // (Story 20.3): fetch it once the terminal settles with a folder.
  const isCompletionTerminal = status.state === "finalized" || status.state === "recovered";
  const terminalSummary = useRecordedSessionSummary(
    status.outputPath,
    isCompletionTerminal && status.outputPath !== null,
  );
  // Cross-restart / prior-run orphan notices (Story 20.3): scan disk for
  // unacknowledged `recovered` sessions on the idle/pre-record surface. Re-scan
  // after a session finalizes so a fresh salvage surfaces without a remount.
  const {
    sessions: recoveredSessions,
    refresh: refreshRecovered,
    acknowledge: acknowledgeRecovered,
  } = useRecoveredSessions();
  useEffect(() => {
    if (status.state === "finalized" || status.state === "recovered") {
      refreshRecovered();
    }
  }, [status.state, refreshRecovered]);
  // The disabled-Start note names the highest-priority blocker (Story 20.2):
  // Screen Recording → Microphone → Camera. Both this name and `can_start` are
  // projected from the same three-leg VM, so today they always agree; the
  // `?? SCREEN_RECORDING_PERMISSION_NAME` fallback guarantees a disabled Start is
  // never left with no note (Screen Recording is always required) should the two
  // ever drift — Start must always tell the user what to fix.
  const blockedBy = permission.canStart
    ? null
    : (blockingPermissionName(permission) ?? SCREEN_RECORDING_PERMISSION_NAME);

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
                // Story 20.1: thread the Webcam card's camera selection
                // (default off; device null = system default camera) the
                // same way — off ships no camera fields at all.
                void start(
                  selectedRecordingTarget(),
                  systemAudioEnabled(),
                  micEnabled(),
                  micDeviceId(),
                  webcamEnabled(),
                  cameraDeviceId(),
                  // Story 21.5: consume the Next-session fields (clears the
                  // form; an untouched form ships no meta at all).
                  consumeRecordingMeta(),
                );
              }}
            >
              {START_RECORDING_LABEL}
            </Button>
          )}
          {blockedBy !== null && !live && (
            <p className="text-muted-foreground text-xs">{startBlockedNote(blockedBy)}</p>
          )}
          {/* The finalized outcome moved from a header one-liner (Story 16.6)
              into the completion Card in the scrolling body below (Story 20.3):
              segment count · size, the folder in mono, and Reveal in Finder. */}
          {/* The failed note lives in the banner's error variant now (Story
              18.4) — a single failure surface, mirroring 18.3's header→banner
              consolidation. */}
        </div>
      </header>

      {/* The in-app active-recording banner + segment meter (Story 18.3):
          pinned between the header and the scrolling body, persistent while
          live, and a pure renderer of the enriched Rust snapshot. Story 18.4:
          on `failed` + `error` it renders the filled recording-red error
          variant with Restart (re-invokes Start with the current capture
          selections) and Dismiss (→ `recording_acknowledge`); it renders `null`
          on any other terminal/idle state. */}
      <ActiveRecordingBanner
        status={status}
        elapsed={elapsed}
        onStop={() => {
          void stop();
        }}
        onRestart={() => {
          // Story 18.4 Restart: re-invoke Start with the CURRENT capture
          // selections, read from the same module-level stores the Start button
          // uses. Reading the stores (not a per-mount hook ref) keeps Restart
          // honest across a view remount — closing/reopening the Recording view
          // resets the hook's local state but never the stores, so Restart can
          // never silently revert a chosen app/mic/webcam/display to the
          // defaults.
          void start(
            selectedRecordingTarget(),
            systemAudioEnabled(),
            micEnabled(),
            micDeviceId(),
            webcamEnabled(),
            cameraDeviceId(),
            consumeRecordingMeta(),
          );
        }}
        onDismiss={() => {
          void acknowledge();
        }}
      />

      <ScrollArea className="min-h-0 flex-1">
        {/* Centered single column at content-max-width (UX-DR29), not a full-bleed
            body — unlike the Bridges pane. */}
        <div className="mx-auto flex w-full max-w-[720px] flex-col gap-6 p-6">
          {/* The completion / in-app-recovery card (Story 20.3, FR-71/FR-73):
              a finalized session renders the plain completion card; the in-app
              `recovered` terminal renders the same shape with a warning edge.
              N/size come from the on-disk manifest (never `segmentsClosed`);
              the card degrades to folder + Reveal when the summary is
              unavailable. */}
          {isCompletionTerminal && status.outputPath !== null && (
            <RecordingSummaryCard
              variant={status.state === "recovered" ? "recovered" : "completion"}
              sessionFolder={status.outputPath}
              title={terminalSummary?.title ?? null}
              screenSegmentCount={terminalSummary?.screenSegmentCount ?? null}
              totalBytes={terminalSummary?.totalBytes ?? null}
            />
          )}
          {/* Cross-restart / prior-run orphan notices (Story 20.3, FR-73): one
              warning-edged recovery card per unacknowledged `recovered` session
              on disk, each independently dismissable (dismiss latches the
              one-time notice). Hidden entirely while a session is live, and the
              current in-app `recovered` terminal (already shown above) is
              filtered out so a just-salvaged session never double-renders. */}
          {!live &&
            recoveredSessions
              .filter((session) => session.sessionFolder !== status.outputPath)
              .map((session) => (
                <RecordingSummaryCard
                  key={session.sessionFolder}
                  variant="recovered"
                  sessionFolder={session.sessionFolder}
                  title={session.title}
                  screenSegmentCount={session.screenSegmentCount}
                  totalBytes={session.totalBytes}
                  onDismiss={() => {
                    acknowledgeRecovered(session.sessionFolder);
                  }}
                />
              ))}
          {/* The Next-session metadata card (Story 21.5): optional Title /
              Participants / Note for the NEXT session, consumed (and cleared)
              by Start into the local manifest; "Use previous" re-fills. */}
          <RecordingMetaCard />
          {/* The permission pre-flight (Story 16.5; mic/camera rows Story 20.2)
              sits above the setup cards: live-detected at render, re-detected
              on focus/return and on every enabled-source change. The Microphone
              and Camera rows render only while that source is enabled (their
              VM legs are non-null) — an absent row is a disabled source, never
              a hidden blocker. */}
          <Card size="sm">
            <CardHeader>
              <CardTitle>Permissions</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-5">
              <RecordingPermissionRow
                name={SCREEN_RECORDING_PERMISSION_NAME}
                access={permission.screenRecording}
                notes={SCREEN_RECORDING_NOTES}
                onRequest={() => {
                  void request();
                }}
                onOpenSettings={openSettings}
              />
              {permission.microphone != null && (
                <RecordingPermissionRow
                  name={MICROPHONE_PERMISSION_NAME}
                  access={permission.microphone}
                  notes={[MICROPHONE_ROW_NOTE]}
                  onRequest={() => {
                    void requestMicrophone();
                  }}
                  onOpenSettings={openMicrophoneSettings}
                />
              )}
              {permission.camera != null && (
                <RecordingPermissionRow
                  name={CAMERA_PERMISSION_NAME}
                  access={permission.camera}
                  notes={[CAMERA_ROW_NOTE]}
                  onRequest={() => {
                    void requestCamera();
                  }}
                  onOpenSettings={openCameraSettings}
                />
              )}
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
                  <RecordingAudioControls active={!live} onPermissionSettled={refresh} />
                </CardContent>
              </Card>
            ) : title === "Webcam" ? (
              // The live Webcam card (Story 20.1, FR-70): the separate-file
              // camera Switch (default off) + the flat camera picker; the
              // lazy Camera-TCC request fires only on enable. `active`
              // freezes pre-Start reconciliation while a session is live.
              <Card key={title} size="sm">
                <CardHeader>
                  <CardTitle>{title}</CardTitle>
                </CardHeader>
                <CardContent>
                  <RecordingWebcamControls active={!live} onPermissionSettled={refresh} />
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
            ) : title === "Destination" ? (
              // The live Destination folder chooser (Story 19.5): the same
              // shared control Settings → Recording mounts — the effective
              // folder, the native dir picker, next-session scope.
              <Card key={title} size="sm">
                <CardHeader>
                  <CardTitle>{title}</CardTitle>
                </CardHeader>
                <CardContent>
                  <RecordingDestinationControls />
                </CardContent>
              </Card>
            ) : title === "Advanced" ? (
              // The collapsed Advanced group (Story 19.5): fps 30/60 behind a
              // hand-rolled disclosure, shared with Settings → Recording.
              <Card key={title} size="sm">
                <CardHeader>
                  <CardTitle>{title}</CardTitle>
                </CardHeader>
                <CardContent>
                  <RecordingAdvancedControls />
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
