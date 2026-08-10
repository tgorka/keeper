/**
 * The quick-capture windows' authority, checked on every machine (Story 45.15,
 * FR-191).
 *
 * **Why a TypeScript test for a Rust config file.** Tauri capability files are
 * window-scoped: a window whose label the `windows` list does not cover renders
 * perfectly and can invoke no plugin permission at all. It cannot hide, it
 * cannot close, it cannot be dragged and it cannot follow a link — and nothing
 * says so anywhere. The capability file's own description calls that "looks
 * like a frontend bug and is not".
 *
 * Story 45.15 turned one statically declared capture window into as many as the
 * user opens, each with a label derived at runtime, so this is precisely the
 * failure the story could most easily have shipped. `notes_window.rs` asserts
 * the same thing — but the `keeper` shell crate does not compile on Linux,
 * where most of this epic was written, so that assertion is prose on the
 * machine that needs it. This half runs everywhere.
 *
 * It follows `no-user-agent-gating.test.ts`, which is this repo's existing
 * idiom for an invariant that is about a file rather than about a function.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const CAPABILITY = resolve(
  import.meta.dirname,
  "../../src-tauri/crates/keeper/capabilities/quick-capture.json",
);

const TAURI_CONF = resolve(import.meta.dirname, "../../src-tauri/crates/keeper/tauri.conf.json");

const capability = JSON.parse(readFileSync(CAPABILITY, "utf8")) as {
  description: string;
  windows: string[];
  permissions: string[];
};

const conf = JSON.parse(readFileSync(TAURI_CONF, "utf8")) as {
  app: { windows: { label: string; url?: string }[] };
};

const CLIENT_TS = readFileSync(resolve(import.meta.dirname, "../lib/ipc/client.ts"), "utf8");

const NOTES_WINDOW_RS = readFileSync(
  resolve(import.meta.dirname, "../../src-tauri/crates/keeper/src/notes_window.rs"),
  "utf8",
);

describe("the capture window's document", () => {
  it("is spelled the same by the static declaration and by the code that creates one", () => {
    // W3Recording's shape: *does this thing name something, and does anything
    // check the thing it names exists?* `capture.html` is named twice — by
    // `tauri.conf.json` for the prewarmed window and by `notes_window.rs`'s
    // `CAPTURE_DOCUMENT` for every window it creates — and nothing made the two
    // agree. Rename the file and the prewarmed window follows while every
    // window opened on a note loads nothing: the works-at-the-root,
    // breaks-everywhere-else shape, and a blank window says nothing about why.
    const declared = conf.app.windows.find((window) => window.url?.startsWith("capture."));
    expect(declared?.url, "tauri.conf.json no longer declares a capture document").toBeDefined();
    expect(NOTES_WINDOW_RS).toContain(`const CAPTURE_DOCUMENT: &str = "${declared?.url}";`);
  });
});

describe("the capture window's events", () => {
  /**
   * Every event name is written twice — once in `notes_window.rs` where it is
   * emitted, once in `client.ts` where it is listened for — and a `listen` on a
   * name nothing emits is the quietest failure in the codebase: no throw, no
   * rejection, no log, just a listener that never fires. Story 45.15 doubled the
   * exposure by adding a second event, and W2Media's framing is the reason this
   * is here at all: **before this wave there was one namer and nothing to
   * disagree with; adding the second namer created the hazard.**
   */
  it.each([
    ["CAPTURE_SHOWN_EVENT", "NOTES_CAPTURE_SHOWN_EVENT"],
    ["CAPTURE_WINDOWS_EVENT", "NOTES_CAPTURE_WINDOWS_EVENT"],
  ])("is emitted and listened for under one name (%s)", (rustName, tsName) => {
    const emitted = new RegExp(`pub const ${rustName}: &str = "([^"]+)";`).exec(NOTES_WINDOW_RS);
    expect(emitted, `${rustName} is no longer declared in notes_window.rs`).not.toBeNull();
    const listened = new RegExp(`export const ${tsName} = "([^"]+)";`).exec(CLIENT_TS);
    expect(listened, `${tsName} is no longer declared in client.ts`).not.toBeNull();
    expect(listened?.[1]).toBe(emitted?.[1]);
  });

  it("keeps the shown event per-window, which several windows made load-bearing", () => {
    // `emit` tells EVERY window; `emit_to` tells one. With a single capture
    // window the two are indistinguishable, which is exactly why the app-wide
    // version survived until this story — and with several, it makes raising one
    // window yank focus through all of them.
    expect(NOTES_WINDOW_RS).toContain("emit_to(window.label(), CAPTURE_SHOWN_EVENT");
  });
});

