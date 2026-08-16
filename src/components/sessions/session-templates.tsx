/**
 * The zone's templates, as a room you can walk into (FR-269, FR-270, FR-271).
 *
 * Everything here but the rename already existed in Rust and reached nobody:
 * `sessions_patterns` has always returned the zone's own `_template/` and every
 * `_template/<name>/` beside it, and the only render path was one `<Select>`
 * inside the create row. So this is a *surface*, not a subsystem — it reads the
 * same list the picker reads (one source of truth: a template that appears in
 * one appears in the other), asks Rust what is inside each one, and opens those
 * files through the same file target the Files pane and the session tree use
 * (AD-109).
 *
 * Every path here is Rust's. `sessions_template_entries` returns a
 * profile-relative `subpath` and that string is handed to the panel strip
 * untouched — this file joins nothing (AD-65). The only thing it takes apart is
 * an *id* Rust minted, to recover the name argument the entries call wants,
 * which is the same prefix test `session-pattern-picker.tsx` already makes.
 *
 * The verbs sit on the headings rather than in the pane header, the way
 * `session-spaces.tsx` puts New space / Restore on the Spaces heading: a third
 * button up there costs a `min-w-0` subtitle ~120px of a ≤512px row and pushes
 * the filter row under the fold, which is what UX-DR85 exists to prevent.
 *
 * **The room is a tree now (FR-284)**, and its rows carry verbs: a template's
 * files and folders can be made, renamed and deleted from here. The tree is
 * derived from the one read this room has ever had — `sessions_template_entries`
 * returns each entry's path *relative to the template*, folders included, so the
 * structure is already in the payload and {@link templateTree} reads it back out.
 * A second directory walk would be a second answer to "what is in this template",
 * and the first one is also what a create copies. Folders are in that payload
 * because a folder derived from the files under it is a folder an EMPTY one can
 * never be: `New folder` shipped able to make a directory this room could not
 * draw, and therefore could never rename or delete.
 *
 * The verb set is deliberately narrower than the session tree's: no reveal, no
 * open-in-default-app, no sync mark. A template is a skeleton on the drive rather
 * than work in progress, and every one of those three would need a field the
 * entries payload does not carry — which would mean widening it, which would mean
 * that second walk.
 */
import { ChevronDown, ChevronRight, Folder, FolderOpen, Pencil, Trash2 } from "lucide-react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  SESSION_PATTERN_INSTALL_LABEL,
  TEMPLATE_ID_PREFIX,
} from "@/components/sessions/session-pattern-picker";
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
import { InputGroup, InputGroupInput } from "@/components/ui/input-group";
import { formatDraftAge } from "@/lib/format-time";
import type { SessionPatternVm, SessionTemplateEntryVm } from "@/lib/ipc/client";
import {
  sessionsTemplateDeleteEntry,
  sessionsTemplateDirNew,
  sessionsTemplateEntries,
  sessionsTemplateFileNew,
  sessionsTemplateRename,
  sessionsTemplateRenameEntry,
} from "@/lib/ipc/client";
import { panelsStore } from "@/lib/stores/panels";
import { syncErrorMessage } from "@/lib/stores/sync";
import { resolveViewer, VIEWER_ICON } from "@/lib/viewers";

/** The room's heading, and the accessible name of the surface itself. */
export const SESSION_TEMPLATES_HEADING = "Templates";

/**
 * What the room says about itself, once, under the heading.
 *
 * The consequence rather than the noun: a template is only interesting because
 * every session made from it starts as a copy of what is left here.
 */
export const SESSION_TEMPLATES_HINT =
  "The skeletons this zone copies a new session out of. Open a file to edit it — every session made from that template starts with what you leave here.";

/** What a zone with no template at all says. */
export const SESSION_TEMPLATES_EMPTY =
  "This zone has no template yet. Write keeper's default down, or make one by name.";

/** What one template with nothing inside it says — honest, and not an error. */
export const SESSION_TEMPLATES_NO_FILES = "Nothing in this template yet.";

/** What a template says while its files have not arrived. */
export const SESSION_TEMPLATES_LOADING = "Reading…";

/** The one-line state while the zone's pattern list is still out. */
export const SESSION_TEMPLATES_READING = "Reading templates…";

/**
 * What one template keeper could not read says — in that template's own
 * section, under its own heading, and only when Rust gave no sentence of its
 * own.
 *
 * Here rather than in the room's live region because a read's answer has the
 * read's lifetime: a zone-level notice outlived the failure it described and sat
 * above sections that were already correct, and nothing cleared it. This line
 * lives in the entries mirror, which every read replaces whole. The heading
 * directly above it is the name that could not be read, so the sentence does not
 * repeat it.
 */
export const SESSION_TEMPLATE_READ_FAILED = "keeper couldn't read what is in this template.";

/** The create verb the owner asked for, and the field it needs. */
export const SESSION_TEMPLATES_NEW = "New template";
export const SESSION_TEMPLATES_NEW_NAME_LABEL = "Template name";

/**
 * What `New template` says when the zone already holds that name.
 *
 * `sessions_template_install` trashes and rewrites the `AGENTS.md` and
 * `about.md` it finds at the destination. That is right for the verb it was
 * written for — adopting keeper's skeleton into a zone that already has one, with
 * the displaced bytes recoverable in `.keeper/trash/` — and wrong for a button
 * called New, beside a Rename that refuses this exact collision with a sentence
 * of its own. So the collision is answered here, before the write, in the words
 * Rename answers it with (`sessions_ipc.rs:2113-2122`).
 *
 * A function because the answer is about a name the operator typed, and naming
 * the template it found is the difference between a refusal and a scolding.
 */
export function sessionTemplateTaken(name: string): string {
  return (
    `${name} already exists — pick another name. ` +
    "New template will not write over a template somebody else made."
  );
}

