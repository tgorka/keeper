/**
 * Adding a reference (FR-265): the picker on the References heading.
 *
 * {@link "@/components/sessions/session-refs"} reads the pointers a session has
 * already written and says which of them broke. This writes one — and the two
 * share a vocabulary on purpose, because a picker that could add a reference its
 * own reader then called `missing` would be a feature that looks like it works
 * until somebody reopens the session.
 *
 * **One list, three sources.** Files, notes and recordings arrive already merged
 * and already ordered by Rust: the session's own files first, because a
 * reference is most often to something the sitting just produced, then the vault
 * newest first. Three tabs would make the operator decide *where* a thing lives
 * before they could look for it, which is the question they opened a search box
 * to avoid.
 *
 * **The search box does not filter the list it shows.** It re-asks. The list is
 * budgeted, so filtering what came back would filter the wrong page, and
 * `tag:project` is a question about the tag hierarchy — answered where the index
 * is (AD-7, AD-65). What is typed goes to Rust verbatim.
 *
 * **The promotion is an offer with its reason attached.** A `workspace/` file is
 * scratch that archiving empties, so a pointer into it is a dangling link with a
 * date on it — but the operator may mean the scratch file, and copying bytes
 * nobody asked to copy is how `artifacts/` fills up with a checkout. So: a
 * checkbox, checked by default, and the sentence saying what it is for. It
 * appears only on rows Rust marked promotable, which is the same fence that
 * would refuse the write (AD-113) — an offer can never appear on a file keeper
 * is going to say no to.
 *
 * **The dialog stays open after a successful add, showing the line as written.**
 * References come in handfuls — a sitting names the three documents it worked
 * from — so closing after each one would make the common case four round trips
 * through a button. Showing the line rather than "Added" is the same reasoning
 * as {@link "@/lib/ipc/client".sessionsSpacesRestore}: both cost the same to
 * send, and only one of them lets the operator see that keeper wrote what they
 * meant. Nothing here composes that markdown; Rust does, and this echoes it.
 */
