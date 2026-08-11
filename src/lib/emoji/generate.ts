#!/usr/bin/env bun
/**
 * Generates `src/lib/emoji/table.ts` — the `:shortcode:` vocabulary (Story
 * 45.11, AD-92).
 *
 * **Why a generator and not a hand-written table.** There are 1855 shortcodes.
 * Nobody maintains that by hand, nobody reviews a hand-edit to it, and the one
 * thing worse than an out-of-date table is a table that is out of date in a way
 * only one entry knows about. So the mapping is *derived*, the derivation is
 * this file, and its output is committed beside it. AD-92 also fixes the other
 * half: nothing calls the network at runtime, ever. The fetch happens here, on
 * a developer's machine, when someone runs this script on purpose.
 *
 * **Two sources, for one reason each.**
 *
 * - `README.md` of <https://github.com/ikatyang/emoji-cheat-sheet> decides
 *   WHICH shortcodes keeper knows. It is the vocabulary the epic names, it is
 *   what a person means by "the GitHub emoji", and it lags the raw API by a
 *   handful of very new entries — which is the correct side to err on for a
 *   vocabulary that ends up as literal characters inside other people's notes.
 * - <https://api.github.com/emojis> decides what each shortcode IS. The
 *   cheat sheet cannot: GitHub renders `:tada:` server-side, so the README
 *   contains the *name* 🎉 and never the character. The API answers with an
 *   image URL whose path carries the codepoints (`.../unicode/1f389.png`), and
 *   that is where the character comes from — the same derivation the cheat
 *   sheet's own `scripts/fetch.ts` performs.
 *
 * **A shortcode with no character is dropped, named, and counted.** GitHub's
 * custom emoji (`:octocat:`, `:shipit:`, `:trollface:` …) are PNGs on GitHub's
 * CDN with no Unicode behind them. A note is a markdown file on somebody's
 * drive; inserting a character that only github.com can render would put a
 * broken image in a file that has to survive without github.com.
 *
 * **Determinism is a property of the pure half.** `parseCheatSheet`,
 * `parseLiterals`, `buildTable` and `renderTable` are pure and total, sort
 * their output, and are the only things the tests touch. `main` is the only
 * part that reaches the network, and it is the only part a test cannot run.
 * Two runs over the same inputs produce byte-identical output — which is what
 * makes a re-generation reviewable as a diff instead of a re-shuffle.
 *
 * Run: `bun run gen:emoji`
 */
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The vocabulary. Raw, because the rendered page is HTML and the table is markdown. */
export const CHEAT_SHEET_URL =
  "https://raw.githubusercontent.com/ikatyang/emoji-cheat-sheet/master/README.md";

/** The characters. The cheat sheet is generated from this and so are we. */
export const GITHUB_EMOJI_API = "https://api.github.com/emojis";

/** Where the generated table lands, relative to the repository root. */
export const TABLE_PATH = "src/lib/emoji/table.ts";

/**
 * A shortcode as it appears between two colons.
 *
 * `+` and `-` are in the class because `:+1:` and `:-1:` are in the vocabulary;
 * without them the two most-used shortcodes on GitHub would be silently absent
 * and the count floor would still pass.
 */
const SHORTCODE_IN_BACKTICKS = /`:([a-z0-9_+-]+):`/g;

/**
 * Every shortcode the cheat sheet lists, in the order it lists them, deduped.
 *
 * The README writes each one inside backticks (`` `:tada:` ``) precisely so it
 * survives GitHub's own emoji rendering, which makes the backticks the only
 * reliable anchor on the page: the unquoted `:tada:` in the neighbouring column
 * is there to be rendered as a picture, and scraping that column would collect
 * the picture's name from some rows and nothing from others.
 */
export function parseCheatSheet(readme: string): string[] {
  const seen = new Set<string>();
  for (const match of readme.matchAll(SHORTCODE_IN_BACKTICKS)) {
    seen.add(match[1] as string);
  }
  return [...seen];
}

/**
 * Shortcode → the character it stands for, for every API entry that has one.
 *
 * The codepoints live in the asset path (`…/unicode/1f1e6-1f1e8.png?v8`),
 * hyphen-joined, and a URL without a `/unicode/` segment is one of GitHub's own
 * PNGs and has no character at all — it is left out of the map rather than
 * mapped to something, so the caller has to decide what to do about it.
 */
export function parseLiterals(api: Readonly<Record<string, string>>): Map<string, string> {
  const literals = new Map<string, string>();
  for (const [shortcode, url] of Object.entries(api)) {
    if (!url.includes("/unicode/")) {
      continue;
    }
    const file = url.split("/").pop() as string;
    const codepoints = (file.split(".png")[0] as string).split("-");
    let literal = "";
    for (const codepoint of codepoints) {
      const value = Number.parseInt(codepoint, 16);
      if (!Number.isFinite(value)) {
        literal = "";
        break;
      }
      literal += String.fromCodePoint(value);
    }
    if (literal !== "") {
      literals.set(shortcode, literal);
    }
  }
  return literals;
}