/** The rename verb: its accessible name (suffixed with the template), its field. */
export const SESSION_TEMPLATE_RENAME = "Rename template";
export const SESSION_TEMPLATE_RENAME_NAME_LABEL = "New template name";
export const SESSION_TEMPLATE_RENAME_CONFIRM = "Rename";
export const SESSION_TEMPLATE_RENAME_FAILED = "keeper couldn't rename this template.";

/** One section, for tests that need to find one by template id. */
export const SESSION_TEMPLATE_SECTION_TESTID = "session-template";

/** One file row, for tests that need to find one by the subpath Rust returned. */
export const SESSION_TEMPLATE_FILE_TESTID = "session-template-file";

/** One folder row, keyed on the template-relative path the paths implied. */
export const SESSION_TEMPLATE_DIR_TESTID = "session-template-dir";

/**
 * The two create verbs on a template's heading, and the field they share.
 *
 * Labelled buttons rather than an icon, and always visible: hover-reveal is
 * right for a row's own edit and delete, and wrong for the verb a section exists
 * to offer — "I don't see the button" was literally true of the create control
 * epic 50 was reported against.
 *
 * The field takes the path INSIDE the template, which is the argument Rust's verb
 * takes: `notes.md` at the root, `refs/inputs.md` in a subfolder keeper makes in
 * the same plan. One field rather than a name plus a folder picker, because the
 * room already shows the folders and a picker would be a second way to say what
 * the tree is already saying.
 */
export const SESSION_TEMPLATE_NEW_FILE = "New file";
export const SESSION_TEMPLATE_NEW_FOLDER = "New folder";
export const SESSION_TEMPLATE_NEW_FILE_LABEL = "File path inside the template";
export const SESSION_TEMPLATE_NEW_FOLDER_LABEL = "Folder path inside the template";
export const SESSION_TEMPLATE_NEW_CONFIRM = "Create";
export const SESSION_TEMPLATE_NEW_FAILED = "keeper couldn't write that into this template.";

/** A row's own verbs: rename and delete, both offered on files and folders. */
export const SESSION_TEMPLATE_ENTRY_RENAME = "Rename";
export const SESSION_TEMPLATE_ENTRY_RENAME_LABEL = "New name";
export const SESSION_TEMPLATE_ENTRY_RENAME_FAILED = "keeper couldn't rename that.";
export const SESSION_TEMPLATE_ENTRY_DELETE = "Delete";
export const SESSION_TEMPLATE_ENTRY_DELETE_FAILED =
  "keeper couldn't delete that. Nothing was changed.";

/** The confirmation, and what it promises — the session tree's wording, extended. */
export const SESSION_TEMPLATE_DELETE_TITLE = "Delete this from the template?";

/**
 * Three sentences, because a template delete has three consequences a session
 * file delete does not.
 *
 * Where it goes: the zone's trash, recoverable, never an unlink. What a folder
 * takes with it: everything inside — the session tree refuses folder deletion on
 * exactly that ground, and this offers it because a template's directories hold a
 * skeleton rather than work, with the trash underneath either way. And what is
 * NOT affected: sessions already made from this template keep their copies, since
 * a create copies rather than references.
 */
export const SESSION_TEMPLATE_DELETE_BODY =
  "keeper moves it into the zone's trash, so it is recoverable from there. A folder goes whole, with everything inside it. Sessions already made from this template keep their own copies.";

/**
 * The name argument `sessions_template_entries` and `sessions_template_rename`
 * take for this pattern, or `undefined` for the zone's own template.
 *
 * The zone's template IS `_template` exactly and has no name — the directory
 * name is its contract (`model::TEMPLATE_DIR`), which is also why it is the one
 * template that cannot be renamed. So the id answers *which kind* this is, and
 * that is the only thing the id is asked.
 *
 * The name itself is the pattern's own `label`, verbatim: for a named template
 * the shell builds the row as `pattern_vm(named_template_id(name), …, name, …)`
 * (`sessions_ipc.rs:731-740`), so the label IS the on-disk folder name. The
 * shell then uses that argument verbatim too — it addresses a directory that
 * already exists, and only a *new* name gets slugged. Re-deriving or slugging it
 * here would make a hand-made `_template/Interview Kit/` unaddressable, which is
 * the sharp end of AD-65: Rust said the name, and this passes on what it said.
 */
function templateName(pattern: SessionPatternVm): string | undefined {
  return pattern.id === TEMPLATE_ID_PREFIX ? undefined : pattern.label;
}

export interface SessionTemplatesProps {
  rootId: string;
  /**
   * Every pattern the zone offers, shell-ordered, or `null` while that read is
   * out. The templates are filtered out of it here rather than upstream so this
   * list and the create row's picker cannot disagree about what a template is.
   */
  patterns: readonly SessionPatternVm[] | null;
  /**
   * Write a template into the zone: a named one when a name is given, the
   * zone's own `_template/` when it is not. Owned by the pane, because the pane
   * owns the re-read that has to happen afterwards and the create row's picker
   * makes the same call. Resolves with Rust's refusal sentence, or `null` when
   * the write landed — the sentence travels back so this surface can say it in
   * its own live region rather than keeping a second copy of the catch.
   */
  onInstallTemplate: (name?: string) => Promise<string | null>;
  /** Set while a write is in flight, so a control cannot be pressed twice. */
  installing: boolean;
  /** Re-read the pattern list — a rename changes what it returns. */
  onChanged: () => void;
  /** Injected for tests; the relative ages are cosmetic. */
  nowMs?: number;
}

