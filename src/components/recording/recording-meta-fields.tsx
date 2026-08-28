/**
 * The "Next session" field set (Story 21.5, 22.3, 42.5; extracted by Story
 * 45.19, FR-197).
 *
 * Title / Participants / Program note / Tags / repeatable custom rows — the
 * five fields a Recording Session's `manifest.json` carries, rendered once and
 * mounted by both surfaces that collect them:
 *
 * - {@link RecordingMetaCard}, bound to the ephemeral pre-Start store, for the
 *   session about to happen;
 * - the details editor on the completion card, bound to a local draft, for the
 *   session that just finished.
 *
 * **The extraction is the point, not a tidy-up.** Before it, the field set
 * existed only inside the pre-Start card, so the editor on the last recording
 * would have been a second rendering of the same five fields — and a field
 * added to one of them (as `tags` and `custom` were, two stories apart) reaches
 * the user on whichever surface the author happened to be editing. A branch
 * reachable only from a second host is a branch nobody tests.
 *
 * `idPrefix` exists because both hosts can be on screen at once: element ids
 * must be unique per document or a `<label for>` points at whichever input the
 * browser found first, which is the one the user is not looking at.
 */
import { Plus, X } from "lucide-react";
import { TagVocabularyInput } from "@/components/tags/tag-vocabulary-input";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { RecordingMetaFields } from "@/lib/stores/recording-meta";

/** Field labels (recording voice). */
export const META_TITLE_LABEL = "Title";
export const META_PARTICIPANTS_LABEL = "Participants";
export const META_NOTE_LABEL = "Program / session note";

/** The tags field label (Story 22.3). The user types comma-separated text;
 *  Story 42.5 moved the decision about what that text MEANS into Rust, and
 *  gave the field completion over the shared tag vocabulary. */
export const META_TAGS_LABEL = "Tags";

/** The add-custom-field affordance's label (Story 22.3). */
export const META_ADD_FIELD_LABEL = "Add field";

/** The custom row's two labels, repeated per row. */
export const META_CUSTOM_NAME_LABEL = "Name";
export const META_CUSTOM_VALUE_LABEL = "Value";

export interface RecordingMetaFieldSetProps {
  /** The values to render. */
  fields: RecordingMetaFields;
  /** Receives one or more changed fields; the host owns where they land. */
  onChange: (patch: Partial<RecordingMetaFields>) => void;
  /**
   * Prefix for every element id this set mints, so two mounted sets never
   * collide. Also prefixes the custom rows' remove-button accessible names,
   * which are otherwise identical between the two hosts.
   */
  idPrefix: string;
  /** Freeze every control while a save is in flight. */
  disabled?: boolean;
}

export function RecordingMetaFieldSet({
  fields,
  onChange,
  idPrefix,
  disabled = false,
}: RecordingMetaFieldSetProps) {
  return (
    <>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${idPrefix}-title`}>{META_TITLE_LABEL}</Label>
        <Input
          id={`${idPrefix}-title`}
          value={fields.title}
          disabled={disabled}
          placeholder="e.g. Weekly sync"
          onChange={(event) => onChange({ title: event.target.value })}
        />
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${idPrefix}-participants`}>{META_PARTICIPANTS_LABEL}</Label>
        <Input
          id={`${idPrefix}-participants`}
          value={fields.participants}
          disabled={disabled}
          placeholder="e.g. Ala, Tomek"
          onChange={(event) => onChange({ participants: event.target.value })}
        />
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${idPrefix}-note`}>{META_NOTE_LABEL}</Label>
        <Input
          id={`${idPrefix}-note`}
          value={fields.note}
          disabled={disabled}
          placeholder="e.g. Zoom demo session"
          onChange={(event) => onChange({ note: event.target.value })}
        />
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${idPrefix}-tags`}>{META_TAGS_LABEL}</Label>
        {/* Story 42.5: completion over the ONE tag vocabulary — the same
            paths the notes tag tree is built from, so a tag that exists only
            on notes is offered here. The field still takes comma-separated
            text, but it no longer decides what a tag is: the raw string goes
            to Rust, which splits and normalises it in the one place that
            rule lives. */}
        <TagVocabularyInput
          id={`${idPrefix}-tags`}
          value={fields.tags}
          disabled={disabled}
          placeholder="e.g. standup, q3, demo (comma-separated)"
          onChange={(tags) => onChange({ tags })}
        />
      </div>
      {/* Story 22.3: repeatable custom name/value rows. Rows with a blank
          name are dropped by Rust on save; the X removes a row immediately. */}
      {fields.custom.map((row, index) => (
        <div
          // Index keying is safe: rows are positional edit slots, never
          // reordered.
          // biome-ignore lint/suspicious/noArrayIndexKey: positional slots
          key={index}
          className="flex items-end gap-2"
        >
          <div className="flex min-w-0 flex-1 flex-col gap-1.5">
            <Label htmlFor={`${idPrefix}-custom-name-${index}`}>{META_CUSTOM_NAME_LABEL}</Label>
            <Input
              id={`${idPrefix}-custom-name-${index}`}
              value={row.name}
              disabled={disabled}
              placeholder="e.g. Ticket"
              onChange={(event) => {
                const custom = fields.custom.map((r, i) =>
                  i === index ? { ...r, name: event.target.value } : r,
                );
                onChange({ custom });
              }}
            />
          </div>
          <div className="flex min-w-0 flex-1 flex-col gap-1.5">
            <Label htmlFor={`${idPrefix}-custom-value-${index}`}>{META_CUSTOM_VALUE_LABEL}</Label>
            <Input
              id={`${idPrefix}-custom-value-${index}`}
              value={row.value}
              disabled={disabled}
              placeholder="e.g. KPR-123"
              onChange={(event) => {
                const custom = fields.custom.map((r, i) =>
                  i === index ? { ...r, value: event.target.value } : r,
                );
                onChange({ custom });
              }}
            />
          </div>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            disabled={disabled}
            aria-label={`Remove field ${index + 1}`}
            onClick={() => {
              onChange({ custom: fields.custom.filter((_, i) => i !== index) });
            }}
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>
      ))}
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className="w-fit gap-1"
        disabled={disabled}
        onClick={() => {
          onChange({ custom: [...fields.custom, { name: "", value: "" }] });
        }}
      >
        <Plus className="size-4" aria-hidden="true" />
        {META_ADD_FIELD_LABEL}
      </Button>
    </>
  );
}