/** What one generation produced: the rows to write, and what it refused to write. */
export interface EmojiTableBuild {
  /** `[shortcode, character]`, sorted by shortcode in code-unit order. */
  readonly rows: ReadonlyArray<readonly [string, string]>;
  /** Cheat-sheet shortcodes with no Unicode character, sorted. */
  readonly dropped: readonly string[];
}

/**
 * The cheat sheet's vocabulary, joined to its characters and sorted.
 *
 * Sorted rather than kept in the cheat sheet's reading order because the
 * reading order is a *category* order that upstream reshuffles whenever Unicode
 * adds a subcategory, and a table whose diff is a reshuffle is a table nobody
 * reviews. The menu does its own ranking (`lib/emoji/match.ts`), so this order
 * is only ever read by a human.
 */
export function buildTable(
  shortcodes: readonly string[],
  literals: ReadonlyMap<string, string>,
): EmojiTableBuild {
  const rows: Array<readonly [string, string]> = [];
  const dropped: string[] = [];
  for (const shortcode of shortcodes) {
    const literal = literals.get(shortcode);
    if (literal === undefined) {
      dropped.push(shortcode);
      continue;
    }
    rows.push([shortcode, literal]);
  }
  rows.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
  dropped.sort();
  return { rows, dropped };
}

/**
 * The exact bytes of `table.ts`.
 *
 * Formatted the way biome would format it — two-space indent, double quotes,
 * trailing commas — so a re-generation is a data diff and never a formatting
 * diff, and so the generated file passes the same lint gate as everything else
 * rather than needing an exemption.
 */
export function renderTable(build: EmojiTableBuild): string {
  const lines: string[] = [
    "// Generated by src/lib/emoji/generate.ts — do not edit by hand.",
    "//",
    "// Vocabulary: https://github.com/ikatyang/emoji-cheat-sheet (README.md)",
    "// Characters: https://api.github.com/emojis (codepoints from the asset path)",
    "//",
    `// ${build.rows.length} shortcodes, sorted.`,
    "//",
    `// ${build.dropped.length} more are in the cheat sheet and deliberately absent: they are`,
    "// GitHub's own PNGs, with no Unicode character behind them, and a note is a markdown file",
    "// that has to survive without github.com.",
    ...wrapComment(build.dropped),
    "",
    "/**",
    " * `:shortcode:` → the character it inserts (Story 45.11, AD-92).",
    " *",
    " * An array of pairs rather than a record: a record with keys like `100` and",
    " * `1234` reorders itself, because JavaScript puts integer-like keys first, and",
    " * the whole point of a generated table is that its order is the generator's",
    " * decision and nobody else's.",
    " */",
    "export const EMOJI_TABLE: ReadonlyArray<readonly [shortcode: string, emoji: string]> = [",
  ];
  for (const [shortcode, emoji] of build.rows) {
    lines.push(`  [${JSON.stringify(shortcode)}, ${JSON.stringify(emoji)}],`);
  }
  lines.push("];", "");
  return lines.join("\n");
}

/**
 * The dropped shortcodes as comment lines inside biome's 100-column budget.
 *
 * Named in the file rather than counted, because "22 were dropped" tells the
 * next reader nothing and "`:octocat:` was dropped" tells them exactly why
 * their `:octocat:` does not complete.
 */
function wrapComment(names: readonly string[]): string[] {
  const lines: string[] = [];
  let current = "//";
  for (const name of names) {
    const candidate = `${current} ${name}`;
    if (candidate.length > 98) {
      lines.push(current);
      current = `// ${name}`;
    } else {
      current = candidate;
    }
  }
  lines.push(current);
  return lines;
}

async function main(): Promise<void> {
  // A User-Agent is required by api.github.com and is a courtesy to
  // raw.githubusercontent.com. This is a developer's machine talking to GitHub,
  // not the app: nothing here ships.
  const headers = { "User-Agent": "keeper/src/lib/emoji/generate.ts" };
  const [readme, api] = await Promise.all([
    fetch(CHEAT_SHEET_URL, { headers }).then((response) => {
      if (!response.ok) {
        throw new Error(`${CHEAT_SHEET_URL} answered ${response.status}`);
      }
      return response.text();
    }),
    fetch(GITHUB_EMOJI_API, { headers }).then((response) => {
      if (!response.ok) {
        throw new Error(`${GITHUB_EMOJI_API} answered ${response.status}`);
      }
      return response.json() as Promise<Record<string, string>>;
    }),
  ]);
  const shortcodes = parseCheatSheet(readme);
  if (shortcodes.length < 1000) {
    // The README's shape changed, or a proxy served something that is not it.
    // Writing a table with nine entries in it would be worse than not writing.
    throw new Error(`the cheat sheet yielded only ${shortcodes.length} shortcodes; refusing`);
  }
  const build = buildTable(shortcodes, parseLiterals(api));
  writeFileSync(TABLE_PATH, renderTable(build), "utf8");
  process.stdout.write(
    `${TABLE_PATH}: ${build.rows.length} shortcodes, ${build.dropped.length} without a character\n`,
  );
}

// `import.meta.main` is the house entry guard in `scripts/`, but `scripts/` is
// outside `tsconfig.json`'s `include` and this file is not: Bun's ImportMeta
// augmentation is not in the typecheck, so the comparison is spelled the way
// `src/test/no-user-agent-gating.test.ts` already spells it.
if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
