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
 * joins a path and never writes a query. It opens the ONE file target the tree
 * and the Files pane use (AD-109, UX-DR91), and only that.
 *
 * **There is no "open it as a note" arm, and there cannot be.** Story 49.2
 * added one; Story 50.1 removed it because no configuration reaches it.
 * `@/lib/vault-link` resolves a file to a note only when a notes vault CONTAINS
 * it, and `SessionsConfig::validate` (`keeper-sync/src/profile/mod.rs:648-654`)
 * refuses a sessions zone that overlaps a notes vault in either direction —
 * "one folder cannot be both a vault and a sessions zone". A session file is
 * therefore never a vault note, and a branch no configuration can reach is a
 * claim the code cannot keep. The text viewer still uses the bridge, for files
 * that genuinely are in a vault.
 *
 * A space can also be written INTO (Story 49.2, FR-273; Story 50.1, FR-277):
 * when Rust says the query names exactly one creatable kind, the header carries
 * a create control that writes a file that space will list. The kind arrives on
 * the VM — TypeScript never parses `keeper.space`, the rule the notes actions
 * already state.
 *
 * **Where that file goes is the shape's answer, and Rust's.**
 * `sessions_file_new_kind` asks `keeper_core::sessions::shape::kind_dir`, which
 * keeps a flat session's files at its root and a folder-shaped session's
 * references in `refs/` and prompts in `prompts/` — the directories that
 * shape's pool actually reads. Until 50.1 it always wrote the root, so this
 * section suppressed the control on every folder-shaped session; that gate is
 * gone because the write is fixed. What survives is the narrower fact that a
 * shape can keep no home for a kind at all — the folder contract has no tasks
 * file, and its log is a `## Log` heading rather than a file — and there the
 * section says so in one line rather than offering nothing and explaining
 * nothing.
 *
 * **That line is Rust's, not this file's.** `sessions_space_files` asks
 * `kind_dir` for this session's shape and puts `KindHasNoHome`'s own sentence
 * on `SessionSpaceFilesVm.noHome`; the section prints it. This file holds no
 * reading of the mapping and no wording of the refusal — the TypeScript mirror
 * that used to live here was a second reader of one contract, and it had
 * already forked the sentence on the day it was written. The shape does not
 * travel to this component at all any more.
 *
 * **The create is the one control here that does not hide.** Edit and Delete
 * are maintenance and keep the rail's hover-reveal (`space-list.tsx`); create
 * is the verb a person comes to a space for, and the report that opened Story
 * 50.1 was literally "I don't see the button". The session create verbs the
 * same person already knows (`session-file-actions.tsx`) are always-visible
 * labelled buttons.
 *
 * And a space can be SHUT (Story 49.3, FR-275, FR-276). Each one renders
 * through `FoldSection`, the app's one fold mechanism, with its title as the
 * disclosure and its header — count, lamp, create, edit, delete, and whatever
 * they had to say — outside the folded region. Where the fold is remembered and
 * what an untouched space does are two different questions with two different
 * answers, and {@link "@/lib/stores/session-spaces-fold"} holds both: a cookie
 * for the spaces this person arranged, and `sessions.spaces_folded` for the
 * ones they never touched.
 */
