import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  revealPath: vi.fn(() => Promise.resolve()),
  recordingRetitle: vi.fn(),
  // Story 45.19: opening the editor reads the session's stored details, and
  // saving them writes back. Defaulted below to "a session with details", so
  // every pre-45.19 case still opens an editor whose four extra fields are
  // simply left alone.
  recordingSessionMeta: vi.fn(),
  recordingMetaUpdate: vi.fn(),
  // Story 42.5: the details editor's Tags field completes over the one tag
  // vocabulary. An empty vocabulary is this suite's world — nothing here is
  // about completion.
  tagsVocabulary: vi.fn(() => Promise.resolve({ entries: [] })),
  // Story 42.4: the card now hosts the note stub, which resolves its own note
  // through these three. Defaulted to "no stub" below, so every pre-42.4 case
  // renders exactly the DOM it always did.
  recordingNoteStub: vi.fn(),
  recordingNoteStubSave: vi.fn(),
  recordingNoteStubDismiss: vi.fn(),
}));

import {
  RECOVERY_DISMISS_LABEL,
  REVEAL_IN_FINDER_LABEL,
  RecordingSummaryCard,
  SUMMARY_DETAILS_UNAVAILABLE,
  SUMMARY_DETAILS_UNAVAILABLE_TESTID,
  SUMMARY_FOLDER_TESTID,
  SUMMARY_RETITLE_CANCEL_TESTID,
  SUMMARY_RETITLE_EDIT_TESTID,
  SUMMARY_RETITLE_FAULT_TESTID,
  SUMMARY_RETITLE_FIELD_TESTID,
  SUMMARY_RETITLE_LABEL,
  SUMMARY_RETITLE_SAVE_TESTID,
  SUMMARY_RETITLE_UNTITLED_LABEL,
} from "@/components/layout/recording-summary-card";
import {
  META_CUSTOM_NAME_LABEL,
  META_CUSTOM_VALUE_LABEL,
  META_NOTE_LABEL,
  META_PARTICIPANTS_LABEL,
  META_TAGS_LABEL,
} from "@/components/recording/recording-meta-fields";
import {
  NOTE_STUB_BODY_TESTID,
  NOTE_STUB_KEPT_TESTID,
} from "@/components/recording/recording-note-stub";
import type {
  RecordingNoteStubVm,
  RecordingSessionMetaVm,
  RecordingSummaryVm,
} from "@/lib/ipc/client";
import {
  recordingMetaUpdate,
  recordingNoteStub,
  recordingNoteStubDismiss,
  recordingNoteStubSave,
  recordingRetitle,
  recordingSessionMeta,
  revealPath,
} from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const mockReveal = vi.mocked(revealPath);
const mockRetitle = vi.mocked(recordingRetitle);
const mockSessionMeta = vi.mocked(recordingSessionMeta);
const mockMetaUpdate = vi.mocked(recordingMetaUpdate);
const mockNoteStub = vi.mocked(recordingNoteStub);
const mockNoteStubSave = vi.mocked(recordingNoteStubSave);
const mockNoteStubDismiss = vi.mocked(recordingNoteStubDismiss);

const FOLDER = "/Users/alice/Movies/keeper/keeper-rec 2026-07-19 14.23.45";

/** Where the rename lands: the template re-rendered with the new title. */
const MOVED_FOLDER = "/Users/alice/Movies/keeper/2026/2026-07-19 1423 standup";

/** The summary the command resolves — the session AT ITS NEW LOCATION. */
const MOVED: RecordingSummaryVm = {
  sessionFolder: MOVED_FOLDER,
  screenSegmentCount: 3,
  title: "Standup",
  totalBytes: 412_000_000,
};

/** The keeper-authored frontmatter, verbatim, and the body it prefilled. */
const STUB_FRONT = "---\ntitle: Standup\nsession: 01JDEVICE-01JSESSION\n---\n";
const STUB_BODY = "# Standup\n\n";

