/**
 * The dev harness's promote commands answer as Rust's do. Each scenario of
 * `command-vectors.json` — recorded by `keeper_agent::promote`'s
 * `every_command_vector_holds` over real files — is staged as a promote
 * fixture and replayed through the harness's own handlers, over IPC: every
 * panel (offers, target facts, revisions, copies, problems), every read,
 * every promotion out, review and archive must come out as Rust's did.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  type ChoiceVm,
  type SessionPromoteVm,
  sessionsArchive,
  sessionsKnowledgeRead,
  sessionsKnowledgeReview,
  sessionsPromotePanel,
  sessionsPromoteTo,
} from "@/lib/ipc/client";
import { installMockShell, stagePromoteFixture } from "../../dev/mock-shell";

type VectorFile = {
  path: string;
  text?: string;
  hex?: string;
  pad?: { with: string; to: number };
  changed: number;
};

type VectorChoice = { item?: number; revision?: string; promote?: boolean; target?: string };

type Step = { expect: unknown } & (
  | { panel: { intent?: VectorChoice[] } }
  | { read: { path: string; copy: boolean } }
  | { promoteTo: { source: string; folder: string; name: string; expected: boolean } }
  | { review: { path: string; reviewed: boolean } }
  | { archive: { choices: VectorChoice[] | "intent" } }
  | { write: VectorFile }
);

const SCENARIOS = (
  JSON.parse(
    readFileSync(
      resolve(
        import.meta.dirname,
        "../../src-tauri/crates/keeper-agent/src/promote/command-vectors.json",
      ),
      "utf8",
    ),
  ) as {
    scenarios: {
      name: string;
      /** `published`: the row records the scenario's copy at its target as its source's. */
      rows: { source: string; target: string; note: string; published?: boolean }[];
      files: VectorFile[];
      dirs?: string[];
      vault: (VectorFile & { reviewers: string[] })[];
      unreadable: string[];
      steps: Step[];
    }[];
  }
).scenarios;

const ROOT = "vectors";
const SESSION = "01J5AAAAAAAAAAAAAAAAAAAAAA";

/** A vector file's bytes: its `text` or `hex`, padded with `pad.with` to `pad.to` bytes. */
function vectorBytes(file: VectorFile): Uint8Array {
  const bytes =
    file.hex === undefined
      ? new TextEncoder().encode(file.text ?? "")
      : new Uint8Array((file.hex.match(/../g) ?? []).map((pair) => Number.parseInt(pair, 16)));
  if (file.pad === undefined) return bytes;
  const padded = new Uint8Array(file.pad.to).fill(file.pad.with.charCodeAt(0));
  padded.set(bytes.subarray(0, file.pad.to));
  return padded;
}

/** The outcome a vector records for a command that answers nothing: `ok`, or its refusal. */
async function outcome<T>(run: () => Promise<T>): Promise<T | { refused: string }> {
  try {
    return await run();
  } catch (error) {
    return { refused: (error as { message: string }).message };
  }
}

/** The choices a step names, by item of the last panel (its rows, then its unlisted files). */
function choicesOf(asked: VectorChoice[] | "intent", shown: SessionPromoteVm): ChoiceVm[] {
  if (asked === "intent") return shown.intent.choices;
  const items = [...shown.rows, ...shown.unlisted].map((item) => item.revision);
  return asked.map((choice) => ({
    revision: choice.item === undefined ? (choice.revision ?? "") : items[choice.item],
    promote: choice.promote ?? false,
    target: choice.target ?? "",
  }));
}

beforeEach(() => {
  clearMocks();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  installMockShell();
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  clearMocks();
});

describe("the dev harness's promote commands", () => {
  for (const scenario of SCENARIOS) {
    it(`answer as Rust does: ${scenario.name}`, async () => {
      const fixture = stagePromoteFixture(ROOT, SESSION, (staged) => {
        staged.head = `---\nid: ${SESSION}\n---\n# Session\n`;
        staged.table = true;
        staged.rows = scenario.rows.map(({ published, ...row }) => {
          const copy = scenario.vault.find((held) => held.path === row.target);
          return published === true && copy !== undefined
            ? {
                ...row,
                published: new TextDecoder("utf-8", { ignoreBOM: true }).decode(vectorBytes(copy)),
              }
            : row;
        });
        staged.files = new Map(
          scenario.files.map((file) => [
            file.path,
            { text: "", bytes: vectorBytes(file), changed: file.changed },
          ]),
        );
        staged.dirs = scenario.dirs ?? [];
        staged.unreadable = Object.fromEntries(scenario.unreadable.map((path) => [path, true]));
        staged.vault = new Map(
          scenario.vault.map((copy) => [
            copy.path,
            {
              text: new TextDecoder("utf-8", { ignoreBOM: true }).decode(vectorBytes(copy)),
              reviewers: copy.reviewers,
              changed: copy.changed,
            },
          ]),
        );
        staged.outRefused = null;
        staged.label = null;
        staged.problems = [];
      });
      let shown: SessionPromoteVm | undefined;
      const candidates = new Map<string, string>();
      const copies = new Map<string, string>();
      const ok = { ok: true };
      for (const step of scenario.steps) {
        let got: unknown = null;
        if ("panel" in step) {
          const choices =
            step.panel.intent === undefined || shown === undefined
              ? []
              : choicesOf(step.panel.intent, shown);
          got = await outcome(() => sessionsPromotePanel(ROOT, SESSION, { choices, notes: [] }));
          if (!("refused" in (got as object))) shown = got as SessionPromoteVm;
        } else if ("read" in step) {
          const { path, copy } = step.read;
          got = await outcome(async () => {
            const read = await sessionsKnowledgeRead(ROOT, SESSION, path, copy);
            (copy ? copies : candidates).set(path, read.revision);
            const bytes = new TextEncoder().encode(read.text).length;
            return bytes <= 4096
              ? { revision: read.revision, bytes, text: read.text }
              : { revision: read.revision, bytes };
          });
        } else if ("promoteTo" in step) {
          const { source, folder, name, expected } = step.promoteTo;
          got = await outcome(async () => {
            await sessionsPromoteTo(
              ROOT,
              SESSION,
              source,
              folder,
              name,
              expected ? (candidates.get(source) ?? null) : null,
            );
            return ok;
          });
        } else if ("review" in step) {
          const { path, reviewed } = step.review;
          got = await outcome(async () => {
            await sessionsKnowledgeReview(ROOT, SESSION, path, reviewed, copies.get(path) ?? "");
            return ok;
          });
        } else if ("archive" in step) {
          const at = shown;
          if (at === undefined) throw new Error("an archive needs a panel first");
          got = await outcome(async () => {
            await sessionsArchive(
              ROOT,
              SESSION,
              choicesOf(step.archive.choices, at),
              true,
              at.revision,
            );
            return ok;
          });
        } else {
          fixture.files.set(step.write.path, {
            text: "",
            bytes: vectorBytes(step.write),
            changed: step.write.changed,
          });
        }
        expect(got, JSON.stringify(Object.keys(step))).toEqual(step.expect);
      }
    });
  }
});
