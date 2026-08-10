/**
 * JSON and JSONL read as a structure, with the line named when it is not one
 * (Story 45.4, FR-177, AD-88).
 *
 * ## Why this parses in TypeScript, and why that is not `notes::csv`'s mistake
 *
 * 44.16 put CSV grammar in `keeper-core` and forbade a second opinion in the
 * webview, for one reason: the CSV view **writes**. A TypeScript parser beside
 * the Rust one would mean the table you looked at and the bytes that got
 * written were two answers to the same question, and one of them would
 * eventually reformat somebody's file.
 *
 * This view does not write. JSON and JSONL are read-only in this story, the
 * bytes are already in the webview because the raw editor is holding them, and
 * a round trip to Rust to be told what the reader is already looking at buys
 * nothing and costs a copy of the file and a command that cannot be compiled on
 * this machine. So the parse is here.
 *
 * What Rust *would* have bought, said plainly: `serde_json` reports a line and
 * a column on its error for free, and it is one parser instead of two. Both are
 * paid for here instead — the line and column are computed below, and
 * `json-structure.test.ts` asserts this scanner's accept/reject verdict agrees
 * with `JSON.parse` on a corpus, so "a second parser" is a claim under test
 * rather than a hope. **If this view ever gains a write path, that argument
 * collapses and the parse moves to `keeper-core`.**
 *
 * ## Why not simply `JSON.parse`
 *
 * Three things a structure view needs and a parsed JavaScript value cannot give
 * back:
 *
 * - **A line.** `JSON.parse`'s error message is engine-specific prose; V8, JSC
 *   and older runtimes word and position it differently, and the acceptance
 *   criterion here is that the reader is pointed at a line.
 * - **The number that is in the file.** `JSON.parse` puts every number through
 *   a double. `{"id": 12345678901234567890}` comes back as
 *   `12345678901234568000`, and a viewer whose entire purpose is to show you
 *   the file must not show you a number the file does not contain.
 * - **Key order and repeats.** An object with the same key twice is a real
 *   thing in real exports; a JavaScript object silently keeps the last one.
 *   Here both rows are shown and the later is marked.
 *
 * ## What is deliberately tolerated
 *
 * A leading byte-order mark. `JSON.parse` rejects it and so does the grammar,
 * but Excel and Notepad write one and a reader whose file will not display is
 * not learning anything about the JSON specification. It is skipped, not
 * reported — the raw view shows the real bytes, which is where a question about
 * bytes belongs.
 */

/** What a value is. The vocabulary the structure view renders one row per. */
export type JsonValueKind = "object" | "array" | "string" | "number" | "boolean" | "null";

/** One value in the document, flattened in document order. */
export interface JsonRow {
  /** Nesting, 0 at the document's own value. */
  depth: number;
  /** The decoded object key this value was stored under, or null. */
  key: string | null;
  /** The array index this value sat at — or, in JSONL, the record's ordinal. */
  index: number | null;
  kind: JsonValueKind;
  /**
   * A scalar's text: the file's own characters for a number, the decoded
   * characters for a string, `true` / `false` / `null` otherwise. Null for a
   * container, whose size is in {@link count} instead.
   *
   * A number is verbatim on purpose — see the module comment. A string is
   * decoded because `\u00e9` is an encoding of the character, not the
   * character, and this view exists to show the thing.
   */
  text: string | null;
  /** Members or elements a container holds. Null for a scalar. */
  count: number | null;
  /** 1-based line **in the file** the value starts on. */
  line: number;
  /** This key already appeared in this same object. */
  duplicate: boolean;
}

/** Where the document stops being JSON, and what was expected instead. */
export interface JsonParseError {
  /** A finished sentence naming what was expected. No trailing full stop. */
  message: string;
  /** 1-based line in the file. */
  line: number;
  /** 1-based column within that line, counted in UTF-16 code units. */
  column: number;
}

/** A document read as a structure. Never throws; a failure is a field. */
export interface JsonStructure {
  /** Values in document order, capped at {@link MAX_STRUCTURE_ROWS}. */
  rows: JsonRow[];
  /**
   * Where parsing stopped. One entry for JSON. For JSONL, one per bad record —
   * a single malformed line does not withhold the lines that are fine, which
   * is most of the value of the format.
   */
  errors: JsonParseError[];
  /** Nothing but whitespace. Not a structure and NOT an error. */
  empty: boolean;
  /** Values the document holds, which may exceed `rows.length`. */
  totalRows: number;
}

/**
 * How many rows the structure view will build.
 *
 * A 200 MB export is refused upstream by `TEXT_EDIT_MAX_BYTES`, but a 2 MB
 * array of 80,000 short records is not, and 80,000 DOM rows is a frozen pane.
 * The cap is stated to the reader with the real total beside it, so a truncated
 * view never reads as a short file.
 */
