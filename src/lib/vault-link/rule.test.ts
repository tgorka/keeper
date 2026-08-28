/**
 * The note/file resolution rule, pinned to the Rust one (Story 45.18, AD-65).
 *
 * **The whole point of this file is the shared vector table.** The rule is
 * authored in `keeper_core::vault_link` and mirrored in `rule.ts`; they never
 * meet at runtime, so nothing except a table both suites load can stop them
 * drifting. The failure this prevents is a vault called `Notes` that offers
 * "Open in Notes" in one surface and not in the other, or a CSV that tables in
 * a note and refuses in a panel — both of which look like unrelated bugs.
 *
 * The tests below the table are the ones the table cannot carry: it holds
 * inputs and outputs, not configurations, so ordering, nesting and the
 * "innermost wins" rule get their own cases here as they do in Rust.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { filePathForNote, notePathForFile, type VaultLocation } from "./rule";

/**
 * Read from the Rust tree rather than copied into `src/`, following
 * `file-asset-url.test.ts` and `file-size.test.ts`: the fixture lives beside
 * the authoring implementation and the mirror reaches for it. `readFileSync`
 * rather than an `import`, so a missing file is a loud failure here rather than
 * an empty table that passes.
 */
const FIXTURE = resolve(
  import.meta.dirname,
  "../../../src-tauri/crates/keeper-core/src/vault-link-vectors.json",
);

interface VaultRow {
  vault_id: string;
  profile_id: string;
  subfolder: string;
}

interface ToNote {
  why: string;
  profile_id: string;
  relative_path: string;
  vault_id: string | null;
  vault_path: string | null;
  vault_dir: string | null;
}

interface ToFile {
  why: string;
  vault_id: string;
  note_path: string;
  relative_path: string | null;
}

const VECTORS = JSON.parse(readFileSync(FIXTURE, "utf8")) as {
  vaults: VaultRow[];
  to_note: ToNote[];
  to_file: ToFile[];
};

const VAULTS: VaultLocation[] = VECTORS.vaults.map((row) => ({
  id: row.vault_id,
  profileId: row.profile_id,
  subfolder: row.subfolder,
}));

function vaultNamed(vaultId: string): VaultLocation {
  const found = VAULTS.find((vault) => vault.id === vaultId);
  if (found === undefined) {
    throw new Error(`the table has no vault ${vaultId}`);
  }
  return found;
}

describe("the shared vector table", () => {
  it("resolves every file to the note path Rust resolves it to", () => {
    for (const vector of VECTORS.to_note) {
      expect(
        notePathForFile(VAULTS, vector.profile_id, vector.relative_path),
        `${vector.profile_id}/${vector.relative_path} — ${vector.why}`,
      ).toEqual(
        vector.vault_id === null
          ? null
          : {
              vaultId: vector.vault_id,
              vaultPath: vector.vault_path,
              vaultDir: vector.vault_dir,
            },
      );
    }
  });

  it("resolves every note to the profile path Rust resolves it to", () => {
    for (const vector of VECTORS.to_file) {
      const vault = vaultNamed(vector.vault_id);
      expect(
        filePathForNote(vault, vector.note_path),
        `${vector.vault_id}:${vector.note_path} — ${vector.why}`,
      ).toEqual(
        vector.relative_path === null
          ? null
          : { profileId: vault.profileId, relativePath: vector.relative_path },
      );
    }
  });

  it("round-trips every resolved file back to the file it came from", () => {
    // The two directions have to compose or one of them is wrong in a way no
    // single-direction table can see: a note opened in Files and then reopened
    // in Notes must be the same note. Rust asserts the same composition over
    // the same rows.
    let checked = 0;
    for (const vector of VECTORS.to_note) {
      if (vector.vault_id === null) {
        continue;
      }
      const forward = notePathForFile(VAULTS, vector.profile_id, vector.relative_path);
      expect(forward).not.toBeNull();
      const back = filePathForNote(vaultNamed(forward?.vaultId ?? ""), forward?.vaultPath ?? "");
      expect(back?.profileId).toBe(vector.profile_id);
      // Case-insensitively, because the dirent's spelling of the vault folder
      // and the configured one legitimately differ; everything after the vault
      // folder must match exactly, which the whole-string compare covers.
      expect(back?.relativePath.toLowerCase()).toBe(vector.relative_path.toLowerCase());
      checked += 1;
    }
    expect(checked).toBeGreaterThanOrEqual(6);
  });

  it("carries enough vectors, and enough vaults, to be worth loading", () => {
    // A table someone empties makes both suites pass while the two languages
    // agree about nothing. A ONE-vault table is worse than short: it cannot
    // tell a per-profile filter from an unconditional match, and it cannot tell
    // longest-match from first-match.
    expect(VECTORS.vaults.length).toBeGreaterThanOrEqual(2);
    expect(new Set(VECTORS.vaults.map((row) => row.profile_id)).size).toBeGreaterThanOrEqual(2);
    expect(VECTORS.to_note.length).toBeGreaterThanOrEqual(12);
    expect(VECTORS.to_file.length).toBeGreaterThanOrEqual(8);
    // Both answers, or the table only proves one half of the rule.
    expect(VECTORS.to_note.filter((row) => row.vault_id === null).length).toBeGreaterThanOrEqual(4);
    expect(VECTORS.to_note.filter((row) => row.vault_id !== null).length).toBeGreaterThanOrEqual(6);
  });
});

