/**
 * What this note has attached, and the one control that puts a session's file
 * in the body (Story 43.7, FR-152, UX-DR56; Story 46.2, AD-103).
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
 * **Two sources, because there are two (Story 46.2, AD-103).** For four epics
 * this read the `files:` key alone and returned "this isn't a recording note"
 * before it had looked at the body at all — so pressing "Attach a file", which
 * writes an embed into the *body* and never touches frontmatter, put a file
 * into a note that the panel named after it could not list. It was a
 * recording-session panel wearing a general name. It now reads both: the
 * session's `files:` where there is one, unchanged, and the body's own embeds.
 * The two are separate lists rather than one merged list because they are in
 * different frames — `files:` is relative to the recordings destination root
 * and a body embed is relative to the vault — and a merged list would be one
 * column of paths with two meanings.
 *
 * **`attachments/` is not the test (Story 46.11, epic 46's spine).** 46.2 listed
 * a body embed only when its target was written under `attachments/`, because
 * that reader is pure and a prefix is a fact about the text where containment is
 * a fact about the disk. 46.11 makes an in-vault attach point at the file where
 * it already lives — `photos/a.png` — so the prefix would hide precisely the
 * files that story adds. The rule is now the spine's: a row is made by the note
 * embedding a file **and the vault holding it**, wherever it lives.
 *
 * That needs a disk the pure reader does not have, so it asks for one.
 * {@link bodyEmbedTargets} says what the text embeds and
 * {@link notesEmbedPaths} says which of those the vault holds — through the same
 * `embed::candidates` + containment resolution the embed viewer and
 * `export::plan` use, so this panel cannot list a file the exporter would refuse
 * to carry or open a different one from the viewer. What the panel does NOT do
 * is guess: an embed nothing has answered about yet is neither listed nor
 * denied, and one whose file is genuinely gone is said out loud rather than
 * dropped.
 *
 * **The empty state is about attachments.** It used to announce that the note
 * is not a recording note, which is true, is a different fact, and is an answer
 * to a question nobody opening a panel called "Attachments" asked.
 *
 * **The embed is not spelled here.** {@link attachmentEmbed} in
 * `@/lib/notes/attach` is the one place this app composes an attachment embed
 * (Story 45.13): this panel, the Files pane's chooser and the editor's file
 * picker all write the same bytes for the same file because they all call it.
 * Before that story there were two spellings for one act — this one, and a
 * dead `![name](rel)` in Rust that nothing rendered.
 *
 * {@link embeddedAttachmentNames} answers "does the body already have it" for
 * the same reason and in the same place, and is pinned to `keeper-core`'s copy
 * of the rule by a shared vector table so the panel and the chooser cannot
 * disagree.
 */
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  notesEmbedPaths,
  type RecordingNoteTargetVm,
  recordingNoteTargets,
} from "@/lib/ipc/client";
import {
  attachmentEmbed,
  attachmentName,
  bodyEmbedPlan,
  bodyEmbedTargets,
  embeddedAttachmentNames,
} from "@/lib/notes/attach";
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
 *
 * Exported since Story 46.11, which added a second surface saying the same thing
 * about the same file: the in-vault chooser's rows. One fact, one spelling — a
 * second word for "this note already has it" would be the kind of drift this
 * feature keeps paying for.
 */
export const ATTACHMENT_PRESENT_LABEL = "In the note";

/**
 * What each of the two lists is called, and *only* when both are on screen.
 *
 * The two answer different questions in different frames — `files:` is relative
 * to the recordings destination root, a body embed is relative to the vault —
 * so a reader looking at both at once needs to know which is which. A panel
 * showing one list needs no heading to disambiguate it from a list that is not
 * there, and a heading over a single list is a word that carries nothing.
 */
const SESSION_LIST_CAPTION = "From this note's properties";
const BODY_LIST_CAPTION = "In this note's body";

/**
 * What the body list says while the probe is in flight.
 *
 * The same discipline the session list's "keeper can't locate this session"
 * follows: while keeper is still looking it says it is looking, because both
 * "this note has no attachments" and "keeper cannot find that file" are claims
 * and neither is true yet. Borrowed verbatim from the note chooser
 * (`ATTACH_SEARCHING_SENTENCE`), which waits on a search for the same reason.
 */
