/**
 * The JSON/JSONL structure scanner (Story 45.4).
 *
 * The load-bearing test in this file is "agrees with JSON.parse about what is
 * JSON". Everything else here is about what a structure view needs that
 * `JSON.parse` cannot give back — a line, a verbatim number, a repeated key —
 * and none of that is worth having if the two disagree about validity.
 */
import { describe, expect, it } from "vitest";
import {
  MAX_STRUCTURE_DEPTH,
  MAX_STRUCTURE_ROWS,
  parseJsonlStructure,
  parseJsonStructure,
} from "./json-structure";

/** Documents a real file might hold, valid and not. Shared by the agreement
 *  test and read by eye when one of the specific tests below fails. */
const CORPUS = [
  "{}",
  "[]",
  '{"a":1}',
  '{"a": 1, "b": [true, false, null]}',
  '  \n {"a"\n:\n"b"}\n ',
  "[1,2,3]",
  '"just a string"',
  "42",
  "-0.5e+10",
  "true",
  "null",
  '{"nested":{"deep":{"deeper":[{"x":1}]}}}',
  '{"unicode":"caf\\u00e9 \\n \\t \\\\ \\" /"}',
  '[{"a":1},{"a":2}]',
  // Not JSON, in the ways files actually fail.
  "{",
  "}",
  "[1,]",
  '{"a":}',
  '{"a" 1}',
  "{a:1}",
  "{'a':1}",
  "[1 2]",
  "01",
  "1.",
  ".5",
  "+1",
  "1e",
  '"unterminated',
  '{"a":1,}',
  "nul",
  "TRUE",
  "",
  "   ",
  "undefined",
  '{"a":1} trailing',
  "[[1],[2]]extra",
  '{"a":"line\nbreak"}',
] as const;

describe("parseJsonStructure agrees with JSON.parse about what is JSON", () => {
  /**
   * The whole justification for a second parser living in TypeScript. If this
   * ever fails, the structure view and every other JSON reader in the app have
   * started disagreeing about the same bytes, and the parse belongs in Rust.
   *
   * Whitespace-only input is the one deliberate divergence and is asserted
   * separately below: `JSON.parse("")` throws, and this scanner reports an
   * empty file, because "this file is empty" is a true and more useful
   * sentence than "unexpected end of input".
   */
  it.each(CORPUS)("verdict matches for %j", (source) => {
    let parseAccepts: boolean;
    try {
      JSON.parse(source);
      parseAccepts = true;
    } catch {
      parseAccepts = false;
    }
    const structure = parseJsonStructure(source);
    if (structure.empty) {
      expect(parseAccepts, "whitespace-only is the documented divergence").toBe(false);
      return;
    }
    expect(structure.errors.length === 0).toBe(parseAccepts);
  });
});

describe("what a structure view needs and JSON.parse cannot give back", () => {
  it("keeps a number's own characters rather than a double's idea of them", () => {
    const source = '{"id": 12345678901234567890, "ratio": 0.1}';
    const { rows } = parseJsonStructure(source);

    const id = rows.find((row) => row.key === "id");
    // The defect this prevents: JSON.parse returns 12345678901234568000 and a
    // viewer whose whole purpose is showing you the file shows you a number the
    // file does not contain.
    expect(id?.text).toBe("12345678901234567890");
    expect(String(JSON.parse(source).id)).not.toBe("12345678901234567890");
    expect(rows.find((row) => row.key === "ratio")?.text).toBe("0.1");
  });

  it("draws both halves of a repeated key and marks the one that loses", () => {
    const { rows } = parseJsonStructure('{"a": 1, "a": 2}');

    const repeated = rows.filter((row) => row.key === "a");
    expect(repeated.map((row) => row.text)).toEqual(["1", "2"]);
    expect(repeated.map((row) => row.duplicate)).toEqual([false, true]);
    // A JavaScript object keeps only the last, which is a different file.
    expect(Object.keys(JSON.parse('{"a": 1, "a": 2}'))).toHaveLength(1);
  });

  it("decodes a string's escapes, because the character is the thing", () => {
    const { rows } = parseJsonStructure('{"who":"caf\\u00e9\\n\\t\\\\\\"/"}');
    expect(rows.find((row) => row.key === "who")?.text).toBe('café\n\t\\"/');
  });

  it("counts what a container holds without a row per member being needed", () => {
    const { rows } = parseJsonStructure('{"list":[1,2,3],"map":{"a":1}}');
    expect(rows[0]).toMatchObject({ kind: "object", count: 2, depth: 0 });
    expect(rows.find((row) => row.key === "list")).toMatchObject({ kind: "array", count: 3 });
    expect(rows.find((row) => row.key === "map")).toMatchObject({ kind: "object", count: 1 });
  });

  it("numbers an array's elements and nests them a level deeper", () => {
    const { rows } = parseJsonStructure('["a","b"]');
    expect(rows.slice(1).map((row) => [row.index, row.depth, row.text])).toEqual([
      [0, 1, "a"],
      [1, 1, "b"],
    ]);
  });
});

