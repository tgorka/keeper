/**
 * The "Next session" metadata card (Story 21.5, FR-71/AD-33).
 *
 * Three optional free-text fields — Title, Participants, Note — describing the
 * NEXT Recording Session only: Start consumes them into the session manifest
 * (`meta` + a title-prefixed folder name) and clears the form; "Use previous"
 * re-fills the just-consumed values for the back-to-back-meetings case.
 * Everything stays local (manifest only — zero egress); leaving the card empty
 * changes nothing about the classic session naming.
 */
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useRecordingMeta } from "@/lib/stores/recording-meta";

/** The card's heading (recording voice: sentence case). */
export const META_CARD_TITLE = "Next session";

/** Field labels (recording voice). */
export const META_TITLE_LABEL = "Title";
export const META_PARTICIPANTS_LABEL = "Participants";
export const META_NOTE_LABEL = "Program / session note";

/** The one-click re-fill affordance's label. */
export const META_REFILL_LABEL = "Use previous";

/** Honest scope note: local manifest only, describes only the next session. */
export const META_LOCAL_NOTE =
  "Saved into the recording's local manifest only. Applies to the next Recording Session.";

export function RecordingMetaCard() {
  const fields = useRecordingMeta((s) => s.fields);
  const last = useRecordingMeta((s) => s.last);
  const setFields = useRecordingMeta((s) => s.setFields);
  const refillLast = useRecordingMeta((s) => s.refillLast);

  return (
    <Card size="sm">
      <CardHeader>
        <div className="flex items-center justify-between gap-2">
          <CardTitle>{META_CARD_TITLE}</CardTitle>
          {last !== null && (
            <Button type="button" size="sm" variant="ghost" onClick={refillLast}>
              {META_REFILL_LABEL}
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="recording-meta-title">{META_TITLE_LABEL}</Label>
          <Input
            id="recording-meta-title"
            value={fields.title}
            placeholder="e.g. Weekly sync"
            onChange={(event) => setFields({ title: event.target.value })}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="recording-meta-participants">{META_PARTICIPANTS_LABEL}</Label>
          <Input
            id="recording-meta-participants"
            value={fields.participants}
            placeholder="e.g. Ala, Tomek"
            onChange={(event) => setFields({ participants: event.target.value })}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="recording-meta-note">{META_NOTE_LABEL}</Label>
          <Input
            id="recording-meta-note"
            value={fields.note}
            placeholder="e.g. Zoom demo session"
            onChange={(event) => setFields({ note: event.target.value })}
          />
        </div>
        <p className="text-muted-foreground text-xs">{META_LOCAL_NOTE}</p>
      </CardContent>
    </Card>
  );
}
