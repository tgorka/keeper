/**
 * The panel strip: the shell's document area (Story 45.1, FR-173, AD-90,
 * UX-DR65).
 *
 * One host over {@link "@/lib/stores/panels"}. It renders every panel left to
 * right, resolves each panel's target every time it shows it, and hands the
 * resolved file to the one viewer registry (Story 45.2, AD-87) rather than
 * deciding anything about formats itself.
 *
 * # Resolution is the point, not an error path
 *
 * A panel stores an identity and nothing else — no name, no size, no absolute
 * path (see `keeper_core::panels`). So showing a panel *is* resolving it, every
 * time: the drive may have been unplugged since the last render, the file may
 * have been renamed on another device, the profile may have been removed. A
 * target that no longer resolves renders the reason and **keeps its place**, so
 * the pane comes back when the drive does. Dropping it would be the same
 * mistake as an app that forgets your open tabs because the network blinked.
 *
 * Every reason a `file` target cannot resolve is composed in Rust and rendered
 * verbatim: `keeper_sync::browse` already words "this volume is not attached",
 * "something else is mounted there" and "this folder is not on disk", and it
 * words them from the same function the Files tree shows them from. Two
 * surfaces wording the absent drive differently is how a user concludes they
 * are two different problems.
 */
import { X } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { ExportFileButton } from "@/components/export/export-file-button";
import {
  FOLD_STRIP,
  FOLD_STRIP_SLOT,
  FoldStripHead,
  FoldStripName,
} from "@/components/layout/fold-strip";
import { PaneHeader } from "@/components/layout/pane-header";
import { deriveTitle, NoteEditor } from "@/components/notes/note-editor";
import { Button } from "@/components/ui/button";
import {
  type FilesEntryVm,
  type FilesListingVm,
  type IpcError,
  type NoteVaultVm,
  notesBodyRead,
  type PanelTargetVm,
  syncBrowse,
} from "@/lib/ipc/client";
import { useNoteDocument } from "@/lib/stores/notes-editor";
import { useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { type Panel, panelsStore, usePanelsStore } from "@/lib/stores/panels";
import { cn } from "@/lib/utils";
import {
  openWithForProfileEntry,
  type ResolvedViewer,
  type ViewerFile,
  viewerComponentFor,
} from "@/lib/viewers";

/** The accessible name of the strip itself. */
export const PANEL_STRIP_LABEL = "Open panels";

/** What the one panel a fresh keeper starts with says. Not an error: nothing is
 *  wrong, nothing has been opened yet, and the sentence says which. */
export const PANEL_EMPTY_SENTENCE = "Nothing is open here yet. Click a file to open it.";

/** The close control on a panel. The last panel has none — see
 *  {@link "@/lib/stores/panels"}'s `closePanel`; a control that refuses on
 *  activation is worse than a control that is not there. */
export const PANEL_CLOSE_LABEL = "Close panel";

/**
 * The fold control, in both of its states (Story 46.13, FR-217).
 *
 * Two labels rather than one toggle word, because the accessible name of a
 * control should say what pressing it does. The `aria-expanded` on the button
 * says which state it is in; the name says which way it goes.
 *
 * A folded panel keeps its place in the strip and its target, and gives up its
 * width. That is what makes it a different act from closing: the last panel may
 * be folded, because the control that undoes it is sitting where the panel was.
 */
export const PANEL_FOLD_LABEL = "Fold panel";
export const PANEL_UNFOLD_LABEL = "Unfold panel";

/** What a panel says while it is finding out whether its target is still there. */
export const PANEL_RESOLVING_SENTENCE = "Reading…";

/** What a panel whose profile is gone says. Distinct from the drive being out:
 *  the folder was removed from keeper, and plugging anything in will not
 *  bring it back. */
export const PANEL_NO_PROFILE_SENTENCE =
  "The folder this file was in is no longer set up in keeper.";

/** What a note panel says when its vault is no longer configured. */
export const PANEL_NO_VAULT_SENTENCE = "The vault this note was in is no longer set up in keeper.";

/**
 * What a panel says for a target this build has no way to show.
 *
 * Reached only by a `recording` target today: nothing in wave 1 opens one, and
 * Story 45.19 is where the Recording surface starts producing them. The panel
 * says so and keeps its place rather than rendering an empty frame — the same
 * rule the registry applies to an unbound viewer id, and for the same DW-172
 * reason: a silent blank pane is a defect nobody can see.
 */
export const PANEL_UNSUPPORTED_SENTENCE = "keeper cannot show a recording in a panel yet.";

/** Test id for one panel frame, suffixed with the panel's id. */
export const PANEL_TESTID = "panel";

/** Test id for the sentence a panel renders instead of its target. A slot, so a
 *  test asserts the sentence rather than re-deriving it. */
export const PANEL_REASON_TESTID = "panel-reason";

/** The last segment of a profile-relative path — the file's own name.
 *
 * Splitting a relative path is not joining a root: AD-65 forbids the frontend
 * composing a location, and this composes nothing. It is used only to name a
 * file that could NOT be resolved, where there is no `FilesEntryVm` to take a
 * name off. */
function fileNameOf(relativePath: string): string {
  const at = relativePath.lastIndexOf("/");
  return at === -1 ? relativePath : relativePath.slice(at + 1);
}

/** The folder a profile-relative path sits in, `""` for the profile root. The
 *  argument `sync_browse` takes to list the directory this file should be in. */
function parentOf(relativePath: string): string {
  const at = relativePath.lastIndexOf("/");
  return at === -1 ? "" : relativePath.slice(0, at);
}

/** What a panel says when its folder listed and its file was not in it. Names
 *  the file, because "not found" without a name is a sentence about nothing. */
export function panelFileGoneSentence(name: string): string {
  return `keeper could not find ${name} in that folder any more.`;
}

/** Structural guard for the IpcError envelope surfaced on a rejection. */
function isIpcError(value: unknown): value is IpcError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof value.code === "string" &&
    "message" in value &&
    typeof value.message === "string"
  );
}

