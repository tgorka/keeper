/**
 * Hints inside a `keeper-media` body, over the real markdown grammar: which
 * keys and values are offered where the caret is, and where Rust's refusal
 * lands in the document. The grammar here is a stand-in for
 * `media_block_schema`'s answer with the shape Rust gives it.
 */
import {
  autocompletion,
  CompletionContext,
  type CompletionResult,
  currentCompletions,
  startCompletion,
} from "@codemirror/autocomplete";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  FilesEntryVm,
  FilesListingVm,
  MediaBlockProblemVm,
  MediaBlockSchemaVm,
  MediaKeyVm,
  RecordingHitVm,
} from "@/lib/ipc/client";
import { mediaFences } from "./media-block";
import {
  type MediaHintSources,
  mediaBlockCompleteSource,
  mediaBlockDiagnostics,
  problemRange,
} from "./media-hints";

const key = (
  name: string,
  place: MediaKeyVm["place"],
  value: MediaKeyVm["value"],
  values: string[] = [],
  source = false,
): MediaKeyVm => ({ key: name, place, value, values, source, doc: `${name} doc` });

const SCHEMA: MediaBlockSchemaVm = {
  keys: [
    key("session", "root", "session", [], true),
    key("transcript", "root", "transcript", [], true),
    key("part", "root", "tables", [], true),
    key("src", "root", "config", [], true),
    key("record", "root", "choice", ["new"], true),
    key("title", "root", "text"),
    key("from", "root", "time"),
    key("to", "root", "time"),
    key("picture", "root", "choice", ["screen", "camera", "both"]),
    key("sound", "root", "choice", ["system", "microphone", "both"]),
    key("marker", "root", "tables"),
    key("version", "root", "version"),
    key("file", "part", "media"),
    key("camera", "part", "video"),
    key("offset", "part", "time"),
    key("system", "part", "track"),
    key("microphone", "part", "track"),
    key("name", "marker", "text"),
    key("at", "marker", "time"),
    key("from", "marker", "time"),
    key("to", "marker", "time"),
  ],
};

const entry = (name: string, kind: FilesEntryVm["kind"], relativePath: string): FilesEntryVm =>
  ({ name, kind, relativePath }) as FilesEntryVm;

const LISTINGS: Record<string, FilesEntryVm[]> = {
  "": [entry("meetings", "folder", "meetings"), entry("notes.md", "file", "notes.md")],
  meetings: [
    entry("kelly.mp4", "video", "meetings/kelly.mp4"),
    entry("kelly.m4a", "audio", "meetings/kelly.m4a"),
    entry("kelly.mp4.transcript.json", "file", "meetings/kelly.mp4.transcript.json"),
    entry("kelly.toml", "file", "meetings/kelly.toml"),
    entry("old", "folder", "meetings/old"),
  ],
};

const HIT = {
  sessionId: "01J8D-01J8KELLY",
  relativePath: "recordings/kelly",
  title: "Kelly sync",
  startedTs: Date.UTC(2026, 8, 28, 12),
} as RecordingHitVm;

function sources(over: Partial<MediaHintSources> = {}): MediaHintSources {
  return {
    schema: async () => SCHEMA,
    recordings: async () => [HIT],
    files: async (subpath) =>
      ({ state: "listed", entries: LISTINGS[subpath] ?? [] }) as unknown as FilesListingVm,
    ...over,
  };
}

/** A note holding one block whose body is `body`; `|` marks the caret. */
function note(body: string): { state: EditorState; pos: number } {
  const text = `# Weekly\n\n\`\`\`keeper-media\n${body}\n\`\`\`\n\nafter\n`;
  const pos = text.indexOf("|");
  const doc = pos < 0 ? text : text.slice(0, pos) + text.slice(pos + 1);
  return {
    state: EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] }),
    pos,
  };
}

async function complete(
  body: string,
  { explicit = true, over = {} }: { explicit?: boolean; over?: Partial<MediaHintSources> } = {},
): Promise<CompletionResult | null> {
  const { state, pos } = note(body);
  return mediaBlockCompleteSource(sources(over))(new CompletionContext(state, pos, explicit));
}

const labels = (result: CompletionResult | null) => result?.options.map((option) => option.label);

