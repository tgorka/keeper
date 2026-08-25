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
 * One level in from that subset are the indented maps: `keeper:`, and in an OKF
 * note `generated:` and `prefixes:`. Those are read but never written — a typed
 * control over a map would write back a flattened version and lose the author's
 * structure — and for a long time "never written" was implemented as "never
 * shown", which put the sentence `nested value — edit it in the note` where
 * `generated.at`, `generated.by` and the prefix map binding every predicate in
 * the document should have been. The value is rendered now, read-only, and the
 * pointer at the note stays.
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
 *
 * # Two addresses, one panel (Story 50.4, FR-283)
 *
 * AD-120 makes the tag the thing that files a file into a space, so on a
 * sessions zone this panel is the surface that files a file — and a session's
 * `README.md` is not a note. It has no vault id, no note id and no body
 * subscription. What it has is the same thing a note has: a block of bytes.
 *
 * So the address is a prop rather than an assumption. A note passes the body
 * subscription it always did; anything else passes {@link FilePropertiesTarget},
 * two functions over `(profile id, subpath)`. Everything below the one `write`
 * funnel is identical, which is the point: a second panel is how two surfaces
 * come to disagree about what a property is, and this file exists so there is
 * exactly one answer.
 *
 * {@link FileProperties} is the adapter that binds the second address. It lives
 * here, beside the panel, so a third surface has somewhere obvious to look
 * instead of somewhere to copy.
 */
import { Copy, FolderOpen, MoreHorizontal, Play, Plus, Video } from "lucide-react";
import { Fragment, useEffect, useId, useRef, useState } from "react";
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
  notesFieldVocabulary,
  notesSave,
  type RecordingNoteTargetVm,
  recordingNoteTargets,
  recordingOpenPath,
  recordingSessionMeta,
  revealPath,
  sessionsFileRename,
  syncReadFrontmatter,
  syncWriteFrontmatter,
  tagsVocabulary,
} from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { useNotesVaultsStore } from "@/lib/stores/notes-vaults";
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
   * The value carries an indented map, or a list of them. `keeper:` is the
   * reserved key that has one; an OKF note's `generated:` and `prefixes:` are
   * the ones its author reads.
   *
   * Read-only, and this is the guarantee rather than a limitation to lift: a
   * typed control over a nested map would either flatten it or invent a shape
   * the parser does not have, and either way the write would destroy structure
   * the author put there. {@link nestedLines} is what makes it readable without
   * making it writable.
   */
  nested: boolean;
  /**
   * The indented lines this entry owns, verbatim and in file order, for
   * {@link nestedRows} to shape.
   *
   * Both indented shapes land here — the `- ` items of a block list and the
   * `key: value` pairs of a map — because it takes both to describe a list OF
   * maps, whose first line is a dash and whose second is a pair. Read only when
   * {@link nested} is set: a plain block list is already in {@link items}, in
   * the form the list control can write back.
   */
  nestedLines: string[];
  /**
   * The line ending the block this entry came from is written with (Story
   * 50.4). Carried per entry rather than passed alongside because a rewrite of
   * a block list emits lines of its own, and emitting `\n` into a `\r\n` block
   * would make the next read see one line where there are three.
   */
  newline: string;
}

export interface ParsedFrontmatter {
  /** The `---` fenced block's span, or null when the note has no frontmatter. */
  block: { from: number; to: number } | null;
  entries: PropertyEntry[];
  /** True when a block exists but holds something this reader will not touch. */
  unparsed: boolean;
  /**
   * The line ending the block is written with — `"\n"`, or `"\r\n"` for a file
   * edited on Windows (Story 50.4).
   *
   * A note keeper wrote is always `"\n"`, which is why this did not exist until
   * the panel gained a second address. A file it did not write can be either,
   * and reading `"---\r\n"` as "this file has no properties" was not a cosmetic
   * bug: the panel would then have added a *second* block above the first, and
   * everything in the original would have become body.
   */
  newline: string;
}

const KEY_LINE = /^([A-Za-z0-9_][A-Za-z0-9_.\- ]*):[ \t]*(.*)$/;
const LIST_ITEM = /^([ \t]*)-[ \t]+(.*)$/;
const NUMBER = /^-?\d+(?:\.\d+)?$/;
const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;

