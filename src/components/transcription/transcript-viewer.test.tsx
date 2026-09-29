import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  TranscriptFileViewer,
  TranscriptViewer,
  voicesDriveFor,
} from "@/components/transcription/transcript-viewer";
import * as ipc from "@/lib/ipc/client";
import { transcriptionStore } from "@/lib/stores/transcription";
import { resolveViewer } from "@/lib/viewers/registry";
import { TRANSCRIPT_FIXTURE } from "../../../dev/transcription-fixture";

vi.mock("@/lib/ipc/client", () => ({
  transcriptRead: vi.fn(),
  transcriptEditUtterance: vi.fn(),
  transcriptAssignSpeaker: vi.fn(),
  transcriptReassignUtterance: vi.fn(),
  transcriptRenameSpeaker: vi.fn(),
  transcriptMergeSpeakers: vi.fn(),
  transcriptSplitUtterance: vi.fn(),
  transcriptInsertUtterance: vi.fn(),
  dictionaryAcceptSuggestion: vi.fn(),
  transcriptionStatus: vi.fn(),
}));
vi.mock("@/components/viewers/text-file-viewer", () => ({
  TextFileViewer: ({ entry }: { entry: { format: string } }) => <p>Text viewer: {entry.format}</p>,
}));
const path = TRANSCRIPT_FIXTURE.path;
beforeEach(() => {
  vi.resetAllMocks();
  transcriptionStore.setState({ status: null, error: null, jobs: {}, people: {}, dictionary: {} });
  vi.mocked(ipc.transcriptRead).mockResolvedValue(structuredClone(TRANSCRIPT_FIXTURE));
  vi.mocked(ipc.transcriptionStatus).mockResolvedValue({
    available: true,
    reason: null,
    models: { state: "ready", sentence: "Ready", missing: [] },
    afterRecording: true,
    language: "auto",
    voicesDrives: [
      {
        profileId: "drive",
        name: "Drive",
        voicesRoot: "/Volumes/merope/tgdrive/voices",
        subfolder: "voices",
        localPath: "/Volumes/merope/tgdrive",
      },
    ],
  });
});
describe("Transcript corrections", () => {
  it("preserves the recognised words, saves an edit on Enter and remembers only an accepted suggestion", async () => {
    const corrected = structuredClone(TRANSCRIPT_FIXTURE);
    corrected.transcript.utterances[0].text = "Kowalski will join us today.";
    corrected.transcript.utterances[0].edited = true;
    vi.mocked(ipc.transcriptEditUtterance).mockResolvedValue({
      transcript: corrected,
      suggestions: [{ from: "Kowalsky", to: "Kowalski" }],
    });
    vi.mocked(ipc.dictionaryAcceptSuggestion).mockResolvedValue([
      { id: "term", text: "Kowalski", aliases: ["Kowalsky"] },
    ]);
    render(<TranscriptViewer path={path} profileId="drive" />);
    fireEvent.click(await screen.findByRole("button", { name: /^Edit u1:/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Edit u1" }), {
      target: { value: "Kowalski will join us today." },
    });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Edit u1" }), { key: "Enter" });
    await screen.findByRole("button", { name: "Accept" });
    expect(
      screen.getByRole("button", { name: "Edit u1: Kowalski will join us today." }),
    ).toBeVisible();
    expect(screen.getByText("Kowalsky will join us today.")).toBeInTheDocument();
    expect(ipc.dictionaryAcceptSuggestion).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Accept" })).not.toBeInTheDocument(),
    );
    expect(transcriptionStore.getState().dictionary.drive).toEqual([
      { id: "term", text: "Kowalski", aliases: ["Kowalsky"] },
    ]);
    expect(ipc.transcriptEditUtterance).toHaveBeenCalledWith(
      path,
      "u1",
      "Kowalski will join us today.",
    );
    expect(ipc.dictionaryAcceptSuggestion).toHaveBeenCalledWith("drive", "Kowalsky", "Kowalski");
  });
  it("cancels a draft without persisting it", async () => {
    render(<TranscriptViewer path={path} />);
    fireEvent.click(await screen.findByRole("button", { name: /^Edit u1:/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Edit u1" }), {
      target: { value: "Not saved" },
    });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Edit u1" }), { key: "Escape" });
    expect(
      screen.getByRole("button", { name: "Edit u1: Kowalsky will join us today." }),
    ).toBeVisible();
    expect(ipc.transcriptEditUtterance).not.toHaveBeenCalled();
  });
  it.each([
    false,
    true,
  ])("confirms an existing or newly named person in the returned transcript (new=%s)", async (create) => {
    const assigned = structuredClone(TRANSCRIPT_FIXTURE);
    assigned.transcript.speakers[1] = {
      ...assigned.transcript.speakers[1],
      name: create ? "Jo" : "Anna Kowalski",
      status: "confirmed",
    };
    vi.mocked(ipc.transcriptAssignSpeaker).mockResolvedValue(assigned);
    render(<TranscriptViewer path={path} />);
    const select = await screen.findByRole("combobox", { name: "This is… S1" });
    fireEvent.change(select, { target: { value: create ? "new" : "p-anna" } });
    if (create) {
      fireEvent.change(screen.getByRole("textbox", { name: "New person name" }), {
        target: { value: "Jo" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Create person" }));
    }
    expect(await screen.findByText("Confirmed")).toBeVisible();
    expect(ipc.transcriptAssignSpeaker).toHaveBeenCalledWith(
      path,
      "S1",
      create ? null : "p-anna",
      create ? "Jo" : null,
    );
    expect(screen.getByRole("combobox", { name: "Speaker for u2" })).toHaveDisplayValue(
      create ? "Jo" : "Anna Kowalski",
    );
  });
  it("keeps a refused correction editable with the shell's sentence", async () => {
    vi.mocked(ipc.transcriptEditUtterance).mockRejectedValue(
      new Error("This transcript is read-only."),
    );
    render(<TranscriptViewer path={path} />);
    fireEvent.click(await screen.findByRole("button", { name: /^Edit u1:/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Edit u1" }), {
      target: { value: "My correction" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save text" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("This transcript is read-only.");
    expect(screen.getByRole("textbox", { name: "Edit u1" })).toHaveValue("My correction");
  });
  it("drops a late transcript read after changing paths", async () => {
    let resolveOld: (value: ipc.TranscriptVm) => void = () => {};
    vi.mocked(ipc.transcriptRead).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveOld = resolve;
        }),
    );
    const next = structuredClone(TRANSCRIPT_FIXTURE);
    next.path = "/other.transcript.json";
    next.transcript.source.files = ["other.wav"];
    vi.mocked(ipc.transcriptRead).mockResolvedValueOnce(next);
    const view = render(<TranscriptViewer path={path} />);
    view.rerender(<TranscriptViewer path={next.path} />);
    await screen.findByRole("heading", { name: "other.wav" });
    await act(async () => resolveOld(TRANSCRIPT_FIXTURE));
    expect(screen.getByRole("heading", { name: "other.wav" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "meeting.mov" })).not.toBeInTheDocument();
  });
  it("hides a speaker with no lines from the legend but offers it as a reassign target", async () => {
    render(<TranscriptViewer path={path} />);
    await screen.findByRole("combobox", { name: "This is… S1" });
    expect(screen.queryByRole("combobox", { name: "This is… S2" })).toBeNull();
    const reassign = screen.getByRole("combobox", { name: "Speaker for u2" });
    expect(within(reassign).getByRole("option", { name: "Speaker 2" })).toBeInTheDocument();
  });
  it("shows Rust's sentence when a line cannot cross between microphone and call", async () => {
    const sentence =
      "A line heard on your microphone cannot move to a voice from the call, or back.";
    vi.mocked(ipc.transcriptReassignUtterance).mockRejectedValue({
      code: "refused",
      message: sentence,
    });
    render(<TranscriptViewer path={path} />);
    const reassign = await screen.findByRole("combobox", { name: "Speaker for u1" });
    fireEvent.change(reassign, { target: { value: "S2" } });
    expect(await screen.findByRole("alert")).toHaveTextContent(sentence);
    expect(reassign).toHaveDisplayValue("Alex");
  });
  it("cancels a label without saving it", async () => {
    render(<TranscriptViewer path={path} />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Rename label" }))[1]);
    fireEvent.change(screen.getByRole("textbox", { name: "Speaker label" }), {
      target: { value: "Should not be saved" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("textbox", { name: "Speaker label" })).toBeNull();
    expect(ipc.transcriptRenameSpeaker).not.toHaveBeenCalled();
  });
  it("hands the bank's people from an assignment to the People list", async () => {
    const assigned = structuredClone(TRANSCRIPT_FIXTURE);
    const jo = { ...assigned.people[1], id: "p-jo", name: "Jo" };
    assigned.people = [...assigned.people, jo];
    vi.mocked(ipc.transcriptAssignSpeaker).mockResolvedValue(assigned);
    render(<TranscriptViewer path={path} />);
    await waitFor(() => expect(transcriptionStore.getState().status).not.toBeNull());
    fireEvent.change(await screen.findByRole("combobox", { name: "This is… S1" }), {
      target: { value: "new" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "New person name" }), {
      target: { value: "Jo" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create person" }));
    await waitFor(() => expect(transcriptionStore.getState().people.drive).toContainEqual(jo));
  });
  it("splits a line at the chosen word and shows the two lines Rust returns", async () => {
    const split = structuredClone(TRANSCRIPT_FIXTURE);
    const u2 = split.transcript.utterances[1];
    split.transcript.utterances.splice(2, 0, {
      ...u2,
      id: "u4",
      text: "keeper release and the dictionary.",
      asrText: "keeper release and the dictionary.",
      words: u2.words.slice(3),
    });
    Object.assign(u2, { text: "Let’s review the", asrText: "Let’s review the" });
    u2.words = u2.words.slice(0, 3);
    vi.mocked(ipc.transcriptSplitUtterance).mockResolvedValue(split);
    render(<TranscriptViewer path={path} />);
    fireEvent.click(await screen.findByRole("button", { name: "Split… u2" }));
    // The first word cannot start the new line: nothing would be left on this one.
    expect(
      screen.getByRole("button", { name: "Start the new line at word 1, Let’s" }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Start the new line at word 4, keeper" }));
    expect(
      await screen.findByRole("button", { name: "Edit u4: keeper release and the dictionary." }),
    ).toBeVisible();
    expect(ipc.transcriptSplitUtterance).toHaveBeenCalledWith(path, "u2", 3);
    expect(screen.getByRole("button", { name: "Edit u2: Let’s review the" })).toBeVisible();
    const lines = within(screen.getByRole("list", { name: "Utterances" }))
      .getAllByRole("button", { name: /^Edit u\d+:/ })
      .map((line) => line.getAttribute("aria-label")?.split(":")[0]);
    expect(lines).toEqual(["Edit u1", "Edit u2", "Edit u4", "Edit u3"]);
    expect(screen.queryByRole("button", { name: /^Start the new line/ })).toBeNull();
  });
  it("keeps the split open with Rust's sentence when it is refused", async () => {
    vi.mocked(ipc.transcriptSplitUtterance).mockRejectedValue({
      code: "refused",
      message: "A line splits between two of its words.",
    });
    render(<TranscriptViewer path={path} />);
    fireEvent.click(await screen.findByRole("button", { name: "Split… u1" }));
    fireEvent.click(screen.getByRole("button", { name: "Start the new line at word 2, will" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "A line splits between two of its words.",
    );
    expect(
      screen.getByRole("button", { name: "Start the new line at word 2, will" }),
    ).toBeEnabled();
  });
  it("offers no split on a line with fewer than two words", async () => {
    const typed = structuredClone(TRANSCRIPT_FIXTURE);
    typed.transcript.utterances[0].words = [];
    typed.transcript.utterances[1].words = typed.transcript.utterances[1].words.slice(0, 1);
    vi.mocked(ipc.transcriptRead).mockResolvedValue(typed);
    render(<TranscriptViewer path={path} />);
    await screen.findByRole("button", { name: "Split… u3" });
    expect(screen.queryByRole("button", { name: "Split… u1" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Split… u2" })).toBeNull();
  });
  it("adds a typed line after another for the chosen speaker", async () => {
    const inserted = structuredClone(TRANSCRIPT_FIXTURE);
    inserted.transcript.utterances.splice(1, 0, {
      id: "u4",
      speaker: "S1",
      origin: "system",
      start: 5,
      end: 5,
      text: "Sounds good.",
      asrText: "",
      edited: true,
      words: [],
    });
    vi.mocked(ipc.transcriptInsertUtterance).mockResolvedValue(inserted);
    render(<TranscriptViewer path={path} />);
    fireEvent.click(await screen.findByRole("button", { name: "Add a line after u1" }));
    const speaker = screen.getByRole("combobox", { name: "Speaker for the line after u1" });
    // It starts on the line's own speaker, and every speaker is offered.
    expect(speaker).toHaveDisplayValue("Alex");
    expect(within(speaker).getAllByRole("option")).toHaveLength(3);
    const text = screen.getByRole("textbox", { name: "Text of the line after u1" });
    fireEvent.change(text, { target: { value: "   " } });
    expect(screen.getByRole("button", { name: "Add line" })).toBeDisabled();
    fireEvent.change(speaker, { target: { value: "S1" } });
    fireEvent.change(text, { target: { value: "Sounds good." } });
    fireEvent.click(screen.getByRole("button", { name: "Add line" }));
    expect(await screen.findByRole("button", { name: "Edit u4: Sounds good." })).toBeVisible();
    expect(ipc.transcriptInsertUtterance).toHaveBeenCalledWith(path, "u1", "S1", "Sounds good.");
    expect(screen.getByRole("combobox", { name: "Speaker for u4" })).toHaveDisplayValue(
      "Speaker 1",
    );
    expect(screen.queryByRole("textbox", { name: "Text of the line after u1" })).toBeNull();
    // A typed line has no recognised text to reveal.
    expect(screen.getByText("Added by hand")).toBeVisible();
    expect(screen.queryByText("Edited · Recognised text")).toBeNull();
  });
  it("cancels an added line without saving it", async () => {
    render(<TranscriptViewer path={path} />);
    fireEvent.click(await screen.findByRole("button", { name: "Add a line after u3" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Text of the line after u3" }), {
      target: { value: "Not saved" },
    });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Text of the line after u3" }), {
      key: "Escape",
    });
    expect(screen.queryByRole("textbox", { name: "Text of the line after u3" })).toBeNull();
    expect(ipc.transcriptInsertUtterance).not.toHaveBeenCalled();
  });
});
describe("Transcript files", () => {
  it("opens a file keeper cannot read as a transcript as JSON text", async () => {
    vi.mocked(ipc.transcriptRead).mockRejectedValue({
      code: "refused",
      message: "This is not a keeper transcript.",
    });
    render(
      <TranscriptFileViewer
        file={{
          name: "transcript.json",
          kind: "file",
          relativePath: "code/transcript.json",
          profileId: "drive",
          absolutePath: "/Volumes/merope/tgdrive/code/transcript.json",
          sizeLabel: null,
          openWith: null,
          writeCaveat: null,
          writeCaveatShort: null,
          writeRefusal: null,
        }}
        entry={resolveViewer({ name: "transcript.json", kind: "file" })}
      />,
    );
    expect(await screen.findByText("Text viewer: json")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });
  it("finds a transcript's drive by whole folder, not by a name that starts the same", () => {
    const drive = (profileId: string, localPath: string) => ({
      profileId,
      name: profileId,
      voicesRoot: `${localPath}/voices`,
      subfolder: "voices",
      localPath,
    });
    const drives = [drive("work", "/Volumes/work"), drive("work2", "/Volumes/work2/")];
    expect(voicesDriveFor(drives, "/Volumes/work2/a.mov.transcript.json", null)?.profileId).toBe(
      "work2",
    );
    expect(voicesDriveFor(drives, "/Volumes/work/a.mov.transcript.json", null)?.profileId).toBe(
      "work",
    );
    expect(voicesDriveFor(drives, "/Volumes/work2/x", "work")?.profileId).toBe("work");
  });
});
