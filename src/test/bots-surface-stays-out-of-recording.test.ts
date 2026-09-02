/**
 * Repo-wide convention test (Story 61.13): the bots surface stays out of every
 * path the recording zero-egress gate scans.
 *
 * Why this exists. The recording feature promises, in six places, that it
 * uploads, shares and transcribes nothing — and `zero-egress.test.ts` keeps
 * that promise literally true by reading the recording sources off disk and
 * failing on any network or affordance token. Epic 61 puts a surface that talks
 * to the network *on purpose* beside it: a chat pane, a `fetch`-shaped wire in
 * Rust, "Upload"-adjacent vocabulary around pasted images. The gate is left
 * untouched and unwidened, which is only honest while no bots file can land
 * inside its globs — otherwise the gate either cries wolf on a legitimate chat
 * call or, worse, gets a token loosened to let one through.
 *
 * Two halves, both mechanical:
 *  (a) no file of the bots surface is in the set the gate scans today, and
 *  (b) no bots file is named or placed so that a scanDir rule would pick it up,
 *      and none spells the gate's own directory as a location — the way a
 *      component later "moved next to its caller" would end up scanned.
 *
 * The gate's glob list is NOT copied here. `recordingSources()` is re-read from
 * the gate's own source text and its `scanDir(...)` / `join(SRC, "...")` calls
 * are interpreted, so a rule added to the gate is a rule enforced here on the
 * same run. The interpreter is strict about the shape it understands and fails
 * loudly on anything else, so a refactor of the gate breaks this test rather
 * than silently narrowing it.
 */
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { basename, dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/** The frontend source root (`src/`). */
const SRC = resolve(dirname(fileURLToPath(import.meta.url)), "..");
/** The recording gate, whose `recordingSources()` is the only source of the globs. */
const GATE = join(SRC, "components/recording/zero-egress.test.ts");
/** The gate's `HERE`: its own directory, which it scans for every `.tsx`. */
const GATE_DIR = dirname(GATE);

/** One `scanDir(dir, predicate)` rule of the gate, with the predicate compiled. */
interface ScanRule {
  dir: string;
  /** The predicate's source text, for messages. */
  text: string;
  matches: (name: string) => boolean;
}

/** The gate's scan surface: directory rules plus explicitly named files. */
interface GateGlobs {
  rules: ScanRule[];
  files: string[];
}

/**
 * The predicate grammar the gate uses: `name.startsWith("x")` / `name.endsWith("y")`
 * clauses joined by `&&`, and nothing else. Anchoring the scanDir regex on this
 * grammar (rather than on a lazy `.+?`) is what keeps a trailing-comma call
 * from being read as a shorter predicate; a clause outside the grammar fails to
 * match at all, and the item-count check below reports it.
 */
const CLAUSE = String.raw`name\.(?:startsWith|endsWith)\("[^"]+"\)`;
const PREDICATE = `(?:${CLAUSE}(?: && )?)+`;

/** Compile a predicate in the grammar above without evaluating source. */
function compilePredicate(text: string): (name: string) => boolean {
  const checks = [...text.matchAll(/name\.(startsWith|endsWith)\("([^"]+)"\)/g)].map(
    ([, method, literal]) =>
      method === "startsWith"
        ? (name: string) => name.startsWith(literal)
        : (name: string) => name.endsWith(literal),
  );
  expect(checks.length, `an empty predicate in ${text}`).toBeGreaterThan(0);
  return (name) => checks.every((check) => check(name));
}

/** Read `recordingSources()` out of the gate and interpret its calls. */
function readGateGlobs(): GateGlobs {
  const source = readFileSync(GATE, "utf8");
  const body = /function recordingSources\(\): string\[\] \{([\s\S]*?)\n\}/.exec(source)?.[1];
  if (!body) {
    throw new Error(`could not find recordingSources() in ${GATE}`);
  }
  // One line, single-spaced, so a call split over several lines reads as one.
  const flat = body.replace(/\s+/g, " ");

  const rules: ScanRule[] = [];
  const scanDirCall = new RegExp(
    String.raw`scanDir\( ?(HERE|join\(SRC, "([^"]+)"\)), ?\(name\) => (${PREDICATE}),? ?\)`,
    "g",
  );
  for (const m of flat.matchAll(scanDirCall)) {
    const [, dirExpr, subdir, predicate] = m;
    rules.push({
      dir: dirExpr === "HERE" ? GATE_DIR : join(SRC, subdir),
      text: predicate,
      matches: compilePredicate(predicate),
    });
  }

  // Explicit files are the `join(SRC, "...")` calls that are NOT a scanDir's dir.
  const withoutScanDirs = flat.replace(scanDirCall, "");
  const files = [...withoutScanDirs.matchAll(/join\(SRC, "([^"]+)"\)/g)].map(([, rel]) =>
    join(SRC, rel),
  );

  // Every list item must have been understood as one or the other, or the
  // interpreter has fallen behind the gate.
  const items = flat.split("...").length - 1 + files.length;
  expect(items, `gate list items not all interpreted: ${flat}`).toBe(rules.length + files.length);
  return { rules, files };
}

/** The concrete set the gate reads today, by the gate's own `scanDir` semantics. */
function scannedToday(globs: GateGlobs): Set<string> {
  const out = new Set<string>(globs.files);
  for (const rule of globs.rules) {
    for (const name of readdirSync(rule.dir)) {
      if (rule.matches(name) && !name.endsWith(".test.ts") && !name.endsWith(".test.tsx")) {
        out.add(join(rule.dir, name));
      }
    }
  }
  return out;
}

/**
 * The bots surface, by namespace rather than by list — the way the gate itself
 * namespaces recording: every production `.ts`/`.tsx` under `src/` whose path
 * has a `bots` directory or a `bot-`/`bots-`/`bots.`/`use-bots` basename.
 */
function botsSurface(dir: string = SRC): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...botsSurface(full));
      continue;
    }
    if (!/\.tsx?$/.test(entry.name) || /\.test\.tsx?$/.test(entry.name)) {
      continue;
    }
    const segments = full.slice(SRC.length + 1).split(sep);
    const inBotsDir = segments.slice(0, -1).includes("bots");
    const botsName = /^(use-)?bots?[-.]/.test(entry.name);
    if (inBotsDir || botsName) {
      files.push(full);
    }
  }
  return files;
}

