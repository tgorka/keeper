/**
 * Frontmatter as typed controls (Story 37.6, FR-107, FR-121).
 *
 * The panel is a **lens over the raw block, never its owner.** That distinction
 * is the whole design. keeper's promise is that it does not mangle files it did
 * not author, which rules out the obvious implementation — parse the block into
 * a map, edit the map, serialise it back — because a round trip through a map
 * loses key order, quoting style, comments and block-versus-flow list form on
 * every single write.
 *
 * So editing one property splices exactly that property's value span back into
 * the block the user already has, and every other byte in it is carried through
 * untouched. An unknown key keeper has never heard of survives a hundred edits to
 * the keys beside it.
 *
 * The panel is also the **only** surface that rewrites the block: it is not in the
 * editor buffer, so the body and the block are written together and neither can
 * overwrite the other. Offsets here are into the block, which is why the block
 * arrives on its own rather than as the head of a document.
 *
 * The reader below deliberately understands only the Obsidian property subset —
 * scalars, block lists, flow lists — which is the same subset
 * `keeper_core::notes::frontmatter` parses. A block it cannot read is not an
 * error and not hidden: it renders raw, with a line saying so, because the
 * note is the user's and the panel is ours.
 *
 * One family of keys is read rather than edited: the `session:`, `recording:`
 * and `files:` a recording note carries (Story 42.4). Those are keeper's own
 * record of where a recording's bytes landed, written in relative form because
 * FR-145 keeps absolute paths out of a synced file — which leaves the reader
 * holding text that names a file and cannot open it. So each of those paths
 * gets one dropdown of actions whose targets Rust composed (AD-65), and the
 * text beside it stays exactly the relative path the note says.
 *
 * The two columns are sized by AD-83's order (Story 44.12): the key column fits
 * its own keys, the user may drag the seam between them, and only what still
 * does not fit truncates — with the whole value one click away. What it
 * replaces was a `w-32` guess beside a `title=` tooltip, which is a cut value
 * with nowhere a keyboard could read the rest.
 *
 * `tags:` is the one key that gets a control of its own (Story 44.14). It is
 * the user's, in the user's note, including on the notes keeper writes — but a
 * tag belongs to a vocabulary, so the row asks the same chooser every other
 * tag surface asks rather than the generic list's bare text box. Two things it
 * will not do, for two different reasons, and it says which: `session:` is
 * read-only beside `id:` because everything about a recording resolves through
 * it, and the `recordings` tag is refused with a sentence because it is what
 * makes the note findable as a recording at all.
 */
import { Copy, FolderOpen, MoreHorizontal, Play, Plus, Video } from "lucide-react";
import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { TagCombobox } from "@/components/notes/tag-combobox";
import { namesTag } from "@/components/tags/tag-match";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { FullValueButton, OverflowValue } from "@/components/ui/overflow-value";
import {
  COLUMN_GRID_CLASS,
  ColumnResizer,
  useResizableColumn,
} from "@/components/ui/resizable-columns";
import { Switch } from "@/components/ui/switch";
import {
  type NoteWriteVm,
  notesSave,
  type RecordingNoteTargetVm,
  recordingNoteTargets,
  recordingOpenPath,
  recordingSessionMeta,
  revealPath,
  tagsVocabulary,
} from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { recordingMetaStore } from "@/lib/stores/recording-meta";
import { truncateGraphemes } from "@/lib/truncate";
import { cn } from "@/lib/utils";

/** Which control the value's shape implies. */
export type PropertyKind = "text" | "number" | "boolean" | "date" | "list";

/** How the value was written, so a rewrite keeps the user's style. */
export type PropertyStyle = "scalar" | "flow" | "block";

export interface PropertyEntry {
  key: string;
  kind: PropertyKind;
  style: PropertyStyle;
  /** The scalar's text, with surrounding quotes stripped. Empty for a list. */
  text: string;
  /** The list's items. Empty for a scalar. */
  items: string[];
  /** Whether the original scalar was double-quoted. */
  quoted: boolean;
  /** Byte offset immediately after the key's colon. */
  valueFrom: number;
  /** Byte offset at the end of the value (the last block-list line, or EOL). */
  valueTo: number;
  /**
   * The value carries an indented map (the reserved `keeper:` key is the one
   * shape that has them). Rendered read-only: a typed control over a nested
   * map would either flatten it or invent a shape the parser does not have.
   */
  nested: boolean;
}

export interface ParsedFrontmatter {
  /** The `---` fenced block's span, or null when the note has no frontmatter. */
  block: { from: number; to: number } | null;
  entries: PropertyEntry[];
  /** True when a block exists but holds something this reader will not touch. */
  unparsed: boolean;
}

const KEY_LINE = /^([A-Za-z0-9_][A-Za-z0-9_.\- ]*):[ \t]*(.*)$/;
const LIST_ITEM = /^([ \t]*)-[ \t]+(.*)$/;
const NUMBER = /^-?\d+(?:\.\d+)?$/;
const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;

/** Strip one layer of matching quotes, if the value has them. */
function unquote(raw: string): { text: string; quoted: boolean } {
  if (raw.length >= 2 && raw.startsWith('"') && raw.endsWith('"')) {
    return { text: raw.slice(1, -1), quoted: true };
  }
  if (raw.length >= 2 && raw.startsWith("'") && raw.endsWith("'")) {
    return { text: raw.slice(1, -1), quoted: false };
  }
  return { text: raw, quoted: false };
}