describe("keys, by where the caret is", () => {
  it("offers only the root keys not yet written, and no second source", async () => {
    const result = await complete('session = "x"\n|\n');
    expect(labels(result)).toEqual([
      "title",
      "from",
      "to",
      "picture",
      "sound",
      "[[marker]]",
      "version",
    ]);
    expect(result?.options.find((option) => option.label === "picture")?.info).toBe("picture doc");
  });

  it("offers every source, and both tables, in a block that names none yet", async () => {
    const result = await complete('title = "Kelly"\n|');
    expect(labels(result)).toEqual(
      expect.arrayContaining(["session", "transcript", "src", "record", "[[part]]", "[[marker]]"]),
    );
    expect(labels(result)).not.toContain("title");
  });

  it("narrows to what is typed", async () => {
    expect(labels(await complete("pi|", { explicit: false }))).toEqual(["picture"]);
    expect(labels(await complete("[[m|", { explicit: false }))).toEqual(["[[marker]]"]);
  });

  it("offers a [[marker]]'s keys under a marker header, minus those written above or below", async () => {
    const result = await complete(
      'session = "x"\ntitle = "t"\n\n[[marker]]\nname = "Intro"\n|\nto = "00:00:05"\n\n[[marker]]\nat = "1"',
    );
    const offered = labels(result) ?? [];
    expect(offered.filter((label) => !label.startsWith("[["))).toEqual(["at", "from"]);
    expect(offered).toContain("[[marker]]");
    // The root names its source, so a part would be a second one.
    expect(offered).not.toContain("[[part]]");
  });

  it("offers a [[part]]'s keys under a part, and no root source beside parts", async () => {
    const part = labels(await complete('[[part]]\nfile = "a.mp4"\n|'));
    expect(part).toEqual(expect.arrayContaining(["camera", "offset", "system", "microphone"]));
    expect(part).not.toContain("file");
    expect(part).not.toContain("title");
    const root = labels(await complete('|\n[[part]]\nfile = "a.mp4"'));
    expect(root).not.toContain("session");
    expect(root).toContain("title");
    expect(root).toContain("[[part]]");
  });

  it("stays quiet on an empty line until asked, and outside every block", async () => {
    expect(await complete('session = "x"\n|', { explicit: false })).toBeNull();
    const { state } = note('session = "x"');
    const source = mediaBlockCompleteSource(sources());
    expect(await source(new CompletionContext(state, state.doc.length, true))).toBeNull();
    expect(await source(new CompletionContext(state, 3, true))).toBeNull();
  });
});

describe("values, by key", () => {
  it("offers a choice's values, replacing the typed quote through the closing one", async () => {
    const { state, pos } = note('session = "x"\nsound = "mi|"');
    const result = await mediaBlockCompleteSource(sources())(
      new CompletionContext(state, pos, false),
    );
    expect(labels(result)).toEqual(['"microphone"']);
    expect(result?.from).toBe(pos - 3);
    expect(result?.to).toBe(pos + 1);
  });

  it("offers every choice right after the equals sign", async () => {
    expect(labels(await complete('session = "x"\npicture = |', { explicit: false }))).toEqual([
      '"screen"',
      '"camera"',
      '"both"',
    ]);
  });

  it("offers the player's time first, when the block had one, and the start", async () => {
    const playerTime = vi.fn(() => 3723.9);
    const { state, pos } = note('session = "x"\nfrom = |');
    const parent = document.body.appendChild(document.createElement("div"));
    const view = new EditorView({ state, parent });
    const result = await mediaBlockCompleteSource(sources({ playerTime }))(
      new CompletionContext(state, pos, true, view),
    );
    view.destroy();
    expect(labels(result)).toEqual(['"01:02:03"', '"00:00:00"']);
    expect(playerTime).toHaveBeenCalledWith(view, mediaFences(state)[0].from);
    expect(labels(await complete('session = "x"\nfrom = |'))).toEqual(['"00:00:00"']);
  });

  it("offers recordings as title · date and writes the identity", async () => {
    const recordings = vi.fn(async () => [HIT]);
    const result = await complete('session = "kel|', { over: { recordings } });
    expect(recordings).toHaveBeenCalledWith("kel");
    const option = result?.options[0];
    expect(option?.label).toBe(`Kelly sync · ${new Date(HIT.startedTs ?? 0).toLocaleDateString()}`);
    expect(option?.apply).toBe(`"${HIT.sessionId}"`);
  });

  it("asks keeper once while a value grows, and narrows the answer as it is typed", async () => {
    const recordings = vi.fn(async () => [
      HIT,
      {
        ...HIT,
        sessionId: "01J8D-01J8RETRO",
        relativePath: "recordings/retro",
        title: "Retro",
      } as RecordingHitVm,
    ]);
    const source = mediaBlockCompleteSource(sources({ recordings }));
    const at = (body: string) => {
      const { state, pos } = note(body);
      return source(new CompletionContext(state, pos, false));
    };

    expect(labels(await at('session = "|'))).toHaveLength(2);
    const grown = await at('session = "kel|');

    expect(recordings).toHaveBeenCalledTimes(1);
    expect(grown?.options.map((option) => option.apply)).toEqual([`"${HIT.sessionId}"`]);
  });

  it("lists the typed folder of the drive, keeping only the files the key takes", async () => {
    expect(labels(await complete('transcript = "|'))).toEqual(["meetings/"]);
    expect(labels(await complete('transcript = "meetings/|'))).toEqual([
      "kelly.mp4.transcript.json",
      "old/",
    ]);
    expect(labels(await complete('src = "meetings/k|'))).toEqual(["kelly.toml"]);
    expect(labels(await complete('[[part]]\nfile = "meetings/|'))).toEqual([
      "kelly.mp4",
      "kelly.m4a",
      "old/",
    ]);
    expect(labels(await complete('[[part]]\nfile = "a.mp4"\ncamera = "meetings/|'))).toEqual([
      "kelly.mp4",
      "old/",
    ]);
  });

  it("offers nothing for a key the caret's table does not have", async () => {
    expect(await complete('session = "x"\nfile = "|')).toBeNull();
  });
});

