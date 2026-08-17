/**
 * The space editor (Epic 43, Story 43.4, FR-149, UX-DR55).
 *
 * A space was a thing you could create and never change: adjusting one meant
 * deleting it and rebuilding it from memory, which is why so few of them exist.
 * This is the surface that ends that — a name, an icon from a fixed set, and the
 * space's terms as Story 43.3's three-state chips.
 *
 * **The icon set is a chooser now, not a wall** (Story 45.20, UX-DR82). 44.4 took
 * ten glyphs to twenty-four and drew them as one flat wrap, which is about the
 * largest a flat wrap gets before picking one means reading every glyph. The set
 * is much larger, {@link "@/components/notes/space-icons"} groups it, and a
 * search field over the names is what makes the size an asset. Still fixed, for
 * 44.4's reason unchanged.
 *
 * **The chips are the ones from the filter bar, told what to do.** Not a copy:
 * {@link TagFilterChip} carries three redundant carriers of a chip's state and
 * the accessible name is the one nobody would notice rotting, so it has exactly
 * one definition and this surface hands it a draft list instead of the store.
 * The draft is deliberately not the live filter — editing a space must not
 * re-filter the note list behind the dialog, and Cancel must be able to leave
 * with nothing having happened anywhere.
 *
 * **keeper does not rewrite a term it could not read.** A space's query is DSL
 * text in a synced markdown file that an agent or a person may have written by
 * hand, and the DSL says far more than a chip bar can: `|`, groups, `path:`,
 * `field:`, `date:`, `link:`, `tag:x/*`. `keeper-core` decomposes the query (one
 * grammar, one parser — never a second one in TypeScript), and when it reports
 * even one term the chips cannot hold, this surface shows the query read-only,
 * names those terms, and saves the stored text back byte for byte. Name and icon
 * stay editable, because refusing the whole editor would send someone back to
 * hand-editing frontmatter just to rename a space, which is the pain the story
 * exists to kill. Quietly dropping a term a user typed is the one outcome worse
 * than either.
 *
 * **A space with no terms left refuses to save.** An empty query is not
 * "everything"; the DSL rejects it, and so does this form, before the round trip
 * — a saved view that silently widens to the whole vault is how a bulk action
 * becomes a data-loss story.
 */
import { useEffect, useId, useMemo, useState } from "react";
import { tagPaths } from "@/components/notes/editor/tag-complete";
import { TagFilterChip } from "@/components/notes/note-filter-bar";
import { matchSpaceIcons, type SpaceIconGroup, spaceIcon } from "@/components/notes/space-icons";
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
import type { NoteSpaceFieldVm, NoteSpaceVm, NoteTemplateVm } from "@/lib/ipc/client";
import { notesSpaceSave, notesSpaceTerms, notesTagTree, notesTemplates } from "@/lib/ipc/client";
import {
  nextTagChipState,
  type TagChip,
  tagChipState,
  withTagTerm,
} from "@/lib/stores/notes-filters";
import { cn } from "@/lib/utils";

/** How many icons the chooser shows before it needs its own scroll region.
 *
 * A cap rather than none: 188 glyphs is a wall, and the fieldset sits above a
 * form whose Save button must stay reachable without scrolling past the
 * alphabet. */
const ICON_GRID_MAX_HEIGHT = "max-h-56";

/** What the chooser says when a search names nothing.
 *
 * It names the search rather than saying "no results", because the failure a
 * person makes here is typing a concept the set spells differently — and the
 * only useful next move is to clear the box and browse, which the sentence
 * says. */
export const SPACE_ICON_NO_MATCH =
  "No icon by that name. Clear the search to browse the whole set.";

/** The sentence over the terms of a space the chips will not touch. */
export const SPACE_TERMS_READONLY =
  "keeper can't show these terms as chips, so it won't rewrite them. The query is kept exactly as it is; the name and icon are still yours to change.";

/** The sentence over the terms of a space whose query does not parse. */
export const SPACE_TERMS_BROKEN =
  "This space's query can't be read, so keeper won't rewrite it. Fix it in the note itself; the name and icon are still yours to change.";

/** What the form says instead of saving a space that would select everything. */
export const SPACE_NO_TERMS = "A space needs at least one term. Add one, or cancel.";

/** What the form says instead of saving a space with no name. */
export const SPACE_NO_NAME = "A space needs a name.";

