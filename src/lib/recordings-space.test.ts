import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  notesSpaces: vi.fn(),
  notesVaults: vi.fn(() => Promise.resolve([])),
  notesVaultActive: vi.fn(() => Promise.resolve(null)),
  notesVaultSetActive: vi.fn(() => Promise.resolve()),
}));

import type { NoteSpaceVm } from "@/lib/ipc/client";
import { openRecordingsSpace, RECORDINGS_SPACE_KEY } from "@/lib/recordings-space";
import { ALL_NOTES_SCOPE, notesFiltersStore } from "@/lib/stores/notes-filters";
import { primaryViewStore } from "@/lib/stores/primary-view";

const SPACE = {
  id: "spaces/2026-08-09-recordings.md",
  name: "Recordings",
  defaultKey: RECORDINGS_SPACE_KEY,
} as unknown as NoteSpaceVm;

beforeEach(() => {
  primaryViewStore.getState().setView("inbox");
  notesFiltersStore.getState().setScope(ALL_NOTES_SCOPE);
  notesFiltersStore.setState({ scope: ALL_NOTES_SCOPE });
});

describe("openRecordingsSpace", () => {
  it("shows the Notes view scoped to the space", () => {
    openRecordingsSpace(SPACE);

    expect(primaryViewStore.getState().view).toBe("notes");
    expect(notesFiltersStore.getState().scope).toEqual({
      kind: "space",
      id: SPACE.id,
      name: SPACE.name,
      defaultKey: RECORDINGS_SPACE_KEY,
    });
  });

  it("keeps the scope when it is already this space, because setScope is a toggle", () => {
    // `notes-filters.setScope` clears the scope when it is handed the one that
    // is already selected — right for a sidebar row, catastrophic for a button
    // that says "take me there": pressing it twice, or pressing it while the
    // Recordings space is showing, would drop the user into every note in the
    // vault and look like the button did nothing useful.
    openRecordingsSpace(SPACE);
    primaryViewStore.getState().setView("recording");

    openRecordingsSpace(SPACE);

    expect(primaryViewStore.getState().view).toBe("notes");
    expect(notesFiltersStore.getState().scope).toMatchObject({ kind: "space", id: SPACE.id });
  });

  it("replaces a different space's scope rather than clearing it", () => {
    notesFiltersStore.getState().setScope({
      kind: "space",
      id: "spaces/2026-08-09-inbox.md",
      name: "Inbox",
      defaultKey: "inbox",
    });

    openRecordingsSpace(SPACE);

    expect(notesFiltersStore.getState().scope).toMatchObject({ kind: "space", id: SPACE.id });
  });
});

describe("the Recordings space's identity", () => {
  it("is spelled the same here as `keeper_core::notes::default_spaces` spells it", () => {
    // Three files name this identity: Rust's `DEFAULT_SPACES` writes it into
    // every seeded space's `keeper.default`, this module reads it back, and
    // `notes-pane.tsx` reads it for the empty state. Nothing made them agree.
    //
    // Rename the Rust key and NOTHING fails: the space still seeds, the sidebar
    // still lists it, and the button this module gates simply stops appearing —
    // for everyone, permanently, with no error anywhere. A dangling name and a
    // live one are the same bytes.
    //
    // A TypeScript test over a Rust file, following `capture-capability.test.ts`
    // and `no-user-agent-gating.test.ts`: the invariant is about a file rather
    // than a function, and the `keeper` shell does not compile on Linux, so the
    // half that runs everywhere is the half worth having.
    const source = readFileSync(
      resolve(process.cwd(), "src-tauri/crates/keeper-core/src/notes/default_spaces.rs"),
      "utf8",
    );
    expect(source).toContain(`key: "${RECORDINGS_SPACE_KEY}",`);
  });
});