describe("accepting a key", () => {
  const views: EditorView[] = [];
  afterEach(() => {
    for (const view of views.splice(0)) view.destroy();
  });

  it('writes `key = ""` with the caret between the quotes and opens the values', async () => {
    const { state, pos } = note('session = "x"\npic|');
    const parent = document.body.appendChild(document.createElement("div"));
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: state.doc,
        selection: { anchor: pos },
        extensions: [
          markdown({ base: markdownLanguage }),
          autocompletion({ override: [mediaBlockCompleteSource(sources())], interactionDelay: 0 }),
        ],
      }),
    });
    views.push(view);
    startCompletion(view);
    await vi.waitFor(() => expect(currentCompletions(view.state)).toHaveLength(1));
    const [option] = currentCompletions(view.state);
    const apply = option.apply;
    if (typeof apply !== "function") throw new Error("a key applies itself");
    apply(view, option, pos - 3, pos);
    const line = view.state.doc.lineAt(view.state.selection.main.head);
    expect(line.text).toBe('picture = ""');
    expect(view.state.selection.main.head).toBe(line.to - 1);
    await vi.waitFor(() =>
      expect(currentCompletions(view.state).map((each) => each.label)).toEqual([
        '"screen"',
        '"camera"',
        '"both"',
      ]),
    );
  });
});

describe("where a refusal lands", () => {
  const problem = (over: Partial<MediaBlockProblemVm>): MediaBlockProblemVm => ({
    message: "This block has `form`, which is not a keeper-media key.",
    line: 2,
    from: 0,
    to: 4,
    ...over,
  });
  const { state } = note('session = "x"\nform = "00:01:00"\n🎬 = 1');
  const fence = mediaFences(state)[0];
  const text = (range: { from: number; to: number }) => state.doc.sliceString(range.from, range.to);

  it("puts the body's line and columns on the document", () => {
    const range = problemRange(state, fence, problem({}));
    expect(text(range)).toBe("form");
    expect(range.message).toBe("This block has `form`, which is not a keeper-media key.");
  });

  it("counts columns in UTF-16 units, as Rust hands them", () => {
    expect(text(problemRange(state, fence, problem({ line: 3, from: 0, to: 2 })))).toBe("🎬");
  });

  it("underlines the whole line for an empty or overlong range", () => {
    expect(text(problemRange(state, fence, problem({ from: 3, to: 3 })))).toBe('form = "00:01:00"');
    expect(text(problemRange(state, fence, problem({ from: 7, to: 400 })))).toBe('"00:01:00"');
  });

  it("puts a problem of the whole block, or past its body, on the opening fence", () => {
    expect(text(problemRange(state, fence, problem({ line: null })))).toBe("```keeper-media");
    expect(text(problemRange(state, fence, problem({ line: 9 })))).toBe("```keeper-media");
  });
});

describe("the underline in a view", () => {
  it("marks what Rust refused, asks again only for a changed body, and clears when it reads", async () => {
    const check = vi.fn(
      async (source: string): Promise<MediaBlockProblemVm | null> =>
        source.includes("form")
          ? {
              message: "This block has `form`, which is not a keeper-media key.",
              line: 2,
              from: 0,
              to: 4,
            }
          : null,
    );
    const { state } = note('session = "x"\nform = "1"');
    const parent = document.body.appendChild(document.createElement("div"));
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: state.doc,
        extensions: [markdown({ base: markdownLanguage }), mediaBlockDiagnostics(check)],
      }),
    });
    try {
      await vi.waitFor(() =>
        expect(view.contentDOM.querySelector(".cm-media-hint-problem")?.textContent).toBe("form"),
      );
      // Typing outside the block moves the underline with its text and asks nothing.
      view.dispatch({ changes: { from: 0, insert: "Intro\n" } });
      await new Promise((resolve) => setTimeout(resolve, 400));
      expect(check).toHaveBeenCalledTimes(1);
      expect(view.contentDOM.querySelector(".cm-media-hint-problem")?.textContent).toBe("form");
      const at = view.state.doc.toString().indexOf("form");
      view.dispatch({ changes: { from: at, to: at + 4, insert: "from" } });
      await vi.waitFor(() =>
        expect(view.contentDOM.querySelector(".cm-media-hint-problem")).toBeNull(),
      );
      expect(check).toHaveBeenCalledTimes(2);
    } finally {
      view.destroy();
    }
  });
});
