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
import { syncErrorMessage } from "@/lib/stores/sync";
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

/**
 * How the section opens, and the three answers it can carry (Story 51.3,
 * FR-289).
 *
 * **A three-option control and not a checkbox**, which is what the ask named.
 * The fold has a user-global setting under it (`sessions.spaces_folded`), so a
 * space has three things to say and not two: folded, unfolded, and *nothing —
 * whatever the setting says*. A two-state checkbox cannot spell the third, and
 * the first Save of any space would then stamp `folded: false` into it and quietly
 * take that space out from under the setting forever. An indeterminate checkbox
 * spells it and is worse: a state a person can leave and cannot get back to with
 * a pointer. Three named options say all three out loud, in the same `<select>`
 * shape the sort controls above already use.
 */
export const SESSION_SPACE_FOLDED_LABEL = "Opens";
export const SESSION_SPACE_FOLDED_OPTIONS: readonly { value: string; label: string }[] = [
  { value: "unset", label: "However the setting says" },
  { value: "folded", label: "Folded" },
  { value: "unfolded", label: "Unfolded" },
];
export const SESSION_SPACE_FOLDED_NOTE =
  "Whether this space's rows start hidden. Folding or unfolding it by hand still wins for as long as you keep that answer.";

/**
 * The row cap, and the sentence that keeps it from being read as a filter
 * (Story 51.3, FR-290).
 *
 * The note says *shows* and never *finds*, because that is the whole distinction
 * between this key and a note space's `keeper.limit`: the query still selects
 * everything, the header still counts everything, and the rest is one press
 * away. A person who read this as "only look at the first 5" would trust a
 * section that was hiding work from them.
 */
export const SESSION_SPACE_ROWS_LABEL = "Rows";
export const SESSION_SPACE_ROWS_NOTE =
  "How many rows to show before the rest folds behind a “Show more”. The space still finds every file, and the count beside its name is the whole list. Leave it empty to show all of them.";

/**
 * Where this space's creates land, and the sentence that keeps the field from
 * being read as a filter (Story 52.5, FR-309).
 *
 * The note says *new files* and never *files*: the key governs writes only, and
 * nothing moves when it is set. It also has to say the three places keeper will
 * not write, because the alternative is finding out at Save.
 *
 * **The "leave it empty" sentence moved out of here** (Story 53.5, FR-320),
 * because an empty box no longer has one meaning. A space whose file names no
 * folder inherits keeper's own answer for it, and a space whose file names the
 * empty string has chosen the session's root — so what an empty box means is a
 * fact about the space in front of the operator, and
 * {@link sessionSpaceCreateDirEmptyNote} composes it. This half is the part that
 * is true of every space.
 */
export const SESSION_SPACE_CREATE_DIR_LABEL = "New files go in";
export const SESSION_SPACE_CREATE_DIR_NOTE =
  "A folder inside the session for files this space creates — “logs”, or “notes/2026”. keeper makes it if it is not there, and the new file still carries this space's tag, which is what makes it appear here. Nothing already in the session moves. Not workspace/, which is scratch that dies with the session; not a folder starting with a dot, which keeper never reads back; and nothing outside the session.";

/**
 * What an EMPTY destination box means for this particular space (Story 53.5,
 * FR-320), or `null` when the box is not empty and says so itself.
 *
 * Three answers, because the field is three-valued and the operator cannot see
 * which of the three they are looking at otherwise:
 *
 * - `typed === null` with a default to inherit — the file names no folder, so
 *   keeper's own answer for this space applies. This is the state EVERY zone
 *   seeded before Story 53.5 is in, and the sentence is what stops an empty box
 *   from reading as "the session's root" when it no longer is.
 * - `typed === ""` — the box was cleared, which is a deliberate *the session's
 *   root* and is written into the file as an empty key so it keeps meaning that.
 *   When there is a default it would otherwise have inherited, the sentence
 *   names it, so the operator can see what they are choosing against.
 * - no default either way — the root, which is where every create went before
 *   any of this existed.
 *
 * `inherited` is Rust's answer (`SessionSpaceVm.createDirDefault`), never a
 * table read here: the surface does not know the defaults and must not learn
 * them (AD-65).
 */