/** Strip one layer of matching quotes, if the value has them. */
export function unquote(raw: string): { text: string; quoted: boolean } {
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
  const empty: ParsedFrontmatter = {
    block: null,
    entries: [],
    unparsed: false,
    newline: "\n",
  };
  // The opening fence carries the document's own ending, and every offset
  // below is measured with it.
  const newline = source.startsWith("---\r\n") ? "\r\n" : source.startsWith("---\n") ? "\n" : null;
  if (newline === null) {
    return empty;
  }
  const closing = source.indexOf(`${newline}---`, 3);
  if (closing < 0) {
    return { block: null, entries: [], unparsed: true, newline };
  }
  const opening = 3 + newline.length;
  const inner = source.slice(opening, closing + newline.length);
  const entries: PropertyEntry[] = [];
  let unparsed = false;
  let offset = opening;

  for (const line of inner.split(newline)) {
    const lineFrom = offset;
    offset += line.length + newline.length;
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
      // Kept verbatim too: `items` flattens `- name: Alice` to one string, and
      // that is exactly the shape a list of maps needs to survive as.
      owner.nestedLines.push(line);
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
      owner.nestedLines.push(line);
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
        nestedLines: [],
        newline,
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
      nestedLines: [],
      newline,
    });
  }

  return {
    block: { from: 0, to: closing + newline.length + 3 },
    entries,
    unparsed,
    newline,
  };
}

/** One line of a nested value, flattened for a render that must not nest boxes. */
export interface NestedRow {
  /**
   * How far in the line sits, `0` for the value's own top level.
   *
   * Counted by comparing source columns rather than dividing by a step size: a
   * vault indenting by four and a vault indenting by two both mean one level
   * in, and a divisor would make the first look twice as deep as it is.
   */
  depth: number;
  /**
   * The dotted key chain from the entry's value down to this line — `at`, or
   * `layout.columns`. List items contribute nothing to it, because their
   * position is not a name and inventing an index would put a number in the
   * accessible name of every field of every item.
   */
  path: string;
  /** The line's own key, or `""` for a line that has none. */
  key: string;
  /** The scalar on this line, or `""` when the value is the lines below it. */
  text: string;
  /** The line opened with a `- `, so it is one entry of a list. */
  item: boolean;
}

/** A `- ` opening a list item, in the same grammar {@link LIST_ITEM} reads. */
const NESTED_ITEM = /^-[ \t]+/;

/**
 * A `key: value` line split in two. The key is `""` for a line the property
 * grammar does not describe, which is then all value — shown rather than
 * dropped, because the reader already accepted it into the block.
 */
function splitPair(line: string): { key: string; text: string } {
  const keyed = KEY_LINE.exec(line);
  return keyed === null
    ? { key: "", text: unquote(line.trim()).text }
    : { key: keyed[1], text: unquote(keyed[2].trim()).text };
}

/**
 * A nested value's lines, shaped for reading.
 *
 * Flat rows carrying their own depth, not a tree of children: the panel paints
 * this inside ONE grid cell of a sidebar, and a box per level would spend the
 * value column's width on indentation before it reached a value. A depth is a
 * number the renderer can cap; a chain of nested flex boxes is not.
 *
 * A `- ` gets a row of its own rather than being folded into the pair beside
 * it. The fields under one item are fields of the ITEM, so they have to sit one
 * level in from the dash — folding the dash into `name:` would leave `role:` at
 * the same depth as `name:` and render Alice's role as a sibling of her name.
 *
 * Takes {@link PropertyEntry.nestedLines}, which the reader has already stripped
 * of blank lines and comments, so every line here has something to show.
 */
export function nestedRows(lines: readonly string[]): NestedRow[] {
  const rows: NestedRow[] = [];
  // The levels currently open, innermost last: the source column each was
  // opened at, and the key that opened it. Depth is this stack's height and
  // `path` is its keys, which is why one stack answers both.
  const open: { column: number; key: string }[] = [];

  const enter = (column: number, key: string): { depth: number; path: string } => {
    // A line at or left of the innermost open level closes it. `>=`, not `>`:
    // a sibling sits at the same column, and `>` would nest every line of a
    // flat map under the first one.
    while (open.length > 0 && open[open.length - 1].column >= column) {
      open.pop();
    }
    open.push({ column, key });
    let path = "";
    for (const level of open) {
      if (level.key !== "") {
        path = path === "" ? level.key : `${path}.${level.key}`;
      }
    }
    return { depth: open.length - 1, path };
  };

  for (const line of lines) {
    const column = line.length - line.trimStart().length;
    const rest = line.slice(column);
    const dash = NESTED_ITEM.exec(rest);
    if (dash === null) {
      const pair = splitPair(rest);
      const level = enter(column, pair.key);
      rows.push({ depth: level.depth, path: level.path, ...pair, item: false });
      continue;
    }
    const opened = enter(column, "");
    const item: NestedRow = {
      depth: opened.depth,
      path: opened.path,
      key: "",
      text: "",
      item: true,
    };
    rows.push(item);
    const pair = splitPair(rest.slice(dash[0].length));
    if (pair.key === "") {
      // `- work`: the item IS the scalar. One row, not a dash parenting a leaf.
      item.text = pair.text;
      continue;
    }
    const level = enter(column + dash[0].length, pair.key);
    rows.push({ depth: level.depth, path: level.path, ...pair, item: false });
  }

  return rows;
}

