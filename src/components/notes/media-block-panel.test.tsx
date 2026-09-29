/**
 * The media block's panel: what it shows in each state, what it asks Rust,
 * and what it hands back to the editor.
 *
 * The player and the lines are the viewer's components with tests of their
 * own; here they are stand-ins that record what the panel hands them and let a
 * test say "the player is at 13 s" or "the player started".
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { forwardRef, useImperativeHandle } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcClient from "@/lib/ipc/client";
import type { MediaBlockVm } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { transcriptionStore } from "@/lib/stores/transcription";
import type { MediaBlockMountArgs } from "./editor/media-block";
import {
  MARK_MOMENT_LABEL,
  MARKER_REMOVE,
  MEDIA_BLOCK_MENU_LABEL,
  MediaBlockPanel,
  NOT_TRANSCRIBED_SENTENCE,
  TRANSCRIBE_LABEL,
} from "./media-block-panel";

const resolveBlock = vi.fn<typeof IpcClient.mediaBlockResolve>();
const editBlock = vi.fn<typeof IpcClient.mediaBlockEdit>();
let written: ((path: string) => void) | null = null;

vi.mock("@/lib/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof IpcClient>()),
  mediaBlockResolve: (profileId: string, source: string) => resolveBlock(profileId, source),
  mediaBlockEdit: (source: string, edit: IpcClient.MarkerEditReq) => editBlock(source, edit),
  listenTranscriptWritten: async (on: (path: string) => void) => {
    written = on;
    return () => {
      written = null;
    };
  },
}));

const player = { seek: vi.fn(), pause: vi.fn() };
let playerProps: Record<string, unknown> | null = null;
vi.mock("@/components/transcription/transcript-player", () => ({
  TranscriptPlayer: forwardRef((props: Record<string, unknown>, ref) => {
    playerProps = props;
    useImperativeHandle(ref, () => player);
    return <div data-testid="player" />;
  }),
}));
vi.mock("@/components/transcription/transcript-lines", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  TranscriptLinesBox: (props: { lines: { id: string; text: string }[]; current: number }) => (
    <ol aria-label="Lines">
      {props.lines.map((line, index) => (
        <li key={line.id} aria-current={index === props.current ? "true" : undefined}>
          {line.text}
        </li>
      ))}
    </ol>
  ),
}));
vi.mock("@/components/transcription/transcript-viewer", () => ({
  TranscriptDialog: ({ path, at }: { path: string | null; at?: number }) =>
    path === null ? null : <div role="dialog">{`${path} at ${at}`}</div>,
}));

const TRANSCRIPT = "/Volumes/d/recordings/kelly/transcript.json";

function vm(over: Partial<MediaBlockVm> = {}): MediaBlockVm {
  return {
    title: "Kelly sync",
    sessionId: "S1",
    transcriptPath: TRANSCRIPT,
    transcribed: true,
    transcribePath: null,
    duration: 60,
    window: { from: 0, to: null },
    picture: null,
    sound: null,
    media: { parts: [], hasCamera: false, hasScreen: true },
    lines: [
      { id: "u1", speaker: "ME", start: 0, end: 5, text: "Hello there, Kelly." },
      { id: "u2", speaker: "S1", start: 12, end: 18, text: "The price is 40 a seat." },
    ],
    speakers: [
      { id: "ME", name: "Alex", origin: "microphone" },
      { id: "S1", name: "Kelly", origin: "system" },
    ],
    markers: [],
    ...over,
  };
}

function args(over: Partial<MediaBlockMountArgs> = {}): MediaBlockMountArgs {
  return {
    profileId: "drive",
    source: 'session = "S1"',
    editable: true,
    replaceSource: vi.fn(() => true),
    noteLink: "Kelly sync",
    register: vi.fn(() => () => {}),
    claimPlayback: vi.fn(() => () => {}),
    ...over,
  };
}

beforeEach(() => {
  resolveBlock.mockReset();
  editBlock.mockReset();
  player.seek.mockReset();
  player.pause.mockReset();
  playerProps = null;
  Object.assign(navigator, { clipboard: { writeText: vi.fn(() => Promise.resolve()) } });
});

afterEach(() => {
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
  transcriptionStore.setState({ jobs: {} });
});

describe("what the block shows", () => {
  it("shows Rust's refusal above the block's own text, never an empty box", async () => {
    resolveBlock.mockRejectedValue({ code: "notesInvalid", message: "Unknown key form." });

    render(<MediaBlockPanel {...args({ source: 'form = "00:12:00"' })} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("Unknown key form.");
    expect(screen.getByText('form = "00:12:00"')).toBeInTheDocument();
  });

  it("draws the player and the lines of what it resolved", async () => {
    resolveBlock.mockResolvedValue(vm());

    render(<MediaBlockPanel {...args()} />);

    expect(await screen.findByRole("heading", { name: "Kelly sync" })).toBeInTheDocument();
    expect(resolveBlock).toHaveBeenCalledWith("drive", 'session = "S1"');
    expect(await screen.findByTestId("player")).toBeInTheDocument();
    expect(
      within(screen.getByRole("list", { name: "Lines" })).getAllByRole("listitem"),
    ).toHaveLength(2);
  });

  it("spans only its window, and says which stretch of the meeting that is", async () => {
    resolveBlock.mockResolvedValue(vm({ window: { from: 10, to: 20 } }));

    render(<MediaBlockPanel {...args()} />);

    expect(await screen.findByText("0:10–0:20 of 1:00")).toBeInTheDocument();
    expect(playerProps?.window).toEqual({ from: 10, to: 20 });
  });

  it("highlights the line being said as the player moves", async () => {
    resolveBlock.mockResolvedValue(vm());
    render(<MediaBlockPanel {...args()} />);
    await screen.findByTestId("player");

    act(() => (playerProps?.onTime as (s: number) => void)(13));

    expect(screen.getByText("The price is 40 a seat.")).toHaveAttribute("aria-current", "true");
  });
});

describe("before the transcript exists", () => {
  it("plays and says it is not transcribed, offering Transcribe where it can run", async () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, transcription: true });
    resolveBlock.mockResolvedValue(
      vm({ transcribed: false, lines: [], transcribePath: "/Volumes/d/recordings/kelly" }),
    );

    render(<MediaBlockPanel {...args()} />);

    expect(await screen.findByText(NOT_TRANSCRIBED_SENTENCE)).toBeInTheDocument();
    expect(await screen.findByTestId("player")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: TRANSCRIBE_LABEL })).toBeInTheDocument();
  });

  it.each([
    ["transcription cannot run here", false, "/Volumes/d/recordings/kelly"],
    ["the session is not transcribable", true, null],
  ])("offers no Transcribe where %s", async (_, capable, transcribePath) => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, transcription: capable });
    resolveBlock.mockResolvedValue(vm({ transcribed: false, lines: [], transcribePath }));

    render(<MediaBlockPanel {...args()} />);

    await screen.findByText(NOT_TRANSCRIBED_SENTENCE);
    expect(screen.queryByRole("button", { name: TRANSCRIBE_LABEL })).toBeNull();
  });

  it("reads the block again when its transcript is written, and not for another", async () => {
    resolveBlock.mockResolvedValue(vm({ transcribed: false, lines: [] }));
    render(<MediaBlockPanel {...args()} />);
    await screen.findByText(NOT_TRANSCRIBED_SENTENCE);
    await waitFor(() => expect(written).not.toBeNull());

    act(() => written?.("/Volumes/d/elsewhere/transcript.json"));
    expect(resolveBlock).toHaveBeenCalledTimes(1);

    resolveBlock.mockResolvedValue(vm());
    act(() => written?.(TRANSCRIPT));

    expect(await screen.findByText("Hello there, Kelly.")).toBeInTheDocument();
    expect(resolveBlock).toHaveBeenCalledTimes(2);
  });
});

describe("markers", () => {
  const MARKED = vm({
    markers: [
      { name: "Deal", from: 12, to: null },
      { name: "Demo", from: 30, to: 40 },
    ],
  });

  it("seeks to a moment, keeping the player as it was", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    render(<MediaBlockPanel {...args()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Go to Deal, 0:12" }));

    expect(player.seek).toHaveBeenCalledWith(12, undefined);
  });

  it("plays a window from its start and pauses at its end", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    render(<MediaBlockPanel {...args()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Play Demo, 0:30–0:40" }));
    expect(player.seek).toHaveBeenCalledWith(30, true);

    act(() => (playerProps?.onTime as (s: number) => void)(39));
    expect(player.pause).not.toHaveBeenCalled();
    act(() => (playerProps?.onTime as (s: number) => void)(40));
    expect(player.pause).toHaveBeenCalledTimes(1);
  });

  it("marks the moment the player is at, with Rust's body spliced into the note", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    editBlock.mockResolvedValue("NEW BODY");
    const mounted = args();
    render(<MediaBlockPanel {...mounted} />);
    await screen.findByTestId("player");
    act(() => (playerProps?.onTime as (s: number) => void)(13.8));

    fireEvent.pointerDown(screen.getByRole("button", { name: MEDIA_BLOCK_MENU_LABEL }), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(await screen.findByRole("menuitem", { name: MARK_MOMENT_LABEL }));
    const name = await screen.findByRole("textbox", { name: "Name" });
    // Prefilled from the line being said, cleaned of what a name cannot carry.
    expect(name).toHaveValue("The price is 40 a seat.");
    fireEvent.click(screen.getByRole("button", { name: "Mark" }));

    await waitFor(() =>
      expect(editBlock).toHaveBeenCalledWith('session = "S1"', {
        op: "add",
        name: "The price is 40 a seat.",
        from: 13,
        to: null,
      }),
    );
    expect(mounted.replaceSource).toHaveBeenCalledWith("NEW BODY");
  });

  it("shows Rust's refusal in the form and writes nothing", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    editBlock.mockRejectedValue({ code: "notesInvalid", message: "This block already has Deal." });
    const mounted = args();
    render(<MediaBlockPanel {...mounted} />);
    await screen.findByTestId("player");

    fireEvent.pointerDown(screen.getByRole("button", { name: MEDIA_BLOCK_MENU_LABEL }), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(await screen.findByRole("menuitem", { name: MARK_MOMENT_LABEL }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Name" }), {
      target: { value: "deal" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Mark" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("This block already has Deal.");
    expect(mounted.replaceSource).not.toHaveBeenCalled();
  });

  it("removes a marker at once — the splice is undoable", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    editBlock.mockResolvedValue('session = "S1"');
    const mounted = args();
    render(<MediaBlockPanel {...mounted} />);

    fireEvent.pointerDown(await screen.findByRole("button", { name: "Marker actions Deal" }), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(await screen.findByRole("menuitem", { name: MARKER_REMOVE }));

    await waitFor(() => expect(mounted.replaceSource).toHaveBeenCalledWith('session = "S1"'));
    expect(editBlock).toHaveBeenCalledWith('session = "S1"', { op: "remove", name: "Deal" });
  });

  it("offers no marker edits where the note cannot be written", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    render(<MediaBlockPanel {...args({ editable: false })} />);

    fireEvent.pointerDown(await screen.findByRole("button", { name: "Marker actions Deal" }), {
      button: 0,
      ctrlKey: false,
    });

    expect(await screen.findByRole("menuitem", { name: "Copy link" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: MARKER_REMOVE })).toBeNull();
  });

  it("copies a link that names this note and the marker", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    render(<MediaBlockPanel {...args()} />);

    fireEvent.pointerDown(await screen.findByRole("button", { name: "Marker actions Deal" }), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Copy link" }));

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("[[Kelly sync#Deal]]");
  });
});

describe("one player per pane", () => {
  it("claims playback when it starts, and gives the claim back when it stops", async () => {
    resolveBlock.mockResolvedValue(vm());
    const release = vi.fn();
    const mounted = args({ claimPlayback: vi.fn(() => release) });
    render(<MediaBlockPanel {...mounted} />);
    await screen.findByTestId("player");

    act(() => (playerProps?.onPlayingChange as (p: boolean) => void)(true));
    expect(mounted.claimPlayback).toHaveBeenCalledTimes(1);
    // The pause the layer calls when another block starts is this player's.
    (vi.mocked(mounted.claimPlayback).mock.calls[0][0] as () => void)();
    expect(player.pause).toHaveBeenCalled();

    act(() => (playerProps?.onPlayingChange as (p: boolean) => void)(false));
    expect(release).toHaveBeenCalled();
  });

  it("takes a marker link's seek through the handle it registers", async () => {
    resolveBlock.mockResolvedValue(vm());
    const mounted = args();
    render(<MediaBlockPanel {...mounted} />);
    await screen.findByTestId("player");

    const calls = vi.mocked(mounted.register).mock.calls;
    const handle = calls[calls.length - 1]?.[0];
    act(() => handle?.seekTo(12));

    expect(player.seek).toHaveBeenCalledWith(12, false);
  });
});

describe("near the screen", () => {
  let observed: ((entries: { isIntersecting: boolean }[]) => void) | null = null;

  beforeEach(() => {
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(callback: (entries: { isIntersecting: boolean }[]) => void) {
          observed = callback;
        }
        observe() {}
        disconnect() {}
      },
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    observed = null;
  });

  it("mounts the player only when the block comes near, and lets it go when it leaves", async () => {
    resolveBlock.mockResolvedValue(vm());
    render(<MediaBlockPanel {...args()} />);
    await screen.findByRole("heading", { name: "Kelly sync" });

    expect(screen.queryByTestId("player")).toBeNull();
    act(() => observed?.([{ isIntersecting: true }]));
    expect(screen.getByTestId("player")).toBeInTheDocument();
    act(() => observed?.([{ isIntersecting: false }]));
    expect(screen.queryByTestId("player")).toBeNull();
  });

  it("keeps a playing block's player when it scrolls away", async () => {
    resolveBlock.mockResolvedValue(vm());
    render(<MediaBlockPanel {...args()} />);
    await screen.findByRole("heading", { name: "Kelly sync" });
    act(() => observed?.([{ isIntersecting: true }]));
    act(() => (playerProps?.onPlayingChange as (p: boolean) => void)(true));

    act(() => observed?.([{ isIntersecting: false }]));

    expect(screen.getByTestId("player")).toBeInTheDocument();
  });
});
