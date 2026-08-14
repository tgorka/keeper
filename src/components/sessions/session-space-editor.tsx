/**
 * The session space editor (FR-261, AD-121).
 *
 * {@link "@/components/notes/space-editor"}'s twin for a zone's `_spaces/`, and
 * deliberately a twin rather than a generalisation of it. The two objects are
 * the same *kind* of thing — a name, an icon from a fixed set, a sort, a rail
 * position, and a stored query shown as three-state chips — and they differ in
 * three facts that reach every branch of the form: a session space has no
 * `limit` and no `template`, its id is a path rather than a ULID, and its tag
 * vocabulary is one session's own rather than a vault index's. Threading four
 * "which kind am I" forks through an 850-line form is the shape that gets one
 * arm changed and the other forgotten.
 *
 * What is shared is shared for real, not copied: the chips, the icon buttons and
 * the frozen-term rows are the notes editor's own components, imported; the
 * query is decomposed by `notesSpaceTerms`, which is pure and vault-free, so
 * this surface reaches the same parser and there is no second reading of `tag:`
 * in TypeScript (AD-20, AD-58); and the sentences the two forms share are that
 * file's constants rather than retyped near-copies.
 *
 * **keeper does not rewrite a term it could not read.** A `_spaces/*.md` file is
 * markdown in a synced zone that an agent or a person may have written by hand,
 * and the DSL says far more than a chip bar can. When Rust reports even one term
 * the chips cannot hold, this shows the query read-only and saves the stored
 * text back byte for byte (FR-121); the name, icon, sort and position stay
 * editable, because refusing the whole form would send someone to hand-edit
 * frontmatter just to rename a space.
 *
 * **A space with no terms refuses to save**, for the reason an empty query is an
 * error in `sessions::spaces::select` rather than a match-everything: a saved
 * view that silently widens to the whole session is how a bulk action becomes a
 * data-loss story.
 */
import { useEffect, useId, useMemo, useState } from "react";
import { TagFilterChip } from "@/components/notes/note-filter-bar";
import {
  FixedTerm,
  IconChoice,
  SPACE_ICON_NO_MATCH,
  SPACE_NO_NAME,
  SPACE_NO_TERMS,
  SPACE_TERMS_READONLY,
} from "@/components/notes/space-editor";
import { matchSpaceIcons, type SpaceIconGroup } from "@/components/notes/space-icons";
import { TagCombobox } from "@/components/notes/tag-combobox";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { spaceQueryText } from "@/hooks/use-notes-actions";
import type { NoteSpaceFieldVm, SessionSpaceVm } from "@/lib/ipc/client";
import { notesSpaceTerms, sessionsSpaceSave } from "@/lib/ipc/client";
import {
  nextTagChipState,
  type TagChip,
  tagChipState,
  withTagTerm,
} from "@/lib/stores/notes-filters";
import { cn } from "@/lib/utils";

/** How many icons the chooser shows before it needs its own scroll region. */
const ICON_GRID_MAX_HEIGHT = "max-h-56";

/** The dialog's title when an existing space is being rewritten. */
export const SESSION_SPACE_EDIT_TITLE = "Edit space";

/** The dialog's title when one is being made. */
export const SESSION_SPACE_NEW_TITLE = "New space";

/**
 * What the dialog says it is for.
 *
 * It names the zone rather than the session, because that is the surprise: a
 * space definition lives in `60-sessions/_spaces/` and every session in the root
 * is read through it, so narrowing this query narrows all of them. Someone who
 * assumes they are editing this one session's view would otherwise discover the
 * scope by breaking last month's.
 */
export const SESSION_SPACE_SCOPE_HINT =
  "A space is a saved query over a session's markdown files. It belongs to the whole sessions zone, so every session is read through it.";

/**
 * The sentence over the terms of a space whose query does not parse.
 *
 * A near-copy of the notes editor's, and the one clause that differs is why it
 * is not imported: over there the repair happens "in the note itself", and a
 * sessions zone has no notes — the thing to open is a file in `_spaces/`. A
 * shared constant would have to say "note or file", which is the voice of
 * software that does not know what it is holding.
 */
export const SESSION_SPACE_TERMS_BROKEN =
  "This space's query can't be read, so keeper won't rewrite it. Fix it in the file itself; the name, icon, sort and position are still yours to change.";

/** What the form says when the write failed. */
export const SESSION_SPACE_SAVE_FAILED = "keeper couldn't save this space. Nothing was changed.";

