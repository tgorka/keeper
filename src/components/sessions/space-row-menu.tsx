/**
 * The verbs a space row has, on a right-click (Story 51.6, FR-297).
 *
 * A space row used to be a bare `<Button>` with one gesture: click, and the file
 * replaces what the active panel is showing. Everything else it could do — open
 * beside, hand the file to the operating system, reveal it, put its path on the
 * clipboard, rename it, delete it — either had no route at all or had one on a
 * different surface, and a right-click answered with the WebView's own text menu.
 *
 * **The donor is `files-pane.tsx`, not `note-row.tsx`, and that is the whole
 * design decision.** Both are `ContextMenu` over a row and the notes one says in
 * terms that it copied the Files pane's construction; the difference is what the
 * items mean. A session file is addressed by `(profileId, path)` and has no note
 * id, so five of the notes menu's seven items — Mark read, Pin/Unpin,
 * Archive/Unarchive — are vault-only facts a session file does not have
 * (`pool.rs` says `is:pinned` in a session space is *false* rather than wrong),
 * while the two verbs a session file most wants are the two the notes menu
 * deliberately lacks: Copy path and Open in the default app, both of which need an
 * absolute path a note row is not allowed to hold.
 *
 * **There is no tab.** keeper's multi-document model is panels (`panels.ts`);
 * `role="tab"` appears in this codebase only on intra-view mode switchers. So the
 * verb is *Open in a new panel*, spelled exactly as the Files pane and the notes
 * row already spell it, rather than a third wording for one gesture.
 *
 * **Keyboard access is built here, where `files-pane.tsx` and `note-row.tsx` both
 * declined to build it.** They declined for a good reason and it does not hold
 * here: every item in the notes menu is also a bare key on the focused row, and
 * every item in the Files menu is also a focusable control on it, so in both
 * places the menu is a second route to verbs a keyboard already had. A space row
 * has no keys and no hover controls — this menu is the ONLY route to five verbs,
 * and a menu only a mouse can open would be five verbs behind a pointer. So the
 * row answers `Shift+F10` and the Menu key by dispatching the `contextmenu` event
 * the platform does not always send, and Radix closes on Escape.
 *
 * Rename and Delete are the same commands the properties panel and the session
 * tree call — `sessions_file_rename` and `sessions_file_delete` — and every
 * refusal they make is Rust's own sentence, printed rather than re-worded.
 */
import type { ReactNode } from "react";
import { useId, useState } from "react";
import {
  addProperty,
  readFrontmatter,
  serialiseScalar,
  spliceProperty,
} from "@/components/notes/properties-panel";
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
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Input } from "@/components/ui/input";
import {
  revealPath,
  sessionsFileDelete,
  sessionsFilePath,
  sessionsFileRename,
  syncOpenEntry,
  syncReadFrontmatter,
} from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { panelsStore } from "@/lib/stores/panels";
import { syncErrorMessage } from "@/lib/stores/sync";

/**
 * The item labels, spelled here rather than imported.
 *
 * What every surface in this repo does with a shared label, and for the reason
 * `note-row.tsx` gives when it does the same: the words are this surface's
 * deliverable, and importing them would make one pane's wording change another's
 * without anybody reading the second one. The two panel labels and the three
 * file labels are `files-pane.tsx`'s, character for character, because two panes
 * that mean the same gesture must say the same words.
 */
export const SPACE_ROW_OPEN_HERE_LABEL = "Open in this panel";
export const SPACE_ROW_OPEN_BESIDE_LABEL = "Open in a new panel";
export const SPACE_ROW_OPEN_LABEL = "Open in the default app";
export const SPACE_ROW_REVEAL_LABEL = "Reveal in Finder";
export const SPACE_ROW_COPY_PATH_LABEL = "Copy path";
export const SPACE_ROW_RENAME_LABEL = "Rename";
export const SPACE_ROW_DELETE_LABEL = "Delete";

