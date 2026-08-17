/**
 * The session's spaces (FR-261, AD-121) — the saved queries a zone reads every
 * one of its sessions through.
 *
 * The tree below lists what the session *holds*, in the order the filesystem
 * hands it over. This lists the same files *by what they are*: the log entries
 * together and newest first, the tasks in board order, the references
 * alphabetically. In the flat contract there are no `logs/` or `refs/` folders
 * to make that grouping for free, so these sections ARE the folders — with the
 * advantage that a file can be in two of them and the disadvantage, stated
 * plainly on the tree itself, that on disk it is one pile of markdown.
 *
 * **Above the files, deliberately** (Story 52.4, the operator's own
 * instruction: *"umiesc spaces ponad files"*). It used to sit after them, and
 * the reason written here was that the tree is the ground truth and this is a
 * reading of it, so a person scanning down should meet the contents before
 * meeting keeper's opinions about them. That argument is about which is more
 * FUNDAMENTAL; the order is about which is read more OFTEN, and this is what a
 * person opens a session to read. The tree is where they go when a space has
 * not surfaced something — which is the second question, and now sits second.
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
 * **The create is the one control here that does not hide, and it is present
 * even where it must refuse** (Story 52.4). Edit and Delete are maintenance and
 * keep the rail's hover-reveal (`space-list.tsx`); create is the verb a person
 * comes to a space for, and the report that opened Story 50.1 was literally "I
 * don't see the button". The report that opened 52.4 was its sibling — *"about
 * space nie ma przycisku dodaj jak inne"* — so where Rust has a REASON there is
 * now a disabled control carrying that reason as its accessible description,
 * rather than a gap a person has to interpret. Absent still, and only, where
 * Rust has neither a kind nor a sentence: a control that refuses without
 * explaining teaches nothing. The session create verbs the same person already
 * knows (`session-file-actions.tsx`) are always-visible labelled buttons.
 *
 * And a space can be SHUT (Story 49.3, FR-275, FR-276). Each one renders
 * through `FoldSection`, the app's one fold mechanism, with its title as the
 * disclosure and its header — count, lamp, create, edit, delete, and whatever
 * they had to say — outside the folded region. Where the fold is remembered and
 * what an untouched space does are different questions with different answers,
 * and {@link "@/lib/stores/session-spaces-fold"} holds the composition: a cookie
 * for the spaces this person arranged, the space's own `keeper.folded` for a
 * definition that says how it opens, and `sessions.spaces_folded` for the ones
 * nobody has answered for at all (Story 51.3, FR-289).
 *
 * **A space can also say how MUCH it shows** (FR-290), and the one thing to keep
 * straight is that `keeper.rows` caps the rows this file paints and never the
 * selection Rust made. The header's count stays the whole selection and the
 * remainder folds behind a *Show N more*, so a capped section always says how
 * much it is not showing. A notes space's `keeper.limit` is the other feature —
 * it narrows the query — and blurring the two here would turn a presentation
 * choice into a filter that hides work.
 */
import { FilePlus, FileText, FolderPlus, Pencil, RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useId, useMemo, useState } from "react";
import { FoldSection } from "@/components/layout/sidebar-group";
import { spaceIcon } from "@/components/notes/space-icons";
import { SessionSpaceEditor } from "@/components/sessions/session-space-editor";
import { SpaceRowMenu } from "@/components/sessions/space-row-menu";
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
import {
  sessionsFileNewKind,
  sessionsSpaceDelete,
  sessionsSpaceNarrow,
  sessionsSpacesRestore,
} from "@/lib/ipc/client";
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
 * The seeded default that shows what declares no kind (Story 52.4).
 *
 * **The identity is `defaultKey`, never the name** — `@/lib/recordings-space`'s
 * rule, for its reason: a default space is renameable like any other (AD-79), so
 * matching on "Untagged" would change this section's behaviour for anyone who
 * called theirs "Loose ends", and a space of the operator's own that happens to
 * be called Untagged would borrow a rule it was never given.
 *
 * The rule it keys is that this one space renders **only when it selected
 * something**. Every other space is a question worth seeing answered — an empty
 * Tasks says the session has no tasks and offers to make one — but this space's
 * whole subject is a residue, and there is nothing to say about a residue that
 * does not exist. It is also the one space keeper can be sure of that about,
 * because its query names no kind and can therefore never be written into: an
 * empty section here offers nothing and asks for nothing.
 *
 * The string is `DEFAULT_SESSION_SPACES`' own key in
 * `keeper-core/src/sessions/spaces.rs`, which Rust writes into the file's
 * `keeper.default` and nothing else writes. `renders no Untagged section when
 * every file declares a kind` is the witness on this side of that boundary.
 */