const STUB: RecordingNoteStubVm = {
  path: "/Users/alice/Movies/keeper/2026-07-19-standup.md",
  filename: "2026-07-19-standup.md",
  contents: `${STUB_FRONT}${STUB_BODY}`,
  bodyOffset: STUB_FRONT.length,
  inVault: false,
  sessionId: "01JDEVICE-01JSESSION",
  relativePath: "2026-07-19-standup.md",
};

/**
 * What the session's manifest currently says (Story 45.19).
 *
 * TWO custom rows and TWO tags on purpose: a change that keeps only the first
 * element of either collection passes every single-item fixture, and nothing in
 * the shape of the result says anything went missing.
 */
const STORED: RecordingSessionMetaVm = {
  title: "Standup",
  participants: "Ada, Grace",
  note: "weekly",
  tags: "standup, q3",
  custom: [
    { name: "Ticket", value: "KPR-1" },
    { name: "Room", value: "Blue" },
  ],
};

beforeEach(() => {
  mockReveal.mockReset();
  mockReveal.mockResolvedValue(undefined);
  mockRetitle.mockReset();
  mockRetitle.mockResolvedValue(MOVED);
  mockSessionMeta.mockReset();
  mockSessionMeta.mockResolvedValue(STORED);
  mockMetaUpdate.mockReset();
  // The command answers with what was STORED, which is what Rust does: it
  // trims, drops nameless rows and re-joins the tag line. Tests that care set
  // their own answer.
  mockMetaUpdate.mockImplementation((_folder, participants, note, tags, custom) =>
    Promise.resolve({ title: STORED.title, participants, note, tags, custom }),
  );
  mockNoteStub.mockReset();
  // No stub by default: a stub is an addition to this card, and every case that
  // predates Story 42.4 must render exactly the DOM it rendered before.
  mockNoteStub.mockResolvedValue(null);
  mockNoteStubSave.mockReset();
  mockNoteStubSave.mockResolvedValue(undefined);
  mockNoteStubDismiss.mockReset();
  mockNoteStubDismiss.mockResolvedValue(true);
  // Reveal is capability-gated: default it ON for the base cases.
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: true });
});

afterEach(() => {
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  vi.clearAllMocks();
});

