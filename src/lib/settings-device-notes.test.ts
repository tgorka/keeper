/**
 * A note that names settings keys says they apply only to this device but are
 * backed up in the account — and that is only true of a key the registry routes
 * to `<login>/settings.<device>.toml` (`settings_sync::synced_file_of`).
 *
 * The registry's answer is read from `docs/settings-keys.md`, generated from it
 * and pinned by `config::keys::tests::docs`, as `file-controlled-keys.test.ts`
 * does: a key that every device shares, or that never leaves this device, under
 * that heading would be a claim the persistence does not keep.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import type { DeviceNote } from "@/lib/settings-device-notes";
import * as notes from "@/lib/settings-device-notes";

const DOC = readFileSync(resolve(import.meta.dirname, "../../docs/settings-keys.md"), "utf8");

/** The keys the generated travel section lists after `- **label**`. */
function travelling(label: string): Set<string> {
  const travel = DOC.slice(DOC.indexOf("## Where each key travels"));
  const line = travel.split("\n").find((each) => each.startsWith(`- **${label}**`)) ?? "";
  // The file the bullet names is backticked too; a key has no `/`.
  return new Set(
    Array.from(line.matchAll(/`([^`]+)`/g), (match) => match[1]).filter(
      (token) => !token.includes("/"),
    ),
  );
}

const deviceFile = travelling("This device's own file");
const keyed = Object.values(notes).filter(
  (value): value is DeviceNote & { keys: readonly string[] } =>
    typeof value === "object" && value !== null && "text" in value && "keys" in value,
);

describe("the keys a this-device note in Settings names", () => {
  it.each(keyed.map((note) => [note.text, note] as const))("%s", (_, note) => {
    for (const key of note.keys) {
      expect(deviceFile, key).toContain(key);
    }
  });

  it("are found at all, so this file is not asserting about an empty list", () => {
    expect(deviceFile).toContain("sync.git_path");
    expect(keyed.length).toBeGreaterThan(5);
  });
});
