import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

// The guard is exercised on real bundles laid out on disk — a directory named
// `keeper.app` with the files a Tauri iOS or macOS build leaves in it — and on
// an `.ipa` zipped around one. The "executable" is any file carrying the
// strings the guard looks for; the point is the guard's arithmetic, not
// Mach-O parsing (AD-173).

const GUARD = join(process.cwd(), "scripts/check-bundle.sh");
const TAURI_CONF = "src-tauri/crates/keeper/tauri.conf.json";

/** `build.devUrl` from the project's own config, so the test cannot drift from it either. */
const DEV_URL: string = JSON.parse(readFileSync(TAURI_CONF, "utf8")).build.devUrl;

/** The module script Vite's built index.html names; also the embedded asset key. */
const ENTRY = "/assets/main-Bx7Qk2Lp.js";
const INDEX_HTML = `<!doctype html>
<html><head>
  <script type="module" crossorigin src="${ENTRY}"></script>
  <link rel="stylesheet" crossorigin href="/assets/main-Cq9d1zXe.css">
</head><body><div id="root"></div></body></html>
`;

/** A stand-in executable: NUL-separated runs, as `strings -a` will find them. */
function binary(...runs: string[]): Buffer {
  return Buffer.from(`\0junk\0${runs.join("\0")}\0`, "utf8");
}

const python3 = spawnSync("python3", ["--version"]).status === 0;

let root: string;
let n = 0;

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "keeper-bundle-guard-"));
});
afterAll(() => {
  rmSync(root, { recursive: true, force: true });
});

/** A fresh `<dir>/keeper.app` per fixture, so no case sees another's files. */
function app(): string {
  const dir = join(root, `case-${n++}`, "keeper.app");
  mkdirSync(dir, { recursive: true });
  return dir;
}

/** An iOS bundle exactly as `tauri ios build --export-method debugging` lays it out. */
function goodIos(): string {
  const dir = app();
  mkdirSync(join(dir, "assets/assets"), { recursive: true });
  writeFileSync(join(dir, "assets/index.html"), INDEX_HTML);
  writeFileSync(join(dir, "assets/assets/main-Bx7Qk2Lp.js"), "export {};\n");
  // A correct build carries the dev URL too: tauri-codegen compiles the whole
  // config into the binary. The guard must not refuse it for that.
  writeFileSync(join(dir, "keeper"), binary(ENTRY, DEV_URL));
  return dir;
}

function run(path: string, env: Record<string, string> = {}) {
  const r = spawnSync("bash", [GUARD, path], {
    encoding: "utf8",
    env: { ...process.env, ...env },
  });
  return { status: r.status, out: r.stdout, err: r.stderr };
}

describe("check-bundle.sh on an iOS .app", () => {
  it("passes a bundle whose assets/, index.html and embedded chunk are all present", () => {
    const r = run(goodIos());
    expect(r.err).toBe("");
    expect(r.status).toBe(0);
    expect(r.out).toContain(`embeds ${ENTRY}`);
  });

  it("refuses an empty assets/ with its own sentence and the correct recipe", () => {
    const dir = app();
    mkdirSync(join(dir, "assets"));
    writeFileSync(join(dir, "keeper"), binary(DEV_URL));
    const r = run(dir);
    expect(r.status).toBe(1);
    expect(r.err).toContain("assets is empty");
    expect(r.err).toContain("bun run tauri ios build --export-method debugging");
  });

  it("refuses a missing assets/ as empty rather than crashing", () => {
    const dir = app();
    writeFileSync(join(dir, "keeper"), binary(DEV_URL));
    const r = run(dir);
    expect(r.status).toBe(1);
    expect(r.err).toContain("assets is empty");
  });

  it("refuses an assets/ that has files but no index.html, with a different sentence", () => {
    const dir = app();
    mkdirSync(join(dir, "assets/assets"), { recursive: true });
    writeFileSync(join(dir, "assets/assets/main-Bx7Qk2Lp.js"), "export {};\n");
    writeFileSync(join(dir, "keeper"), binary(ENTRY, DEV_URL));
    const r = run(dir);
    expect(r.status).toBe(1);
    expect(r.err).toContain("has no index.html");
    expect(r.err).not.toContain("is empty");
  });

  it("refuses a binary that carries the dev-server URL and no frontend, naming the URL", () => {
    const dir = goodIos();
    writeFileSync(join(dir, "keeper"), binary(DEV_URL));
    const r = run(dir);
    expect(r.status).toBe(1);
    expect(r.err).toContain("embeds no frontend");
    expect(r.err).toContain(`pointed at ${DEV_URL}`);
    expect(r.err).not.toContain("is empty");
    expect(r.err).not.toContain("has no index.html");
  });

  it("reads the dev URL from tauri.conf.json, not from a copy", () => {
    // A config whose devUrl differs from the real one: the sentence must
    // follow the config, so a binary carrying the REAL URL is not "pointed at"
    // anything under the other config.
    const fakeRoot = join(root, "fake-root");
    mkdirSync(join(fakeRoot, "src-tauri/crates/keeper"), { recursive: true });
    writeFileSync(
      join(fakeRoot, "src-tauri/crates/keeper/tauri.conf.json"),
      readFileSync(TAURI_CONF, "utf8").replace(DEV_URL, "http://127.0.0.1:9999"),
    );
    const dir = goodIos();
    writeFileSync(join(dir, "keeper"), binary("http://127.0.0.1:9999/"));
    const r = run(dir, { KEEPER_REPO_ROOT: fakeRoot });
    expect(r.status).toBe(1);
    expect(r.err).toContain("pointed at http://127.0.0.1:9999");
  });

  it("refuses a binary that embeds no frontend even without the dev URL", () => {
    const dir = goodIos();
    writeFileSync(join(dir, "keeper"), binary("nothing of note"));
    const r = run(dir);
    expect(r.status).toBe(1);
    expect(r.err).toContain("embeds no frontend");
    expect(r.err).not.toContain("pointed at");
  });
});