describe("RecordingSummaryCard", () => {
  it("renders the completion variant: count, size, folder, and Reveal", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    expect(screen.getByText(/Saved 3 segments · 412 MB/)).toBeInTheDocument();
    expect(screen.getByText(FOLDER)).toBeInTheDocument();
    const reveal = screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL });
    fireEvent.click(reveal);
    expect(mockReveal).toHaveBeenCalledWith(FOLDER);
    // No dismiss on the completion variant (a finalized session is never dismissed).
    expect(screen.queryByRole("button", { name: RECOVERY_DISMISS_LABEL })).not.toBeInTheDocument();
  });

  it("says '1 segment' (singular) for a single-segment session", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={1}
        totalBytes={1_000_000}
      />,
    );
    expect(screen.getByText(/Saved 1 segment · 1 MB/)).toBeInTheDocument();
  });

  it("renders the recovered variant: interruption copy, warning edge, and Dismiss", () => {
    const onDismiss = vi.fn();
    render(
      <RecordingSummaryCard
        variant="recovered"
        sessionFolder={FOLDER}
        screenSegmentCount={2}
        totalBytes={200_000_000}
        onDismiss={onDismiss}
      />,
    );

    expect(
      screen.getByText(/A recording was interrupted; 2 segments were saved/),
    ).toBeInTheDocument();
    // The bridge-degraded warning edge (the bridge-card recipe). The card root,
    // not the live region — the announced outcome is a non-interactive block
    // inside it now that the card carries a rename editor.
    const card = screen.getByText(/A recording was interrupted/).closest("[data-slot='card']");
    expect(card?.className).toContain("border-bridge-degraded/50");
    expect(card?.className).toContain("text-bridge-degraded");

    fireEvent.click(screen.getByRole("button", { name: RECOVERY_DISMISS_LABEL }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("degrades to a figureless headline (never '0 segments · 0 MB') when the summary is unavailable", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={null}
        totalBytes={null}
      />,
    );
    // The honest degraded shape: outcome + folder + Reveal, no fabricated zero.
    expect(screen.getByText("Recording saved")).toBeInTheDocument();
    expect(screen.queryByText(/0 segments/)).not.toBeInTheDocument();
    expect(screen.queryByText(/0 MB/)).not.toBeInTheDocument();
    expect(screen.getByText(FOLDER)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL })).toBeInTheDocument();
  });

  it("degrades the recovered variant to the interruption headline without fabricated figures", () => {
    render(
      <RecordingSummaryCard
        variant="recovered"
        sessionFolder={FOLDER}
        screenSegmentCount={null}
        totalBytes={null}
        onDismiss={vi.fn()}
      />,
    );
    expect(screen.getByText("A recording was interrupted")).toBeInTheDocument();
    expect(screen.queryByText(/0 segments/)).not.toBeInTheDocument();
  });

  it("hides the Reveal button when revealInFileManager is false", () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: false });
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={1}
        totalBytes={1_000_000}
      />,
    );
    expect(screen.queryByRole("button", { name: REVEAL_IN_FINDER_LABEL })).not.toBeInTheDocument();
  });

  it("saves a typed title through recordingRetitle with the session's folder", async () => {
    const onRetitled = vi.fn();
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
        onRetitled={onRetitled}
      />,
    );

    // An untitled session invites a name rather than offering a verb.
    fireEvent.click(screen.getByText(SUMMARY_RETITLE_UNTITLED_LABEL));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    await waitFor(() => expect(mockRetitle).toHaveBeenCalledWith(FOLDER, "Standup"));
    await waitFor(() => expect(onRetitled).toHaveBeenCalledWith(MOVED));
  });

  it("re-renders the folder and title the rename RESOLVED, and reveals the new path", async () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    // The old path is gone from disk, so it must be gone from the card: the
    // mono line — and Reveal in Finder, which points at it — is the NEW one.
    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(MOVED_FOLDER),
    );
    expect(screen.queryByText(FOLDER)).not.toBeInTheDocument();
    expect(screen.getByText("Standup")).toBeInTheDocument();
    // The editor closed on success, so the affordance reads as a rename now.
    expect(screen.getByText(SUMMARY_RETITLE_LABEL)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL }));
    expect(mockReveal).toHaveBeenCalledWith(MOVED_FOLDER);
  });

  it("prints a live-session refusal verbatim and keeps the typed text", async () => {
    mockRetitle.mockRejectedValue({
      code: "recordingSessionLive",
      message: "This recording is still running. Stop it before renaming the session.",
      accountId: null,
      retriable: false,
    });
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_RETITLE_FAULT_TESTID).textContent).toBe(
        "This recording is still running. Stop it before renaming the session.",
      ),
    );
    // The words the refusal is about are still there to be corrected, and the
    // session has not moved.
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Standup");
    expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(FOLDER);

    // Cancel retracts the refusal along with the edit.
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_CANCEL_TESTID));
    expect(screen.queryByTestId(SUMMARY_RETITLE_FAULT_TESTID)).not.toBeInTheDocument();
  });

  it("disables Save while the rename is in flight", async () => {
    // The rename is held open so the button can be observed mid-flight. The
    // executor form, not `Promise.withResolvers`: the project compiles against
    // `lib: ES2020`, where that constructor method does not exist.
    let land!: (summary: RecordingSummaryVm) => void;
    mockRetitle.mockReturnValue(
      new Promise<RecordingSummaryVm>((resolve) => {
        land = resolve;
      }),
    );
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    await waitFor(() => expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeDisabled());
    expect(mockRetitle).toHaveBeenCalledTimes(1);
    // A second click while the first is in flight sends nothing.
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    expect(mockRetitle).toHaveBeenCalledTimes(1);

    land(MOVED);
    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(MOVED_FOLDER),
    );
  });

  it("disables Save while the text is unchanged, and enables it on a real edit", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    // The editor opens seeded with the current title — nothing to save yet.
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Standup");
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeDisabled();

    // Whitespace around the same title is the same title.
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "  Standup  " },
    });
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeDisabled();

    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Retro" },
    });
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeEnabled();

    // Clearing a titled session IS an edit (it moves back to the untitled path).
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "" },
    });
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeEnabled();
  });

  it("dismisses the folder the session moved to, not the path the rename invalidated", async () => {
    // A dismissal is keyed off the manifest at the folder it is handed, and the
    // pre-rename folder no longer has one — latching there is a silent no-op
    // and the card returns on the next scan.
    const onDismiss = vi.fn();
    render(
      <RecordingSummaryCard
        variant="recovered"
        sessionFolder={FOLDER}
        screenSegmentCount={2}
        totalBytes={200_000_000}
        onDismiss={onDismiss}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(MOVED_FOLDER),
    );

    fireEvent.click(screen.getByRole("button", { name: RECOVERY_DISMISS_LABEL }));
    expect(onDismiss).toHaveBeenCalledWith(MOVED_FOLDER);
  });

  it("holds the renamed session's folder and title while the owner adopts the move", async () => {
    const { rerender } = render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    await waitFor(() => expect(screen.getByText("Standup")).toBeInTheDocument());

    // The owner adopts the new folder and re-fetches its summary, so the title
    // and figures are momentarily absent again. The card must not flash back to
    // "untitled with no figures" for a session it just renamed.
    rerender(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={MOVED_FOLDER}
        title={null}
        screenSegmentCount={null}
        totalBytes={null}
      />,
    );
    expect(screen.getByText("Standup")).toBeInTheDocument();
    expect(screen.getByText(/Saved 3 segments · 412 MB/)).toBeInTheDocument();

    // A DIFFERENT session in the same slot retires the override entirely.
    rerender(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder="/Users/alice/Movies/keeper/keeper-rec 2026-07-20 09.00.00"
        title={null}
        screenSegmentCount={1}
        totalBytes={1_000_000}
      />,
    );
    await waitFor(() => expect(screen.queryByText("Standup")).not.toBeInTheDocument());
    expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(
      "/Users/alice/Movies/keeper/keeper-rec 2026-07-20 09.00.00",
    );
  });

  it("re-seeds the editor when the title lands, so an empty draft cannot clear it", async () => {
    // The completion card's title arrives one IPC round trip after the terminal
    // settles. An editor opened inside that window shows an empty field for a
    // session that HAS a title, and saving it would clear the title and move the
    // folder back to the untitled path.
    const { rerender } = render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title={null}
        screenSegmentCount={null}
        totalBytes={null}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("");
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeDisabled();

    rerender(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Standup"),
    );
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeDisabled();
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    expect(mockRetitle).not.toHaveBeenCalled();
  });

  it("keeps a draft the user typed when the title lands mid-edit", async () => {
    const { rerender } = render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title={null}
        screenSegmentCount={null}
        totalBytes={null}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Retro" },
    });

    rerender(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    // The re-seed is for an UNTOUCHED draft only — typed words are the user's.
    await waitFor(() => expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeEnabled());
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Retro");
  });

  it("keeps the rename editor out of the live region and carries focus with it", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    const field = screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID);
    // Focus lands on the field the click opened, not on document.body.
    expect(field).toHaveFocus();
    // A text input inside an aria-atomic `role="status"` re-announces the whole
    // card — headline and full folder path — on every keystroke.
    const live = screen.getByRole("status");
    expect(live).not.toContainElement(field);
    expect(live).not.toContainElement(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    // The announced outcome still IS a live region.
    expect(live).toContainElement(screen.getByTestId(SUMMARY_FOLDER_TESTID));

    // Leaving the editor returns focus to the control that opened it.
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_CANCEL_TESTID));
    expect(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID)).toHaveFocus();
  });

  it("presents the note stub inside the card, prefilled, with the cursor in the body", async () => {
    mockNoteStub.mockResolvedValue(STUB);
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    const body = await screen.findByTestId<HTMLTextAreaElement>(NOTE_STUB_BODY_TESTID);
    // Resolved from the folder this card is showing, and prefilled with the
    // body alone — keeper's frontmatter is never on screen to be broken.
    expect(mockNoteStub).toHaveBeenCalledWith(FOLDER);
    expect(body.value).toBe(STUB_BODY);
    // UX-DR51: the cursor is in the body, not on the card. Waited for rather
    // than asserted straight after the `findBy` — the textarea is in the DOM as
    // soon as the stub resolves and the focus lands an effect later, and that
    // gap is only wide enough to lose under the load of a full run.
    await waitFor(() => expect(body).toHaveFocus());
    // Story 42.4 does not displace Story 20.3 — the recording still reports
    // what it saved, and where.
    expect(screen.getByText(/Saved 3 segments · 412 MB/)).toBeInTheDocument();
    expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(FOLDER);
    // And the note sits OUTSIDE the live region: a textarea inside an
    // aria-atomic `role="status"` re-announces the whole card per keystroke.
    expect(screen.getByRole("status")).not.toContainElement(body);
  });

  it("dismisses the stub from the card with one key, leaving the summary whole", async () => {
    mockNoteStub.mockResolvedValue(STUB);
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.keyDown(await screen.findByTestId(NOTE_STUB_BODY_TESTID), { key: "Escape" });

    await waitFor(() => expect(mockNoteStubDismiss).toHaveBeenCalledWith(FOLDER));
    expect(mockNoteStubDismiss).toHaveBeenCalledTimes(1);
    expect(mockNoteStubSave).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.queryByTestId(NOTE_STUB_BODY_TESTID)).not.toBeInTheDocument(),
    );
    // The card the note was dismissed from is untouched.
    expect(screen.getByText(/Saved 3 segments · 412 MB/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL })).toBeInTheDocument();
    expect(screen.queryByTestId(NOTE_STUB_KEPT_TESTID)).not.toBeInTheDocument();
  });

  it("renders the summary in full when the stub could not be written", async () => {
    // The write failed and was logged in Rust. Finalize still succeeded, and
    // this card must go on saying exactly that.
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    await waitFor(() => expect(mockNoteStub).toHaveBeenCalledWith(FOLDER));
    expect(screen.queryByTestId(NOTE_STUB_BODY_TESTID)).not.toBeInTheDocument();
    expect(screen.getByText("Standup")).toBeInTheDocument();
    expect(screen.getByText(/Saved 3 segments · 412 MB/)).toBeInTheDocument();
    expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(FOLDER);
    expect(screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL })).toBeInTheDocument();
    // The rename affordance still works with nothing in the note's place.
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveFocus();
  });

  it("never resolves a stub for a recovered card", async () => {
    // A crash salvage surfaces hours later, which is not the minute the story
    // is about — and one recovery scan would otherwise become one directory
    // read per listed session.
    mockNoteStub.mockResolvedValue(STUB);
    render(
      <RecordingSummaryCard
        variant="recovered"
        sessionFolder={FOLDER}
        screenSegmentCount={2}
        totalBytes={200_000_000}
        onDismiss={vi.fn()}
      />,
    );

    await waitFor(() =>
      expect(screen.getByText(/A recording was interrupted/)).toBeInTheDocument(),
    );
    expect(mockNoteStub).not.toHaveBeenCalled();
    expect(screen.queryByTestId(NOTE_STUB_BODY_TESTID)).not.toBeInTheDocument();
  });

  it("follows the session to its new folder when a rename moves it mid-note", async () => {
    mockNoteStub.mockResolvedValue(STUB);
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );
    const body = await screen.findByTestId(NOTE_STUB_BODY_TESTID);
    fireEvent.change(body, { target: { value: `${STUB_BODY}Half a sentence` } });

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    // The rename MOVES the session; the note is resolved from the folder the
    // card now points at, and the words typed into it survive the move.
    await waitFor(() => expect(mockNoteStub).toHaveBeenCalledWith(MOVED_FOLDER));
    expect(screen.getByTestId(NOTE_STUB_BODY_TESTID)).toHaveValue(`${STUB_BODY}Half a sentence`);
  });

  // --- Story 45.19: every field of the "Next session" form, on the last
  // recording. ----------------------------------------------------------------

  /** Open the editor and wait for the session's stored details to land. */
  const openDetails = async (): Promise<void> => {
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    await waitFor(() =>
      expect(screen.getByLabelText(META_PARTICIPANTS_LABEL)).toHaveValue(STORED.participants),
    );
  };

  it("opens the editor on what the session's manifest actually holds", async () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    await openDetails();
    // Asserted on the CALL as well as the values: the folder is what the read
    // is about, and a read of the wrong session would fill the form with
    // somebody else's meeting and look entirely normal.
    expect(mockSessionMeta).toHaveBeenCalledWith(FOLDER);
    // The readable case asserts the ABSENCE of the unreadable one, and that the
    // fields are live. Without these two, a change that raised
    // `detailsUnavailable` for every session would still satisfy every
    // `toHaveValue` below — the values would be right and frozen, which is the
    // same DOM to a value assertion and a dead editor to the user.
    expect(screen.queryByTestId(SUMMARY_DETAILS_UNAVAILABLE_TESTID)).toBeNull();
    expect(screen.getByLabelText(META_PARTICIPANTS_LABEL)).toBeEnabled();
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Standup");
    expect(screen.getByLabelText(META_NOTE_LABEL)).toHaveValue(STORED.note);
    expect(screen.getByLabelText(META_TAGS_LABEL)).toHaveValue(STORED.tags);
    // BOTH custom rows: a read that kept only the first would leave a card that
    // silently drops the second on the next save.
    expect(
      screen.getAllByLabelText(META_CUSTOM_NAME_LABEL).map((i) => (i as HTMLInputElement).value),
    ).toEqual(["Ticket", "Room"]);
    expect(
      screen.getAllByLabelText(META_CUSTOM_VALUE_LABEL).map((i) => (i as HTMLInputElement).value),
    ).toEqual(["KPR-1", "Blue"]);
  });

  it("sends every edited detail to the manifest, and leaves the title alone", async () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    await openDetails();
    fireEvent.change(screen.getByLabelText(META_PARTICIPANTS_LABEL), {
      target: { value: "Ada, Grace, Sam" },
    });
    fireEvent.change(screen.getByLabelText(META_NOTE_LABEL), { target: { value: "monthly" } });
    fireEvent.change(screen.getByLabelText(META_TAGS_LABEL), {
      target: { value: "standup, q3, demo" },
    });
    fireEvent.change(screen.getAllByLabelText(META_CUSTOM_VALUE_LABEL)[1] as HTMLElement, {
      target: { value: "Green" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    // The CALL, not merely the fact that a save happened: the tag line goes
    // whole (Story 42.5 — Rust splits it), and both custom rows travel.
    await waitFor(() =>
      expect(mockMetaUpdate).toHaveBeenCalledWith(
        FOLDER,
        "Ada, Grace, Sam",
        "monthly",
        "standup, q3, demo",
        [
          { name: "Ticket", value: "KPR-1" },
          { name: "Room", value: "Green" },
        ],
      ),
    );
    // The title did not change, so the session did not MOVE. A details edit
    // that renamed the folder would be a file operation nobody asked for.
    expect(mockRetitle).not.toHaveBeenCalled();
  });

  it("writes the details before the rename, so a refused rename cannot cost them", async () => {
    mockRetitle.mockRejectedValue({
      code: "recordingSessionLive",
      message: "Stop the recording first.",
    });
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    await openDetails();
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Retro" },
    });
    fireEvent.change(screen.getByLabelText(META_PARTICIPANTS_LABEL), { target: { value: "Ada" } });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_RETITLE_FAULT_TESTID)).toHaveTextContent(
        "Stop the recording first.",
      ),
    );
    // The field POINTS at that sentence, and the reference RESOLVES. A dangling
    // `aria-describedby` renders byte-identically — the attribute is there, the
    // testid query still finds the paragraph, both look right — and the only
    // thing lost is the announcement to the person who cannot see the red text.
    //
    // `toHaveAccessibleDescription` rather than reading the attribute and
    // looking the id up by hand: it computes the description THROUGH the
    // reference, so it cannot pass while the reference dangles, and it is also
    // the shortest way to write the assertion. The checking form and the
    // convenient form are the same form, so the next person cannot write the
    // weaker one by accident.
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveAccessibleDescription(
      "Stop the recording first.",
    );
    // The details landed first and stayed landed; only the rename was refused.
    expect(mockMetaUpdate).toHaveBeenCalledTimes(1);
    expect(mockMetaUpdate.mock.invocationCallOrder[0]).toBeLessThan(
      mockRetitle.mock.invocationCallOrder[0] as number,
    );
    // And the editor is still open on the user's text, so the reason is
    // actionable rather than a sentence beside an empty card.
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Retro");
  });

  it("does not attempt the rename when the details write failed", async () => {
    // The invariant that makes the two sequenced writes safe to share ONE fault
    // slot: they live in one `try`, so a failed details write SKIPS the rename.
    // Nothing pinned it. Move the rename out of that block — which reads like
    // ordinary "do both halves" code — and a refused details write would be
    // followed by a rename that MOVES the session, so the user gets a folder in
    // a new place and a sentence saying the save failed.
    mockMetaUpdate.mockRejectedValue({ code: "internal", message: "The disk is full." });
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    await openDetails();
    fireEvent.change(screen.getByLabelText(META_PARTICIPANTS_LABEL), { target: { value: "Ada" } });
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Retro" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_RETITLE_FAULT_TESTID)).toHaveTextContent(
        "The disk is full.",
      ),
    );
    // The session did not move. The witness for this absence is the test above,
    // which asserts the rename IS called on the same mock when the details
    // succeed — so a rename that stopped being wired at all fails there.
    expect(mockRetitle).not.toHaveBeenCalled();
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Retro");
  });

  it("repaints the fields from what Rust stored, not from what was typed", async () => {
    // Rust trims, drops a nameless custom row and re-joins the tag line. The
    // editor must show what is in the file — echoing the request back would
    // show the user their own typing and hide the rules that applied.
    mockMetaUpdate.mockResolvedValue({
      title: "Standup",
      participants: "Ada, Grace",
      note: "weekly",
      tags: "standup, q3",
      custom: [{ name: "Ticket", value: "KPR-1" }],
    });
    mockRetitle.mockRejectedValue({ code: "internal", message: "nope" });
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    await openDetails();
    fireEvent.change(screen.getByLabelText(META_PARTICIPANTS_LABEL), {
      target: { value: "   Ada, Grace   " },
    });
    // A title edit too, so the rename's refusal keeps the editor open and the
    // repainted fields are observable.
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Retro" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    await waitFor(() =>
      expect(screen.getByLabelText(META_PARTICIPANTS_LABEL)).toHaveValue("Ada, Grace"),
    );
    expect(screen.getAllByLabelText(META_CUSTOM_NAME_LABEL)).toHaveLength(1);
  });

  it("freezes the detail fields, and writes nothing, for a session with no manifest", async () => {
    mockSessionMeta.mockResolvedValue(null);
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_DETAILS_UNAVAILABLE_TESTID)).toHaveTextContent(
        SUMMARY_DETAILS_UNAVAILABLE,
      ),
    );
    expect(screen.getByLabelText(META_PARTICIPANTS_LABEL)).toBeDisabled();
    expect(screen.getByLabelText(META_TAGS_LABEL)).toBeDisabled();

    // The name still goes through, because the rename path has its own refusal
    // and will say so; the four fields keeper cannot read send nothing at all.
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Retro" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    await waitFor(() => expect(mockRetitle).toHaveBeenCalledWith(FOLDER, "Retro"));
    expect(mockMetaUpdate).not.toHaveBeenCalled();
  });

  it("reads and writes the folder the session moved to, not the one it was renamed from", async () => {
    const { rerender } = render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title={null}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );
    // Rename first: the card now points at MOVED_FOLDER even though the owner
    // still holds the old path.
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(MOVED_FOLDER),
    );
    rerender(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={MOVED_FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    mockSessionMeta.mockClear();
    await openDetails();
    expect(mockSessionMeta).toHaveBeenCalledWith(MOVED_FOLDER);
    fireEvent.change(screen.getByLabelText(META_NOTE_LABEL), { target: { value: "after" } });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    await waitFor(() => expect(mockMetaUpdate).toHaveBeenCalled());
    expect(mockMetaUpdate.mock.calls[0]?.[0]).toBe(MOVED_FOLDER);
  });

  it("gives each card's fields their own ids, so two open editors cannot cross labels", async () => {
    // Two recovery cards on screen at once is the ordinary shape of a scan that
    // salvaged two sessions, and the card's own comment claims its element ids
    // are per-card. Nothing enforced that: a constant prefix renders two inputs
    // with one id, every `<label for>` resolves to whichever the browser found
    // first, and typing a participant into the second card's field edits the
    // label of the first. Byte-identical to a single-card test.
    render(
      <>
        <RecordingSummaryCard
          variant="recovered"
          sessionFolder={FOLDER}
          title="Standup"
          screenSegmentCount={2}
          totalBytes={1_000_000}
          onDismiss={vi.fn()}
        />
        <RecordingSummaryCard
          variant="recovered"
          sessionFolder={MOVED_FOLDER}
          title="Retro"
          screenSegmentCount={1}
          totalBytes={1_000}
          onDismiss={vi.fn()}
        />
      </>,
    );

    for (const open of screen.getAllByTestId(SUMMARY_RETITLE_EDIT_TESTID)) {
      fireEvent.click(open);
    }
    await waitFor(() => expect(screen.getAllByLabelText(META_PARTICIPANTS_LABEL)).toHaveLength(2));
    const ids = screen.getAllByLabelText(META_PARTICIPANTS_LABEL).map((field) => field.id);
    expect(new Set(ids).size).toBe(2);
  });

  it("edits the details of a RECOVERED session too, on that card's own folder", async () => {
    // The second host for this editor. A recovery scan mounts these, one per
    // salvaged session, and a branch reachable only from the completion card is
    // a branch every completion-card test would pass over.
    render(
      <RecordingSummaryCard
        variant="recovered"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={2}
        totalBytes={1_000_000}
        onDismiss={vi.fn()}
      />,
    );

    await openDetails();
    fireEvent.change(screen.getByLabelText(META_NOTE_LABEL), { target: { value: "salvaged" } });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    await waitFor(() =>
      expect(mockMetaUpdate).toHaveBeenCalledWith(
        FOLDER,
        STORED.participants,
        "salvaged",
        STORED.tags,
        STORED.custom,
      ),
    );
  });
});
