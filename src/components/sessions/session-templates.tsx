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
 */
import { Pencil } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  SESSION_PATTERN_INSTALL_LABEL,
  TEMPLATE_ID_PREFIX,
} from "@/components/sessions/session-pattern-picker";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupInput } from "@/components/ui/input-group";
import { formatDraftAge } from "@/lib/format-time";
import type { SessionPatternVm, SessionTemplateEntryVm } from "@/lib/ipc/client";
import { sessionsTemplateEntries, sessionsTemplateRename } from "@/lib/ipc/client";
import { panelsStore } from "@/lib/stores/panels";
import { syncErrorMessage } from "@/lib/stores/sync";

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

/**
 * One template: its label, what is inside it, and — for a named one — the
 * rename.
 *
 * The rename field starts on the name the template already has, which is the
 * name most renames are a small edit to. That seeding is a mount-time value and
 * stays honest because a successful rename changes the id this section is keyed
 * on: the section remounts under the new name rather than holding the old draft.
 */
function TemplateSection({
  template,
  files,
  failure,
  rootId,
  nowMs,
  editing,
  renaming,
  onEdit,
  onCancelEdit,
  onRename,
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
  nowMs: number;
  editing: boolean;
  renaming: boolean;
  onEdit: () => void;
  onCancelEdit: () => void;
  onRename: (next: string) => void;
}) {
  const [draft, setDraft] = useState(template.label);
  // The zone's own template is the one thing here without a name to change: the
  // directory name IS the contract, and there is exactly one of it per zone.
  const renameable = template.id !== TEMPLATE_ID_PREFIX;

  return (
    <div
      data-testid={`${SESSION_TEMPLATE_SECTION_TESTID}-${template.id}`}
      className="flex flex-col gap-1 rounded-md border border-border px-3 py-2"
    >
      <div className="flex min-w-0 items-center gap-2">
        <h4 className="min-w-0 truncate font-medium text-sm">{template.label}</h4>
        {files !== null && (
          <span className="figures shrink-0 text-muted-foreground text-xs">{files.length}</span>
        )}
        <span className="min-w-0 flex-1" />
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

      {failure !== null ? (
        // Rust's own sentence, in the section it is about. One refused read costs
        // this template's files and nothing else — its siblings show theirs.
        <p className="text-muted-foreground text-xs">{failure}</p>
      ) : files === null ? (
        <p className="text-muted-foreground text-xs">{SESSION_TEMPLATES_LOADING}</p>
      ) : files.length === 0 ? (
        <p className="text-muted-foreground text-xs">{SESSION_TEMPLATES_NO_FILES}</p>
      ) : (
        <ul aria-label={template.label} className="flex flex-col">
          {files.map((entry) => (
            <li
              key={entry.subpath}
              data-testid={`${SESSION_TEMPLATE_FILE_TESTID}-${entry.subpath}`}
            >
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 w-full min-w-0 justify-start gap-2 px-2 font-normal"
                // The subpath Rust composed, handed over as it arrived. A join
                // here would be a second answer to a question Rust already
                // answered, and the two would drift the day a zone's subfolder
                // stops being the zone's name (AD-65).
                onClick={() =>
                  panelsStore.getState().setActiveTarget({
                    kind: "file",
                    profileId: rootId,
                    relativePath: entry.subpath,
                  })
                }
              >
                {/* `entry.name` is the file's path INSIDE the template, not a
                    basename, since the walk started reaching into subdirectories
                    (`sessions_ipc.rs`, `pattern_files`) — `prompts/hand-off.md`.
                    So the DIRECTORY is what gives way when the pane narrows: one
                    `truncate` over the whole string clipped the tail and left
                    `prompts/pro…` standing where the filename should be, which is
                    the half that says which row this is. Split for display only;
                    the path that opens the file is still Rust's `subpath`,
                    untouched (AD-65). */}
                <span className="flex min-w-0 flex-1 items-baseline text-sm">
                  <span className="min-w-0 truncate text-muted-foreground">
                    {entry.name.slice(0, entry.name.lastIndexOf("/") + 1)}
                  </span>
                  <span className="shrink-0">
                    {entry.name.slice(entry.name.lastIndexOf("/") + 1)}
                  </span>
                </span>
                <span className="figures shrink-0 text-muted-foreground text-xs">
                  {formatDraftAge(entry.mtimeMs, nowMs)}
                </span>
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
