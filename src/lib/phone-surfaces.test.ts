import { describe, expect, it } from "vitest";
import { sidebarViews } from "@/components/layout/sidebar-pane";
import type { CapabilitiesVm } from "@/lib/ipc/client";
import { type PhoneSurface, phoneRoutesView, phoneSurfaceFor } from "@/lib/phone-surfaces";
import { DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import type { PrimaryView } from "@/lib/stores/primary-view";

/** Every capability shape: all off, each flag alone, and all on. */
function capabilityShapes(): CapabilitiesVm[] {
  const flags = Object.keys(DEFAULT_CAPABILITIES) as Array<keyof CapabilitiesVm>;
  const allOn = Object.fromEntries(flags.map((flag) => [flag, true])) as CapabilitiesVm;
  return [
    DEFAULT_CAPABILITIES,
    allOn,
    ...flags.map((flag) => ({ ...DEFAULT_CAPABILITIES, [flag]: true })),
  ];
}

describe("phone surfaces (Story 66.1, AD-197, AD-27)", () => {
  it("routes the two chat windows and every surface it names, and nothing else", () => {
    const phone: CapabilitiesVm = { ...DEFAULT_CAPABILITIES, bots: true, sync: true };
    const routed: Record<PrimaryView, PhoneSurface | "level-0" | null> = {
      inbox: "level-0",
      archive: "level-0",
      approval: "approval",
      bridges: "bridges",
      settings: "settings",
      bots: "bots",
      sync: "sync",
      // Story 66.3: Files rides `sync`. Not yet on the phone: 66.4 adds notes.
      files: "files",
      notes: null,
      recording: null,
      recordings: null,
      sessions: null,
      tasks: null,
    };
    for (const [view, expected] of Object.entries(routed) as Array<
      [PrimaryView, PhoneSurface | "level-0" | null]
    >) {
      expect(phoneRoutesView(view, phone), view).toBe(expected !== null);
      expect(phoneSurfaceFor(view, phone), view).toBe(expected === "level-0" ? null : expected);
    }
  });

  it("keeps a gated surface absent where its capability is off", () => {
    expect(phoneSurfaceFor("bots", DEFAULT_CAPABILITIES)).toBeNull();
    expect(phoneSurfaceFor("sync", DEFAULT_CAPABILITIES)).toBeNull();
    expect(phoneSurfaceFor("files", DEFAULT_CAPABILITIES)).toBeNull();
    expect(phoneSurfaceFor("approval", DEFAULT_CAPABILITIES)).toBe("approval");
  });

  it("names a surface only for a view the sidebar registry can draw", () => {
    // The table must not invent a route the drawer never offers: every surface
    // is reachable from a row under the capabilities that gate it.
    const allOn = capabilityShapes()[1];
    const rows = sidebarViews(allOn).map((entry) => entry.view);
    const surfaces: PhoneSurface[] = ["approval", "bridges", "settings", "bots", "sync", "files"];
    for (const view of surfaces) {
      expect(phoneSurfaceFor(view, allOn), view).toBe(view);
      expect(rows, view).toContain(view);
    }
  });

  it("the drawer's phone filter leaves a row for every base entry on every capability shape", () => {
    // The base entries — Chats, Archive, Approvals, Bridges, Settings — need no
    // capability, so on no shape may the phone drop one of them (AD-27 cuts
    // both ways: no dead row, and no vanished row either).
    for (const shape of capabilityShapes()) {
      const kept = sidebarViews(shape).filter((entry) => phoneRoutesView(entry.view, shape));
      const labels = kept.map((entry) => entry.label);
      expect(labels).toEqual(
        expect.arrayContaining(["Chats", "Archive", "Approvals", "Bridges", "Settings"]),
      );
      // And every kept row is one the shell renders.
      for (const entry of kept) {
        expect(phoneRoutesView(entry.view, shape), entry.label).toBe(true);
      }
    }
  });
});
