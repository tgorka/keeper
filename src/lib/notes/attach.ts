/**
 * The one path an attachment takes into a note (Story 45.13, FR-188, FR-189,
 * UX-DR76).
 *
 * # Why this module exists
 *
 * Before this story there were two inserters and they disagreed.
 * `AttachmentsPanel` (Story 43.7) wrote `![[recordings/…/screen.mov]]`, which
 * is Obsidian's embed and which `live-preview.ts` decorates into a player.
 * `keeper_core`'s `attachment_markdown` — reachable only through
 * `notes_attachment_drop`, a command with no caller in this app since epic 37 —
 * wrote `![name](attachments/name.png)`, which decorates into nothing. Two
 * spellings for one act, one of them dead and untested. The dead one is gone
 * and this module is the survivor, so that a file attached from the panel, from
 * a Files-pane multiselection and from the file picker lands as the same bytes.
 *
 * # Which spelling won, and why
 *
 * `![[vault-relative/path]]`. Not because it was here first: it is Obsidian's
 * own embed syntax, so the vault stays a folder Obsidian reads unchanged; it is
 * the only spelling this app's decoration layer renders as a player, an image
 * or a table; and it is what a person typing by hand would write, so an embed
 * made by keeper is indistinguishable from one made by hand — including to
 * `recording-embed.ts`, which is the point.
 *
 * Both spellings still *count* as holding the file — see
 * {@link embeddedAttachmentNames}. Only one is ever written.
 *
 * # An absolute path is never the answer
 *
 * FR-145 forbids an absolute path in a synced artefact, and the reason is
 * mechanical rather than stylistic: the vault syncs to other machines, where
 * `/Users/someone/Desktop/photo.png` names nothing — or, worse, names a
 * different file. So a source outside the vault is copied into the vault and
 * the note names the copy. Rust does the copying (it is the only process that
 * knows where the vault is, AD-65); this module never sees an absolute path and
 * never joins one.
 *
 * # The duplicate rule, and where it also lives
 *
 * {@link embeddedAttachmentNames} is a mirror of
 * `keeper_core::notes::attach::embedded_attachment_names`. The mirror exists
 * because the open editor's buffer lives only in the webview — Rust cannot read
 * what has not been saved — while the note chooser has to answer the same
 * question about notes that are closed on disk, which only Rust can read.
 *
 * The two are pinned to each other by
 * `src-tauri/crates/keeper-core/src/notes/attach-vectors.json`, which both test
 * suites load. If they drift, the chooser offers a note that the panel then
 * refuses to write into — the same "two answers to one question" the two
 * inserters were. A mirror documented as a mirror drifts within a month; a
 * mirror pinned to a shared table fails on the commit that breaks it.
 *
 * The rules, restated so this file is readable without the Rust open: an embed
 * is `![[target]]` or `![alt](dest)`; a link without the `!` is not one; fenced
 * and inline code hold no links; a `#anchor` is dropped; a markdown destination
 * is percent-decoded, may be wrapped in `<>`, may carry a quoted title, and is
 * skipped when it has a URL scheme; the key is the last `/`-separated segment,
 * folded to lower case.
 */

/**
 * The one spelling. Nothing else in this app composes an attachment embed.
 *
 * Takes a vault-relative path — the string Rust resolved and handed over. The
 * webview never builds one (AD-65) and never holds an absolute one (FR-145).
 */
export function attachmentEmbed(relativePath: string): string {
  return `![[${relativePath}]]`;
}

/**
 * What separates two embeds written in one gesture.
 *
 * A newline rather than a space: an embed renders as a block — a player, an
 * image, a table — and two block widgets sharing a line is a layout keeper does
 * not have. Selecting four files and getting four lines is also what the person
 * pointed at.
 */
const EMBED_SEPARATOR = "\n";

/** The last `/`-separated component of a path: the file's own name. */
export function attachmentName(relativePath: string): string {
  const cut = relativePath.lastIndexOf("/");
  return cut === -1 ? relativePath : relativePath.slice(cut + 1);
}

/**
 * The characters a wikilink target cannot carry.
 *
 * `]` closes it, `[` opens a nested one, `|` starts an alias, `#` starts an
 * anchor and a newline ends the whole link — Obsidian's reader and keeper's
 * both stop at each of these, so a file whose name holds one cannot be named by
 * `![[…]]` at all. Every one of them is a legal character in a macOS filename,
 * so this is a case that happens rather than a theoretical one, and the honest
 * answer is a sentence rather than an embed that points at the wrong file.
 */