import { FilePlus, FolderPlus, Pencil, RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { FoldSection } from "@/components/layout/sidebar-group";
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
import { sessionsFileNewKind, sessionsSpaceDelete, sessionsSpacesRestore } from "@/lib/ipc/client";
import { panelsStore } from "@/lib/stores/panels";
import {
  isSpaceFolded,
  setSpaceFolded,
  spaceFoldKey,
  useSessionSpacesFold,
} from "@/lib/stores/session-spaces-fold";
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

/** What the delete confirmation asks and answers. */
export const SESSION_SPACE_DELETE_TITLE = "Delete this space?";
export const SESSION_SPACE_DELETE_BODY =
  "The definition goes to the trash — the files it listed are untouched. Every session in this zone stops showing the section.";
export const SESSION_SPACE_DELETE_CONFIRM = "Delete space";
export const SESSION_SPACE_DELETE_FAILED = "keeper couldn't delete this space.";

/** One row, for tests that need to find one by path. */
export const SESSION_SPACE_FILE_TESTID = "session-space-file";

/**
 * The id prefix of one space's folded region (Story 49.3).
 *
 * The disclosure's `aria-controls` points at it, so it has to be unique in the
 * document — a space id is, within a zone, and only one zone's spaces are on
 * screen at a time. It replaces the section's old `data-testid`: the section is
 * now a `<section aria-label={space.name}>`, which a test finds by its name.
 *
 * **The space id is percent-encoded onto it, never pasted.** A space id is its
 * zone-relative path — `_spaces/<filename>.md` — and a hand-written space file
 * may be called `my tasks.md`, so the pasted form `session-space-_spaces/my
 * tasks.md` is TWO IDREFs to an HTML parser: `aria-controls` then resolves to
 * nothing and the disclosure controls nothing for assistive technology while
 * working perfectly for a pointer, which is the worst shape this bug could
 * have. `encodeURIComponent` and not a slug: it is reversible, so two spaces
 * whose names differ only in the characters a slug would strip — `my tasks.md`
 * and `my-tasks.md` — keep two different ids instead of both pointing at the
 * first region rendered.
 */
export const SESSION_SPACE_FOLD_ID = "session-space";

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
   * Whether a create is already in flight ANYWHERE on this session, and the
   * way to claim or release that.
   *
   * **One flag across two components, held by `SessionDetail`.**
   * `sessions_file_new_kind` names the file from the clock to the minute and
   * from an empty title, so two creates started in the same minute compute
   * `taken_in` before either write lands and both resolve to
   * `YYYY-MM-DD-HHMM-untitled.md` — `files::compile_new` emits a plain
   * `WriteFile`, so the second silently overwrites the first and the `tag:
   * task` file becomes a `tag: log` one.
   *
   * The Files heading (`session-file-actions.tsx`) offers *New log* and *New
   * prompt* on the same session at the same time, and both post an empty title
   * through the same command — so a flag private to this section would leave
   * "New prompt up there, New note down here, in the same minute" as a press a
   * person can actually perform. Two sibling `useState`s were exactly that.
   * Serialising in Rust is the real fix and is that crate's to make; one flag
   * on their common parent is what removes the reachable press.
   */
  writing: boolean;
  onWriting: (writing: boolean) => void;
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
  writing,
  onWriting,
  spaces,
  selections,
  onChanged,
}: SessionSpacesProps) {
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

  /**
   * Open one of this session's files — the ONE opener the row click and the
   * create both go through (FR-274).
   *
   * The ONE file target the tree and the Files pane use (AD-109, UX-DR91), on
   * the `subpath` Rust composed (AD-65): a second path-join here would be a
   * second answer to where a file lives.
   *
   * **Story 49.2's note arm is gone (Story 50.1).** It opened a row as a vault
   * note whenever `@/lib/vault-link` placed the file inside a registered vault
   * — and `SessionsConfig::validate` refuses a sessions zone that overlaps a
   * notes vault in either direction, so that placement is a configuration the
   * product does not allow to exist. With the arm went the press guard and the
   * vault-list hydration it needed: this opener is one synchronous store write
   * with nothing to race and nothing to say.
   *
   * `"replace"` is `setActiveTarget`'s own gesture, which is what AD-90 gives a
   * single click on a list row.
   */
  const openSpaceFile = useCallback(
    (subpath: string) => {
      panelsStore.getState().setActiveTarget({
        kind: "file",
        profileId: rootId,
        relativePath: subpath,
      });
    },
    [rootId],
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
            writing={writing}
            onWriting={onWriting}
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
  /** Whether ANY create on this session already has one in flight. */
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
  // Rust's answer, and the only thing that decides whether this space's QUERY
  // can be written into — the query is never parsed here.
  const kind = space.newFileKind;
  // …and whether this session's contract keeps anywhere to put that kind, in
  // Rust's own words. `shape::kind_dir` decides it, `sessions_space_files`
  // projects its `KindHasNoHome` sentence onto this payload, and this file
  // prints it. There is deliberately no TypeScript reading of the mapping: a
  // second one drifts, and the copy that lived here had already forked the
  // refusal's wording on the day it was written.
  //
  // Absent AND explained, rather than absent in silence: a section that offers
  // nothing and says nothing is the report that opened this story. While the
  // selections have not arrived there is no answer yet, so the verb waits with
  // the rows rather than appearing and then vanishing.
  const noHome = selection?.noHome ?? null;
  const creatable = kind !== null && selection !== null && noHome === null;

  // Whether this space is folded, and where that is remembered (Story 49.3,
  // FR-275). Keyed by root and space id rather than by session: the definition
  // belongs to the zone, so a person who shut Tasks meant Tasks, not Tasks in
  // this one session. A space with nothing recorded follows
  // `sessions.spaces_folded`, which is what {@link isSpaceFolded} composes.
  const foldKey = spaceFoldKey(rootId, space.id);
  const folded = useSessionSpacesFold((state) => isSpaceFolded(state, foldKey));

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
    <FoldSection
      label={space.name}
      // The space's OWN glyph, not a chevron — the one place this section
      // departs from the notes rail's three folds. At ~208px of card width the
      // header already carries a count and three controls, and a chevron
      // beside the glyph would spend 14 more of the pixels the space's name is
      // truncating out of (Story 49.2's arithmetic). What tells a person a
      // folded space is folded rather than empty is the count, which stays in
      // the header: "12" over no rows is a fold, and an empty space says so in
      // words. `aria-expanded` states it outright for anyone not reading
      // pixels.
      icon={Glyph}
      // The two attributes the notes rail's glyph carries (`space-list.tsx`),
      // because they say what the pixels cannot: `data-space-icon` is the
      // STORED name, and an icon this build no longer knows draws the same
      // fallback glyph as no icon at all.
      iconProps={{ "data-slot": "space-icon", "data-space-icon": space.icon ?? "none" }}
      folded={folded}
      onToggle={() => setSpaceFolded(foldKey, !folded)}
      // Encoded, not pasted: a space id is a path and may hold whitespace —
      // see {@link SESSION_SPACE_FOLD_ID}.
      id={`${SESSION_SPACE_FOLD_ID}-${encodeURIComponent(space.id)}`}
      // A card title, not the register's label: this name is what the person
      // typed into `keeper.space`, and 11px uppercase would shout it in the
      // same voice as the SPACES heading above it.
      labelClassName="font-medium text-sm"
      className="group gap-1 rounded-md border border-border px-3 py-2"
      bodyClassName="flex flex-col gap-1"
      actions={
        <>
          {/* A space whose query keeper cannot read is a fault and gets the
              fault lamp; a healthy one gets a spacer of the same width so the
              count does not shuffle sideways. The failure is already in the
              subtitle's own text, so the lamp stays silent rather than saying
              it twice (UX-DR43). */}
          {broken ? (
            <Lamp state="fault" label={null} data-slot="space-dot" />
          ) : (
            <span aria-hidden="true" data-slot="space-dot" className="size-1.5 shrink-0" />
          )}
          {!broken && !loading && (
            <span className="shrink-0 text-muted-foreground text-xs">{files.length}</span>
          )}
          {/* Edit and Delete are in the DOM always and revealed on hover or
              focus — the notes rail's pattern for a per-space row control
              (`space-list.tsx`), and `focus-visible:opacity-100` is what keeps
              an affordance that only exists under a pointer reachable from a
              keyboard. The destructive one is last, so a hand travelling along
              the row reaches edit before delete.

              **Create does not hide** (Story 50.1). It is first because it is
              what a person comes to a space to DO, and hover-reveal is the
              wrong pattern for the one verb a section exists to offer: the
              report that opened this story was "I don't see the button", and
              the session create verbs the same person already knows
              (`session-file-actions.tsx`) are always-visible labelled buttons.
              Maintenance may hide; the verb may not.

              In `actions`, which is OUTSIDE the folded region (Story 49.3,
              `sidebar-group.tsx:20-25`): a space you have shut is still a space
              you can write into, and a create whose button vanished with the
              rows would make folding a way to lose a verb.

              Three controls and no more: the row already holds a glyph, a
              truncating name, a dot and a count, and at the narrowest card this
              app draws a fourth button would leave the space's own name about a
              word wide — which is also why the disclosure is the title rather
              than a fourth button. When the space cannot be written into,
              create is ABSENT and not disabled — a control that exists only to
              refuse teaches nothing (the `showNoteInFiles` precedent) — and
              when the reason is this session's shape, {@link noHome} says it in
              the notice below rather than leaving a gap. */}
          {creatable && (
            <button
              type="button"
              aria-label={`${SESSION_SPACE_NEW_NOTE} ${space.name}`}
              onClick={newNote}
              disabled={writing}
              className={cn(
                "shrink-0 rounded-md p-1 text-muted-foreground outline-none",
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
        </>
      }
      // Outside the folded region, with the header controls it belongs to: a
      // space that is broken is broken whether or not its rows are showing, and
      // a fault whose only explanation folds away with the thing it is about is
      // a lamp nobody can read.
      notice={
        <>
          {subtitle !== null && (
            <p
              data-slot="space-subtitle"
              // The whole of what Rust said, on the title, for a pointer; the
              // keyboard path to it is the pencil, whose form lists every warning
              // in full. A line in a section this narrow cannot hold one of them,
              // and the editor is where the value gets fixed anyway.
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
          {/* Why this space offers no create, when the reason is the session's
              own contract rather than its query. Muted, not destructive:
              nothing is broken and nothing failed — this shape simply keeps
              that kind somewhere other than in a file, and the person is
              entitled to know which. */}
          {noHome !== null && (
            <p data-slot="space-no-home" className="text-muted-foreground text-xs">
              {noHome}
            </p>
          )}
        </>
      }
    >
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
    </FoldSection>
  );
}
