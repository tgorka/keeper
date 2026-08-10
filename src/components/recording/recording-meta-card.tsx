/**
 * The "Next session" metadata card (Story 21.5, FR-71/AD-33).
 *
 * The five optional fields — Title, Participants, Note, Tags, custom rows —
 * describing the NEXT Recording Session only: Start consumes them into the
 * session manifest (`meta` + a title-prefixed folder name) and clears the form;
 * "Use previous" re-fills the just-consumed values for the back-to-back-meetings
 * case. Everything stays local (manifest only — zero egress); leaving the card
 * empty changes nothing about the classic session naming.
 *
 * The fields themselves live in {@link RecordingMetaFieldSet} since Story 45.19,
 * because the editor on the FINISHED session collects the same five and a
 * second rendering of them would drift from this one field by field.
 */
import { RecordingMetaFieldSet } from "@/components/recording/recording-meta-fields";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useRecordingMeta } from "@/lib/stores/recording-meta";

/** The card's heading (recording voice: sentence case). */
export const META_CARD_TITLE = "Next session";

/** The one-click re-fill affordance's label. */
export const META_REFILL_LABEL = "Use previous";

/** Honest scope note: local manifest only, describes only the next session. */
export const META_LOCAL_NOTE =
  "Saved into the recording's local manifest only. Applies to the next Recording Session.";

/** The id prefix this host mints its fields under. Unchanged from Story 21.5
 *  so the pre-Start form's element ids are the ones they have always been. */
const META_FIELD_PREFIX = "recording-meta";

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
        <RecordingMetaFieldSet fields={fields} onChange={setFields} idPrefix={META_FIELD_PREFIX} />
        <p className="text-muted-foreground text-xs">{META_LOCAL_NOTE}</p>
      </CardContent>
    </Card>
  );
}