/**
 * The frontmatter key whose value also names the file (FR-97, FR-295).
 *
 * Named because two things read it: the control that writes it, and the rename
 * verb the write is routed through when it changes. A string literal in both
 * places is how one of them comes to be spelled `Title`.
 */
export const TITLE_KEY = "title";

/**
 * Render a scalar the way it was written, quoting only when it must be.
 *
 * Exported for the space row's Rename, which sets this same key from a menu
 * instead of from the panel's own field: two serialisers for one value would
 * disagree about quoting the first time somebody's title contained a colon.
 */
export function serialiseScalar(value: string, quoted: boolean): string {
  const needsQuotes =
    quoted || value === "" || value.trim() !== value || /[:#[\]{}]/.test(value.charAt(0));
  return needsQuotes ? ` "${value.replace(/"/g, '\\"')}"` : ` ${value}`;
}

/**
 * Render a list in the style the user already used, with the block's own line
 * ending.
 *
 * The ending matters for a block list and only for one: its items are lines,
 * and a `\n` item inside a `\r\n` block would be read back as part of the
 * `tags:` line rather than under it.
 */
function serialiseList(items: string[], style: PropertyStyle, newline: string): string {
  if (style === "block") {
    return items.length === 0
      ? " []"
      : `${newline}${items.map((item) => `  - ${item}`).join(newline)}`;
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

/**
 * Add a property, creating the block when the file has none.
 *
 * A fresh block is written with `\n`: keeper authors it, and the promise is
 * about the bytes it did *not* author — the body below, which is untouched
 * either way. An existing block keeps its own ending.
 */
export function addProperty(source: string, key: string, value: string): string {
  const parsed = readFrontmatter(source);
  if (parsed.block === null) {
    return `---\n${key}:${value}\n---\n${source}`;
  }
  // Immediately before the closing fence, so the user's ordering is preserved
  // and a new key never jumps above one they put first.
  const closing = parsed.block.to - 3 - parsed.newline.length;
  const line = `${parsed.newline}${key}:${value}`;
  return `${source.slice(0, closing)}${line}${source.slice(closing)}`;
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

/**
 * What a nested value's full-value affordance opens, and the heading over it.
 *
 * A noun, like {@link UNPARSED_BLOCK_LABEL} above it: both hand back the file's
 * own bytes once the panel has shown as much of them as it can fit.
 */
export const NESTED_VALUE_LABEL = "Nested value";

/**
 * The line under a nested value.
 *
 * What is left of `nested value — edit it in the note`, which used to be the
 * WHOLE of what the panel said about a `generated:` or a `prefixes:` block. The
 * pointer at the note stays, because the render above it is still read-only and
 * a reader who wants to change a prefix has to be told where to go.
 */
export const NESTED_VALUE_HINT = "read-only here — edit it in the note";

/**
 * How many lines of a nested value the panel paints before it stops.
 *
 * A prefix map binding every predicate in an OKF document runs to twenty lines,
 * and twenty lines inside one row is taller than the whole rest of the panel: it
 * would push the tag row and the Add-a-property box off the bottom of a sidebar
 * whose every other row is one line. What is cut is one press away, whole and
 * monospaced, exactly as the unparsed block already offers itself — rather than
 * behind a scroller competing with the panel's own.
 */
export const NESTED_ROWS_SHOWN = 8;

/**
 * The left inset for each level of a nested value, capped.
 *
 * Capped rather than multiplied because this is painted INSIDE the value
 * column, the narrow half of a sidebar: a map five levels deep would spend the
 * column on indentation and leave nothing for the values that are the point.
 * Past the last entry every level draws at the same inset, and the keys still
 * say where they are.
 */
const NESTED_INSETS = ["", "pl-3", "pl-6"] as const;

/**
 * How the panel reaches a file that is not a note (Story 50.4, FR-283).
 *
 * Two functions and no identifiers: the adapter that supplies them holds the
 * address, and the panel stays a lens over a block whatever is behind it.
 */
export interface FilePropertiesTarget {
  /**
   * Write the whole block back. `null` disables editing, which is the file
   * address's spelling of `subscriptionId === null`.
   *
   * Rejects with keeper's own sentence — the clobber refusal is the one that
   * matters — and the panel shows it verbatim beside a way out.
   */
  write: ((nextBlock: string) => Promise<void>) | null;
  /**
   * Write the block **and** make the filename follow the new title, as one act
   * (FR-295).
   *
   * A second function rather than a flag on {@link FilePropertiesTarget.write},
   * because the two are different commands with different guarantees: `write`
   * splices a block, and this compiles a journaled plan that moves the file and
   * rewrites what pointed at it. `null` where the address has no rename — a file
   * outside any session, whose name keeper does not derive.
   *
   * Rejects with keeper's own sentence, including the two the panel exists to
   * carry: a title that folds to nothing has not been written either, and a
   * collision names the file it would have overwritten.
   */
  rename: ((nextBlock: string) => Promise<void>) | null;
  /** Read the file again, for the refusal that says it changed underneath. */
  reread: () => void;
}

/** The note's frontmatter block, verbatim, or `""` when it has none. */
interface PropertiesBlock {
  frontmatter: string;
}

/**
 * A note: the write goes through the body subscription, and takes the buffer
 * with it.
 *
 * The `?: never` fields are what make the union discriminate on presence rather
 * than on a tag nobody would have to pass. A call site cannot accidentally
 * satisfy both halves.
 */
interface NoteAddress {
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
  /**
   * Rename the note's file to follow its new title (FR-97).
   *
   * **Two writes here, one plan on the file address, and the asymmetry is
   * deliberate.** A note's identity is its ULID, so links, pins and unread marks
   * survive whichever half lands first — the worst outcome is a filename that
   * lags its title, which is the state every note in the vault is in today
   * because `notes_rename` shipped with no call site. A session file's identity
   * IS its path, so there the two halves must be one journaled plan or the
   * pointers break.
   *
   * Takes the plain new title because that is what `notes_rename` takes: it
   * derives the filename itself and rewrites nothing, the id having already done
   * that job.
   */
  rename?: ((title: string) => Promise<void>) | null;
  file?: never;
}

/** Any other file, addressed by `(profile id, subpath)` behind the adapter. */
interface FileAddress {
  file: FilePropertiesTarget;
  body?: never;
  subscriptionId?: never;
  baseRev?: never;
  onSaved?: never;
}

export type PropertiesPanelProps =
  | (PropertiesBlock & NoteAddress)
  | (PropertiesBlock & FileAddress);

/**
 * What the panel says when a write did not land and nothing worded it.
 *
 * The note path's own sentence, kept because it is the honest one there:
 * `notes_save` writes a conflict copy rather than refusing, so a rejection is a
 * real fault — a drive that went out — and not an answer with words of its own.
 */
const WRITE_FAILED = "keeper couldn't write that property. The note is unchanged on disk.";

/**
 * The same, for the file address. A different noun and nothing else: what the
 * reader is looking at is a file, and calling it a note would be the panel
 * telling them about a surface they are not on.
 *
 * Almost never seen — every refusal this pair of commands makes arrives with
 * Rust's own sentence, and this is what stands in when a rejection carries none.
 */
const FILE_WRITE_FAILED = "keeper couldn't write that property. The file is unchanged on disk.";

/** The word on the control the clobber refusal offers (Story 50.4, row 10). */
export const PROPERTIES_REREAD_LABEL = "Re-read";

/**
 * Both addresses, resolved to the three things the panel actually does with one.
 *
 * Everything below this function is one panel over one block. `save` is `null`
 * when editing is off — which a note spells `subscriptionId === null` and a file
 * spells `write: null` — and `reread` exists only where a write can refuse
 * rather than fault, which is the file address alone.
 *
 * `save`'s second argument is **the new title when this write changes it, and
 * `null` otherwise** (FR-97, FR-295). That is what makes a retitle one act from
 * the panel's point of view: the panel does not know which surface it is serving
 * and must not start knowing, so it reports *what changed* and the address
 * decides what that costs. On a session file it costs a journaled plan that moves
 * the file and rewrites its pointers; on a note it costs a second command after
 * the save; on an address with no rename it costs nothing and the property is
 * written like any other.
 *
 * A plain function rather than a hook or a branch inside the component so the
 * union narrows once, in one place, instead of at every prop the two halves do
 * not share.
 */
function addressOf(props: PropertiesPanelProps): {
  save: ((nextBlock: string, retitle: string | null) => Promise<void>) | null;
  reread: (() => void) | null;
} {
  if (props.file !== undefined) {
    const { write, rename, reread } = props.file;
    if (write === null) {
      return { save: null, reread };
    }
    return {
      // The rename command writes the block itself, so this is a choice between
      // two writers and never both: two commands over one block would be two
      // journal rows, and the second would be guarding against the first.
      save: (nextBlock, retitle) =>
        retitle !== null && rename !== null ? rename(nextBlock) : write(nextBlock),
      reread,
    };
  }
  const { subscriptionId, body, baseRev, onSaved } = props;
  // Normalised once: `rename` is optional on the note address (a call site that
  // has no vault id cannot supply one), and `undefined` and `null` are the same
  // fact here — there is no rename.
  const rename = props.rename ?? null;
  if (subscriptionId === null) {
    return { save: null, reread: null };
  }
  return {
    save: (nextBlock, retitle) =>
      notesSave(subscriptionId, body, baseRev, nextBlock).then(
        (result) => {
          onSaved(body, result);
          // After the save, never before it: `notes_rename` moves the file and
          // writes nothing into it, so renaming first would leave the new name on
          // a file that still says the old title if the save then failed.
          if (retitle !== null && rename !== null) {
            return rename(retitle);
          }
        },
        () => {
          throw new Error(WRITE_FAILED);
        },
      ),
    reread: null,
  };
}

export function PropertiesPanel(props: PropertiesPanelProps) {
  const { frontmatter } = props;
  const { save, reread } = addressOf(props);
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

  /**
   * The one write funnel, and the one place that notices a title changed.
   *
   * `retitle` is the new title's plain text when this write is a title write, and
   * `null` for every other key — read back through {@link unquote} rather than
   * threaded down from the control, because the control's job is to produce the
   * *serialised* value and a second parameter for the raw one would be a second
   * contract on every kind of control for the sake of one key.
   */
  const write = (nextBlock: string, retitle: string | null = null): void => {
    if (save === null) {
      return;
    }
    void save(nextBlock, retitle)
      .then(() => {
        setFailure(null);
      })
      .catch((error: unknown) => {
        // Said out loud, because the control has already moved and a silent
        // failure would leave the panel showing a value the file does not have.
        // The message is whichever address worded it: Rust's own refusal for a
        // file, and the standing sentence for a note.
        setFailure(error instanceof Error ? error.message : WRITE_FAILED);
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
          <pre className="min-w-0 flex-1 overflow-x-auto font-mono text-meta">{preview}</pre>
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
                    onChange={(value) =>
                      write(
                        spliceProperty(frontmatter, entry, value),
                        // The rename is asked for by the same press that writes
                        // the title, so the two cannot come apart — and on the
                        // session-file address they are literally one plan.
                        entry.key === TITLE_KEY ? unquote(value.trim()).text : null,
                      )
                    }
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
                  write(
                    addProperty(
                      frontmatter,
                      TAGS_KEY,
                      serialiseList(tags, NEW_TAGS_STYLE, parsed.newline),
                    ),
                  )
                }
              />
            </div>
          </Fragment>
        )}
        {/* The one resizer in the app that paints its own hairline: the key /
            value split is grid CELLS, so there is no box on either side of it
            to own a `border-r`. Everywhere else a column owns the edge and the
            handle only lights it — see `ColumnResizerProps.seam`. */}
        <ColumnResizer {...keyColumn.resizerProps} seam="self" />
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
          {/* The way out of a clobber refusal, offered where the refusal is
              read (Story 50.4, row 10). Only the file address has one: a note
              save that loses a race writes a conflict copy rather than
              refusing, so there is nothing there for a re-read to resolve. */}
          {reread === null ? null : (
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="ml-2 h-6"
              onClick={() => {
                setFailure(null);
                reread();
              }}
            >
              {PROPERTIES_REREAD_LABEL}
            </Button>
          )}
        </p>
      )}
    </section>
  );
}

/**
 * The sentence to show for a rejected properties write.
 *
 * `invoke` guarantees an `IpcError` whose `message` is composed in Rust to be
 * rendered verbatim, and for this pair of commands the message is the answer:
 * "these properties changed on disk, re-read the file". Narrowed with `in` and
 * `typeof` rather than asserted, the way `use-text-file`'s own `sentence` is
 * and for the same reason — an unchecked cast would put `undefined` on screen
 * the day the envelope changed.
 */
function refusalSentence(error: unknown, fallback: string): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const { message } = error;
    if (typeof message === "string" && message !== "") {
      return message;
    }
  }
  return fallback;
}

