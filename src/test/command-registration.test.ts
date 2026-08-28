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
  ...new Set(
    [...CLIENT.matchAll(/invoke(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"/g)].map(([, name]) => name),
  ),
].sort();

/**
 * The names registered on a given target.
 *
 * **Scoped per target, and that is the whole point of this function.** The
 * shell registers commands in three places: the shared `generate_handler!`
 * literal inside `macro_rules! keeper_with_commands`, a `#[cfg(desktop)]` call
 * site that splices in the desktop-only commands, and a `#[cfg(not(desktop))]`
 * call site that splices in `Unsupported` iOS twins so the handler list is the
 * same shape on every target (AD-27, AD-33).
 *
 * A scan that reads the whole file and keeps only the `name` half of
 * `path::name` cannot tell those apart, because the desktop entry is
 * `notes_ipc::foo` and its twin is `ipc::foo` — same captured name. Story 48.4
 * proved that empirically: deleting a command's DESKTOP registration left this
 * suite green, because the iOS twin satisfied the check. The guard was
 * answering "is this command registered somewhere" when the question it exists
 * to ask is "is it registered on the target that implements it".
 *
 * So each call site is read separately and the shared literal is added to both.
 */
function registeredOn(target: "desktop" | "mobile"): Set<string> {
  const cfg = target === "desktop" ? "#[cfg(desktop)]" : "#[cfg(not(desktop))]";
  const at = SHELL_LIB.indexOf(`${cfg}\n    let builder = keeper_with_commands!(`);
  if (at < 0) {
    throw new Error(`no ${cfg} keeper_with_commands! call site — the shell's shape changed`);
  }
  const open = SHELL_LIB.indexOf("(", SHELL_LIB.indexOf("keeper_with_commands!", at));
  const close = SHELL_LIB.indexOf("\n    );", open);
  const spliced = SHELL_LIB.slice(open, close);
  // The shared literal: everything the macro body registers on every target.
  const body = SHELL_LIB.indexOf("tauri::generate_handler![");
  const shared = SHELL_LIB.slice(body, SHELL_LIB.indexOf("\n            ]", body));
  return new Set(
    [...`${shared}\n${spliced}`.matchAll(/(?:^|\s)[a-z_]+::([a-z0-9_]+)\s*(?:,|\n)/g)].map(
      ([, name]) => name,
    ),
  );
}

const registered = registeredOn("desktop");

describe("the commands the frontend invokes", () => {
  it("come from client.ts and nowhere else, so reading one file is complete", () => {
    // The scan above is only exhaustive while `client.ts` is the single bridge.
    // A second `invoke` import elsewhere would put commands outside this file's
    // reach and silently narrow every assertion below to a subset.
    const bridges = [...CLIENT.matchAll(/from "@tauri-apps\/api\/core"/g)];
    expect(bridges).toHaveLength(1);
  });

  it("are each registered on the DESKTOP builder", () => {
    // Named rather than counted: a count tells you something is wrong, a list
    // tells you which command stopped working and which story owns it.
    //
    // "Desktop" is load-bearing. Until story 48.4 this read the whole file and
    // an iOS `Unsupported` twin could satisfy it for a command whose desktop
    // registration had been deleted — a feature dead on the only platform that
    // ships, with a green tree. That is the exact class this file exists to
    // stop, and it survived here for two epics.
    expect(invoked.filter((command) => !registered.has(command))).toEqual([]);
  });

  it("keeps the two targets' handler lists the same shape", () => {
    // AD-27/AD-33: iOS carries `Unsupported` twins so the list is identical on
    // every target and `cargo check --target aarch64-apple-ios` stays green.
    // Asserted rather than assumed, because it is what makes the scan above
    // meaningful: if the twins stopped mirroring, a desktop-only reading would
    // start reporting commands iOS never had.
    const mobile = registeredOn("mobile");
    // A scan-sanity floor, not a target. It is lower than the desktop side's
    // because iOS carries twins for only the commands a phone can reach at
    // all — measured at 192 when this was written, and the number exists to
    // catch a regex that stopped matching, not to pin a count that will move.
    expect(mobile.size).toBeGreaterThan(150);
    // Every twin names a command the desktop also registers. The reverse is
    // deliberately not asserted — desktop-only commands with no twin are the
    // normal case (`CapabilitiesVm.notes` is false on iOS, so nothing calls
    // them) and requiring a twin for each would be a different, wrong rule.
    expect([...mobile].filter((name) => !registered.has(name))).toEqual([]);
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