/** What resolving a file target produced. */
type FileResolution =
  | { readonly status: "resolving" }
  | { readonly status: "resolved"; readonly entry: FilesEntryVm }
  | { readonly status: "unresolved"; readonly reason: string };

/** Turn one listing into this panel's answer about one file in it. */
function resolveFrom(listing: FilesListingVm, relativePath: string): FileResolution {
  if (listing.state !== "listed" || listing.entries === null) {
    return {
      status: "unresolved",
      // Rust composed this sentence for exactly this state and the Files tree
      // shows the same one; the fallback covers a state that carries none.
      reason: listing.detail ?? panelFileGoneSentence(fileNameOf(relativePath)),
    };
  }
  const entry = listing.entries.find((candidate) => candidate.relativePath === relativePath);
  if (entry === undefined) {
    return { status: "unresolved", reason: panelFileGoneSentence(fileNameOf(relativePath)) };
  }
  return { status: "resolved", entry };
}

/**
 * Resolve a file target against what is on disk right now.
 *
 * Lists the file's own folder rather than asking for the file, because
 * `sync_browse` is the ONE directory reader (AD-74) and it already carries the
 * containment rule, the volume check and the Rust-composed sentence for every
 * way a folder can fail to be readable. A second command that stat'ed one path
 * would be a second place those rules live.
 *
 * **Lifted into {@link PanelFrame} by Story 53.3, exactly as
 * {@link noteVaultReason} was by 50.1 and for the same reason**: two decisions
 * now turn on this answer and they must not be able to disagree — what the body
 * draws, and whether this panel draws a header row at all. `null` for a target
 * that is not a file, and `null` while the panel is folded, which is what keeps
 * a folded panel from reading a directory nobody can see (its body is unmounted,
 * so this used to stop happening by construction).
 */
function useFileResolution(target: PanelTargetVm | null, folded: boolean): FileResolution | null {
  const profileId = target?.kind === "file" ? target.profileId : null;
  const relativePath = target?.kind === "file" ? target.relativePath : null;
  const [resolution, setResolution] = useState<FileResolution | null>(null);

  useEffect(() => {
    if (profileId === null || relativePath === null || folded) {
      setResolution(null);
      return;
    }
    let live = true;
    setResolution({ status: "resolving" });
    syncBrowse(profileId, parentOf(relativePath))
      .then((listing) => {
        if (live) {
          setResolution(resolveFrom(listing, relativePath));
        }
      })
      .catch((error: unknown) => {
        if (live) {
          setResolution({
            status: "unresolved",
            // Rust words a refused or unknown profile; anything else is shown
            // as it arrived rather than replaced with a guess.
            reason: isIpcError(error) ? error.message : PANEL_NO_PROFILE_SENTENCE,
          });
        }
      });
    return () => {
      live = false;
    };
  }, [profileId, relativePath, folded]);

  return resolution;
}