/** The rename form, and what it says the title does. */
export const SPACE_ROW_RENAME_TITLE = "Rename this file?";
export const SPACE_ROW_RENAME_FIELD_LABEL = "New title";

/**
 * The body, which says the filename follows and what else moves with it.
 *
 * Worded because this is the surface where a person finds out that a title is
 * not only a label: `docs/sessions.md` refused this verb for years on the grounds
 * that a rename is a link-rewriting problem, and the answer to that refusal is
 * the sentence below — the links follow, in the same write.
 */
export const SPACE_ROW_RENAME_BODY =
  "keeper renames the file to match, keeping any date stamp in its name, and rewrites the links in this session that named it — both in one write, or neither.";

/** The confirmation, borrowing the session tree's own two sentences. */
export const SPACE_ROW_DELETE_TITLE = "Delete this file?";
export const SPACE_ROW_DELETE_BODY =
  "keeper moves it to the trash, so it is recoverable from there. Any reference pointing at it will report it missing.";

/** What a failed verb says when Rust has nothing more specific. */
export const SPACE_ROW_RENAME_FAILED =
  "keeper couldn't rename that file. Nothing was changed on disk.";
export const SPACE_ROW_DELETE_FAILED = "keeper couldn't delete that file. Nothing was changed.";

/** What a failed path lookup says — the one verb whose failure is not a write. */
export const SPACE_ROW_PATH_FAILED = "keeper couldn't work out where that file is on this disk.";

/**
 * Why a Rename offered on the session's record changes its title and not the
 * row's name.
 *
 * Rust does not refuse this rename: `files::renames` (`sessions/files.rs:414`)
 * answers *false* for the three record names and `files::compile_rename`
 * (`:769`) then compiles the title write with no move at all — deliberately,
 * because "refusing the title edit as well would make the one file whose title
 * is the session's own headline the one file whose headline cannot be changed"
 * (`:405-409`). `sessionsFileRename`'s own doc says the same on this side
 * (`client.ts:5733`).
 *
 * So the item stays, and stays live. What it grows is this sentence, because the
 * session TREE labels its rows with the filename (`session-tree.tsx:340`): a
 * title write there is a verb whose whole effect is off screen, and a control
 * that appears to do nothing is worse than one that says what it does.
 */
export const SPACE_ROW_RENAME_KEEPS_NAME =
  "This one keeps its filename — keeper reads the session's shape off that name — so the title changes and nothing moves.";

/** The frontmatter key a rename is derived from. */
const TITLE_KEY = "title";

/**
 * The three names whose filename does not follow their title.
 *
 * `files::RECORD_NAMES` (`sessions/files.rs:62`), compared against the whole
 * session-relative path exactly as Rust compares it (`:411-415`): a
 * `notes/README.md` somebody wrote is not the record, and it renames.
 *
 * Spelled here rather than read off the wire because nothing on the wire carries
 * it — `SessionEntryVm` publishes `locked` and `undeletable`, and Rust exposes
 * this one as a `bool` predicate with no sentence to print. Nothing is *gated*
 * on it (see {@link SPACE_ROW_RENAME_KEEPS_NAME}), so the cost of it drifting is
 * a sentence that stops appearing rather than a verb that stops working.
 */
const RECORD_NAMES: readonly string[] = ["AGENTS.md", "about.md", "README.md"];