describe("the bots surface stays out of the recording zero-egress gate (Story 61.13)", () => {
  const globs = readGateGlobs();
  const bots = botsSurface();

  it("reads the gate's globs from the gate itself, not from a copy", () => {
    expect(existsSync(GATE), "the recording gate moved; this test follows it").toBe(true);
    // The gate has one rule over its own directory and at least the three
    // prefix rules its doc comment lists; fewer means the interpreter missed one.
    expect(globs.rules.some((rule) => rule.dir === GATE_DIR)).toBe(true);
    expect(globs.rules.length).toBeGreaterThanOrEqual(4);
    expect(globs.files.length).toBeGreaterThanOrEqual(3);
    // The one shared file the epic touched must be in the interpreted set, or
    // the interpretation is not the gate's list.
    expect(globs.files).toContain(join(SRC, "components/command-palette/actions.ts"));
    expect(scannedToday(globs).size, "the gate's own non-vacuity floor").toBeGreaterThan(5);
  });

  it("(a) no bots surface file is in the set the gate scans today", () => {
    expect(bots.length, "the bots surface must never be scanned vacuously").toBeGreaterThan(10);
    const scanned = scannedToday(globs);
    const overlap = bots.filter((file) => scanned.has(file));
    expect(
      overlap,
      "a bots file is inside the recording zero-egress scan; the gate would now read a " +
        "network-talking surface as a recording egress, or be widened to excuse it",
    ).toEqual([]);
  });

  it("(b) no bots file is placed or named so a gate rule would claim it, and none spells the gate's directory", () => {
    // The prefixes the gate keys on, taken from its rules: `recording-` and
    // `use-record` today. A bots file carrying one is one move away from being
    // scanned — a rename into `src/lib/` would do it.
    const gatePrefixes = globs.rules.flatMap((rule) =>
      [...rule.text.matchAll(/startsWith\("([^"]+)"\)/g)].map(([, p]) => p),
    );
    expect(gatePrefixes.length).toBeGreaterThan(0);
    // The gate's own directory is the one glob that claims EVERY `.tsx` in it,
    // so it is the one location a bots component can be moved into and be
    // swallowed silently. A bots file that spells that path — a lazy import, a
    // barrel, a route — is pointing at where it must never live. (An import
    // *from* a recording module elsewhere is not this hazard: the gate reads
    // text, and a bots file outside its globs stays unread whatever it imports.)
    const recordingDir = `${GATE_DIR.slice(SRC.length + 1)
      .split(sep)
      .join("/")}/`;

    for (const file of bots) {
      const name = basename(file);
      const dir = dirname(file);
      for (const rule of globs.rules) {
        expect(
          dir === rule.dir && rule.matches(name),
          `${file} would be claimed by the gate rule scanDir(${rule.dir}, ${rule.text})`,
        ).toBe(false);
      }
      expect(globs.files.includes(file), `${file} is named by the gate`).toBe(false);
      expect(
        gatePrefixes.some((prefix) => name.startsWith(prefix)),
        `${name} carries a recording prefix the gate keys on`,
      ).toBe(false);
      expect(
        readFileSync(file, "utf8").includes(recordingDir),
        `${file} spells "${recordingDir}" — the gate scans every .tsx there, so the bots surface ` +
          "must never point at it as a location",
      ).toBe(false);
    }
  });
});
