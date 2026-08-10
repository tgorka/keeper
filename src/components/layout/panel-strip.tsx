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
import { useCallback, useEffect, useState } from "react";
import { ExportFileButton } from "@/components/export/export-file-button";
import { NoteEditor } from "@/components/notes/note-editor";
import { Button } from "@/components/ui/button";
import {
  type FilesEntryVm,
  type FilesListingVm,
  type IpcError,
  type PanelTargetVm,
  syncBrowse,
} from "@/lib/ipc/client";
import { useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { type Panel, panelsStore, usePanelsStore } from "@/lib/stores/panels";
import { cn } from "@/lib/utils";
import { openWithForProfileEntry, type ViewerFile, viewerComponentFor } from "@/lib/viewers";

/** The accessible name of the strip itself. */
export const PANEL_STRIP_LABEL = "Open panels";

/** What the one panel a fresh keeper starts with says. Not an error: nothing is
 *  wrong, nothing has been opened yet, and the sentence says which. */
export const PANEL_EMPTY_SENTENCE = "Nothing is open here yet. Click a file to open it.";

/** The close control on a panel. The last panel has none — see
 *  {@link "@/lib/stores/panels"}'s `closePanel`; a control that refuses on
 *  activation is worse than a control that is not there. */
export const PANEL_CLOSE_LABEL = "Close panel";

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
 */
function useFileResolution(profileId: string, relativePath: string): FileResolution {
  const [resolution, setResolution] = useState<FileResolution>({ status: "resolving" });

  useEffect(() => {
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
  }, [profileId, relativePath]);

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

/** A file target: resolved through `sync_browse`, drawn by the registry. */
function FilePanelBody({ profileId, relativePath }: { profileId: string; relativePath: string }) {
  const resolution = useFileResolution(profileId, relativePath);

  if (resolution.status === "resolving") {
    return <PanelReason reason={PANEL_RESOLVING_SENTENCE} />;
  }
  if (resolution.status === "unresolved") {
    return <PanelReason reason={resolution.reason} />;
  }

  const entry = resolution.entry;
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
  };
  const { entry: viewerEntry, Component } = viewerComponentFor(file);
  return <Component file={file} entry={viewerEntry} />;
}

/** A note target: the vault has to exist before the editor can open anything. */
function NotePanelBody({ vaultId, noteId }: { vaultId: string; noteId: string }) {
  const vaults = useNotesVaultsStore((s) => s.vaults);
  const onOpenNote = useCallback(
    (next: string) =>
      panelsStore.getState().setActiveTarget({ kind: "note", vaultId, noteId: next }),
    [vaultId],
  );

  if (vaults === null) {
    return <PanelReason reason={PANEL_RESOLVING_SENTENCE} />;
  }
  if (!vaults.some((vault) => vault.id === vaultId)) {
    return <PanelReason reason={PANEL_NO_VAULT_SENTENCE} />;
  }
  return <NoteEditor vaultId={vaultId} noteId={noteId} onOpenNote={onOpenNote} />;
}

/** What one panel is showing. */
function PanelBody({ target }: { target: PanelTargetVm | null }) {
  if (target === null) {
    return <PanelReason reason={PANEL_EMPTY_SENTENCE} />;
  }
  switch (target.kind) {
    case "file":
      return <FilePanelBody profileId={target.profileId} relativePath={target.relativePath} />;
    case "note":
      return <NotePanelBody vaultId={target.vaultId} noteId={target.noteId} />;
    case "recording":
      return <PanelReason reason={PANEL_UNSUPPORTED_SENTENCE} />;
  }
}

/** What the panel's header calls it. A file has a name; nothing else does yet,
 *  and the note editor draws its own title under this. */
function panelName(target: PanelTargetVm | null): string {
  if (target === null) {
    return "Panel";
  }
  switch (target.kind) {
    case "file":
      return fileNameOf(target.relativePath);
    case "note":
      return "Note";
    case "recording":
      return "Recording";
  }
}

/** One panel: a header that names it and can close it, and the target below. */
function PanelFrame({
  panel,
  active,
  closable,
}: {
  panel: Panel;
  active: boolean;
  closable: boolean;
}) {
  const name = panelName(panel.target);
  return (
    <section
      aria-label={name}
      data-testid={`${PANEL_TESTID}-${panel.id}`}
      data-active={active ? "true" : undefined}
      // Clicking anywhere in a panel focuses it, which is what makes the next
      // single click in the browser replace THIS panel rather than the one that
      // happened to be focused before.
      onFocusCapture={() => panelsStore.getState().focusPanel(panel.id)}
      onMouseDown={() => panelsStore.getState().focusPanel(panel.id)}
      className={cn(
        "flex h-full min-w-[280px] flex-1 flex-col overflow-hidden border-border border-r bg-background last:border-r-0",
        active && "ring-1 ring-ring ring-inset",
      )}
    >
      <header className="flex shrink-0 items-center gap-2 border-border border-b px-3 py-2">
        {/* Deliberately not a heading. The viewer inside draws the document's
            own heading, and a second `h2` naming the same file would put two
            entries in a screen reader's heading list for one document. The
            panel is named by the section's `aria-label`, which is how a reader
            jumps between panels — a tab strip's job, not an outline's. */}
        <span className="min-w-0 flex-1 truncate font-medium text-sm">{name}</span>
        {/* Story 45.21. A file only: a note panel's Export is in the editor's
            own Actions menu, which is the surface that can flush the buffer
            before the bytes are read off the disk. Two Export controls over one
            note, one of which exported the last autosave, is the shape this
            placement exists to refuse. */}
        {panel.target?.kind === "file" && (
          <ExportFileButton
            profileId={panel.target.profileId}
            relativePath={panel.target.relativePath}
          />
        )}
        {closable && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-6 shrink-0 px-2 text-xs"
            onClick={() => panelsStore.getState().closePanel(panel.id)}
          >
            {PANEL_CLOSE_LABEL}
          </Button>
        )}
      </header>
      <div className="min-h-0 flex-1 overflow-auto">
        <PanelBody target={panel.target} />
      </div>
    </section>
  );
}

/** Every open panel, left to right. */
export function PanelStrip() {
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
        />
      ))}
    </section>
  );
}
