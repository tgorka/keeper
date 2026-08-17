/**
 * The session detail (Phase 7, FR-233, FR-254, UX-DR89): the drill-in a board
 * row opens — header with tags, the properties widget, lineage chips, the
 * session's own file tree, what it points at, and the rendered activity log.
 *
 * **The section order is files → refs → log**, and it inverted when the flat
 * contract landed. Under the folder shape the log was the session: it lived in
 * the README and the files were the supporting cast. Flat, the files ARE the
 * session — every log entry, prompt and reference is one of them — so the tree
 * is both the map and the contents, and it goes first. The log goes last
 * because it is the one section that grows without bound; anything under it
 * would drift out of reach as the session aged.
 *
 * A review surface, so the ordering choices all point one way: the log renders
 * NEWEST FIRST (the file on disk keeps the zone's newest-last convention —
 * only this projection reverses), and inside a file section the newest file is
 * first. Everything shown is a fresh projection of the zone's files, re-read
 * on the changed event — an agent writing on disk moves this view live.
 *
 * **Two reads, not one** (FR-254). The record — header, properties, log —
 * comes from `sessions_detail`; the files come from `sessions_tree`, which
 * costs a directory walk and one `Engine::pending` query. Binding them into
 * one payload would make every log re-read pay for the tree. Both re-read on
 * the same event, so the surface still moves as one thing.
 *
 * Files open in the panel strip beside the board through the SAME file target
 * the Files pane sets (AD-109, UX-DR91) — one editor, one viewer registry, no
 * second open path. `workspace/` rows carry the write fence's own refusal
 * sentence; the fence in Rust is what enforces it (AD-113).
 */
import { ArrowLeft, Pencil, Pin } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { SessionAddRef } from "@/components/sessions/session-add-ref";
import { SessionBoard } from "@/components/sessions/session-board";
import { SessionFileActions } from "@/components/sessions/session-file-actions";
import { SESSION_REFS_HEADING, SessionRefs } from "@/components/sessions/session-refs";
import { SessionSpaces } from "@/components/sessions/session-spaces";
import { SessionTree } from "@/components/sessions/session-tree";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type {
  SessionDetailVm,
  SessionEntryVm,
  SessionReferencesVm,
  SessionSpaceFilesVm,
  SessionSpaceVm,
  SessionTreeVm,
} from "@/lib/ipc/client";
import {
  listenSessionsChanged,
  sessionsDetail,
  sessionsRefs,
  sessionsSpaceFiles,
  sessionsSpaces,
  sessionsSpacesFoldedGet,
  sessionsTree,
} from "@/lib/ipc/client";
import { panelsStore } from "@/lib/stores/panels";
import {
  hydrateSessionSpacesFold,
  sessionSpacesFoldStore,
  setSpacesFoldedDefault,
} from "@/lib/stores/session-spaces-fold";

/** The way back to the board. */
export const SESSION_DETAIL_BACK_LABEL = "Back to sessions";

/** Section headings — the zone's own vocabulary, not keeper's. */
export const SESSION_DETAIL_LOG_HEADING = "Log";
export const SESSION_DETAIL_FILES_HEADING = "Files";
export const SESSION_DETAIL_PROPERTIES_HEADING = "Properties";

/**
 * The zone's own sentence about `workspace/` (FR-237), on the Files heading
 * rather than on a section of its own.
 *
 * The tree nests, so `workspace/` is one row among the session's sections and
 * has nowhere to hang a paragraph. Saying it once, above the tree, is also the
 * more useful place for it: the caveat explains a rule about the session's
 * shape, and the fence's own refusal sentence is what explains an individual
 * locked row.
 */
export const SESSION_DETAIL_WORKSPACE_CAVEAT =
  "workspace/ is scratch — not versioned, not synced, dies with the session. Read-only in keeper; promote what matters into artifacts.";

/** What an empty log says, honestly. */
export const SESSION_DETAIL_NO_LOG = "No log entries yet.";

