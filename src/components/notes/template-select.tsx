/**
 * The one template chooser (Story 44.7, generalised by Story 45.16).
 *
 * Two surfaces now pick a template — a space's "New notes start from" and
 * Settings → Quick capture's "Captures start from" — and the rule that makes
 * this control honest is subtle enough that having it twice would mean having
 * it once and having a bug once.
 *
 * **A `<select>` whose value matches no option renders the FIRST one.** Here
 * that first option reads "No template", so a stored template the list does not
 * contain would make the control claim a setting the file does not have — and
 * the next save would make that claim true. So the stored value always gets an
 * option of its own when it is unlisted, and the control shows what is stored
 * rather than what the list happens to know about.
 *
 * **Unlisted and missing are two states, not one.** A list that has not arrived
 * — or one whose read failed — tells exactly the same lie as a template that
 * has been deleted, and only the second is evidence of anything. So the extra
 * option is keyed on `unlisted` and the red sentence waits for `loaded`. An
 * empty list from a failed read must never put a warning under a perfectly good
 * setting and invite the user to clear it.
 *
 * **Nothing here rewrites what is on disk.** An unlisted template stays
 * selected and stays stored; a list shrinking is not a licence to edit somebody
 * else's vault.
 */
import type { NoteTemplateVm } from "@/lib/ipc/client";

/**
 * What the chooser calls "hand out nothing".
 *
 * A sentinel `""` rather than a `null` option value, because a `<select>`'s
 * value is always a string and a `null` would arrive back as the literal text
 * `"null"` — which is a path, and one keeper would then go looking for.
 */
export const NO_TEMPLATE = "";

export interface TemplateSelectProps {
  /** The `<label>`'s `htmlFor` target. */
  id: string;
  /** The stored path, verbatim. {@link NO_TEMPLATE} means none. */
  value: string;
  /** The vault's templates, as `notes_templates` listed them. */
  choices: readonly NoteTemplateVm[];
  /**
   * Whether `choices` is an ANSWER rather than the absence of one. A vault with
   * genuinely no templates and a read that failed both arrive as an empty
   * array, and only the first is evidence a stored template is gone.
   */
  loaded: boolean;
  onChange: (path: string) => void;
  /** The sentence for a stored template keeper knows is not in the vault. */
  missingSentence: string;
  /** The standing explanation, shown whenever there is nothing wrong. */
  note: string;
}

export function TemplateSelect({
  id,
  value,
  choices,
  loaded,
  onChange,
  missingSentence,
  note,
}: TemplateSelectProps) {
  const unlisted = value !== NO_TEMPLATE && !choices.some((choice) => choice.path === value);
  const missing = unlisted && loaded;
  return (
    <>
      <select
        id={id}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="h-9 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <option value={NO_TEMPLATE}>No template</option>
        {choices.map((choice) => (
          <option key={choice.path} value={choice.path}>
            {choice.name}
          </option>
        ))}
        {unlisted && (
          <option value={value}>{missing ? `${value} — not in this vault` : value}</option>
        )}
      </select>
      {missing ? (
        <p data-slot="template-missing" className="text-destructive text-sm">
          {missingSentence}
        </p>
      ) : (
        <p className="text-muted-foreground text-sm">{note}</p>
      )}
    </>
  );
}
