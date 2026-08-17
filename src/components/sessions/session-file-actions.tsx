/**
 * Growing the pool (FR-262): the buttons that add a file to a session.
 *
 * They sit on the Files heading rather than under the tree, because they act on
 * the thing the heading names and a person looking for "how do I add one" looks
 * where the list begins. The tree itself stays what its own doc calls it — a
 * review surface — and gains only the one per-row verb that is about a row.
 *
 * **Four buttons for three ideas.** *New log* and *New prompt* write a file whose
 * NAME and TAG keeper chooses (`YYYY-MM-DD-HHMM-slug.md`, `tags: [log]`), and
 * that choosing is the entire point: in a flat session those two fields are what
 * decide whether the zone's spaces will ever list the file, so a log the
 * operator named freehand is a log nothing can find. *New file* is the general
 * escape hatch — any name, three extensions, any folder — and it deliberately
 * writes NO kind tag, because keeper does not know what an operator's new file
 * is and guessing `log` would file a stray thought as history. The detail's
 * *unfiled* list is where such a file surfaces, with the sentence that says how
 * to file it.
 *
 * **The fourth is *New folder*** (FR-287), and it is a different idea from the
 * other three: it writes no bytes at all. A session's kinds are still tags and
 * never directories (AD-120) — a folder here is a *container*, for what is not
 * markdown or for thirty of something — and the markdown inside one is read
 * exactly as markdown in the session root is (FR-285). Its field takes the
 * **path inside the session** rather than a name plus the folder menu, which is
 * the Templates room's own shape: only the last segment is folded to a slug and
 * the ones in front of it address folders already on the drive, so joining a
 * picked folder onto a typed name in here would be this surface composing a path
 * (AD-65) with a fold applied to half of it.
 *
 * **The log button is shape-aware.** A folder-shaped session's log lives in
 * `## Log` inside its README and `sessionsLogToday` appends a heading there; a
 * flat one's log is a file. Same button, same words, two commands — picked on
 * `shape`, because offering both would ask the operator to know which contract
 * their own session follows.
 *
 * Nothing here composes a path or a filename (AD-65): the title goes to Rust and
 * the subpath comes back, which is what opens the file in the one editor
 * (AD-109). A namer here would be a second namer, and the two would disagree
 * about collisions the moment an agent wrote a file between the read and the
 * create.
 *
 * **The in-flight flag is not this component's.** Every create here posts an
 * empty title, and so does the create in every writable space below
 * ({@link "@/components/sessions/session-spaces"}); Rust names such a file from
 * the clock to the minute, so two of them started in the same minute resolve to
 * one filename and the second write wins. The flag that removes that press has
 * to span both surfaces, so `SessionDetail` holds it and both children are
 * handed it.
 *
 * The two menus are native `<select>` elements, matching
 * {@link "@/components/sessions/session-space-editor"} rather than the Radix
 * `Select` used elsewhere in the app. One of them must offer the session's own
 * root, whose value is the empty string, and Radix's `Select` throws on an
 * empty-valued item by design — a sentinel like `"__root__"` would then have to
 * be translated back into `""` on the way to Rust, which is a path composed in
 * the frontend wearing a disguise.
 */
import { FilePlus2, FolderPlus, MessageSquarePlus, NotebookPen } from "lucide-react";
import { useCallback, useId, useState } from "react";
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
import type { SessionEntryVm, SessionFileKind } from "@/lib/ipc/client";
import {
  sessionsDirNew,
  sessionsFileNew,
  sessionsFileNewKind,
  sessionsLogToday,
} from "@/lib/ipc/client";
import { panelsStore } from "@/lib/stores/panels";
import { syncErrorMessage } from "@/lib/stores/sync";

export const SESSION_FILE_NEW_LOG_LABEL = "New log";
export const SESSION_FILE_NEW_PROMPT_LABEL = "New prompt";
export const SESSION_FILE_NEW_LABEL = "New file";
export const SESSION_DIR_NEW_LABEL = "New folder";

/** The dialog's own words. */
export const SESSION_FILE_NEW_TITLE = "New file";
export const SESSION_FILE_NEW_BODY =
  "Named as you type it, in the folder you pick. A new markdown file declares no kind, so it lands under Unfiled until you tag it.";