/** The sentence a panel shows instead of its target. */
function PanelReason({ reason }: { reason: string }) {
  return (
    <p
      data-testid={PANEL_REASON_TESTID}
      className="px-4 py-3 text-muted-foreground text-sm"
      role="status"
    >
      {reason}
    </p>
  );
}

/**
 * What a file panel has to show, once its folder has answered (Story 53.3).
 *
 * The pair of states a body can render, and the union is what makes the frame's
 * one decision expressible: only `resolved` can carry a viewer, and only a
 * viewer can promise a header row. `null` — the absence of this whole value — is
 * the resolution still being in flight, or a target that is not a file, or a
 * folded panel that is reading nothing.
 */
type FilePanelView =
  | { readonly status: "unresolved"; readonly reason: string }
  | { readonly status: "resolved"; readonly view: ResolvedViewer & { readonly file: ViewerFile } };

/**
 * One resolved file, ready to hand to the registry (Story 53.3).
 *
 * Built where the resolution is now read, so the frame and the body cannot end
 * up holding two `ViewerFile`s for one row — and so the registry is asked once
 * per resolution rather than once per body render. The decision the frame needs
 * from it is {@link ResolvedViewer.ownsHostRow}; the body needs everything else.
 */
function viewerFor(profileId: string, entry: FilesEntryVm): ResolvedViewer & { file: ViewerFile } {
  const file: ViewerFile = {
    name: entry.name,
    kind: entry.kind,
    relativePath: entry.relativePath,
    profileId,
    // Rust composed it; the panel only passes it on, and only as an action's
    // argument (AD-65). A panel restored where the drive is out never gets this
    // far, so no viewer is ever handed a stale absolute path.
    absolutePath: entry.absolutePath,
    sizeLabel: entry.size?.label ?? null,
    openWith: openWithForProfileEntry(profileId, entry.relativePath),
    // AD-102's standing sentence for a file keeper will write and does not
    // manage. Composed in Rust and carried on the listing row, so the panel
    // neither words it nor decides when it applies.
    writeCaveat: entry.write.caveat,
    // And the one-line form of it, composed in Rust beside the other (Story
    // 53.3): the surface folds the sentence, and never by clipping it.
    writeCaveatShort: entry.write.caveatShort,
    // And the other verdict on the same row: why keeper will not write HERE.
    // `reason` is `Some` exactly when `writable` is false, so the conditional
    // is reading the pair as Rust guarantees it rather than defending against
    // it. The workspace fence (AD-113) arrives this way — `sync_browse` builds
    // the scope with the profile's sessions zone named, so a `workspace/` file
    // is refused on the listing, before any surface offers to edit it.
    writeRefusal: entry.write.writable ? null : entry.write.reason,
  };
  return { ...viewerComponentFor(file), file };
}

/** A file target: resolved by the frame above, drawn by the registry. `frame` is
 *  the host's controls, non-null only when the resolved viewer promised to draw
 *  a row for them (Story 53.3). */
function FilePanelBody({
  view,
  frame,
}: {
  view: ResolvedViewer & { file: ViewerFile };
  frame: ReactNode;
}) {
  return <view.Component file={view.file} entry={view.entry} frame={frame} />;
}

/**
 * Why a note panel cannot show its editor — or null, when it can (Story 50.1).
 *
 * Pure, and lifted out of the body, because two decisions now turn on it and
 * they must not be able to disagree: what the body draws, and whether the
 * FRAME draws a header row of its own. Since 50.1 a note panel that mounts its
 * editor draws no panel header at all — the editor's own row carries the
 * panel's fold and close — so a note whose vault is gone would have had no
 * header, and therefore no way to close the panel, if the two answers were
 * derived separately and came apart.
 */
