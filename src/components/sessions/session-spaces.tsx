/**
 * The session's spaces (FR-261, AD-121) — the saved queries a zone reads every
 * one of its sessions through.
 *
 * The tree above lists what the session *holds*, in the order the filesystem
 * hands it over. This lists the same files *by what they are*: the log entries
 * together and newest first, the tasks in board order, the references
 * alphabetically. In the flat contract there are no `logs/` or `refs/` folders
 * to make that grouping for free, so these sections ARE the folders — with the
 * advantage that a file can be in two of them and the disadvantage, stated
 * plainly on the tree above, that on disk it is one pile of markdown.
 *
 * **After the files, deliberately** (the operator's own ordering): the tree is
 * the ground truth and this is a reading of it, so a person scanning down meets
 * the session's contents before meeting keeper's opinions about them.
 *
 * **The `error`/`warnings` split is notes' split, exactly** (Story 44.4): a
 * query that will not parse is an `error`, the space then selects **nothing**,
 * and it never widens to the whole session — a saved view that silently matched
 * everything is how a bulk action becomes a data-loss story. A `sort` or `order`
 * keeper could not read is a `warning`: the space still selects what it selects
 * and is simply not obeying one line of its own file, so it lists normally under
 * a quieter sentence. Sending someone to fix a query that is fine is worse than
 * saying nothing.
 *
 * Every row opens through the ONE file target the tree and the Files pane use
 * (AD-109, UX-DR91), on the `subpath` Rust composed (AD-65) — this file never
 * joins a path and never writes a query.
 */
import { FolderPlus, Pencil, RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { spaceIcon } from "@/components/notes/space-icons";
import { SessionSpaceEditor } from "@/components/sessions/session-space-editor";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Lamp } from "@/components/ui/lamp";
import { formatDraftAge } from "@/lib/format-time";
import type { SessionSpaceFilesVm, SessionSpaceVm } from "@/lib/ipc/client";
import { sessionsSpaceDelete, sessionsSpacesRestore } from "@/lib/ipc/client";
import { panelsStore } from "@/lib/stores/panels";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** The section heading. */
export const SESSION_SPACES_HEADING = "Spaces";

/**
 * What the section says about itself, once, under the heading.
 *
 * It names the zone rather than the session for the editor's reason: these
 * definitions are shared by every session in the root, and the person about to
 * press the pencil is the person who needs to know that before pressing it.
 */
export const SESSION_SPACES_HINT =
  "Saved queries over this session's markdown. They belong to the zone, so every session is read through them.";

/** What a zone with no space definitions at all says. */
export const SESSION_SPACES_EMPTY = "This zone has no spaces. Restore the defaults, or make one.";

/** What one space with nothing to show says. */
export const SESSION_SPACES_NO_FILES = "Nothing in this session yet.";

/**
 * What a space says while its selections have not arrived.
 *
 * Distinct from "nothing yet", and that distinction is the whole reason it
 * exists: the definitions and the selections are two reads (FR-261), so there is
 * a real moment where keeper knows a space is called Tasks and does not yet know
 * what is in it. Drawing that as an empty section would be a wrong answer shown
 * confidently.
 */
export const SESSION_SPACES_LOADING = "Reading…";

/** The subtitle a space with an unparseable query carries — notes' own words. */
export const SESSION_SPACE_BROKEN_SUBTITLE = "This space's query can't be read";

/** The subtitle a space whose `sort` or `order` keeper could not read carries. */
export const SESSION_SPACE_SETTINGS_SUBTITLE = "Some of this space's settings can't be read";

/** The restore control's accessible name. */
export const SESSION_SPACES_RESTORE = "Restore default spaces";

/** What restore says when there was nothing to do. */
export const SESSION_SPACES_RESTORE_NOTHING = "Nothing was missing.";

/** What restore says when keeper could not write. */
export const SESSION_SPACES_RESTORE_FAILED = "keeper couldn't restore the default spaces.";