export interface FilePropertiesProps {
  /** The sync profile the file is in. */
  profileId: string;
  /** Profile-relative, as Rust produced it. Never composed here (AD-65). */
  relativePath: string;
  /**
   * The write landed, so whatever else is showing this file should look again.
   *
   * The editor buffer holding the same bytes is the one that must: a properties
   * write changes the file underneath it, and a Save from a stale buffer would
   * put the old block back.
   */
  onWritten: () => void;
  /**
   * The rename landed and the file is at `nextRelativePath` now, so whatever is
   * showing it should be re-ADDRESSED rather than re-read (Story 52.2, FR-302).
   *
   * Supplied only by a host that holds a panel target it can move. Called
   * INSTEAD of {@link onWritten} for a rename that MOVED the file, because for
   * that one the two are opposite instructions: `onWritten` says "read the
   * address you have again", and after a move that address is the one Rust just
   * emptied — which is what put "is no longer in tgdrive" over a file that had
   * merely been renamed.
   *
   * **A rename that moved nothing gets `onWritten` even here**, because then the
   * address the host holds is still the right one and re-reading it is exactly
   * what has to happen: `sessions_file_rename` writes the new title either way,
   * and it answers with the path it was given for the session record — whose
   * filename the shape reader keys on (`files::renames`) — and for any title
   * whose slug is unchanged (`files::rename_target`). Calling `onRenamed` with a
   * path the host already holds re-points nothing, so the buffer beside this
   * panel would still be holding the OLD block and the next Save would put it
   * back. That is the one way this panel could lose somebody's edit, and it is
   * the whole reason this hook is not the unconditional answer to a rename.
   *
   * `nextRelativePath` is the subpath `sessions_file_rename` answered with,
   * passed through untouched. Never the old path plus the new title (AD-65).
   */
  onRenamed?: (nextRelativePath: string) => void;
  /**
   * The `---` block this panel is holding, or `null` while the read is out — and
   * `null` again if it refuses (Story 52.3).
   *
   * The pane below hides exactly the bytes this form holds, so what travels is
   * the block itself and never a boolean about whether a form was mounted — a
   * boolean is how the two came to disagree about a BOM'd file and about the
   * first frame, before the read resolved.
   */
  onBlock?: (block: string | null) => void;
}

