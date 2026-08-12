/**
 * The lifecycle verbs, on a row's overflow menu (Phase 7, FR-238..FR-248).
 *
 * The `note-actions.tsx` posture: verbs are thin calls into the typed client,
 * nothing here holds state, and the UI updates when the changed event streams
 * the new truth back — no optimistic overlay. Delete is behind its own
 * confirmed dialog (destruction earns a dialog, the note-delete rule);
 * unarchive offers continuation first and says why (FR-248).
 */
import { MoreHorizontal } from "lucide-react";
import { useState } from "react";
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { SessionRowVm } from "@/lib/ipc/client";
import {
  revealPath,
  sessionsArchive,
  sessionsCreateFrom,
  sessionsDelete,
  sessionsLogToday,
  sessionsSetPinned,
  sessionsUnarchive,
} from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";

export const SESSION_ACTIONS_LABEL = "Session actions";
export const SESSION_PIN_LABEL = "Pin";
export const SESSION_UNPIN_LABEL = "Unpin";
export const SESSION_LOG_TODAY_LABEL = "Log today";
export const SESSION_NEW_LIKE_THIS_LABEL = "New like this";
export const SESSION_ARCHIVE_LABEL = "Archive…";
export const SESSION_UNARCHIVE_LABEL = "Unarchive";
export const SESSION_DELETE_LABEL = "Delete…";
export const SESSION_REVEAL_LABEL = "Reveal in Finder";

/** The delete dialog's words: what goes, where it goes, and that it returns. */
export const SESSION_DELETE_TITLE = "Delete this session?";
export const SESSION_DELETE_BODY =
  "The whole folder — workspace included — moves into the zone's own trash (.keeper/trash), where it can be brought back. Nothing is erased.";
export const SESSION_DELETE_CONFIRM = "Delete session";

/**
 * The archive dialog's words, quoting the zone's own checklist. This story
 * runs the two fs steps (empty the workspace, file under archive/<year>); the
 * per-row promote review arrives with the promote panel story.
 */
export const SESSION_ARCHIVE_TITLE = "Archive this session?";
export const SESSION_ARCHIVE_BODY =
  "Per the zone's rules: the workspace is emptied (a .gitkeep stays), and the folder is filed under archive by the year it closed. Promote anything still in the workspace first — its contents do not survive archiving.";
export const SESSION_ARCHIVE_CONFIRM = "Archive session";

export interface SessionActionsProps {
  rootId: string;
  /** The zone's absolute root, for Reveal. */
  rootPath: string;
  row: SessionRowVm;
  /** Called with the new-like-this ref target after a successful create. */
  onCreatedFrom?: (rootId: string, path: string) => void;
}

export function SessionActions({ rootId, rootPath, row, onCreatedFrom }: SessionActionsProps) {
  const canReveal = useCapabilitiesStore((s) => s.capabilities.revealInFileManager);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [confirmArchive, setConfirmArchive] = useState(false);

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={SESSION_ACTIONS_LABEL}
            className="size-7"
          >
            <MoreHorizontal aria-hidden className="size-4" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onSelect={() => void sessionsSetPinned(rootId, row.id, !row.pinned)}>
            {row.pinned ? SESSION_UNPIN_LABEL : SESSION_PIN_LABEL}
          </DropdownMenuItem>
          {row.status === "active" && (
            <DropdownMenuItem onSelect={() => void sessionsLogToday(rootId, row.id)}>
              {SESSION_LOG_TODAY_LABEL}
            </DropdownMenuItem>
          )}
          <DropdownMenuItem
            onSelect={() => {
              void sessionsCreateFrom(rootId, row.id, row.title).then((ref) => {
                onCreatedFrom?.(ref.rootId, ref.path);
              });
            }}
          >
            {SESSION_NEW_LIKE_THIS_LABEL}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          {row.status === "active" ? (
            <DropdownMenuItem onSelect={() => setConfirmArchive(true)}>
              {SESSION_ARCHIVE_LABEL}
            </DropdownMenuItem>
          ) : (
            <DropdownMenuItem onSelect={() => void sessionsUnarchive(rootId, row.id)}>
              {SESSION_UNARCHIVE_LABEL}
            </DropdownMenuItem>
          )}
          {canReveal && (
            <DropdownMenuItem onSelect={() => void revealPath(`${rootPath}/${row.path}`)}>
              {SESSION_REVEAL_LABEL}
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onSelect={() => setConfirmDelete(true)}>
            {SESSION_DELETE_LABEL}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <AlertDialog open={confirmArchive} onOpenChange={setConfirmArchive}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{SESSION_ARCHIVE_TITLE}</AlertDialogTitle>
            <AlertDialogDescription>{SESSION_ARCHIVE_BODY}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={() => void sessionsArchive(rootId, row.id, [], true)}>
              {SESSION_ARCHIVE_CONFIRM}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{SESSION_DELETE_TITLE}</AlertDialogTitle>
            <AlertDialogDescription>{SESSION_DELETE_BODY}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={() => void sessionsDelete(rootId, row.id)}>
              {SESSION_DELETE_CONFIRM}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