export function sessionSpaceCreateDirEmptyNote(
  typed: string | null,
  inherited: string,
): string | null {
  if (typed !== null && typed.trim() !== "") {
    return null;
  }
  if (inherited === "") {
    return "New files go at the session's root, which is where they go when nothing names a folder.";
  }
  return typed === null
    ? `This space's file names no folder, so keeper's own answer for it applies: new files go in “${inherited}”.`
    : `This space's file names the session's own folder, so new files go there rather than in keeper's “${inherited}”.`;
}

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
  const foldedId = useId();
  const rowsId = useId();
  const createDirId = useId();
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
  // The three answers, held as the `<select>`'s own word rather than as a
  // `boolean | null`: the control's value has to be a string, and mapping in one
  // direction only — at Save — keeps "nothing said" from having two spellings in
  // this file.
  const [folded, setFolded] = useState(() =>
    space?.folded === true ? "folded" : space?.folded === false ? "unfolded" : "unset",
  );
  // Text for `order`'s reason, one comment up. An empty box is "no cap", which
  // is what a missing key already means.
  const [rows, setRows] = useState(() => (space?.rows == null ? "" : String(space.rows)));
  // Text OR `null`, and the `null` is load-bearing (Story 53.5, FR-320):
  // untouched means the file's own state — which for every space seeded before
  // this story is "no key at all", the state that inherits keeper's own folder
  // for it. Any keystroke, including the one that empties the box, makes this a
  // string and therefore an ANSWER; clearing a folder is a deliberate "the
  // session's own root" and is saved as the empty key that keeps saying so.
  // Collapsing the two into `""` is what would let a rename silently hand a
  // space back to a default it had chosen against.
  const [createDir, setCreateDir] = useState<string | null>(space?.createDir ?? null);
  // Rust's answer for what an absent key inherits, never a table read here
  // (AD-65). `""` for a space that claims no default, or claims one that names
  // nowhere — About and the residue.
  const inheritedDir = space?.createDirDefault ?? "";
  // Recomputed on every render rather than memoised: it is one comparison and a
  // template string, and the value has to track the box the operator is typing
  // in — clearing a folder changes what an empty box means, and the sentence
  // saying so is the only place that change is visible.
  const emptyDirNote = sessionSpaceCreateDirEmptyNote(createDir, inheritedDir);
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
        // The two keys this story added, and they are sent on EVERY save
        // including one that changed something else entirely: `render_edit`
        // replaces the whole `keeper:` map, so a form that omitted them would
        // delete the operator's answers the first time they renamed a space.
        folded: folded === "folded" ? true : folded === "unfolded" ? false : null,
        // A cap has to be a whole number above zero, and anything else writes no
        // key at all. `parseInt` accepts "5 rows" and "5.9", so the digits are
        // checked first — and 0 goes the same way as the empty box, because a
        // section with no rows under a header that still counts them is not a
        // cap somebody meant.
        rows: /^\d+$/.test(rows.trim()) ? Number.parseInt(rows.trim(), 10) || null : null,
        // Sent on every save for `folded`'s reason. Trimmed only: this is a
        // path, and Rust owns what a path may be — an escaping, scratch or
        // dotted directory comes back as a rejection with its own sentence,
        // which is what the catch below now shows.
        //
        // `null` straight through, so a field nobody touched writes no key and
        // the space keeps inheriting (Story 53.5). A touched-and-empty box is
        // `""`, which is the answer "the session's own root" and is written as
        // the empty key that keeps saying it.
        createDir: createDir === null ? null : createDir.trim(),
      });
      onSaved();
    } catch (raw: unknown) {
      // Rust's own sentence, not a generic one. A refused destination names which
      // rule it broke — scratch that dies with the session, a dotted folder the
      // scan never reads back, a path that leaves the session — and three
      // different refusals reading identically is the silence this surface is
      // supposed to have ended.
      setFailure(syncErrorMessage(raw, SESSION_SPACE_SAVE_FAILED));
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

          This form is taller than a 900px window — the icon chooser, five
          selects, two help paragraphs and the term chips — and the shadcn panel
          constrains WIDTH only (`ui/dialog.tsx:55`), centred with `top-1/2
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

          `notes/space-editor.tsx` is this surface's deliberate twin and carries
          the identical pair. */}
      <DialogContent className="flex max-h-[85vh] flex-col gap-4 overflow-hidden sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {space === null ? SESSION_SPACE_NEW_TITLE : SESSION_SPACE_EDIT_TITLE}
          </DialogTitle>
          <DialogDescription>{SESSION_SPACE_SCOPE_HINT}</DialogDescription>
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

          {/* The icon chooser (UX-DR82), the notes editor's arrangement and its
              components: a search over the names, then one labelled group per
              section. "No icon" stays outside the search, because it is not an
              icon and no query names it — filtering it away would make "take the
              glyph off" a thing you can only do by clearing the box first. */}
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

          {/* How it opens and how much it shows: one row, because they are the
              two answers to "what does this space look like when I arrive", and
              each carries its own line saying what it does. Below the sort rather
              than beside it — the sort row is already three controls wide at
              `sm:max-w-lg`, and a fifth would wrap into a column of one. */}
          <div className="flex flex-wrap items-end gap-3">
            <div className="flex min-w-40 flex-1 flex-col gap-1.5">
              <Label htmlFor={foldedId}>{SESSION_SPACE_FOLDED_LABEL}</Label>
              <select
                id={foldedId}
                value={folded}
                onChange={(event) => setFolded(event.target.value)}
                className="h-9 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                {SESSION_SPACE_FOLDED_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex w-24 flex-col gap-1.5">
              <Label htmlFor={rowsId}>{SESSION_SPACE_ROWS_LABEL}</Label>
              <Input
                id={rowsId}
                type="number"
                min="1"
                step="1"
                inputMode="numeric"
                // "All" and not "0": zero is not a cap this form can write, and a
                // placeholder that showed it would be teaching the one value the
                // save throws away.
                placeholder="All"
                value={rows}
                onChange={(event) => setRows(event.target.value)}
              />
            </div>
          </div>
          <p data-slot="folded-note" className="-mt-2 text-muted-foreground text-sm">
            {SESSION_SPACE_FOLDED_NOTE}
          </p>
          <p data-slot="rows-note" className="-mt-2 text-muted-foreground text-sm">
            {SESSION_SPACE_ROWS_NOTE}
          </p>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor={createDirId}>{SESSION_SPACE_CREATE_DIR_LABEL}</Label>
            <Input
              id={createDirId}
              // The placeholder is what an empty box MEANS for this space, and
              // since Story 53.5 that is not one sentence: a space whose file
              // names no folder inherits keeper's own answer for it, so the
              // inherited folder is what the empty box is doing. Drawn as the
              // placeholder and never as the value, because a value would be
              // persisted by the next Save — keeper installing a default into the
              // operator's file, which is the write AD-121 forbids.
              placeholder={
                createDir === null && inheritedDir !== ""
                  ? inheritedDir
                  : "The session's own folder"
              }
              // `?? ""` only because a DOM input's value must be a string. The
              // three-valued answer lives in the state, and any keystroke —
              // including the one that empties the box — moves it out of `null`.
              value={createDir ?? ""}
              onChange={(event) => setCreateDir(event.target.value)}
            />
            <p data-slot="create-dir-note" className="text-muted-foreground text-sm">
              {SESSION_SPACE_CREATE_DIR_NOTE}
            </p>
            {emptyDirNote !== null && (
              <p data-slot="create-dir-empty-note" className="text-muted-foreground text-sm">
                {emptyDirNote}
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
                {/* Story 53.2 gave this mount the close it never had, and it is
                    not a prop: the chooser owns whether its list is unfolded, so
                    it opens folded and the caret leaving it — or a press anywhere
                    else on this form — folds it again, with no toggle of this
                    dialog's own. No `onDismiss`, because there is nothing of this
                    form's to unmount. Escape is not that path here and cannot be:
                    Radix's dismissable layer claims it at the document in the
                    capture phase and closes the whole editor first, which is this
                    dialog's own older decision. */}
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