export const MAX_STRUCTURE_ROWS = 5_000;

/**
 * How deep the structure view will descend.
 *
 * A recursive-descent parser on `[[[[[…` is a stack overflow, and a stack
 * overflow in a viewer is a blank pane — the one outcome this story forbids. A
 * depth this document exceeds is reported as a parse error naming the depth,
 * which is a true statement about a file no human wrote.
 */
export const MAX_STRUCTURE_DEPTH = 128;

/** Thrown inside the scanner, caught at the entry points, never escapes. */
class ScanFailure extends Error {
  constructor(
    message: string,
    readonly offset: number,
  ) {
    super(message);
    this.name = "ScanFailure";
  }
}

/** Offsets of the first character of every line, for offset -> line/column. */
function lineStartsOf(text: string): number[] {
  const starts = [0];
  for (let at = text.indexOf("\n"); at !== -1; at = text.indexOf("\n", at + 1)) {
    starts.push(at + 1);
  }
  return starts;
}

/** The 1-based line an offset falls on, by binary search over `starts`. */
function lineAt(starts: number[], offset: number): number {
  let low = 0;
  let high = starts.length - 1;
  while (low < high) {
    const mid = (low + high + 1) >> 1;
    if (starts[mid] <= offset) {
      low = mid;
    } else {
      high = mid - 1;
    }
  }
  return low + 1;
}

/** Whitespace JSON allows between tokens. Not `\v`, not `\f`, not U+00A0 — the
 *  grammar lists exactly four and a viewer that accepted more would disagree
 *  with every other JSON reader the file will meet. */
const WHITESPACE = new Set([" ", "\t", "\n", "\r"]);

/** The escapes a JSON string may carry, and what each stands for. */
const ESCAPES: Record<string, string> = {
  '"': '"',
  "\\": "\\",
  "/": "/",
  b: "\b",
  f: "\f",
  n: "\n",
  r: "\r",
  t: "\t",
};

/**
 * One pass over one JSON document, emitting rows and stopping at the first
 * thing that is not JSON.
 *
 * A class rather than a fold because the position is genuinely mutable state
 * threaded through every production; passing it back and forth would be the
 * same state with more places to drop it.
 */
class Scanner {
  private pos = 0;
  private total = 0;
  readonly rows: JsonRow[] = [];

  constructor(
    private readonly text: string,
    private readonly starts: number[],
    /** Added to every emitted line, so a JSONL record reports its file line. */
    private readonly lineOffset: number,
  ) {}

  /** Everything the document holds, including rows past the cap. */
  get totalRows(): number {
    return this.total;
  }

  /** Parse one whole document and refuse anything after it. */
  document(rootIndex: number | null): void {
    this.skip();
    if (this.pos >= this.text.length) {
      throw new ScanFailure("the file ends before a value", this.pos);
    }
    this.value(0, null, rootIndex);
    this.skip();
    if (this.pos < this.text.length) {
      throw new ScanFailure("there is more text after the value the file ends with", this.pos);
    }
  }

  private skip(): void {
    while (this.pos < this.text.length && WHITESPACE.has(this.text[this.pos])) {
      this.pos += 1;
    }
  }

  /** Record a value, unless the cap has been reached — the count keeps rising
   *  either way, because the reader is told how many were not drawn. */
  private emit(row: JsonRow): void {
    this.total += 1;
    if (this.rows.length < MAX_STRUCTURE_ROWS) {
      this.rows.push(row);
    }
  }

  private value(depth: number, key: string | null, index: number | null): void {
    if (depth > MAX_STRUCTURE_DEPTH) {
      throw new ScanFailure(
        `this file nests more than ${MAX_STRUCTURE_DEPTH} levels deep, which keeper will not draw`,
        this.pos,
      );
    }
    const start = this.pos;
    const line = lineAt(this.starts, start) + this.lineOffset;
    const char = this.text[start];

    if (char === "{") {
      const row: JsonRow = {
        depth,
        key,
        index,
        kind: "object",
        text: null,
        count: 0,
        line,
        duplicate: false,
      };
      this.emit(row);
      row.count = this.object(depth);
      return;
    }
    if (char === "[") {
      const row: JsonRow = {
        depth,
        key,
        index,
        kind: "array",
        text: null,
        count: 0,
        line,
        duplicate: false,
      };
      this.emit(row);
      row.count = this.array(depth);
      return;
    }
    if (char === '"') {
      this.emit({
        depth,
        key,
        index,
        kind: "string",
        text: this.string(),
        count: null,
        line,
        duplicate: false,
      });
      return;
    }
    for (const [word, kind] of [
      ["true", "boolean"],
      ["false", "boolean"],
      ["null", "null"],
    ] as const) {
      if (this.text.startsWith(word, start)) {
        this.pos = start + word.length;
        this.emit({ depth, key, index, kind, text: word, count: null, line, duplicate: false });
        return;
      }
    }
    if (char === "-" || (char >= "0" && char <= "9")) {
      this.emit({
        depth,
        key,
        index,
        kind: "number",
        // Verbatim. The whole reason this is not `JSON.parse`.
        text: this.number(),
        count: null,
        line,
        duplicate: false,
      });
      return;
    }
    throw new ScanFailure("a value was expected here", start);
  }