describe("the quick-capture capability", () => {
  it("covers the prewarmed window by its exact label", () => {
    // Read out of tauri.conf.json rather than written as a literal here: the
    // two files must agree, and a literal in the middle would agree with
    // neither if the declaration were renamed.
    const declared = conf.app.windows.find((window) => window.url?.startsWith("capture."));
    expect(declared, "tauri.conf.json no longer declares a capture window").toBeDefined();
    expect(capability.windows).toContain(declared?.label);
  });

  it("covers every window a second capture can be given, by glob", () => {
    // `keeper_core::capture::capture_label` builds `quick-capture-<16 hex>`.
    // Without this entry the second capture window matches nothing and is
    // inert; with an exact label instead of a glob it would match nothing too,
    // because the hash is not known until the note is.
    expect(capability.windows).toContain("quick-capture-*");
    const matcher = new RegExp(
      `^${"quick-capture-*".replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*")}$`,
    );
    expect(matcher.test("quick-capture-cc4b3bd3e1a2a1ca")).toBe(true);
    // And it must not be so wide that it covers the main window, which has a
    // capability of its own and a very different one.
    expect(matcher.test("main")).toBe(false);
  });

  it("grants exactly the window permissions this story's chrome needs", () => {
    // Each of these is a control the user can see. A missing grant is a button
    // that renders and does nothing, which is the whole hazard.
    for (const permission of [
      // Escape, unchanged since AD-60.
      "core:window:allow-hide",
      // The close button (FR-191) — Escape being the only way out is a way out
      // nobody discovers.
      "core:window:allow-close",
      // The Linux compositor race `keeper://notes-capture-shown` recovers from.
      "core:window:allow-set-focus",
      // The unlocked window's drag (FR-192). An undecorated window has no title
      // bar, so this IS the lock icon's mechanism.
      "core:window:allow-start-dragging",
    ]) {
      expect(capability.permissions).toContain(permission);
    }
  });

  it("grants what mounting the real note editor drags in with it", () => {
    // Story 45.14 mounts NoteEditor here, and two of its controls are plugin
    // calls rather than our own commands: Attach a File opens a dialog, and a
    // link opens through the opener plugin. Either one missing is a control
    // that works in the main window and silently rejects in this one — the same
    // affordance with a different outcome, which is worse than not having it.
    expect(capability.permissions).toContain("dialog:allow-open");
    expect(capability.permissions).toContain("opener:default");
  });

  it("still refuses everything a capture window has no business doing", () => {
    // Least privilege is the file's whole premise, so the negative half is
    // asserted rather than assumed. `set-position` in particular: Rust places
    // these windows, and a webview that could place itself could place itself
    // off screen.
    for (const permission of [
      "core:window:allow-set-position",
      "core:window:allow-set-size",
      "deep-link:default",
      "global-shortcut:default",
    ]) {
      expect(capability.permissions).not.toContain(permission);
    }
  });

  it("describes what it grants, so the next reader is not told a lie", () => {
    // This file used to say "No opener, no dialog, no deep-link". Two of those
    // three are now false. A description that contradicts the permissions below
    // it is the shape that has already cost this epic an afternoon: a contract
    // stated in a comment and enforced nowhere.
    for (const [permission, word] of [
      ["opener:default", "opener"],
      ["dialog:allow-open", "dialog"],
      ["core:window:allow-close", "close"],
      ["core:window:allow-start-dragging", "drag"],
    ] as const) {
      if (capability.permissions.includes(permission)) {
        expect(capability.description.toLowerCase()).toContain(word);
        expect(capability.description).not.toMatch(new RegExp(`no ${word}`, "i"));
      }
    }
  });
});
