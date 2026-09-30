import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createRef, StrictMode } from "react";
import { beforeEach, describe, expect, it, type MockInstance, vi } from "vitest";
import {
  BACK_LABEL,
  FRAME_PRIME_SECONDS,
  SCRUB_LABEL,
} from "@/components/notes/editor/media-playback";
import {
  TRANSCRIBE_AGAIN_BODY,
  TRANSCRIBE_AGAIN_LABEL,
} from "@/components/transcription/transcribe-again";
import { PART_NOT_HERE_SENTENCE } from "@/components/transcription/transcript-player";
import {
  TranscriptFileViewer,
  TranscriptViewer,
  type TranscriptViewerHandle,
  voicesDriveFor,
} from "@/components/transcription/transcript-viewer";
import { TranscriptionJob } from "@/components/transcription/transcription-job";
import { DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { WINDOW_ROW_ATTR, WINDOW_VIEWPORT_ATTR } from "@/components/ui/window-list";
import * as ipc from "@/lib/ipc/client";
import { transcriptionStore } from "@/lib/stores/transcription";
import { resolveViewer } from "@/lib/viewers/registry";
import { withListGeometry } from "@/test/layout";
import { SESSION_TRANSCRIPT_FIXTURE, TRANSCRIPT_FIXTURE } from "../../../dev/transcription-fixture";

vi.mock("@/lib/ipc/client", () => ({
  transcriptRead: vi.fn(),
  transcriptMedia: vi.fn(),
  transcriptAddSpeaker: vi.fn(),
  transcriptEditUtterance: vi.fn(),
  transcriptAssignSpeaker: vi.fn(),
  transcriptReassignUtterance: vi.fn(),
  transcriptRenameSpeaker: vi.fn(),
  transcriptMergeSpeakers: vi.fn(),
  transcriptSplitUtterance: vi.fn(),
  transcriptInsertUtterance: vi.fn(),
  dictionaryAcceptSuggestion: vi.fn(),
  transcriptionStatus: vi.fn(),
  transcriptionStart: vi.fn(),
  transcriptionCancel: vi.fn(),
  transcriptClip: vi.fn(),
  listenTranscriptWritten: vi.fn(),
}));
vi.mock("@/components/viewers/text-file-viewer", () => ({
  TextFileViewer: ({ entry }: { entry: { format: string } }) => <p>Text viewer: {entry.format}</p>,
}));
vi.mock("@/components/viewers/text-viewer", () => ({
  TextEditorSurface: ({
    content,
    language,
    readOnly,
  }: {
    content: string;
    language: string | null;
    readOnly?: boolean;
  }) => (
    <pre data-language={language} data-readonly={readOnly ? "" : undefined}>
      {content}
    </pre>
  ),
}));
const path = TRANSCRIPT_FIXTURE.path;
beforeEach(() => {
  vi.resetAllMocks();
  transcriptionStore.setState({ status: null, error: null, jobs: {}, people: {}, dictionary: {} });
  vi.mocked(ipc.transcriptRead).mockResolvedValue(structuredClone(TRANSCRIPT_FIXTURE));
  vi.mocked(ipc.transcriptMedia).mockResolvedValue({
    parts: [],
    hasCamera: false,
    hasScreen: false,
  });
  vi.mocked(ipc.transcriptionStart).mockResolvedValue("job");
  vi.mocked(ipc.listenTranscriptWritten).mockResolvedValue(() => {});
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
/** The line a ⋯ belongs to. */
const row = (id: string) =>
  screen.getByRole("button", { name: `Line actions ${id}` }).closest("li") as HTMLElement;
/** Open a menu from the keyboard, the way every menu here must be reachable. */
async function openMenu(trigger: HTMLElement): Promise<HTMLElement> {
  fireEvent.keyDown(trigger, { key: "Enter" });
  return screen.findByRole("menu");
}
async function lineMenu(id: string): Promise<HTMLElement> {
  return openMenu(await screen.findByRole("button", { name: `Line actions ${id}` }));
}
async function lineAction(id: string, item: string): Promise<void> {
  fireEvent.click(within(await lineMenu(id)).getByRole("menuitem", { name: item }));
}
/** A submenu of `menu`, opened with the arrow key. */
async function submenu(menu: HTMLElement, name: string): Promise<HTMLElement> {
  fireEvent.keyDown(within(menu).getByRole("menuitem", { name }), { key: "ArrowRight" });
  await waitFor(() => expect(screen.getAllByRole("menu")).toHaveLength(2));
  return screen.getAllByRole("menu")[1];
}
/** The line's "Change speaker" choices. */
async function speakerChoices(id: string): Promise<HTMLElement> {
  return submenu(await lineMenu(id), "Change speaker");
}
async function chipMenu(name: string): Promise<HTMLElement> {
  return openMenu(await screen.findByRole("button", { name: new RegExp(`^${name},`) }));
}
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
    await lineAction("u1", "Edit text");
    const field = screen.getByRole("textbox", { name: "Edit u1" });
    // Edit text lands the reader in the field, not back on the ⋯.
    await waitFor(() => expect(field).toHaveFocus());
    fireEvent.change(field, { target: { value: "Kowalski will join us today." } });
    fireEvent.keyDown(field, { key: "Enter" });
    await screen.findByRole("button", { name: "Accept" });
    expect(within(row("u1")).getByText("Kowalski will join us today.")).toBeVisible();
    expect(within(row("u1")).getByText("Kowalsky will join us today.")).toBeInTheDocument();
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
    await lineAction("u1", "Edit text");
    fireEvent.change(screen.getByRole("textbox", { name: "Edit u1" }), {
      target: { value: "Not saved" },
    });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Edit u1" }), { key: "Escape" });
    expect(within(row("u1")).getByText("Kowalsky will join us today.")).toBeVisible();
    expect(screen.queryByRole("textbox", { name: "Edit u1" })).toBeNull();
    expect(ipc.transcriptEditUtterance).not.toHaveBeenCalled();
  });
  it("reaches every line action from the keyboard, with the ⋯ in the tab order", async () => {
    vi.mocked(ipc.transcriptMedia).mockResolvedValue({
      hasCamera: false,
      hasScreen: false,
      parts: [{ ...SESSION_MEDIA.parts[0], file: "memo.m4a", camera: null, audioTracks: [] }],
    });
    render(<TranscriptViewer path={path} />);
    await screen.findByLabelText("memo.m4a");
    const trigger = screen.getByRole("button", { name: "Line actions u2" });
    expect(trigger).not.toHaveAttribute("tabindex", "-1");
    const menu = await openMenu(trigger);
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual([
      "Play from here",
      "Copy clip from here…",
      "Edit text",
      "Change speaker",
      "Split…",
      "Add a line after",
    ]);
  });
  it.each([
    false,
    true,
  ])("confirms an existing or newly named person in the returned transcript (new=%s)", async (create) => {
    const assigned = structuredClone(TRANSCRIPT_FIXTURE);
    const name = create ? "Jo" : "Anna Kowalski";
    assigned.transcript.speakers[1] = {
      ...assigned.transcript.speakers[1],
      name,
      status: "confirmed",
    };
    vi.mocked(ipc.transcriptAssignSpeaker).mockResolvedValue(assigned);
    render(<TranscriptViewer path={path} />);
    const people = await submenu(await chipMenu("Speaker 1"), "This is…");
    if (create) {
      fireEvent.click(within(people).getByRole("menuitem", { name: "New person…" }));
      fireEvent.change(screen.getByRole("textbox", { name: "New person name" }), {
        target: { value: "Jo" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Create person" }));
    } else {
      // The candidate comes first, with the score it was suggested at.
      const first = within(people).getAllByRole("menuitem")[0];
      expect(first).toHaveTextContent("Anna Kowalskisuggested 0.63");
      fireEvent.click(first);
    }
    expect(
      await screen.findByRole("button", { name: `${name}, Confirmed, 1 line` }),
    ).toBeInTheDocument();
    expect(ipc.transcriptAssignSpeaker).toHaveBeenCalledWith(
      path,
      "S1",
      create ? null : "p-anna",
      create ? "Jo" : null,
    );
    expect(within(row("u2")).getByText(name)).toBeInTheDocument();
  });
  it("keeps a refused correction editable with the shell's sentence", async () => {
    vi.mocked(ipc.transcriptEditUtterance).mockRejectedValue(
      new Error("This transcript is read-only."),
    );
    render(<TranscriptViewer path={path} />);
    await lineAction("u1", "Edit text");
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
    next.transcript.source.title = "other.wav";
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
    await screen.findByRole("button", { name: /^Speaker 1,/ });
    expect(screen.queryByRole("button", { name: /^Speaker 2,/ })).toBeNull();
    const choices = await speakerChoices("u2");
    expect(within(choices).getByRole("menuitemradio", { name: "Speaker 2" })).toBeInTheDocument();
  });
  it("shows Rust's sentence when a line cannot cross between microphone and call", async () => {
    const sentence =
      "A line heard on your microphone cannot move to a voice from the call, or back.";
    vi.mocked(ipc.transcriptReassignUtterance).mockRejectedValue({
      code: "refused",
      message: sentence,
    });
    render(<TranscriptViewer path={path} />);
    const choices = await speakerChoices("u1");
    expect(within(choices).getByRole("menuitemradio", { name: "Alex" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    fireEvent.click(within(choices).getByRole("menuitemradio", { name: "Speaker 2" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(sentence);
    expect(ipc.transcriptReassignUtterance).toHaveBeenCalledWith(path, "u1", "S2");
    expect(within(row("u1")).getByText("Alex")).toBeInTheDocument();
  });
  it("cancels a label without saving it", async () => {
    render(<TranscriptViewer path={path} />);
    fireEvent.click(
      within(await chipMenu("Speaker 1")).getByRole("menuitem", { name: "Rename label…" }),
    );
    const field = screen.getByRole("textbox", { name: "Speaker label" });
    expect(field).toHaveValue("Speaker 1");
    fireEvent.change(field, { target: { value: "Should not be saved" } });
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
    const people = await submenu(await chipMenu("Speaker 1"), "This is…");
    fireEvent.click(within(people).getByRole("menuitem", { name: "New person…" }));
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
    await lineAction("u2", "Split…");
    // The first word cannot start the new line: nothing would be left on this one.
    expect(
      screen.getByRole("button", { name: "Start the new line at word 1, Let’s" }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Start the new line at word 4, keeper" }));
    expect(await screen.findByRole("button", { name: "Line actions u4" })).toBeInTheDocument();
    expect(ipc.transcriptSplitUtterance).toHaveBeenCalledWith(path, "u2", 3);
    expect(within(row("u2")).getByText("Let’s review the")).toBeVisible();
    expect(within(row("u4")).getByText("keeper release and the dictionary.")).toBeVisible();
    const lines = within(screen.getByRole("list", { name: "Utterances" }))
      .getAllByRole("button", { name: /^Line actions u\d+$/ })
      .map((line) => line.getAttribute("aria-label"));
    expect(lines).toEqual([
      "Line actions u1",
      "Line actions u2",
      "Line actions u4",
      "Line actions u3",
    ]);
    expect(screen.queryByRole("button", { name: /^Start the new line/ })).toBeNull();
  });
  it("keeps the split open with Rust's sentence when it is refused", async () => {
    vi.mocked(ipc.transcriptSplitUtterance).mockRejectedValue({
      code: "refused",
      message: "A line splits between two of its words.",
    });
    render(<TranscriptViewer path={path} />);
    await lineAction("u1", "Split…");
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
    for (const [id, offered] of [
      ["u1", false],
      ["u2", false],
      ["u3", true],
    ] as const) {
      const menu = await lineMenu(id);
      expect(within(menu).queryByRole("menuitem", { name: "Split…" }) !== null).toBe(offered);
      fireEvent.keyDown(menu, { key: "Escape" });
      await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    }
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
    await lineAction("u1", "Add a line after");
    const speaker = screen.getByRole("combobox", { name: "Speaker for the line after u1" });
    // It starts on the line's own speaker, and every speaker is offered.
    expect(speaker).toHaveTextContent("Alex");
    const text = screen.getByRole("textbox", { name: "Text of the line after u1" });
    fireEvent.change(text, { target: { value: "   " } });
    expect(screen.getByRole("button", { name: "Add line" })).toBeDisabled();
    fireEvent.keyDown(speaker, { key: "Enter" });
    const options = await screen.findAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["Alex", "Speaker 1", "Speaker 2"]);
    fireEvent.click(options[1]);
    await waitFor(() => expect(speaker).toHaveTextContent("Speaker 1"));
    fireEvent.change(text, { target: { value: "Sounds good." } });
    fireEvent.click(screen.getByRole("button", { name: "Add line" }));
    expect(await screen.findByRole("button", { name: "Line actions u4" })).toBeInTheDocument();
    expect(ipc.transcriptInsertUtterance).toHaveBeenCalledWith(path, "u1", "S1", "Sounds good.");
    expect(within(row("u4")).getByText("Speaker 1")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Text of the line after u1" })).toBeNull();
    // A typed line has no recognised text to reveal.
    expect(within(row("u4")).getByText("Added by hand")).toBeVisible();
    expect(screen.queryByText("Edited · Recognised text")).toBeNull();
  });
  it("cancels an added line without saving it", async () => {
    render(<TranscriptViewer path={path} />);
    await lineAction("u3", "Add a line after");
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
describe("Speakers", () => {
  it("adds a speaker on the chosen track with its label and offers it to every line", async () => {
    const added = structuredClone(TRANSCRIPT_FIXTURE);
    added.transcript.speakers.push({
      ...added.transcript.speakers[2],
      id: "S3",
      origin: "microphone",
      name: "Guest",
    });
    vi.mocked(ipc.transcriptAddSpeaker).mockResolvedValue(added);
    render(<TranscriptViewer path={path} />);
    fireEvent.click(await screen.findByRole("button", { name: "Add speaker" }));
    const heard = screen.getByRole("radiogroup", { name: "Heard on" });
    fireEvent.click(within(heard).getByRole("radio", { name: "Microphone" }));
    expect(within(heard).getByRole("radio", { name: "Microphone" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    fireEvent.change(screen.getByRole("textbox", { name: "New speaker label" }), {
      target: { value: "  Guest " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Guest added.");
    expect(ipc.transcriptAddSpeaker).toHaveBeenCalledWith(path, "microphone", "Guest");
    expect(screen.queryByRole("form", { name: "Add speaker" })).toBeNull();
    const choices = await speakerChoices("u1");
    expect(within(choices).getByRole("menuitemradio", { name: "Guest" })).toBeInTheDocument();
  });
  it("asks for no track when the transcript heard only one, and keeps a refused entry", async () => {
    const mixed = structuredClone(TRANSCRIPT_FIXTURE);
    mixed.transcript.source.parts[0].tracks = [{ track: null, origin: "mixed" }];
    mixed.transcript.speakers = mixed.transcript.speakers.map((s) => ({ ...s, origin: "mixed" }));
    vi.mocked(ipc.transcriptRead).mockResolvedValue(mixed);
    vi.mocked(ipc.transcriptAddSpeaker).mockRejectedValue({
      code: "refused",
      message: "The transcript changed on disk.",
    });
    render(<TranscriptViewer path={path} />);
    fireEvent.click(await screen.findByRole("button", { name: "Add speaker" }));
    expect(screen.queryByRole("radiogroup", { name: "Heard on" })).toBeNull();
    fireEvent.change(screen.getByRole("textbox", { name: "New speaker label" }), {
      target: { value: "Guest" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The transcript changed on disk.");
    expect(ipc.transcriptAddSpeaker).toHaveBeenCalledWith(path, "mixed", "Guest");
    expect(screen.getByRole("textbox", { name: "New speaker label" })).toHaveValue("Guest");
  });
  it("lists each person once on a line, and still offers a lineless voice nobody else is", async () => {
    const twice = structuredClone(TRANSCRIPT_FIXTURE);
    const [, s1, s2] = twice.transcript.speakers;
    Object.assign(s1, { personId: "p-anna", name: "Anna Kowalski", status: "confirmed" });
    // A lineless voice that names Anna again: the file the owner opened.
    Object.assign(s2, { personId: "p-anna", name: "Anna Kowalski", status: "confirmed" });
    twice.transcript.speakers.push({ ...s2, id: "S3", personId: null, name: null });
    twice.transcript.speakers.push({ ...s2, id: "S4", personId: "p-jo", name: "Jo" });
    vi.mocked(ipc.transcriptRead).mockResolvedValue(twice);
    render(<TranscriptViewer path={path} />);
    const choices = await speakerChoices("u2");
    expect(
      within(choices)
        .getAllByRole("menuitemradio")
        .map((o) => o.textContent),
    ).toEqual(["Alex", "Anna Kowalski", "Speaker 3", "Jo"]);
  });
  it("counts each speaker's lines on its chip, with its status in the name", async () => {
    render(<TranscriptViewer path={path} />);
    const legend = await screen.findByRole("list", { name: "Speakers" });
    expect(
      within(legend)
        .getAllByRole("button", { name: /lines?$/ })
        .map((chip) => chip.getAttribute("aria-label")),
    ).toEqual(["Alex, You, 2 lines", "Speaker 1, Suggested, 1 line"]);
  });
});
describe("Source", () => {
  it("shows the transcript's JSON read-only, laid out as keeper writes the file", async () => {
    render(<TranscriptViewer path={path} />);
    fireEvent.mouseDown(await screen.findByRole("tab", { name: "Source" }));
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));
    const source = await screen.findByText((_, element) => element?.tagName === "PRE");
    expect(source.textContent).toBe(`${JSON.stringify(TRANSCRIPT_FIXTURE.transcript, null, 2)}\n`);
    expect(source).toHaveAttribute("data-language", "json");
    expect(source).toHaveAttribute("data-readonly");
    expect(screen.queryByRole("list", { name: "Utterances" })).toBeNull();
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Transcript" }));
    fireEvent.click(screen.getByRole("tab", { name: "Transcript" }));
    expect(await screen.findByRole("list", { name: "Utterances" })).toBeInTheDocument();
  });
});
describe("Transcribe again", () => {
  it("asks first, then replaces the transcript from its source and reads the new one", async () => {
    let progress: ((p: ipc.TranscriptionProgressVm) => void) | undefined;
    vi.mocked(ipc.transcriptionStart).mockImplementation((_path, onProgress) => {
      progress = onProgress;
      return Promise.resolve("job");
    });
    render(<TranscriptViewer path={path} />);
    const redo = await screen.findByRole("button", { name: `${TRANSCRIBE_AGAIN_LABEL}…` });
    fireEvent.click(redo);
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent(TRANSCRIBE_AGAIN_BODY);
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(ipc.transcriptionStart).not.toHaveBeenCalled();

    fireEvent.click(redo);
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: TRANSCRIBE_AGAIN_LABEL,
      }),
    );
    await waitFor(() =>
      expect(ipc.transcriptionStart).toHaveBeenCalledWith(
        TRANSCRIPT_FIXTURE.sourcePath,
        expect.any(Function),
        true,
      ),
    );
    const reads = vi.mocked(ipc.transcriptRead).mock.calls.length;
    await act(async () =>
      progress?.({
        jobId: "job",
        phase: "done",
        part: 1,
        parts: 1,
        message: null,
        transcriptPath: path,
        fraction: 1,
        elapsedMs: 1_000,
        replaceable: false,
      }),
    );
    await waitFor(() =>
      expect(vi.mocked(ipc.transcriptRead).mock.calls.length).toBeGreaterThan(reads),
    );
  });
  it("is not offered where the transcript's source is gone", async () => {
    vi.mocked(ipc.transcriptRead).mockResolvedValue({
      ...structuredClone(TRANSCRIPT_FIXTURE),
      sourcePath: null,
    });
    render(<TranscriptViewer path={path} />);
    await screen.findByRole("list", { name: "Utterances" });
    await waitFor(() => expect(transcriptionStore.getState().status).not.toBeNull());
    expect(screen.queryByRole("button", { name: `${TRANSCRIBE_AGAIN_LABEL}…` })).toBeNull();
  });
  it("turns the strip's Try again into Transcribe again… when a corrected transcript is in the way", async () => {
    const media = "/Volumes/merope/tgdrive/meeting.mov";
    const failed = (replaceable: boolean): ipc.TranscriptionProgressVm => ({
      jobId: "job",
      phase: "failed",
      part: 0,
      parts: 0,
      message: "This transcript has corrections in it.",
      transcriptPath: null,
      fraction: null,
      elapsedMs: 0,
      replaceable,
    });
    transcriptionStore.setState({ jobs: { [media]: failed(false) } });
    const { unmount } = render(<TranscriptionJob path={media} onOpen={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: `${TRANSCRIBE_AGAIN_LABEL}…` })).toBeNull();
    unmount();

    transcriptionStore.setState({ jobs: { [media]: failed(true) } });
    render(<TranscriptionJob path={media} onOpen={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: `${TRANSCRIBE_AGAIN_LABEL}…` }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: TRANSCRIBE_AGAIN_LABEL,
      }),
    );
    await waitFor(() =>
      expect(ipc.transcriptionStart).toHaveBeenCalledWith(media, expect.any(Function), true),
    );
  });
});
/** A two-part session with a camera beside each screen segment and two sound tracks. */
const SESSION_MEDIA: ipc.TranscriptMediaVm = {
  hasCamera: true,
  hasScreen: true,
  parts: [0, 1].map((index) => ({
    file: `screen-000${index}.mov`,
    offset: index * 21,
    duration: 21,
    screen: {
      via: "file" as const,
      profileId: "p1",
      relativePath: `rec/screen-000${index}.mov`,
      kind: "video" as const,
    },
    camera: {
      via: "file" as const,
      profileId: "p1",
      relativePath: `rec/camera-000${index}.mov`,
      kind: "video" as const,
    },
    audioTracks: [
      { index: 0, origin: "system" as const },
      { index: 1, origin: "microphone" as const },
    ],
    here: true,
  })),
};
const sessionPath = SESSION_TRANSCRIPT_FIXTURE.path;
const partLine = (text: string) =>
  screen.getByText((_, element) => element?.tagName === "P" && element.textContent === text);
describe("Player", () => {
  let play: MockInstance<HTMLMediaElement["play"]>;
  beforeEach(() => {
    vi.mocked(ipc.transcriptRead).mockResolvedValue(structuredClone(SESSION_TRANSCRIPT_FIXTURE));
    vi.mocked(ipc.transcriptMedia).mockResolvedValue(structuredClone(SESSION_MEDIA));
    play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  });
  it("highlights the line being spoken as the player plays, and not with Follow off", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    const main = (await screen.findByLabelText("screen-0000.mov")) as HTMLMediaElement;
    main.currentTime = 7;
    fireEvent.timeUpdate(main);
    expect(row("u2")).toHaveAttribute("aria-current", "true");
    expect(row("u1")).not.toHaveAttribute("aria-current");
    const follow = screen.getByRole("button", { name: "Follow the transcript" });
    expect(follow).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(follow);
    expect(follow).toHaveAttribute("aria-pressed", "false");
    expect(row("u2")).not.toHaveAttribute("aria-current");
  });
  it("takes the transcript to the line under the scrub while it is dragged, even after the reader scrolled away", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    await screen.findByLabelText("screen-0000.mov");
    const box = screen.getByRole("list", { name: "Utterances" }).parentElement as HTMLElement;
    const scrolled = vi.fn();
    Object.defineProperty(box, "scrollTop", { configurable: true, get: () => 0, set: scrolled });
    // jsdom lays nothing out: a viewport two lines tall, so the later lines are off-screen.
    Object.defineProperty(box, "clientHeight", { configurable: true, get: () => 150 });
    fireEvent.scroll(box);
    // The reader's own scroll pauses following…
    fireEvent.wheel(box);
    // …and a drag of the scrub is a seek, which takes it back: `input`, not a release.
    fireEvent.input(screen.getByRole("slider", { name: SCRUB_LABEL }), {
      target: { value: "30" },
    });
    expect(row("u5")).toHaveAttribute("aria-current", "true");
    expect(scrolled).toHaveBeenCalled();
    expect(scrolled.mock.lastCall?.[0]).toBeGreaterThan(0);
    // Back 10 seconds moves it too.
    scrolled.mockClear();
    fireEvent.wheel(box);
    fireEvent.click(screen.getByRole("button", { name: BACK_LABEL }));
    expect(row("u3")).toHaveAttribute("aria-current", "true");
  });
  it("plays from a line in the next part: that part loads and starts at the line", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    const first = (await screen.findByLabelText("screen-0000.mov")) as HTMLMediaElement;
    expect(partLine("Part 1 of 2 · screen-0000.mov")).toBeInTheDocument();
    await lineAction("u4", "Play from here");
    expect(partLine("Part 2 of 2 · screen-0001.mov")).toBeInTheDocument();
    // The first part's file is let go, not left decoding behind the second.
    expect(first).not.toHaveAttribute("src");
    const second = screen.getByLabelText("screen-0001.mov") as HTMLMediaElement;
    fireEvent.loadedMetadata(second);
    expect(second.currentTime).toBeCloseTo(0.5);
    expect(play.mock.contexts).toContain(second);
    expect(row("u4")).toHaveAttribute("aria-current", "true");
  });
  it("carries on into the next part when one ends", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    const first = (await screen.findByLabelText("screen-0000.mov")) as HTMLMediaElement;
    fireEvent.ended(first);
    const second = screen.getByLabelText("screen-0001.mov") as HTMLMediaElement;
    fireEvent.loadedMetadata(second);
    // At its start: the frame prime's millisecond is the only move it gets.
    expect(second.currentTime).toBeLessThanOrEqual(FRAME_PRIME_SECONDS);
    expect(play.mock.contexts).toContain(second);
    expect(partLine("Part 2 of 2 · screen-0001.mov")).toBeInTheDocument();
  });
  it("asks every unplayed video for its first frame, and never moves one that was placed", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    const screenVideo = (await screen.findByLabelText("screen-0000.mov")) as HTMLVideoElement;
    const camera = screen.getByLabelText("camera-0000.mov") as HTMLVideoElement;
    fireEvent.loadedMetadata(screenVideo);
    fireEvent.loadedMetadata(camera);
    expect(screenVideo.currentTime).toBe(FRAME_PRIME_SECONDS);
    expect(camera.currentTime).toBe(FRAME_PRIME_SECONDS);
    // A part loaded for a seek lands where it was sent, not a millisecond in.
    await lineAction("u5", "Play from here");
    const second = screen.getByLabelText("screen-0001.mov") as HTMLVideoElement;
    fireEvent.loadedMetadata(second);
    expect(second.currentTime).toBeCloseTo(7);
  });
  it("offers the picture and sound choices as icon groups only when there are two of each", async () => {
    const { unmount } = render(<TranscriptViewer path={sessionPath} />);
    const picture = await screen.findByRole("radiogroup", { name: "Picture" });
    const sound = screen.getByRole("radiogroup", { name: "Sound" });
    expect(
      within(picture)
        .getAllByRole("radio")
        .map((r) => [r.getAttribute("aria-label"), r.getAttribute("aria-checked")]),
    ).toEqual([
      ["Screen", "false"],
      ["Camera", "false"],
      ["Screen and camera", "true"],
    ]);
    expect(
      within(sound)
        .getAllByRole("radio")
        .map((r) => r.getAttribute("aria-label")),
    ).toEqual(["Call", "Microphone", "Call and microphone"]);
    fireEvent.click(within(picture).getByRole("radio", { name: "Screen" }));
    expect(within(picture).getByRole("radio", { name: "Screen" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.queryByLabelText("camera-0000.mov")).toBeNull();
    // Choosing the segment already chosen keeps it rather than clearing the group.
    fireEvent.click(within(picture).getByRole("radio", { name: "Screen" }));
    expect(within(picture).getByRole("radio", { name: "Screen" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    const pin = screen.getByRole("button", { name: "Keep the player on top" });
    expect(pin).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(pin);
    expect(pin).toHaveAttribute("aria-pressed", "false");
    unmount();
    vi.mocked(ipc.transcriptRead).mockResolvedValue(structuredClone(TRANSCRIPT_FIXTURE));
    vi.mocked(ipc.transcriptMedia).mockResolvedValue({
      hasCamera: false,
      hasScreen: false,
      parts: [{ ...SESSION_MEDIA.parts[0], file: "memo.m4a", camera: null, audioTracks: [] }],
    });
    render(<TranscriptViewer path={path} />);
    expect(await screen.findByLabelText("memo.m4a")).toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "Picture" })).toBeNull();
    expect(screen.queryByRole("radiogroup", { name: "Sound" })).toBeNull();
    expect(partLine("memo.m4a")).toBeInTheDocument();
  });
  it("still has its files after Strict Mode hands the elements back", async () => {
    render(
      <StrictMode>
        <TranscriptViewer path={sessionPath} />
      </StrictMode>,
    );
    expect(await screen.findByLabelText("screen-0000.mov")).toHaveAttribute(
      "src",
      "keeper-file://p1/rec/screen-0000.mov",
    );
    expect(screen.getByLabelText("camera-0000.mov")).toHaveAttribute(
      "src",
      "keeper-file://p1/rec/camera-0000.mov",
    );
  });
  it("keeps its place while the reader looks at the Source", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    await lineAction("u4", "Play from here");
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Source" }));
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Transcript" }));
    expect(partLine("Part 2 of 2 · screen-0001.mov")).toBeInTheDocument();
    expect(screen.getByLabelText("screen-0001.mov")).toHaveAttribute("src");
  });
  it("lets go of every media element when the viewer closes", async () => {
    const { unmount } = render(<TranscriptViewer path={sessionPath} />);
    const main = await screen.findByLabelText("screen-0000.mov");
    const camera = screen.getByLabelText("camera-0000.mov");
    unmount();
    expect(main).not.toHaveAttribute("src");
    expect(camera).not.toHaveAttribute("src");
  });
  it("plays from a line's speaker square, and a time click only moves the player", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    await screen.findByLabelText("screen-0000.mov");
    expect(
      within(row("u4")).getByText((_, element) => element?.textContent === "00:00:21 · 6 s"),
    ).toBeInTheDocument();
    fireEvent.click(within(row("u4")).getByRole("button", { name: "Play from 00:00:21" }));
    const second = screen.getByLabelText("screen-0001.mov") as HTMLMediaElement;
    Object.defineProperty(second, "readyState", { configurable: true, get: () => 1 });
    fireEvent.loadedMetadata(second);
    expect(second.currentTime).toBeCloseTo(0.5);
    expect(play.mock.contexts).toContain(second);
    play.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Go to 00:00:28, u5" }));
    expect(second.currentTime).toBeCloseTo(7);
    expect(row("u5")).toHaveAttribute("aria-current", "true");
  });
  it("says each toggle's state in its tooltip as well as in aria-pressed", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    const follow = await screen.findByRole("button", { name: "Follow the transcript" });
    fireEvent.focus(follow);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Follow the transcript: on");
    fireEvent.click(follow);
    expect(follow).toHaveAttribute("aria-pressed", "false");
    fireEvent.blur(follow);
    fireEvent.focus(follow);
    await waitFor(() =>
      expect(screen.getByRole("tooltip")).toHaveTextContent("Follow the transcript: off"),
    );
  });
  it("takes the reader to a speaker's line nearest the player, leaving it paused", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    const first = (await screen.findByLabelText("screen-0000.mov")) as HTMLMediaElement;
    fireEvent.loadedMetadata(first);
    Object.defineProperty(first, "readyState", { configurable: true, get: () => 1 });
    first.currentTime = 16;
    fireEvent.timeUpdate(first);
    // Speaker 1 speaks 6–14 and 21.5–27: at 16 the first is two seconds away.
    fireEvent.click(
      within(await chipMenu("Speaker 1")).getByRole("menuitem", {
        name: "Go to their nearest line",
      }),
    );
    expect(first.currentTime).toBeCloseTo(6);
    expect(row("u2")).toHaveAttribute("aria-current", "true");
    expect(play).not.toHaveBeenCalled();
  });
  it("takes the reader to a speaker's next line after the player, leaving it paused", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    const first = (await screen.findByLabelText("screen-0000.mov")) as HTMLMediaElement;
    fireEvent.loadedMetadata(first);
    Object.defineProperty(first, "readyState", { configurable: true, get: () => 1 });
    first.currentTime = 16;
    fireEvent.timeUpdate(first);
    // Speaker 1's line 6–14 is the nearer; the next one starts at 21.5, in part 2.
    fireEvent.click(
      within(await chipMenu("Speaker 1")).getByRole("menuitem", {
        name: "Go to their next line",
      }),
    );
    expect(row("u4")).toHaveAttribute("aria-current", "true");
    expect(partLine("Part 2 of 2 · screen-0001.mov")).toBeInTheDocument();
    expect(play).not.toHaveBeenCalled();
  });
  it("opens at the time it was asked for, paused", async () => {
    render(<TranscriptViewer path={sessionPath} at={30} />);
    await screen.findByLabelText("screen-0001.mov");
    expect(row("u5")).toHaveAttribute("aria-current", "true");
    fireEvent.loadedMetadata(screen.getByLabelText("screen-0001.mov"));
    expect((screen.getByLabelText("screen-0001.mov") as HTMLMediaElement).currentTime).toBeCloseTo(
      9,
    );
    expect(play).not.toHaveBeenCalled();
  });
  it("never hands a part that is not on this Mac to a video", async () => {
    const media = structuredClone(SESSION_MEDIA);
    media.parts[0].here = false;
    vi.mocked(ipc.transcriptMedia).mockResolvedValue(media);
    render(<TranscriptViewer path={sessionPath} />);
    expect(await screen.findByText(PART_NOT_HERE_SENTENCE)).toBeInTheDocument();
    expect(screen.queryByLabelText("screen-0000.mov")).toBeNull();
    expect(screen.queryByLabelText("camera-0000.mov")).toBeNull();
    // The next part is here, and plays.
    await lineAction("u4", "Play from here");
    expect(screen.getByLabelText("screen-0001.mov")).toHaveAttribute(
      "src",
      "keeper-file://p1/rec/screen-0001.mov",
    );
  });
  it("takes a part's length from its file when Rust could not tell it", async () => {
    const media = structuredClone(SESSION_MEDIA);
    for (const part of media.parts) {
      part.offset = 0;
      part.duration = 0;
    }
    vi.mocked(ipc.transcriptMedia).mockResolvedValue(media);
    render(<TranscriptViewer path={sessionPath} />);
    const first = (await screen.findByLabelText("screen-0000.mov")) as HTMLMediaElement;
    Object.defineProperty(first, "duration", { configurable: true, get: () => 21 });
    fireEvent.loadedMetadata(first);
    expect(screen.getByText("0:00 / 0:21")).toBeInTheDocument();
    expect(screen.getByRole("slider", { name: SCRUB_LABEL })).toHaveAttribute("max", "21");
  });
  it("plays a session outside every synced folder from its recording", async () => {
    const media = structuredClone(SESSION_MEDIA);
    media.parts[0].screen = {
      via: "recording",
      sessionId: "01J8A-01J8B",
      relativePath: "2026-09-28 standup/screen-0000.mov",
      kind: "video",
    };
    vi.mocked(ipc.transcriptMedia).mockResolvedValue(media);
    render(<TranscriptViewer path={sessionPath} />);
    expect(await screen.findByLabelText("screen-0000.mov")).toHaveAttribute(
      "src",
      "keeper-recording://01J8A-01J8B/2026-09-28%20standup/screen-0000.mov",
    );
  });
});
describe("Search", () => {
  beforeEach(() => {
    vi.mocked(ipc.transcriptRead).mockResolvedValue(structuredClone(SESSION_TRANSCRIPT_FIXTURE));
    vi.mocked(ipc.transcriptMedia).mockResolvedValue(structuredClone(SESSION_MEDIA));
    vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  });
  it("counts matches, steps both ways round, and moves the player with Follow on", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    await screen.findByLabelText("screen-0000.mov");
    const search = screen.getByRole("searchbox", { name: "Search the transcript" });
    fireEvent.keyDown(screen.getByRole("region", { name: "Transcript viewer" }), {
      key: "f",
      metaKey: true,
    });
    expect(search).toHaveFocus();
    fireEvent.change(search, { target: { value: "KEEPER" } });
    expect(screen.getByText("2 matches")).toBeInTheDocument();
    expect([...document.querySelectorAll("mark")].map((m) => m.textContent)).toEqual([
      "keeper",
      "keeper",
    ]);
    fireEvent.keyDown(search, { key: "Enter" });
    expect(screen.getByText("1 of 2")).toBeInTheDocument();
    fireEvent.keyDown(search, { key: "Enter" });
    expect(screen.getByText("2 of 2")).toBeInTheDocument();
    // u6 is in the second part, and Follow takes the player there.
    expect(partLine("Part 2 of 2 · screen-0001.mov")).toBeInTheDocument();
    fireEvent.keyDown(search, { key: "Enter", shiftKey: true });
    expect(screen.getByText("1 of 2")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Previous match" }));
    expect(screen.getByText("2 of 2")).toBeInTheDocument();
  });
  it("finds a speaker by name, and says when nothing matches", async () => {
    render(<TranscriptViewer path={sessionPath} />);
    const search = await screen.findByRole("searchbox", { name: "Search the transcript" });
    fireEvent.change(search, { target: { value: "alex" } });
    expect(screen.getByText("3 matches")).toBeInTheDocument();
    fireEvent.change(search, { target: { value: "zebra" } });
    expect(screen.getByText("No matches")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next match" })).toBeDisabled();
  });
});
describe("Copying a clip", () => {
  const clip = { markdown: '```keeper-media\nsession = "s"\n```\n', lines: 1 };
  beforeEach(() => {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn(() => Promise.resolve()) },
      configurable: true,
    });
  });
  it("copies Rust's composition of a line's window, words included unless unticked", async () => {
    vi.mocked(ipc.transcriptClip).mockResolvedValue(clip);
    render(<TranscriptViewer path={path} />);
    await lineAction("u2", "Copy clip from here…");
    const dialog = await screen.findByRole("dialog", { name: "Copy a clip" });
    expect(within(dialog).getByRole("textbox", { name: "From" })).toHaveValue("00:00:06");
    expect(within(dialog).getByRole("textbox", { name: "To" })).toHaveValue("00:00:14");
    expect(await within(dialog).findByText("1 line")).toBeInTheDocument();
    expect(ipc.transcriptClip).toHaveBeenLastCalledWith(path, "00:00:06", "00:00:14", true);
    fireEvent.click(
      within(dialog).getByRole("checkbox", {
        name: "Include the words, for Obsidian and other apps",
      }),
    );
    await waitFor(() =>
      expect(ipc.transcriptClip).toHaveBeenLastCalledWith(path, "00:00:06", "00:00:14", false),
    );
    const copy = within(dialog).getByRole("button", { name: "Copy" });
    await waitFor(() => expect(copy).toBeEnabled());
    fireEvent.click(copy);
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(clip.markdown);
    expect(await within(dialog).findByRole("status")).toHaveTextContent(
      "Copied. Paste it into any note.",
    );
  });
  it("shows Rust's refusal of a time and keeps Copy unavailable", async () => {
    vi.mocked(ipc.transcriptClip).mockImplementation(async (_, from) => {
      if (from === "00:00:6") throw { code: "invalid", message: "From is not a time." };
      return clip;
    });
    render(<TranscriptViewer path={path} />);
    await lineAction("u2", "Copy clip from here…");
    const dialog = await screen.findByRole("dialog", { name: "Copy a clip" });
    await within(dialog).findByText("1 line");
    fireEvent.change(within(dialog).getByRole("textbox", { name: "From" }), {
      target: { value: "00:00:6" },
    });
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("From is not a time.");
    expect(within(dialog).getByRole("button", { name: "Copy" })).toBeDisabled();
  });
  it("copies the whole transcript as a note embed from the header menu", async () => {
    vi.mocked(ipc.transcriptClip).mockResolvedValue(clip);
    render(<TranscriptViewer path={path} />);
    const menu = await openMenu(await screen.findByRole("button", { name: "Transcript actions" }));
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Copy as note embed" }));
    const dialog = await screen.findByRole("dialog", { name: "Copy a clip" });
    expect(within(dialog).getByRole("textbox", { name: "From" })).toHaveValue("");
    await waitFor(() => expect(ipc.transcriptClip).toHaveBeenCalledWith(path, null, null, true));
  });
});
describe("A transcript written elsewhere", () => {
  it("reads its own transcript again, and no other", async () => {
    let written: (path: string) => void = () => {};
    vi.mocked(ipc.listenTranscriptWritten).mockImplementation(async (handler) => {
      written = handler;
      return () => {};
    });
    render(<TranscriptViewer path={path} />);
    await screen.findByRole("list", { name: "Utterances" });
    await waitFor(() => expect(ipc.listenTranscriptWritten).toHaveBeenCalled());
    const corrected = structuredClone(TRANSCRIPT_FIXTURE);
    corrected.transcript.utterances[0].text = "Corrected in another window.";
    vi.mocked(ipc.transcriptRead).mockResolvedValue(corrected);
    act(() => written("/Volumes/merope/tgdrive/other.transcript.json"));
    expect(ipc.transcriptRead).toHaveBeenCalledTimes(1);
    act(() => written(path));
    expect(await within(row("u1")).findByText("Corrected in another window.")).toBeVisible();
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
describe("In a note", () => {
  beforeEach(() => {
    vi.mocked(ipc.transcriptRead).mockResolvedValue(structuredClone(SESSION_TRANSCRIPT_FIXTURE));
    vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  });
  it("shows only its window's lines and speakers, plays the block's media, and adds the block's actions", async () => {
    const scroller = document.createElement("div");
    const onRemove = vi.fn();
    render(
      <TranscriptViewer
        path={sessionPath}
        scroller={scroller}
        window={{ from: 20, to: 30 }}
        title={null}
        media={structuredClone(SESSION_MEDIA)}
        menuItems={<DropdownMenuItem onSelect={onRemove}>Remove widget</DropdownMenuItem>}
      />,
    );
    await screen.findByLabelText("screen-0000.mov");
    expect(ipc.transcriptMedia).not.toHaveBeenCalled();
    // No dialog chrome: no heading, no Source tab.
    expect(screen.queryByRole("heading")).toBeNull();
    expect(screen.queryByRole("tab")).toBeNull();
    const ids = within(screen.getByRole("list", { name: "Utterances" }))
      .getAllByRole("listitem")
      .map((item) =>
        item.querySelector("[aria-label^='Line actions']")?.getAttribute("aria-label"),
      );
    // [20, 30) overlaps u3 (15–24), u4 (21.5–27) and u5 (28–33), and nothing else.
    expect(ids).toEqual(["Line actions u3", "Line actions u4", "Line actions u5"]);
    expect(screen.getByRole("button", { name: /^Alex,/ })).toHaveAccessibleName(/2 lines$/);
    expect(screen.getByRole("button", { name: /^Speaker 1,/ })).toHaveAccessibleName(/1 line$/);
    // The player's scrub spans the window only.
    const scrub = screen.getByRole("slider", { name: SCRUB_LABEL });
    expect(scrub).toHaveAttribute("min", "20");
    expect(scrub).toHaveAttribute("max", "30");
    const menu = await openMenu(screen.getByRole("button", { name: "Transcript actions" }));
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Remove widget" }));
    expect(onRemove).toHaveBeenCalled();
  });
  it("windows a long meeting against the note's scroller, and a scroll of the note stops following", async () => {
    const long = structuredClone(SESSION_TRANSCRIPT_FIXTURE);
    const template = long.transcript.utterances[0];
    long.transcript.utterances = Array.from({ length: 300 }, (_, index) => ({
      ...template,
      id: `l${index}`,
      start: index * 0.1,
      end: index * 0.1 + 0.1,
    }));
    vi.mocked(ipc.transcriptRead).mockResolvedValue(long);
    const geometry = withListGeometry({ viewport: 600, row: 90 });
    try {
      const scroller = document.createElement("div");
      // The test geometry answers the viewport's height by this mark.
      scroller.setAttribute(WINDOW_VIEWPORT_ATTR, "");
      document.body.append(scroller);
      render(
        <TranscriptViewer
          path={sessionPath}
          scroller={scroller}
          media={structuredClone(SESSION_MEDIA)}
        />,
        { container: scroller.appendChild(document.createElement("div")) },
      );
      const list = await screen.findByRole("list", { name: "Utterances" });
      // The lines start 200px down the note, wherever it is scrolled.
      Object.defineProperty(list, "getBoundingClientRect", {
        configurable: true,
        value: () => ({ top: 200 - scroller.scrollTop }) as DOMRect,
      });
      geometry.scrollTo(scroller, 0);
      const mounted = () => scroller.querySelectorAll(`[${WINDOW_ROW_ATTR}]`).length;
      expect(mounted()).toBeLessThan(30);
      geometry.scrollTo(scroller, 200 + 150 * 90);
      await waitFor(() => expect(row("l150")).toBeInTheDocument());
      expect(mounted()).toBeLessThan(30);
      // A seek follows: the note is scrolled to the line being said…
      geometry.scrollTo(scroller, 0);
      fireEvent.input(screen.getByRole("slider", { name: SCRUB_LABEL }), {
        target: { value: "20" },
      });
      await waitFor(() => expect(scroller.scrollTop).toBeGreaterThan(200 + 150 * 90));
      // …until the reader scrolls the note themself.
      geometry.scrollTo(scroller, 0);
      fireEvent.wheel(scroller);
      const main = screen.getByLabelText("screen-0000.mov") as HTMLMediaElement;
      main.currentTime = 25;
      fireEvent.timeUpdate(main);
      expect(scroller.scrollTop).toBe(0);
      // The line being said is there when the reader scrolls to it.
      geometry.scrollTo(scroller, 200 + 250 * 90 - 300);
      await waitFor(() => expect(row("l250")).toHaveAttribute("aria-current", "true"));
      scroller.remove();
    } finally {
      geometry.undo();
    }
  });
  it("holds a seek asked before the player is awake and lands it when it wakes", async () => {
    const viewer = createRef<TranscriptViewerHandle>();
    const view = render(
      <TranscriptViewer
        ref={viewer}
        path={sessionPath}
        scroller={null}
        media={structuredClone(SESSION_MEDIA)}
        playerAwake={false}
        asleepText="The player starts when this block is on screen."
      />,
    );
    expect(
      await screen.findByText("The player starts when this block is on screen."),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("screen-0000.mov")).toBeNull();
    act(() => viewer.current?.seek(30));
    view.rerender(
      <TranscriptViewer
        ref={viewer}
        path={sessionPath}
        scroller={null}
        media={structuredClone(SESSION_MEDIA)}
      />,
    );
    const second = (await screen.findByLabelText("screen-0001.mov")) as HTMLMediaElement;
    fireEvent.loadedMetadata(second);
    expect(second.currentTime).toBeCloseTo(9);
    expect(row("u5")).toHaveAttribute("aria-current", "true");
  });
});