export function noteVaultReason(
  vaults: readonly NoteVaultVm[] | null,
  vaultId: string,
): string | null {
  if (vaults === null) {
    return PANEL_RESOLVING_SENTENCE;
  }
  return vaults.some((vault) => vault.id === vaultId) ? null : PANEL_NO_VAULT_SENTENCE;
}

/** A note target: the vault has to exist before the editor can open anything.
 *  `reason` is {@link noteVaultReason}'s answer, decided by the frame above so
 *  that the frame knows whether this is going to draw the panel's header. */
function NotePanelBody({
  vaultId,
  noteId,
  reason,
  frame,
}: {
  vaultId: string;
  noteId: string;
  reason: string | null;
  frame: ReactNode;
}) {
  const onOpenNote = useCallback(
    (next: string) =>
      panelsStore.getState().setActiveTarget({ kind: "note", vaultId, noteId: next }),
    [vaultId],
  );

  if (reason !== null) {
    return <PanelReason reason={reason} />;
  }
  return <NoteEditor vaultId={vaultId} noteId={noteId} onOpenNote={onOpenNote} frame={frame} />;
}

/** What one panel is showing.
 *
 *  `noteReason`, `fileView` and `frame` are all the FRAME's answers, decided
 *  above so that the frame knows whether the thing below it is going to draw the
 *  panel's header row. `frame` is non-null for exactly one of them at a time —
 *  see {@link PanelFrame} — and whichever body is not drawing the row ignores it. */
function PanelBody({
  target,
  emptySentence,
  noteReason,
  fileView,
  frame,
}: {
  target: PanelTargetVm | null;
  emptySentence: string;
  noteReason: string | null;
  fileView: FilePanelView | null;
  frame: ReactNode;
}) {
  if (target === null) {
    return <PanelReason reason={emptySentence} />;
  }
  switch (target.kind) {
    case "file":
      // `null` is the resolution not having landed yet, or having failed: the
      // frame holds the sentence for both, because the same answer decides
      // whether it kept its own row.
      if (fileView === null) {
        return <PanelReason reason={PANEL_RESOLVING_SENTENCE} />;
      }
      if (fileView.status === "unresolved") {
        return <PanelReason reason={fileView.reason} />;
      }
      return <FilePanelBody view={fileView.view} frame={frame} />;
    case "note":
      return (
        <NotePanelBody
          vaultId={target.vaultId}
          noteId={target.noteId}
          reason={noteReason}
          frame={frame}
        />
      );
    case "recording":
      return <PanelReason reason={PANEL_UNSUPPORTED_SENTENCE} />;
  }
}

/**
 * The note's own title, for a panel that has to say which note it is holding.
 *
 * Two sources, because a panel outlives the editor inside it. While the note is
 * open the title is the FIRST LINE OF THE BUFFER — the same derivation the
 * editor's own heading uses, so a title being typed and the name of the panel
 * holding it never disagree. Folded, there is no buffer: the editor is
 * unmounted, its mirror is dropped, and a panel restored from disk at launch
 * never had one. So a folded note panel reads the note once through
 * `notes_body_read`, which is the call Rust already provides for exactly this —
 * "the read half of the one read-modify-write a surface can do to a note it has
 * not opened in the editor" — and nothing else changes on that strip until it
 * is unfolded.
 *
 * `null` for every other kind of target, and `null` while the one read is in
 * flight: the caller falls back to naming the KIND, which is what a panel said
 * about every note before this.
 */
function useNoteTitle(
  vaultId: string | null,
  noteId: string | null,
  folded: boolean,
): string | null {
  const live = useNoteDocument(vaultId, noteId, (d) =>
    d.text === "" ? null : deriveTitle(d.text),
  );
  const [read, setRead] = useState<{ readonly key: string; readonly title: string } | null>(null);
  useEffect(() => {
    if (!folded || vaultId === null || noteId === null) {
      return;
    }
    let alive = true;
    notesBodyRead(vaultId, noteId).then(
      (body) => {
        if (alive) {
          setRead({ key: `${vaultId}\u0000${noteId}`, title: deriveTitle(body.text) });
        }
      },
      // A note that cannot be read is a note this strip cannot name, which is
      // the state it was already in. The unfolded panel says why; a 48px strip
      // has nowhere to put a sentence and no reason to shout.
      () => {},
    );
    return () => {
      alive = false;
    };
  }, [folded, vaultId, noteId]);
  if (live !== null) {
    return live;
  }
  if (vaultId === null || noteId === null || read === null) {
    return null;
  }
  return read.key === `${vaultId}\u0000${noteId}` ? read.title : null;
}