const WIKILINK_HOSTILE = /[[\]|#\n\r]/;

/**
 * Whether an embed can name this path at all.
 *
 * The same rule {@link planAttachments} refuses on, asked *before* the offer
 * rather than after it (Story 46.11). A file browser over a real vault can show
 * a file whose name holds a `#`, and a row that offers a button and then answers
 * with a refusal is the shape this feature already decided against for the
 * duplicate case: the reason goes where the button would have been.
 */
export function wikilinkNameable(relPath: string): boolean {
  return !WIKILINK_HOSTILE.test(relPath);
}

/**
 * The file names this body already embeds, in either embed spelling, folded to
 * lower case.
 *
 * Mirrors `keeper_core::notes::attach::embedded_attachment_names`; see the
 * module comment for why a mirror exists and how it is pinned.
 *
 * By NAME rather than by path because that is the join key every surface in
 * this feature uses: Story 40.4 renames a session folder after its note is
 * written, so `![[old/screen.mov]]` and `![[new/screen.mov]]` are one file
 * shown twice. Folded because APFS is case-insensitive, so `Photo.PNG` and
 * `photo.png` are one file on the machine that wrote the note.
 */
export function embeddedAttachmentNames(body: string): Set<string> {
  const names = new Set<string>();
  for (const link of extractLinks(body)) {
    if (!link.embed) {
      continue;
    }
    const name = attachmentName(link.target).toLowerCase();
    if (name !== "") {
      names.add(name);
    }
  }
  return names;
}

/**
 * Whether an embed target names a note rather than a file.
 *
 * Mirrors `keeper_core::notes::export::names_a_note` and is pinned to it by
 * `attach-vectors.json`, which both test suites load. It has to be *that* rule
 * rather than a new one: if a surface called something an attachment that
 * `export::plan` classifies as a transclusion, it would list — or offer — a file
 * the export then refuses to carry, and the two receipts for one note would
 * disagree.
 *
 * Pinned rather than merely cited since Story 46.11, which is the second caller
 * 46.2 named as the trigger: {@link bodyEmbedTargets} reads it and the in-vault
 * chooser declines to offer a `.md` by it, so a drift would now show up as a
 * chooser offering a file the panel will not list.
 *
 * `![[daily.md]]` is explicit. `![[Other Note]]` has no extension at all,
 * because a wikilink names a note by its title — so an extensionless target is
 * a note by construction, not a file whose extension somebody forgot. The test
 * is on the last segment, so a folder called `photo.png` cannot make
 * `photo.png/index` look like an image. A dotfile needs no special case:
 * `.gitignore` yields the extension `gitignore`, which is not `md`, so the
 * ordinary arm already answers "file".
 */
export function namesANote(target: string): boolean {
  const name = attachmentName(target);
  const dot = name.lastIndexOf(".");
  return dot === -1 ? true : name.slice(dot + 1).toLowerCase() === "md";
}

/**
 * Every embed target in this body that names a **file** rather than a note,
 * spelled as the note spells it, in document order, deduplicated on the folded
 * target.
 *
 * The syntactic half of "what does this note have" (Story 46.2, AD-103; widened
 * by Story 46.11). It is deliberately only a half, and saying which half is the
 * whole design of this pair:
 *
 * - This function knows what the *text* says. It is pure, it runs over an
 *   unsaved buffer on every keystroke, and it has no disk.
 * - Whether the vault actually holds any of these files is a question only a
 *   `stat` answers, and `notes_embed_paths` answers it — through the same
 *   `embed::candidates` + containment resolution `notes_embed_read`, the
 *   `keeper-note://` protocol and `export::plan` all use. {@link bodyEmbedPlan}
 *   joins the two answers.
 *
 * **No `attachments/` prefix test.** 46.2 had one, because it had no disk and a
 * prefix is a fact about the text: the folder the copy path writes into was the
 * only thing it could be sure of. Story 46.11 makes an in-vault attach point at
 * the file where it already lives — `photos/a.png`, which never acquires the
 * prefix — so the prefix would now hide exactly the files this epic added. The
 * spine's ruling: a row is made by the note embedding a file and the vault
 * holding it, wherever it lives. This is the first clause; the probe is the
 * second. A bare `![[photo.png]]` is therefore no longer excluded either — it
 * was excluded because only a `stat` could say which of two candidates it
 * meant, and now something does the `stat`.
 *
 * A mention is not an attachment: `!` is the whole of the difference, exactly
 * as {@link embeddedAttachmentNames} and `export::plan` both read it. Code is
 * not a use, anchors are dropped and an external URL is not a file — all three
 * come from {@link extractLinks}, the one link grammar, and none of them is
 * decided here.
 */
export function bodyEmbedTargets(body: string): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const link of extractLinks(body)) {
    if (!link.embed) {
      continue;
    }
    const folded = link.target.toLowerCase();
    if (namesANote(link.target) || seen.has(folded)) {
      continue;
    }
    seen.add(folded);
    out.push(link.target);
  }
  return out;
}