/**
 * The five facts a space can order by, in the words `keeper-core::notes::sort`
 * writes into frontmatter (FR-158).
 *
 * The keys ARE the stored vocabulary — this list is what makes the dropdown and
 * the file agree — but nothing here decides what an unknown value means or what
 * a bare key's direction is. Rust decides both, and hands the answer over as
 * `sortEffective`; this array only draws it.
 */
export const SPACE_SORT_KEYS: readonly { key: string; label: string }[] = [
  { key: "order", label: "Order" },
  { key: "name", label: "Name" },
  { key: "created", label: "Created" },
  { key: "modified", label: "Modified" },
  { key: "recorded", label: "Recorded" },
];

/**
 * What each direction is called, per key.
 *
 * "Ascending" is a word about the machine. What a reader wants to know is
 * whether the newest is at the top, and for `name` that question has different
 * words — so the labels are per key rather than one pair reused five times.
 */
const SORT_DIR_LABELS: Readonly<Record<string, readonly [string, string]>> = {
  order: ["Lowest first", "Highest first"],
  name: ["A to Z", "Z to A"],
  created: ["Oldest first", "Newest first"],
  modified: ["Oldest first", "Newest first"],
  recorded: ["Oldest first", "Newest first"],
};

/**
 * What the editor says over a `recorded` sort.
 *
 * The rule for a note with no session is a real decision (`notes::sort`), and a
 * decision nobody is told about is indistinguishable from an accident. It is one
 * line under the control rather than a tooltip, because the person choosing
 * `recorded` is exactly the person who needs it.
 */
export const SPACE_SORT_RECORDED_NOTE =
  "A note that isn't about a recording is placed by the date it was created, in among the recordings rather than at either end.";

/**
 * What the template chooser calls "hand out nothing".
 *
 * A sentinel `""` rather than a `null` option value, because a `<select>`'s
 * value is always a string and a `null` would arrive back as the literal text
 * `"null"` — which is a path, and one keeper would then go looking for.
 */
export const SPACE_NO_TEMPLATE = "";

/**
 * What the editor says when the space names a template that is not in the
 * vault.
 *
 * The stored value is still shown and still selected. keeper does not silently
 * drop a setting it cannot resolve — the template may be on the other machine,
 * mid-sync, or simply renamed, and clearing the field on the user's behalf
 * would lose the one clue about what it used to point at. Notes created here
 * are still created; the create path says the same thing in its own words.
 */
export const SPACE_TEMPLATE_MISSING =
  "This space names a template that isn't in the vault. Notes created here are still created, just without it — pick another, or restore the template.";

/**
 * The line under the sort control, for the two keys whose behaviour their own
 * name does not give away. Keyed by the canonical `<key> <dir>`.
 *
 * `name`, `created` and `modified` have none, deliberately: a sentence under
 * every control is a sentence nobody reads, and those three do exactly what they
 * are called. The two here do not. `recorded` is meaningful only for a note that
 * came from a session, and `order` promises a manual ordering while most notes
 * have never been given one — a reader who is not told that concludes the sort
 * is broken rather than that the vault is unordered, which is precisely the
 * misreading AD-81 exists to prevent.
 *
 * The `order` wording is Story 44.5's own, verbatim, because it describes
 * `order::cmp_order`'s rule and that story owns it. Note that the alphabet does
 * **not** reverse with the direction — only the position does — which is why
 * neither line says "reverse alphabetically".
 */
export const SPACE_SORT_NOTES: Readonly<Record<string, string>> = {
  "order asc":
    "Each note's own position, lowest first. Most notes have none, so they fall in alphabetically after the ones that do.",
  "order desc":
    "Each note's own position, highest first. Most notes have none, so they fall in alphabetically after the ones that do.",
  "recorded asc": SPACE_SORT_RECORDED_NOTE,
  "recorded desc": SPACE_SORT_RECORDED_NOTE,
};

/** The editable half of a space's query, once the chips can hold all of it. */
interface Draft {
  tags: readonly TagChip[];
  flags: readonly string[];
  origin: string | null;
  text: string | null;
  /**
   * `field:key=value` / `field:key!=value` terms, in the order the query wrote
   * them. A list rather than one slot, because two field terms are two
   * questions — `status` and `priority` — and neither one displaces the other.
   */
  fields: readonly NoteSpaceFieldVm[];
}

/**
 * What the editor knows about the space's query. `pending` is the state before
 * the decomposition lands; the two terminal states are the two the surface
 * behaves differently in, and there is no fourth in which some terms are chips
 * and some are not — see `NoteSpaceTermsVm`, which is an enum for exactly that
 * reason.
 */