/**
 * Read the leading `---` block.
 *
 * Takes either a block on its own — which is what the body channel delivers — or a
 * whole document, because a leading block is a leading block either way.
 *
 * Offsets, not values, are the output that matters: they are what lets a write
 * touch one span and leave the rest of the block alone.
 */
export function readFrontmatter(source: string): ParsedFrontmatter {
  const empty: ParsedFrontmatter = { block: null, entries: [], unparsed: false };
  if (!source.startsWith("---\n")) {
    return empty;
  }
  const closing = source.indexOf("\n---", 3);
  if (closing < 0) {
    return { block: null, entries: [], unparsed: true };
  }
  const inner = source.slice(4, closing + 1);
  const entries: PropertyEntry[] = [];
  let unparsed = false;
  let offset = 4;

  for (const line of inner.split("\n")) {
    const lineFrom = offset;
    offset += line.length + 1;
    if (line.trim() === "" || line.trimStart().startsWith("#")) {
      continue;
    }

    const item = LIST_ITEM.exec(line);
    if (item) {
      const owner = entries.length > 0 ? entries[entries.length - 1] : undefined;
      // A dash with no key above it, or under a scalar, is not the subset.
      if (owner === undefined || owner.style === "flow" || owner.text !== "") {
        unparsed = true;
        continue;
      }
      owner.style = "block";
      owner.kind = "list";
      owner.items.push(unquote(item[2].trim()).text);
      owner.valueTo = lineFrom + line.length;
      continue;
    }

    // An indented `key: value` belongs to the entry above it — the one level of
    // nesting the property subset allows, under the reserved `keeper:` key.
    if (/^[ \t]+[A-Za-z0-9_]/.test(line)) {
      const owner = entries.length > 0 ? entries[entries.length - 1] : undefined;
      if (owner === undefined || owner.text !== "") {
        unparsed = true;
        continue;
      }
      owner.nested = true;
      owner.valueTo = lineFrom + line.length;
      continue;
    }

    const keyed = KEY_LINE.exec(line);
    if (!keyed) {
      unparsed = true;
      continue;
    }
    const rawValue = keyed[2].trim();
    // Anchors, aliases, tags and block scalars are outside the Obsidian
    // property subset. The reader refuses them rather than reinterpreting
    // them, which is what keeps the panel from inventing a value (AD-55).
    if (/^[!&*|>]/.test(rawValue)) {
      unparsed = true;
      continue;
    }
    const valueFrom = lineFrom + keyed[1].length + 1;
    const valueTo = lineFrom + line.length;
    if (rawValue.startsWith("[") && rawValue.endsWith("]")) {
      entries.push({
        key: keyed[1],
        kind: "list",
        style: "flow",
        text: "",
        items: rawValue
          .slice(1, -1)
          .split(",")
          .map((part) => unquote(part.trim()).text)
          .filter((part) => part !== ""),
        quoted: false,
        valueFrom,
        valueTo,
        nested: false,
      });
      continue;
    }
    const { text, quoted } = unquote(rawValue);
    let kind: PropertyKind = "text";
    if (text === "true" || text === "false") {
      kind = "boolean";
    } else if (NUMBER.test(text)) {
      kind = "number";
    } else if (ISO_DATE.test(text)) {
      kind = "date";
    }
    entries.push({
      key: keyed[1],
      kind,
      style: "scalar",
      text,
      items: [],
      quoted,
      valueFrom,
      valueTo,
      nested: false,
    });
  }

  return { block: { from: 0, to: closing + 4 }, entries, unparsed };
}

