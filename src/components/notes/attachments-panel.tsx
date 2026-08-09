/**
 * A recording note's own attachments, and the one control that puts one in the
 * body (Story 43.7, FR-152, UX-DR56).
 *
 * Story 42.4 taught the note where its files are and the properties panel shows
 * them; Story 43.5 taught the body to render one as a player, an image, an
 * audio element or a chip. Between the two there was nothing: the only way to
 * get an attachment into the body was to type `![[` and a path from memory, in
 * a frame — relative to the recordings destination root — that is not the frame
 * the user's file manager shows them. This panel is that missing step and
 * nothing else.
 *
 * **An inserter, not a renderer.** What it writes is `![[<the note's own
 * path>]]`, byte for byte the text a person would have typed. There is no
 * keeper-specific marker, no attribute and no second syntax: the note stays a
 * file Obsidian reads the same way, and an embed made here is indistinguishable
 * from one made by hand — including to `recording-embed.ts`, which is the point.
 * A panel that wrote its own dialect would be a panel whose notes only keeper
 * can read.
 *
 * **The list is the NOTE's, not the session folder's.** Every row comes from the
 * note's own `files:` key. A file that appeared in the session folder after the
 * stub was written is not one of this note's attachments, and offering it here
 * would quietly make this panel a second file browser — which is Story 43.8's
 * job, in its own surface, with its own rules. The index is asked only what each
 * listed file IS, so the row can say whether inserting yields a player or a
 * chip.
 *
 * **Why this is a panel and not more `/` commands.** Story 43.9 has just made
 * the slash menu open for the first time, and two affordances that both insert
 * things is exactly how a surface gets confusing — so the split has to be a real
 * one. It is: `/` offers a closed table of six literal insertions that are the
 * same in every note, triggered only at the start of an empty line, computed
 * synchronously from a `Date`. Attachments are none of those things. They are
 * per-note, they arrive from an IPC call that can answer `null`, they carry
 * states a completion row cannot express — "already in this note", "keeper
 * can't locate this file" — and the sentence a photo belongs in the middle of is
 * not an empty line. `/` answers "what can this editor insert?"; the panel
 * answers "what does THIS note have?", which is a question about the note and
 * which `/` has no way to know the answer to. See the spec for the full
 * reasoning and for the option that was kept open.
 *
 * **A note with no `session:` gets a sentence, not an empty list.** An empty
 * list reads as "this note has no attachments" when the truth is "this is not a
 * recording note at all", and the two are different facts about the note.
 *
 * `WIKILINK` is imported from the editor's own module rather than re-spelled
 * here: "no second embed syntax" is the epic's rule, and a second regex for the
 * one syntax is how a second syntax starts. The import costs no bundle weight
 * that matters — `wikilink.ts`'s only runtime dependency is the IPC client this
 * file already imports, and its CodeMirror imports are type-only.
 */
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { type RecordingNoteTargetVm, recordingNoteTargets } from "@/lib/ipc/client";
import { WIKILINK } from "./editor/wikilink";
import { type ParsedFrontmatter, readFrontmatter, recordingSessionId } from "./properties-panel";

/** The panel's accessible name, and the word on the button that opens it. */
export const ATTACHMENTS_LABEL = "Attachments";

/** The insert action's label. One verb, because it does one thing. */
const ATTACHMENT_INSERT_LABEL = "Insert";

/**
 * What a row reads instead of offering the action, once the body embeds it.
 *
 * Said rather than disabled, and it replaces the control rather than sitting
 * beside it: with no button there is no second press to guard against, so "the
 * panel cannot embed the same attachment twice" is a fact about what is on
 * screen and not a rule enforced in a click handler.
 */
const ATTACHMENT_PRESENT_LABEL = "In the note";

/** The key carrying a recording note's files, each relative to the recordings
 *  destination root — the same frame `recording:` is written in. */
const FILES_KEY = "files";

/** The last `/`-separated component of a relative path. */
function fileName(relativePath: string): string {
  const segments = relativePath.split("/").filter((segment) => segment !== "");
  return segments[segments.length - 1] ?? relativePath;
}

/**
 * The paths this note calls its own, in the order the note lists them.
 *
 * `files:` is a block list in every note keeper writes, but a note is a file a
 * human may edit, so a single scalar `files: one.mov` is read as the one entry
 * it plainly is rather than as nothing. A nested map under the key is read as
 * nothing: that is a shape this reader does not have, and guessing at it is how
 * a panel offers to insert a path that is not there.
 *
 * The session folder — `recording:` — is deliberately absent. It is not an
 * attachment: there is no element for a directory, so an embed of one renders
 * as the link it already was.
 */
export function noteAttachments(parsed: ParsedFrontmatter): string[] {
  const entry = parsed.entries.find((candidate) => candidate.key === FILES_KEY);
  if (entry === undefined || entry.nested) {
    return [];
  }
  const listed = entry.items.length > 0 ? entry.items : [entry.text];
  return listed.map((path) => path.trim()).filter((path) => path !== "");
}

/**
 * The file names the body already embeds.
 *
 * Matched by NAME, not by the whole path, because that is the join key every
 * other surface in this feature uses and for the same reason: Story 40.4
 * renames a session folder after the note is written, so `![[old/screen.mov]]`
 * and `![[new/screen.mov]]` are one file rendered twice — a duplicate by the
 * only definition the reader can see. `recording-embed.ts` resolves by name
 * too, so the panel's idea of "already there" and the widget's idea of "this is
 * that file" cannot drift apart.
 *
 * Links are ignored and embeds are not: `[[screen.mov]]` is a mention, `!` is
 * the whole of the difference, and the panel only ever writes the second.
 */