type Terms =
  | { readonly kind: "pending" }
  | { readonly kind: "chips"; readonly draft: Draft }
  | { readonly kind: "frozen"; readonly reason: string; readonly terms: readonly string[] };

export function SpaceEditor({
  vaultId,
  space,
  onClose,
  onSaved,
}: {
  vaultId: string;
  space: NoteSpaceVm;
  /** Leave without writing anything. */
  onClose: () => void;
  /** The space was written; the list should re-read itself. */
  onSaved: () => void;
}) {
  const nameId = useId();
  const sortKeyId = useId();
  const sortDirId = useId();
  const orderId = useId();
  const templateId = useId();
  const [name, setName] = useState(space.name);
  const [icon, setIcon] = useState<string | null>(space.icon);
  // The icon search, held here rather than inside the chooser: clearing it on
  // Cancel is free, and a query that outlived the dialog would re-open it
  // filtered to whatever somebody typed last week.
  const [iconQuery, setIconQuery] = useState("");
  // Memoised because it rebuilds six groups and up to 188 entries, and the
  // dialog re-renders on every keystroke in the NAME field too.
  const iconGroups: readonly SpaceIconGroup[] = useMemo(
    () => matchSpaceIcons(iconQuery),
    [iconQuery],
  );
  // Seeded from `sortEffective`, never from `sort`: the raw value may be empty
  // or a word keeper does not know, and working out what either resolves to is
  // a rule that exists once, in Rust. The form shows what the list is actually
  // doing, which is also what makes Save a repair rather than a rewrite.
  const [sortKey, setSortKey] = useState(() => space.sortEffective.split(" ")[0] ?? "modified");
  const [sortDir, setSortDir] = useState(() => space.sortEffective.split(" ")[1] ?? "desc");
  // Held as text, because a number input's value is text and `Number("")` is 0
  // — which would silently reposition a space the moment someone cleared the
  // box to retype it. An unreadable box is "unpositioned", the same thing an
  // absent key means.
  const [order, setOrder] = useState(() => (space.order === 0 ? "" : String(space.order)));
  const [terms, setTerms] = useState<Terms>({ kind: "pending" });
  const [vaultTags, setVaultTags] = useState<readonly string[]>([]);
  // The stored path, verbatim. Seeded from the file rather than resolved
  // against the list, so a template that is missing right now stays selected
  // and stays visible instead of being cleared by a render.
  const [template, setTemplate] = useState(space.template ?? SPACE_NO_TEMPLATE);
  const [templateChoices, setTemplateChoices] = useState<readonly NoteTemplateVm[]>([]);
  // Whether the list above is an ANSWER or merely the absence of one. A vault
  // with genuinely no templates and a read that failed both leave `choices`
  // empty, and only the first of them is evidence that a stored template is
  // gone. Without this the two were one state and a transient IPC failure would
  // have accused a perfectly good setting.
  const [templatesLoaded, setTemplatesLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void notesSpaceTerms(space.query)
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
        // command: its row already says so, and its editor says the same thing
        // rather than offering an empty chip set one Save away from selecting
        // the whole vault.
        if (!cancelled) {
          setTerms({ kind: "frozen", reason: SPACE_TERMS_BROKEN, terms: [space.query] });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [space.query]);

  useEffect(() => {
    let cancelled = false;
    void notesTagTree(vaultId)
      .then((tree) => {
        if (!cancelled) {
          setVaultTags(tagPaths(tree.nodes));
        }
      })
      .catch(() => {
        // A vocabulary that will not load leaves the chooser with nothing to
        // browse, and since 44.13 that no longer means nothing to do: the
        // field still takes a typed tag, because creating is allowed here.
        // The chips already on the space still cycle and still come off.
      });
    return () => {
      cancelled = true;
    };
  }, [vaultId]);

  useEffect(() => {
    let cancelled = false;
    void notesTemplates(vaultId)
      .then((found) => {
        if (!cancelled) {
          setTemplateChoices(found);
          setTemplatesLoaded(true);
        }
      })
      .catch(() => {
        // A list that will not load leaves the chooser with nothing to browse.
        // The stored value is still rendered and still saved: a failed read of
        // the vault's templates is not evidence that this space's one is gone,
        // and clearing it here would turn a transient error into a lost setting.
      });
    return () => {
      cancelled = true;
    };
  }, [vaultId]);

  const trimmedName = name.trim();
  // "No terms left" is the refusal the AC names: an empty query is not
  // "everything", and every axis a draft can carry is enumerated here so a term
  // added later cannot quietly make this test wrong.
  const emptyDraft =
    terms.kind === "chips" &&
    terms.draft.tags.length === 0 &&
    terms.draft.flags.length === 0 &&
    terms.draft.origin === null &&
    terms.draft.text === null &&
    terms.draft.fields.length === 0;
  const refusal =
    trimmedName === "" ? SPACE_NO_NAME : emptyDraft ? SPACE_NO_TERMS : (failure ?? null);
  // The stored template is not one of the options the chooser can offer —
  // either because it is gone, or because the list has not arrived yet.
  const templateUnlisted =
    template !== SPACE_NO_TEMPLATE && !templateChoices.some((choice) => choice.path === template);
  // …and keeper actually knows it is gone. Only ever asserted against a list
  // that ANSWERED: an empty list because the read failed is not evidence the
  // template is missing, and saying it is would put a red sentence under a
  // perfectly good setting and invite the user to clear it.
  const templateMissing = templateUnlisted && templatesLoaded;

  // A query may name the same unhandled term twice, so the read-only rows carry
  // an occurrence counter rather than the bare text: two identical keys is a
  // React bug, and the position would be a lie about which term is which.
  const frozenRows: { key: string; term: string }[] = [];
  if (terms.kind === "frozen") {
    const seen = new Map<string, number>();
    for (const term of terms.terms) {
      const nth = seen.get(term) ?? 0;
      seen.set(term, nth + 1);
      frozenRows.push({ key: `${term}#${nth}`, term });
    }
  }

  // The same counter, for the same reason, over the field chips — and here the
  // position has to be carried alongside rather than folded into the key.
  // `field:status!=done field:status!=deferred` is a legal pair and so is the
  // same term written twice, so a chip is removed BY POSITION: removing "the
  // one that reads status" would take both. The key stays value-derived, so a
  // chip keeps its identity when the one before it goes.
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
      await notesSpaceSave(vaultId, {
        id: space.id,
        name: trimmedName,
        // The frozen arm hands back the bytes it was given. Re-emitting from
        // chips here is the failure this whole surface is arranged to prevent.
        query: terms.kind === "chips" ? spaceQueryText(terms.draft) : space.query,
        // Always the canonical `<key> <dir>`, which is what makes saving a
        // space whose stored sort keeper could not read into a repair: the form
        // showed the fallback and said why, and this writes what was on screen.
        sort: `${sortKey} ${sortDir}`,
        limit: space.limit,
        icon,
        // An empty or unreadable box is "unpositioned" — the same 0 an absent
        // `keeper.order` means — rather than a reason to refuse the whole save.
        order: Number.parseFloat(order.trim()) || 0,
        // Trimmed to the sentinel, so "no template" reaches Rust as the empty
        // string it already treats as absent rather than as a path of spaces.
        template: template.trim() === "" ? null : template.trim(),
      });
      onSaved();
    } catch {
      setFailure("keeper couldn't save this space. Nothing was changed.");
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
      {/* The form scrolls, and the panel is what caps it (Story 52.6, FR-310).

          This form is taller than a 900px window — the icon chooser, the sort
          and layout selects, the help copy and the term chips — and the shadcn
          panel constrains WIDTH only (`ui/dialog.tsx:55`), centred with `top-1/2
          -translate-y-1/2`. A transform creates no scroll container, so the top
          of the form was not merely clipped, it was unreachable.

          The override is the one Settings already established
          (`settings-dialog.tsx:110`), copied rather than reinvented: the panel
          becomes a height-capped flex column that clips, the header and footer
          size to content, and the body between them is `flex-1 min-h-0` so it
          takes the remaining bounded height and scrolls inside it. `min-h-0` is
          load-bearing — a flex child defaults to `min-height:auto` (= its
          content size), which grows past the cap and bleeds out of the dialog
          instead of scrolling. `min-w-0` lets the help copy wrap instead of
          clipping on the right, and `-mr-2 pr-2` keeps the scrollbar off the
          controls. Never the `grid-rows-[…minmax(0,1fr)]` alternative: Tailwind's
          arbitrary-value parser drops the comma inside `minmax()` and emits no
          rule at all, so the cap silently never applies.

          One extra step this form needs that Settings did not: two of the body's
          children hold a scroll region of their own — the icon grid's
          `ICON_GRID_MAX_HEIGHT` and the tag combobox's always-rendered
          `max-h-48` listbox. A flex item whose own overflow is not `visible` has
          an automatic minimum size of ZERO, so those two are the only children
          that can give ground, and the flex algorithm hands them the whole
          negative free space: the icon chooser and the tag list collapse to a
          sliver and the body never scrolls at all. Same algorithm, other axis, as
          the pane-header defect in `files-pane.tsx:2063`. `shrink-0` on the icon
          fieldset and the Terms section is what makes the overflow land on the
          body, where the scrollbar is.

          `sessions/session-space-editor.tsx` is this surface's deliberate twin
          and carries the identical pair. */}
      <DialogContent className="flex max-h-[85vh] flex-col gap-4 overflow-hidden sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Edit space</DialogTitle>
          <DialogDescription>
            A space is a saved filter. Changing it here changes what it selects.
          </DialogDescription>
        </DialogHeader>

        <div className="-mr-2 flex min-h-0 min-w-0 flex-1 flex-col gap-4 overflow-y-auto pr-2">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor={nameId}>Name</Label>
            <Input
              id={nameId}
              value={name}
              onChange={(event) => setName(event.target.value)}
              autoComplete="off"
            />
          </div>

          {/* The icon chooser (Story 45.20, UX-DR82).

              A search field over the names, then one labelled grid per group.
              The search is what makes 188 glyphs usable and the groups are what
              make it browsable without one; either alone is the wall 44.4's flat
              wrap of twenty-four became.

              "No icon" stays outside the search, always, and that is deliberate:
              it is not an icon and no query names it, so filtering it away would
              make "take the glyph off this space" a thing you can only do by
              clearing the box first. */}
          {/* `shrink-0` because this fieldset holds a scroll region: without it
              it is one of the only two children that can give ground and the
              body's overflow collapses it instead of scrolling. See the panel
              comment above. */}
          <fieldset className="flex shrink-0 flex-col gap-1.5">
            <legend className="font-medium text-sm">Icon</legend>
            <Input
              type="search"
              value={iconQuery}
              onChange={(event) => setIconQuery(event.target.value)}
              // A label rather than a placeholder: a placeholder disappears the
              // moment somebody types, taking the only description of the field
              // with it, and it is not an accessible name at all in some
              // screen readers.
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
                  <span id={`space-icon-group-${group.label}`} className="label-caps text-faint">
                    {group.label}
                  </span>
                  {/* `group` + `aria-labelledby`, so a screen reader reading the
                      chooser hears which section each glyph is in rather than a
                      run of a hundred and eighty unrelated buttons. */}
                  {/* biome-ignore lint/a11y/useSemanticElements: `<fieldset>` is the
                      semantic form-grouping element and this is a button grid inside
                      a dialog that already owns the form; a legend cannot be the
                      styled heading span the section labels share. */}
                  <div
                    role="group"
                    aria-labelledby={`space-icon-group-${group.label}`}
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
              rather than inventing a second sentence, so the row in the rail and
              the dialog say the same thing about the same file. */}
          {space.warnings.length > 0 && (
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
                {SPACE_SORT_KEYS.map((option) => (
                  <option key={option.key} value={option.key}>
                    {option.label}
                  </option>
                ))}
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
                {/* Worded per key: "ascending" is a word about the machine, and
                    what a reader wants to know is whether the newest is on top
                    — a question `name` asks in different words. */}
                <option value="asc">
                  {(SORT_DIR_LABELS[sortKey] ?? ["Ascending", "Descending"])[0]}
                </option>
                <option value="desc">
                  {(SORT_DIR_LABELS[sortKey] ?? ["Ascending", "Descending"])[1]}
                </option>
              </select>
            </div>
            <div className="flex w-24 flex-col gap-1.5">
              <Label htmlFor={orderId}>Rail position</Label>
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
          {SPACE_SORT_NOTES[`${sortKey} ${sortDir}`] !== undefined && (
            <p data-slot="sort-note" className="-mt-2 text-muted-foreground text-sm">
              {SPACE_SORT_NOTES[`${sortKey} ${sortDir}`]}
            </p>
          )}

          <div className="flex flex-col gap-1.5">
            <Label htmlFor={templateId}>New notes start from</Label>
            <select
              id={templateId}
              value={template}
              onChange={(event) => setTemplate(event.target.value)}
              className="h-9 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <option value={SPACE_NO_TEMPLATE}>No template</option>
              {templateChoices.map((choice) => (
                <option key={choice.path} value={choice.path}>
                  {choice.name}
                </option>
              ))}
              {/* The stored value, whenever it is not one of the options above.
                  Rendered as its own option so the select can show what the
                  file says: a `<select>` whose value matches no option renders
                  the FIRST one, which here reads as "No template" — a lie about
                  the file, and one the next Save would make true. Keyed on
                  `unlisted` rather than on `missing`, because a list that has
                  not arrived yet tells the same lie as a template that is gone;
                  only the SENTENCE below waits for keeper to actually know. */}
              {templateUnlisted && (
                <option value={template}>
                  {templateMissing ? `${template} — not in this vault` : template}
                </option>
              )}
            </select>
            {templateMissing ? (
              <p data-slot="template-missing" className="text-destructive text-sm">
                {SPACE_TEMPLATE_MISSING}
              </p>
            ) : (
              <p className="text-muted-foreground text-sm">
                A template is a note tagged <code>template</code>. Notes created in this space copy
                its body and its other tags.
              </p>
            )}
          </div>

          {/* `shrink-0` for the same reason as the icon fieldset: the tag
              combobox's listbox is a scroll region, which would otherwise make
              this section absorb the body's overflow instead of scrolling it. */}
          <section aria-label="Terms" className="flex shrink-0 flex-col gap-2">
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
                      // Sans: a term is a word the owner wrote, and the same word
                      // is set in the room's voice as an editable chip two
                      // branches below. Freezing it must not change its face.
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
                  {/* Removed BY POSITION, not by value: `field:status!=done
                      field:status!=deferred` is a legal pair, and removing "the
                      one that reads status" would take both. The key is the
                      value plus an occurrence counter (see `fieldRows`), so a
                      chip keeps its identity when the one before it goes. */}
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
                {/* Story 44.13. This was a `<select>`, and 43.4's reason for
                    making it one was that a free-text box would become a second
                    definition of what a tag is. It would not: the typed text
                    goes into the query DSL verbatim and `notes::query` runs
                    every `tag:` through `tags::normalise` on the way back in
                    (query.rs), which is the same road a hand-written space
                    travels. So creating is allowed HERE and refused on the
                    filter bar — a space is a document being authored, and
                    naming the tag the work is about to carry before the first
                    note carries it is ordinary; a live filter naming a tag no
                    note has is just an unexplained empty list. */}
                <TagCombobox
                  label="Add a tag"
                  placeholder="Type or browse"
                  vocabulary={vaultTags}
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

/**
 * One icon in the picker.
 *
 * `aria-pressed` is right here and wrong on a tag chip: this control has two
 * states and its name does not have to carry a third.
 *
 * Exported for the sessions space editor (FR-261), which edits the same kind of
 * object with the same icon set. A second copy of a two-state button is the
 * kind of duplication nobody notices rotting — one of them would keep
 * `aria-pressed` and the other would grow a `role`.
 */
export function IconChoice({
  name,
  selected,
  onSelect,
  label,
}: {
  name: string | null;
  selected: boolean;
  onSelect: () => void;
  label: string;
}) {
  const Glyph = spaceIcon(name);
  return (
    <button
      type="button"
      aria-pressed={selected}
      aria-label={label}
      data-space-icon={name ?? "none"}
      onClick={onSelect}
      className={cn(
        "rounded-md border p-2 outline-none focus-visible:ring-2 focus-visible:ring-ring",
        selected ? "border-ring bg-accent text-accent-foreground" : "border-input",
      )}
    >
      <Glyph aria-hidden="true" className="size-4" />
    </button>
  );
}

/**
 * A term the chips can hold but not cycle: a lens, an origin, a search.
 *
 * Shown rather than hidden, and removable rather than frozen, because a term
 * that is narrowing a space and is not on its editor is a term the next person
 * will spend an afternoon looking for. Widening these to three states is a
 * bigger question about what a negated lens means (Story 43.3 left it open on
 * purpose), so they are two-state here: present, or taken off.
 *
 * Exported for the sessions space editor, for {@link IconChoice}'s reason: the
 * `data-slot="filter-chip"` marker is what every chip test in the app finds
 * these by, and two definitions of it would drift.
 */
export function FixedTerm({ label, onRemove }: { label: string; onRemove: () => void }) {
  return (
    <span
      data-slot="filter-chip"
      className="inline-flex shrink-0 items-center gap-1 rounded-full bg-accent px-2 py-0.5 text-accent-foreground text-xs"
    >
      {label}
      <button
        type="button"
        aria-label={`Remove ${label}`}
        onClick={onRemove}
        className="rounded-full outline-none hover:bg-background/40 focus-visible:ring-2 focus-visible:ring-ring"
      >
        ×
      </button>
    </span>
  );
}