export function SessionTemplates({
  rootId,
  patterns,
  onInstallTemplate,
  installing,
  onChanged,
  nowMs = Date.now(),
}: SessionTemplatesProps) {
  const [newName, setNewName] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [renaming, setRenaming] = useState(false);
  /**
   * What each template holds — and, for the ones keeper could not read, what it
   * said instead — tagged with the root it was all read for.
   *
   * The tag is the `rowsRootId` stale-guard the board already uses: switching
   * root re-reads, and until the new answer lands the sections say "Reading…"
   * rather than showing the previous zone's files under this zone's names.
   *
   * The refusals sit here rather than in `notice` because they belong to the
   * read: this mirror is replaced whole by the next one, so a sentence about a
   * template cannot outlive the answer it came from.
   */
  const [entries, setEntries] = useState<{
    rootId: string;
    byId: ReadonlyMap<string, SessionTemplateEntryVm[]>;
    failed: ReadonlyMap<string, string>;
  } | null>(null);

  const templates = useMemo(
    () => (patterns ?? []).filter((pattern) => pattern.kind === "template"),
    [patterns],
  );

  // One read per template, fired together and settled one by one. `templates`
  // changes identity on every pattern re-read the pane does, which is exactly the
  // re-read signal wanted: a create, a rename and a root switch all reach the
  // pane's nonce or its root, and both re-read the patterns this depends on.
  //
  // `allSettled`, not `all`: `Promise.all` threw away every sibling that had
  // already resolved the moment one read rejected, so a single template keeper
  // will not address — a name `template_at` refuses, a directory that moved —
  // left EVERY section on "Reading…" for good, with nothing to press. A refusal
  // now costs exactly the section it belongs to.
  useEffect(() => {
    if (templates.length === 0) {
      return;
    }
    let live = true;
    void Promise.allSettled(
      templates.map((template) => sessionsTemplateEntries(rootId, templateName(template))),
    ).then((results) => {
      if (!live) {
        return;
      }
      const byId = new Map<string, SessionTemplateEntryVm[]>();
      const failed = new Map<string, string>();
      // Paired by index — `allSettled` answers in the order it was asked — so
      // each outcome keeps the template it is about and the sentence can be shown
      // under that template's own heading.
      templates.forEach((template, index) => {
        const result = results[index];
        if (result.status === "fulfilled") {
          byId.set(template.id, result.value);
        } else {
          failed.set(template.id, syncErrorMessage(result.reason, SESSION_TEMPLATE_READ_FAILED));
        }
      });
      setEntries({ rootId, byId, failed });
    });
    return () => {
      live = false;
    };
  }, [rootId, templates]);

  /**
   * Make a template by name.
   *
   * An empty field is not a refusal to report — it is a question nobody
   * answered, and the field is right there — so the confirm is simply inert and
   * Rust is never asked. The name is cleared only when the write landed, the
   * way the board's own create row clears its title.
   *
   * A name the zone already holds is a refusal, and this surface makes it:
   * `sessions_template_install` trash-then-writes through an occupied
   * destination, which is not what a button called New may do. See
   * {@link sessionTemplateTaken}.
   */
  const create = useCallback(() => {
    const name = newName.trim();
    // Nothing is asked until the list is in hand either: the collision below is
    // decided against what the zone actually holds, and deciding it against a
    // list that has not arrived would let New template write over the very
    // template the read was about to show.
    if (name === "" || patterns === null) {
      return;
    }
    // Compared against `templateName`, which is the on-disk folder name, so the
    // zone's own `_template/` — whose label is a display name and not a
    // directory — is never what a typed name collides with. Case-insensitively
    // because the drives this syncs to are: `Interview` and `interview` are one
    // directory there, and the second create would land on the first.
    const taken = templates.find(
      (template) => templateName(template)?.toLowerCase() === name.toLowerCase(),
    );
    if (taken !== undefined) {
      setNotice(sessionTemplateTaken(taken.label));
      return;
    }
    setNotice(null);
    void onInstallTemplate(name).then((refusal) => {
      setNotice(refusal);
      if (refusal === null) {
        setNewName("");
      }
    });
  }, [newName, patterns, templates, onInstallTemplate]);

  /** Adopt keeper's own skeleton as this zone's `_template/` — no name, by design. */
  const installZoneTemplate = useCallback(() => {
    setNotice(null);
    void onInstallTemplate().then(setNotice);
  }, [onInstallTemplate]);

  /**
   * Rename a named template.
   *
   * Rust decides whether the new name is a name at all, whether it collides,
   * and whether the source is still there; each refusal is a sentence naming
   * which of those happened, and this prints that sentence rather than
   * paraphrasing it. Nothing local changes on a refusal — no re-read, no closed
   * form — so the operator can correct the name they typed.
   */
  const rename = useCallback(
    (name: string, next: string) => {
      const wanted = next.trim();
      if (wanted === "") {
        return;
      }
      setRenaming(true);
      setNotice(null);
      sessionsTemplateRename(rootId, name, wanted)
        .then(() => {
          setEditing(null);
          onChanged();
        })
        .catch((raw: unknown) => setNotice(syncErrorMessage(raw, SESSION_TEMPLATE_RENAME_FAILED)))
        .finally(() => setRenaming(false));
    },
    [rootId, onChanged],
  );

  const read = entries !== null && entries.rootId === rootId ? entries : null;

  return (
    <section aria-label={SESSION_TEMPLATES_HEADING} className="flex flex-col gap-2">
      {/* Wraps rather than clips: the heading, the name field and the create
          verb are three things in a row that is also a pane that resizes. */}
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
          {SESSION_TEMPLATES_HEADING}
        </h3>
        <span className="min-w-0 flex-1" />
        <InputGroup className="w-48 shrink-0">
          <InputGroupInput
            placeholder={SESSION_TEMPLATES_NEW_NAME_LABEL}
            aria-label={SESSION_TEMPLATES_NEW_NAME_LABEL}
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              // Guarded by the flag the confirm button beside it is disabled by:
              // Enter went straight past that guard, and two creates from one
              // held key are two writes to the same destination.
              if (e.key === "Enter" && !installing) {
                create();
              }
            }}
          />
        </InputGroup>
        {/* Inert while a write is out, and while the list the collision check
            needs has not arrived. */}
        <Button type="button" size="sm" disabled={installing || patterns === null} onClick={create}>
          {SESSION_TEMPLATES_NEW}
        </Button>
      </div>
      <p className="text-muted-foreground text-xs">{SESSION_TEMPLATES_HINT}</p>
      {notice !== null && (
        // One live region for the verbs on this surface: create and rename both
        // answer here, and each answer is a sentence. What a READ said belongs to
        // the read, and is shown in the section it is about.
        <p role="status" className="text-muted-foreground text-xs">
          {notice}
        </p>
      )}

      {patterns === null ? (
        <p className="text-muted-foreground text-xs">{SESSION_TEMPLATES_READING}</p>
      ) : templates.length === 0 ? (
        <div className="flex flex-col items-start gap-1">
          <p className="text-muted-foreground text-xs">{SESSION_TEMPLATES_EMPTY}</p>
          {/* The zone-template path, still reachable from here. Same words the
              create row's picker uses for the same write — one sentence per
              verb, wherever the verb is offered. */}
          <button
            type="button"
            disabled={installing}
            onClick={installZoneTemplate}
            className="text-left text-primary text-xs underline-offset-2 hover:underline disabled:opacity-60"
          >
            {SESSION_PATTERN_INSTALL_LABEL}
          </button>
        </div>
      ) : (
        templates.map((template) => (
          <TemplateSection
            key={template.id}
            template={template}
            files={read?.byId.get(template.id) ?? null}
            failure={read?.failed.get(template.id) ?? null}
            rootId={rootId}
            name={templateName(template)}
            onNotice={setNotice}
            onChanged={onChanged}
            nowMs={nowMs}
            editing={editing === template.id}
            renaming={renaming}
            onEdit={() => {
              setNotice(null);
              setEditing(template.id);
            }}
            onCancelEdit={() => setEditing(null)}
            onRename={(next) => {
              const name = templateName(template);
              if (name !== undefined) {
                rename(name, next);
              }
            }}
          />
        ))
      )}
    </section>
  );
}

