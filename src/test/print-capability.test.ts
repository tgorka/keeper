import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * The window label and the capability glob, which are in two files and one
 * language apart.
 *
 * Every capability in this app is window-scoped, and a window whose label
 * matches no `windows` entry can invoke nothing at all — it renders and sits
 * inert. There is no error: the page simply never reads its file and never says
 * it is ready, and the export times out after two minutes with a sentence about
 * the page rather than about the permission that was missing. That is the
 * failure this test exists to make loud.
 */
const ROOT = resolve(__dirname, "../..");

describe("the print window can reach the app it was opened by", () => {
  it("builds a label the capability's glob matches", () => {
    const rust = readFileSync(resolve(ROOT, "src-tauri/crates/keeper/src/sync_ipc.rs"), "utf8");
    const built = /let label = format!\("(print-[^"]*)"/.exec(rust);
    expect(built, "sync_export_pdf no longer builds a label this can read").not.toBeNull();

    const capability = JSON.parse(
      readFileSync(resolve(ROOT, "src-tauri/crates/keeper/capabilities/print.json"), "utf8"),
    ) as { windows: string[]; permissions: string[] };

    // `print-{}` against `print-*`: the prefix is the whole of the match, and
    // the token after it is alphanumerics by construction.
    const prefix = (built?.[1] ?? "").replace(/\{\}.*$/, "");
    expect(capability.windows.some((glob) => glob === `${prefix}*`)).toBe(true);

    // Least privilege, asserted rather than described: this window is off-screen
    // on purpose, and a window permission would let the webview move itself back.
    expect(capability.permissions).toEqual(["core:default"]);
  });
});