/**
 * The `unfiled` notice (AD-119): root markdown that declares no kind.
 *
 * Not an error and not styled as one — a hand-dropped note is an ordinary way
 * to use a folder, and a person mid-thought should not be scolded by their own
 * tooling. It is a *nudge*, and it exists because in a flat session an untagged
 * file is invisible to every space: it would sit on disk being skipped by the
 * one surface that was supposed to show it.
 */
export const SESSION_DETAIL_UNFILED_HEADING = "Unfiled";
export const SESSION_DETAIL_UNFILED_HINT =
  "No kind tag, so no space will list these. Add tags: [log], [ref], [prompt] or [task] to file them.";

/**
 * Open the session's record in the strip.
 *
 * **One name, one label, since Story 52.1.** The record was `about.md` under the
 * flat contract and `README.md` under the folder one, so this was a pair of
 * labels picked by shape. Both contracts keep the record at `README.md` now, so
 * there is one file to name and the button names it — "Open record" would be
 * keeper's word for a file the operator knows by its filename, and the whole
 * point of the flat contract is that the files are the truth.
 */
export const SESSION_RECORD_NAME = "README.md";
export const SESSION_DETAIL_OPEN_RECORD_LABEL = "Open README";

export interface SessionDetailProps {
  rootId: string;
  /**
   * The zone subfolder ("60-sessions"), for the README target.
   *
   * Every OTHER path on this surface arrives composed from Rust
   * (`SessionEntryVm.subpath`, AD-65). The README keeps a join here because it
   * is the one file the header opens whether or not the tree has loaded — and
   * it is a fixed name at a known place, not a path a walk discovered.
   */
  subfolder: string;
  sessionId: string;
  onBack: () => void;
}