/** The indent per level, and the base pad — the session tree's own numbers. */
const INDENT_PX = 16;
const PAD_PX = 8;

/**
 * One row of a template's tree: an entry Rust listed, or a folder its paths
 * imply.
 *
 * `SessionTemplateEntryVm.name` is the entry's path **relative to the template**
 * (`prompts/hand-off.md`) — story 49.1's own decision, so that two files sharing
 * a basename are two distinguishable rows. That makes the payload a flat list
 * with the structure already in it, and this is where the structure is read back
 * out. **No second walk**: the room has exactly one reader
 * (`sessions_template_entries`), and the nesting is a row's `parent` because a
 * path said so, not because anything looked again.
 *
 * A folder is a row the shell NAMES, and it stopped being one this room derives
 * from the files inside it: an empty directory names no file, so it had no row,
 * and a row that is not drawn is one no rename, no delete and no create prefill
 * can reach. That was `New folder`'s whole result — a directory on the drive the
 * room it was added to could not show — and the skeleton's own `artifacts/` and
 * `workspace/` were invisible for the same reason. The ancestor walk below stays
 * anyway, as the answer to a payload that names a file under a folder it did not
 * list: a row with no parent row is worse than one implied.
 */
export interface SessionTemplateNode {
  /** Template-relative path — this row's identity, and the verbs' `rel`. */
  relPath: string;
  /** The last segment: what the row shows, and what a rename edits. */
  name: string;
  /** The parent's `relPath`, `""` at the template's root. */
  parent: string;
  /** 1-based, for `aria-level` and the indent. */
  depth: number;
  isDir: boolean;
  /**
   * The entry Rust listed for this row — a file, or a directory it named — or
   * `null` for a folder only another row's path implied.
   */
  entry: SessionTemplateEntryVm | null;
}

/**
 * Group Rust's flat rows into a tree, in render order.
 *
 * **The order inside a folder is still the shell's.** `sessions_template_entries`
 * answers newest change first, and that survives here: children keep the order
 * they arrived in, and a folder takes the position of whichever came first — its
 * own row, or the first file under it. A re-sort would replace the shell's
 * decision with this file's, and the room would disagree with the picker about
 * what "first" means.
 *
 * A directory the payload names takes precedence over one an ancestor walk
 * implied: same row, same place in the order, and the entry underneath it is the
 * real one, so a folder that arrived after its own contents is still the folder
 * Rust listed.
 */
export function templateTree(entries: readonly SessionTemplateEntryVm[]): SessionTemplateNode[] {
  const nodes = new Map<string, SessionTemplateNode>();
  const children = new Map<string, string[]>();
  const add = (node: SessionTemplateNode) => {
    nodes.set(node.relPath, node);
    const siblings = children.get(node.parent);
    if (siblings === undefined) {
      children.set(node.parent, [node.relPath]);
    } else {
      siblings.push(node.relPath);
    }
  };

  for (const entry of entries) {
    const parts = entry.name.split("/");
    // A dotfile is not a row, and this asks the question rather than trusting the
    // answer: `pattern_files` skips them and `is_placeholder` drops `.gitkeep`,
    // but every verb a row carries is refused for one (`EntryError::Dotfile`), so
    // a row nothing can act on is a row not to draw.
    if (parts.some((part) => part.startsWith("."))) {
      continue;
    }
    let parent = "";
    // Every ancestor, so a file three deep still has a chain of folders above it
    // when the payload named none of them.
    for (let index = 0; index < parts.length - 1; index += 1) {
      const relPath = parts.slice(0, index + 1).join("/");
      if (!nodes.has(relPath)) {
        add({
          relPath,
          name: parts[index],
          parent,
          depth: index + 1,
          isDir: true,
          entry: null,
        });
      }
      parent = relPath;
    }
    const already = nodes.get(entry.name);
    if (already === undefined) {
      add({
        relPath: entry.name,
        name: parts[parts.length - 1],
        parent,
        depth: parts.length,
        isDir: entry.isDir,
        entry,
      });
    } else if (already.entry === null && entry.isDir) {
      // Implied first by a file underneath it, and named by the payload after —
      // one row either way, keeping the place the order gave it.
      already.entry = entry;
    }
  }

  const out: SessionTemplateNode[] = [];
  const walk = (parent: string) => {
    for (const relPath of children.get(parent) ?? []) {
      const node = nodes.get(relPath);
      if (node === undefined) {
        continue;
      }
      out.push(node);
      if (node.isDir) {
        walk(relPath);
      }
    }
  };
  walk("");
  return out;
}

