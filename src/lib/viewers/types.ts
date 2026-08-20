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

import type { ComponentType, ReactNode } from "react";
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
 *
 * **HTML is out of `source` for exactly that reason turned around** (Story
 * 55.5): it gained a rendered half, which is something a surface acts on, and
 * the remembered-view cookie is keyed by format — so leaving it in `source`
 * would file "I prefer the Page tab" under a key every `.rs` and `.py` also
 * reads.
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
  | "html"
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
export type RenderedView = "markdown" | "table" | "structure" | "html";

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
  /**
   * The standing sentence to show before this file is edited, or `null` when
   * keeper manages it (Story 46.14, AD-102).
   *
   * `FilesWriteVm.caveat`, composed in Rust and rendered verbatim — never
   * paraphrased here, the same rule `FilesEntrySyncVm.detail` follows. AD-102
   * gave keeper a second writer for files no vault holds, so a file can now be
   * writable *and* unmanaged: keeper saves it, and gives it no note history, no
   * search index and no conflict copy. A reader has to know that BEFORE the
   * first keystroke, because an edit that quietly does less than the vault path
   * does is worse than the refusal it replaced.
   *
   * `null` for every surface that has no such verdict to carry — a recording,
   * a note embed, a file outside every profile.
   */
  readonly writeCaveat: string | null;
  /**
   * The same standing sentence in ONE line, or `null` alongside
   * {@link ViewerFile.writeCaveat}'s `null` (Story 53.3, FR-318).
   *
   * `FilesWriteVm.caveatShort`, and it exists because Story 53.3 lets a reader
   * fold the caveat away: what folding it shows is this, and what it must never
   * show is nothing. AD-102 is narrowed here rather than deleted — the fact that
   * this file has no history stays on screen before the first keystroke, and the
   * full four sentences are one press away.
   *
   * **Composed in Rust, exactly like the long form, and for a sharper reason.**
   * A webview that clipped {@link ViewerFile.writeCaveat} to one line would be
   * paraphrasing it — and a character count lands mid-clause, in the clause that
   * names what is absent. So `WriteScope::unmanaged_caveat_short` writes this
   * sentence and both forms are built from one list of absences, which is what
   * keeps them from drifting apart.
   *
   * `Some` exactly when `caveat` is, on Rust's side: a surface therefore never
   * has to decide what to show for a file that has one form and not the other.
   */
  readonly writeCaveatShort: string | null;
  /**
   * Why keeper will not write this file's LOCATION, or `null` when it will
   * (Story 45.3's `FilesWriteVm.writable`/`reason`, threaded here by Story
   * 50.3's fix).
   *
   * The sibling of {@link ViewerFile.writeCaveat} and read the same way: a
   * whole sentence composed by `keeper_sync::files_write::WriteRefusal`,
   * rendered verbatim, never paraphrased and never re-derived. The two are
   * mutually exclusive by construction — a location keeper refuses has no
   * caveat, because a caveat is what it says about a file it WILL write.
   *
   * {@link ViewerEntry.writable} is the other half of 45.2's two questions and
   * this is not a duplicate of it: that one is the FORMAT's verdict, the same
   * for every `.md` in the world, and this one is the LOCATION's. A session's
   * `workspace/` file (AD-113) is markdown of a perfectly writable format
   * sitting somewhere every write refuses, and it is the case that proves the
   * two cannot be folded into one.
   *
   * Carried on the file rather than discovered from a refused save, because a
   * surface has to decide what to OFFER before the first keystroke: the Save
   * button, the format toolbar and the slash menu are all controls that would
   * otherwise announce their own refusal. Rust has already answered — the
   * verdict rides on the listing row — so passing it on is not the frontend
   * deciding which locations are writable (AD-65); composing a sentence here,
   * or testing for a `workspace` segment here, would be.
   *
   * `null` for every surface with no such verdict to carry — a recording, a
   * note embed, a file outside every profile.
   */
  readonly writeRefusal: string | null;
}

/**
 * What every viewer component is handed. No bytes: reading a file is Story
 * 45.3's path and a viewer loads its own, so the registry never becomes a place
 * that reads the disk.
 */
export interface ViewerProps {
  /** The file to show. */
  readonly file: ViewerFile;
  /** The row that selected this viewer — passed in rather than re-resolved, so
   *  a viewer and the surface that mounted it cannot disagree about the format
   *  they are looking at. */
  readonly entry: ViewerEntry;
  /**
   * The controls of whatever FRAME is holding this viewer — a panel's fold and
   * its close and its Export — or `null`/absent when that frame draws a row of
   * its own and keeps them (Story 53.3, FR-317).
   *
   * **A viewer handed these MUST draw a header row in every state it can
   * render**, including while it is loading and for a file it refuses to open:
   * the host has given up its own row on the strength of that promise, and a
   * viewer that drew none would leave the panel with no title, no fold and no
   * way to close it. `ownsHostRow` on the row that resolved the component is
   * which viewers make the promise (`components.tsx`), and a host must consult
   * it rather than deciding for itself — that is the same rule AD-87 states for
   * every other question about a format.
   *
   * Shaped as a node rather than a boolean plus a callback registry, exactly as
   * `PaneHeader`'s fourth group is (`pane-header.tsx`): the controls are the
   * host's own, so their labels, their handlers and their store are the host's
   * too, and what travels is the rendered thing.
   */
  readonly frame?: ReactNode;
}

/** A viewer component: {@link ViewerProps} in, a rendered file out. */
export type ViewerComponent = ComponentType<ViewerProps>;