export function SessionDetail({ rootId, subfolder, sessionId, onBack }: SessionDetailProps) {
  const [detail, setDetail] = useState<SessionDetailVm | null>(null);
  const [tree, setTree] = useState<SessionTreeVm | null>(null);
  const [refs, setRefs] = useState<SessionReferencesVm | null>(null);
  const [spaces, setSpaces] = useState<readonly SessionSpaceVm[]>([]);
  const [spaceFiles, setSpaceFiles] = useState<readonly SessionSpaceFilesVm[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Bumped by a write from the spaces section. The changed event covers writes
  // that land in the zone's own tree, but a space definition is saved through a
  // plan whose completion this surface hears about no sooner than the watcher
  // does — and "no sooner" is a race, not a guarantee. An explicit re-read after
  // a write keeps the section honest without waiting on the filesystem.
  const [reload, setReload] = useState(0);
  /**
   * ONE create in flight across this whole session, not one per section.
   *
   * Two children offer a create — the Files heading's *New log* / *New prompt*
   * / *New file* and every writable space's *New note in …* — and the two
   * kind-creates both post `sessions_file_new_kind` with an EMPTY title. Rust
   * names such a file `YYYY-MM-DD-HHMM-untitled.md` from the clock to the
   * minute and `files::compile_new` emits a plain `WriteFile`, so two presses
   * in the same minute compute `taken_in` before either write lands, resolve to
   * one filename, and the second silently overwrites the first — a `tag: task`
   * file becoming a `tag: log` one.
   *
   * The flag lives HERE because that is the lowest node both surfaces share:
   * each of them held its own `useState` until Story 50.1's review, which
   * removed only the presses within one component and left "New prompt above,
   * New note below" reachable. Serialising in Rust is the real fix and is that
   * crate's to make; this is what removes the press a person can perform.
   */
  const [writing, setWriting] = useState(false);

  /**
   * Restore the spaces' fold, and read the default it falls back to
   * (Story 49.3, FR-275, FR-276).
   *
   * **Here, at the mount point, and not inside `SessionSpaces`.** A `hydrate…`
   * call is the one part of a remembered fold that no store-level test can see
   * the absence of: the store passes every one of its own assertions with the
   * restore never wired up, and the person gets a fold that forgets itself on
   * every visit. That is DW-172, and Story 48.1's mutation M3 measured it — a
   * removed `hydrate…` killed exactly one test, the one at the mount point. The
   * matching test for this one is in `session-detail.test.tsx`.
   *
   * Once per document, not once per session: the cookie holds every space in
   * every root, and {@link hydrateSessionSpacesFold} is idempotent, so a second
   * detail cannot undo a fold the person has changed since the first.
   *
   * **The cookie does not wait for the setting.** Reading it needs no IPC, and
   * the spaces themselves arrive from the sibling effect below — five
   * independent `invoke`s with no ordering between them. Restoring the fold
   * inside the setting read's `.then` therefore let the spaces payload win the
   * race and paint every space the person had folded OPEN, against the store's
   * initial `defaultFolded: false`, before snapping it shut a moment later.
   * Only the fallback for spaces with NOTHING recorded genuinely needs Rust, so
   * only that waits; a space somebody decided about is restored before anything
   * paints.
   *
   * The setting arrives second and moves `defaultFolded` alone — never
   * `recorded`, which is what keeps a hand-made answer outranking it — and it
   * is applied only if nothing else moved the fallback while the read was out.
   * Settings writes that fallback the moment somebody flips the switch, and a
   * read issued before the flip is older news than the flip; the `hydrate…`
   * latch used to cover that for free, and it is spelled out here now that the
   * restore no longer waits behind it. A read that FAILS corrects nothing:
   * unfolded is the registry's own default, and the store already holds either
   * that or whatever another surface read.
   */
  useEffect(() => {
    let live = true;
    // The fallback the store already holds: this call restores the COOKIE, and
    // the setting arrives on its own below.
    const seeded = sessionSpacesFoldStore.getState().defaultFolded;
    hydrateSessionSpacesFold(document.cookie, seeded);
    sessionsSpacesFoldedGet().then(
      (folded) => {
        // Still `seeded` means nothing newer has been said about the fallback.
        if (live && sessionSpacesFoldStore.getState().defaultFolded === seeded) {
          setSpacesFoldedDefault(folded);
        }
      },
      () => {
        // Nothing to say and nothing to correct — see above.
      },
    );
    return () => {
      live = false;
    };
  }, []);

  // Read on mount and re-read on the changed event — an agent's write on
  // disk moves this surface without a keystroke (FR-234's detail half).
  // biome-ignore lint/correctness/useExhaustiveDependencies: `reload` is a deliberate re-run trigger, not a read — the spaces section bumps it after a write so the two space reads happen again without waiting on the watcher.
  useEffect(() => {
    let live = true;
    const read = () => {
      sessionsDetail(rootId, sessionId).then(
        (vm) => {
          if (live) {
            setDetail(vm);
            setError(null);
          }
        },
        (e: unknown) => {
          if (live) {
            setError(e instanceof Error ? e.message : String(e));
          }
        },
      );
      // The tree's own failure does NOT blank the record: a session whose
      // files could not be walked still has a log worth reading, and the
      // record's error slot is where a real failure to find the session is
      // reported. A missing tree renders as a session with no files, which
      // is what a caller with no answer can honestly say.
      sessionsTree(rootId, sessionId).then(
        (vm) => {
          if (live) {
            setTree(vm);
          }
        },
        () => {
          if (live) {
            setTree(null);
          }
        },
      );
      // A third read, for the same reason the tree is a second one (FR-255):
      // the reference scan parses every markdown file in the session and asks
      // the vault index about each target, and a log re-read should not pay for
      // that. Its failure is as local as the tree's — a session whose refs
      // could not be scanned still has a log and files worth reading.
      sessionsRefs(rootId, sessionId).then(
        (vm) => {
          if (live) {
            setRefs(vm);
          }
        },
        () => {
          if (live) {
            setRefs(null);
          }
        },
      );
      // The spaces are two more reads for the same reason (FR-261), and they
      // stay two: the definitions are the zone's and change when someone edits
      // one, while the selections are this session's and change whenever any
      // file in it does. Folding them together would re-parse five queries on
      // every keystroke an agent makes in a log file.
      //
      // They also fail locally — a zone with no `_spaces/` yet is the ordinary
      // state of a session created before this shipped, and it must not blank
      // the record. `[]` is the honest rendering: no spaces, and the section
      // offers to restore the defaults.
      sessionsSpaces(rootId).then(
        (vm) => {
          if (live) {
            setSpaces(vm);
          }
        },
        () => {
          if (live) {
            setSpaces([]);
          }
        },
      );
      sessionsSpaceFiles(rootId, sessionId).then(
        (vm) => {
          if (live) {
            setSpaceFiles(vm);
          }
        },
        () => {
          if (live) {
            // `[]`, not `null`: null means "still reading" to the section, and
            // a read that already failed is not still reading. Every space then
            // draws its own empty state rather than an eternal "Reading…".
            setSpaceFiles([]);
          }
        },
      );
    };
    read();
    let unlisten: (() => void) | null = null;
    void listenSessionsChanged((changedRootId) => {
      if (live && changedRootId === rootId) {
        read();
      }
    }).then((stop) => {
      if (live) {
        unlisten = stop;
      } else {
        stop();
      }
    });
    return () => {
      live = false;
      unlisten?.();
    };
  }, [rootId, sessionId, reload]);

  // Files open in the strip through the one file target (AD-109). The path
  // is the entry's own `subpath`, composed in Rust (AD-65) — this surface
  // never joins one. The workspace fence in Rust keeps read-only honest.
  const openFile = useCallback(
    (entry: SessionEntryVm) => {
      if (entry.isDir) {
        return;
      }
      panelsStore.getState().setActiveTarget({
        kind: "file",
        profileId: rootId,
        relativePath: entry.subpath,
      });
    },
    [rootId],
  );

  // The record's filename is a constant, not a guess and no longer a branch on
  // the shape (Story 52.1): both contracts keep it at `README.md`, so `shape` is
  // read here for what the surface OFFERS and never for what a file is called.
  const openRecord = useCallback(() => {
    if (detail === null) {
      return;
    }
    panelsStore.getState().setActiveTarget({
      kind: "file",
      profileId: rootId,
      relativePath: `${subfolder}/${detail.path}/${SESSION_RECORD_NAME}`,
    });
  }, [detail, rootId, subfolder]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-border border-b px-6 py-2">
        <Button type="button" variant="ghost" size="sm" onClick={onBack} className="gap-1">
          <ArrowLeft aria-hidden className="size-3.5" />
          {SESSION_DETAIL_BACK_LABEL}
        </Button>
      </div>
      {error !== null && (
        <div role="alert" className="m-6 rounded-md bg-destructive/10 p-3 text-destructive text-sm">
          {error}
        </div>
      )}
      {detail !== null && (
        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 py-4">
          {/* Header: title, status, pin, tags, lineage, summary. */}
          <header className="flex flex-col gap-1">
            <div className="flex min-w-0 items-center gap-2">
              {detail.pinned && (
                <Pin aria-label="pinned" className="size-3.5 text-muted-foreground" />
              )}
              <h2 className="min-w-0 flex-1 truncate font-heading text-title">{detail.title}</h2>
              <Badge variant="outline">
                {detail.status === "archived" && detail.archivedYear !== null
                  ? `archived ${detail.archivedYear}`
                  : detail.status}
              </Badge>
              <Button type="button" variant="outline" size="sm" onClick={openRecord}>
                <Pencil aria-hidden className="size-3.5" />
                {SESSION_DETAIL_OPEN_RECORD_LABEL}
              </Button>
            </div>
            {detail.summary !== "" && (
              <p className="text-muted-foreground text-sm">{detail.summary}</p>
            )}
            <div className="flex flex-wrap items-center gap-1">
              {detail.tags.map((tag) => (
                <Badge key={tag} variant="secondary">
                  {tag}
                </Badge>
              ))}
              {/* Lineage chips (UX-DR89): navigable directions arrive with the
                  cross-session router; ids render inert but visible for now. */}
              {detail.continues.map((id) => (
                <Badge key={`c-${id}`} variant="outline" title={`continues ${id}`}>
                  ← continues
                </Badge>
              ))}
              {detail.continuedBy.map((id) => (
                <Badge key={`b-${id}`} variant="outline" title={`continued by ${id}`}>
                  continued →
                </Badge>
              ))}
            </div>
          </header>

          {/* The properties widget (FR-227): user-tier frontmatter, read here,
              edited in the README's own properties panel — one writer. */}
          {detail.properties.length > 0 && (
            <section
              aria-label={SESSION_DETAIL_PROPERTIES_HEADING}
              className="rounded-md border border-border px-3 py-2"
            >
              <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
                {detail.properties.map((property) => (
                  <div key={property.key} className="contents">
                    <dt className="text-muted-foreground">{property.key}</dt>
                    <dd className="min-w-0 truncate">{property.value.split("\n").join(", ")}</dd>
                  </div>
                ))}
              </dl>
            </section>
          )}

          {/* The session's own file tree, in the zone's own order (FR-254) —
              FIRST, and fully expanded. In a flat session the files ARE the
              structure, so this is both the map and the contents; anything
              above it would be read past. */}
          <section aria-label={SESSION_DETAIL_FILES_HEADING} className="flex flex-col gap-1">
            <div className="flex items-center justify-between gap-2">
              <h3 className="flex items-baseline gap-2 font-medium text-muted-foreground text-xs uppercase tracking-wide">
                {SESSION_DETAIL_FILES_HEADING}
              </h3>
              {/* The verbs that GROW the pool, on the heading of the thing they
                  grow (FR-262). They re-read through the same counter the
                  spaces section bumps, so one write refreshes every read on
                  this surface rather than each section keeping its own idea of
                  what is on disk. */}
              <SessionFileActions
                rootId={rootId}
                sessionId={sessionId}
                shape={detail.shape}
                entries={tree?.entries ?? []}
                busy={writing}
                onBusy={setWriting}
                onChanged={() => setReload((n) => n + 1)}
              />
            </div>
            <p className="text-muted-foreground text-xs">{SESSION_DETAIL_WORKSPACE_CAVEAT}</p>
            <SessionTree
              rootId={rootId}
              sessionId={sessionId}
              entries={tree?.entries ?? []}
              truncated={tree?.truncated ?? false}
              onOpen={openFile}
              onChanged={() => setReload((n) => n + 1)}
            />
          </section>

          {/* Root markdown that declares no kind (AD-119) — directly under the
              tree, because the fix is to edit one of the files just listed.
              Absent for a clean session, which is what makes it a signal. */}
          {detail.unfiled.length > 0 && (
            <section
              aria-label={SESSION_DETAIL_UNFILED_HEADING}
              className="flex flex-col gap-1 rounded-md border border-border border-dashed px-3 py-2"
            >
              <h3 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
                {SESSION_DETAIL_UNFILED_HEADING}
              </h3>
              <p className="text-muted-foreground text-xs">{SESSION_DETAIL_UNFILED_HINT}</p>
              <ul className="flex flex-wrap gap-1">
                {detail.unfiled.map((name) => (
                  <li key={name}>
                    <Badge variant="outline" className="font-mono text-xs">
                      {name}
                    </Badge>
                  </li>
                ))}
              </ul>
            </section>
          )}

          {/* The zone's saved queries, read against this session (FR-261) —
              AFTER the files, on the operator's own ordering. The tree is what
              is there; this is what it means. A person who has just seen the
              filenames can read a section called Tasks and know which of those
              files it is talking about; the reverse order would ask them to
              trust a grouping before seeing the thing grouped. */}
          <SessionSpaces
            rootId={rootId}
            sessionId={sessionId}
            // The section lists AND writes under both contracts (Story 50.1):
            // `sessions_file_new_kind` writes where the shape keeps the kind,
            // and a kind this session's shape keeps no home for arrives as a
            // sentence on `SessionSpaceFilesVm.noHome`. So the shape itself
            // does not travel: one reader of `shape::kind_dir`, in Rust.
            spaces={spaces}
            selections={spaceFiles}
            // The verb a space offers where its create is refused because the
            // record already exists (Story 51.7, FR-299). WHICH space that is is
            // Rust's answer, per space, on `openRecord`; the label and the target
            // are the header's own, because the record is one fixed name at a
            // known place and this surface already opens it from up there.
            recordLabel={SESSION_DETAIL_OPEN_RECORD_LABEL}
            onOpenRecord={openRecord}
            // The same flag the Files heading above is handed — both surfaces
            // offer a create that posts an empty title, and one filename is
            // what they would collide on.
            writing={writing}
            onWriting={setWriting}
            onChanged={() => setReload((n) => n + 1)}
          />

          {/* The board (FR-263) — after the spaces, because a space is the
              question "which files are tasks?" and the board is what those files
              say about themselves.

              **Both shapes** (Story 51.7, FR-299). This used to render for a flat
              session only, and the reason was true when it was written: a
              folder-shaped session had no pool to tag, so its board would have
              been four empty columns saying nothing true. Story 51.1 put that
              shape's root markdown into the pool, so a `task`-tagged file there
              is a card — and the shape was standing in for the real question,
              which is whether there is anything tagged.

              That question is `detail.tasks`, and there is deliberately no second
              predicate beside it: a session with nothing to tag has no cards, and
              the board answers an empty list with the sentence that says what a
              task is rather than with columns (`task-board.tsx`). A "does it have
              a pool" flag could only ever disagree with the cards by being wrong.

              Drag-and-drop and the per-card dropdown are untouched: the drop
              writes `status:` and `order:` through `sessions_task_move`, which
              re-reads the column it is dropping into and knows nothing of the
              shape. */}
          <SessionBoard
            rootId={rootId}
            sessionId={sessionId}
            tasks={detail.tasks}
            // A card knows its session-relative path; the path that OPENS it
            // is the `subpath` Rust composed for the same file in the tree
            // (AD-65). Looked up rather than joined, so there is still exactly
            // one place in the app that knows how a zone path is built.
            onOpen={(relPath) => {
              const entry = tree?.entries.find((candidate) => candidate.relPath === relPath);
              if (entry !== undefined) {
                openFile(entry);
              }
            }}
            onChanged={() => setReload((n) => n + 1)}
          />

          {/* What the session points at (FR-255) — after the files, because it
              is the same question asked the other way: what it holds, then what
              it names. */}
          <section aria-label={SESSION_REFS_HEADING} className="flex flex-col gap-1">
            <div className="flex items-center justify-between gap-2">
              <h3 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
                {SESSION_REFS_HEADING}
              </h3>
              {/* The verb that grows this list, on the heading of the list it
                  grows — the Files section's own placement (FR-265). It shares
                  the reload counter for the same reason: adding a reference
                  writes a markdown file, so the tree, the spaces and this list
                  are all a keystroke out of date until they re-read. */}
              <SessionAddRef
                rootId={rootId}
                sessionId={sessionId}
                onChanged={() => setReload((n) => n + 1)}
              />
            </div>
            <SessionRefs
              refs={refs?.refs ?? []}
              missing={refs?.missing ?? 0}
              truncated={refs?.truncated ?? false}
            />
          </section>

          {/* The rendered activity log, newest first (the review order), and
              LAST on the surface. It is the section that grows without bound —
              a session running for weeks has dozens of entries — so anything
              placed under it would drift out of reach as the session aged.
              Everything above is a fixed-height answer to "what is this?"; the
              log is the unbounded answer to "what happened?", and that is the
              order a person reads them in. */}
          <section aria-label={SESSION_DETAIL_LOG_HEADING} className="flex flex-col gap-2">
            <h3 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
              {SESSION_DETAIL_LOG_HEADING}
            </h3>
            {detail.log.length === 0 ? (
              <p className="text-muted-foreground text-xs">{SESSION_DETAIL_NO_LOG}</p>
            ) : (
              <ol className="flex flex-col gap-2">
                {detail.log.map((entry) => (
                  <li
                    key={`${entry.date}-${entry.title}`}
                    className="rounded-md border border-border px-3 py-2"
                  >
                    <p className="flex items-baseline gap-2 text-sm">
                      <span className="figures shrink-0 text-muted-foreground">{entry.date}</span>
                      {entry.title !== "" && <span className="font-medium">{entry.title}</span>}
                    </p>
                    {entry.body !== "" && (
                      <p className="mt-1 whitespace-pre-wrap text-muted-foreground text-sm">
                        {entry.body}
                      </p>
                    )}
                  </li>
                ))}
              </ol>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