describe("a malformed file names the line", () => {
  it("points at the line the file stops being JSON on, not at the file", () => {
    const source = '{\n  "a": 1,\n  "b": oops\n}\n';
    const { errors, rows } = parseJsonStructure(source);

    expect(errors).toHaveLength(1);
    expect(errors[0].line).toBe(3);
    expect(errors[0].column).toBe(8);
    expect(errors[0].message).toBe("a value was expected here");
    // The rows read before the failure survive, so the view can say where it
    // got to rather than showing nothing at all.
    expect(rows.find((row) => row.key === "a")?.text).toBe("1");
  });

  it("names a missing colon rather than reporting a generic failure", () => {
    const { errors } = parseJsonStructure('{"a" 1}');
    expect(errors[0].message).toBe("a colon was expected after the property name");
    expect(errors[0]).toMatchObject({ line: 1, column: 6 });
  });

  it("names an unquoted property name", () => {
    expect(parseJsonStructure("{a:1}").errors[0].message).toBe(
      "a property name in double quotes was expected here",
    );
  });

  it("names a string that is never closed, at the quote that opened it", () => {
    const { errors } = parseJsonStructure('{\n  "a": "starts here\n}\n');
    expect(errors[0].message).toBe("this text is never closed with a quote");
    expect(errors[0].line).toBe(2);
  });

  it("names a leading zero rather than blaming the digit after it", () => {
    const { errors } = parseJsonStructure('{\n  "n": 0123\n}');
    expect(errors[0].message).toBe("a number may not have a leading zero");
    expect(errors[0].line).toBe(2);
  });

  it("names text after the value, which is how two files get concatenated", () => {
    const { errors } = parseJsonStructure('{"a":1}\n{"b":2}\n');
    expect(errors[0].message).toBe("there is more text after the value the file ends with");
    expect(errors[0].line).toBe(2);
  });

  it("refuses a document nested past the depth it will draw, and says so", () => {
    const deep = `${"[".repeat(MAX_STRUCTURE_DEPTH + 5)}1${"]".repeat(MAX_STRUCTURE_DEPTH + 5)}`;
    const { errors } = parseJsonStructure(deep);
    // Not a stack overflow, which in a viewer is the blank pane this story
    // exists to forbid.
    expect(errors[0].message).toContain(`${MAX_STRUCTURE_DEPTH} levels deep`);
  });
});

describe("an empty file is a file, not a failure", () => {
  it.each(["", "   ", "\n\n", "\uFEFF"])("reports emptiness for %j", (source) => {
    const structure = parseJsonStructure(source);
    expect(structure).toMatchObject({ empty: true, errors: [], rows: [], totalRows: 0 });
  });

  it("reports emptiness for JSONL too, including a file of blank lines", () => {
    expect(parseJsonlStructure("\n\n  \n")).toMatchObject({ empty: true, errors: [], rows: [] });
  });
});

describe("a byte-order mark is skipped rather than reported", () => {
  it("reads a BOM-prefixed document as the document it is", () => {
    const { rows, errors } = parseJsonStructure('\uFEFF{"a":1}');
    expect(errors).toEqual([]);
    expect(rows.find((row) => row.key === "a")?.text).toBe("1");
    // The divergence is deliberate and is the reason the agreement test above
    // holds the corpus BOM-free: JSON.parse refuses this file.
    expect(() => JSON.parse('\uFEFF{"a":1}')).toThrow();
  });

  it("keeps the column honest on the line the mark sits on", () => {
    const { errors } = parseJsonStructure("\uFEFF{oops}");
    expect(errors[0]).toMatchObject({ line: 1, column: 3 });
  });
});

describe("JSONL", () => {
  it("reads one record per line and numbers them", () => {
    const { rows, errors } = parseJsonlStructure('{"a":1}\n{"a":2}\n');
    expect(errors).toEqual([]);
    const records = rows.filter((row) => row.depth === 0);
    expect(records.map((row) => [row.index, row.line])).toEqual([
      [0, 1],
      [1, 2],
    ]);
  });

  it("keeps the good lines when one line is truncated, and names that line", () => {
    const { rows, errors } = parseJsonlStructure('{"a":1}\n{"a":\n{"a":3}\n');

    // Most of why JSONL exists: 99,999 readable records and one bad write.
    expect(errors).toHaveLength(1);
    expect(errors[0].line).toBe(2);
    expect(rows.filter((row) => row.key === "a").map((row) => row.text)).toEqual(["1", "3"]);
  });

  it("skips blank lines rather than calling a trailing newline a bad record", () => {
    const { errors, rows } = parseJsonlStructure('\n{"a":1}\n\n\n');
    expect(errors).toEqual([]);
    expect(rows.filter((row) => row.depth === 0)).toHaveLength(1);
    // The record's file line is its real one, not its ordinal among records.
    expect(rows[0].line).toBe(2);
  });

  it("reads a CRLF file, because a Windows export is not a malformed file", () => {
    const { errors, rows } = parseJsonlStructure('{"a":1}\r\n{"a":2}\r\n');
    expect(errors).toEqual([]);
    expect(rows.filter((row) => row.key === "a")).toHaveLength(2);
  });
});

describe("a file too big to draw says how much it is not drawing", () => {
  it("caps the rows and still reports the true total", () => {
    const source = `[${Array.from({ length: MAX_STRUCTURE_ROWS + 50 }, (_, at) => at).join(",")}]`;
    const { rows, totalRows, errors } = parseJsonStructure(source);

    expect(errors).toEqual([]);
    expect(rows).toHaveLength(MAX_STRUCTURE_ROWS);
    // The array itself is a row, so the total is the elements plus one.
    expect(totalRows).toBe(MAX_STRUCTURE_ROWS + 51);
  });
});