export const SESSION_SPACE_UNTAGGED_KEY = "untagged";

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
 * The repair an over-specified space offers, suffixed with the query it will
 * write (Story 53.4, FR-319).
 *
 * **It says what it will do before it is pressed**, which is what makes it a
 * repair rather than a settings toggle: the query comes from Rust on the
 * selection's `narrowTo`, so the string a person reads on the control and the
 * string that lands in `keeper.space` are one value from one function (AD-65).
 * "Fix this space" was the rejected wording — it names no outcome, and this
 * control writes to a file the operator owns.
 *
 * It is offered only where Rust offered it: beside the arity refusal, on a space
 * claiming a default whose query asks for a single `tag:` term. A space that
 * claims nothing has no authority for a term and gets no control — the editor,
 * which the pencil already opens, is the answer there.
 */
export const SESSION_SPACE_NARROW = "Narrow this space to";

/** What the repair says when it wrote, in the notice the section already has. */
export const SESSION_SPACE_NARROWED = "Narrowed";

/** What the repair says when keeper could not write. */
export const SESSION_SPACE_NARROW_FAILED =
  "keeper couldn't narrow this space. Nothing was written.";

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

/**
 * The row cap's control, both directions (Story 51.3, FR-290).
 *
 * "Show 7 more" and not the sync card's "Show all 12": there the unfolded size
 * is a fixed setting, so the button can only honestly name the size it will grow
 * to, whereas here the remainder is exactly known — the cap is what the file
 * says and the selection is already in hand. Naming the remainder is also what
 * makes the control the answer to the question the header raises: the header
 * says 10, the section shows 3, and the button accounts for the other 7.
 */
export const SESSION_SPACE_ROWS_MORE = (n: number) => `Show ${n} more`;
export const SESSION_SPACE_ROWS_LESS = "Show less";

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
  /** The session record's own name, per shape — `session-detail.tsx` composes it. */
  recordLabel: string;
  /** Open that record in the strip. */
  onOpenRecord: () => void;
}