/** The create control's accessible name. */
export const SESSION_SPACES_NEW = "New space";

/** The delete control's accessible name, suffixed with the space. */
export const SESSION_SPACE_DELETE = "Delete space";

/** The edit control's accessible name, suffixed with the space. */
export const SESSION_SPACE_EDIT = "Edit space";

/** What the delete confirmation asks and answers. */
export const SESSION_SPACE_DELETE_TITLE = "Delete this space?";
export const SESSION_SPACE_DELETE_BODY =
  "The definition goes to the trash — the files it listed are untouched. Every session in this zone stops showing the section.";
export const SESSION_SPACE_DELETE_CONFIRM = "Delete space";
export const SESSION_SPACE_DELETE_FAILED = "keeper couldn't delete this space.";

/** One row, for tests that need to find one by path. */
export const SESSION_SPACE_FILE_TESTID = "session-space-file";

/** One section, for tests that need to find one by space id. */
export const SESSION_SPACE_SECTION_TESTID = "session-space";

export interface SessionSpacesProps {
  rootId: string;
  /** The zone's definitions, in rail order — read by the detail surface. */
  spaces: readonly SessionSpaceVm[];
  /**
   * What each space selected out of this session, or `null` while that second
   * read is still out. Not an empty array: see {@link SESSION_SPACES_LOADING}.
   */
  selections: readonly SessionSpaceFilesVm[] | null;
  /** Re-read both payloads — a write here changes what the other read returns. */
  onChanged: () => void;
}

