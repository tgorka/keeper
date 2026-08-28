/**
 * The generator is pure where it matters (Story 45.11, AD-92).
 *
 * Nothing here touches the network. `main` is the only part of `generate.ts`
 * that does, and it is deliberately the only part with no test: a test that
 * fetched github.com would be a test that fails on a train, and the whole point
 * of AD-92 is that the fetch happens once, on purpose, on a developer's machine.
 *
 * What IS tested is everything a regeneration can get wrong quietly: the parse
 * anchor, the codepoint arithmetic, what happens to a shortcode with no
 * character, and — the one that makes the committed table reviewable — that two
 * runs over the same inputs produce byte-identical output.
 */
import { describe, expect, it } from "vitest";
import { buildTable, parseCheatSheet, parseLiterals, renderTable } from "./generate";

/**
 * A slice of the real README, kept verbatim including its noise.
 *
 * The unquoted `:grinning:` in the middle column and the `[top](#…)` links are
 * both in the real page and are both things a careless anchor would collect, so
 * they stay in the fixture doing their job.
 */
const README = `# emoji-cheat-sheet

#### Face Smiling

| ​ | ​ | ​ | ​ | ​ | ​ |
| --- | --- | --- | --- | --- | --- |
| [top](#smileys--emotion) | :grinning: | \`:grinning:\` | :smile: | \`:smile:\` | [top](#table-of-contents) |
| [top](#people--body) | :+1: | \`:+1:\` <br /> \`:thumbsup:\` | :-1: | \`:-1:\` | [top](#table-of-contents) |
| [top](#flags) | :jp: | \`:jp:\` | :octocat: | \`:octocat:\` | [top](#table-of-contents) |
`;

/** Real URLs, in the real shape, cache-buster and all. */
const API: Record<string, string> = {
  grinning: "https://github.githubassets.com/images/icons/emoji/unicode/1f600.png?v8",
  smile: "https://github.githubassets.com/images/icons/emoji/unicode/1f604.png?v8",
  "+1": "https://github.githubassets.com/images/icons/emoji/unicode/1f44d.png?v8",
  thumbsup: "https://github.githubassets.com/images/icons/emoji/unicode/1f44d.png?v8",
  "-1": "https://github.githubassets.com/images/icons/emoji/unicode/1f44e.png?v8",
  jp: "https://github.githubassets.com/images/icons/emoji/unicode/1f1ef-1f1f5.png?v8",
  octocat: "https://github.githubassets.com/images/icons/emoji/octocat.png?v8",
  // A custom PNG whose NAME is valid hexadecimal. `octocat` alone cannot prove
  // the `/unicode/` rule is doing anything, because parsing it as hex fails by
  // accident; `cafe` parses cleanly to U+CAFE and would sail through as a
  // Korean syllable if the rule were removed. Two of GitHub's real custom
  // emoji already parse partially this way (`electron` → U+000E).
  cafe: "https://github.githubassets.com/images/icons/emoji/cafe.png?v8",
  // A `/unicode/` path that is not codepoints. Not a shape GitHub serves today,
  // which is exactly why the arithmetic must refuse it rather than trust it.
  broken: "https://github.githubassets.com/images/icons/emoji/unicode/notahex.png?v8",
  unlisted: "https://github.githubassets.com/images/icons/emoji/unicode/1f989.png?v8",
};

describe("parseCheatSheet", () => {
  it("reads the backticked column and never the rendered one", () => {
    // The middle column exists to be turned into a picture by GitHub. Anchoring
    // on backticks is what makes the parse survive that.
    expect(parseCheatSheet(README)).toEqual([
      "grinning",
      "smile",
      "+1",
      "thumbsup",
      "-1",
      "jp",
      "octocat",
    ]);
  });

  it("keeps `+1` and `-1`, the two the character class could silently drop", () => {
    expect(parseCheatSheet(README)).toContain("+1");
    expect(parseCheatSheet(README)).toContain("-1");
  });

  it("says nothing rather than something when handed a page that is not the cheat sheet", () => {
    // A 404 page, an error from a proxy, a redirect notice. The generator's own
    // floor check turns this into a refusal; the parse's job is to not invent.
    expect(parseCheatSheet("<html><body>404: Not Found</body></html>")).toEqual([]);
  });
});