export const SESSION_FILE_NEW_NAME_LABEL = "Title";
export const SESSION_FILE_NEW_KIND_LABEL = "Kind";
export const SESSION_FILE_NEW_FOLDER_LABEL = "Folder";
export const SESSION_FILE_NEW_CONFIRM = "Create";
export const SESSION_FILE_NEW_CANCEL = "Cancel";

/** The session's own root — the pool, and where a flat session's markdown goes. */
export const SESSION_FILE_ROOT_LABEL = "Session root";

/** Failures, in keeper's voice for when Rust has nothing more specific to say. */
export const SESSION_FILE_NEW_FAILED = "keeper couldn't create that file. Nothing was written.";
export const SESSION_FILE_NEW_LOG_FAILED = "keeper couldn't create that log. Nothing was written.";
export const SESSION_FILE_NEW_PROMPT_FAILED =
  "keeper couldn't create that prompt. Nothing was written.";
export const SESSION_DIR_NEW_FAILED = "keeper couldn't create that folder. Nothing was written.";

/**
 * The folder dialog's own words.
 *
 * The body names the one exception rather than promising what the reader does
 * not do: `dir_rel` refuses `workspace/` and nothing else, so `artifacts` is a
 * folder this dialog will happily make — and `sessions_root::UNSCANNED_DIRS`
 * skips it at every depth, so markdown created into it is in no pool, no space
 * and not even *Unfiled*. That is a legitimate folder to want (it is where
 * promoted output lives), so the honest fix is the sentence and not a refusal.
 * The example moved off `artifacts/` for the same reason: an example is the
 * strongest thing in a dialog, and this one used to point at the one folder
 * whose markdown vanishes.
 */
export const SESSION_DIR_NEW_TITLE = "New folder";
export const SESSION_DIR_NEW_BODY =
  "The path inside this session — log at its root, log/2026 inside a folder that is already there. Only the last part is folded to a name; markdown you put in it is read like markdown in the root, except in artifacts, which keeper leaves to the tools that write there. Kinds are still tags, so a folder is a container and not a filing system.";
export const SESSION_DIR_NEW_NAME_LABEL = "Folder";

/** The three writable extensions, in the order the menu offers them. */
export const SESSION_FILE_KINDS: readonly { value: SessionFileKind; label: string }[] = [
  { value: "md", label: "Markdown (.md)" },
  { value: "csv", label: "Table (.csv)" },
  { value: "json", label: "Data (.json)" },
];

export interface SessionFileActionsProps {
  rootId: string;
  sessionId: string;
  /** Which contract this session follows — the log button branches on it. */
  shape: string;
  /**
   * The session's own tree, for the folder menu.
   *
   * Read from the entries already on screen rather than fetched again: the menu
   * offers folders that are *visible*, so a folder in the list is one the
   * operator can see. `workspace/` rows arrive carrying the write fence's own
   * refusal in `locked`, and are dropped here for that reason — offering a
   * destination keeper is going to refuse is a control that exists to fail.
   */
  entries: readonly SessionEntryVm[];
  /**
   * Whether a create is already in flight ANYWHERE on this session, and the
   * way to claim or release that.
   *
   * **Held by `SessionDetail`, shared with `SessionSpaces`.** Both surfaces
   * post `sessions_file_new_kind` with an empty title, which names the file
   * from the clock to the minute — so *New prompt* here and *New note* in a
   * space, pressed in the same minute, both resolve to
   * `YYYY-MM-DD-HHMM-untitled.md` and the second `WriteFile` silently
   * overwrites the first. A flag private to this component only removed half
   * of that press. See {@link "@/components/sessions/session-spaces"}'s
   * `writing` prop, which is the same flag.
   */
  busy: boolean;
  onBusy: (busy: boolean) => void;
  /** Re-read the surface after a write, without waiting on the watcher. */
  onChanged: () => void;
}