export function SessionSpaces({ rootId, spaces, selections, onChanged }: SessionSpacesProps) {
  const [editing, setEditing] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<SessionSpaceVm | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  // Every tag every file in this session carries, for the editor's chooser. The
  // session's own rather than the zone's, and taken from what the spaces already
  // selected rather than from a sixth command: the pool was read once to answer
  // `sessions_space_files`, and asking for it again to build a word list would
  // be a second walk for a dropdown. A file no space lists is a file with no
  // kind tag, which the Unfiled notice above already names.
  const vocabulary = useMemo(() => {
    const seen = new Set<string>();
    for (const selection of selections ?? []) {
      for (const file of selection.files) {
        for (const tag of file.tags) {
          seen.add(tag);
        }
      }
    }
    return [...seen].sort((a, b) => a.localeCompare(b));
  }, [selections]);

  const byId = useMemo(() => {
    const map = new Map<string, SessionSpaceFilesVm>();
    for (const selection of selections ?? []) {
      map.set(selection.spaceId, selection);
    }
    return map;
  }, [selections]);

  /**
   * Re-create the defaults this zone is missing.
   *
   * Rust decides what "missing" means and answers with the NAMES it wrote, so
   * this reports them rather than a count: "About and Prompts" tells the
   * operator whether keeper agreed with them about what was gone, and "2" does
   * not. A refusal carries Rust's own sentence when it has one — that sentence
   * names the file it could not write, which is the difference between a bug
   * report and a `chmod`.
   */
  const restore = useCallback(() => {
    setRestoring(true);
    setNotice(null);
    sessionsSpacesRestore(rootId)
      .then((restored) => {
        setNotice(
          restored.names.length === 0
            ? SESSION_SPACES_RESTORE_NOTHING
            : `Restored ${restored.names.join(", ")}.`,
        );
        onChanged();
      })
      .catch((raw: unknown) => setNotice(syncErrorMessage(raw, SESSION_SPACES_RESTORE_FAILED)))
      .finally(() => setRestoring(false));
  }, [rootId, onChanged]);

  const confirmDelete = useCallback(() => {
    if (deleting === null) {
      return;
    }
    const target = deleting;
    setDeleting(null);
    setNotice(null);
    sessionsSpaceDelete(rootId, target.id)
      .then(onChanged)
      .catch((raw: unknown) => setNotice(syncErrorMessage(raw, SESSION_SPACE_DELETE_FAILED)));
  }, [deleting, rootId, onChanged]);

  const edited = spaces.find((space) => space.id === editing) ?? null;
  // One clock read per render, handed down — a row that called `Date.now()`
  // itself would make two files written in the same second disagree about how
  // long ago that was (the tree's own rule).
  const nowMs = Date.now();

  return (
    <section aria-label={SESSION_SPACES_HEADING} className="flex flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <h3 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
          {SESSION_SPACES_HEADING}
        </h3>
        <span className="flex-1" />
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={SESSION_SPACES_NEW}
          title={SESSION_SPACES_NEW}
          onClick={() => setCreating(true)}
          className="h-7 px-2"
        >
          <FolderPlus aria-hidden="true" className="size-3.5" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={SESSION_SPACES_RESTORE}
          title={SESSION_SPACES_RESTORE}
          disabled={restoring}
          onClick={restore}
          className="h-7 px-2"
        >
          <RotateCcw aria-hidden="true" className="size-3.5" />
        </Button>
      </div>
      <p className="text-muted-foreground text-xs">{SESSION_SPACES_HINT}</p>
      {notice !== null && (
        // A live region: restore and delete both answer here, and both are
        // presses whose whole result is a sentence.
        <p role="status" className="text-muted-foreground text-xs">
          {notice}
        </p>
      )}

      {spaces.length === 0 ? (
        <p className="text-muted-foreground text-xs">{SESSION_SPACES_EMPTY}</p>
      ) : (
        spaces.map((space) => (
          <SpaceSection
            key={space.id}
            space={space}
            selection={byId.get(space.id) ?? null}
            loading={selections === null}
            nowMs={nowMs}
            rootId={rootId}
            onEdit={() => setEditing(space.id)}
            onDelete={() => setDeleting(space)}
          />
        ))
      )}

      {/* Keyed on the space, so opening a second editor after the first seeds
          its form from the right space rather than from stale state. */}
      {edited !== null && (
        <SessionSpaceEditor
          key={edited.id}
          rootId={rootId}
          space={edited}
          vocabulary={vocabulary}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            onChanged();
          }}
        />
      )}
      {creating && (
        <SessionSpaceEditor
          rootId={rootId}
          space={null}
          vocabulary={vocabulary}
          onClose={() => setCreating(false)}
          onSaved={() => {
            setCreating(false);
            onChanged();
          }}
        />
      )}

      <AlertDialog open={deleting !== null} onOpenChange={(open) => !open && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{SESSION_SPACE_DELETE_TITLE}</AlertDialogTitle>
            <AlertDialogDescription>{SESSION_SPACE_DELETE_BODY}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>
              {SESSION_SPACE_DELETE_CONFIRM}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}