/** What the panel's header calls it, and what its folded spine reads.
 *
 *  A note is named by its own title where one could be resolved
 *  ({@link useNoteTitle}): "Note" over a strip standing beside three other
 *  panels answers the question a name is asked. */
function panelName(target: PanelTargetVm | null, noteTitle: string | null): string {
  if (target === null) {
    return "Panel";
  }
  switch (target.kind) {
    case "file":
      return fileNameOf(target.relativePath);
    case "note":
      return noteTitle ?? "Note";
    case "recording":
      return "Recording";
  }
}

/**
 * One panel: a header that names it, folds it and can close it, and the target
 * below.
 *
 * # Folded is a different frame, not a hidden body
 *
 * A folded panel renders **no** {@link PanelBody}, and that is deliberate rather
 * than incidental. A body kept mounted behind `hidden` would keep its
 * subscriptions, its `sync_browse` and its editor buffer alive, which is exactly
 * the cost the reader was trying to reclaim — and for a note panel it would keep
 * a document mirror open over a note nobody can see. It also drops `flex-1` and
 * the 280px floor, because the whole visible point of folding is that the
 * neighbours get the width.
 *
 * The header is {@link PaneHeader} (AD-104): identity absorbs the slack, the
 * actions sit last. A panel has no status element yet, so it passes none — see
 * that module for why an empty reserved slot is not the same thing as no slot.
 * Panels are `flex-1` inside a horizontally scrolling strip, so this header gets
 * NARROWER than the note editor's rather than wider, which is the regime the
 * shrink rules were written for.
 *
 * # And no header at all when what is below draws one
 *
 * Story 50.1 for a note, Story 53.3 for a file: the surface inside already draws
 * a row naming the same thing, so the panel hands its controls down instead of
 * drawing a second band above them. The two cases differ only in how the answer
 * is reached — a note's is a pure store read, and a file's is the folder listing
 * plus the registry's answer about what will draw it — and they meet in one
 * `frame` node so a note panel and a file panel cannot come to offer different
 * chrome.
 *
 * **The row is given up only when the thing below has PROMISED to draw one.** A
 * `.pdf` resolves to a viewer with no chrome; a listing that has not landed yet
 * and one that failed are sentences, and a sentence carries no fold and no
 * close. `ownsHostRow` is the registry's promise (`components.tsx`), read here
 * and never guessed at from a format — and `savable` is deliberately NOT part of
 * this decision, because it is decided inside the frame from a read that has not
 * happened yet. A frame handed the controls draws the row in every state,
 * including the four in which it used to draw none.
 */
