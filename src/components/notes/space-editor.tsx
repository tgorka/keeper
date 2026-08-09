/**
 * The space editor (Epic 43, Story 43.4, FR-149, UX-DR55).
 *
 * A space was a thing you could create and never change: adjusting one meant
 * deleting it and rebuilding it from memory, which is why so few of them exist.
 * This is the surface that ends that — a name, an icon from a fixed set, and the
 * space's terms as Story 43.3's three-state chips.
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
import {
  Archive,
  Bell,
  Bookmark,
  Briefcase,
  CalendarDays,
  Clock,
  Code,
  FileText,
  Flag,
  Folder,
  Globe,
  Hash,
  Heart,
  Inbox,
  Layers,
  Lightbulb,
  type LucideIcon,
  Mic,
  Pin,
  Search,
  Star,
  Tag,
  Target,
  Users,
  Video,
  Zap,
} from "lucide-react";
import { useEffect, useId, useState } from "react";
import { tagPaths } from "@/components/notes/editor/tag-complete";
import { TagFilterChip } from "@/components/notes/note-filter-bar";
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
import type { NoteSpaceVm } from "@/lib/ipc/client";
import { notesSpaceSave, notesSpaceTerms, notesTagTree } from "@/lib/ipc/client";
import {
  nextTagChipState,
  type TagChip,
  tagChipState,
  withTagTerm,
} from "@/lib/stores/notes-filters";
import { cn } from "@/lib/utils";

/**
 * The icons a space may carry, keyed by the name stored in its frontmatter.
 *
 * Fixed, and now twenty-four rather than ten (Story 44.4). Still fixed, because
 * an open picker is a decision the user makes every time and a value keeper then
 * validates forever; wider, because ten shapes cannot tell a rail of saved views
 * apart once the four defaults have taken four of them. The keys are lucide's own
 * names, so the value in the file is a thing a human hand-editing frontmatter can
 * guess, and a name that is not in this map draws {@link SpaceIconFallback}
 * without keeper rewriting what is on disk.
 *
 * The first four are load-bearing rather than decorative: `inbox`,
 * `calendar-days`, `pin` and `video` are what the seeded defaults ask for
 * (Story 44.3), so the set covering them is what makes the rail render as the
 * fixed rows it replaced.
 */
export const SPACE_ICONS: Readonly<Record<string, LucideIcon>> = {
  inbox: Inbox,
  "calendar-days": CalendarDays,
  pin: Pin,
  video: Video,
  archive: Archive,
  bell: Bell,
  bookmark: Bookmark,
  briefcase: Briefcase,
  clock: Clock,
  code: Code,
  "file-text": FileText,
  flag: Flag,
  folder: Folder,
  globe: Globe,
  hash: Hash,
  heart: Heart,
  lightbulb: Lightbulb,
  mic: Mic,
  search: Search,
  star: Star,
  tag: Tag,
  target: Target,
  users: Users,
  zap: Zap,
};

/**
 * What a space draws when it has no icon — and what it draws when its stored
 * icon is not in {@link SPACE_ICONS} any more.
 *
 * The unknown case renders this rather than nothing, because a row with a hole
 * where every sibling has a glyph reads as a broken space rather than as an
 * unfamiliar icon name. The *stored value* is untouched: the picker simply shows
 * nothing selected, and saving without choosing sends the name straight back. An
 * icon set shrinking must not silently rewrite what is in someone's vault, for
 * the same reason a query term keeper cannot parse is not rewritten either.
 */
export const SpaceIconFallback: LucideIcon = Layers;

/** The glyph a space's stored icon name draws. */
export function spaceIcon(name: string | null): LucideIcon {
  return (name !== null ? SPACE_ICONS[name] : undefined) ?? SpaceIconFallback;
}

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
  const [name, setName] = useState(space.name);
  const [icon, setIcon] = useState<string | null>(space.icon);
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

  const trimmedName = name.trim();
  // "No terms left" is the refusal the AC names: an empty query is not
  // "everything", and every axis a draft can carry is enumerated here so a term
  // added later cannot quietly make this test wrong.
  const emptyDraft =
    terms.kind === "chips" &&
    terms.draft.tags.length === 0 &&
    terms.draft.flags.length === 0 &&
    terms.draft.origin === null &&
    terms.draft.text === null;
  const refusal =
    trimmedName === "" ? SPACE_NO_NAME : emptyDraft ? SPACE_NO_TERMS : (failure ?? null);

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
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Edit space</DialogTitle>
          <DialogDescription>
            A space is a saved filter. Changing it here changes what it selects.
          </DialogDescription>
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

          <fieldset className="flex flex-col gap-1.5">
            <legend className="font-medium text-sm">Icon</legend>
            <div className="flex flex-wrap gap-1">
              <IconChoice
                name={null}
                selected={icon === null}
                onSelect={() => setIcon(null)}
                label="No icon"
              />
              {Object.keys(SPACE_ICONS).map((key) => (
                <IconChoice
                  key={key}
                  name={key}
                  selected={icon === key}
                  onSelect={() => setIcon(key)}
                  label={key}
                />
              ))}
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
                      className="rounded-md bg-muted px-2 py-1 font-mono text-xs"
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
 */
function IconChoice({
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
 */
function FixedTerm({ label, onRemove }: { label: string; onRemove: () => void }) {
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