function SpaceSection({
  space,
  selection,
  loading,
  nowMs,
  rootId,
  onEdit,
  onDelete,
}: {
  space: SessionSpaceVm;
  selection: SessionSpaceFilesVm | null;
  loading: boolean;
  nowMs: number;
  rootId: string;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const broken = space.error !== null;
  const misread = space.warnings.length > 0;
  const subtitle = broken
    ? SESSION_SPACE_BROKEN_SUBTITLE
    : misread
      ? SESSION_SPACE_SETTINGS_SUBTITLE
      : null;
  const Glyph = spaceIcon(space.icon);
  const files = selection?.files ?? [];

  return (
    <div
      data-testid={`${SESSION_SPACE_SECTION_TESTID}-${space.id}`}
      className="group flex flex-col gap-1 rounded-md border border-border px-3 py-2"
    >
      <div className="flex min-w-0 items-center gap-2">
        {/* A space whose query keeper cannot read is a fault and gets the fault
            lamp; a healthy one gets a spacer of the same width so the glyph
            column does not shuffle sideways. The failure is already in the
            heading's own text below, so the lamp stays silent rather than
            saying it twice (UX-DR43). */}
        {broken ? (
          <Lamp state="fault" label={null} data-slot="space-dot" />
        ) : (
          <span aria-hidden="true" data-slot="space-dot" className="size-1.5 shrink-0" />
        )}
        <Glyph
          aria-hidden="true"
          data-slot="space-icon"
          data-space-icon={space.icon ?? "none"}
          className="size-4 shrink-0 text-muted-foreground"
        />
        <h4 className="min-w-0 truncate font-medium text-sm">{space.name}</h4>
        {!broken && !loading && (
          <span className="shrink-0 text-muted-foreground text-xs">{files.length}</span>
        )}
        <span className="flex-1" />
        {/* Always in the DOM, revealed on hover or focus: an affordance that
            only exists under a pointer is one a keyboard cannot reach. The
            destructive one is last, so a hand travelling along the row reaches
            edit before delete. */}
        <button
          type="button"
          aria-label={`${SESSION_SPACE_EDIT} ${space.name}`}
          onClick={onEdit}
          className={cn(
            "shrink-0 rounded-md p-1 text-muted-foreground outline-none",
            "opacity-0 focus-visible:opacity-100 group-hover:opacity-100",
            "hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
          )}
        >
          <Pencil aria-hidden="true" className="size-3.5" />
        </button>
        <button
          type="button"
          aria-label={`${SESSION_SPACE_DELETE} ${space.name}`}
          onClick={onDelete}
          className={cn(
            "shrink-0 rounded-md p-1 text-muted-foreground outline-none",
            "opacity-0 focus-visible:opacity-100 group-hover:opacity-100",
            "hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
          )}
        >
          <Trash2 aria-hidden="true" className="size-3.5" />
        </button>
      </div>

      {subtitle !== null && (
        <p
          data-slot="space-subtitle"
          // The whole of what Rust said, on the title, for a pointer; the
          // keyboard path to it is the pencil, whose form lists every warning in
          // full. A line in a section this narrow cannot hold one of them, and
          // the editor is where the value gets fixed anyway.
          title={misread ? space.warnings.join(" ") : (space.error ?? undefined)}
          className="text-destructive text-xs"
        >
          {subtitle}
        </p>
      )}
      {/* The selection's own error, when it has one — an empty query, which
          parses fine and still selects nothing, so it never reaches `error`
          above. Rust worded it; this prints it rather than inventing a second
          sentence for the same state. */}
      {selection?.error != null && selection.error !== space.error && (
        <p className="text-destructive text-xs">{selection.error}</p>
      )}

      {loading ? (
        <p className="text-muted-foreground text-xs">{SESSION_SPACES_LOADING}</p>
      ) : files.length === 0 ? (
        !broken &&
        selection?.error == null && (
          <p className="text-muted-foreground text-xs">{SESSION_SPACES_NO_FILES}</p>
        )
      ) : (
        <ul aria-label={space.name} className="flex flex-col">
          {files.map((file) => (
            <li key={file.relPath} data-testid={`${SESSION_SPACE_FILE_TESTID}-${file.relPath}`}>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                // Path-identified rather than id-identified is recorded, not
                // drawn: keeper never stamps an `id:` into a file it did not
                // author, so in a zone of hand-written markdown this would be
                // true of nearly every row — a badge on all of them is decor,
                // not information. It reaches a test and a stylesheet here, and
                // reaches a person only where it changes what they can do (a
                // rename breaks the reference), which is not this list.
                data-unstable-identity={file.unstableIdentity ? "" : undefined}
                className="h-7 w-full min-w-0 justify-start gap-2 px-2 font-normal"
                onClick={() =>
                  panelsStore.getState().setActiveTarget({
                    kind: "file",
                    profileId: rootId,
                    relativePath: file.subpath,
                  })
                }
              >
                <span className="min-w-0 flex-1 truncate text-sm">{file.title}</span>
                <span className="figures shrink-0 text-muted-foreground text-xs">
                  {formatDraftAge(file.mtimeMs, nowMs)}
                </span>
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