  /** Members of an object, the opening brace already at `pos`. */
  private object(depth: number): number {
    this.pos += 1;
    this.skip();
    if (this.text[this.pos] === "}") {
      this.pos += 1;
      return 0;
    }
    const seen = new Set<string>();
    let members = 0;
    for (;;) {
      this.skip();
      if (this.text[this.pos] !== '"') {
        throw new ScanFailure("a property name in double quotes was expected here", this.pos);
      }
      const name = this.string();
      this.skip();
      if (this.text[this.pos] !== ":") {
        throw new ScanFailure("a colon was expected after the property name", this.pos);
      }
      this.pos += 1;
      this.skip();
      if (this.pos >= this.text.length) {
        throw new ScanFailure("the file ends before the property's value", this.pos);
      }
      const before = this.rows.length;
      this.value(depth + 1, name, null);
      // Marked on the row rather than dropped: a JavaScript object keeps the
      // last of a repeated key and shows you a file that is not the one on
      // disk. Both are drawn, and the reader is told which one loses.
      if (seen.has(name) && before < this.rows.length) {
        this.rows[before].duplicate = true;
      }
      seen.add(name);
      members += 1;

      this.skip();
      if (this.text[this.pos] === ",") {
        this.pos += 1;
        continue;
      }
      if (this.text[this.pos] === "}") {
        this.pos += 1;
        return members;
      }
      throw new ScanFailure(
        this.pos >= this.text.length
          ? "the file ends before this object is closed"
          : "a comma or a closing brace was expected here",
        this.pos,
      );
    }
  }

  /** Elements of an array, the opening bracket already at `pos`. */
  private array(depth: number): number {
    this.pos += 1;
    this.skip();
    if (this.text[this.pos] === "]") {
      this.pos += 1;
      return 0;
    }
    let elements = 0;
    for (;;) {
      this.skip();
      if (this.pos >= this.text.length) {
        throw new ScanFailure("the file ends before this array is closed", this.pos);
      }
      this.value(depth + 1, null, elements);
      elements += 1;
      this.skip();
      if (this.text[this.pos] === ",") {
        this.pos += 1;
        continue;
      }
      if (this.text[this.pos] === "]") {
        this.pos += 1;
        return elements;
      }
      throw new ScanFailure(
        this.pos >= this.text.length
          ? "the file ends before this array is closed"
          : "a comma or a closing bracket was expected here",
        this.pos,
      );
    }
  }

  /** A string's decoded characters, the opening quote already at `pos`. */
  private string(): string {
    const open = this.pos;
    this.pos += 1;
    let out = "";
    for (;;) {
      if (this.pos >= this.text.length) {
        throw new ScanFailure("this text is never closed with a quote", open);
      }
      const char = this.text[this.pos];
      if (char === '"') {
        this.pos += 1;
        return out;
      }
      if (char === "\\") {
        const escaped = this.text[this.pos + 1];
        if (escaped === "u") {
          const digits = this.text.slice(this.pos + 2, this.pos + 6);
          if (!/^[0-9a-fA-F]{4}$/.test(digits)) {
            throw new ScanFailure("a \\u escape needs four hexadecimal digits", this.pos);
          }
          out += String.fromCharCode(Number.parseInt(digits, 16));
          this.pos += 6;
          continue;
        }
        const literal = escaped === undefined ? undefined : ESCAPES[escaped];
        if (literal === undefined) {
          throw new ScanFailure("this backslash does not begin an escape JSON allows", this.pos);
        }
        out += literal;
        this.pos += 2;
        continue;
      }
      // A JSON string cannot span lines, so a raw newline inside one is almost
      // always a quote nobody closed — and the useful place to point is the
      // quote that opened it, not the end of the line the reader can see.
      if (char === "\n" || char === "\r") {
        throw new ScanFailure("this text is never closed with a quote", open);
      }
      // Any other raw control character is a real corruption signal — a
      // truncated write, a binary file with a .json name — so it is named
      // rather than absorbed into a value that would then look fine.
      if (char < " ") {
        throw new ScanFailure("a control character must be escaped inside text", this.pos);
      }
      out += char;
      this.pos += 1;
    }
  }

  /** Consume a run of ASCII digits and say how many. Its own method rather
   *  than a closure inside {@link number}, which would allocate one per number
   *  in a file that can hold thousands. */
  private digitRun(): number {
    const from = this.pos;
    while (
      this.pos < this.text.length &&
      this.text[this.pos] >= "0" &&
      this.text[this.pos] <= "9"
    ) {
      this.pos += 1;
    }
    return this.pos - from;
  }