export interface SpaceRowMenuProps {
  /** The sessions root, which is also the sync profile (AD-90, AD-107). */
  rootId: string;
  sessionId: string;
  /** Session-relative path — what the write verbs address. */
  relPath: string;
  /**
   * Profile-relative path, composed in Rust (AD-65) — what a panel target and
   * the system opener take. Never joined on this side.
   */
  subpath: string;
  /** The row's own words, which the rename field opens on. */
  title: string;
  /** Open it where the row's own click opens it: one implementation, not two. */
  onOpen: (subpath: string) => void;
  /** Re-read the space, because a rename or a delete changed the pool. */
  onChanged: () => void;
  /**
   * The section's live region, where every sentence a verb here produces goes.
   *
   * The surface already has one, under the `Spaces` heading, and every other
   * verb in this pane reports through it. A paragraph of my own inside the row's
   * `<li>` would be a second place to look for the same class of sentence — and
   * the one the reader has learnt is the one that is already there.
   */
  onNotice: (notice: string | null) => void;
  /**
   * Why this row's Delete is refused, when it is (FR-262) — Rust's own sentence,
   * `SessionEntryVm::undeletable` verbatim.
   *
   * Non-null renders Delete DISABLED with the sentence as its description rather
   * than dropping it, which is `session-tree.tsx:458-462`'s finding: the files a
   * person will actually try to delete are the ones whose refusal is surprising,
   * and a control that quietly is not there teaches nothing. A caller with no
   * per-path answer omits this and gets the live Delete it always had.
   */
  deleteRefusal?: string | null;
  /**
   * Whether renaming this row means anything. `false` OMITS the item.
   *
   * Omitted rather than disabled, which is the other half of the same
   * distinction: a directory and a `.png` have no frontmatter `title:` for
   * {@link commitRename} to splice, so there is no refusal to explain — the verb
   * simply is not one this row has. `session-tree.tsx:471` drops its Delete on a
   * scratch row for the matching reason (UX-DR43).
   */
  renamable?: boolean;
  /**
   * Whether the row is a directory, in which case the three verbs that address a
   * FILE are absent: both panel items and the system opener.
   *
   * `files-pane.tsx:1675-1686,1965-1975`'s rule, ported rather than re-decided —
   * a folder is not a panel target (`rowTarget` answers `null` for one) and its
   * own gesture is expand/collapse. Reveal and Copy path stay, because a folder
   * has a location like anything else, and those two are exactly what
   * `files-pane.test.tsx:1938-1942` pins for this case.
   */
  directory?: boolean;
  /** The row. Radix renders it as the trigger, so its own DOM is unchanged. */
  children: ReactNode;
}

