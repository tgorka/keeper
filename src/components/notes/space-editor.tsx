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
  CalendarDays,
  Flag,
  Folder,
  Inbox,
  Layers,
  type LucideIcon,
  Star,
  Tag,
  Users,
  Zap,
} from "lucide-react";
import { useEffect, useId, useState } from "react";
import { tagPaths } from "@/components/notes/editor/tag-complete";
import { TagFilterChip } from "@/components/notes/note-filter-bar";
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
 * Fixed and small on purpose. An open icon picker is a decision the user has to
 * make every time and a value keeper then has to validate forever; eight
 * recognisable shapes are enough to tell a sidebar's worth of saved views apart
 * at a glance, which is the whole job. The keys are lucide's own names, so the
 * value in the file is a thing a human hand-editing frontmatter can guess.
 */
export const SPACE_ICONS: Readonly<Record<string, LucideIcon>> = {
  inbox: Inbox,
  star: Star,
  flag: Flag,
  folder: Folder,
  tag: Tag,
  "calendar-days": CalendarDays,
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
  const addTagId = useId();
  const [name, setName] = useState(space.name);
  const [icon, setIcon] = useState<string | null>(space.icon);
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
        // No tag list means no add-a-tag control; the chips already on the space
        // still cycle and still come off.
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
        sort: space.sort,
        limit: space.limit,
        icon,
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
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor={addTagId}>Add a tag</Label>
                  {/* The vault's own tags, never a free-text field: a tag is
                      whatever `notes::tags::normalise` says it is, and a box
                      that let someone type one would be a second definition of
                      that in TypeScript. */}
                  <select
                    id={addTagId}
                    value=""
                    className="h-9 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    onChange={(event) => {
                      const tag = event.target.value;
                      if (tag !== "") {
                        editDraft((draft) => ({
                          ...draft,
                          tags: withTagTerm(draft.tags, tag, "include"),
                        }));
                      }
                    }}
                  >
                    <option value="">Choose a tag…</option>
                    {vaultTags
                      .filter((tag) => tagChipState(terms.draft.tags, tag) === "off")
                      .map((tag) => (
                        <option key={tag} value={tag}>
                          {tag}
                        </option>
                      ))}
                  </select>
                </div>
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