export function SessionFileActions({
  rootId,
  sessionId,
  shape,
  entries,
  busy,
  onBusy,
  onChanged,
}: SessionFileActionsProps) {
  const titleId = useId();
  const kindId = useId();
  const folderId = useId();
  const dirId = useId();
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [kind, setKind] = useState<SessionFileKind>("md");
  const [parent, setParent] = useState("");
  const [dirOpen, setDirOpen] = useState(false);
  const [dirPath, setDirPath] = useState("");
  const [notice, setNotice] = useState<string | null>(null);

  // Open what was just written, through the one file target (AD-109), on the
  // subpath Rust composed (AD-65). Writing a file and leaving the operator to
  // hunt for it in a tree that has not re-read yet is half a verb; this is the
  // other half.
  const opened = useCallback(
    (subpath: string) => {
      panelsStore.getState().setActiveTarget({
        kind: "file",
        profileId: rootId,
        relativePath: subpath,
      });
      onChanged();
    },
    [rootId, onChanged],
  );

  const newKind = useCallback(
    (tag: string, fallback: string) => {
      onBusy(true);
      setNotice(null);
      sessionsFileNewKind(rootId, sessionId, tag, "")
        .then(opened)
        .catch((raw: unknown) => setNotice(syncErrorMessage(raw, fallback)))
        .finally(() => onBusy(false));
    },
    [rootId, sessionId, opened, onBusy],
  );

  const newLog = useCallback(() => {
    if (shape === "flat") {
      newKind("log", SESSION_FILE_NEW_LOG_FAILED);
      return;
    }
    // The folder contract's log is a heading inside README.md, and this is the
    // command that writes one. It answers with nothing to open — the README was
    // already reachable — so this arm only re-reads.
    onBusy(true);
    setNotice(null);
    sessionsLogToday(rootId, sessionId)
      .then(() => onChanged())
      .catch((raw: unknown) => setNotice(syncErrorMessage(raw, SESSION_FILE_NEW_LOG_FAILED)))
      .finally(() => onBusy(false));
  }, [shape, rootId, sessionId, onChanged, newKind, onBusy]);

  const newPrompt = useCallback(() => newKind("prompt", SESSION_FILE_NEW_PROMPT_FAILED), [newKind]);

  const create = useCallback(() => {
    onBusy(true);
    setNotice(null);
    sessionsFileNew(rootId, sessionId, parent, title, kind)
      .then((subpath) => {
        setOpen(false);
        setTitle("");
        opened(subpath);
      })
      .catch((raw: unknown) => setNotice(syncErrorMessage(raw, SESSION_FILE_NEW_FAILED)))
      .finally(() => onBusy(false));
  }, [rootId, sessionId, parent, title, kind, opened, onBusy]);

  // A folder write answers nothing to open — there is no file, and the tree is
  // where the row appears — so this only re-reads. `onChanged` is what makes the
  // new folder a row, and therefore an option in *New file*'s Folder menu.
  const createDir = useCallback(() => {
    onBusy(true);
    setNotice(null);
    sessionsDirNew(rootId, sessionId, dirPath)
      .then(() => {
        setDirOpen(false);
        setDirPath("");
        onChanged();
      })
      .catch((raw: unknown) => setNotice(syncErrorMessage(raw, SESSION_DIR_NEW_FAILED)))
      .finally(() => onBusy(false));
  }, [rootId, sessionId, dirPath, onChanged, onBusy]);

  const folders = entries.filter((entry) => entry.isDir && entry.locked === null);

  return (
    <>
      <div className="flex items-center gap-1">
        <Button type="button" variant="ghost" size="sm" onClick={newLog} disabled={busy}>
          <NotebookPen aria-hidden className="size-3.5" />
          {SESSION_FILE_NEW_LOG_LABEL}
        </Button>
        {/* Offered under both contracts (Story 50.1). The gate that used to sit
            here was `shape === "flat"`, and its recorded reason —
            "a folder-shaped session keeps its prompts in `prompts/`, where the
            kind is the directory; a tagged file there would be filed twice" —
            was never true of the reader: `pool::read_one` derives a kind from
            tags alone (AD-120) and never looks at the path, so a tagged file in
            `prompts/` is filed once and an untagged one is filed not at all.
            The real reason was the writer: `sessions_file_new_kind` wrote into
            the session ROOT, which a folder-shaped session's pool does not
            read. It now asks `shape::kind_dir` and writes into `prompts/`, so
            the gate is a leftover and not a guard. A kind this shape genuinely
            has no home for is refused by Rust with its own sentence, which
            lands in the notice below. */}
        <Button type="button" variant="ghost" size="sm" onClick={newPrompt} disabled={busy}>
          <MessageSquarePlus aria-hidden className="size-3.5" />
          {SESSION_FILE_NEW_PROMPT_LABEL}
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => {
            setNotice(null);
            setOpen(true);
          }}
          disabled={busy}
        >
          <FilePlus2 aria-hidden className="size-3.5" />
          {SESSION_FILE_NEW_LABEL}
        </Button>
        {/* Always visible, like its three siblings and unlike anything gated on
            `shape`: a container is a container under both contracts, and the two
            paths keeper refuses — `workspace/` and anything that leaves the
            session — are refused by Rust with its own sentence rather than by a
            button that is not there. */}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => {
            setNotice(null);
            setDirOpen(true);
          }}
          disabled={busy}
        >
          <FolderPlus aria-hidden className="size-3.5" />
          {SESSION_DIR_NEW_LABEL}
        </Button>
      </div>
      {/* One failure, said once (UX-DR43). The dialog owns the sentence while
          it is open — a refusal repeated above and behind the modal reads as two
          things having gone wrong. */}
      {notice !== null && !open && !dirOpen && (
        <p role="status" className="text-destructive text-xs">
          {notice}
        </p>
      )}

      <Dialog
        open={open}
        onOpenChange={(next) => {
          if (busy) return;
          setOpen(next);
        }}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{SESSION_FILE_NEW_TITLE}</DialogTitle>
            <DialogDescription>{SESSION_FILE_NEW_BODY}</DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor={titleId}>{SESSION_FILE_NEW_NAME_LABEL}</Label>
              <Input id={titleId} value={title} onChange={(e) => setTitle(e.target.value)} />
            </div>
            <div className="flex flex-wrap items-end gap-3">
              <div className="flex min-w-32 flex-1 flex-col gap-1.5">
                <Label htmlFor={kindId}>{SESSION_FILE_NEW_KIND_LABEL}</Label>
                <select
                  id={kindId}
                  value={kind}
                  onChange={(e) => setKind(e.target.value as SessionFileKind)}
                  className="h-9 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                >
                  {SESSION_FILE_KINDS.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="flex min-w-32 flex-1 flex-col gap-1.5">
                <Label htmlFor={folderId}>{SESSION_FILE_NEW_FOLDER_LABEL}</Label>
                <select
                  id={folderId}
                  value={parent}
                  onChange={(e) => setParent(e.target.value)}
                  className="h-9 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                >
                  <option value="">{SESSION_FILE_ROOT_LABEL}</option>
                  {folders.map((folder) => (
                    <option key={folder.relPath} value={folder.relPath}>
                      {folder.relPath}
                    </option>
                  ))}
                </select>
              </div>
            </div>
          </div>
          {notice !== null && (
            <p role="status" className="text-destructive text-sm">
              {notice}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => setOpen(false)} disabled={busy}>
              {SESSION_FILE_NEW_CANCEL}
            </Button>
            <Button type="button" onClick={create} disabled={busy || title.trim() === ""}>
              {SESSION_FILE_NEW_CONFIRM}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={dirOpen}
        onOpenChange={(next) => {
          if (busy) return;
          setDirOpen(next);
        }}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{SESSION_DIR_NEW_TITLE}</DialogTitle>
            <DialogDescription>{SESSION_DIR_NEW_BODY}</DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor={dirId}>{SESSION_DIR_NEW_NAME_LABEL}</Label>
            <Input id={dirId} value={dirPath} onChange={(e) => setDirPath(e.target.value)} />
          </div>
          {notice !== null && (
            <p role="status" className="text-destructive text-sm">
              {notice}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => setDirOpen(false)} disabled={busy}>
              {SESSION_FILE_NEW_CANCEL}
            </Button>
            {/* An empty field is a question nobody answered rather than a
                refusal to report, so the confirm is inert and Rust is never
                asked — the Templates room's rule, and the same one *New file*
                applies to its title. */}
            <Button type="button" onClick={createDir} disabled={busy || dirPath.trim() === ""}>
              {SESSION_FILE_NEW_CONFIRM}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
