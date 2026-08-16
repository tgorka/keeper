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
 * Every row opens on the `subpath` Rust composed (AD-65) — this file never
 * joins a path and never writes a query. WHICH surface it opens in is Story
 * 45.18's question, not a second one: a file that lives inside a notes vault
 * opens as its note, and one that does not keeps the ONE file target the tree
 * and the Files pane use (AD-109, UX-DR91). Being outside a vault is an
 * ordinary configuration, not a failure, so the fallback says nothing.
 *
 * A space can also be written INTO (Story 49.2, FR-273): when Rust says the
 * query names exactly one creatable kind AND the session follows the flat
 * contract, the header carries a create control that writes a file that space
 * will list. The kind arrives on the VM — TypeScript never parses
 * `keeper.space`, the rule the notes actions already state.
 *
 * The shape half of that gate is the sibling verb's, for the sibling verb's
 * reason (`session-file-actions.tsx` gates `New prompt` the same way): a
 * folder-shaped session's pool is `README.md` plus `refs/` and `prompts/`
 * (`sessions_root.rs::read_ref_sources`), while `sessions_file_new_kind` writes
 * a stamped file into the session ROOT. Offering the control there would write
 * a file no space in that session can ever list — a create whose whole result
 * is invisible. Absent, not disabled: the `showNoteInFiles` precedent again.
 */