/**
 * The rows that render, with the folders the person collapsed applied.
 *
 * The session tree's `visibleRows`, inverted: it keeps the set of OPEN folders,
 * seeded once from the entries it was handed, and closes `workspace/` because a
 * session's scratch has no contract about its size. Neither applies here. A
 * template arrives whole and its folders ARE its shape, so everything starts
 * open; and keeping the *closed* set instead of the open one means a re-read
 * after a write cannot re-close a folder the person opened, and a folder created
 * a second ago is open rather than collapsed because it was not in a set seeded
 * before it existed.
 */
function visibleRows(
  rows: readonly SessionTemplateNode[],
  closed: ReadonlySet<string>,
): SessionTemplateNode[] {
  return rows.filter((row) => {
    if (row.parent === "") {
      return true;
    }
    // Every ancestor, not just the parent: a collapsed folder hides its whole
    // subtree, however deep the row sits.
    const parts = row.parent.split("/");
    for (let index = 0; index < parts.length; index += 1) {
      if (closed.has(parts.slice(0, index + 1).join("/"))) {
        return false;
      }
    }
    return true;
  });
}

/**
 * One template: its label, the tree of what is inside it, the two create verbs,
 * and — for a named one — the rename of the template itself.
 *
 * The template-rename field starts on the name the template already has, which is
 * the name most renames are a small edit to. That seeding is a mount-time value
 * and stays honest because a successful rename changes the id this section is
 * keyed on: the section remounts under the new name rather than holding the old
 * draft.
 *
 * **Every refusal goes up to the room's one live region** (`onNotice`), rather
 * than a second one per section: a write's answer is about a verb the person just
 * pressed, and the room already has the place that says those. What a READ said
 * still belongs to the read and stays in `failure`, under this heading.
 */
