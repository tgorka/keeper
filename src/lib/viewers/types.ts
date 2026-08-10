/**
 * The vocabulary of the one viewer registry (Story 45.2, FR-174, AD-87, AD-91).
 *
 * Split from the table in `registry.ts` so two things stay true.
 *
 * **A viewer component can import its props without importing the table**, and
 * the table can therefore stay React-free. Story 45.5 reads {@link
 * ViewerEntry.icon} off a row to draw a glyph in a Files row that mounts no
 * viewer at all; a `ComponentType` sitting in the row would pull React into
 * that path and make an icon lookup cost a component tree.
 *
 * **The row carries names, not implementations.** `icon` is a string, not a
 * lucide component, and `language` is an id, not a CodeMirror extension. The
 * failure that prevents is a registry that has to import every viewer's
 * dependencies in order to answer "what is this file" — which would make the
 * cheapest question in the app the most expensive one, and would put the
 * bundle-size cost of a document renderer on a surface that only wanted an
 * icon.
 */

import type { ComponentType } from "react";
import type { RecordingNoteTargetKind } from "@/lib/ipc/client";

/**
 * Which component renders a file — the registry's whole answer.
 *
 * Deliberately coarse. A `.md`, a `.csv` and a `.rs` are all `text` because
 * AD-88 says raw and rendered are ONE component: the raw half is a text editor
 * over the real bytes in every case, and which rendered half it offers beside
 * that is {@link ViewerEntry.rendered}, not a different viewer. Giving each
 * format its own id would be three components that must agree about saving,
 * and "the read path and the write path disagree about what the file says" is
 * the defect AD-88 exists to prevent.
 *
 * `unknown` is a first-class member of this union and not an absence (AD-91).
 * A format keeper cannot render still has a viewer; it names the file, states
 * the size and offers the two actions that leave keeper.
 */
export type ViewerId = "video" | "image" | "audio" | "text" | "document" | "folder" | "unknown";

/**
 * The format a row names, one per row, stable across releases.
 *
 * This is what Story 45.4 switches its rendered half on and what Story 45.12
 * matches an embed against. It is NOT an extension: `.md` and `.markdown` are
 * one format, and a surface that switched on the extension instead would have
 * to know that they are the same — which is precisely the knowledge this
 * registry exists to hold in one place.
 *
 * `source` covers every programming language: they differ in
 * {@link ViewerEntry.language} and in nothing else a surface acts on, so a
 * per-language format id would be twenty entries in a union whose only reader
 * would immediately collapse them back to one branch.
 */
export type ViewerFormat =
  | "video"
  | "image"
  | "audio"
  | "folder"
  | "unknown"
  | "markdown"
  | "csv"
  | "json"
  | "jsonl"
  | "plain"
  | "source"
  | "pdf"
  | "docx"
  | "pptx"
  | "xlsx";

/**
 * The view Story 45.4 offers BESIDE raw, or `null` when raw is the only view
 * there is.
 *
 * `null` is not "not decided yet" — it is the positive statement that this
 * format has no structure to render, so the toggle is absent rather than
 * present and showing the same bytes twice. A `.rs` file has no rendered form;
 * offering a toggle that changes nothing is how a control loses its meaning.
 */
export type RenderedView = "markdown" | "table" | "structure";

/**
 * The syntax a file deserves in the raw editor (Story 45.6).
 *
 * An ID, not a grammar. Only `@codemirror/lang-markdown` is in `package.json`
 * today, so most of these have no grammar in this build and the raw editor
 * degrades them to plain text. That is deliberate: the table states what the
 * file IS, and adding a grammar later is a change in the editor's mapping, not
 * a change to the vocabulary. The alternative — listing only the languages the
 * editor can currently highlight — makes the table a record of an
 * implementation gap and guarantees somebody re-derives the real answer
 * somewhere else.
 */
export type LanguageId =
  | "markdown"
  | "csv"
  | "json"
  | "plain"
  | "rust"
  | "typescript"
  | "javascript"
  | "python"
  | "go"
  | "shell"
  | "sql"
  | "html"
  | "css"
  | "xml"
  | "yaml"
  | "toml"
  | "ini"
  | "java"
  | "c"
  | "cpp"
  | "ruby"
  | "php"
  | "lua"
  | "swift"
  | "kotlin";

/**
 * The name of the glyph a surface draws for a row (Story 45.5 maps it to a
 * component).
 *
 * A name rather than a component so this module stays React-free (see the
 * module header), and so the choice of icon set stays the rendering surface's
 * business. Every row has one, which is what makes "adding a format is a row"
 * true: a new format cannot arrive without an icon and render as a blank cell.
 */
export type IconName =
  | "file-video"
  | "file-image"
  | "file-audio"
  | "file-text"
  | "file-code"
  | "file-table"
  | "file-json"
  | "file-document"
  | "folder"
  | "file-question";

/**
 * One row of the registry: everything keeper knows about how to show a format,
 * in one frozen object.
 *
 * **Rows are module singletons and are compared by identity.** Two surfaces
 * resolving the same file get the very same object, which is what makes
 * "the Files pane and a note embed agree" a fact a test can assert with `toBe`
 * rather than a property somebody has to re-check by reading both call sites.
 */