describe("check-bundle.sh on a macOS .app", () => {
  // Tauri embeds the frontend in the desktop executable and writes nothing to
  // Contents/Resources, so the guard reads the chunk name from the dist/ the
  // build produced, under KEEPER_REPO_ROOT.
  let macRoot: string;
  beforeAll(() => {
    macRoot = join(root, "mac-root");
    mkdirSync(join(macRoot, "src-tauri/crates/keeper"), { recursive: true });
    cpSync(TAURI_CONF, join(macRoot, "src-tauri/crates/keeper/tauri.conf.json"));
    mkdirSync(join(macRoot, "dist"));
    writeFileSync(join(macRoot, "dist/index.html"), INDEX_HTML);
  });

  function macApp(bin: Buffer): string {
    const dir = app();
    mkdirSync(join(dir, "Contents/MacOS"), { recursive: true });
    mkdirSync(join(dir, "Contents/Resources"));
    writeFileSync(join(dir, "Contents/Resources/icon.icns"), "");
    writeFileSync(join(dir, "Contents/MacOS/keeper"), bin);
    return dir;
  }

  it("passes a bundle whose executable embeds the chunk dist/index.html names", () => {
    const r = run(macApp(binary(ENTRY, DEV_URL)), { KEEPER_REPO_ROOT: macRoot });
    expect(r.err).toBe("");
    expect(r.status).toBe(0);
  });

  it("refuses a --debug executable and points at tauri:build:signed", () => {
    const r = run(macApp(binary(DEV_URL)), { KEEPER_REPO_ROOT: macRoot });
    expect(r.status).toBe(1);
    expect(r.err).toContain("embeds no frontend");
    expect(r.err).toContain(`pointed at ${DEV_URL}`);
    expect(r.err).toContain("bun run tauri:build:signed");
  });

  it("refuses rather than passes when there is no dist/ to check against", () => {
    const bare = join(root, "bare-root");
    mkdirSync(join(bare, "src-tauri/crates/keeper"), { recursive: true });
    cpSync(TAURI_CONF, join(bare, "src-tauri/crates/keeper/tauri.conf.json"));
    const r = run(macApp(binary(ENTRY, DEV_URL)), { KEEPER_REPO_ROOT: bare });
    expect(r.status).toBe(1);
    expect(r.err).toContain("dist/index.html does not exist");
  });

  it("checks a bare executable the same way (CI's --no-bundle build)", () => {
    const bin = join(root, "keeper-bare");
    writeFileSync(bin, binary(DEV_URL));
    const r = run(bin, { KEEPER_REPO_ROOT: macRoot });
    expect(r.status).toBe(1);
    expect(r.err).toContain("embeds no frontend");
    expect(r.err).toContain("bun run tauri:build");
  });

  // The defect this test exists for, measured on hesperia 2026-09-04: the guard
  // refused a perfectly good 136 MB signed release build. `strings -a … | grep
  // -qF` quits at the first hit, `strings` dies of SIGPIPE, and under `set -o
  // pipefail` the pipeline yields 141, which the guard read as "the chunk is
  // absent". Every fixture above is small enough that `strings` finishes before
  // `grep` quits, so none of them could ever have caught it. This one carries
  // the chunk near the FRONT and megabytes of padding behind it, which is the
  // shape that makes an early-exiting reader race its producer.
  it("passes a large executable whose chunk sits early, where an early-exiting grep would race", () => {
    const padding = Buffer.alloc(24 * 1024 * 1024, 0x41);
    const big = Buffer.concat([binary(ENTRY, DEV_URL), padding]);
    const r = run(macApp(big), { KEEPER_REPO_ROOT: macRoot });
    expect(r.err).toBe("");
    expect(r.status).toBe(0);
  });
});

describe.skipIf(!python3)("check-bundle.sh on an .ipa", () => {
  function ipa(appDir: string): string {
    // `Payload/keeper.app` is what Xcode's export writes; python3's zipfile is
    // the zip writer this host has.
    const stage = join(root, `ipa-${n++}`);
    mkdirSync(join(stage, "Payload"), { recursive: true });
    cpSync(appDir, join(stage, "Payload/keeper.app"), { recursive: true });
    const out = join(stage, "keeper.ipa");
    const z = spawnSync("python3", ["-m", "zipfile", "-c", out, "Payload"], {
      cwd: stage,
      encoding: "utf8",
    });
    expect(z.status).toBe(0);
    return out;
  }

  it("passes a correct IPA", () => {
    const r = run(ipa(goodIos()));
    expect(r.err).toBe("");
    expect(r.status).toBe(0);
  });

  it("refuses an IPA whose bundle has an empty assets/", () => {
    const dir = app();
    mkdirSync(join(dir, "assets"));
    writeFileSync(join(dir, "keeper"), binary(DEV_URL));
    const r = run(ipa(dir));
    expect(r.status).toBe(1);
    expect(r.err).toContain("assets is empty");
  });

  it("refuses a file that is not a zip", () => {
    const fake = join(root, "not-a-zip.ipa");
    writeFileSync(fake, "hello");
    const r = run(fake);
    expect(r.status).toBe(1);
    expect(r.err).toContain("is not a zip archive");
  });
});
