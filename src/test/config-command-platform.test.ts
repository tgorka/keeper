/**
 * `config_layers` is registered on **every** platform, not spliced in as a
 * desktop extra (Story 46.7, AD-98).
 *
 * **Why a source-text test.** `keeper_with_commands!` is one macro invoked
 * twice: the shared literal in its body registers on every target, and each
 * call site splices its own `$extra` list in front — `#[cfg(desktop)]` at
 * lib.rs:819 and `#[cfg(not(desktop))]` at :940. An entry in the wrong half
 * still compiles, still passes `command-registration.test.ts` (whose scan is
 * deliberately not scoped to the literal, so that ninety correctly-registered
 * desktop commands are not reported as missing), and still works on every
 * machine any of this is developed on. It fails on an iOS build only, at
 * runtime, as `Command config_layers not found` — and the Settings surface that
 * calls it renders no section and no override markers, which is exactly the
 * silence AD-98 exists to remove.
 *
 * The `keeper` shell does not build on Linux (AD-55/AD-56), and this fact
 * cannot be asserted in Rust anyway: both halves type-check. So it is asserted
 * here, in the half that runs everywhere, as `capture-capability.test.ts` and
 * `file-scheme-registration.test.ts` do for their own cross-platform facts.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const SHELL_LIB = readFileSync(
  resolve(import.meta.dirname, "../../src-tauri/crates/keeper/src/lib.rs"),
  "utf8",
);

/**
 * The macro body's command literal — the half that reaches every target.
 *
 * Sliced between `generate_handler![` and the closing `])` that follows it,
 * which is inside the macro definition and therefore before either call site.
 */
const sharedLiteral = (() => {
  const start = SHELL_LIB.indexOf("tauri::generate_handler![");
  const end = SHELL_LIB.indexOf("\n            ])", start);
  return SHELL_LIB.slice(start, end);
})();

describe("the shared command literal", () => {
  it("was found, so the assertions below are about something", () => {
    // The guard on the guard: a failed slice is an empty string, and an empty
    // string contains nothing, which would make the real assertion below pass
    // by reporting that a command is not in a list that does not exist.
    expect(sharedLiteral.length).toBeGreaterThan(1000);
    // A command that is unambiguously platform-neutral, to prove the slice
    // caught the body and not some later fragment.
    expect(sharedLiteral).toContain("ipc::app_ping");
    // ...and one that is unambiguously a desktop extra, to prove the slice
    // STOPPED before the call sites.
    expect(sharedLiteral).not.toContain("sync_ipc::sync_profiles");
  });

  it("registers config_layers, so a phone gets an empty stack rather than a rejection", () => {
    expect(sharedLiteral).toContain("ipc::config_layers");
  });
});