/** What the note embeds, split by what the vault could be asked about it. */
export interface BodyEmbedPlan {
  /**
   * The vault-relative paths of the files the vault holds, in document order,
   * deduplicated on the resolved path.
   *
   * The *resolved* path and not the target as written, because those differ for
   * a bare name: `![[photo.png]]` resolving in the attachments folder is a row
   * for `attachments/photo.png`, which is the file the note actually shows.
   * Deduplicated after resolution for the same reason `export::plan` is — two
   * spellings that land on one file are one file.
   */
  present: readonly string[];
  /**
   * Targets the vault does not hold, as the note spells them. A real state: a
   * file deleted or moved after the embed was written.
   */
  missing: readonly string[];
  /**
   * Targets nothing has been asked about yet. Never empty on the first frame
   * after an embed is typed, and the reason a surface must not say "no
   * attachments" from `present.length === 0` alone.
   */
  pending: readonly string[];
}

/**
 * Join {@link bodyEmbedTargets} to what the vault said about each target.
 *
 * `resolved` maps a target to the vault-relative path it resolves to, `null`
 * when the vault holds no such file, and is *absent* for a target that has not
 * been asked about. Three answers and not two: "not asked yet" and "not there"
 * are different facts, and a surface that collapsed them would accuse the vault
 * of having lost a file for the first frame after every keystroke that writes
 * an embed.
 *
 * Line for line with `keeper_core::notes::export::plan`, which takes its own
 * probe as `&dyn Fn(&str) -> bool` for the same reason: the rule is pure and
 * the disk is the caller's. `plan`'s `notes` bucket is this function's silence
 * — {@link bodyEmbedTargets} has already dropped transclusions — and its
 * `missing` is this one's.
 */
export function bodyEmbedPlan(
  body: string,
  resolved: ReadonlyMap<string, string | null>,
): BodyEmbedPlan {
  const present: string[] = [];
  const missing: string[] = [];
  const pending: string[] = [];
  for (const target of bodyEmbedTargets(body)) {
    if (!resolved.has(target)) {
      pending.push(target);
      continue;
    }
    const rel = resolved.get(target) ?? null;
    if (rel === null) {
      missing.push(target);
    } else if (!present.includes(rel)) {
      present.push(rel);
    }
  }
  return { present, missing, pending };
}

/** What one gesture will write, what it will not, and why not. */
export interface AttachmentPlan {
  /**
   * The text to splice, exactly. `""` when the plan writes nothing, which a
   * caller must check before touching the buffer — an empty insert would still
   * put a save and an undo step in the way of a person who changed nothing.
   */
  text: string;
  /** The paths this plan writes, in the order they were offered. */
  inserted: readonly string[];
  /** The paths the note already holds, in the order they were offered. */
  alreadyThere: readonly string[];
  /** The paths no wikilink can name, in the order they were offered. */
  unnameable: readonly string[];
  /**
   * What to tell the person about everything this plan declined, or `null` when
   * it declined nothing.
   *
   * Never absent when something was refused. Silently doing nothing is the
   * failure this story was written to end.
   */
  refusal: string | null;
}