/** Render a scalar the way it was written, quoting only when it must be. */
function serialiseScalar(value: string, quoted: boolean): string {
  const needsQuotes =
    quoted || value === "" || value.trim() !== value || /[:#[\]{}]/.test(value.charAt(0));
  return needsQuotes ? ` "${value.replace(/"/g, '\\"')}"` : ` ${value}`;
}

/** Render a list in the style the user already used. */
function serialiseList(items: string[], style: PropertyStyle): string {
  if (style === "block") {
    return items.length === 0 ? " []" : `\n${items.map((item) => `  - ${item}`).join("\n")}`;
  }
  return ` [${items.join(", ")}]`;
}

/**
 * Splice one property's value into the document.
 *
 * Returns the whole new document. Every byte outside `[valueFrom, valueTo)` is
 * the caller's original — that is the FR-121 guarantee, expressed as code
 * rather than as a promise.
 */
export function spliceProperty(source: string, entry: PropertyEntry, value: string): string {
  return source.slice(0, entry.valueFrom) + value + source.slice(entry.valueTo);
}

/** Add a property, creating the block when the note has none. */
export function addProperty(source: string, key: string, value: string): string {
  const parsed = readFrontmatter(source);
  if (parsed.block === null) {
    return `---\n${key}:${value}\n---\n${source}`;
  }
  // Immediately before the closing fence, so the user's ordering is preserved
  // and a new key never jumps above one they put first.
  const closing = parsed.block.to - 4;
  return `${source.slice(0, closing)}\n${key}:${value}${source.slice(closing)}`;
}

/** The remembered-width id of the key column. Stable across releases: change
 * it and everyone's dragged width silently reverts to fitted. */
export const PROPERTY_KEY_COLUMN = "properties-key";

/** What the seam between the two columns sizes, for its accessible name. */
export const PROPERTIES_COLUMN_LABEL = "the property name column";

/**
 * The panel's accessible name, and the word on the control that opens it —
 * spelled once, like `ATTACHMENTS_LABEL`, because Story 46.5 moved that control
 * into the note's Actions menu and a menu item and a `<section>` that disagreed
 * would be two names for one thing.
 */
export const PROPERTIES_LABEL = "Properties";

/** What the key column holds, named for the panel that opens a long key. */
export const PROPERTY_NAME_LABEL = "Property name";

/** What the unparsed-block preview's affordance opens. */
export const UNPARSED_BLOCK_LABEL = "Properties block";

/**
 * How much of a block keeper will not parse is previewed inline, in
 * user-perceived characters.
 *
 * Counted in graphemes rather than UTF-16 units (Story 44.12): the previous
 * `slice(0, 400)` cut wherever the 400th code unit fell, which in a note with
 * an emoji or a combining mark is the middle of a character, and the preview
 * then showed a replacement glyph that is in nobody's file.
 */
export const UNPARSED_PREVIEW_GRAPHEMES = 400;

export interface PropertiesPanelProps {
  /** The note's frontmatter block, verbatim, or `""` when it has none. */
  frontmatter: string;
  /**
   * The editor's buffer.
   *
   * A property write is a write of the whole note, so the body goes with it —
   * sending the block alone would drop whatever the user has typed since the last
   * autosave.
   */
  body: string;
  /** The body subscription every write goes through. Null disables editing. */
  subscriptionId: string | null;
  /** The revision the buffer opened at. */
  baseRev: string;
  /** Adopt the write once Rust has acknowledged it. */
  onSaved: (body: string, write: NoteWriteVm) => void;
}

export function PropertiesPanel({
  frontmatter,
  body,
  subscriptionId,
  baseRev,
  onSaved,
}: PropertiesPanelProps) {
  const parsed = readFrontmatter(frontmatter);
  const [newKey, setNewKey] = useState("");
  const [failure, setFailure] = useState<string | null>(null);
  const canReveal = useCapabilitiesStore((state) => state.capabilities.revealInFileManager);
  const sessionId = recordingSessionId(parsed);
  const [targets, setTargets] = useState<RecordingNoteTargetVm[] | null>(null);
  const keyColumn = useResizableColumn(PROPERTY_KEY_COLUMN, PROPERTIES_COLUMN_LABEL);
  // The note's `tags:` row, whatever shape it was written in, or null for a
  // note that has none — a block list, a flow list, a bare `tags: standup`
  // scalar, or an empty value. Story 44.14 admitted only the lists and the
  // empty one, so a note whose single tag had been written inline got the
  // generic text box: no vocabulary, no chips, and the exact surface the
  // chooser exists to replace.
  //
  // `nested` is refused because an indented map under `tags:` is not a tag
  // list, and the panel renders those read-only everywhere else too. The FIRST
  // match wins, and identity is what the grid compares below: a hand-edited
  // block can carry `tags:` twice, and two rows both claiming to be the tag row
  // would write to two spans, the second landing on offsets the first moved.
  const tagsEntry = parsed.entries.find((entry) => entry.key === TAGS_KEY && !entry.nested) ?? null;

  // Resolved by session id, once per note: the id is the handle that survives
  // a Story 40.4 retitle, so this answers "where is this recording NOW" while
  // the note goes on saying where it was when the stub was written. A failure
  // and an unknown session are the same state — no targets — because the
  // surface's answer to both is the note's own text and no dead affordance.
  useEffect(() => {
    if (sessionId === null) {
      setTargets(null);
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

  const write = (nextBlock: string): void => {
    if (subscriptionId === null) {
      return;
    }
    void notesSave(subscriptionId, body, baseRev, nextBlock)
      .then((result) => {
        setFailure(null);
        onSaved(body, result);
      })
      .catch(() => {
        // Said out loud, because the control has already moved and a silent
        // failure would leave the panel showing a value the file does not have.
        setFailure("keeper couldn't write that property. The note is unchanged on disk.");
      });
  };

  if (parsed.unparsed) {
    const preview = truncateGraphemes(frontmatter, UNPARSED_PREVIEW_GRAPHEMES);
    return (
      <section aria-label={PROPERTIES_LABEL} className="border-b px-3 py-2 text-xs">
        <p className="text-muted-foreground">
          This note's properties can't be parsed, so keeper is showing them exactly as they are on
          disk rather than rewriting them.
        </p>
        <div className="mt-1 flex items-start gap-1">
          <pre className="min-w-0 flex-1 overflow-x-auto font-mono text-[11px]">{preview}</pre>
          {preview !== frontmatter && (
            <FullValueButton name={UNPARSED_BLOCK_LABEL} value={frontmatter} monospace />
          )}
        </div>
      </section>
    );
  }

  return (
    <section
      aria-label={PROPERTIES_LABEL}
      className="flex flex-col gap-1 border-b px-3 py-2 text-xs"
    >
      {/* Always rendered, because the tag row is always here (Story 45.17). A
          note with no frontmatter at all is the commonest note there is, and it
          is exactly the one whose tags a person wants to set. */}
      <div
        ref={keyColumn.containerRef}
        className={cn(COLUMN_GRID_CLASS, "items-start gap-y-1")}
        style={keyColumn.gridStyle(parsed.entries.length + (tagsEntry === null ? 1 : 0))}
      >
        {parsed.entries.map((entry) => {
          // A recording note's own two path keys get actions; a `files:` key in
          // somebody else's note is somebody else's list, and stays a control.
          const isRecordingPathKey = entry.key === RECORDING_KEY || entry.key === FILES_KEY;
          const showsRecordingPaths = sessionId !== null && isRecordingPathKey && !entry.nested;
          return (
            <Fragment key={entry.key}>
              <div className="col-start-1 min-w-0 pr-2 text-muted-foreground">
                <OverflowValue name={PROPERTY_NAME_LABEL} value={entry.key} />
              </div>
              <div className="col-start-3 flex min-w-0 items-start gap-2 pl-2">
                {showsRecordingPaths ? (
                  <RecordingPaths entry={entry} targets={targets} canReveal={canReveal} />
                ) : entry === tagsEntry ? (
                  <TagsProperty
                    entry={entry}
                    sessionId={sessionId}
                    onChange={(tags) =>
                      write(spliceProperty(frontmatter, entry, serialiseTags(tags, entry)))
                    }
                  />
                ) : (
                  <PropertyControl
                    entry={entry}
                    onChange={(value) => write(spliceProperty(frontmatter, entry, value))}
                  />
                )}
              </div>
            </Fragment>
          );
        })}
        {/* The note has no `tags:` key. The row is offered anyway, and writes
            the key on the first tag — a person editing tags should not have to
            know that "tags" is the name of a frontmatter field, type it into
            the generic Add-a-property box and only then get the chooser. */}
        {tagsEntry === null && (
          <Fragment key={TAGS_KEY}>
            <div className="col-start-1 min-w-0 pr-2 text-muted-foreground">
              <OverflowValue name={PROPERTY_NAME_LABEL} value={TAGS_KEY} />
            </div>
            <div className="col-start-3 flex min-w-0 items-start gap-2 pl-2">
              <TagsProperty
                entry={null}
                sessionId={sessionId}
                onChange={(tags) =>
                  write(addProperty(frontmatter, TAGS_KEY, serialiseList(tags, NEW_TAGS_STYLE)))
                }
              />
            </div>
          </Fragment>
        )}
        <ColumnResizer {...keyColumn.resizerProps} />
      </div>
      {/* "Record another like this" (Story 45.19, FR-197) — a note-level
          action, not a property row: it is about the whole session, and the
          block has no key it belongs under. Present only for a recording note
          whose folder resolved, so it can never offer to open a form over a
          session that is not on this machine. */}
      {sessionId !== null && <RecordAnotherLikeThis targets={targets} />}
      <div className="flex items-center gap-2 pt-1">
        <Input
          value={newKey}
          onChange={(event) => setNewKey(event.target.value)}
          placeholder="Add a property"
          aria-label="New property name"
          className="h-7 w-32 text-xs"
        />
        <Button
          size="sm"
          variant="ghost"
          disabled={newKey.trim() === ""}
          onClick={() => {
            write(addProperty(frontmatter, newKey.trim(), ' ""'));
            setNewKey("");
          }}
        >
          Add
        </Button>
      </div>
      {failure === null ? null : (
        <p role="alert" className="text-destructive">
          {failure}
        </p>
      )}
    </section>
  );
}

interface PropertyControlProps {
  entry: PropertyEntry;
  /** Receives the serialised value, ready to splice after the key's colon. */
  onChange: (value: string) => void;
}

/** Which HTML input a scalar kind wants. Lists and booleans never reach here. */
const INPUT_TYPES: Record<PropertyKind, string> = {
  text: "text",
  number: "number",
  date: "date",
  boolean: "text",
  list: "text",
};

function PropertyControl({ entry, onChange }: PropertyControlProps) {
  // The ULID is identity, not metadata: links resolve through it (FR-97). So
  // is `session:` — 42.6's targets, 43.2's `is:recording` predicate and the
  // Recordings space every one resolve through it, and not one of them is
  // visible from this panel, so a typo here would break a note in a way its
  // author could not see from where they made it. Both are shown and copyable
  // and neither is editable, which is one rule for keeper's two identity keys
  // rather than two rules that can drift.
  //
  // Being read-only is exactly what makes them need the overflow affordance:
  // an editable value can be scrolled with the caret, and these cannot be
  // scrolled at all.
  if (entry.key === "id" || entry.key === SESSION_KEY) {
    return (
      <div className="min-w-0 flex-1 text-[11px] text-muted-foreground">
        <OverflowValue name={entry.key} value={entry.text} monospace />
      </div>
    );
  }

  if (entry.nested) {
    return <span className="text-muted-foreground">nested value — edit it in the note</span>;
  }

  if (entry.kind === "boolean") {
    return (
      <Switch
        aria-label={entry.key}
        checked={entry.text === "true"}
        onCheckedChange={(checked) => onChange(` ${checked ? "true" : "false"}`)}
      />
    );
  }

  if (entry.kind === "list") {
    return (
      <div className="flex flex-1 flex-wrap items-center gap-1">
        {entry.items.map((item) => (
          <button
            key={item}
            type="button"
            className="rounded bg-muted px-1.5 py-0.5"
            aria-label={`Remove ${item} from ${entry.key}`}
            onClick={() =>
              onChange(
                serialiseList(
                  entry.items.filter((candidate) => candidate !== item),
                  entry.style,
                ),
              )
            }
          >
            {item} ×
          </button>
        ))}
        <Input
          aria-label={`Add to ${entry.key}`}
          placeholder="add"
          className="h-6 w-24 text-xs"
          onKeyDown={(event) => {
            const value = event.currentTarget.value.trim();
            if (event.key === "Enter" && value !== "") {
              event.preventDefault();
              event.currentTarget.value = "";
              onChange(serialiseList([...entry.items, value], entry.style));
            }
          }}
        />
      </div>
    );
  }

  const inputType = INPUT_TYPES[entry.kind];
  return (
    <Input
      aria-label={entry.key}
      type={inputType}
      defaultValue={entry.text}
      className="h-7 flex-1 text-xs"
      // On blur, not on keystroke: every edit here is a file write, and a write
      // per character would be a commit per character on the next cadence tick.
      onBlur={(event) => {
        if (event.target.value !== entry.text) {
          onChange(serialiseScalar(event.target.value, entry.quoted));
        }
      }}
    />
  );
}

/**
 * The frontmatter key holding a note's tags.
 *
 * Singled out of the generic list control because a tag is not an arbitrary
 * string. The vault has a vocabulary of them (Story 42.5) and a bare text box
 * beside it is how `Standup` and `standup` become two tags nobody meant.
 */
const TAGS_KEY = "tags";

/**
 * The style a `tags:` key keeper writes for the first time is written in.
 *
 * A block list, because that is what Obsidian's own tag control writes and what
 * the overwhelming majority of vault notes already carry — a note keeper gave a
 * flow list to would be the odd one out in its own folder. It applies only to a
 * key that did not exist: an existing `tags:` keeps whatever style the file
 * already used, which is [`serialiseTags`]'s whole job.
 */
const NEW_TAGS_STYLE: PropertyStyle = "block";

/**
 * The tags an entry holds, whatever shape the file wrote them in.
 *
 * A scalar is ONE tag, not zero: `tags: standup` means the note is tagged
 * `standup`, and reading it as an empty list would have shown a person their
 * own tag missing from the row that claims to list their tags — and then
 * silently dropped it on the next edit.
 */
export function tagsOf(entry: PropertyEntry): string[] {
  if (entry.kind === "list") {
    return entry.items;
  }
  const only = entry.text.trim();
  return only === "" ? [] : [only];
}

/**
 * Render a tag list back into the value span, keeping the file's own style.
 *
 * The one case the style cannot survive is a scalar that has to hold two tags,
 * and it becomes a FLOW list rather than a block one: the value was written on
 * the key's own line, and a flow list is still on the key's own line. Promoting
 * `tags: standup` to three lines because somebody added a second tag is a
 * bigger edit to their file than the edit they asked for.
 *
 * Going the other way it stays a list. Removing the second of two tags leaves
 * `[standup]` rather than reverting to `standup`, because a value that changes
 * shape in both directions makes every removal a structural rewrite, and the
 * note is the user's.
 */
export function serialiseTags(tags: string[], entry: PropertyEntry): string {
  if (entry.kind === "list") {
    return serialiseList(tags, entry.style);
  }
  // A scalar holds at most one tag, so from here the count is 0 — the last tag
  // came off — or two or more, because one was added. It is never 1: nothing
  // can rewrite a one-tag scalar into a different one-tag scalar. So there is
  // no "still exactly one" arm and no `tags[0] ?? ""`; both were a branch and a
  // fabricated value for a case no input reaches, and a mutation to the
  // boundary between them survived the whole suite because nothing could tell
  // the two apart.
  return tags.length === 0 ? serialiseScalar("", entry.quoted) : serialiseList(tags, "flow");
}

/**
 * keeper's classification tag (Story 43.2, FR-147): what a *person* browsing
 * the tag tree sees to know this note is a recording, since [`SESSION_KEY`] is
 * a machine predicate and invisible there.
 *
 * Spelled here as well as in `keeper_core::notes::recording_note::RECORDINGS_TAG`,
 * which stays the authority. This surface only ever asks whether a tag on
 * screen NAMES that tag, and it asks `namesTag` — so every spelling Rust folds
 * onto it (`Recordings`, `RECORDINGS`, `recordings `) is protected here too.
 * Comparing with `===` would have protected exactly one of them and left the
 * other three looking identical on screen and removable.
 */
export const RECORDINGS_TAG = "recordings";

/** The tag row's chooser, named on both the toggle and the field it opens. */
export const ADD_NOTE_TAG = "Add a tag";

/**
 * Why keeper will not take `recordings` off a recording note.
 *
 * A sentence, not a disabled `×`. The tag sits in a row of chips that all come
 * off with one press, so removing only this one's affordance would be a
 * control that does nothing and says nothing — and the consequence is
 * invisible from this panel: the note would vanish from the Recordings space
 * and from the tag tree while still being on disk, which reads as data loss.
 */
export function recordingsTagRefusal(tag: string): string {
  return (
    `"${tag}" is how keeper marks this note as a recording — it is what puts the note in the ` +
    "Recordings space and in the tag tree, so keeper kept it. Every other tag here is yours."
  );
}

interface TagsPropertyProps {
  /**
   * The note's `tags:` entry, or **null for a note that has none yet** — which
   * is most notes, and the case Story 44.14 could not serve at all.
   */
  entry: PropertyEntry | null;
  /** The note's session id, or null when it is not a recording note. */
  sessionId: string | null;
  /**
   * Receives the note's whole tag list, in order.
   *
   * The LIST and not the serialised text, because the two callers write it
   * differently — one splices an existing span keeping the file's style, the
   * other creates the key — and a component that had to know which was which
   * would be a component that knows about frontmatter offsets.
   */
  onChange: (tags: string[]) => void;
}

/**
 * The tag row (Story 44.14, FR-170).
 *
 * These are the USER's tags in the user's note. What stood here was the
 * generic list control's bare text box: no vocabulary, so a second casing of a
 * tag the vault already had silently became a second tag, and nothing on
 * screen said which of them keeper depends on. The chips and the chooser are
 * the two halves Story 44.13 settled — a field you can type into over a list
 * you can browse — reading the one vocabulary Story 42.5 owns.
 *
 * The vocabulary is read when the chooser opens, not when the panel mounts:
 * the panel is on screen for as long as someone is reading a note and the
 * vocabulary is wanted for the few seconds they are picking from it.
 *
 * Exactly one tag is refused, and only on a recording note — see
 * [`recordingsTagRefusal`]. Everything else, including a tag keeper itself
 * wrote into the stub, comes off with one press.
 */
function TagsProperty({ entry, sessionId, onChange }: TagsPropertyProps) {
  // Read once per render, because every use below has to agree: the chips, the
  // removal, what the chooser already has, and what an addition appends to.
  const items = entry === null ? [] : tagsOf(entry);
  const [adding, setAdding] = useState(false);
  const [vocabulary, setVocabulary] = useState<readonly string[]>([]);
  const [refusal, setRefusal] = useState<string | null>(null);
  const addRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!adding) {
      return;
    }
    let cancelled = false;
    void tagsVocabulary()
      .then((vm) => {
        if (!cancelled) {
          setVocabulary(vm.entries.map((tag) => tag.path));
        }
      })
      .catch(() => {
        // Nothing to browse, and typing still works: creating is allowed here,
        // so an unreadable vocabulary costs the completion and not the edit.
        if (!cancelled) {
          setVocabulary([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [adding]);

  // A stable ref callback, so the field takes focus once when the chooser
  // opens and not again on every render of the panel behind it.
  const focusChooser = useCallback((node: HTMLInputElement | null) => {
    node?.focus();
  }, []);

  function closeChooser(): void {
    setAdding(false);
    addRef.current?.focus();
  }

  function remove(item: string): void {
    // Only on a recording note: `recordings` on somebody's own note is
    // somebody's own tag, and keeper does not own rows in a vault it did not
    // write. The predicate is the note's `session:`, the same one the file
    // actions above read, so there is no second answer to "is this a
    // recording note" in this file.
    if (sessionId !== null && namesTag(item, [RECORDINGS_TAG])) {
      setRefusal(recordingsTagRefusal(item));
      return;
    }
    setRefusal(null);
    onChange(items.filter((candidate) => candidate !== item));
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-1">
      <div className="flex flex-wrap items-center gap-1">
        {items.map((item) => (
          <button
            key={item}
            type="button"
            className="rounded bg-muted px-1.5 py-0.5"
            aria-label={`Remove ${item} from ${TAGS_KEY}`}
            onClick={() => remove(item)}
          >
            {item} ×
          </button>
        ))}
        <Button
          ref={addRef}
          type="button"
          variant="ghost"
          size="xs"
          className="shrink-0 text-muted-foreground"
          aria-expanded={adding}
          onClick={() => (adding ? closeChooser() : setAdding(true))}
        >
          <Plus aria-hidden="true" className="size-3" />
          {ADD_NOTE_TAG}
        </Button>
      </div>
      {adding && (
        <TagCombobox
          label={ADD_NOTE_TAG}
          placeholder="Type or browse"
          vocabulary={vocabulary}
          chosen={items}
          // A note may carry a tag no other note carries yet — that is what a
          // first note about a new client is — so this surface creates, and
          // the text goes out exactly as typed. What a tag MEANS is settled in
          // `keeper-core/src/notes/tags.rs`, at the boundary, and folding it
          // here would be the second place that decides (AD-20, Story 42.5).
          allowCreate
          inputRef={focusChooser}
          onChoose={(tag) => {
            setRefusal(null);
            onChange([...items, tag]);
          }}
          onDismiss={closeChooser}
        />
      )}
      {refusal !== null && (
        <p role="alert" className="text-destructive">
          {refusal}
        </p>
      )}
    </div>
  );
}

/**
 * The frontmatter key carrying a recording note's immutable session identity
 * (Story 42.4). Its presence is what makes a note a recording note.
 */
const SESSION_KEY = "session";

/** The key carrying the session folder, relative to the recordings destination. */
const RECORDING_KEY = "recording";

/** The key carrying the session's files, each relative to the same root. */
const FILES_KEY = "files";

/**
 * The dropdown's accessible name. Every path in the block gets one, so the name
 * carries the path it acts on — otherwise a screen reader walking a session's
 * files hears the same control four times.
 */
export const NOTE_PATH_ACTIONS_LABEL = "Actions for";

/**
 * The Reveal item's label. Worded identically to `RECORDINGS_REVEAL_LABEL` (the
 * recordings browser) and `REVEAL_IN_FINDER_LABEL` (the recording completion
 * card): one affordance, one wording, wherever it appears.
 */
export const NOTE_REVEAL_LABEL = "Reveal in Finder";

/** The Preview item's label — it hands the file to the system handler and stops caring. */
export const NOTE_PREVIEW_LABEL = "Preview";

/** The copy item's label. */
export const NOTE_COPY_PATH_LABEL = "Copy path";

/**
 * The note's session id, or `null` for a note that is not about a recording.
 *
 * `session:` is the whole test. It is the identity Story 42.4 writes and the
 * only handle that survives a retitle, so a note carrying one is a recording
 * note; a note without one that happens to have a `files:` key is somebody's
 * own note, and keeper does not put buttons in it.
 *
 * Exported because the live-preview layer asks the same question of the same
 * block to decide whether an `![[…]]` embed is a recording's file: two readings
 * of one predicate is how a note grows a player its properties panel refuses to
 * put a Preview button on.
 */
export function recordingSessionId(parsed: ParsedFrontmatter): string | null {
  if (parsed.unparsed) {
    return null;
  }
  const entry = parsed.entries.find((candidate) => candidate.key === SESSION_KEY);
  if (entry === undefined || entry.nested) {
    return null;
  }
  const id = entry.text.trim();
  return id === "" ? null : id;
}

/** The last `/`-separated component of a relative path. */
function fileName(relativePath: string): string {
  const segments = relativePath.split("/").filter((segment) => segment !== "");
  return segments[segments.length - 1] ?? relativePath;
}

/**
 * Where one of the note's paths is now, or `undefined` when keeper cannot say.
 *
 * The folder is matched by KIND and a file by NAME — never by comparing the
 * note's relative path to the target's. The two frames legitimately disagree:
 * Story 40.4 renames a session folder after the stub is written, so a note made
 * before the rename carries the old path while the index carries the new one.
 * File names do not change when their folder does, which is what makes the name
 * the one join key that survives a retitle.
 *
 * This is a lookup, never a composition: the surface reads targets Rust built
 * and joins nothing itself (AD-65).
 */
function targetFor(
  targets: RecordingNoteTargetVm[] | null,
  relativePath: string,
  wanted: "folder" | "file",
): RecordingNoteTargetVm | undefined {
  if (targets === null) {
    return undefined;
  }
  if (wanted === "folder") {
    return targets.find((target) => target.kind === "folder");
  }
  const name = fileName(relativePath);
  return targets.find(
    (target) => target.kind !== "folder" && fileName(target.relativePath) === name,
  );
}

/** The duplicate-session affordance's label (Story 45.19, FR-197). */
export const RECORD_ANOTHER_LABEL = "Record another like this";

/**
 * What it says when the session's manifest will not load.
 *
 * The alternative would be opening the Recording pane with a blank form, which
 * says "this session had no details" about a session keeper simply could not
 * read. Nothing is filled and nothing is navigated to, because a surface that
 * moved the user somewhere useless would make them work out what happened.
 */
export const RECORD_ANOTHER_UNREADABLE =
  "keeper can't read that session's details, so there is nothing to copy.";

/** Test id for the duplicate-session affordance. */
export const RECORD_ANOTHER_TESTID = "record-another-like-this";

/** Test id for the unreadable-session line beside it. */
export const RECORD_ANOTHER_FAULT_TESTID = "record-another-fault";

/**
 * "Record another like this" (Story 45.19, FR-197).
 *
 * Fills the Recording pane's "Next session" form from THIS session's stored
 * metadata and shows that pane. **It stops there.** The recorder is not
 * touched: no `recording_start`, no capture selection changed, nothing armed.
 * A recorder that begins without a deliberate press is a recorder people stop
 * trusting, and the person who pressed a button in a note was asking to set a
 * session up, not to be recorded.
 *
 * The session is located by the folder target Rust resolved — the same list the
 * paths above act on — so this composes no path of its own (AD-65) and follows
 * a Story 40.4 retitle for free. No folder target means the session is not on
 * this machine, and the button is absent rather than present and failing.
 */
function RecordAnotherLikeThis({ targets }: { targets: RecordingNoteTargetVm[] | null }) {
  const [fault, setFault] = useState<string | null>(null);
  const folder = targetFor(targets, "", "folder");
  if (folder === undefined) {
    return null;
  }
  return (
    <div className="flex flex-col gap-1 pt-1">
      <Button
        size="sm"
        variant="ghost"
        className="w-fit gap-1"
        data-testid={RECORD_ANOTHER_TESTID}
        onClick={() => {
          setFault(null);
          void recordingSessionMeta(folder.absolutePath)
            .then((meta) => {
              if (meta === null) {
                setFault(RECORD_ANOTHER_UNREADABLE);
                return;
              }
              // The whole form at once, replacing whatever was in it: this is
              // "like THIS session", so a field this session left empty must
              // come across empty rather than keep a leftover from the last
              // thing the user typed into the pane.
              recordingMetaStore.getState().setFields({
                title: meta.title,
                participants: meta.participants,
                note: meta.note,
                tags: meta.tags,
                custom: meta.custom.map((row) => ({ name: row.name, value: row.value })),
              });
              primaryViewStore.getState().setView("recording");
            })
            .catch(() => {
              setFault(RECORD_ANOTHER_UNREADABLE);
            });
        }}
      >
        <Video aria-hidden="true" className="size-3" />
        {RECORD_ANOTHER_LABEL}
      </Button>
      {fault !== null && (
        <p role="alert" className="text-destructive" data-testid={RECORD_ANOTHER_FAULT_TESTID}>
          {fault}
        </p>
      )}
    </div>
  );
}

interface RecordingPathsProps {
  /** The `recording:` or `files:` entry, verbatim from the block. */
  entry: PropertyEntry;
  /** The session's targets, or `null` when keeper cannot locate the session. */
  targets: RecordingNoteTargetVm[] | null;
  /** Whether this platform has a user-visible file manager to reveal into. */
  canReveal: boolean;
}

/**
 * A recording note's paths: the text the note carries, and what you can do
 * about it (Story 42.4, FR-142, FR-145).
 *
 * Read-only, unlike every other control in this panel, and deliberately: these
 * two keys are keeper's record of where a recording's bytes actually landed.
 * Typing a different path into them moves no file — it only makes the note lie
 * about a folder that still exists under its old name. The `id` key above is
 * read-only for the same reason, and this is that rule applied to the other
 * facts keeper owns in a note it wrote.
 */
function RecordingPaths({ entry, targets, canReveal }: RecordingPathsProps) {
  // `recording:` is one scalar — the session folder — and `files:` is a list.
  // Each `files:` entry is written relative to the destination root, not to the
  // folder line above it, precisely so it resolves on its own.
  const paths = entry.key === RECORDING_KEY ? [entry.text] : entry.items;
  const wanted = entry.key === RECORDING_KEY ? "folder" : "file";

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
      {paths.map((path) => (
        <RecordingPath
          key={path}
          relativePath={path}
          target={targetFor(targets, path, wanted)}
          canReveal={canReveal}
        />
      ))}
    </div>
  );
}

interface RecordingPathProps {
  /** The path exactly as the note carries it: relative, and the visible text. */
  relativePath: string;
  /** Where it is now, or `undefined` when keeper cannot say. */
  target: RecordingNoteTargetVm | undefined;
  canReveal: boolean;
}

/**
 * One path, one dropdown.
 *
 * The visible text is always the note's own relative path (FR-145): the
 * absolute path is the ARGUMENT of an action and never appears on screen, so
 * nothing here can leak a home directory into a screenshot — or, worse, into
 * the note, since the panel writes back what it renders.
 *
 * **An action is offered only when it has a target.** A session keeper cannot
 * locate — unknown to the index, or a folder that is not on this machine —
 * keeps its text and its Copy path (the relative text is still worth having on
 * the clipboard) and loses the two actions that would open something. A Reveal
 * that opens nothing is worse than an absent one twice over: it tells the
 * reader the recording is there, and then fails at the moment they believed
 * it. Absence says the true thing immediately, and it is the same rule
 * `revealInFileManager` is gated by one line down.
 *
 * The path itself is the overflow affordance (Story 44.12). A recording path is
 * long, the panel is narrow, and what stood here was `title=` — a tooltip a
 * keyboard never sees, on the one value in the panel a caret cannot scroll.
 */
function RecordingPath({ relativePath, target, canReveal }: RecordingPathProps) {
  return (
    <div className="flex min-w-0 items-center gap-1">
      <div className="min-w-0 flex-1 text-[11px]">
        <OverflowValue name={fileName(relativePath)} value={relativePath} monospace />
      </div>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="size-6 shrink-0"
            aria-label={`${NOTE_PATH_ACTIONS_LABEL} ${relativePath}`}
          >
            <MoreHorizontal aria-hidden="true" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {canReveal && target !== undefined && (
            <DropdownMenuItem
              onSelect={() => {
                // Best effort: the reveal either happens or the file manager
                // said no, and neither is something to interrupt a note with.
                void revealPath(target.absolutePath).catch(() => {});
              }}
            >
              <FolderOpen aria-hidden="true" />
              {NOTE_REVEAL_LABEL}
            </DropdownMenuItem>
          )}
          {target?.kind === "video" && (
            <DropdownMenuItem
              onSelect={() => {
                void recordingOpenPath(target.absolutePath).catch(() => {});
              }}
            >
              <Play aria-hidden="true" />
              {NOTE_PREVIEW_LABEL}
            </DropdownMenuItem>
          )}
          <DropdownMenuItem
            onSelect={() => {
              // The absolute path is the useful one — it pastes into a terminal
              // or a Finder "Go to folder" and lands. The relative text is the
              // fallback for a session keeper could not locate: it is what the
              // note says, and copying what is on screen is never wrong.
              void navigator.clipboard
                ?.writeText(target?.absolutePath ?? relativePath)
                .catch(() => {});
            }}
          >
            <Copy aria-hidden="true" />
            {NOTE_COPY_PATH_LABEL}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
