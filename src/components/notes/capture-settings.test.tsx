import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteTemplateVm, NoteVaultVm } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  notesTemplates: vi.fn(),
  notesCaptureImpact: vi.fn(),
  notesVaultSettingsSave: vi.fn(),
}));

import {
  CAPTURE_IMPACT_TITLE,
  CAPTURE_SAVE_FAILED,
  CAPTURE_SECTION_TITLE,
  CAPTURE_TAG_NOTE,
  CAPTURE_TEMPLATE_MISSING,
  CAPTURE_TEMPLATE_NOTE,
  CaptureSettingsSection,
} from "@/components/notes/capture-settings";
import { notesCaptureImpact, notesTemplates, notesVaultSettingsSave } from "@/lib/ipc/client";
import { notesVaultsStore, resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";

const mockTemplates = vi.mocked(notesTemplates);
const mockImpact = vi.mocked(notesCaptureImpact);
const mockSave = vi.mocked(notesVaultSettingsSave);

/**
 * Two templates, never one.
 *
 * A fixture holding a single item cannot tell the right answer from a mutant
 * that keeps only the first, so a chooser fixture with one option would pass a
 * `.slice(0, 1)` that drops every template but the first from the menu.
 */
const TEMPLATES: NoteTemplateVm[] = [
  { path: "templates/capture.md", name: "Capture" },
  { path: "templates/journal-entry.md", name: "Journal Entry" },
];

function vault(p: Partial<NoteVaultVm> = {}): NoteVaultVm {
  return {
    id: p.id ?? "v1",
    profileId: p.profileId ?? "v1",
    name: p.name ?? "Second Brain",
    subfolder: p.subfolder ?? "notes",
    root: p.root ?? "/Users/t/Sync/notes",
    indexed: p.indexed ?? true,
    noteCount: p.noteCount ?? 12,
    unreadCount: p.unreadCount ?? 0,
    captureTemplate: p.captureTemplate ?? null,
    captureTag: p.captureTag ?? null,
    cadence: p.cadence ?? {
      commitIdleMs: 2000,
      pushIntervalMs: 30000,
      pushOnBlur: true,
    },
  };
}

/** Mount the section over a hydrated store holding `vaults`. */
function mount(vaults: NoteVaultVm[], activeVaultId: string | null = vaults[0]?.id ?? null) {
  notesVaultsStore.setState({ vaults, activeVaultId, hydrated: true });
  render(<CaptureSettingsSection open={true} />);
}

beforeEach(() => {
  resetNotesVaultsStoreForTest();
  mockTemplates.mockResolvedValue(TEMPLATES);
  mockImpact.mockResolvedValue([]);
  mockSave.mockImplementation(async (_id, settings) =>
    vault({
      captureTemplate: settings.captureTemplate === "" ? null : (settings.captureTemplate ?? null),
      // Mirrors the one thing Rust does that the form cannot: the tag comes
      // back folded, and the marker comes back refused. A stub that echoed the
      // input would make every assertion below an assertion about the stub.
      captureTag: fold(settings.captureTag),
    }),
  );
});

afterEach(() => {
  vi.clearAllMocks();
  resetNotesVaultsStoreForTest();
});

/** `keeper_core::notes::seed::capture_tag`, in the little the form can see. */
function fold(typed: string | null | undefined): string | null {
  if (typed === null || typed === undefined) {
    return null;
  }
  const canonical = typed.trim().replace(/^#+/, "").toLowerCase().replace(/\s+/g, "-");
  if (canonical === "" || canonical === "template") {
    return null;
  }
  return canonical;
}

describe("CaptureSettingsSection", () => {
  it("renders nothing when no folder has been flagged as a vault", () => {
    mount([], null);
    expect(screen.queryByText(CAPTURE_SECTION_TITLE)).toBeNull();
    expect(mockTemplates).not.toHaveBeenCalled();
  });

  /**
   * Two vaults and neither active: there is no honest answer to "which vault's
   * captures", so the section stays away rather than guessing at the first.
   */
  it("renders nothing with several vaults and none of them active", () => {
    mount([vault({ id: "v1" }), vault({ id: "v2" })], null);
    expect(screen.queryByText(CAPTURE_SECTION_TITLE)).toBeNull();
  });

  it("configures the vault that is active, not the first one listed", async () => {
    mount([vault({ id: "v1" }), vault({ id: "v2" })], "v2");
    await waitFor(() => expect(mockTemplates).toHaveBeenCalledWith("v2"));
  });

  it("shows the vault's stored template and tag rather than an empty form", async () => {
    mount([vault({ captureTemplate: "templates/capture.md", captureTag: "capture" })]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());
    expect(screen.getByLabelText("Captures start from")).toHaveValue("templates/capture.md");
    expect(screen.getByLabelText("Tag every capture with")).toHaveValue("capture");
  });

  it("offers every template the vault has, not only the first", async () => {
    mount([vault()]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());
    const options = within(screen.getByLabelText("Captures start from")).getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      "No template",
      "Capture",
      "Journal Entry",
    ]);
  });

  /**
   * Both explanations are handed to children as props — the template one to
   * `<TemplateSelect note=…>`, the tag one rendered here — and every other test
   * in this file asserts what a control DOES rather than what it says. A
   * component handed the wrong sentence, or an empty one, would pass all of
   * them: the tag line is the only place the folding rule and the
   * leave-it-empty escape hatch are stated at all.
   */
  it("explains both controls, in the words the surface owns", async () => {
    mount([vault()]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());
    expect(screen.getByText(CAPTURE_TEMPLATE_NOTE)).toBeTruthy();
    expect(screen.getByText(CAPTURE_TAG_NOTE)).toBeTruthy();
  });

  /**
   * The saved VM goes back into the shared mirror, or the vault switcher and
   * every other reader keep the pre-save tag until something else refetches.
   * Nothing renders this, so no assertion about the screen can see it.
   */
  it("puts what Rust stored back into the shared vault list", async () => {
    mount([vault({ id: "v1" }), vault({ id: "v2", captureTag: "other" })], "v1");
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());

    const field = screen.getByLabelText("Tag every capture with");
    fireEvent.change(field, { target: { value: "Capture" } });
    fireEvent.blur(field);

    await waitFor(() => {
      const mirrored = notesVaultsStore.getState().vaults;
      expect(mirrored?.find((entry) => entry.id === "v1")?.captureTag).toBe("capture");
    });
    // …and only that vault. A mirror that replaced the list would take the
    // other vault's setting with it.
    expect(notesVaultsStore.getState().vaults).toHaveLength(2);
    expect(notesVaultsStore.getState().vaults?.find((entry) => entry.id === "v2")?.captureTag).toBe(
      "other",
    );
  });

  /**
   * A7 (W3Capture's shape, one level out). The field has two producers and they
   * are SEQUENCED, not racing: the user's typing, then the save's response. Blur
   * and keep typing while the write is in flight and the response would land on
   * top of the keystrokes made since — silently, and with a value that looks
   * authoritative because it came from Rust.
   */
  it("does not overwrite what is being typed with the answer to an older save", async () => {
    // An array rather than a nullable local: TypeScript narrows a `let` that is
    // only assigned inside a callback to `never` at the read site, and the cast
    // that would silence it is the fixture-cast W3NoteFile warned about.
    const pending: ((vm: NoteVaultVm) => void)[] = [];
    mockSave.mockImplementation(() => new Promise<NoteVaultVm>((resolve) => pending.push(resolve)));
    mount([vault()]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());

    const field = screen.getByLabelText("Tag every capture with");
    fireEvent.change(field, { target: { value: "capture" } });
    fireEvent.blur(field);
    await waitFor(() => expect(pending).toHaveLength(1));

    // The person kept typing while the write was in flight.
    fireEvent.change(field, { target: { value: "capture/mobile" } });
    pending[0]?.(vault({ captureTag: "capture" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(field).toHaveValue("capture/mobile");
  });

  /**
   * A10. The same guard on the chooser, and it survived its first probe — the
   * code was right and nothing held it there. Reachable: two choices while the
   * first write is still in flight, after which the older answer would put the
   * older template back under a control already showing the newer one.
   */
  it("does not overwrite a newer template choice with the answer to an older save", async () => {
    const pending: ((vm: NoteVaultVm) => void)[] = [];
    mockSave.mockImplementation(() => new Promise<NoteVaultVm>((resolve) => pending.push(resolve)));
    mount([vault()]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());

    const select = screen.getByLabelText("Captures start from");
    fireEvent.change(select, { target: { value: "templates/capture.md" } });
    await waitFor(() => expect(pending).toHaveLength(1));
    fireEvent.change(select, { target: { value: "templates/journal-entry.md" } });
    await waitFor(() => expect(pending).toHaveLength(2));

    // The FIRST write answers last. Its answer is about a template the person
    // has already moved on from.
    pending[1]?.(vault({ captureTemplate: "templates/journal-entry.md" }));
    pending[0]?.(vault({ captureTemplate: "templates/capture.md" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(2));
    expect(select).toHaveValue("templates/journal-entry.md");
  });

  it("saves the chosen template as its vault-relative path and expresses nothing else", async () => {
    mount([vault()]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText("Captures start from"), {
      target: { value: "templates/capture.md" },
    });

    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockSave).toHaveBeenCalledWith("v1", {
      subfolder: null,
      journalTemplate: null,
      defaultTemplate: null,
      captureTemplate: "templates/capture.md",
      captureTag: null,
      cadence: null,
    });
  });

  it("clears the template to an empty path rather than leaving the setting alone", async () => {
    mount([vault({ captureTemplate: "templates/capture.md" })]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText("Captures start from"), { target: { value: "" } });

    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockSave.mock.calls[0]?.[1].captureTemplate).toBe("");
    expect(mockSave.mock.calls[0]?.[1].captureTag).toBeNull();
  });

  it("keeps showing a template the vault no longer has, and says so", async () => {
    mount([vault({ captureTemplate: "templates/gone.md" })]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());

    const select = screen.getByLabelText("Captures start from");
    expect(select).toHaveValue("templates/gone.md");
    await screen.findByText(CAPTURE_TEMPLATE_MISSING);
    expect(within(select).getByText("templates/gone.md — not in this vault")).toBeTruthy();
  });

  /**
   * An empty list from a failed read is not evidence a template is gone, and
   * accusing a perfectly good setting is how a person clears one that works.
   */
  it("does not call a template missing when the list simply failed to load", async () => {
    // `mockRejectedValue` builds its rejected promise the moment it is
    // CONFIGURED, so a rejection nothing ends up calling is an unhandled
    // rejection vitest reports while the test passes. The implementation form
    // defers the throw to the call, where the component's `catch` is waiting.
    mockTemplates.mockImplementation(async () => {
      throw new Error("offline");
    });
    mount([vault({ captureTemplate: "templates/capture.md" })]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());

    const select = await screen.findByLabelText("Captures start from");
    expect(select).toHaveValue("templates/capture.md");
    expect(screen.queryByText(CAPTURE_TEMPLATE_MISSING)).toBeNull();
  });

  it("saves the tag on blur and shows the folded spelling Rust stored", async () => {
    mount([vault()]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());

    const field = screen.getByLabelText("Tag every capture with");
    fireEvent.change(field, { target: { value: "#Quick Capture" } });
    fireEvent.blur(field);

    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockSave).toHaveBeenCalledWith("v1", {
      subfolder: null,
      journalTemplate: null,
      defaultTemplate: null,
      captureTemplate: null,
      captureTag: "#Quick Capture",
      cadence: null,
    });
    // AD-34-8: the field must end up showing what is in force, not what was
    // typed, or the form and the notes disagree about the tag's spelling.
    await waitFor(() => expect(field).toHaveValue("quick-capture"));
  });

  /**
   * A capture tagged `template` would make every captured thought a scaffold
   * (AD-82). Rust refuses it, and the form has to show the refusal rather than
   * keep displaying a tag no note will carry.
   */
  it("shows the tag cleared when Rust refused the template marker", async () => {
    mount([vault()]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());

    const field = screen.getByLabelText("Tag every capture with");
    fireEvent.change(field, { target: { value: "template" } });
    fireEvent.blur(field);

    await waitFor(() => expect(mockSave).toHaveBeenCalledWith("v1", expect.anything()));
    await waitFor(() => expect(field).toHaveValue(""));
  });

  it("keeps the old value and says so when the save does not land", async () => {
    mockSave.mockImplementation(async () => {
      throw new Error("read-only volume");
    });
    mount([vault({ captureTag: "capture" })]);
    await waitFor(() => expect(mockTemplates).toHaveBeenCalled());

    const field = screen.getByLabelText("Tag every capture with");
    fireEvent.change(field, { target: { value: "inbox" } });
    fireEvent.blur(field);

    await screen.findByText(CAPTURE_SAVE_FAILED);
    expect(field).toHaveValue("inbox");
  });

  // -------------------------------------------------------------------------
  // The consequence, which is the reason this surface exists
  // -------------------------------------------------------------------------

  it("asks Rust what the tag being TYPED would cost, before it is saved", async () => {
    mount([vault()]);
    await waitFor(() => expect(mockImpact).toHaveBeenCalledWith("v1", null));

    fireEvent.change(screen.getByLabelText("Tag every capture with"), {
      target: { value: "capture" },
    });

    await waitFor(() => expect(mockImpact).toHaveBeenCalledWith("v1", "capture"));
    expect(mockSave).not.toHaveBeenCalled();
  });

  it("renders every space the tag would displace, not only the first", async () => {
    mockImpact.mockResolvedValue([
      "A new note can't satisfy is:untagged, so this note is in the vault but won't appear in Inbox.",
      "A new note can't satisfy is:untagged, so this note is in the vault but won't appear in Unfiled.",
    ]);
    mount([vault({ captureTag: "capture" })]);

    await screen.findByText(CAPTURE_IMPACT_TITLE);
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0]?.textContent).toContain("Inbox");
    expect(items[1]?.textContent).toContain("Unfiled");
  });

  it("says nothing about consequences when there are none", async () => {
    mount([vault({ captureTag: "capture" })]);
    await waitFor(() => expect(mockImpact).toHaveBeenCalled());
    expect(screen.queryByText(CAPTURE_IMPACT_TITLE)).toBeNull();
  });

  /**
   * keeper could not work out the consequence, so it claims none. An invented
   * sentence about Inbox would be a claim about a query nobody evaluated.
   */
  it("shows no consequence rather than a guessed one when the read fails", async () => {
    mockImpact.mockImplementation(async () => {
      throw new Error("no index yet");
    });
    mount([vault({ captureTag: "capture" })]);
    await waitFor(() => expect(mockImpact).toHaveBeenCalled());
    expect(screen.queryByText(CAPTURE_IMPACT_TITLE)).toBeNull();
  });

  it("asks about no tag at all when the field is emptied", async () => {
    mount([vault({ captureTag: "capture" })]);
    await waitFor(() => expect(mockImpact).toHaveBeenCalledWith("v1", "capture"));

    fireEvent.change(screen.getByLabelText("Tag every capture with"), { target: { value: "  " } });

    await waitFor(() => expect(mockImpact).toHaveBeenLastCalledWith("v1", null));
  });
});