/**
 * Decide what attaching these files to this body actually does.
 *
 * Pure, and the whole of the decision: all three entry points call this and
 * then differ only in how the text is delivered — spliced at the caret of an
 * open editor, or appended to a note read from disk. That is what makes "the
 * same result from anywhere" a property of the code rather than a promise in a
 * comment.
 *
 * A path offered twice in one gesture is written once: the second occurrence is
 * a duplicate of the first by the time it is reached, which is the same rule
 * applied to the same body, not a special case.
 */
export function planAttachments(body: string, relativePaths: readonly string[]): AttachmentPlan {
  const held = embeddedAttachmentNames(body);
  const inserted: string[] = [];
  const alreadyThere: string[] = [];
  const unnameable: string[] = [];

  for (const path of relativePaths) {
    if (!wikilinkNameable(path)) {
      unnameable.push(path);
      continue;
    }
    const key = attachmentName(path).toLowerCase();
    if (held.has(key)) {
      alreadyThere.push(path);
      continue;
    }
    held.add(key);
    inserted.push(path);
  }

  return {
    text: inserted.map(attachmentEmbed).join(EMBED_SEPARATOR),
    inserted,
    alreadyThere,
    unnameable,
    refusal: refusalSentence(alreadyThere, unnameable),
  };
}

/**
 * The body a note gets when the insert is an append rather than a caret splice
 * — the Files-pane entry point, where the note is not open and has no caret.
 *
 * Only ever ADDS a separator, never a terminator. That is what keeps the three
 * entry points byte-identical: a caret at the end of a body ending in a newline
 * produces exactly this, and appending a trailing newline here would make the
 * closed-note path write a byte the open-note path does not.
 */
export function bodyWithAttachments(body: string, plan: AttachmentPlan): string {
  if (plan.text === "") {
    return body;
  }
  if (body === "" || body.endsWith("\n")) {
    return body + plan.text;
  }
  return `${body}\n${plan.text}`;
}