describe("notePathForFile", () => {
  const outer: VaultLocation = { id: "v-outer", profileId: "p1", subfolder: "notes" };
  const inner: VaultLocation = { id: "v-inner", profileId: "p1", subfolder: "notes/journal" };

  it("answers with the innermost vault holding the file, whichever order they arrive in", () => {
    // First-match would answer the outer vault, where that note id does not
    // exist — so the surface would say a file it is showing has no note.
    for (const order of [
      [outer, inner],
      [inner, outer],
    ]) {
      expect(notePathForFile(order, "p1", "notes/journal/2026-01-01.md")).toEqual({
        vaultId: "v-inner",
        vaultPath: "2026-01-01.md",
        vaultDir: "",
      });
    }
  });

  it("still answers the outer vault for a file only it holds", () => {
    expect(notePathForFile([outer, inner], "p1", "notes/inbox/idea.md")).toEqual({
      vaultId: "v-outer",
      vaultPath: "inbox/idea.md",
      vaultDir: "inbox",
    });
  });

  it("matches the subfolder component by component, never as a string prefix", () => {
    // `notesy` shares five characters with `notes` and is a different folder.
    expect(notePathForFile([outer], "p1", "notesy/x.md")).toBeNull();
    expect(notePathForFile([outer], "p1", "not/notes/x.md")).toBeNull();
  });

  it("refuses the vault directory itself, which is a folder and has no note", () => {
    const nested: VaultLocation = { id: "v1", profileId: "p1", subfolder: "notes/inner" };
    expect(notePathForFile([nested], "p1", "notes/inner")).toBeNull();
    expect(notePathForFile([nested], "p1", "notes/inner/")).toBeNull();
    expect(notePathForFile([nested], "p1", "notes")).toBeNull();
    expect(notePathForFile([nested], "p1", "notes/inner/a.md")).not.toBeNull();
  });

  it("skips a vault whose id is empty, because no command could address it", () => {
    expect(notePathForFile([{ ...outer, id: "" }], "p1", "notes/x.md")).toBeNull();
  });

  it("keeps the filesystem's own case in the answer while ignoring it in the match", () => {
    const messy: VaultLocation = { id: "v1", profileId: "p1", subfolder: "Notes\\Daily/" };
    expect(notePathForFile([messy], "p1", "notes/daily/Sub Folder/Meeting Notes.MD")).toEqual({
      vaultId: "v1",
      vaultPath: "Sub Folder/Meeting Notes.MD",
      vaultDir: "Sub Folder",
    });
  });
});

describe("filePathForNote", () => {
  const vault: VaultLocation = { id: "v1", profileId: "p1", subfolder: "notes" };

  it("composes the stored subfolder with the note path, keeping the stored case", () => {
    expect(filePathForNote({ ...vault, subfolder: "Second Brain" }, "a b/x.md")).toEqual({
      profileId: "p1",
      relativePath: "Second Brain/a b/x.md",
    });
  });

  it("is nothing for a vault whose profile id is empty", () => {
    // A `file` panel target with an empty profile id is refused by
    // `isRestorableTarget` before it is ever opened, so composing one here
    // would hand a surface a target the panel store will not restore — an
    // action that works until a restart and then silently does not.
    expect(filePathForNote({ ...vault, profileId: "" }, "x.md")).toBeNull();
  });

  it("is nothing for a profile that carries no vault subfolder", () => {
    // What `notes_ipc.rs` projects for an unflagged folder. Treating "" as a
    // vault at the profile root would offer "Show in Files" on a note whose
    // file would be looked for in the wrong place.
    expect(filePathForNote({ ...vault, subfolder: "" }, "x.md")).toBeNull();
    expect(filePathForNote({ ...vault, subfolder: "//" }, "x.md")).toBeNull();
  });

  it("refuses every note path that climbs or is absolute", () => {
    for (const hostile of [
      "/etc/passwd",
      "\\\\server\\share\\x.md",
      "\\x.md",
      "C:/secrets.md",
      "c:\\secrets.md",
      "../x.md",
      "notes/../../x.md",
      "./x.md",
      "",
      "/",
    ]) {
      expect(filePathForNote(vault, hostile), hostile).toBeNull();
    }
  });

  it("treats a backslash in a note's own name as a character, not a separator", () => {
    // Legal on Linux. Splitting on it would compose a path to a file that does
    // not exist, and the panel would say the note's file is gone.
    expect(filePathForNote(vault, "a\\b.md")).toEqual({
      profileId: "p1",
      relativePath: "notes/a\\b.md",
    });
  });
});