function TemplateSection({
  template,
  files,
  failure,
  rootId,
  name,
  nowMs,
  editing,
  renaming,
  onEdit,
  onCancelEdit,
  onRename,
  onNotice,
  onChanged,
}: {
  template: SessionPatternVm;
  /** What Rust says is inside, or `null` while that read is out or was refused. */
  files: SessionTemplateEntryVm[] | null;
  /**
   * Why this template's read was refused, in Rust's words, or `null`. Separate
   * from `files` because "not here yet" and "not coming" are different answers,
   * and a section that says "Reading…" forever is the second one lying.
   */
  failure: string | null;
  rootId: string;
  /**
   * The template's own name argument — `undefined` for the zone's `_template/` —
   * as {@link templateName} decided it. Passed down rather than re-derived: the
   * entry verbs address a template exactly as the entries read does.
   */
  name: string | undefined;
  nowMs: number;
  editing: boolean;
  renaming: boolean;
  onEdit: () => void;
  onCancelEdit: () => void;
  onRename: (next: string) => void;
  /** Say this in the room's live region, or clear it with `null`. */
  onNotice: (sentence: string | null) => void;
  /** Re-read the room after a write landed — story 49.1's nonce. */
  onChanged: () => void;
}) {
  const [draft, setDraft] = useState(template.label);
  /** Which create form is open and what is typed in it, `null` for neither. */
  const [minting, setMinting] = useState<{ kind: "file" | "dir"; draft: string } | null>(null);
  /** Which entry is being renamed and what is typed for it. */
  const [renamingEntry, setRenamingEntry] = useState<{ relPath: string; draft: string } | null>(
    null,
  );
  /**
   * Which entry the confirmation is about, `null` when it is closed. The node
   * rather than its path, so the dialog can still name it after a re-read has
   * removed the row that opened it.
   */
  const [deleting, setDeleting] = useState<SessionTemplateNode | null>(null);
  /** Set while a write of this section's is out, so nothing is pressed twice. */
  const [busy, setBusy] = useState(false);
  /** Which folders the person collapsed — see {@link visibleRows} for why not the open ones. */
  const [closed, setClosed] = useState<ReadonlySet<string>>(() => new Set<string>());
  /**
   * The roving tabindex's memory: exactly one row is in the tab order, and it is
   * the last one focused rather than always the first — the session tree's rule,
   * so Tab back into the room returns where the person left it.
   */
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const rowRefs = useRef(new Map<string, HTMLDivElement>());

  // The zone's own template is the one thing here without a name to change: the
  // directory name IS the contract, and there is exactly one of it per zone.
  const renameable = template.id !== TEMPLATE_ID_PREFIX;

  const tree = useMemo(() => templateTree(files ?? []), [files]);
  const rows = useMemo(() => visibleRows(tree, closed), [tree, closed]);
  const active = rows.some((row) => row.relPath === activeKey)
    ? activeKey
    : (rows[0]?.relPath ?? null);

  const toggle = useCallback((relPath: string) => {
    setClosed((previous) => {
      const next = new Set(previous);
      if (!next.delete(relPath)) {
        next.add(relPath);
      }
      return next;
    });
  }, []);

  const focusRow = useCallback((relPath: string) => {
    setActiveKey(relPath);
    rowRefs.current.get(relPath)?.focus();
  }, []);

  const open = useCallback(
    (node: SessionTemplateNode) => {
      // A folder now carries an entry of its own, so "has a payload" no longer
      // means "opens something": a directory has no file target, and pressing one
      // folds it (see the row's own handler).
      if (node.isDir || node.entry === null) {
        return;
      }
      // The subpath Rust composed, handed over as it arrived. A join here would be
      // a second answer to a question Rust already answered, and the two would
      // drift the day a zone's subfolder stops being the zone's name (AD-65).
      panelsStore.getState().setActiveTarget({
        kind: "file",
        profileId: rootId,
        relativePath: node.entry.subpath,
      });
    },
    [rootId],
  );

  /**
   * Open a create form, prefilled with the folder the person is standing in.
   *
   * The field takes a path, so the folder half of the question is already
   * answered by where the focus is: pressing *New file* on a row inside
   * `prompts/` offers `prompts/` rather than the template's root. It is a prefill
   * and not a constraint — the whole path stays editable.
   */
  const startMinting = useCallback(
    (kind: "file" | "dir") => {
      onNotice(null);
      const here = tree.find((row) => row.relPath === active);
      const folder = here === undefined ? "" : here.isDir ? here.relPath : here.parent;
      setMinting({ kind, draft: folder === "" ? "" : `${folder}/` });
    },
    [active, tree, onNotice],
  );

  /**
   * Write the new file or folder.
   *
   * An empty field is not a refusal to report — it is a question nobody answered,
   * and the field is right there — so the confirm is inert and Rust is never
   * asked. Every other refusal is Rust's sentence, said in the room's live region
   * and not paraphrased.
   *
   * A new **file** is opened in the panel on the way out: the reason to make a
   * file in a template is to write in it, and the command answers the path that
   * opens it precisely so the caller does not have to compose one.
   */
  const mint = useCallback(() => {
    if (minting === null) {
      return;
    }
    const rel = minting.draft.trim();
    if (rel === "") {
      return;
    }
    const kind = minting.kind;
    setBusy(true);
    onNotice(null);
    const write =
      kind === "file"
        ? sessionsTemplateFileNew(rootId, name, rel).then((subpath) => {
            panelsStore.getState().setActiveTarget({
              kind: "file",
              profileId: rootId,
              relativePath: subpath,
            });
          })
        : sessionsTemplateDirNew(rootId, name, rel);
    write
      .then(() => {
        setMinting(null);
        onChanged();
      })
      .catch((raw: unknown) => onNotice(syncErrorMessage(raw, SESSION_TEMPLATE_NEW_FAILED)))
      .finally(() => setBusy(false));
  }, [minting, rootId, name, onNotice, onChanged]);

  /**
   * Rename one entry.
   *
   * Rust decides whether the new name is a name at all, whether it collides, and
   * whether the entry is still there. Nothing local changes on a refusal — no
   * re-read, no closed form — so the name that was typed can be corrected.
   */
  const renameEntry = useCallback(() => {
    if (renamingEntry === null) {
      return;
    }
    const next = renamingEntry.draft.trim();
    if (next === "") {
      return;
    }
    setBusy(true);
    onNotice(null);
    sessionsTemplateRenameEntry(rootId, name, renamingEntry.relPath, next)
      .then(() => {
        setRenamingEntry(null);
        onChanged();
      })
      .catch((raw: unknown) =>
        onNotice(syncErrorMessage(raw, SESSION_TEMPLATE_ENTRY_RENAME_FAILED)),
      )
      .finally(() => setBusy(false));
  }, [renamingEntry, rootId, name, onNotice, onChanged]);

  const confirmDelete = useCallback(() => {
    if (deleting === null) {
      return;
    }
    const target = deleting;
    setDeleting(null);
    onNotice(null);
    sessionsTemplateDeleteEntry(rootId, name, target.relPath)
      .then(onChanged)
      .catch((raw: unknown) =>
        onNotice(syncErrorMessage(raw, SESSION_TEMPLATE_ENTRY_DELETE_FAILED)),
      );
  }, [deleting, rootId, name, onNotice, onChanged]);

  const onKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>, node: SessionTemplateNode) => {
      const index = rows.findIndex((row) => row.relPath === node.relPath);
      const step = (target: number) => {
        const next = rows[Math.min(Math.max(target, 0), rows.length - 1)];
        if (next !== undefined) {
          event.preventDefault();
          focusRow(next.relPath);
        }
      };
      const isOpen = node.isDir && !closed.has(node.relPath);
      switch (event.key) {
        case "ArrowDown":
          step(index + 1);
          break;
        case "ArrowUp":
          step(index - 1);
          break;
        case "Home":
          step(0);
          break;
        case "End":
          step(rows.length - 1);
          break;
        case "ArrowRight":
          if (!node.isDir) {
            break;
          }
          if (isOpen) {
            // Already open: the right arrow walks INTO the folder, which is the
            // next row exactly when that row is a child of this one.
            if (rows[index + 1]?.parent === node.relPath) {
              step(index + 1);
            }
          } else {
            event.preventDefault();
            toggle(node.relPath);
          }
          break;
        case "ArrowLeft":
          if (isOpen) {
            event.preventDefault();
            toggle(node.relPath);
          } else if (node.parent !== "") {
            event.preventDefault();
            focusRow(node.parent);
          }
          break;
        case "Enter":
          event.preventDefault();
          if (node.isDir) {
            toggle(node.relPath);
          } else {
            open(node);
          }
          break;
        default:
          break;
      }
    },
    [rows, closed, focusRow, toggle, open],
  );

  return (
    <div
      data-testid={`${SESSION_TEMPLATE_SECTION_TESTID}-${template.id}`}
      className="flex flex-col gap-1 rounded-md border border-border px-3 py-2"
    >
      {/* Wraps rather than clips: the label, the count and three verbs are more
          than a narrow pane fits on one line. */}
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <h4 className="min-w-0 truncate font-medium text-sm">{template.label}</h4>
        {files !== null && (
          // The rows, not the payload: the payload carries folders now, and it can
          // carry a dotfile the tree drops, so a count off the raw list would name
          // a row that is not there.
          <span className="figures shrink-0 text-muted-foreground text-xs">{tree.length}</span>
        )}
        <span className="min-w-0 flex-1" />
        {/* Always visible and labelled, unlike the row verbs below: these are the
            verbs this section exists to offer, and a control revealed on hover is
            a control the person reporting "I don't see the button" was right
            about. */}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={busy}
          onClick={() => startMinting("file")}
          className="h-7 px-2 font-normal"
        >
          {SESSION_TEMPLATE_NEW_FILE}
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={busy}
          onClick={() => startMinting("dir")}
          className="h-7 px-2 font-normal"
        >
          {SESSION_TEMPLATE_NEW_FOLDER}
        </Button>
        {renameable && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label={`${SESSION_TEMPLATE_RENAME} ${template.label}`}
            title={`${SESSION_TEMPLATE_RENAME} ${template.label}`}
            onClick={onEdit}
            className="h-7 px-2"
          >
            <Pencil aria-hidden="true" className="size-3.5" />
          </Button>
        )}
      </div>

      {editing && (
        <div className="flex gap-2">
          <InputGroup>
            <InputGroupInput
              // Autofocused: the form exists because the user just pressed
              // Rename, and the new name is the one question it asks. No lint
              // suppression above it, unlike the create row's field:
              // `InputGroupInput` is a component, so `lint/a11y/noAutofocus`
              // never fires here and a suppression would be a comment
              // pretending to do something.
              autoFocus
              placeholder={SESSION_TEMPLATE_RENAME_NAME_LABEL}
              aria-label={SESSION_TEMPLATE_RENAME_NAME_LABEL}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                // Guarded by the flag the confirm button beside it is disabled
                // by: Enter went straight past that guard, and the second rename
                // of a double tap carries the SAME source name — Rust refuses it
                // because the source just moved, and the refusal paints over a
                // rename that worked.
                if (e.key === "Enter" && !renaming) {
                  onRename(draft);
                }
                if (e.key === "Escape") {
                  onCancelEdit();
                }
              }}
            />
          </InputGroup>
          <Button type="button" size="sm" disabled={renaming} onClick={() => onRename(draft)}>
            {SESSION_TEMPLATE_RENAME_CONFIRM}
          </Button>
        </div>
      )}

      {minting !== null && (
        <div className="flex gap-2">
          <InputGroup>
            <InputGroupInput
              autoFocus
              placeholder={
                minting.kind === "file"
                  ? SESSION_TEMPLATE_NEW_FILE_LABEL
                  : SESSION_TEMPLATE_NEW_FOLDER_LABEL
              }
              aria-label={
                minting.kind === "file"
                  ? SESSION_TEMPLATE_NEW_FILE_LABEL
                  : SESSION_TEMPLATE_NEW_FOLDER_LABEL
              }
              value={minting.draft}
              onChange={(e) => setMinting({ kind: minting.kind, draft: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !busy) {
                  mint();
                }
                if (e.key === "Escape") {
                  setMinting(null);
                }
              }}
            />
          </InputGroup>
          <Button type="button" size="sm" disabled={busy} onClick={mint}>
            {SESSION_TEMPLATE_NEW_CONFIRM}
          </Button>
        </div>
      )}

      {renamingEntry !== null && (
        <div className="flex items-center gap-2">
          {/* The path being renamed, said out loud: the field edits the last
              segment only, and a bare "New name" over a tree of twenty rows is a
              question about which one. */}
          <span className="min-w-0 shrink truncate text-muted-foreground text-xs">
            {renamingEntry.relPath}
          </span>
          <InputGroup className="min-w-0 flex-1">
            <InputGroupInput
              autoFocus
              placeholder={SESSION_TEMPLATE_ENTRY_RENAME_LABEL}
              aria-label={SESSION_TEMPLATE_ENTRY_RENAME_LABEL}
              value={renamingEntry.draft}
              onChange={(e) =>
                setRenamingEntry({ relPath: renamingEntry.relPath, draft: e.target.value })
              }
              onKeyDown={(e) => {
                if (e.key === "Enter" && !busy) {
                  renameEntry();
                }
                if (e.key === "Escape") {
                  setRenamingEntry(null);
                }
              }}
            />
          </InputGroup>
          <Button type="button" size="sm" disabled={busy} onClick={renameEntry}>
            {SESSION_TEMPLATE_RENAME_CONFIRM}
          </Button>
        </div>
      )}

      {failure !== null ? (
        // Rust's own sentence, in the section it is about. One refused read costs
        // this template's files and nothing else — its siblings show theirs.
        <p className="text-muted-foreground text-xs">{failure}</p>
      ) : files === null ? (
        <p className="text-muted-foreground text-xs">{SESSION_TEMPLATES_LOADING}</p>
      ) : rows.length === 0 ? (
        <p className="text-muted-foreground text-xs">{SESSION_TEMPLATES_NO_FILES}</p>
      ) : (
        // A `div`, not a `ul`: an ARIA tree with a roving tabindex is not a list,
        // and `ul`/`li` would fight the pattern (the session tree's own note).
        <div role="tree" aria-label={template.label} className="flex flex-col">
          {rows.map((node) => {
            const isOpen = node.isDir && !closed.has(node.relPath);
            return (
              <div
                key={node.relPath}
                ref={(element) => {
                  if (element === null) {
                    rowRefs.current.delete(node.relPath);
                  } else {
                    rowRefs.current.set(node.relPath, element);
                  }
                }}
                role="treeitem"
                tabIndex={active === node.relPath ? 0 : -1}
                aria-level={node.depth}
                aria-expanded={node.isDir ? isOpen : undefined}
                aria-label={node.name}
                // Keyed on what the row IS, not on whether it has a payload: a
                // folder Rust listed carries one now, and a folder's identity is
                // its path either way — an implied row and a listed row for one
                // directory must not be two different test ids.
                data-testid={
                  node.isDir || node.entry === null
                    ? `${SESSION_TEMPLATE_DIR_TESTID}-${node.relPath}`
                    : `${SESSION_TEMPLATE_FILE_TESTID}-${node.entry.subpath}`
                }
                onKeyDown={(event) => onKeyDown(event, node)}
                onFocus={() => setActiveKey(node.relPath)}
                className="group flex items-center gap-1 rounded-sm px-2 py-1 hover:bg-accent/50 focus-visible:outline-2 focus-visible:outline-ring"
                style={{ paddingInlineStart: `${(node.depth - 1) * INDENT_PX + PAD_PX}px` }}
              >
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  // Off the tab order: the ROW is what Tab reaches, and this
                  // button is the row's own primary gesture rather than a second
                  // stop beside it.
                  tabIndex={-1}
                  className="h-6 min-w-0 flex-1 justify-start gap-1 px-1 font-normal"
                  onClick={() => (node.isDir ? toggle(node.relPath) : open(node))}
                >
                  {node.isDir ? (
                    <>
                      {isOpen ? (
                        <ChevronDown aria-hidden="true" className="size-3.5 shrink-0" />
                      ) : (
                        <ChevronRight aria-hidden="true" className="size-3.5 shrink-0" />
                      )}
                      {isOpen ? (
                        <FolderOpen
                          aria-hidden="true"
                          className="size-4 shrink-0 text-muted-foreground"
                        />
                      ) : (
                        <Folder
                          aria-hidden="true"
                          className="size-4 shrink-0 text-muted-foreground"
                        />
                      )}
                    </>
                  ) : (
                    <>
                      {/* The chevron's width, kept, so names line up under the
                          folder they are in rather than sliding left. */}
                      <span aria-hidden="true" className="size-3.5 shrink-0" />
                      <TemplateFileIcon name={node.name} />
                    </>
                  )}
                  {/* The basename now, not the whole path: the folder above it says
                      the rest, which is what a tree is for. The path a press opens
                      is still Rust's `subpath`, untouched (AD-65). */}
                  <span className="truncate text-sm">{node.name}</span>
                </Button>

                {/* Files only. A directory's mtime moves whenever anything in it is
                    added or removed, so an age beside a folder would answer a
                    question nobody asked of it — and a folder the payload did not
                    name has no mtime at all, which would make the column appear
                    and disappear for one directory. */}
                {!node.isDir && node.entry !== null && node.entry.mtimeMs > 0 && (
                  <span className="figures w-16 shrink-0 text-right text-muted-foreground text-xs">
                    {formatDraftAge(node.entry.mtimeMs, nowMs)}
                  </span>
                )}

                {/* The row's own verbs. Icon-only and revealed on hover or focus —
                    the room is read far more often than it is written to — and in
                    the tab order only while their row is the focused one. Offered
                    on folders as well as files, because a template's shape is its
                    folders and the trash makes taking one back cheap. */}
                <span className="flex shrink-0 items-center gap-0.5 opacity-0 focus-within:opacity-100 group-hover:opacity-100">
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    tabIndex={active === node.relPath ? 0 : -1}
                    aria-label={`${SESSION_TEMPLATE_ENTRY_RENAME} ${node.relPath}`}
                    title={SESSION_TEMPLATE_ENTRY_RENAME}
                    className="size-6"
                    onClick={() => {
                      onNotice(null);
                      // Seeded with the name it has: most renames are a small edit
                      // to it, and the extension is part of what may be edited.
                      setRenamingEntry({ relPath: node.relPath, draft: node.name });
                    }}
                  >
                    <Pencil aria-hidden="true" className="size-3.5" />
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    tabIndex={active === node.relPath ? 0 : -1}
                    aria-label={`${SESSION_TEMPLATE_ENTRY_DELETE} ${node.relPath}`}
                    title={SESSION_TEMPLATE_ENTRY_DELETE}
                    className="size-6 text-muted-foreground hover:text-destructive"
                    onClick={() => {
                      onNotice(null);
                      setDeleting(node);
                    }}
                  >
                    <Trash2 aria-hidden="true" className="size-3.5" />
                  </Button>
                </span>
              </div>
            );
          })}
        </div>
      )}

      {/* The confirmation names the entry, because "Delete this?" over a tree is a
          question about which row. */}
      <AlertDialog open={deleting !== null} onOpenChange={(next) => !next && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{SESSION_TEMPLATE_DELETE_TITLE}</AlertDialogTitle>
            <AlertDialogDescription>
              {deleting?.relPath} — {SESSION_TEMPLATE_DELETE_BODY}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>
              {SESSION_TEMPLATE_ENTRY_DELETE}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/**
 * A file's icon, through the one viewer registry — so a `.csv` in a template looks
 * like a `.csv` in the Files pane and in a session's own tree. The registry
 * decides; this maps its answer to the shared icon table.
 */
function TemplateFileIcon({ name }: { name: string }) {
  const Icon = VIEWER_ICON[resolveViewer({ name, kind: "file" }).icon];
  return <Icon aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />;
}