function PanelFrame({
  panel,
  active,
  closable,
  emptySentence,
}: {
  panel: Panel;
  active: boolean;
  closable: boolean;
  emptySentence: string;
}) {
  const vaults = useNotesVaultsStore((s) => s.vaults);
  // Story 50.1: a note panel draws NO header of its own. The editor below it
  // already draws a row that says which note, where it lives and what can be
  // done to it, and the panel's row said `Note` — a word the note's own title
  // says better — for the price of a 40px band and a seam. So the panel hands
  // its two controls down instead of drawing a row to hold them.
  //
  // Only when the editor is what is going to be there, though. A note whose
  // vault is gone shows a sentence, and a sentence cannot carry a fold or a
  // close: that panel keeps its own row, and the ONE rule that decides which
  // it is is `noteVaultReason`, read here and passed down rather than asked
  // twice.
  const noteReason =
    panel.target?.kind === "note" ? noteVaultReason(vaults, panel.target.vaultId) : null;
  const noteOwnsRow = panel.target?.kind === "note" && noteReason === null;
  // Story 53.3's half of the same rule, and the reason the resolution moved up
  // here: `ownsHostRow` is only knowable once the folder has answered and the
  // registry has said what draws the row it answered with.
  const fileResolution = useFileResolution(panel.target, panel.folded);
  const fileProfileId = panel.target?.kind === "file" ? panel.target.profileId : null;
  const fileView = useMemo<FilePanelView | null>(() => {
    if (fileResolution === null || fileProfileId === null) {
      return null;
    }
    if (fileResolution.status === "resolving") {
      return null;
    }
    return fileResolution.status === "unresolved"
      ? { status: "unresolved", reason: fileResolution.reason }
      : { status: "resolved", view: viewerFor(fileProfileId, fileResolution.entry) };
  }, [fileResolution, fileProfileId]);
  const fileOwnsRow = fileView?.status === "resolved" && fileView.view.ownsHostRow;
  const noteTitle = useNoteTitle(
    panel.target?.kind === "note" ? panel.target.vaultId : null,
    panel.target?.kind === "note" ? panel.target.noteId : null,
    panel.folded,
  );
  const name = panelName(panel.target, noteTitle);
  const FoldGlyph = panel.folded ? FOLD_STRIP.unfoldIcon : FOLD_STRIP.foldIcon;
  // Folded, the tooltip and the accessible name are ONE string and it carries
  // the panel's name, because a folded panel has nothing else on screen: a
  // pointer that hovered a bare chevron would learn only that the strip folds,
  // not which of four files this one is. Open, the panel names itself an inch
  // to the left, so the control only has to say what it does. Whichever it is,
  // `title` and `aria-label` are the same words — a control whose tooltip and
  // whose spoken name disagree cannot be operated by anyone saying what they
  // see (WCAG 2.5.3), and with the text gone the tooltip IS the visible label.
  const foldName = panel.folded ? `${PANEL_UNFOLD_LABEL}: ${name}` : PANEL_FOLD_LABEL;
  const fold = (
    <Button
      type="button"
      variant="ghost"
      size={FOLD_STRIP.headControlSize}
      // The name says which way the control goes; `aria-expanded` says where it
      // is now.
      aria-expanded={!panel.folded}
      aria-label={foldName}
      title={foldName}
      className="shrink-0"
      onClick={() => panelsStore.getState().toggleFold(panel.id)}
    >
      <FoldGlyph aria-hidden="true" />
    </Button>
  );
  const close = closable ? (
    <Button
      type="button"
      variant="ghost"
      size="icon-sm"
      aria-label={PANEL_CLOSE_LABEL}
      title={PANEL_CLOSE_LABEL}
      className="shrink-0"
      onClick={() => panelsStore.getState().closePanel(panel.id)}
    >
      <X aria-hidden="true" />
    </Button>
  ) : null;
  // Story 45.21. A file only: a note panel's Export is in the editor's own
  // Actions menu, which is the surface that can flush the buffer before the
  // bytes are read off the disk. Two Export controls over one note, one of which
  // exported the last autosave, is the shape this placement exists to refuse.
  const exportFile =
    panel.target?.kind === "file" ? (
      <ExportFileButton
        profileId={panel.target.profileId}
        relativePath={panel.target.relativePath}
      />
    ) : null;
  // What the PANEL's controls are, wherever the row that carries them is drawn
  // — this frame's own header, the note editor's, or the file frame's. One node
  // either way, so a note panel and a file panel cannot come to offer different
  // chrome.
  //
  // Export travels with them for a FILE and not for a note, which is the same
  // rule as when this row was the panel's own: the control belongs to whoever
  // can read the bytes off the disk, and for a note that is the editor.
  const frame = (
    <>
      {exportFile}
      {fold}
      {close}
    </>
  );
  // Which surface below is drawing this panel's row, if either. At most one, and
  // the node goes to whichever it is: `PanelBody` hands it to the body it draws,
  // and a body that draws no row ignores it.
  const handedDown = noteOwnsRow || fileOwnsRow ? frame : null;
  return (
    <section
      aria-label={name}
      data-testid={`${PANEL_TESTID}-${panel.id}`}
      data-active={active ? "true" : undefined}
      data-folded={panel.folded ? "true" : undefined}
      data-fold-strip={panel.folded ? FOLD_STRIP_SLOT : undefined}
      // Clicking anywhere in a panel focuses it, which is what makes the next
      // single click in the browser replace THIS panel rather than the one that
      // happened to be focused before.
      onFocusCapture={() => panelsStore.getState().focusPanel(panel.id)}
      onMouseDown={() => panelsStore.getState().focusPanel(panel.id)}
      className={cn(
        "flex h-full flex-col overflow-hidden border-border border-r bg-background last:border-r-0",
        // A folded panel is a folded strip, and it is now the same 48px as
        // every other one (`fold-strip.tsx`). `w-auto` made its width a
        // consequence of whatever its one button happened to measure — the
        // only one of the four that nothing measured, and the reason four
        // strips side by side were not the same width.
        panel.folded ? cn(FOLD_STRIP.widthClass, "shrink-0 grow-0") : "min-w-[280px] flex-1",
        // The active mark is the ring. An inset ring draws on all four sides,
        // so on top of this panel's own trailing edge the right side would be
        // 2px while the other three are 1px — the panel cancels its border
        // rather than growing an edge, and DESIGN.md's hairline holds.
        active && "border-r-transparent ring-1 ring-ring ring-inset",
      )}
    >
      {panel.folded ? (
        // Folded: the control that undoes it, and the panel's name down the
        // strip. No body — see this function's header for why a folded panel
        // unmounts rather than hides.
        //
        // The band is {@link FoldStripHead}, which is this head, the drawer's
        // and every surface column's: 40px, DESIGN.md's `pane-header`, ending
        // in the rule that runs across every pane beside it. It used to be
        // spelled here, and the OTHER THREE strips were the ones that got it
        // wrong — 44px, with their divider 8px lower. `fold-strip.tsx` owns
        // the sum now, so there is nowhere left for the four to disagree.
        <>
          <FoldStripHead className="justify-center">{fold}</FoldStripHead>
          <FoldStripName name={name} />
        </>
      ) : noteOwnsRow || fileOwnsRow ? null : (
        <PaneHeader
          // No `border-b` and no `py-*`: `PaneHeader` owns its own bottom edge
          // and its own 40px height, and spelling either here draws it twice.
          className="px-3"
          // Deliberately not a heading. The viewer inside draws the document's
          // own heading, and a second `h2` naming the same file would put two
          // entries in a screen reader's heading list for one document. The
          // panel is named by the section's `aria-label`, which is how a reader
          // jumps between panels — a tab strip's job, not an outline's.
          //
          // The treatment is the shared one every foldable surface names itself
          // in, minus the heading semantics: DESIGN.md's `pane-header`
          // typography, which this row was the one place not to use.
          identity={<span className={FOLD_STRIP.titleClass}>{name}</span>}
          actions={frame}
        />
      )}
      {panel.folded ? null : (
        <div className="min-h-0 flex-1 overflow-auto">
          <PanelBody
            target={panel.target}
            emptySentence={emptySentence}
            noteReason={noteReason}
            fileView={fileView}
            // Handed to every body and consumed by exactly one: the note editor
            // or the file frame, whichever is drawing this panel's row. A body
            // that draws no row ignores it and the row above is this frame's.
            frame={handedDown}
          />
        </div>
      )}
    </section>
  );
}

/**
 * Every open panel, left to right.
 *
 * `emptySentence` is the one thing the host gets to say, and it exists because
 * the strip has two hosts since Story 46.12. The default names the gesture that
 * fills a panel in the Files surface; the Notes surface passes its own, because
 * "click a file to open it" is the wrong instruction beside a list of notes and
 * a first-run panel is the very first thing either surface shows. It is a prop
 * threaded to the frame rather than a module default or a store value, so the
 * sentence depends on which surface is rendering and not on which one mounted
 * last.
 */
export function PanelStrip({ emptySentence = PANEL_EMPTY_SENTENCE }: { emptySentence?: string }) {
  const panels = usePanelsStore((s) => s.panels);
  const activeId = usePanelsStore((s) => s.activeId);
  return (
    <section aria-label={PANEL_STRIP_LABEL} className="flex min-w-0 flex-1 overflow-x-auto">
      {panels.map((panel) => (
        <PanelFrame
          key={panel.id}
          panel={panel}
          active={panel.id === activeId}
          // The last panel cannot be closed, so it does not offer to be.
          closable={panels.length > 1}
          emptySentence={emptySentence}
        />
      ))}
    </section>
  );
}