export function SpaceRowMenu({
  rootId,
  sessionId,
  relPath,
  subpath,
  title,
  onOpen,
  onChanged,
  onNotice,
  deleteRefusal = null,
  renamable = true,
  directory = false,
  children,
}: SpaceRowMenuProps) {
  // The hook lives inside the component so the surface that mounts this does not
  // have to plumb a store it has no other use for.
  const canReveal = useCapabilitiesStore((state) => state.capabilities.revealInFileManager);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(title);
  const [deleting, setDeleting] = useState(false);
  // One id per mounted row, for the two items that carry a sentence beside their
  // label — `attach-file-button.tsx:219-229`'s construction, and for its reason:
  // Radix names an item from its text content, so a hint left to concatenate
  // would make the control answer to "Deletekeeper deletes one file at a
  // time…" — unspeakable by anybody using speech input (WCAG 2.5.3). The name is
  // the word on the item; the sentence is a description.
  const hintId = useId();
  const keepsItsName = RECORD_NAMES.includes(relPath);

  const target = { kind: "file", profileId: rootId, relativePath: subpath } as const;

  /**
   * The absolute path, asked for when the verb runs rather than when the row
   * renders.
   *
   * Two items need it and nine rows in ten will never be asked for either, so a
   * fetch on press costs one round trip where a fetch on mount would cost one per
   * row and a field on the view model would widen every row of every space. The
   * items are present either way: the verb exists, and a control that appears
   * late is a control a person has already stopped looking for.
   */
  const withPath = (use: (absolutePath: string) => void): void => {
    onNotice(null);
    void sessionsFilePath(rootId, subpath)
      .then(use)
      .catch((raw: unknown) => onNotice(syncErrorMessage(raw, SPACE_ROW_PATH_FAILED)));
  };

  /**
   * The menu opened without a pointer.
   *
   * Radix's trigger listens for the `contextmenu` DOM event, which macOS WebKit
   * does not raise for `Shift+F10` and which has no Menu key to raise it at all —
   * so the keystroke is translated into the event Radix is already waiting for,
   * positioned at the row rather than at a cursor that does not exist. Binding
   * Radix's open API directly instead would need a controlled `ContextMenu` and a
   * second source of truth for whether the menu is open.
   */
  const openWithoutAPointer = (event: React.KeyboardEvent<HTMLElement>): void => {
    if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) {
      return;
    }
    event.preventDefault();
    const box = event.currentTarget.getBoundingClientRect();
    event.currentTarget.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        clientX: Math.round(box.left + 8),
        clientY: Math.round(box.bottom - 4),
      }),
    );
  };

  /**
   * The properties panel's title write, from a menu.
   *
   * **One implementation, and this is what makes it one:** the block is read from
   * disk, the `title:` value span is spliced by the same `spliceProperty` the
   * panel splices with, and the result goes to the same `sessions_file_rename`.
   * A "rename" command that took a bare title would be a second write protocol
   * for one field, and the two would disagree about quoting the first time
   * somebody's title contained a colon.
   */
  const commitRename = (): void => {
    const next = draft.trim();
    setRenaming(false);
    onNotice(null);
    void syncReadFrontmatter(rootId, subpath)
      .then((block) => {
        const entry =
          readFrontmatter(block).entries.find((row) => row.key === TITLE_KEY && !row.nested) ??
          null;
        const nextBlock =
          entry === null
            ? addProperty(block, TITLE_KEY, serialiseScalar(next, false))
            : spliceProperty(block, entry, serialiseScalar(next, entry.quoted));
        return sessionsFileRename(rootId, subpath, block, nextBlock);
      })
      .then((nextSubpath) => {
        // Story 52.2: the rename answers with the file's new profile-relative
        // subpath, and a pane left on the old one renders "is no longer in
        // tgdrive" over a file that merely changed its name. So the panes showing
        // it follow it. The subpath is passed through untouched (AD-65).
        //
        // Matched on the TARGET, never on which pane has focus, and that is the
        // difference between following a rename and hijacking a pane. The
        // requirement is "do not move a pane that is not showing this file", and
        // the target says that exactly: a pane showing the file follows whether
        // or not it is the focused one, a pane showing something else is left
        // alone, and a focus change while the round trip is in flight — a whole
        // `syncReadFrontmatter` plus `sessionsFileRename` — can no longer decide
        // which pane a rename moves. `retargetPanels` rather than the section's
        // `onOpen` for the same reason `onOpen` was right for the row's click and
        // wrong here: the opener moves the ACTIVE pane, which after a rename is
        // whichever one the reader happened to click into last.
        panelsStore.getState().retargetPanels(target, {
          kind: "file",
          profileId: rootId,
          relativePath: nextSubpath,
        });
        onChanged();
      })
      .catch((raw: unknown) => onNotice(syncErrorMessage(raw, SPACE_ROW_RENAME_FAILED)));
  };

  const confirmDelete = (): void => {
    setDeleting(false);
    onNotice(null);
    void sessionsFileDelete(rootId, sessionId, relPath)
      .then(() => {
        onChanged();
      })
      .catch((raw: unknown) => onNotice(syncErrorMessage(raw, SPACE_ROW_DELETE_FAILED)));
  };

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild onKeyDown={openWithoutAPointer}>
          {children}
        </ContextMenuTrigger>
        {/* The rules separate what happens in this window from what leaves it,
            from what only names the file, from what changes it — the Files
            pane's grouping, plus the two write verbs it does not have. Delete is
            last so the item under the cursor when the menu opens is never the
            one that removes the file. */}
        <ContextMenuContent>
          {/* The three verbs that address a FILE, absent on a directory. */}
          {!directory && (
            <>
              <ContextMenuItem onSelect={() => onOpen(subpath)}>
                {SPACE_ROW_OPEN_HERE_LABEL}
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => panelsStore.getState().openPanel(target)}>
                {SPACE_ROW_OPEN_BESIDE_LABEL}
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem
                onSelect={() => {
                  void syncOpenEntry(rootId, subpath).catch(() => undefined);
                }}
              >
                {SPACE_ROW_OPEN_LABEL}
              </ContextMenuItem>
            </>
          )}
          {canReveal && (
            <ContextMenuItem
              onSelect={() =>
                withPath((absolutePath) => {
                  void revealPath(absolutePath).catch(() => undefined);
                })
              }
            >
              {SPACE_ROW_REVEAL_LABEL}
            </ContextMenuItem>
          )}
          <ContextMenuItem
            onSelect={() =>
              withPath((absolutePath) => {
                // Best effort, exactly as the Files pane's is: a clipboard the
                // webview refuses is not worth a sentence, and the path is one
                // press from being on screen either way.
                void navigator.clipboard?.writeText(absolutePath).catch(() => undefined);
              })
            }
          >
            {SPACE_ROW_COPY_PATH_LABEL}
          </ContextMenuItem>
          <ContextMenuSeparator />
          {renamable && (
            <ContextMenuItem
              aria-label={SPACE_ROW_RENAME_LABEL}
              aria-describedby={keepsItsName ? `${hintId}-rename` : undefined}
              className={keepsItsName ? "flex-col items-start gap-0.5" : undefined}
              onSelect={() => {
                onNotice(null);
                setDraft(title);
                setRenaming(true);
              }}
            >
              <span>{SPACE_ROW_RENAME_LABEL}</span>
              {keepsItsName && (
                <span id={`${hintId}-rename`} className="text-muted-foreground text-xs">
                  {SPACE_ROW_RENAME_KEEPS_NAME}
                </span>
              )}
            </ContextMenuItem>
          )}
          <ContextMenuItem
            variant="destructive"
            disabled={deleteRefusal !== null}
            aria-label={SPACE_ROW_DELETE_LABEL}
            aria-describedby={deleteRefusal === null ? undefined : `${hintId}-delete`}
            className={deleteRefusal === null ? undefined : "flex-col items-start gap-0.5"}
            onSelect={() => {
              onNotice(null);
              setDeleting(true);
            }}
          >
            <span>{SPACE_ROW_DELETE_LABEL}</span>
            {deleteRefusal !== null && (
              <span id={`${hintId}-delete`} className="text-muted-foreground text-xs">
                {deleteRefusal}
              </span>
            )}
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>

      <AlertDialog open={renaming} onOpenChange={(next) => !next && setRenaming(false)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{SPACE_ROW_RENAME_TITLE}</AlertDialogTitle>
            {/* The record's dialog says what the record's rename does. The body
                above it promises a file that follows its title and links
                rewritten in the same write, and for the three names in
                `RECORD_NAMES` neither half happens — a confirmation that
                promised a move keeper will not make would be the one sentence
                here that is not true. */}
            <AlertDialogDescription>
              {relPath} — {keepsItsName ? SPACE_ROW_RENAME_KEEPS_NAME : SPACE_ROW_RENAME_BODY}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <Input
            autoFocus
            value={draft}
            aria-label={SPACE_ROW_RENAME_FIELD_LABEL}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              // Enter commits, because a form with one field is a field somebody
              // types into and presses Enter on. Escape is the dialog's own.
              if (event.key === "Enter" && draft.trim() !== "") {
                event.preventDefault();
                commitRename();
              }
            }}
          />
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction disabled={draft.trim() === ""} onClick={commitRename}>
              {SPACE_ROW_RENAME_LABEL}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* The session tree's confirmation, worded the same: the file is named in
          it, because "delete this file?" over a list of nine is a question about
          which one. */}
      <AlertDialog open={deleting} onOpenChange={(next) => !next && setDeleting(false)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{SPACE_ROW_DELETE_TITLE}</AlertDialogTitle>
            <AlertDialogDescription>
              {relPath} — {SPACE_ROW_DELETE_BODY}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>{SPACE_ROW_DELETE_LABEL}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
