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
 */
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { type NoteWriteVm, notesSave } from "@/lib/ipc/client";

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
    return (
      <section aria-label="Properties" className="border-b px-3 py-2 text-xs">
        <p className="text-muted-foreground">
          This note's properties can't be parsed, so keeper is showing them exactly as they are on
          disk rather than rewriting them.
        </p>
        <pre className="mt-1 overflow-x-auto font-mono text-[11px]">
          {frontmatter.slice(0, 400)}
        </pre>
      </section>
    );
  }

  return (
    <section aria-label="Properties" className="flex flex-col gap-1 border-b px-3 py-2 text-xs">
      {parsed.entries.map((entry) => (
        <div key={entry.key} className="flex items-center gap-2">
          <span className="w-32 shrink-0 truncate text-muted-foreground">{entry.key}</span>
          <PropertyControl
            entry={entry}
            onChange={(value) => write(spliceProperty(frontmatter, entry, value))}
          />
        </div>
      ))}
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
  // The ULID is identity, not metadata: links resolve through it (FR-97), so
  // it is shown and copyable but never editable.
  if (entry.key === "id") {
    return <code className="font-mono text-[11px] text-muted-foreground">{entry.text}</code>;
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