/**
 * The properties panel over a file addressed by `(profile id, subpath)`
 * (Story 50.4, FR-283).
 *
 * **An adapter, not a second panel.** It owns exactly what the note editor owns
 * on the other side — the read, the block it is holding, and what to do once a
 * write lands — and renders {@link PropertiesPanel} with it. Everything a
 * person sees and every rule about what may be edited is the panel's, unchanged.
 *
 * **Nothing until the read resolves, and nothing if it refuses.** Rust routes
 * the read through the same `WriteScope` as the write, so a `workspace/` file
 * (AD-113) or one keeper cannot edit rejects — and the panel simply is not
 * there, rather than being there and refusing on the first keystroke.
 *
 * # A title change renames the file (Story 51.6, FR-295)
 *
 * The `rename` half of the address is `sessions_file_rename`, which writes the
 * block, moves the file and rewrites the pointers that named it in one journaled
 * plan. It is offered unconditionally rather than behind a "is this a sessions
 * zone" test on this side: **the question is Rust's to answer**, and it answers
 * it by looking the subpath up in the zone's scanned rows. A file in no session
 * rejects with a sentence, which is the same shape every other refusal here has,
 * and a probe written in TypeScript would be a second definition of "is this in a
 * session" that could disagree with the first.
 *
 * # The pane the file was open in follows it (Story 52.2, FR-302)
 *
 * A rename used to leave the open panel on the address Rust had just emptied,
 * where `panel-strip.tsx`'s standing rule for a `file` target that stops
 * resolving renders Rust's reason — "is no longer in tgdrive. It was moved,
 * renamed or deleted outside keeper." That rule is right for a file renamed on
 * another device and wrong here, because here keeper knows exactly where the
 * file went: `sessions_file_rename` answers with the new subpath for this
 * purpose (`sessions_ipc.rs:3655-3656`).
 *
 * So a host that holds a panel target passes {@link FilePropertiesProps.onRenamed}
 * and re-points it with that answer — through the panels store's own
 * `retargetPanels`, which is the verb for this and not the single-click one: a
 * rename moves the panels that are SHOWING the file rather than the one that has
 * focus, and a title commits on blur, so the click that moved focus lands first.
 * A host that passes no `onRenamed` is unchanged, which is what keeps the note
 * embed and every other caller on today's behaviour.
 *
 * And a rename that answered with the path the panel already holds is `onWritten`
 * rather than `onRenamed` — see {@link FilePropertiesProps.onRenamed}, because
 * that arm is where this feature could have cost somebody their retitle.
 */