const BODY_LOOKING_SENTENCE = "Looking…";

/**
 * What the panel says when the probe itself failed — a vault that went away, a
 * volume that stopped answering.
 *
 * Distinct from "the vault does not hold it": keeper did not find out, and
 * saying it did would blame the note for keeper's own outage. The note's text is
 * still on screen in the editor, which is the one place this fact is never in
 * doubt.
 */
const BODY_UNCHECKED_SENTENCE =
  "keeper could not check which of this note's files are in the vault, so it is not listing them.";

/** The key carrying a recording note's files, each relative to the recordings
 *  destination root — the same frame `recording:` is written in. */
const FILES_KEY = "files";

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
  const name = attachmentName(relativePath);
  const found = targets.find(
    (target) => target.kind !== "folder" && attachmentName(target.relativePath) === name,
  );
  return found?.kind ?? null;
}

export interface AttachmentsPanelProps {
  /**
   * The vault the note lives in, so the panel can ask what the vault holds
   * (Story 46.11).
   *
   * A vault id and never a path: the webview does not know where the vault is
   * and must not learn (AD-65). The panel still holds no absolute path, and
   * still never acts on a file — it asks one question and renders the answer.
   */
  vaultId: string;
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

/** No target has been asked about. Shared, so a re-render with nothing embedded
 *  does not hand the plan a fresh map and re-render everything below it. */
const NOTHING_RESOLVED: ReadonlyMap<string, string | null> = new Map();

export function AttachmentsPanel({ vaultId, frontmatter, body, onInsert }: AttachmentsPanelProps) {
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

  // What the TEXT embeds (Story 46.11). Pure, off the live buffer, so a file
  // attached a keystroke ago is a target a keystroke ago — which is the whole of
  // the defect 46.2 fixed and is not given up here.
  const embedTargets = bodyEmbedTargets(body);
  // A key, not the array: the array is fresh on every keystroke and the targets
  // in it change only when an embed is written or removed, which is when the
  // vault is worth asking again. `AttachToNoteDialog` keys its own search the
  // same way and for the same reason. `\u0000` cannot occur in a target — the
  // one link grammar stops a target at its own line — so the join round-trips.
  const targetsKey = embedTargets.join("\u0000");

  // Three states again, and the same three: `undefined` is "not asked yet",
  // `null` is "asked, and keeper could not find out", a map is the answer.
  const [resolved, setResolved] = useState<ReadonlyMap<string, string | null> | null | undefined>(
    undefined,
  );

  useEffect(() => {
    // Cleared first, like the session resolve above: this component outlives the
    // note in it, and the previous note's answers are about the previous note's
    // embeds.
    setResolved(undefined);
    if (targetsKey === "") {
      // Nothing embedded is not a question. Answered here rather than left
      // `undefined`, or a note with no embeds would sit on "Looking…" forever.
      setResolved(NOTHING_RESOLVED);
      return;
    }
    const asked = targetsKey.split("\u0000");
    let live = true;
    void notesEmbedPaths(vaultId, asked)
      .then((paths) => {
        if (live) {
          // Zipped by position, which is the command's contract: one answer per
          // target, in the order asked. A short reply leaves the tail
          // unanswered rather than silently pairing the wrong path with the
          // wrong embed.
          // The path only: this panel lists what the vault holds, and the
          // `kind` the answer now also carries (Story 55.4) is the note
          // decoration's business, not a row's.
          setResolved(new Map(asked.map((target, at) => [target, paths[at]?.relPath ?? null])));
        }
      })
      .catch(() => {
        if (live) {
          setResolved(null);
        }
      });
    return () => {
      live = false;
    };
  }, [vaultId, targetsKey]);

  // The session's own list, and nothing for a note that has no session: a
  // `files:` key in an ordinary note is somebody else's list, in a frame this
  // panel has no root for.
  const sessionFiles = sessionId === null ? [] : noteAttachments(parsed);
  const embedded = embeddedAttachmentNames(body);
  // The other source (Story 46.2, AD-103), joined to what the vault said
  // (Story 46.11). `null` — the probe failed — is not "nothing is embedded": it
  // is every target still unanswered, which is what `NOTHING_RESOLVED` makes it.
  const plan = bodyEmbedPlan(body, resolved ?? NOTHING_RESOLVED);

  // The two lists are kept disjoint, and Story 46.11 is why they now have to be
  // said to be.
  //
  // 46.2 recorded "a body attachment whose name collides with a `files:` entry"
  // as two rows for two files, correct because the two lists were in two roots
  // and the body list only ever held `attachments/`. Both halves of that stopped
  // being true here: the body list now holds any path, and a session file
  // inserted by the panel's own Insert button is embedded at the note's own
  // spelling of it — so the same file would appear twice, once with a kind and
  // an Insert-turned-caption and once bare.
  //
  // Matched by NAME, which is the join key every surface in this feature uses
  // and for Story 40.4's reason: the session folder is renamed after the note is
  // written, so `![[old/screen.mov]]` and `![[new/screen.mov]]` are one file.
  // It is the same key `embedded` and `kindOf` above already join on, so the row
  // that says "In the note" and the row this drops are the same row.
  //
  // This is also what keeps the panel from accusing a recording note of
  // embedding a file the vault does not hold. A recordings destination root may
  // be anywhere — the fixture's is `~/Movies/keeper` — so a session embed
  // resolves to nothing *in the vault*, and it renders anyway because
  // `recording-embed.ts` resolves it against the session index instead. The
  // session list is the authority on those files and already says what it knows,
  // including that it cannot locate them.
  const sessionNames = new Set(sessionFiles.map((path) => attachmentName(path).toLowerCase()));
  const ownedBySession = (path: string) => sessionNames.has(attachmentName(path).toLowerCase());
  const bodyFiles = plan.present.filter((path) => !ownedBySession(path));
  const bodyMissing = plan.missing.filter((path) => !ownedBySession(path));
  const bodyPending = plan.pending.filter((path) => !ownedBySession(path));
  // Everything the body section has to say, not only its rows: a note whose one
  // photograph has been deleted is not a note with no attachments, and it must
  // not be told that it is. A failed probe needs no clause of its own — it
  // leaves every target unanswered, so it is already `bodyPending`.
  const bodySpeaks = bodyFiles.length > 0 || bodyMissing.length > 0 || bodyPending.length > 0;
  const bothLists = sessionFiles.length > 0 && bodySpeaks;

  if (sessionFiles.length === 0 && !bodySpeaks) {
    return (
      <section aria-label={ATTACHMENTS_LABEL} className="border-b px-3 py-2 text-xs">
        <p className="text-muted-foreground">
          {sessionId === null
            ? // About attachments, because that is what the panel is named and
              // what the person opening it asked about. The old sentence here
              // announced that the note is not a recording note, which is true
              // and is an answer to a question nobody asked — and it was the
              // wall an attached file hit, since it was returned before the
              // body had been read at all.
              //
              // No longer "a file from attachments/" (Story 46.11): that folder
              // stopped being the test, so naming it here would describe a
              // narrower panel than the one underneath.
              "This note has no attachments — nothing in it embeds a file this vault holds. Attaching one adds it here."
            : // Kept exactly: a recording note whose properties list no files
              // is a different fact from a note with no attachments, and the
              // two sentences are the two facts.
              "This recording note's properties list no files, so there is nothing to insert."}
        </p>
      </section>
    );
  }

  return (
    <section aria-label={ATTACHMENTS_LABEL} className="border-b px-3 py-2 text-xs">
      {sessionFiles.length === 0 ? null : (
        <>
          {bothLists ? <p className="text-muted-foreground">{SESSION_LIST_CAPTION}</p> : null}
          <ul className="flex flex-col gap-0.5">
            {sessionFiles.map((relativePath) => {
              const kind = kindOf(targets, relativePath);
              // Folded, because {@link embeddedAttachmentNames} folds: APFS is
              // case-insensitive, so `Screen.MOV` in the note is this file.
              const present = embedded.has(attachmentName(relativePath).toLowerCase());
              return (
                <li key={relativePath} className="flex min-w-0 items-center gap-2">
                  {/* The name, with the note's own relative path as the tooltip.
                      The absolute path is never on screen (FR-145) — this panel
                      does not even hold one, since it never acts on a file. */}
                  <span
                    className="min-w-0 flex-1 truncate font-mono text-meta"
                    title={relativePath}
                  >
                    {attachmentName(relativePath)}
                  </span>
                  {kind === null ? null : (
                    <span className="shrink-0 text-muted-foreground">{kind}</span>
                  )}
                  {present ? (
                    <span className="shrink-0 text-muted-foreground">
                      {ATTACHMENT_PRESENT_LABEL}
                    </span>
                  ) : (
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      className="h-6 shrink-0"
                      // The accessible name is the path that will land in the
                      // note, not the name on screen: a session's four files are
                      // four identical "Insert" buttons to anyone not looking.
                      aria-label={`${ATTACHMENT_INSERT_LABEL} ${relativePath}`}
                      // Spelled in one place for the whole app (Story 45.13), so
                      // this row and the Files pane's chooser write the same
                      // bytes.
                      onClick={() => onInsert(attachmentEmbed(relativePath))}
                    >
                      {ATTACHMENT_INSERT_LABEL}
                    </Button>
                  )}
                </li>
              );
            })}
          </ul>
          {/* `null`, not "falsy": while the answer is still in flight the panel
              says nothing, because "keeper can't find it" is a claim and it is
              not true yet. Attached to this list only — it is a claim about the
              session, and the body's files were never looked for there. */}
          {targets === null ? (
            <p className="pt-1 text-muted-foreground">
              keeper can't locate this session right now, so it can't say what these files are.
              Inserting still writes what the note says.
            </p>
          ) : null}
        </>
      )}
      {!bodySpeaks ? null : (
        <>
          {bothLists ? <p className="pt-1 text-muted-foreground">{BODY_LIST_CAPTION}</p> : null}
          {bodyFiles.length === 0 ? null : (
            <ul className="flex flex-col gap-0.5">
              {bodyFiles.map((relativePath) => (
                <li key={relativePath} className="flex min-w-0 items-center gap-2">
                  {/* The path the vault RESOLVED, not the target as written: a
                      bare `![[photo.png]]` is a row for the file it actually
                      shows, which is the one the viewer opens and the export
                      carries. Still no absolute path (FR-145). */}
                  <span
                    className="min-w-0 flex-1 truncate font-mono text-meta"
                    title={relativePath}
                  >
                    {attachmentName(relativePath)}
                  </span>
                  {/* No kind, and no Insert.

                      No kind because nothing has been asked about this file:
                      `kindOf` matches the session index by NAME, so a session
                      holding its own `photo.png` would label the vault's
                      `attachments/photo.png` from a different file entirely.
                      Reading the extension here would be the second classifier
                      Story 43.5 exists to prevent.

                      No Insert because the row exists *because* the body embeds
                      it, so the one label a session row wears once inserted is
                      the only one this row could ever wear. No new verb: reveal
                      and open both need an absolute path, and FR-145 is the
                      reason this panel holds none. */}
                  <span className="shrink-0 text-muted-foreground">{ATTACHMENT_PRESENT_LABEL}</span>
                </li>
              ))}
            </ul>
          )}
          {/* The three things the body list can say instead of, or as well as,
              its rows (Story 46.11) — in the order of how much they claim.

              A probe still in flight says only that keeper is looking. A probe
              that FAILED says so about itself rather than about the note: keeper
              did not find out, and "the vault does not have it" would be blaming
              the note for keeper's own outage. Both leave every target
              unanswered, which is why they are two arms of one condition rather
              than two independent lines.

              A target the vault answered `null` for IS named: this note embeds a
              file that is not there, and dropping the row in silence is exactly
              the failure this epic is about. Not a row, though — a row in this
              list means "the vault has it", and this does not. */}
          {bodyPending.length === 0 ? null : (
            <p className="pt-1 text-muted-foreground">
              {resolved === null ? BODY_UNCHECKED_SENTENCE : BODY_LOOKING_SENTENCE}
            </p>
          )}
          {bodyMissing.length === 0 ? null : (
            <p className="pt-1 text-muted-foreground">
              {`This note embeds ${bodyMissing.join(", ")}, which ${
                bodyMissing.length === 1 ? "is" : "are"
              } not in this vault.`}
            </p>
          )}
        </>
      )}
    </section>
  );
}
