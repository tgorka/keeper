import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { ALL_SPACE_ID } from "./all-spaces";

/**
 * One string, two languages, and nothing else holding them together.
 *
 * Rust sends this id on the space it composes out of every folder's rows; the
 * rail compares against it to decide that the row is the virtual all-spaces
 * view with no file to edit or delete. If the two ever disagree the row keeps
 * working — and silently grows a pencil that opens nothing and a bin that
 * deletes nothing, which is the kind of break that ships.
 */
describe("the all-spaces space id", () => {
  it("is the same string on both sides of the wire", () => {
    const rust = readFileSync(
      resolve(__dirname, "../../../src-tauri/crates/keeper/src/notes_ipc.rs"),
      "utf8",
    );
    const match = /const ALL_SPACE_ID: &str = "([^"]+)"/.exec(rust);
    expect(match, "the Rust constant is gone or renamed").not.toBeNull();
    expect(match?.[1]).toBe(ALL_SPACE_ID);
  });
});
