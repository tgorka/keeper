/**
 * The `keeper-file://` scheme is registered, checked on every machine
 * (Story 45.7, FR-180, AD-65).
 *
 * **Why a TypeScript test for Rust source.** Story 45.19's shape: *does this
 * thing name something, and does anything check the thing it names exists?* —
 * because a reference and a dangling reference are the same bytes.
 * `src/lib/viewers/file-asset-url.ts` composes `keeper-file://…` URLs and hands
 * them to a `<video src>`. That names a protocol handler. Nothing checked the
 * handler was wired.
 *
 * Delete the `register_asynchronous_uri_scheme_protocol` block from
 * `keeper/src/lib.rs` and: the frontend still composes the same URLs, every
 * TypeScript test still passes, `cargo check` is clean, and **every image,
 * video, audio file and PDF in the Files pane silently fails to load** — an
 * `error` event with no status, which this story's own viewer reports as
 * "keeper could not open …, and the platform did not say why". The most
 * honest sentence available, about a defect that is entirely keeper's.
 *
 * It cannot be caught in Rust where it belongs: the `keeper` shell crate does
 * not build on Linux (AD-55, AD-56), so an assertion there is prose on the
 * machine most of this epic was written on. This half runs everywhere, which
 * is the whole point — it covers exactly the half the shell owns and this box
 * cannot compile. The pattern is Story 45.15's `capture-capability.test.ts`,
 * taken verbatim because it is the right one.
 *
 * What this does NOT prove: that Tauri actually serves a byte. Only a running
 * app does that, and the gate check is named in this story's spec — open a
 * video from the Files pane and confirm it shows a frame. This proves the
 * declaration exists and the three spellings of the scheme agree.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { FILE_ASSET_SCHEME } from "@/lib/viewers/file-asset-url";

function rust(relative: string): string {
  return readFileSync(resolve(import.meta.dirname, "../../src-tauri", relative), "utf8");
}

const SHELL_LIB = rust("crates/keeper/src/lib.rs");
const FILE_PROTOCOL = rust("crates/keeper/src/file_protocol.rs");
const FILE_ASSET = rust("crates/keeper-core/src/file_asset.rs");

describe("the keeper-file:// scheme the frontend composes for", () => {
  it("is registered on the Tauri builder with its own handler", () => {
    // Both halves, because either alone is a dangling reference: a
    // registration naming no handler will not compile on macOS, and a handler
    // nothing registers is the silent case this file exists for.
    expect(SHELL_LIB).toContain(
      "register_asynchronous_uri_scheme_protocol(\n        file_protocol::SCHEME,",
    );
    expect(SHELL_LIB).toContain(
      "file_protocol::handle(ctx.app_handle().clone(), &request, responder)",
    );
  });

  it("declares the module the registration names", () => {
    // `mod file_protocol;` is desktop-gated with the rest of the notes and
    // files surface. Without it the registration does not resolve — a compile
    // error on macOS, and invisible here, which is why it is asserted.
    expect(SHELL_LIB).toContain("mod file_protocol;");
  });

  it("spells the scheme once in Rust, not twice", () => {
    // `file_protocol` RE-EXPORTS the core constant rather than declaring a
    // second literal, so the registration and the URL grammar cannot come to
    // disagree. This is the seam; the assertion is that it is still a seam.
    expect(FILE_PROTOCOL).toContain("pub const SCHEME: &str = keeper_core::file_asset::SCHEME;");
  });

  it("agrees with the TypeScript composer about what the scheme is called", () => {
    // The third spelling. `file-asset-url-vectors.json` already pins the
    // composer to the PARSER; this pins it to the REGISTRATION, which no
    // vector table can reach because the registration is not a URL.
    expect(FILE_ASSET).toContain(`pub const SCHEME: &str = "${FILE_ASSET_SCHEME}";`);
  });
});