/** `a`, `a and b`, `a, b and c` — the way a person lists things out loud. */
function nameList(paths: readonly string[]): string {
  const names = paths.map(attachmentName);
  if (names.length <= 1) {
    return names.join("");
  }
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/**
 * What the surface says about what it declined.
 *
 * Two clauses because they are two different facts about two different files,
 * and a person who selected six and got four needs to know which two and why.
 */
function refusalSentence(
  alreadyThere: readonly string[],
  unnameable: readonly string[],
): string | null {
  const clauses: string[] = [];
  if (alreadyThere.length > 0) {
    const plural = alreadyThere.length > 1;
    clauses.push(
      `${nameList(alreadyThere)} ${plural ? "are" : "is"} already in this note, so keeper left ${
        plural ? "them" : "it"
      } out.`,
    );
  }
  if (unnameable.length > 0) {
    const plural = unnameable.length > 1;
    clauses.push(
      `${nameList(unnameable)} ${plural ? "have names" : "has a name"} an embed cannot spell — a ` +
        `[, ], |, # or line break ends the link — so keeper left ${plural ? "them" : "it"} out. ` +
        `Renaming the file is the fix.`,
    );
  }
  return clauses.length === 0 ? null : clauses.join(" ");
}

// ---------------------------------------------------------------------------
// The link scanner: a mirror of keeper_core::notes::links::extract, restricted
// to what the duplicate rule needs.
//
// Line for line with the Rust rather than idiomatic TypeScript, on purpose.
// This is the half of the module the shared vector table pins, and a reader
// checking one against the other should be able to do it by eye.
// ---------------------------------------------------------------------------

/** One link exactly as the author wrote it. */
interface RawLink {
  /** The path or title being pointed at, anchor removed, decoded. */
  target: string;
  /** `![[…]]` or `![…](…)`: show the target rather than link to it. */
  embed: boolean;
  /** Where the whole link syntax ends, `!` included. */
  end: number;
}

/** Every link in a body, in document order. */
function extractLinks(body: string): RawLink[] {
  const code = codeSpans(body);
  const out: RawLink[] = [];
  let at = 0;

  while (at < body.length) {
    let start: number;
    let embed: boolean;
    if (body[at] === "!" && at + 1 < body.length && body[at + 1] === "[") {
      start = at + 1;
      embed = true;
    } else if (body[at] === "[") {
      start = at;
      embed = false;
    } else {
      at += 1;
      continue;
    }

    if (inCode(code, at) || isEscaped(body, at)) {
      at += 1;
      continue;
    }

    const parsed = body.startsWith("[[", start)
      ? wikilink(body, start, embed)
      : markdownLink(body, start, embed);

    if (parsed === null) {
      at += 1;
    } else {
      at = parsed.end;
      out.push(parsed);
    }
  }

  return out;
}

/** Whether the character at `at` is preceded by an odd number of backslashes. */
function isEscaped(body: string, at: number): boolean {
  let run = 0;
  while (at - run - 1 >= 0 && body[at - run - 1] === "\\") {
    run += 1;
  }
  return run % 2 === 1;
}

/** End of the line containing `at`, which is as far as a link may reach. */
function lineLimit(body: string, at: number): number {
  const nl = body.indexOf("\n", Math.min(at, body.length));
  return nl === -1 ? body.length : nl;
}

/** `[[target]]`, `[[target|alias]]`. `start` is the first `[`. */
function wikilink(body: string, start: number, embed: boolean): RawLink | null {
  const innerStart = start + 2;
  const limit = lineLimit(body, innerStart);
  const close = body.slice(innerStart, limit).indexOf("]]");
  if (close === -1) {
    return null;
  }
  const inner = body.slice(innerStart, innerStart + close);
  const bar = inner.indexOf("|");
  const targetRaw = bar === -1 ? inner : inner.slice(0, bar);
  const target = stripAnchor(targetRaw).trim();
  if (target === "") {
    // `[[#heading]]` points inside this very note; there is no file named.
    return null;
  }
  return { target, embed, end: innerStart + close + 2 };
}

/** `[text](target)`, `![alt](target)`. */
function markdownLink(body: string, start: number, embed: boolean): RawLink | null {
  const limit = lineLimit(body, start);
  const bracket = body.slice(start + 1, limit).indexOf("]");
  if (bracket === -1) {
    return null;
  }
  const textEnd = start + 1 + bracket;
  if (body[textEnd + 1] !== "(") {
    return null;
  }

  const destStart = textEnd + 2;
  const destEnd = closingParen(body, destStart, lineLimit(body, destStart));
  if (destEnd === null) {
    return null;
  }
  let dest = body.slice(destStart, destEnd).trim();

  // `(path "Title")` — the optional title is not part of the destination. The
  // quote must follow whitespace, or `it's notes.md` would lose its tail.
  const quote = dest.search(/["']/);
  if (quote > 0 && /\s$/.test(dest.slice(0, quote))) {
    dest = dest.slice(0, quote).trimEnd();
  }
  // `(<path with spaces>)`
  dest = trimEdges(dest, "<", ">");

  if (dest === "" || isExternal(dest)) {
    return null;
  }
  const anchored = stripAnchor(dest);
  if (anchored === "") {
    return null;
  }
  // Obsidian percent-encodes spaces and non-ASCII in markdown-style links, so
  // the raw target has to be decoded before it can match a path on disk. A
  // destination that is not valid percent-encoding is taken literally, which is
  // what Rust's lossy decode does with it.
  return { target: decodeLossy(anchored), embed, end: destEnd + 1 };
}

/**
 * Match the `)` that closes a destination, tolerating one level of balanced
 * parentheses — Wikipedia paths are full of them.
 */
function closingParen(body: string, from: number, limit: number): number | null {
  let depth = 0;
  for (let i = from; i < limit; i += 1) {
    const c = body[i];
    if (c === "(") {
      depth += 1;
    } else if (c === ")") {
      if (depth === 0) {
        return i;
      }
      depth -= 1;
    }
  }
  return null;
}

/** Drop a `#heading` or `#^block` anchor from a target. */
function stripAnchor(target: string): string {
  const hash = target.indexOf("#");
  return hash === -1 ? target : target.slice(0, hash);
}

/** Strip every leading `lead` and every trailing `tail`, as Rust's
 *  `trim_start_matches` / `trim_end_matches` do. */
function trimEdges(text: string, lead: string, tail: string): string {
  let from = 0;
  let to = text.length;
  while (from < to && text[from] === lead) {
    from += 1;
  }
  while (to > from && text[to - 1] === tail) {
    to -= 1;
  }
  return text.slice(from, to);
}

/**
 * Whether a markdown destination points outside the vault. Anything with a URL
 * scheme does, and so does a bare in-page anchor.
 */
function isExternal(dest: string): boolean {
  if (dest.startsWith("#")) {
    return true;
  }
  const colon = dest.indexOf(":");
  return colon > 0 && /^[A-Za-z0-9+\-.]+$/.test(dest.slice(0, colon));
}

/** Percent-decode, falling back to the raw text — `decodeURIComponent` throws
 *  on a stray `%`, where Rust's `decode_utf8_lossy` does not. */
function decodeLossy(target: string): string {
  try {
    return decodeURIComponent(target);
  } catch {
    return target;
  }
}

/**
 * The byte ranges of `text` that are code, sorted and disjoint.
 *
 * Mirrors `keeper_core::notes::tags::code_spans`. A wikilink inside a fenced
 * block is documentation about wikilinks, not a use of one, and a note that
 * explains the syntax must not become a note keeper refuses to attach to.
 */
function codeSpans(text: string): [number, number][] {
  const spans: [number, number][] = [];
  let fence: [string, number] | null = null;
  let at = 0;

  for (;;) {
    const bounds = lineBounds(text, at);
    if (bounds === null) {
      break;
    }
    const [ls, le, next] = bounds;
    const line = text.slice(ls, le);
    const trimmed = line.trimStart();
    const indent = line.length - trimmed.length;
    const run = fenceRun(trimmed);

    if (fence !== null) {
      claim(spans, ls, next);
      if (
        run !== null &&
        run[0] === fence[0] &&
        run[1] >= fence[1] &&
        trimmed.slice(run[1]).trim() === ""
      ) {
        fence = null;
      }
    } else if (run !== null && indent < 4) {
      // Four spaces of indent is itself a code block opener in markdown, so a
      // "fence" that deep is literal text.
      claim(spans, ls, next);
      fence = run;
    } else {
      scanInlineCode(line, ls, spans);
    }

    at = next;
  }

  return spans;
}

/** Whether `at` falls inside one of the sorted, disjoint code spans. */
function inCode(spans: readonly [number, number][], at: number): boolean {
  let lo = 0;
  let hi = spans.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (spans[mid][0] <= at) {
      lo = mid + 1;
    } else {
      hi = mid;
    }
  }
  return lo > 0 && spans[lo - 1][1] > at;
}

/** Start, end-without-terminator and next-line-start, or `null` past the end. */
function lineBounds(s: string, at: number): [number, number, number] | null {
  if (at >= s.length) {
    return null;
  }
  const nl = s.indexOf("\n", at);
  if (nl === -1) {
    return [at, s.length, s.length];
  }
  let end = nl;
  if (end > at && s[end - 1] === "\r") {
    end -= 1;
  }
  return [at, end, nl + 1];
}

/** The opening run of a fence line: three or more backticks or tildes. */
function fenceRun(trimmed: string): [string, number] | null {
  const marker = trimmed[0];
  if (marker !== "`" && marker !== "~") {
    return null;
  }
  let len = 0;
  while (len < trimmed.length && trimmed[len] === marker) {
    len += 1;
  }
  return len >= 3 ? [marker, len] : null;
}

/** Append a span, merging it into the previous one when they touch. Fenced
 *  blocks arrive a line at a time and must come out as one range. */
function claim(spans: [number, number][], start: number, end: number): void {
  const last = spans[spans.length - 1];
  if (last !== undefined && last[1] >= start) {
    last[1] = end;
  } else {
    spans.push([start, end]);
  }
}

/** Inline `` `code` `` runs on one line. */
function scanInlineCode(line: string, base: number, spans: [number, number][]): void {
  let i = 0;
  while (i < line.length) {
    if (line[i] !== "`") {
      i += 1;
      continue;
    }
    let open = 0;
    while (i + open < line.length && line[i + open] === "`") {
      open += 1;
    }

    // A span closes on a backtick run of exactly the same length.
    let j = i + open;
    let closed: number | null = null;
    while (j < line.length) {
      if (line[j] === "`") {
        let run = 0;
        while (j + run < line.length && line[j + run] === "`") {
          run += 1;
        }
        if (run === open) {
          closed = j + run;
          break;
        }
        j += run;
      } else {
        j += 1;
      }
    }

    if (closed === null) {
      // An unmatched run is literal text, not the start of code.
      i += open;
    } else {
      claim(spans, base + i, base + closed);
      i = closed;
    }
  }
}