/**
 * The facts a session space can order by, in `keeper-core::notes::sort`'s own
 * words (FR-158) — the same four keys a note space offers, minus `recorded`.
 *
 * `recorded` is dropped rather than inherited: it asks when the recording a note
 * came from was made, and a session's markdown pool holds no recordings at all,
 * so every file would fall to the same fallback branch and the option would be
 * `created` wearing another name. A control that quietly does something else is
 * worse than one control fewer. A file that nonetheless *stores* `recorded` is
 * still shown it — see the unlisted-key option in the markup below.
 */
export const SESSION_SPACE_SORT_KEYS: readonly { key: string; label: string }[] = [
  { key: "order", label: "Order" },
  { key: "name", label: "Name" },
  { key: "created", label: "Created" },
  { key: "modified", label: "Modified" },
];

/** What each direction is called, per key — "ascending" is a word about the
 *  machine, and for `name` the question has different words entirely. */
const SORT_DIR_LABELS: Readonly<Record<string, readonly [string, string]>> = {
  order: ["Lowest first", "Highest first"],
  name: ["A to Z", "Z to A"],
  created: ["Oldest first", "Newest first"],
  modified: ["Oldest first", "Newest first"],
};

/**
 * The line under the sort control, for the one key whose behaviour its name does
 * not give away. Keyed by the canonical `<key> <dir>`.
 *
 * Worded for files rather than notes: `order` promises a manual ordering while
 * most of a session's files have never been given one — the task board writes
 * `keeper.order` on the cards it moves and nothing else does. A reader not told
 * that concludes the sort is broken rather than that the pool is unordered.
 */
export const SESSION_SPACE_SORT_NOTES: Readonly<Record<string, string>> = {
  "order asc":
    "Each file's own position, lowest first. Most files have none, so they fall in alphabetically after the ones that do.",
  "order desc":
    "Each file's own position, highest first. Most files have none, so they fall in alphabetically after the ones that do.",
};

/** The editable half of a space's query, once the chips can hold all of it. */
interface Draft {
  tags: readonly TagChip[];
  flags: readonly string[];
  origin: string | null;
  text: string | null;
  /**
   * `field:key=value` / `field:key!=value` terms, in written order. A list
   * rather than one slot, because two field terms are two questions — and the
   * task board asks `status` while nothing else does.
   */
  fields: readonly NoteSpaceFieldVm[];
}

/**
 * What the editor knows about the space's query. There is no fourth state in
 * which some terms are chips and some are not — `NoteSpaceTermsVm` is a
 * two-variant enum for exactly that reason (FR-149, UX-DR55).
 */
type Terms =
  | { readonly kind: "pending" }
  | { readonly kind: "chips"; readonly draft: Draft }
  | { readonly kind: "frozen"; readonly reason: string; readonly terms: readonly string[] };

/** An empty draft — what a brand-new space starts from. */
const EMPTY_DRAFT: Draft = { tags: [], flags: [], origin: null, text: null, fields: [] };

export interface SessionSpaceEditorProps {
  /** The sessions root whose `_spaces/` is being written. */
  rootId: string;
  /**
   * The space being rewritten, or `null` to create one.
   *
   * A create seeds nothing and asks for everything, deliberately: the five
   * defaults cover the shapes the template ships, so a space made by hand is one
   * nobody had a default for.
   */
  space: SessionSpaceVm | null;
  /**
   * Every tag the session's own files carry, for the chooser — the session's
   * rather than the zone's, because a space is written while looking at one
   * session and the tags in front of you are the ones you mean.
   *
   * A tag outside this list can still be typed. A space is a document being
   * authored, and naming the tag the work is about to carry before any file
   * carries it is ordinary; the live filter bar refuses creation for the
   * opposite reason — a filter naming a tag nothing has is just an unexplained
   * empty list.
   */
  vocabulary: readonly string[];
  /** Leave without writing anything. */
  onClose: () => void;
  /** The space was written; the section should re-read itself. */
  onSaved: () => void;
}

