/**
 * What the preview does when its own chunks never arrive (Story 51.5).
 *
 * `mountMarkdownPreview` documents itself as never rejecting: every caller was
 * written to read `failure` and show the raw view, and none of them wraps the
 * call. Six of the modules it needs arrive through `import()`, so a rejection
 * from any one of them used to travel straight out of the returned promise —
 * past every caller, into the runtime as an unhandled rejection. In production
 * that is an offline reader or a deploy that moved a chunk; under vitest it was
 * a host unmounted while the wave was in flight, which is how it was found (CI
 * reported 17 unhandled rejections over a suite in which all 4375 tests passed).
 *
 * This file is separate from `markdown-preview.test.ts` for a mechanical
 * reason: that suite imports the real grammar and the real decoration layer
 * statically, on purpose, and a module mocked here would be mocked for it too.
 */

import { describe, expect, it, vi } from "vitest";
import { mountMarkdownPreview } from "./markdown-preview";

// The failure is injected at the module boundary rather than by stubbing
// `Promise.all`, because the contract under test is about the `import()` wave
// itself: a factory that throws is exactly what a missing chunk looks like to
// the caller of a dynamic import.
vi.mock("@codemirror/lang-markdown", () => {
  throw new Error("Failed to fetch dynamically imported module: lang-markdown");
});

describe("a preview whose chunks do not arrive", () => {
  it("resolves with a sentence rather than rejecting", async () => {
    const host = document.createElement("div");

    // No `rejects` matcher and no try/catch: the assertion IS that this await
    // completes. A throw here fails the test by being a throw.
    const preview = await mountMarkdownPreview(host, "# hello\n", {
      vaultId: "vault-1",
      assetUrl: (rel) => rel,
      onOpenLink: () => {},
    });

    const PREFIX =
      "keeper could not load its editor for this document, so the source is below, unchanged: ";
    expect(preview.failure).toContain(PREFIX);
    // The caught reason is carried and not swallowed into a bare apology. Not
    // matched literally: the wording belongs to whatever refused the import —
    // here vitest's module mocker, in production the fetch — and pinning that
    // text would make this test about the injector rather than the contract.
    expect((preview.failure ?? "").slice(PREFIX.length).length).toBeGreaterThan(0);
    // The raw view is what the reader is looking at, so the host must not be
    // holding a fragment of a render that never finished.
    expect(host.childNodes.length).toBe(0);
    // And the two no-op arms are reachable: a caller that adopts new text into
    // a preview it was told had failed must not crash for having tried.
    expect(preview.setContent("# other\n")).toBeNull();
    expect(() => preview.destroy()).not.toThrow();
  });
});
