/**
 * The dev harness reads a promote fixture's files through the same identity
 * its panel and its writes use — a root and a session of it — so a browser
 * review never shows one document while its promotion names another.
 *
 * `sync_read_text` names a drive (its profile id) and a path in it; the panel,
 * the promotion and the review name a sessions root and a session. Two roots
 * holding the same session id and the same relative names are two drives:
 * each viewer must show its own root's bytes, the revision each panel
 * advertises must be the SHA-256 of exactly those bytes (as Rust's
 * `KnowledgeNoteVm.revision` is), and a review ticked or unticked under one
 * root must be read back in that root's vault copy only.
 */
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  sessionsKnowledgeRead,
  sessionsKnowledgeReview,
  sessionsPromotePanel,
  sessionsPromoteTo,
  syncReadText,
} from "@/lib/ipc/client";
import { installMockShell, stagePromoteFixture } from "../../dev/mock-shell";

const SESSION = "01J8SESSIONAAAAAAAAAAAAAAA";
const IN_DRIVE = "60-sessions/active/2026-08-12-keeper-sessions";
const NOTE = "artifacts/knowledge/2026-10-05-taxes/what-to-bring.md";
const COPY = "10-notes/knowledge/what-to-bring.md";

async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
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

describe("the dev harness's promote reads", () => {
  it("serve each root its own session's bytes, the ones its panel's revision names", async () => {
    const bring: Record<string, string> = {
      p1: "Bring the PIT-37 and the receipts.",
      p2: "Bring the passport.",
    };
    for (const [root, line] of Object.entries(bring)) {
      stagePromoteFixture(root, SESSION, (fixture) => {
        const note = fixture.files.get(NOTE);
        if (note !== undefined) {
          note.text = note.text.replace("Bring the PIT-37, the receipts and the ID.", line);
        }
      });
    }

    const revisions = new Set<string>();
    for (const [root, line] of Object.entries(bring)) {
      const read = await syncReadText(root, `${IN_DRIVE}/${NOTE}`);
      expect(read.text).toContain(line);
      const panel = await sessionsPromotePanel(root, SESSION, { choices: [], notes: [] });
      const note = panel.knowledge.find((candidate) => candidate.path === NOTE);
      expect(note?.revision).toBe(await sha256Hex(read.text ?? ""));
      revisions.add(note?.revision ?? "");
    }
    expect(revisions.size).toBe(2);
  });

  it("read back a review in the vault copy of the root it was written under only", async () => {
    stagePromoteFixture("p1", SESSION);
    stagePromoteFixture("p2", SESSION);

    const read = await sessionsKnowledgeRead("p2", SESSION, NOTE, true);
    await sessionsKnowledgeReview("p2", SESSION, NOTE, false, read.revision);

    const ticked = await syncReadText("p1", COPY);
    const unticked = await syncReadText("p2", COPY);
    expect(ticked.text).toContain("  - by: human:tgorka\n");
    expect(unticked.text).not.toContain("verified:");
    const panels = await Promise.all(
      ["p1", "p2"].map((root) => sessionsPromotePanel(root, SESSION, { choices: [], notes: [] })),
    );
    expect(
      panels.map((panel) => panel.knowledge.find((note) => note.path === NOTE)?.reviewedByMe),
    ).toEqual([true, false]);
  });

  it("never adopts a vault file that holds the unreviewed note's exact bytes", async () => {
    const ledger = "artifacts/knowledge/2026-10-05-taxes/the-whole-ledger.md";
    const target = "10-notes/knowledge/the-whole-ledger.md";
    for (const row of ["none", "three cells"]) {
      const fixture = stagePromoteFixture("p1", SESSION, (staged) => {
        const text = staged.files.get(ledger)?.text ?? "";
        if (row === "three cells") {
          // A person's line in the vault file: the note is newer here, and
          // still nothing is offered over a file that is not its copy.
          staged.vault.set(target, { text: `${text}A person's line.\n`, reviewers: [], changed: 1 });
          staged.rows.push({ source: ledger, target, note: "knowledge" });
        } else {
          staged.vault.set(target, { text, reviewers: [], changed: 1 });
        }
      });
      const text = fixture.files.get(ledger)?.text ?? "";
      await expect(
        sessionsPromoteTo(
          "p1",
          SESSION,
          ledger,
          "10-notes/knowledge",
          "the-whole-ledger.md",
          await sha256Hex(text),
        ),
      ).rejects.toMatchObject({ code: "internal" });
      expect(fixture.vault.get(target)?.reviewers).toEqual([]);
      const panel = await sessionsPromotePanel("p1", SESSION, { choices: [], notes: [] });
      const note = panel.knowledge.find((candidate) => candidate.path === ledger);
      expect(note?.reviewedByMe).toBe(false);
      expect(note?.foreignCopy !== null).toBe(row === "three cells");
      if (row === "three cells") {
        // A file that is not the note's copy is never offered as a target.
        expect(note?.destination).toBeNull();
        expect(note?.unavailable).toBe(note?.foreignCopy);
        // Read as it is, it still takes no review: it is not the note's copy.
        const read = await sessionsKnowledgeRead("p1", SESSION, ledger, true);
        await expect(
          sessionsKnowledgeReview("p1", SESSION, ledger, true, read.revision),
        ).rejects.toMatchObject({ message: note?.foreignCopy });
        expect(fixture.vault.get(target)?.reviewers).toEqual([]);
      }
    }
  });
});