export function SessionSpaceEditor({
  rootId,
  space,
  vocabulary,
  onClose,
  onSaved,
}: SessionSpaceEditorProps) {
  const nameId = useId();
  const sortKeyId = useId();
  const sortDirId = useId();
  const orderId = useId();
  const iconGroupId = useId();
  const [name, setName] = useState(space?.name ?? "");
  const [icon, setIcon] = useState<string | null>(space?.icon ?? null);
  const [iconQuery, setIconQuery] = useState("");
  const iconGroups: readonly SpaceIconGroup[] = useMemo(
    () => matchSpaceIcons(iconQuery),
    [iconQuery],
  );
  // Seeded from `sortEffective`, never from `sort`: the stored value may be
  // empty or a word keeper does not know, and working out what either resolves
  // to is a rule that exists once, in Rust. The form shows what the list is
  // actually doing, which is what makes Save a repair rather than a rewrite.
  const [sortKey, setSortKey] = useState(() => space?.sortEffective.split(" ")[0] ?? "modified");
  const [sortDir, setSortDir] = useState(() => space?.sortEffective.split(" ")[1] ?? "desc");
  // Held as text, because a number input's value is text and `Number("")` is 0
  // — which would silently reposition a space the moment somebody cleared the
  // box to retype it. An empty box is "unpositioned", which is what a missing
  // key already means.
  const [order, setOrder] = useState(() =>
    space === null || space.order === 0 ? "" : String(space.order),
  );
  // A create starts with chips and an empty draft; only a stored query has to be
  // decomposed, and only a stored query can turn out to be unrepresentable.
  const [terms, setTerms] = useState<Terms>(
    space === null ? { kind: "chips", draft: EMPTY_DRAFT } : { kind: "pending" },
  );
  const [saving, setSaving] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const storedQuery = space?.query ?? null;
  useEffect(() => {
    if (storedQuery === null) {
      return;
    }
    let cancelled = false;
    void notesSpaceTerms(storedQuery)
      .then((read) => {
        if (cancelled) {
          return;
        }
        setTerms(
          read.kind === "chips"
            ? {
                kind: "chips",
                draft: {
                  tags: read.tags.map((entry) => ({ tag: entry.tag, term: entry.term })),
                  flags: read.flags,
                  origin: read.origin,
                  text: read.text,
                  fields: read.fields,
                },
              }
            : { kind: "frozen", reason: SPACE_TERMS_READONLY, terms: read.terms },
        );
      })
      .catch(() => {
        // A query that does not parse is a state of the space, not a failed
        // command: the row already says so, and the editor says the same rather
        // than offering an empty chip set one Save away from selecting nothing.
        if (!cancelled) {
          setTerms({ kind: "frozen", reason: SESSION_SPACE_TERMS_BROKEN, terms: [storedQuery] });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [storedQuery]);

  const trimmedName = name.trim();
  // Every axis a draft can carry is enumerated, so a term added later cannot
  // quietly make this test wrong.
  const emptyDraft =
    terms.kind === "chips" &&
    terms.draft.tags.length === 0 &&
    terms.draft.flags.length === 0 &&
    terms.draft.origin === null &&
    terms.draft.text === null &&
    terms.draft.fields.length === 0;
  const refusal =
    trimmedName === "" ? SPACE_NO_NAME : emptyDraft ? SPACE_NO_TERMS : (failure ?? null);

  // A query may name the same unhandled term twice, so the read-only rows carry
  // an occurrence counter rather than the bare text: two identical keys is a
  // React bug, and deduplicating would be a lie about what the file says.
  const frozenRows: { key: string; term: string }[] = [];
  if (terms.kind === "frozen") {
    const seen = new Map<string, number>();
    for (const term of terms.terms) {
      const nth = seen.get(term) ?? 0;
      seen.set(term, nth + 1);
      frozenRows.push({ key: `${term}#${nth}`, term });
    }
  }

  // The same counter over the field chips, with the position carried alongside:
  // `field:status!=done field:status!=deferred` is a legal pair, so a chip is
  // removed BY POSITION — removing "the one that reads status" would take both.
  // This is the task board's own vocabulary, so a session's spaces meet it far
  // more often than a vault's do.
  const fieldRows: { key: string; field: NoteSpaceFieldVm; at: number }[] = [];
  if (terms.kind === "chips") {
    const seen = new Map<string, number>();
    terms.draft.fields.forEach((field, at) => {
      const label = `field:${field.key}${field.op}${field.value}`;
      const nth = seen.get(label) ?? 0;
      seen.set(label, nth + 1);
      fieldRows.push({ key: `${label}#${nth}`, field, at });
    });
  }

  function editDraft(next: (draft: Draft) => Draft): void {
    setTerms((current) =>
      current.kind === "chips" ? { kind: "chips", draft: next(current.draft) } : current,
    );
  }

  async function save(): Promise<void> {
    if (terms.kind === "pending" || refusal !== null || saving) {
      return;
    }
    setSaving(true);
    setFailure(null);
    try {
      await sessionsSpaceSave(rootId, {
        // A path, not a ULID — and `null` for a create, which is what tells Rust
        // to derive a filename from the name rather than rewrite a file.
        id: space?.id ?? null,
        name: trimmedName,
        // The frozen arm hands back the bytes it was given. Re-emitting from
        // chips here is the exact failure this surface is arranged to prevent.
        query: terms.kind === "chips" ? spaceQueryText(terms.draft) : (space?.query ?? ""),
        // Always the canonical `<key> <dir>`, which is what makes saving a space
        // whose stored sort keeper could not read a repair: the form showed the
        // fallback and said so, and this writes what was on screen.
        sort: `${sortKey} ${sortDir}`,
        icon,
        // An empty or unreadable box is "unpositioned" — the same zero an absent
        // key means — rather than a reason to refuse the whole save.
        order: Number.parseFloat(order.trim()) || 0,
      });
      onSaved();
    } catch {
      setFailure(SESSION_SPACE_SAVE_FAILED);
      setSaving(false);
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(next) => {
        if (!next) {
          onClose();
        }
      }}
    >
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {space === null ? SESSION_SPACE_NEW_TITLE : SESSION_SPACE_EDIT_TITLE}
          </DialogTitle>
          <DialogDescription>{SESSION_SPACE_SCOPE_HINT}</DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor={nameId}>Name</Label>
            <Input
              id={nameId}
              value={name}
              onChange={(event) => setName(event.target.value)}
              autoComplete="off"
            />
          </div>

          {/* The icon chooser (UX-DR82), the notes editor's arrangement and its
              components: a search over the names, then one labelled group per
              section. "No icon" stays outside the search, because it is not an
              icon and no query names it — filtering it away would make "take the
              glyph off" a thing you can only do by clearing the box first. */}
          <fieldset className="flex flex-col gap-1.5">
            <legend className="font-medium text-sm">Icon</legend>
            <Input
              type="search"
              value={iconQuery}
              onChange={(event) => setIconQuery(event.target.value)}
              aria-label="Search icons"
              placeholder="Search icons"
              autoComplete="off"
              className="h-8"
            />
            <div className={cn("flex flex-col gap-2 overflow-y-auto", ICON_GRID_MAX_HEIGHT)}>
              <div className="flex flex-wrap gap-1">
                <IconChoice
                  name={null}
                  selected={icon === null}
                  onSelect={() => setIcon(null)}
                  label="No icon"
                />
              </div>
              {iconGroups.map((group) => (
                <div key={group.label} className="flex flex-col gap-1">
                  <span id={`${iconGroupId}-${group.label}`} className="label-caps text-faint">
                    {group.label}
                  </span>
                  {/* biome-ignore lint/a11y/useSemanticElements: `<fieldset>` is the
                      semantic form-grouping element and this is a button grid inside
                      a dialog that already owns the form; a legend cannot be the
                      styled heading span the section labels share. */}
                  <div
                    role="group"
                    aria-labelledby={`${iconGroupId}-${group.label}`}
                    className="flex flex-wrap gap-1"
                  >
                    {Object.keys(group.icons).map((key) => (
                      <IconChoice
                        key={key}
                        name={key}
                        selected={icon === key}
                        onSelect={() => setIcon(key)}
                        label={key}
                      />
                    ))}
                  </div>
                </div>
              ))}
              {iconGroups.length === 0 && (
                <p data-slot="icon-search-empty" className="text-muted-foreground text-sm">
                  {SPACE_ICON_NO_MATCH}
                </p>
              )}
            </div>
          </fieldset>

          {/* Rust already worded what it could not read; the form repeats it
              rather than inventing a second sentence, so the row in the section
              and the dialog say the same thing about the same file. */}
          {space !== null && space.warnings.length > 0 && (
            <ul aria-label="What keeper couldn't read" className="flex flex-col gap-1">
              {space.warnings.map((said) => (
                <li key={said} data-slot="space-warning" className="text-destructive text-sm">
                  {said}
                </li>
              ))}
            </ul>
          )}

          <div className="flex flex-wrap items-end gap-3">
            <div className="flex min-w-32 flex-1 flex-col gap-1.5">
              <Label htmlFor={sortKeyId}>Sort by</Label>
              <select
                id={sortKeyId}
                value={sortKey}
                onChange={(event) => setSortKey(event.target.value)}
                className="h-9 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                {SESSION_SPACE_SORT_KEYS.map((option) => (
                  <option key={option.key} value={option.key}>
                    {option.label}
                  </option>
                ))}
                {/* The stored key, whenever it is not one of the four above —
                    `recorded`, or a word a hand-written file used. Rendered as
                    its own option so the select can show what the file says: a
                    `<select>` whose value matches no option renders the FIRST
                    one, which here reads as "Order" — a lie about the file, and
                    one the next Save would make true. */}
                {!SESSION_SPACE_SORT_KEYS.some((option) => option.key === sortKey) && (
                  <option value={sortKey}>{sortKey}</option>
                )}
              </select>
            </div>
            <div className="flex min-w-32 flex-1 flex-col gap-1.5">
              <Label htmlFor={sortDirId}>Direction</Label>
              <select
                id={sortDirId}
                value={sortDir}
                onChange={(event) => setSortDir(event.target.value)}
                className="h-9 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <option value="asc">
                  {(SORT_DIR_LABELS[sortKey] ?? ["Ascending", "Descending"])[0]}
                </option>
                <option value="desc">
                  {(SORT_DIR_LABELS[sortKey] ?? ["Ascending", "Descending"])[1]}
                </option>
              </select>
            </div>
            <div className="flex w-24 flex-col gap-1.5">
              <Label htmlFor={orderId}>Position</Label>
              <Input
                id={orderId}
                type="number"
                step="any"
                inputMode="decimal"
                placeholder="0"
                value={order}
                onChange={(event) => setOrder(event.target.value)}
              />
            </div>
          </div>
          {SESSION_SPACE_SORT_NOTES[`${sortKey} ${sortDir}`] !== undefined && (
            <p data-slot="sort-note" className="-mt-2 text-muted-foreground text-sm">
              {SESSION_SPACE_SORT_NOTES[`${sortKey} ${sortDir}`]}
            </p>
          )}

          <section aria-label="Terms" className="flex flex-col gap-2">
            <span className="font-medium text-sm">Terms</span>
            {terms.kind === "pending" && (
              <p className="text-muted-foreground text-sm">Reading this space's terms…</p>
            )}
            {terms.kind === "frozen" && (
              <>
                <p className="text-muted-foreground text-sm">{terms.reason}</p>
                <ul className="flex flex-col gap-1">
                  {frozenRows.map((row) => (
                    <li
                      key={row.key}
                      data-slot="frozen-term"
                      className="rounded-md bg-muted px-2 py-1 text-xs"
                    >
                      {row.term}
                    </li>
                  ))}
                </ul>
              </>
            )}
            {terms.kind === "chips" && (
              <>
                <div className="flex flex-wrap items-center gap-1">
                  {terms.draft.tags.map((chip) => (
                    <TagFilterChip
                      key={chip.tag}
                      chip={chip}
                      onCycle={(tag) =>
                        editDraft((draft) => ({
                          ...draft,
                          tags: withTagTerm(
                            draft.tags,
                            tag,
                            nextTagChipState(tagChipState(draft.tags, tag)),
                          ),
                        }))
                      }
                      onRemove={(tag) =>
                        editDraft((draft) => ({
                          ...draft,
                          tags: withTagTerm(draft.tags, tag, "off"),
                        }))
                      }
                    />
                  ))}
                  {terms.draft.flags.map((flag) => (
                    <FixedTerm
                      key={`is:${flag}`}
                      label={`is:${flag}`}
                      onRemove={() =>
                        editDraft((draft) => ({
                          ...draft,
                          flags: draft.flags.filter((held) => held !== flag),
                        }))
                      }
                    />
                  ))}
                  {terms.draft.origin !== null && (
                    <FixedTerm
                      label={`origin:${terms.draft.origin}`}
                      onRemove={() => editDraft((draft) => ({ ...draft, origin: null }))}
                    />
                  )}
                  {fieldRows.map((row) => (
                    <FixedTerm
                      key={row.key}
                      label={`${row.field.key} ${row.field.op} ${row.field.value}`}
                      onRemove={() =>
                        editDraft((draft) => ({
                          ...draft,
                          fields: draft.fields.filter((_, at) => at !== row.at),
                        }))
                      }
                    />
                  ))}
                  {terms.draft.text !== null && (
                    <FixedTerm
                      label={`text:${terms.draft.text}`}
                      onRemove={() => editDraft((draft) => ({ ...draft, text: null }))}
                    />
                  )}
                </div>
                <TagCombobox
                  label="Add a tag"
                  placeholder="Type or browse"
                  vocabulary={vocabulary}
                  chosen={terms.draft.tags.map((chip) => chip.tag)}
                  allowCreate
                  onChoose={(tag) =>
                    editDraft((draft) => ({
                      ...draft,
                      tags: withTagTerm(draft.tags, tag, "include"),
                    }))
                  }
                />
              </>
            )}
          </section>

          {refusal !== null && (
            <p role="status" className="text-destructive text-sm">
              {refusal}
            </p>
          )}
        </div>

        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            disabled={terms.kind === "pending" || refusal !== null || saving}
            onClick={() => void save()}
          >
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
