/**
 * The registry's own tests (Story 45.2).
 *
 * What these have to prove is narrow and load-bearing: resolution is TOTAL,
 * the kind dominates the extension, and two callers get the same object. The
 * cross-crate half — that this table can never contradict `kind_for_file_name`
 * — lives in `classifier-agreement.test.ts`, because it reads Rust.
 */

import { describe, expect, it } from "vitest";
import type { RecordingNoteTargetKind } from "@/lib/ipc/client";
import {
  extensionOf,
  FILE_FORMAT_ENTRIES,
  FILE_FORMATS,
  registeredViewerIds,
  resolveViewer,
  UNKNOWN_ENTRY,
  type ViewerFile,
  type ViewerSubject,
} from "@/lib/viewers";

/** A file as a surface would hand it over. Only `name` and `kind` decide the
 *  answer; the rest is what the viewer renders and is irrelevant here. */
function file(name: string, kind: RecordingNoteTargetKind): ViewerFile {
  return {
    name,
    kind,
    relativePath: `folder/${name}`,
    profileId: "profile-1",
    absolutePath: `/Volumes/merope/folder/${name}`,
    sizeLabel: "12 kB",
    openWith: null,
    writeCaveat: null,
    writeCaveatShort: null,
    writeRefusal: null,
  };
}

describe("resolveViewer — a registered kind resolves to its viewer", () => {
  it.each([
    ["screen-0000.mov", "video", "video", "video"],
    ["whiteboard.png", "image", "image", "image"],
    ["room-tone.wav", "audio", "audio", "audio"],
    ["2026-08", "folder", "folder", "folder"],
  ] as const)("%s (%s) resolves to the %s viewer", (name, kind, viewer, format) => {
    const entry = resolveViewer(file(name, kind));
    expect(entry.viewer).toBe(viewer);
    expect(entry.format).toBe(format);
  });

  it.each([
    ["notes.md", "markdown", "text"],
    ["budget.csv", "csv", "text"],
    ["manifest.json", "json", "text"],
    ["events.jsonl", "jsonl", "text"],
    ["events.ndjson", "jsonl", "text"],
    ["README.txt", "plain", "text"],
    ["main.rs", "source", "text"],
    ["report.pdf", "pdf", "document"],
    ["deck.pptx", "pptx", "document"],
  ] as const)("%s refines inside kind file to format %s", (name, format, viewer) => {
    const entry = resolveViewer(file(name, "file"));
    expect(entry.format).toBe(format);
    expect(entry.viewer).toBe(viewer);
  });

  it("takes the kind's answer even when the extension says otherwise", () => {
    // The one rule that makes this not a second classifier: Rust decided this
    // is an image, so the registry does not look at `.md` at all. A `.heic`
    // added to IMAGE_EXTENSIONS in keeper-core must render as an image here
    // with no change to this file.
    expect(resolveViewer(file("chart.md", "image")).viewer).toBe("image");
    expect(resolveViewer(file("chart.pdf", "video")).viewer).toBe("video");
    expect(resolveViewer(file("notes.md", "folder")).viewer).toBe("folder");
  });
});

describe("resolveViewer — an unregistered format is the unknown viewer, not an error", () => {
  it.each([
    "board.sketchpad",
    "archive.zip",
    "installer.exe",
    "clip.mov.bak",
    "Makefile",
    "notes",
    ".gitignore",
    "trailing.",
  ])("%s resolves to the unknown entry", (name) => {
    expect(resolveViewer(file(name, "file"))).toBe(UNKNOWN_ENTRY);
  });

  it("does not throw and does not return undefined for a hostile name", () => {
    // `payload.constructor` is a file `touch` can create. Looked up in a plain
    // object literal it would resolve to Object's constructor — a function
    // wearing a row's type — and the viewer would crash reading `.label`.
    for (const name of ["payload.constructor", "x.__proto__", "a.toString", "b.hasOwnProperty"]) {
      const entry = resolveViewer(file(name, "file"));
      expect(entry).toBe(UNKNOWN_ENTRY);
      expect(typeof entry.label).toBe("string");
    }
  });
});

describe("resolveViewer — resolution is total", () => {
  const names = [
    "",
    ".",
    "..",
    "/",
    "a/",
    ".env",
    ".env.local",
    "notes.MD",
    "CAMERA.MOV",
    "2026/a.mov/notes.txt",
    "a.b.c.d.csv",
    "спутник.jsonl",
    "file name with spaces.pdf",
    "emoji-😀.txt",
    `${"x".repeat(300)}.json`,
  ];
  const kinds: RecordingNoteTargetKind[] = ["video", "image", "audio", "file", "folder"];

  it("every name crossed with every kind yields a complete row", () => {
    for (const name of names) {
      for (const kind of kinds) {
        const entry = resolveViewer(file(name, kind));
        expect(entry).toBeDefined();
        expect(typeof entry.viewer).toBe("string");
        expect(typeof entry.format).toBe("string");
        expect(typeof entry.label).toBe("string");
        expect(typeof entry.icon).toBe("string");
        expect(typeof entry.writable).toBe("boolean");
      }
    }
  });

  it("survives a kind this build's bindings do not know", () => {
    // An older frontend against a newer Rust enum. The Record index is
    // `undefined` at runtime however well typed it is, and a panel must show
    // something rather than crash on a kind it has never heard of.
    const future = { ...file("thing.qqq", "file"), kind: "hologram" as RecordingNoteTargetKind };
    expect(resolveViewer(future)).toBe(UNKNOWN_ENTRY);
  });

  it("is case-insensitive about the extension", () => {
    expect(resolveViewer(file("NOTES.MD", "file")).format).toBe("markdown");
    expect(resolveViewer(file("Budget.CsV", "file")).format).toBe("csv");
  });
});

