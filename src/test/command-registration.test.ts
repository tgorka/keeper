/**
 * Every command the frontend invokes is registered on the Tauri builder.
 *
 * **Why this file exists.** Story 45.19 generalised a shape three of epic 45's
 * stories were bitten by independently: *does this thing name something, and
 * does anything check the thing it names exists?* — because a reference and a
 * dangling reference are the same bytes.
 *
 * `client.ts` names a command as a string. `ipc.rs` defines a function. Only
 * `lib.rs`'s `generate_handler!` list joins them, and dropping a name from that
 * list breaks nothing anyone can see: the function still exists so `cargo check`
 * is clean, every frontend test mocks `client.ts` so the suite is green, and at
 * runtime the call rejects with "Command <name> not found". A whole feature,
 * invisible, with a green tree.
 *
 * This is not hypothetical here. `lib.rs`'s own comment records it shipping:
 * registering a second `invoke_handler` discarded the first, leaving the
 * desktop build with nine sync commands reachable and *every other command in
 * the app* answering "Command not found" — no account restore, no capability
 * probe, no recording, no bridges — through v0.4.0–v0.4.2. That specific
 * regression has a Rust guard. The general one had nothing.
 *
 * It cannot be caught in Rust where it belongs. The `keeper` shell crate does
 * not build on Linux (AD-55, AD-56), so an assertion there is prose on the box
 * most of this epic was written on. This half runs everywhere and covers
 * exactly the half the shell owns. The pattern is Story 45.15's
 * `capture-capability.test.ts` and Story 45.7's `file-scheme-registration.test.ts`.
 *
 * **What this does NOT prove:** that Tauri routes a real call, that the
 * capability files grant the window permission to make it, or that the handler
 * does anything. Only a running app shows that, and each story's spec names its
 * own gate check. This proves the two lists agree.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

function source(relative: string): string {
  return readFileSync(resolve(import.meta.dirname, "..", relative), "utf8");
}

const CLIENT = source("lib/ipc/client.ts");
const SHELL_LIB = readFileSync(
  resolve(import.meta.dirname, "../../src-tauri/crates/keeper/src/lib.rs"),
  "utf8",
);

/**
 * The command names the frontend asks for, read from the only file allowed to
 * ask. `client.ts` holds the sole `@tauri-apps/api/core` import in `src/`, which
 * is what makes reading one file a complete answer rather than a sample — and
 * the first assertion below is what keeps that true.
 */
const invoked = [
  ...new Set([...CLIENT.matchAll(/invoke(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"/g)].map(([, name]) => name)),
].sort();

/**
 * Every `path::name` mentioned in the shell's entry point. Deliberately not
 * scoped to the `generate_handler!` literal: the desktop commands are spliced
 * in at the macro's call site through `$($extra,)*`, so a literal-only scan
 * would report ninety perfectly-registered commands as missing and teach the
 * next reader to disbelieve this file. The trailing comma is optional because
 * the last entry in a list does not have one.
 */
const registered = new Set(
  [...SHELL_LIB.matchAll(/(?:^|\s)[a-z_]+::([a-z0-9_]+)\s*(?:,|\n\s*\])/g)].map(([, name]) => name),
);

describe("the commands the frontend invokes", () => {
  it("come from client.ts and nowhere else, so reading one file is complete", () => {
    // The scan above is only exhaustive while `client.ts` is the single bridge.
    // A second `invoke` import elsewhere would put commands outside this file's
    // reach and silently narrow every assertion below to a subset.
    const bridges = [...CLIENT.matchAll(/from "@tauri-apps\/api\/core"/g)];
    expect(bridges).toHaveLength(1);
  });

  it("are each registered on the builder", () => {
    // Named rather than counted: a count tells you something is wrong, a list
    // tells you which command stopped working and which story owns it.
    expect(invoked.filter((command) => !registered.has(command))).toEqual([]);
  });

  it("is a list long enough that a broken scan would be obvious", () => {
    // The guard on the guard. Both sides are extracted by regex, and a regex
    // that quietly stops matching yields two empty sets, which agree perfectly.
    // This is the assertion that fails when the extraction breaks rather than
    // the registration.
    expect(invoked.length).toBeGreaterThan(200);
    expect(registered.size).toBeGreaterThan(200);
  });
});
