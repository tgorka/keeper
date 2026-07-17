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
import {
  RecordingPermissionRow,
  SCREEN_RECORDING_PERMISSION_NAME,
} from "@/components/recording/recording-permission-row";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useRecordingPermission } from "@/hooks/use-recording-permission";

/** Honest local-only subtitle (recording voice: sentence case, no exclamation
 * marks). Recording adds zero network destinations. */
const RECORDING_SUBTITLE = "Recorded locally. Nothing uploads.";

/** The gated Start affordance's label (recording voice). */
export const START_RECORDING_LABEL = "Start recording";

/** Names the blocking permission while Start is disabled (FR-67). */
export const START_BLOCKED_NOTE = `Start needs the ${SCREEN_RECORDING_PERMISSION_NAME} permission.`;

/** Placeholder copy for each not-yet-built setup card (recording voice). */
const PLACEHOLDER_COPY = "Configured in a later update.";

/** The setup cards this shell reserves, each a later-story surface. */
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
          <Button
            type="button"
            disabled={!permission.canStart}
            onClick={() => {
              // Inert placeholder — capture wiring lands in Story 16.6.
            }}
          >
            {START_RECORDING_LABEL}
          </Button>
          {!permission.canStart && (
            <p className="text-muted-foreground text-xs">{START_BLOCKED_NOTE}</p>
          )}
        </div>
      </header>

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

          {SETUP_CARDS.map((title) => (
            <Card key={title} size="sm">
              <CardHeader>
                <CardTitle>{title}</CardTitle>
                <p className="text-muted-foreground text-sm">{PLACEHOLDER_COPY}</p>
              </CardHeader>
            </Card>
          ))}
        </div>
      </ScrollArea>
    </section>
  );
}