describe("resolveViewer — two surfaces get the same answer", () => {
  it("returns the identical frozen row for two independently built descriptors", () => {
    // The Files pane and a note embed build their own descriptor for the same
    // file, with different paths and different actions. If these were two
    // objects, the two surfaces could be given two different opinions.
    const fromFiles: ViewerFile = {
      name: "budget.csv",
      kind: "file",
      relativePath: "finance/budget.csv",
      profileId: "profile-1",
      absolutePath: "/Volumes/merope/finance/budget.csv",
      sizeLabel: "4 kB",
      openWith: null,
      writeCaveat: null,
      writeCaveatShort: null,
      // …and one of them sits somewhere keeper refuses to write. The registry
      // answers from the NAME: a refusal changes what a surface may offer over
      // the file, never which viewer draws it, and a build that resolved a
      // fenced file to some read-only row would give the two surfaces two
      // different opinions about what a `.csv` is.
      writeRefusal:
        "60-sessions/active/2026-08-10-keeper/workspace/budget.csv is inside a session's " +
        "workspace — scratch that is not versioned, not synced, and dies with the session.",
    };
    const fromNote: ViewerFile = {
      name: "budget.csv",
      kind: "file",
      relativePath: "2026/session/budget.csv",
      profileId: null,
      absolutePath: null,
      sizeLabel: null,
      openWith: async () => undefined,
      writeCaveat: null,
      writeCaveatShort: null,
      writeRefusal: null,
    };
    expect(resolveViewer(fromFiles)).toBe(resolveViewer(fromNote));
  });

  it("freezes every row so no surface can edit the shared answer", () => {
    for (const entry of [UNKNOWN_ENTRY, ...FILE_FORMAT_ENTRIES]) {
      expect(Object.isFrozen(entry)).toBe(true);
    }
  });
});

describe("resolveViewer asks for only what it reads", () => {
  it("resolves from a bare name and kind", () => {
    // The Files pane asks once per row of a virtualised tree and mounts no
    // viewer. If this needed a whole ViewerFile it would allocate a path, an
    // absolute path and an `openWith` closure per row per render — and that
    // `openWith: null`, written purely to satisfy a type, is a lie the next
    // reader takes literally as "this file cannot be opened".
    const subject: ViewerSubject = { name: "budget.csv", kind: "file" };
    expect(resolveViewer(subject).icon).toBe("file-table");
    // And the same answer a full descriptor gets, by identity.
    expect(resolveViewer(subject)).toBe(resolveViewer(file("budget.csv", "file")));
  });
});

describe("extensionOf", () => {
  it.each([
    ["notes.md", "md"],
    ["NOTES.MD", "md"],
    ["clip.mov.bak", "bak"],
    ["2026/a.mov/notes.txt", "txt"],
    [".env.local", "local"],
  ])("%s has extension %s", (name, expected) => {
    expect(extensionOf(name)).toBe(expected);
  });

  it.each([
    ".gitignore",
    "Makefile",
    "notes",
    "",
    ".",
    "..",
    "trailing.",
    "a/",
  ])("%s has no extension", (name) => {
    expect(extensionOf(name)).toBeNull();
  });
});

describe("the table itself", () => {
  it("keys every row on a bare lowercase extension", () => {
    for (const extension of FILE_FORMATS.keys()) {
      expect(extension).toBe(extension.toLowerCase());
      expect(extension.startsWith(".")).toBe(false);
      expect(extension).not.toBe("");
    }
  });

  it("claims no extension twice", () => {
    // Two rows claiming `json` is last-one-wins, silently: the surface would
    // render whichever row the array happened to end with.
    const claimed = [...FILE_FORMATS.keys()];
    expect(new Set(claimed).size).toBe(claimed.length);
  });

  it("gives every text row a language and every non-text row none", () => {
    // Story 45.6's rule that a binary is never handed to the text editor is
    // this assertion: `language !== null` is the predicate, and it must be
    // exactly the text-shaped rows.
    for (const entry of FILE_FORMAT_ENTRIES) {
      expect(entry.language === null).toBe(entry.viewer !== "text");
    }
    expect(UNKNOWN_ENTRY.language).toBeNull();
  });

  it("offers a rendered half only where the format has a structure", () => {
    for (const entry of FILE_FORMAT_ENTRIES) {
      if (entry.rendered !== null) {
        expect(["markdown", "csv", "json", "jsonl", "html"]).toContain(entry.format);
      }
    }
  });

  it("refuses to call a document or a medium writable", () => {
    for (const entry of FILE_FORMAT_ENTRIES) {
      expect(entry.writable).toBe(entry.viewer === "text");
    }
    expect(UNKNOWN_ENTRY.writable).toBe(false);
    for (const kind of ["video", "image", "audio", "folder"] as const) {
      expect(resolveViewer(file("x", kind)).writable).toBe(false);
    }
  });

  it("names every viewer id the table can produce", () => {
    const ids = registeredViewerIds();
    expect(ids.has("unknown")).toBe(true);
    for (const entry of FILE_FORMAT_ENTRIES) {
      expect(ids.has(entry.viewer)).toBe(true);
    }
  });
});
