import { render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionReferenceVm } from "@/lib/ipc/client";

const openUrl = vi.fn();
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (url: unknown) => openUrl(url),
}));

import {
  missingSummary,
  SESSION_REFS_ALL_RESOLVED,
  SESSION_REFS_EMPTY,
  SESSION_REFS_LABEL,
  SESSION_REFS_TRUNCATED,
  SessionRefs,
} from "@/components/sessions/session-refs";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";

function ref(over: Partial<SessionReferenceVm> & Pick<SessionReferenceVm, "kind">) {
  return {
    target: over.target ?? "40-media/clip.mov",
    label: over.label ?? "clip.mov",
    source: "README.md",
    panelTarget: null,
    url: null,
    notice: null,
    ...over,
  } as SessionReferenceVm;
}

/** The rows the widget exists to tell apart. */
const NOTE = ref({
  kind: "note",
  target: "Vault as a lens",
  label: "Vault as a lens",
  panelTarget: { kind: "note", vaultId: "tgdrive", noteId: "01JLENS" },
});
const RECORDING = ref({
  kind: "recording",
  target: "Standup",
  label: "Standup 2026-08-10",
  panelTarget: { kind: "note", vaultId: "tgdrive", noteId: "01JREC" },
});
const FILE = ref({
  kind: "file",
  panelTarget: { kind: "file", profileId: "tgdrive", relativePath: "40-media/clip.mov" },
});
const EXTERNAL = ref({
  kind: "external",
  target: "https://example.com/rfc",
  label: "the RFC",
  url: "https://example.com/rfc",
});
const MISSING = ref({
  kind: "missing",
  target: "40-media/moved.m4a",
  label: "the recording",
  source: "refs/inputs.md",
  notice:
    "40-media/moved.m4a: this session points at something the drive does not have — keeper looked for 60-sessions/active/2026-08-10-keeper/40-media/moved.m4a and 40-media/moved.m4a",
});

function mount(over: Partial<React.ComponentProps<typeof SessionRefs>> = {}) {
  return render(
    <SessionRefs refs={[NOTE, FILE, EXTERNAL]} missing={0} truncated={false} {...over} />,
  );
}

/** What the strip is showing — the panel a click on a row would have filled. */
function opened() {
  return activePanel(panelsStore.getState()).target;
}

beforeEach(() => {
  resetPanelsStoreForTest();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("SessionRefs", () => {
  it("says nothing is broken when nothing is, rather than making it be read off the rows", () => {
    mount();
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_REFS_ALL_RESOLVED);
  });

  it("leads with the count when something is broken, and says how many", () => {
    mount({ refs: [MISSING, FILE], missing: 1 });
    expect(screen.getByRole("status")).toHaveTextContent(missingSummary(1));
    mount({ refs: [MISSING, MISSING], missing: 2 });
    expect(screen.getAllByRole("status")[1]).toHaveTextContent("2 references point at");
  });

  it("names what keeper looked for on a missing row, so the fix is one move away", () => {
    mount({ refs: [MISSING], missing: 1 });
    expect(screen.getByText(/keeper looked for/)).toHaveTextContent(
      "60-sessions/active/2026-08-10-keeper/40-media/moved.m4a and 40-media/moved.m4a",
    );
    // And it says which file to go and edit.
    const list = screen.getByRole("list", { name: SESSION_REFS_LABEL });
    expect(within(list).getByText("refs/inputs.md")).toBeInTheDocument();
  });

  it("gives a missing row nothing to click, because there is nothing to open", () => {
    mount({ refs: [MISSING], missing: 1 });
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("opens a note through the panel target Rust composed", () => {
    mount({ refs: [NOTE] });
    screen.getByRole("button", { name: /Vault as a lens/ }).click();
    expect(opened()).toEqual({ kind: "note", vaultId: "tgdrive", noteId: "01JLENS" });
  });

  it("opens a file through the profile-relative path, never one joined here", () => {
    mount({ refs: [FILE] });
    screen.getByRole("button", { name: /clip\.mov/ }).click();
    expect(opened()).toEqual({
      kind: "file",
      profileId: "tgdrive",
      relativePath: "40-media/clip.mov",
    });
  });

  it("sends an external link to the system browser and never to a panel", async () => {
    openUrl.mockResolvedValue(undefined);
    mount({ refs: [EXTERNAL] });
    screen.getByRole("button", { name: /the RFC/ }).click();
    await waitFor(() => {
      expect(openUrl).toHaveBeenCalledWith("https://example.com/rfc");
    });
    expect(opened()).toBeNull();
  });

  it("calls a recording a recording and a file a file", () => {
    mount({ refs: [RECORDING, FILE] });
    const list = screen.getByRole("list", { name: SESSION_REFS_LABEL });
    expect(within(list).getByText("recording")).toBeInTheDocument();
    expect(within(list).getByText("file")).toBeInTheDocument();
    // The recording's LABEL is the note's title, not its link text.
    expect(within(list).getByText("Standup 2026-08-10")).toBeInTheDocument();
  });

  it("says an empty session references nothing rather than rendering nothing", () => {
    mount({ refs: [] });
    expect(screen.getByText(SESSION_REFS_EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
  });

  it("says a truncated scan was truncated", () => {
    mount({ truncated: true });
    expect(screen.getByText(SESSION_REFS_TRUNCATED)).toBeInTheDocument();
  });
});