export function embeddedAttachmentNames(body: string): Set<string> {
  const names = new Set<string>();
  // Stateful regex, shared with the decoration layer: reset before use.
  WIKILINK.lastIndex = 0;
  for (const match of body.matchAll(WIKILINK)) {
    if (match[0].startsWith("!")) {
      names.add(fileName(match[1]));
    }
  }
  return names;
}

/**
 * What the index says one of the note's files IS, or `null` when it cannot say.
 *
 * Never decided here. The kind is 43.5's one answer, computed in Rust by
 * `kind_for_file_name`, and a frontend that classified `.png` itself would be
 * the second extension table that story exists to prevent. `null` means keeper
 * does not know — an unplugged volume, a session the index cannot place, a file
 * since deleted — and an honest row says nothing about the kind rather than
 * guessing from the name it can see.
 */
function kindOf(
  targets: readonly RecordingNoteTargetVm[] | null | undefined,
  relativePath: string,
): RecordingNoteTargetVm["kind"] | null {
  if (targets === null || targets === undefined) {
    return null;
  }
  const name = fileName(relativePath);
  const found = targets.find(
    (target) => target.kind !== "folder" && fileName(target.relativePath) === name,
  );
  return found?.kind ?? null;
}

export interface AttachmentsPanelProps {
  /** The note's frontmatter block, verbatim — the same text the properties
   *  panel reads, so both surfaces answer "is this a recording note" alike. */
  frontmatter: string;
  /** The buffer as the editor has it *now*, so a row flips to "in the note" on
   *  the keystroke after the insert rather than after the next save. */
  body: string;
  /** Put this text where the caret is. The editor owns the caret; this panel
   *  owns only the text, which is the whole of the separation. */
  onInsert: (text: string) => void;
}

export function AttachmentsPanel({ frontmatter, body, onInsert }: AttachmentsPanelProps) {
  const parsed = readFrontmatter(frontmatter);
  const sessionId = recordingSessionId(parsed);
  // Three states, not two: `undefined` is "not asked yet", `null` is "asked,
  // and keeper cannot place this session". Collapsing them would make the
  // panel accuse the volume of being missing for the first frame of every
  // note it opens, which is a sentence that must only appear when it is true.
  const [targets, setTargets] = useState<RecordingNoteTargetVm[] | null | undefined>(undefined);

  // The same resolve the properties panel does, by session id and once per
  // note, because the id is the handle that survives a Story 40.4 retitle. A
  // failure and an unknown session are one state — no targets — since the
  // panel's answer to both is the note's own text and no claim about the files.
  useEffect(() => {
    // Cleared first: this component outlives the note in it, and a stale list
    // would otherwise label the new note's files with the old note's kinds.
    setTargets(undefined);
    if (sessionId === null) {
      return;
    }
    let live = true;
    void recordingNoteTargets(sessionId)
      .then((resolved) => {
        if (live) {
          setTargets(resolved);
        }
      })
      .catch(() => {
        if (live) {
          setTargets(null);
        }
      });
    return () => {
      live = false;
    };
  }, [sessionId]);

  if (sessionId === null) {
    return (
      <section aria-label={ATTACHMENTS_LABEL} className="border-b px-3 py-2 text-xs">
        <p className="text-muted-foreground">
          This note has no session, so it isn't a recording note — keeper knows of no files that
          belong to it.
        </p>
      </section>
    );
  }

  const attachments = noteAttachments(parsed);
  const embedded = embeddedAttachmentNames(body);

  if (attachments.length === 0) {
    return (
      <section aria-label={ATTACHMENTS_LABEL} className="border-b px-3 py-2 text-xs">
        <p className="text-muted-foreground">
          This recording note's properties list no files, so there is nothing to insert.
        </p>
      </section>
    );
  }

  return (
    <section aria-label={ATTACHMENTS_LABEL} className="border-b px-3 py-2 text-xs">
      <ul className="flex flex-col gap-0.5">
        {attachments.map((relativePath) => {
          const kind = kindOf(targets, relativePath);
          const present = embedded.has(fileName(relativePath));
          return (
            <li key={relativePath} className="flex min-w-0 items-center gap-2">
              {/* The name, with the note's own relative path as the tooltip.
                  The absolute path is never on screen (FR-145) — this panel
                  does not even hold one, since it never acts on a file. */}
              <span className="min-w-0 flex-1 truncate font-mono text-[11px]" title={relativePath}>
                {fileName(relativePath)}
              </span>
              {kind === null ? null : (
                <span className="shrink-0 text-muted-foreground">{kind}</span>
              )}
              {present ? (
                <span className="shrink-0 text-muted-foreground">{ATTACHMENT_PRESENT_LABEL}</span>
              ) : (
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  className="h-6 shrink-0"
                  // The accessible name is the path that will land in the note,
                  // not the name on screen: a session's four files are four
                  // identical "Insert" buttons to anyone not looking at it.
                  aria-label={`${ATTACHMENT_INSERT_LABEL} ${relativePath}`}
                  // The embed, spelled here and nowhere else, exactly as a
                  // person types it: `!`, the wikilink, the note's own path.
                  onClick={() => onInsert(`![[${relativePath}]]`)}
                >
                  {ATTACHMENT_INSERT_LABEL}
                </Button>
              )}
            </li>
          );
        })}
      </ul>
      {/* `null`, not "falsy": while the answer is still in flight the panel
          says nothing, because "keeper can't find it" is a claim and it is not
          true yet. */}
      {targets === null ? (
        <p className="pt-1 text-muted-foreground">
          keeper can't locate this session right now, so it can't say what these files are.
          Inserting still writes what the note says.
        </p>
      ) : null}
    </section>
  );
}
