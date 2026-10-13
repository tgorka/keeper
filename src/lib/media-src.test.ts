/**
 * The front's one custom-scheme URL speller (Epic 98, story 98.3 #3).
 *
 * Two halves: the two spellings, chosen by what Rust said about this build, and
 * a scan of `src/` that fails on any other file spelling such a URL itself — a
 * second speller is a URL that is right on the Mac and a dead image on the
 * tablet.
 */
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import { recordingAssetUrl } from "@/components/notes/editor/media-playback";
import { mediaSrc } from "@/lib/media-src";
import { platformStore } from "@/lib/stores/platform";
import { fileAssetUrl } from "@/lib/viewers/file-asset-url";

afterEach(() => {
  platformStore.getState().applySnapshot({ urlForm: "scheme", tablet: false, disclosure: null });
});

describe("mediaSrc", () => {
  it("spells a URL the way Tauri serves the scheme on each platform", () => {
    expect(mediaSrc("keeper-file", "01P/a%20b.png", "scheme")).toBe("keeper-file://01P/a%20b.png");
    expect(mediaSrc("keeper-file", "01P/a%20b.png", "localhost")).toBe(
      "http://keeper-file.localhost/01P/a%20b.png",
    );
  });

  it("takes the spelling Rust said this build has, and follows it when it changes", () => {
    expect(recordingAssetUrl("01S", "a b/c.mov")).toBe("keeper-recording://01S/a%20b/c.mov");
    platformStore
      .getState()
      .applySnapshot({ urlForm: "localhost", tablet: true, disclosure: null });
    expect(recordingAssetUrl("01S", "a b/c.mov")).toBe(
      "http://keeper-recording.localhost/01S/a%20b/c.mov",
    );
    expect(fileAssetUrl("01P", "x.png")).toBe("http://keeper-file.localhost/01P/x.png");
  });

  it("spells the first URL in the form Rust wrote into the webview before the bundle ran", async () => {
    // What the shell's initialization script sets (`ipc::platform_plugin`);
    // nothing is applied to the store after it is created. The modules are
    // imported afresh, after the global exists, because module creation is
    // the moment under test — a static import ran before this line.
    window.__KEEPER_PLATFORM__ = { urlForm: "localhost", tablet: true, disclosure: null };
    vi.resetModules();
    try {
      const { fileAssetUrl: firstFrame } = await import("@/lib/viewers/file-asset-url");
      const { platformStore: fresh } = await import("@/lib/stores/platform");
      expect(firstFrame("01P", "x.png")).toBe("http://keeper-file.localhost/01P/x.png");
      expect(fresh.getState().tablet).toBe(true);
    } finally {
      delete window.__KEEPER_PLATFORM__;
      vi.resetModules();
    }
  });
});

const SRC_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const HELPER = path.join(SRC_DIR, "lib", "media-src.ts");
/**
 * A custom-scheme URL spelled in code: a literal `keeper-…://`, a template's
 * `${scheme}://` or `.localhost/${…}`, a concatenated `+ "://"`, or a
 * `".localhost/"` literal. It does not see a URL assembled from pieces that
 * carry neither the separator nor the host suffix as literals (an array
 * `join`, a variable holding `"://"`); the helper's doc comment is the rule
 * there.
 */
const SPELLING =
  /keeper-(media|file|note|recording):\/\/|\}:\/\/|\.localhost\/\$\{|\+\s*["'`]:\/\/|["'`]\.localhost\//;

/** Every non-test `.ts`/`.tsx` file under `dir`. */
function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      return entry.name === "test" ? [] : sources(full);
    }
    return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [full] : [];
  });
}

/** `text` with `//` and `/* … *\/` comments blanked, so prose naming a scheme is not code. */
function code(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/.*$/gm, "$1");
}

describe("no component spells a custom-scheme URL itself", () => {
  it("walks the files that compose these URLs", () => {
    // A walk that found nothing — a moved `src/`, a broken file filter — would
    // leave the scan below green forever.
    expect(sources(SRC_DIR).map((file) => path.relative(SRC_DIR, file))).toEqual(
      expect.arrayContaining([
        "lib/viewers/file-asset-url.ts",
        "components/notes/editor/media-playback.ts",
        "components/notes/note-editor.tsx",
      ]),
    );
  });

  it("leaves every `keeper-…://` URL and every `…}://` composition to the helper", () => {
    const offenders = sources(SRC_DIR)
      .filter((file) => file !== HELPER)
      .filter((file) => SPELLING.test(code(readFileSync(file, "utf8"))))
      .map((file) => path.relative(SRC_DIR, file));
    expect(offenders).toEqual([]);
  });

  it("catches a component that spells one", () => {
    // The scan's own guard: a pattern that matched nothing would pass forever.
    expect(code('const src = "keeper-note://" + vault; // a URL')).toMatch(SPELLING);
    expect(code('const src = scheme + "://" + rest;')).toMatch(SPELLING);
    expect(code('const src = "http://" + scheme + ".localhost/" + rest;')).toMatch(SPELLING);
    expect(code("/** names `keeper-file://` in prose */ const x = 1;")).not.toMatch(SPELLING);
  });
});