export function SessionSpaces({
  rootId,
  sessionId,
  writing,
  onWriting,
  spaces,
  selections,
  onChanged,
  recordLabel,
  onOpenRecord,
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
  // be a second walk for a dropdown. A file no space of a kind lists is a file
  // with no kind tag, which the `Untagged` space itself names.
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
   * The spaces this session actually draws a section for.
   *
   * Every definition, minus the residue space when there is no residue — see
   * {@link SESSION_SPACE_UNTAGGED_KEY}. A selection that has not arrived is not
   * an empty one: while `selections` is `null` this space stays out, so it
   * appears WITH its rows rather than appearing, saying "Reading…", and then
   * disappearing because the answer was zero.
   *
   * The zone-has-no-spaces sentence below still keys on `spaces`, not on this: a
   * zone whose only definition is a residue space with nothing in it has a space,
   * and telling the operator it has none would send them to Restore for a file
   * that is already there.
   */
  const drawn = useMemo(
    () =>
      spaces.filter(
        (space) =>
          space.defaultKey !== SESSION_SPACE_UNTAGGED_KEY ||
          (byId.get(space.id)?.files.length ?? 0) > 0,
      ),
    [spaces, byId],
  );

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
        drawn.map((space) => (
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
            recordLabel={recordLabel}
            onOpenRecord={onOpenRecord}
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
  recordLabel,
  onOpenRecord,
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
  recordLabel: string;
  onOpenRecord: () => void;
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
  // projects `create_refused`'s sentence onto this payload, and this file prints
  // it. There is deliberately no TypeScript reading of the mapping: a second one
  // drifts, and the copy that lived here had already forked the refusal's wording
  // on the day it was written.
  const refusal = selection?.noHome ?? null;
  // The create has THREE states, and Rust decides which by answering with a
  // kind, a sentence, or neither (Story 52.4).
  //
  // - a kind and no refusal: present and pressable;
  // - a refusal: present, DISABLED, and described BY that refusal — About's "a
  //   session has one about record", the Untagged space's "every one of its terms
  //   is a negation", a two-term query's "more than one thing". This is the
  //   story's own report: *"about space nie ma przycisku dodaj jak inne"*. A
  //   refusal a person can read beats a control that is silently absent, and the
  //   sentence is not repeated here — the control points at the one Rust already
  //   worded in the notice below;
  // - neither: ABSENT, which is the one case the `showNoteInFiles` precedent
  //   still governs. `tag:project/alpha` names a tag that is not a kind and a
  //   query keeper cannot parse says so through its own fault lamp; a control
  //   that refuses without explaining teaches nothing.
  //
  // While the selections have not arrived there is no answer to any of this, so
  // the verb waits with the rows rather than appearing and then vanishing.
  const creatable = kind !== null && refusal === null;
  const showCreate = selection !== null && (creatable || refusal !== null);
  // The refusal's own element, so the disabled control can point at it. `useId`
  // rather than the space id: a space id is a path and may hold whitespace, which
  // an IDREF cannot (see {@link SESSION_SPACE_FOLD_ID}).
  const refusalId = useId();
  // The repair that refusal carries, when it carries one (Story 53.4, FR-319).
  //
  // A query, composed in Rust, on the SAME payload as the sentence: it is set
  // only where `noHome` is the arity refusal AND the space claims a default that
  // asks for a single `tag:` term, so this file decides nothing about when a
  // repair applies and reads no `keeper.space` to find its term. What it does
  // with the string is print it, so the person reads what the press will write
  // before pressing, and send an id back.
  const narrowTo = selection?.narrowTo ?? null;

  // Whether this space is folded, and where that is remembered (Story 49.3,
  // FR-275; Story 51.3, FR-289). Keyed by root and space id rather than by
  // session: the definition belongs to the zone, so a person who shut Tasks
  // meant Tasks, not Tasks in this one session. The space's OWN `keeper.folded`
  // goes in as the middle layer and is never resolved here —
  // {@link isSpaceFolded} composes all four steps, and doing any of it twice is
  // how a hand-fold comes to lose to a file.
  const foldKey = spaceFoldKey(rootId, space.id);
  const folded = useSessionSpacesFold((state) => isSpaceFolded(state, foldKey, space.folded));

  // How much of the selection this section draws (Story 51.3, FR-290).
  //
  // **A render cap, not a selection cap.** `files` is the WHOLE selection —
  // Rust was asked for all of it, the header counts all of it, and this only
  // decides how many rows are painted. Notes' `keeper.limit` narrows the query
  // instead; a session holds tens of files, so there is no read to save here,
  // and a section that had selected 3 of 12 could not say how many it was
  // hiding.
  //
  // **The remainder folds, it does not scroll.** A nested scroll area inside a
  // scrolling pane is what the sync card rejected (`sync-pane.tsx:254-319`),
  // and a session's detail is exactly that pane: two scrollbars a few pixels
  // apart, one of which swallows the wheel.
  const [showingAll, setShowingAll] = useState(false);
  const cap = space.rows;
  const visible = cap === null || showingAll ? files : files.slice(0, cap);
  const hidden = files.length - visible.length;

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
    // The space that pressed it (Story 52.5): a space may name the directory its
    // creates land in, and Rust reads that definition — this file sends an id and
    // composes no path.
    sessionsFileNewKind(rootId, sessionId, kind, "", space.id)
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
  }, [kind, rootId, sessionId, space.id, space.name, onOpen, onNotice, onChanged, onWriting]);

  /**
   * Narrow this space to the single term its default asks for (Story 53.4,
   * FR-319).
   *
   * **An id and nothing else.** The query is Rust's — it read it off the default
   * this space claims — and the verb reads it again server-side rather than
   * trusting what this file is holding, so a stale payload narrows nothing.
   * `narrowTo` is printed on the control for the person and never sent.
   *
   * It shares `writing` with the create, because both are writes into this
   * session's zone and neither should be pressed twice while the first is in
   * flight. The sentence lands in the section's one live region, named with the
   * space for {@link newNote}'s reason: this notice sits above every section.
   *
   * `onChanged` after it lands, because the query that changed is on the
   * definitions payload and the rows it now selects are on the other one — only
   * a re-read makes the section agree with the file.
   */
  const narrow = useCallback(() => {
    if (narrowTo === null) {
      return;
    }
    onWriting(true);
    onNotice(null);
    sessionsSpaceNarrow(rootId, space.id)
      .then(() => {
        onNotice(`${SESSION_SPACE_NARROWED} ${space.name} to ${narrowTo}.`);
        onChanged();
      })
      .catch((raw: unknown) =>
        onNotice(`${space.name}: ${syncErrorMessage(raw, SESSION_SPACE_NARROW_FAILED)}`),
      )
      .finally(() => onWriting(false));
  }, [narrowTo, rootId, space.id, space.name, onNotice, onChanged, onWriting]);

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
              than a fourth button.

              **A space that cannot be written into still gets the control**
              (Story 52.4), disabled and described by Rust's own refusal. The
              `showNoteInFiles` precedent — a control that exists only to refuse
              teaches nothing — still governs the case where there is nothing to
              teach: no kind AND no sentence, which is `tag:project/alpha` and a
              query that will not parse. Where there IS a sentence, absence was
              the worse answer, because "why is there no button on About when
              every other space has one" is a question the surface was leaving
              the person to answer for themselves. */}
          {/* Where a create is refused because the record ALREADY EXISTS, the
              verb that applies is opening it (Story 51.7, FR-299) — in the
              create's own slot, because it answers the same question a person
              came to this space with. Rust decides it: `openRecord` is set only
              where the refusal is `KindHasNoHome::OnlyOne`, so this file reads
              no query and knows no filename. The label is the header's, which
              already names `about.md` or `README` from the shape. */}
          {selection?.openRecord === true && (
            <button
              type="button"
              aria-label={recordLabel}
              onClick={onOpenRecord}
              className={cn(
                "shrink-0 rounded-md p-1 text-muted-foreground outline-none",
                "hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
              )}
            >
              <FileText aria-hidden="true" className="size-3.5" />
            </button>
          )}
          {showCreate && (
            <button
              type="button"
              aria-label={`${SESSION_SPACE_NEW_NOTE} ${space.name}`}
              // Rust's sentence, referenced rather than repeated: it is already
              // rendered once in the notice below, and a second copy on this
              // control is a second thing to keep in step. `undefined` and not
              // the id when there is no refusal, so the attribute never points
              // at an element that is not in the DOM.
              aria-describedby={refusal === null ? undefined : refusalId}
              onClick={newNote}
              disabled={writing || !creatable}
              className={cn(
                "shrink-0 rounded-md p-1 text-muted-foreground outline-none",
                "hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring",
                // Not `disabled:pointer-events-none`, which the rail's own
                // buttons carry: this control is disabled for a REASON a person
                // is meant to read, so it keeps the cursor that says so.
                "disabled:cursor-not-allowed disabled:opacity-50",
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
          {/* Why this space offers no create, in Rust's words — this session's
              contract keeping that kind nowhere, the query asking for more than
              one thing so that "what would a file made here be?" has no single
              answer, or every one of its terms being a negation so that it names
              no kind at all. Muted, not destructive: nothing is broken and
              nothing failed, and the person is entitled to know which.

              It carries the id the disabled create points at (Story 52.4), which
              is why it renders here whether or not the rows are showing: an
              `aria-describedby` whose target had folded away would describe
              nothing. */}
          {refusal !== null && (
            <p id={refusalId} data-slot="space-no-home" className="text-muted-foreground text-xs">
              {refusal}
            </p>
          )}
          {/* The repair, directly under the sentence that explains why it is
              there (Story 53.4, FR-319). Rust decides whether it exists —
              `narrowTo` is set only beside the arity refusal, on a space claiming
              a default that asks for one `tag:` term — so a space with no
              authority for a term offers the pencil and nothing else.

              **It names its outcome.** The label ends in the query it will write,
              which is the difference between a repair and a settings toggle: a
              person can read what is about to happen to a file they own before
              pressing, and what they read is the string that gets written.

              A bordered button, not one of the header's ghost glyphs and not the
              row-cap's underlined link: this one WRITES, and it should not look
              like the two controls beside it that only rearrange what is on
              screen. In the notice rather than in `actions` for the same reason
              the sentence is: the header spends its ~208px on a name, a count and
              three controls, and this control has to carry a query in its label.

              No confirmation. It changes one key in one file through the same save
              the editor uses, and it is undone by editing the space — an
              AlertDialog here would be the weight of Delete on a reversible
              edit. */}
          {narrowTo !== null && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              data-slot="space-narrow"
              aria-label={`${SESSION_SPACE_NARROW} ${narrowTo}: ${space.name}`}
              disabled={writing}
              onClick={narrow}
              className="h-7 self-start px-2 font-normal text-xs"
            >
              {`${SESSION_SPACE_NARROW} ${narrowTo}`}
            </Button>
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
          {visible.map((file) => (
            <li key={file.relPath} data-testid={`${SESSION_SPACE_FILE_TESTID}-${file.relPath}`}>
              {/* Every verb a row has beyond opening it (Story 51.6, FR-297).
                  A wrapper rather than controls in the row: this header is
                  ~208px wide and already spends its width on a truncating title
                  and a date, and the menu is the surface that can hold six verbs
                  without taking a pixel from either. It renders the Button as its
                  own trigger, so single-click Open is exactly what it was, and it
                  reports its refusals into this section's one live region rather
                  than growing a second sentence inside the row. */}
              <SpaceRowMenu
                rootId={rootId}
                sessionId={sessionId}
                relPath={file.relPath}
                subpath={file.subpath}
                title={file.title}
                onOpen={onOpen}
                onChanged={onChanged}
                onNotice={onNotice}
              >
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  // Path-identified rather than id-identified is recorded, not
                  // drawn: keeper never stamps an `id:` into a file it did not
                  // author, so in a zone of hand-written markdown this would be
                  // true of nearly every row — a badge on all of them is decor,
                  // not information. It reaches a test and a stylesheet here and
                  // reaches a person nowhere: a rename now rewrites the pointers
                  // that named the file, in the same journaled plan (Story 51.6),
                  // so path identity no longer costs them anything a badge would
                  // be warning them about.
                  data-unstable-identity={file.unstableIdentity ? "" : undefined}
                  className="h-7 w-full min-w-0 justify-start gap-2 px-2 font-normal"
                  onClick={() => onOpen(file.subpath)}
                >
                  <span className="min-w-0 flex-1 truncate text-sm">{file.title}</span>
                  <span className="figures shrink-0 text-muted-foreground text-xs">
                    {formatDraftAge(file.mtimeMs, nowMs)}
                  </span>
                </Button>
              </SpaceRowMenu>
            </li>
          ))}
        </ul>
      )}
      {/* The cap's control, below the rows it is about — the sync card's
          placement, and for its reason: the last row shown is where the eye
          already is when the list runs out. A link-weight control and not a
          Button, because it changes how much of a list is on screen and must not
          carry the weight of Open or New.

          Absent when the cap is doing nothing in either direction: no remainder
          and nothing unfolded means a button that would say "Show 0 more".
          Named with the space, because a detail with five capped sections would
          otherwise offer five buttons a screen reader calls the same thing. */}
      {(hidden > 0 || showingAll) && (
        <button
          type="button"
          data-slot="space-rows-fold"
          onClick={() => setShowingAll((shown) => !shown)}
          aria-label={`${showingAll ? SESSION_SPACE_ROWS_LESS : SESSION_SPACE_ROWS_MORE(hidden)}: ${space.name}`}
          className="self-start text-muted-foreground text-xs underline decoration-dotted hover:text-foreground"
        >
          {showingAll ? SESSION_SPACE_ROWS_LESS : SESSION_SPACE_ROWS_MORE(hidden)}
        </button>
      )}
    </FoldSection>
  );
}