export interface ViewerEntry {
  /** Which component renders it. */
  readonly viewer: ViewerId;
  /** The format this row names. */
  readonly format: ViewerFormat;
  /** What a person calls this format — rendered in the unknown viewer's facts
   *  and available to any surface that wants to say what it is holding. */
  readonly label: string;
  /** The glyph name (Story 45.5). */
  readonly icon: IconName;
  /** The rendered half Story 45.4 offers, or `null` when raw is the only view. */
  readonly rendered: RenderedView | null;
  /** The raw editor's syntax id (Story 45.6), or `null` when the bytes are not
   *  text — which is also the signal that this format must never be offered to
   *  a text editor at all. */
  readonly language: LanguageId | null;
  /**
   * Whether keeper may write this FORMAT's bytes back.
   *
   * One of TWO questions, and both must say yes before a surface offers an
   * edit. This one is about the format: a PDF, a DOCX and a `.mov` are
   * read-only because a lossy round trip through a document container is how
   * people lose work (the epic's "what is NOT in this epic"). Story 45.3's
   * `FilesWriteVm.writable` is the other question — whether this LOCATION can
   * be written. A Markdown file on a read-only volume is not editable either.
   */
  readonly writable: boolean;
}

/**
 * Everything resolution actually reads: a name and Rust's answer about it.
 *
 * **`kind` is required and it comes from Rust.** That is not a convenience
 * field, it is the enforcement of the story's rule that a surface never
 * switches on an extension: a caller cannot even ask this registry a question
 * without having Rust's answer to "what is this file" in hand. There is no
 * name-only overload and there must never be one — the first one added makes
 * the extension the primary key at exactly one call site, and then at two.
 *
 * **Narrower than {@link ViewerFile}, and that is load-bearing rather than
 * tidy.** An icon cell asks the registry once per row of a virtualised tree
 * and mounts no viewer at all. Making it fabricate a whole {@link ViewerFile}
 * would mean allocating a `relativePath`, an `absolutePath` and an `openWith`
 * closure per row per render purely to satisfy a type — and the `openWith:
 * null` it would have to write there is a lie a later reader takes literally
 * as "this file cannot be opened".
 */
export interface ViewerSubject {
  /** The file's own name, with no path in it — what the viewer renders. */
  readonly name: string;
  /** What Rust says this is (`kind_for_file_name`, or the dirent for a folder). */
  readonly kind: RecordingNoteTargetKind;
}

/**
 * The file a surface hands a VIEWER: what {@link ViewerSubject} resolves on,
 * plus everything the viewer renders and acts through.
 */
export interface ViewerFile extends ViewerSubject {
  /** The path relative to whatever root the surface is browsing, `/`-joined.
   *  Safe to render: FR-145's rule against an absolute path reaching a screen
   *  (or a note) is the same rule that keeps a home directory out of a
   *  screenshot. */
  readonly relativePath: string;
  /**
   * The sync profile `relativePath` is relative to, or `null` when this file
   * is not inside one.
   *
   * Every read and write command Story 45.3 exposes is scoped to a profile id
   * plus a profile-relative subpath, so that Rust re-resolves the path through
   * `keeper_sync::browse`'s containment rule on every call and the frontend
   * never names a location the engine has not agreed to. A viewer therefore
   * cannot load its own bytes without this, and it must not substitute
   * {@link ViewerFile.absolutePath}: reading through an absolute path would go
   * around the containment check, which is the one thing AD-65 exists to keep.
   *
   * `null` is a fact worth carrying, not a gap. A panel can VIEW a file
   * outside every profile; it cannot read or write it through these commands,
   * and a viewer holding `null` says so in a sentence rather than offering a
   * save that will fail (the epic's "a file outside a vault ... cannot be
   * written, and the surface says why").
   */
  readonly profileId: string | null;
  /**
   * The same file resolved by RUST, or `null` when the surface has no such
   * handle.
   *
   * ONLY ever an action's argument, and NEVER rendered. The frontend does not
   * construct this — it echoes one Rust composed (AD-65) — and the moment it
   * appears in a label FR-145 is broken.
   */
  readonly absolutePath: string | null;
  /**
   * The size, already formatted by Story 45.5's Rust formatter, or `null` when
   * the surface does not know it.
   *
   * A label rather than a byte count on purpose: there is exactly one place
   * that turns bytes into words (`keeper_core::size::format_file_size`,
   * decimal, so keeper's number equals Finder's number for the same file), and
   * a TypeScript formatter here would be the fifth one in this repo and the
   * third that disagrees.
   */
  readonly sizeLabel: string | null;
  /**
   * Hand the file to the system's default application, or `null` when this
   * surface has no opener it is allowed to use.
   *
   * A thunk supplied by the surface rather than a call the viewer makes,
   * because WHICH opener is legal depends on where the file came from: a
   * profile entry goes through `sync_open_entry` (profile id plus a
   * profile-relative subpath, so the command cannot be pointed at an arbitrary
   * location), and a recording goes through `recording_open_path`, whose root
   * is the recordings destination. The registry knows formats, not provenance,
   * and a viewer that picked the opener itself would eventually pick the wrong
   * one and get a refusal the user reads as a broken button.
   *
   * `null` means the action is ABSENT, never disabled — 43.5's rule for Reveal
   * on a platform with no file manager, applied to the same shape of fact.
   */
  readonly openWith: (() => Promise<void>) | null;
}

/**
 * What every viewer component is handed. Exactly two fields, and no bytes:
 * reading a file is Story 45.3's path and a viewer loads its own, so the
 * registry never becomes a place that reads the disk.
 */
export interface ViewerProps {
  /** The file to show. */
  readonly file: ViewerFile;
  /** The row that selected this viewer — passed in rather than re-resolved, so
   *  a viewer and the surface that mounted it cannot disagree about the format
   *  they are looking at. */
  readonly entry: ViewerEntry;
}

/** A viewer component: {@link ViewerProps} in, a rendered file out. */
export type ViewerComponent = ComponentType<ViewerProps>;