export function FileProperties({
  profileId,
  relativePath,
  onWritten,
  onRenamed,
  onBlock,
}: FilePropertiesProps) {
  const [block, setBlock] = useState<string | null>(null);
  // Bumped by the re-read the clobber refusal offers. A counter rather than a
  // second copy of the effect: re-reading is the same read.
  const [reload, setReload] = useState(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `reload` is a deliberate re-run trigger, not a read — the panel bumps it when a write refused because the block changed underneath.
  useEffect(() => {
    let live = true;
    setBlock(null);
    void syncReadFrontmatter(profileId, relativePath)
      .then((read) => {
        if (live) {
          setBlock(read);
        }
      })
      .catch(() => {
        // No panel. Rust has already said why on whichever surface asked for
        // the file itself; a second copy of the sentence here would be the same
        // refusal twice.
        if (live) {
          setBlock(null);
        }
      });
    return () => {
      live = false;
    };
  }, [profileId, relativePath, reload]);

  // Reported from an effect rather than from the call sites of `setBlock`, so the
  // pending state — `null`, set at the start of every read — is reported too. A
  // host that hides these bytes has to know about the frame before they arrive.
  useEffect(() => {
    onBlock?.(block);
  }, [block, onBlock]);

  if (block === null) {
    return null;
  }

  return (
    <PropertiesPanel
      frontmatter={block}
      file={{
        write: (nextBlock) =>
          syncWriteFrontmatter(profileId, relativePath, block, nextBlock).then(
            (landed) => {
              setBlock(landed);
              onWritten();
            },
            (error: unknown) => {
              throw new Error(refusalSentence(error, FILE_WRITE_FAILED));
            },
          ),
        // The block is not adopted from the answer here, unlike `write` above:
        // this command answers with the file's new SUBPATH rather than with the
        // block it wrote.
        //
        // That subpath is forwarded rather than dropped, because it is the only
        // thing that can tell a host WHERE the file went — Rust returns it for
        // exactly this (`sessions_ipc.rs:3655-3656`) and joining the old path to
        // the new title here would be the webview composing a path (AD-65). A
        // host that can re-address itself gets `onRenamed` and does not want
        // `onWritten`: re-reading the old address is what rendered "is no longer
        // in tgdrive" over a file that had only been renamed. A host with no
        // panel target to move gets today's `onWritten` unchanged.
        //
        // **And so does a rename that answered with the path this panel is
        // already addressed by**, which is not an edge case: it is the session
        // record and every other one of the three names whose filename does not
        // follow its title (`files::renames`), and it is any title edit whose
        // slug is unchanged (`files::rename_target`). The title was still
        // written, so the bytes beside this panel are stale, and `onRenamed`
        // with a path its host already holds re-points nothing — the buffer
        // would keep the OLD block and the next Save would put it back, which is
        // the one way this panel could lose somebody's edit. So the news goes to
        // `onWritten`, and this panel re-reads its own block for the same
        // reason: it is holding a block that no longer matches the file, and the
        // next property edit spliced out of it would refuse as a clobber.
        rename: (nextBlock) =>
          sessionsFileRename(profileId, relativePath, block, nextBlock).then(
            (nextRelativePath) => {
              if (nextRelativePath === relativePath) {
                setReload((n) => n + 1);
                onWritten();
                return;
              }
              if (onRenamed === undefined) {
                onWritten();
                return;
              }
              onRenamed(nextRelativePath);
            },
            (error: unknown) => {
              throw new Error(refusalSentence(error, FILE_WRITE_FAILED));
            },
          ),
        reread: () => setReload((n) => n + 1),
      }}
    />
  );
}

interface PropertyControlProps {
  entry: PropertyEntry;
  /** Receives the serialised value, ready to splice after the key's colon. */
  onChange: (value: string) => void;
}

/** Which HTML input a scalar kind wants. Lists and booleans never reach here. */
/**
 * Keys whose values are a convention rather than free text.
 *
 * `stage` is the one that was asked for and the rest are its neighbours: a
 * small vocabulary the vault settles into, written by hand in every note, and
 * therefore written four slightly different ways. keeper does not know which
 * words are allowed — that is the vault's business — so this offers what the
 * vault already uses and accepts anything else typed. A closed dropdown here
 * would be keeper deciding a convention it did not invent.
 */
const SUGGESTED_KEYS = new Set(["stage", "status", "location", "audience", "author"]);

/**
 * A scalar with a suggestion list behind it.
 *
 * `<datalist>`, not `<select>`: the list is what the vault has done so far, not
 * what it may do, and a control that refuses a new value would make the fifth
 * stage impossible to write from the panel that shows the other four.
 *
 * The vocabulary is read on focus rather than on mount. A note with eight
 * suggested keys would otherwise ask eight questions of the index every time
 * anybody opened the panel, to fill lists nobody had opened.
 */
function SuggestedProperty({
  entry,
  onChange,
}: {
  entry: PropertyEntry;
  onChange: (value: string) => void;
}) {
  const listId = useId();
  const vaultId = useNotesVaultsStore((state) => state.activeVaultId);
  const [options, setOptions] = useState<readonly string[]>([]);

  const load = () => {
    if (vaultId === null || options.length > 0) {
      return;
    }
    void notesFieldVocabulary(vaultId, entry.key)
      .then(setOptions)
      // A vocabulary that will not load is a control with no suggestions, never
      // an error: the field still takes whatever is typed into it.
      .catch(() => setOptions([]));
  };

  return (
    <>
      <Input
        aria-label={entry.key}
        list={listId}
        defaultValue={entry.text}
        className="h-7 flex-1 text-xs"
        onFocus={load}
        onBlur={(event) => {
          if (event.target.value !== entry.text) {
            onChange(serialiseScalar(event.target.value, entry.quoted));
          }
        }}
      />
      <datalist id={listId}>
        {options.map((option) => (
          <option key={option} value={option} />
        ))}
      </datalist>
    </>
  );
}

/** Test id on each rendered line of a nested value. */
export const NESTED_ROW_TESTID = "nested-row";

/**
 * A nested value, read-only (the owner's item 1).
 *
 * Each line the reader kept, as `key value` at its own inset, every part of it
 * truncating and offering itself in full the way every other value in this panel
 * does. That is what keeps a twenty-line prefix map inside the value column: the
 * lines are flex rows over a `min-w-0` cell, so a URI long enough to widen the
 * sidebar is cut and handed over whole instead.
 *
 * No control, and deliberately so — see {@link PropertyEntry.nested}. What
 * changed is only that the panel now shows what it will not write.
 */
function NestedValue({ entry }: { entry: PropertyEntry }) {
  const rows = nestedRows(entry.nestedLines);
  const shown = rows.length > NESTED_ROWS_SHOWN ? rows.slice(0, NESTED_ROWS_SHOWN) : rows;
  return (
    <div className="min-w-0 flex-1">
      {shown.map((row, index) => (
        <div
          // Position, not the key: a hand-edited map can carry the same key
          // twice, and a list of maps has no key at all for its dash rows, so
          // two rows sharing a React key would collapse into one — a line of
          // the user's file silently missing from the panel that claims to be
          // showing it.
          //
          // Sound here because these rows are derived from immutable source
          // lines and replaced whole on every read: never reordered, never
          // spliced in place. `links-panel.tsx` carries the same suppression
          // for the same reason, and measured what goes wrong without it.
          // biome-ignore lint/suspicious/noArrayIndexKey: a nested map's rows have no unique key of their own; the list is replaced whole and never reordered
          key={`${index} ${row.path}`}
          data-testid={NESTED_ROW_TESTID}
          className={cn(
            "flex min-w-0 items-baseline gap-1",
            NESTED_INSETS[Math.min(row.depth, NESTED_INSETS.length - 1)],
          )}
        >
          {/* The file's own item marker, and not hidden from a screen reader:
              an item whose value is the map below it has nothing else on its
              line, so hiding the dash would make it a row that announces
              nothing at all. */}
          {row.item && <span className="shrink-0 text-muted-foreground">-</span>}
          {row.key === "" ? null : (
            <div className="min-w-0 text-muted-foreground">
              <OverflowValue name={PROPERTY_NAME_LABEL} value={row.key} />
            </div>
          )}
          {row.text === "" ? null : (
            <div className="min-w-0 flex-1 text-meta">
              <OverflowValue name={`${entry.key}.${row.path}`} value={row.text} monospace />
            </div>
          )}
        </div>
      ))}
      <div className="flex items-baseline gap-1">
        <p className="min-w-0 flex-1 text-meta text-muted-foreground">{NESTED_VALUE_HINT}</p>
        {/* Only when there is more, so a two-line `generated:` map grows no
            affordance — the same condition the unparsed preview uses. */}
        {rows.length > shown.length && (
          <FullValueButton
            name={NESTED_VALUE_LABEL}
            value={entry.nestedLines.join(entry.newline)}
            monospace
          />
        )}
      </div>
    </div>
  );
}

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
  // `created` and `updated` join them, and `updated` is the sharper case:
  // keeper stamps it on EVERY save (`save_document`), so a value typed here is
  // overwritten by the keystroke that saves it. Whatever a person believed they
  // were recording, they were not recording it. `created` is written once and
  // then never again, so a typo there is a lie that stays — which is the same
  // reason from the other end.
  //
  // Shown and copyable, never editable: the dates are worth reading, and the
  // panel is where somebody would look for them.
  if (entry.key === "created" || entry.key === "updated") {
    return (
      <div className="min-w-0 flex-1 text-meta text-muted-foreground">
        <OverflowValue name={entry.key} value={entry.text} monospace />
      </div>
    );
  }

  if (entry.key === "id" || entry.key === SESSION_KEY) {
    return (
      <div className="min-w-0 flex-1 text-meta text-muted-foreground">
        <OverflowValue name={entry.key} value={entry.text} monospace />
      </div>
    );
  }

  if (entry.nested) {
    return <NestedValue entry={entry} />;
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
                  entry.newline,
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
              onChange(serialiseList([...entry.items, value], entry.style, entry.newline));
            }
          }}
        />
      </div>
    );
  }

  if (SUGGESTED_KEYS.has(entry.key)) {
    return <SuggestedProperty entry={entry} onChange={onChange} />;
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
    return serialiseList(tags, entry.style, entry.newline);
  }
  // A scalar holds at most one tag, so from here the count is 0 — the last tag
  // came off — or two or more, because one was added. It is never 1: nothing
  // can rewrite a one-tag scalar into a different one-tag scalar. So there is
  // no "still exactly one" arm and no `tags[0] ?? ""`; both were a branch and a
  // fabricated value for a case no input reaches, and a mutation to the
  // boundary between them survived the whole suite because nothing could tell
  // the two apart.
  return tags.length === 0
    ? serialiseScalar("", entry.quoted)
    : serialiseList(tags, "flow", entry.newline);
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
          // The user pressed "Add tag" to get here, so the caret comes to the
          // field and the list is unfolded for browsing (Story 53.2). One prop,
          // in the control that owns the fold: this was a ref callback whose
          // body was `node?.focus()`, and the browse half of UX-DR61 rode on
          // that focus as a side effect — nothing said so, and deleting the ref
          // left this panel with a bare field and no list until you typed.
          openOnMount
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
      <div className="min-w-0 flex-1 text-meta">
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