import { FilePlus, FolderPlus, Pencil, RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
import type { NoteVaultVm, SessionSpaceFilesVm, SessionSpaceVm } from "@/lib/ipc/client";
import { sessionsFileNewKind, sessionsSpaceDelete, sessionsSpacesRestore } from "@/lib/ipc/client";
import {
  ensureNotesVaultsHydrated,
  notesVaultsStore,
  useNotesVaultsStore,
} from "@/lib/stores/notes-vaults";
import { panelsStore } from "@/lib/stores/panels";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";
import { notePathForFile, openNoteForFile } from "@/lib/vault-link";

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

/**
 * The create control's accessible name, suffixed with the space (Story 49.2).
 *
 * "in Tasks" rather than "New task": the person is looking at a section called
 * Tasks, and naming the space is what tells them WHERE the file will land. The
 * kind is Rust's word, not a noun this file is free to inflect.
 */
export const SESSION_SPACE_NEW_NOTE = "New note in";

/** What the create says when keeper could not write. */
export const SESSION_SPACE_NEW_NOTE_FAILED =
  "keeper couldn't create that note. Nothing was written.";

/**
 * What a row says when keeper could not read the vault list at all.
 *
 * Being outside every vault is a configuration and stays silent; not KNOWING
 * is a failure and does not. Without this sentence the two collapse into the
 * same silent file target, and the person whose zone IS a vault is shown the
 * file surface with nothing on screen saying why — the exact behaviour Story
 * 49.2 exists to remove, wearing the fallback's clothes.
 */
export const SESSION_SPACE_VAULTS_UNKNOWN =
  "keeper couldn't read the notes vaults, so this opened as a file.";

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
  /**
   * The session these spaces are read against — what a create writes into.
   *
   * The definitions belong to the zone and the selections to the session, so
   * the section needs both ids the moment it can write: `sessions_file_new_kind`
   * puts the file in this session's pool, not in the zone's.
   */
  sessionId: string;
  /**
   * Which contract this session follows — the create control branches on it.
   *
   * The section itself renders under both shapes: a folder-shaped session's
   * pool is still read through the zone's spaces, so the LISTINGS are true
   * there. Writing is what is not. See the shape paragraph at the top.
   */
  shape: string;
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

export function SessionSpaces({
  rootId,
  sessionId,
  shape,
  spaces,
  selections,
  onChanged,
}: SessionSpacesProps) {
  const [editing, setEditing] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  /**
   * ONE create in flight across the whole section, not one per space.
   *
   * `sessions_file_new_kind` names the file from the clock to the minute and
   * from an empty title, so two creates started in the same minute compute
   * `taken_in` before either write lands and both resolve to
   * `YYYY-MM-DD-HHMM-untitled.md` — `files::compile_new` emits a plain
   * `WriteFile`, so the second silently overwrites the first and the `tag:
   * task` file becomes a `tag: log` one. Per-space flags are what made that
   * reachable from the UI; the Files heading has always shared one `busy` for
   * the same reason (`session-file-actions.tsx`). Serialising in Rust is the
   * real fix and is that crate's to make; one flag here is what removes the
   * press a person can actually perform.
   */
  const [writing, setWriting] = useState(false);
  const [deleting, setDeleting] = useState<SessionSpaceVm | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const vaults = useNotesVaultsStore((each) => each.vaults);
  /** Which row press is the current one — see {@link openSpaceFile}. */
  const openSeq = useRef(0);

  // The same hydration the text viewer does, for the same reason: Sessions can
  // be the first surface a window opens and nothing else here reads the vault
  // list, so without this every row would resolve to "no vault" until the
  // person had visited Notes — a feature that works for whoever wrote it and
  // nobody else.
  useEffect(() => {
    void ensureNotesVaultsHydrated();
  }, []);

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

  /**
   * Open one of this session's files — the ONE opener the row click and the
   * create both go through (Story 49.2, FR-274).
   *
   * **Note first, file honestly.** `notePathForFile` is synchronous precisely
   * so a caller can ask whether the note EXISTS before committing to a
   * surface: inside a registered vault the full editor is what the person
   * meant by "open the note", and outside every vault the file target is not a
   * degraded answer, it is the only correct one — which is why that arm says
   * nothing. Guessing the notes view for a zone that is not a vault would land
   * them in a pane with nothing in it.
   *
   * **`null` vaults is not an empty vault list.** `notes-vaults.ts` keeps that
   * distinction on purpose — "you have no vault" versus "keeper has not
   * looked" — and collapsing it here would open the FILE surface for a
   * vault-backed zone during the hydration window, and forever after a
   * rejected `notes_vaults` read, since the mount's one best-effort attempt
   * leaves the mirror unhydrated. So an unknown list is awaited rather than
   * read as empty, and if it is STILL unknown the fallback speaks
   * ({@link SESSION_SPACE_VAULTS_UNKNOWN}) instead of pretending to be the
   * ordinary out-of-vault case.
   *
   * A vault-backed file that the index cannot place still opens: `openNoteForFile`
   * hands back Story 45.18's own sentence, this prints it, and the file viewer
   * shows the bytes. Refusing to open anything because the nicer surface was
   * unavailable would be keeper withholding a file it can read.
   *
   * **`"replace"`, both arms.** A row is a single click and AD-90 gives a
   * single click the replace gesture; the file arm always used it, so the note
   * arm asks for it too. Otherwise one click grows the strip inside a vault
   * and replaces outside it.
   */
  const openSpaceFile = useCallback(
    (subpath: string) => {
      // The `requestSeq` idiom this codebase already uses for a race it cannot
      // order (`command-palette.tsx`, `recording-audio-controls.tsx`): resolving
      // a row costs one or two IPC round trips whose price depends on how
      // crowded that file's own vault directory is, so two rows clicked in
      // succession really can finish out of order — and the loser would take
      // the panel, the active vault, the primary view and the notice with it.
      openSeq.current += 1;
      const press = openSeq.current;
      // Superseded is `press !== openSeq.current`, checked at every point this
      // resolution would touch shared state.
      const asFile = () =>
        panelsStore.getState().setActiveTarget({
          kind: "file",
          profileId: rootId,
          relativePath: subpath,
        });
      const decide = (known: readonly NoteVaultVm[]) => {
        if (press !== openSeq.current) {
          return;
        }
        if (notePathForFile(known, rootId, subpath) === null) {
          asFile();
          return;
        }
        void openNoteForFile(known, rootId, subpath, {
          gesture: "replace",
          // Inside the bridge too, not only out here: the vault switch and the
          // panel target both happen in there, past the awaits.
          stillWanted: () => press === openSeq.current,
        }).then((sentence) => {
          if (press !== openSeq.current || sentence === null) {
            return;
          }
          setNotice(sentence);
          asFile();
        });
      };
      // A new press, so whatever the last one said is no longer the answer.
      setNotice(null);
      if (vaults !== null) {
        decide(vaults);
        return;
      }
      void ensureNotesVaultsHydrated().then(() => {
        const known = notesVaultsStore.getState().vaults;
        if (known !== null) {
          decide(known);
          return;
        }
        if (press !== openSeq.current) {
          return;
        }
        setNotice(SESSION_SPACE_VAULTS_UNKNOWN);
        asFile();
      });
    },
    [vaults, rootId],
  );

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
            sessionId={sessionId}
            shape={shape}
            writing={writing}
            onWriting={setWriting}
            onOpen={openSpaceFile}
            onNotice={setNotice}
            onChanged={onChanged}
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
  sessionId,
  shape,
  writing,
  onWriting,
  onOpen,
  onNotice,
  onChanged,
  onEdit,
  onDelete,
}: {
  space: SessionSpaceVm;
  selection: SessionSpaceFilesVm | null;
  loading: boolean;
  nowMs: number;
  rootId: string;
  sessionId: string;
  /** The session's contract — the create is a flat-shape verb. */
  shape: string;
  /** Whether ANY space in this section already has a create in flight. */
  writing: boolean;
  /** Claim or release that one flag. */
  onWriting: (writing: boolean) => void;
  /** The section's one opener — a row press and a create both end here. */
  onOpen: (subpath: string) => void;
  /** The section's live region: every sentence a verb produces goes there. */
  onNotice: (notice: string | null) => void;
  /** Re-read, because the new file is in the other payload. */
  onChanged: () => void;
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
  // Rust's answer, and the only thing that decides whether this space can be
  // written into — the query above is never parsed here.
  const kind = space.newFileKind;
  // …and the shape, which the query cannot know about. A folder-shaped
  // session's pool excludes root-level markdown, and the root is where
  // `sessions_file_new_kind` writes, so a create there produces a file this
  // space can never list. `New prompt` refuses on the same shape for the same
  // family of reason.
  const creatable = kind !== null && shape === "flat";

  /**
   * Write a file this space will list, and open it (Story 49.2, FR-273).
   *
   * **No title dialog, mirroring `session-file-actions.tsx`'s `newKind`.** New
   * log and New prompt already write with an empty title and let Rust name the
   * file `untitled`; the person then types the real title into the note they
   * are now looking at. A second, differently-shaped naming flow for the same
   * verb would make the two buttons disagree about what pressing "new" means.
   *
   * `onChanged` before the open, because the definitions and the selections are
   * two payloads: only the re-read puts the new row in this space.
   */
  const newNote = useCallback(() => {
    if (kind === null) {
      return;
    }
    onWriting(true);
    onNotice(null);
    sessionsFileNewKind(rootId, sessionId, kind, "")
      .then((subpath) => {
        onChanged();
        onOpen(subpath);
      })
      // Named, because the live region this lands in sits under the `Spaces`
      // heading — above every section, and possibly a scroll away from the
      // button that was pressed. With two creatable spaces on screen the
      // unprefixed sentence cannot say which one failed. The rejected
      // alternative was a second live region inside the section: more DOM, one
      // more thing for a screen reader to find, and Story 49.3 moves this
      // section's sentences into the fold's `notice` anyway.
      .catch((raw: unknown) =>
        onNotice(
          `${SESSION_SPACE_NEW_NOTE} ${space.name}: ${syncErrorMessage(raw, SESSION_SPACE_NEW_NOTE_FAILED)}`,
        ),
      )
      .finally(() => onWriting(false));
  }, [kind, rootId, sessionId, space.name, onOpen, onNotice, onChanged, onWriting]);

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
            edit before delete, and create is first because it is what a person
            comes to a space to do — the other two are maintenance.

            Three controls and no more: the row already holds a dot, a glyph, a
            truncating name and a count, and at the narrowest card this app
            draws a fourth button would leave the space's own name about a word
            wide. When the space cannot be written into, create is ABSENT and
            not disabled — a control that exists only to refuse teaches nothing
            (the `showNoteInFiles` precedent). That covers both halves of
            `creatable`: no kind, and the folder contract. */}
        {creatable && (
          <button
            type="button"
            aria-label={`${SESSION_SPACE_NEW_NOTE} ${space.name}`}
            onClick={newNote}
            disabled={writing}
            className={cn(
              "shrink-0 rounded-md p-1 text-muted-foreground outline-none",
              "opacity-0 focus-visible:opacity-100 group-hover:opacity-100",
              "hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
            )}
          >
            <FilePlus aria-hidden="true" className="size-3.5" />
          </button>
        )}
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
                onClick={() => onOpen(file.subpath)}
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
