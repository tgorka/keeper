/**
 * The Recording primary view shell (Story 16.3, ⌘5).
 *
 * A single non-chat utility surface living beside Bridges and Settings — no chat
 * list, no timeline, no composer, no live capture state (all deferred to 16.6).
 * This story ships only the empty shell: a centered card stack of setup
 * placeholders (Source / Audio / Webcam / Destination / Segmenting / Advanced),
 * each configured in a later update. The whole surface is capability-gated at the
 * app-shell / sidebar level so it renders only when `recording` is on (desktop
 * macOS ≥ 13.0), never a dead affordance.
 *
 * It reuses the {@link BridgesPane} outer chrome (`<section>`/`<header>`/
 * `<ScrollArea>`) for visual consistency with the other primary views, but — per
 * UX-DR29 — centers its content at content-max-width (`mx-auto w-full
 * max-w-[720px]`, the conversation-pane realization) rather than going full-bleed.
 */
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";

/** Honest local-only subtitle (recording voice: sentence case, no exclamation
 * marks). Recording adds zero network destinations. */
const RECORDING_SUBTITLE = "Recorded locally. Nothing uploads.";

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
  return (
    <section
      aria-label="Recording"
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background"
    >
      <header className="shrink-0 border-border border-b px-6 py-4">
        <h1 className="font-heading font-medium text-lg">Recording</h1>
        <p className="text-muted-foreground text-sm">{RECORDING_SUBTITLE}</p>
      </header>

      <ScrollArea className="min-h-0 flex-1">
        {/* Centered single column at content-max-width (UX-DR29), not a full-bleed
            body — unlike the Bridges pane. */}
        <div className="mx-auto flex w-full max-w-[720px] flex-col gap-6 p-6">
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