  /** A number's own characters, verbatim, the sign or first digit at `pos`. */
  private number(): string {
    const start = this.pos;
    if (this.text[this.pos] === "-") {
      this.pos += 1;
    }
    if (this.text[this.pos] === "0") {
      this.pos += 1;
      // `01` is two tokens to the grammar and a typo to everyone else. Named,
      // because "a value was expected" at the second digit explains nothing.
      if (this.text[this.pos] >= "0" && this.text[this.pos] <= "9") {
        throw new ScanFailure("a number may not have a leading zero", start);
      }
    } else if (this.digitRun() === 0) {
      throw new ScanFailure("a number needs at least one digit", start);
    }
    if (this.text[this.pos] === ".") {
      this.pos += 1;
      if (this.digitRun() === 0) {
        throw new ScanFailure("a number needs a digit after the decimal point", start);
      }
    }
    if (this.text[this.pos] === "e" || this.text[this.pos] === "E") {
      this.pos += 1;
      if (this.text[this.pos] === "+" || this.text[this.pos] === "-") {
        this.pos += 1;
      }
      if (this.digitRun() === 0) {
        throw new ScanFailure("a number needs a digit after the exponent", start);
      }
    }
    return this.text.slice(start, this.pos);
  }
}

/** The mark Excel and Notepad put in front of the text. See the module note. */
const BOM = "\uFEFF";

/** Turn a scanner failure into the row-and-column sentence the banner shows. */
function errorAt(
  failure: ScanFailure,
  starts: number[],
  lineOffset: number,
  columnOffset: number,
): JsonParseError {
  const line = lineAt(starts, failure.offset);
  return {
    message: failure.message,
    line: line + lineOffset,
    column: failure.offset - starts[line - 1] + 1 + (line === 1 ? columnOffset : 0),
  };
}

/**
 * One JSON document, read as a structure.
 *
 * Never throws. A file that is not JSON comes back with `errors` populated and
 * whatever rows were read before the failure — which is what lets the view say
 * "this stopped being JSON on line 12" instead of showing nothing.
 */
export function parseJsonStructure(text: string): JsonStructure {
  const columnOffset = text.startsWith(BOM) ? BOM.length : 0;
  const body = text.slice(columnOffset);
  if (body.trim() === "") {
    return { rows: [], errors: [], empty: true, totalRows: 0 };
  }
  const starts = lineStartsOf(body);
  const scanner = new Scanner(body, starts, 0);
  try {
    scanner.document(null);
  } catch (failure) {
    if (!(failure instanceof ScanFailure)) {
      throw failure;
    }
    return {
      rows: scanner.rows,
      errors: [errorAt(failure, starts, 0, columnOffset)],
      empty: false,
      totalRows: scanner.totalRows,
    };
  }
  return { rows: scanner.rows, errors: [], empty: false, totalRows: scanner.totalRows };
}

/**
 * JSON Lines: one document per line, read as one structure.
 *
 * **A bad line does not withhold the good ones.** That is most of why the
 * format exists — a 100,000-record log with one truncated write is 99,999
 * records somebody still needs to read — so each line is parsed on its own and
 * a failure is recorded against its own file line while the rest render.
 *
 * A blank line is skipped rather than reported. Writers append a trailing
 * newline and some append two; refusing a file over that would be pedantry
 * about a file that is completely fine.
 */
export function parseJsonlStructure(text: string): JsonStructure {
  const body = text.startsWith(BOM) ? text.slice(BOM.length) : text;
  if (body.trim() === "") {
    return { rows: [], errors: [], empty: true, totalRows: 0 };
  }
  const rows: JsonRow[] = [];
  const errors: JsonParseError[] = [];
  let total = 0;
  let record = 0;
  const lines = body.split("\n");
  for (let at = 0; at < lines.length; at += 1) {
    // A CRLF file's `\r` belongs to the terminator, not to the record.
    const line = lines[at].endsWith("\r") ? lines[at].slice(0, -1) : lines[at];
    if (line.trim() === "") {
      continue;
    }
    const starts = lineStartsOf(line);
    const scanner = new Scanner(line, starts, at);
    try {
      scanner.document(record);
    } catch (failure) {
      if (!(failure instanceof ScanFailure)) {
        throw failure;
      }
      errors.push(errorAt(failure, starts, at, 0));
      record += 1;
      total += scanner.totalRows;
      continue;
    }
    for (const row of scanner.rows) {
      if (rows.length < MAX_STRUCTURE_ROWS) {
        rows.push(row);
      }
    }
    total += scanner.totalRows;
    record += 1;
  }
  return { rows, errors, empty: false, totalRows: total };
}