describe("parseLiterals", () => {
  it("derives the character from the codepoints in the asset path", () => {
    expect(parseLiterals(API).get("grinning")).toBe("😀");
  });

  it("joins a multi-codepoint sequence instead of taking the first", () => {
    expect(parseLiterals(API).get("jp")).toBe("🇯🇵");
  });

  it("leaves out an emoji GitHub renders as its own PNG", () => {
    // `:octocat:` has no Unicode behind it. Mapping it to *something* is the
    // failure this guards: a note is a file that has to survive without GitHub.
    expect(parseLiterals(API).has("octocat")).toBe(false);
  });

  it("leaves out a custom PNG even when its name reads as hexadecimal", () => {
    // The `/unicode/` segment is the rule; the codepoint arithmetic is not a
    // second one. `cafe.png` parses perfectly well as U+CAFE, so without the
    // rule this shortcode would insert 쫾 into somebody's note.
    expect(parseLiterals(API).has("cafe")).toBe(false);
  });

  it("refuses a unicode path that is not codepoints", () => {
    // Not a shape GitHub serves. If it ever becomes one, a table full of
    // control characters is worse than a generator that produced nothing.
    expect(parseLiterals(API).has("broken")).toBe(false);
  });

  it("maps two shortcodes that share a character to the same character", () => {
    const literals = parseLiterals(API);

    expect(literals.get("+1")).toBe("👍");
    expect(literals.get("thumbsup")).toBe("👍");
  });
});

describe("buildTable", () => {
  it("keeps the cheat sheet's vocabulary and no more", () => {
    // `unlisted` has a perfectly good character and is not in the cheat sheet.
    // The cheat sheet decides the vocabulary, so it stays out.
    const { rows } = buildTable(parseCheatSheet(README), parseLiterals(API));

    expect(rows.map(([shortcode]) => shortcode)).not.toContain("unlisted");
  });

  it("names what it dropped rather than counting it", () => {
    const { rows, dropped } = buildTable(parseCheatSheet(README), parseLiterals(API));

    expect(dropped).toEqual(["octocat"]);
    expect(rows.map(([shortcode]) => shortcode)).not.toContain("octocat");
  });

  it("sorts, so the committed file's order is the generator's decision", () => {
    const { rows } = buildTable(parseCheatSheet(README), parseLiterals(API));

    expect(rows.map(([shortcode]) => shortcode)).toEqual([
      "+1",
      "-1",
      "grinning",
      "jp",
      "smile",
      "thumbsup",
    ]);
  });
});

describe("the generated bytes", () => {
  it("are identical when the generator runs twice", () => {
    // The acceptance condition for a checked-in generated file: a regeneration
    // that changed nothing must produce an empty diff, or nobody can tell a
    // real change from a re-run.
    const once = renderTable(buildTable(parseCheatSheet(README), parseLiterals(API)));
    const twice = renderTable(buildTable(parseCheatSheet(README), parseLiterals(API)));

    expect(twice).toBe(once);
  });

  it("are identical when the cheat sheet reorders its own sections", () => {
    // Upstream reshuffles its categories whenever Unicode adds a subcategory.
    // Sorting is what keeps that from landing as an 1855-line diff.
    const shuffled = [...parseCheatSheet(README)].reverse();
    const inOrder = renderTable(buildTable(parseCheatSheet(README), parseLiterals(API)));

    expect(renderTable(buildTable(shuffled, parseLiterals(API)))).toBe(inOrder);
  });

  it("say where they came from and what they left out", () => {
    const rendered = renderTable(buildTable(parseCheatSheet(README), parseLiterals(API)));

    expect(rendered).toContain("do not edit by hand");
    expect(rendered).toContain("https://github.com/ikatyang/emoji-cheat-sheet");
    expect(rendered).toContain("octocat");
  });

  it("are formatted the way the repository is formatted", () => {
    // A generated file that needs a lint exemption is a generated file that
    // stops being regenerated.
    const rendered = renderTable(buildTable(parseCheatSheet(README), parseLiterals(API)));

    expect(rendered).toContain('  ["+1", "👍"],\n');
    expect(rendered.endsWith("];\n")).toBe(true);
    for (const line of rendered.split("\n")) {
      expect(line.length, line).toBeLessThanOrEqual(100);
    }
  });

  it("parse back into the vocabulary they were built from", () => {
    // The strongest statement available without evaluating the file: every row
    // the build produced is present, spelled as a quoted pair.
    const build = buildTable(parseCheatSheet(README), parseLiterals(API));
    const rendered = renderTable(build);

    for (const [shortcode, emoji] of build.rows) {
      expect(rendered).toContain(`  [${JSON.stringify(shortcode)}, ${JSON.stringify(emoji)}],`);
    }
  });
});