import { AudioLines, FileText, Link2, Paperclip, Plus } from "lucide-react";
import { useCallback, useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import type { SessionRefAddedVm, SessionRefCandidateVm } from "@/lib/ipc/client";
import { sessionsRefAdd, sessionsRefCandidates } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

export const SESSION_ADD_REF_LABEL = "Add reference";

/** The dialog's own words. */
export const SESSION_ADD_REF_TITLE = "Add a reference";
export const SESSION_ADD_REF_BODY =
  "Point this session at a file, a note or a recording. Search by name, by path, or by tag with tag:name.";
export const SESSION_ADD_REF_SEARCH_LABEL = "Search";
export const SESSION_ADD_REF_URL_LABEL = "Or a link";
export const SESSION_ADD_REF_URL_PLACEHOLDER = "https://…";
export const SESSION_ADD_REF_ALIAS_LABEL = "Call it";
export const SESSION_ADD_REF_ALIAS_PLACEHOLDER = "Optional — the target names itself";
export const SESSION_ADD_REF_FILE_LABEL = "Write it in";
export const SESSION_ADD_REF_CONFIRM = "Add";
export const SESSION_ADD_REF_CLOSE = "Done";

/** The candidate list's accessible name, and the prefix one row's testid carries. */
export const SESSION_ADD_REF_LIST_LABEL = "Reference candidates";
export const SESSION_ADD_REF_ROW_TESTID = "session-ref-candidate";

/** What a search that matched nothing says — never an empty box (UX-DR44). */
export const SESSION_ADD_REF_NONE =
  "Nothing here matches that. Try a word from the name, part of the path, or tag: followed by a tag.";

/** How the file menu marks a file keeper would have to create first. */
export const SESSION_ADD_REF_NEW_FILE_SUFFIX = " — new file";

/** The promotion offer, and the reason it exists. */
export const SESSION_ADD_REF_PROMOTE_LABEL = "Copy it into artifacts/ first";
export const SESSION_ADD_REF_PROMOTE_BODY =
  "This file is in workspace/, which is scratch and is emptied when the session is archived. A reference into it will break; a copy in artifacts/ will not.";

/** The list hit its budget — named rather than hidden, as the tree's notice is. */
export const SESSION_ADD_REF_TRUNCATED =
  "Too many matches to list them all — narrow the search to see the rest.";

/** Failures, in keeper's voice for when Rust has nothing more specific to say. */
export const SESSION_ADD_REF_FAILED = "keeper couldn't add that reference. Nothing was written.";
export const SESSION_ADD_REF_LIST_FAILED = "keeper couldn't read what this session could point at.";

/** What the confirmation says above the line Rust wrote. */
export function addedSummary(added: SessionRefAddedVm): string {
  return added.promoted === null
    ? `Added to ${added.file}.`
    : `Added to ${added.file}, and copied to ${added.promoted}.`;
}

/**
 * The icon per kind, matching {@link "@/components/sessions/session-refs"}'s own
 * table so one thing keeps one glyph either side of the write.
 */
const KIND_ICON = {
  note: FileText,
  recording: AudioLines,
  file: Paperclip,
  external: Link2,
} as const;

function iconFor(kind: string) {
  return KIND_ICON[kind as keyof typeof KIND_ICON] ?? KIND_ICON.file;
}

export interface SessionAddRefProps {
  rootId: string;
  sessionId: string;
  /** Re-read the surface after a write, without waiting on the watcher. */
  onChanged: () => void;
}

export function SessionAddRef({ rootId, sessionId, onChanged }: SessionAddRefProps) {
  const searchId = useId();
  const urlId = useId();
  const aliasId = useId();
  const fileId = useId();
  const promoteId = useId();

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [url, setUrl] = useState("");
  const [alias, setAlias] = useState("");
  const [file, setFile] = useState("");
  const [promote, setPromote] = useState(true);
  const [picked, setPicked] = useState<SessionRefCandidateVm | null>(null);
  const [candidates, setCandidates] = useState<SessionRefCandidateVm[]>([]);
  const [targets, setTargets] = useState<string[]>([]);
  const [defaultTarget, setDefaultTarget] = useState("");
  const [truncated, setTruncated] = useState(false);
  const [busy, setBusy] = useState(false);
  const [added, setAdded] = useState<SessionRefAddedVm | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Re-ask on every keystroke, debounced, and ignore a reply a later query has
  // already superseded — `use-notes-search`'s own rule. Without the guard, a
  // slow answer to "re" lands after the answer to "report" and the list
  // contradicts the box the operator is looking at.
  useEffect(() => {
    if (!open) return;
    let live = true;
    const timer = setTimeout(() => {
      sessionsRefCandidates(rootId, sessionId, query)
        .then((vm) => {
          if (!live) return;
          setCandidates(vm.candidates);
          setTargets(vm.targets);
          setDefaultTarget(vm.defaultTarget);
          setTruncated(vm.truncated);
          // Adopt keeper's default only while the operator has not chosen: a
          // re-read on the next keystroke must not move the destination out
          // from under a file they already picked.
          setFile((current) => (current === "" ? vm.defaultTarget : current));
        })
        .catch((raw: unknown) => {
          if (!live) return;
          setNotice(syncErrorMessage(raw, SESSION_ADD_REF_LIST_FAILED));
        });
    }, 120);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [open, rootId, sessionId, query]);

  const typedUrl = url.trim() !== "";

  const add = useCallback(() => {
    const link = url.trim();
    const kind = link === "" ? picked?.kind : "external";
    const target = link === "" ? picked?.target : link;
    if (kind === undefined || target === undefined) return;

    setBusy(true);
    setNotice(null);
    setAdded(null);
    sessionsRefAdd(rootId, sessionId, {
      kind,
      target,
      label: alias.trim() === "" ? null : alias.trim(),
      file,
      // A URL is not in anybody's workspace, and a row Rust did not mark
      // promotable would have the flag ignored anyway — sent as false so the
      // request says what was meant rather than relying on that.
      promote: link === "" && promote && (picked?.promotable ?? false),
    })
      .then((vm) => {
        setAdded(vm);
        // Cleared so the next reference starts clean; the destination and the
        // search survive, because the next one is usually beside this one.
        setUrl("");
        setAlias("");
        setPicked(null);
        onChanged();
      })
      .catch((raw: unknown) => setNotice(syncErrorMessage(raw, SESSION_ADD_REF_FAILED)))
      .finally(() => setBusy(false));
  }, [rootId, sessionId, picked, url, alias, file, promote, onChanged]);

  // Keeper's default is a file it would create; it is only in `targets` once it
  // exists. Offered either way, marked when it is the one that does not exist
  // yet, so "write it in" never has an empty menu (UX-DR44).
  const fileOptions =
    defaultTarget === "" || targets.includes(defaultTarget) ? targets : [defaultTarget, ...targets];
  const ready = typedUrl || picked !== null;

  return (
    <>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={() => {
          setNotice(null);
          setAdded(null);
          setOpen(true);
        }}
      >
        <Plus aria-hidden className="size-3.5" />
        {SESSION_ADD_REF_LABEL}
      </Button>
      {/* One failure, said once (UX-DR43): the dialog owns the sentence while
          it is open. */}
      {notice !== null && !open && (
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
        <DialogContent className="sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>{SESSION_ADD_REF_TITLE}</DialogTitle>
            <DialogDescription>{SESSION_ADD_REF_BODY}</DialogDescription>
          </DialogHeader>

          <div className="flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor={searchId}>{SESSION_ADD_REF_SEARCH_LABEL}</Label>
              <Input
                id={searchId}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                disabled={typedUrl}
              />
            </div>

            {/* A list of buttons marked `aria-current`, matching `chat-row`:
                one row is the chosen one and the rest are ordinary controls. */}
            {!typedUrl && (
              <ul
                aria-label={SESSION_ADD_REF_LIST_LABEL}
                className="flex max-h-64 flex-col overflow-y-auto rounded-md border border-border"
              >
                {candidates.length === 0 ? (
                  <li className="px-2 py-3 text-muted-foreground text-xs">
                    {SESSION_ADD_REF_NONE}
                  </li>
                ) : (
                  candidates.map((row) => {
                    const Icon = iconFor(row.kind);
                    const chosen = picked?.target === row.target && picked.kind === row.kind;
                    return (
                      <li key={`${row.kind}:${row.target}`}>
                        <button
                          aria-current={chosen ? "true" : undefined}
                          className={
                            chosen
                              ? "flex w-full min-w-0 items-center gap-2 bg-accent px-2 py-1.5 text-left"
                              : "flex w-full min-w-0 items-center gap-2 px-2 py-1.5 text-left hover:bg-accent/50"
                          }
                          data-testid={`${SESSION_ADD_REF_ROW_TESTID}-${row.target}`}
                          onClick={() => setPicked(row)}
                          type="button"
                        >
                          <Icon aria-hidden className="size-4 shrink-0 text-muted-foreground" />
                          <span className="min-w-0 flex-1 truncate text-sm">{row.label}</span>
                          <span className="shrink-0 truncate text-meta text-muted-foreground">
                            {row.detail}
                          </span>
                        </button>
                      </li>
                    );
                  })
                )}
              </ul>
            )}
            {truncated && !typedUrl && (
              <p role="status" className="text-meta text-muted-foreground">
                {SESSION_ADD_REF_TRUNCATED}
              </p>
            )}

            {/* A typed link is the fourth source, and it takes over the moment
                it has anything in it: an external URL has nothing to pick from,
                and leaving the list live beside it would make two controls
                compete to be the answer. */}
            <div className="flex flex-col gap-1.5">
              <Label htmlFor={urlId}>{SESSION_ADD_REF_URL_LABEL}</Label>
              <Input
                id={urlId}
                onChange={(e) => setUrl(e.target.value)}
                placeholder={SESSION_ADD_REF_URL_PLACEHOLDER}
                value={url}
              />
            </div>

            {picked?.promotable === true && !typedUrl && (
              <div className="flex flex-col gap-1.5 rounded-md border border-border p-2">
                <div className="flex items-center gap-2">
                  <Checkbox
                    checked={promote}
                    id={promoteId}
                    onCheckedChange={(next) => setPromote(next === true)}
                  />
                  <Label htmlFor={promoteId}>{SESSION_ADD_REF_PROMOTE_LABEL}</Label>
                </div>
                <p className="text-meta text-muted-foreground">{SESSION_ADD_REF_PROMOTE_BODY}</p>
              </div>
            )}

            <div className="flex flex-wrap items-end gap-3">
              <div className="flex min-w-32 flex-1 flex-col gap-1.5">
                <Label htmlFor={aliasId}>{SESSION_ADD_REF_ALIAS_LABEL}</Label>
                <Input
                  id={aliasId}
                  onChange={(e) => setAlias(e.target.value)}
                  placeholder={SESSION_ADD_REF_ALIAS_PLACEHOLDER}
                  value={alias}
                />
              </div>
              <div className="flex min-w-32 flex-1 flex-col gap-1.5">
                <Label htmlFor={fileId}>{SESSION_ADD_REF_FILE_LABEL}</Label>
                {/* Native, matching the other two menus on this surface
                    ({@link "@/components/sessions/session-file-actions"} and the
                    space editor) so the Files and References dialogs read as one
                    pair of controls rather than two designs. */}
                <select
                  className="h-9 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  id={fileId}
                  onChange={(e) => setFile(e.target.value)}
                  value={file}
                >
                  {fileOptions.map((target) => (
                    <option key={target} value={target}>
                      {targets.includes(target)
                        ? target
                        : `${target}${SESSION_ADD_REF_NEW_FILE_SUFFIX}`}
                    </option>
                  ))}
                </select>
              </div>
            </div>
          </div>

          {/* What was written, in the words Rust wrote it in. */}
          {added !== null && (
            <div role="status" className="flex flex-col gap-0.5">
              <p className="text-muted-foreground text-sm">{addedSummary(added)}</p>
              <code className="truncate rounded-md bg-muted px-2 py-1 font-mono text-meta">
                {added.line}
              </code>
            </div>
          )}
          {notice !== null && (
            <p role="status" className="text-destructive text-sm">
              {notice}
            </p>
          )}
          <DialogFooter>
            <Button disabled={busy} onClick={() => setOpen(false)} type="button" variant="ghost">
              {SESSION_ADD_REF_CLOSE}
            </Button>
            <Button disabled={busy || !ready} onClick={add} type="button">
              {SESSION_ADD_REF_CONFIRM}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
