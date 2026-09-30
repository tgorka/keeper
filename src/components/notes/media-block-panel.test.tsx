/**
 * The media block's panel: what it shows in each state, what it asks Rust,
 * and what it hands back to the editor.
 *
 * The transcript viewer and the player have tests of their own; here they are
 * stand-ins that record what the panel hands them and let a test say "the
 * player is at 13 s" or "the player started". The viewer's stand-in draws the
 * two slots the block fills — its markers and its ⋯ items — because those are
 * the block's.
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { forwardRef, type ReactNode, useImperativeHandle } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type * as IpcClient from "@/lib/ipc/client";
import type { MediaBlockVm } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { transcriptionStore } from "@/lib/stores/transcription";
import type { MediaBlockMountArgs } from "./editor/media-block";
import {
  EDIT_BLOCK_SOURCE_LABEL,
  MARK_MOMENT_LABEL,
  MARKER_REMOVE,
  MEDIA_BLOCK_MENU_LABEL,
  MediaBlockPanel,
  NOT_TRANSCRIBED_SENTENCE,
  REMOVE_WIDGET_LABEL,
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

/** The untranscribed block's own player. */
const player = { seek: vi.fn(), pause: vi.fn() };
let playerProps: Record<string, unknown> | null = null;
vi.mock("@/components/transcription/transcript-player", () => ({
  TranscriptPlayer: forwardRef((props: Record<string, unknown>, ref) => {
    playerProps = props;
    useImperativeHandle(ref, () => player);
    return <div data-testid="player" />;
  }),
}));

/** The embedded viewer, whose handle is the transcribed block's player. */
const viewer = { seek: vi.fn(), pause: vi.fn() };
let viewerProps: Record<string, unknown> | null = null;
vi.mock("@/components/transcription/transcript-viewer", () => ({
  TranscriptDialog: ({ path, at }: { path: string | null; at?: number }) =>
    path === null ? null : <div role="dialog">{`${path} at ${at}`}</div>,
  TranscriptViewer: forwardRef((props: Record<string, unknown>, ref) => {
    viewerProps = props;
    useImperativeHandle(ref, () => viewer);
    return (
      <div data-testid="viewer">
        {props.markers as ReactNode}
        <DropdownMenu>
          <DropdownMenuTrigger aria-label="Transcript actions">⋯</DropdownMenuTrigger>
          <DropdownMenuContent>{props.menuItems as ReactNode}</DropdownMenuContent>
        </DropdownMenu>
      </div>
    );
  }),
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

const SCROLLER = document.createElement("div");

function args(over: Partial<MediaBlockMountArgs> = {}): MediaBlockMountArgs {
  return {
    profileId: "drive",
    source: 'session = "S1"',
    editable: true,
    replaceSource: vi.fn(() => true),
    noteLink: "Kelly sync",
    register: vi.fn(() => () => {}),
    claimPlayback: vi.fn(() => () => {}),
    scroller: SCROLLER,
    editSource: vi.fn(),
    remove: vi.fn(),
    ...over,
  };
}

/** Radix opens a menu on a primary pointer-down, not on a click. */
async function openMenu(name: string): Promise<void> {
  fireEvent.pointerDown(await screen.findByRole("button", { name }), { button: 0, ctrlKey: false });
}

beforeEach(() => {
  resolveBlock.mockReset();
  editBlock.mockReset();
  for (const each of [player, viewer]) {
    each.seek.mockReset();
    each.pause.mockReset();
  }
  playerProps = null;
  viewerProps = null;
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

  it("is the transcript viewer, embedded in the note's scroller, over the block's window", async () => {
    resolveBlock.mockResolvedValue(vm({ window: { from: 10, to: 20 } }));

    render(<MediaBlockPanel {...args()} />);

    await screen.findByTestId("viewer");
    expect(resolveBlock).toHaveBeenCalledWith("drive", 'session = "S1"');
    expect(viewerProps).toMatchObject({
      path: TRANSCRIPT,
      profileId: "drive",
      // Given, so the viewer grows in the note and has no scroll box of its own.
      scroller: SCROLLER,
      window: { from: 10, to: 20 },
      title: "Kelly sync",
    });
    // The read-only list of lines is gone: the viewer is the only one.
    expect(screen.queryByTestId("player")).toBeNull();
  });

  it("says no title where the block names none, rather than the files it plays", async () => {
    resolveBlock.mockResolvedValue(vm({ title: null }));

    render(<MediaBlockPanel {...args()} />);

    await screen.findByTestId("viewer");
    expect(viewerProps?.title).toBeNull();
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

    expect(await screen.findByTestId("viewer")).toBeInTheDocument();
    expect(resolveBlock).toHaveBeenCalledTimes(2);
  });

  it("plays only its window before there are lines to bound it", async () => {
    resolveBlock.mockResolvedValue(
      vm({ transcribed: false, lines: [], window: { from: 10, to: 20 } }),
    );

    render(<MediaBlockPanel {...args()} />);

    await screen.findByTestId("player");
    expect(playerProps?.window).toEqual({ from: 10, to: 20 });
  });
});

describe("the block's own verbs, from the menu and nowhere else", () => {
  it("offers Edit block source and Remove widget under the viewer's ⋯", async () => {
    resolveBlock.mockResolvedValue(vm());
    const mounted = args();
    render(<MediaBlockPanel {...mounted} />);

    await openMenu("Transcript actions");
    fireEvent.click(await screen.findByRole("menuitem", { name: EDIT_BLOCK_SOURCE_LABEL }));
    expect(mounted.editSource).toHaveBeenCalledTimes(1);

    await openMenu("Transcript actions");
    fireEvent.click(await screen.findByRole("menuitem", { name: REMOVE_WIDGET_LABEL }));
    expect(mounted.remove).toHaveBeenCalledTimes(1);
  });

  it("offers them on a block that is not transcribed yet, which has a menu of its own", async () => {
    resolveBlock.mockResolvedValue(vm({ transcribed: false, lines: [] }));
    const mounted = args();
    render(<MediaBlockPanel {...mounted} />);

    await openMenu(MEDIA_BLOCK_MENU_LABEL);
    fireEvent.click(await screen.findByRole("menuitem", { name: REMOVE_WIDGET_LABEL }));

    expect(mounted.remove).toHaveBeenCalledTimes(1);
  });

  it("offers them beside Rust's refusal, where the source is the thing to fix", async () => {
    resolveBlock.mockRejectedValue({ code: "notesInvalid", message: "Unknown key form." });
    const mounted = args();
    render(<MediaBlockPanel {...mounted} />);

    await openMenu(MEDIA_BLOCK_MENU_LABEL);
    fireEvent.click(await screen.findByRole("menuitem", { name: EDIT_BLOCK_SOURCE_LABEL }));

    expect(mounted.editSource).toHaveBeenCalledTimes(1);
  });

  it("offers neither where the note cannot be written", async () => {
    resolveBlock.mockResolvedValue(vm());
    render(<MediaBlockPanel {...args({ editable: false })} />);

    await openMenu("Transcript actions");

    await screen.findByRole("menu");
    expect(screen.queryByRole("menuitem", { name: EDIT_BLOCK_SOURCE_LABEL })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: REMOVE_WIDGET_LABEL })).toBeNull();
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

    expect(viewer.seek).toHaveBeenCalledWith(12, undefined);
  });

  it("plays a window from its start and pauses at its end", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    render(<MediaBlockPanel {...args()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Play Demo, 0:30–0:40" }));
    expect(viewer.seek).toHaveBeenCalledWith(30, true);

    act(() => (viewerProps?.onTime as (s: number) => void)(39));
    expect(viewer.pause).not.toHaveBeenCalled();
    act(() => (viewerProps?.onTime as (s: number) => void)(40));
    expect(viewer.pause).toHaveBeenCalledTimes(1);
  });

  it("marks the moment the player is at, with Rust's body spliced into the note", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    editBlock.mockResolvedValue("NEW BODY");
    const mounted = args();
    render(<MediaBlockPanel {...mounted} />);
    await screen.findByTestId("viewer");
    act(() => (viewerProps?.onTime as (s: number) => void)(13.8));

    await openMenu("Transcript actions");
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
    await screen.findByTestId("viewer");

    await openMenu("Transcript actions");
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

    await openMenu("Marker actions Deal");
    fireEvent.click(await screen.findByRole("menuitem", { name: MARKER_REMOVE }));

    await waitFor(() => expect(mounted.replaceSource).toHaveBeenCalledWith('session = "S1"'));
    expect(editBlock).toHaveBeenCalledWith('session = "S1"', { op: "remove", name: "Deal" });
  });

  it("offers no marker edits where the note cannot be written", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    render(<MediaBlockPanel {...args({ editable: false })} />);

    await openMenu("Marker actions Deal");

    expect(await screen.findByRole("menuitem", { name: "Copy link" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: MARKER_REMOVE })).toBeNull();
  });

  it("copies a link that names this note and the marker", async () => {
    resolveBlock.mockResolvedValue(MARKED);
    render(<MediaBlockPanel {...args()} />);

    await openMenu("Marker actions Deal");
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
    await screen.findByTestId("viewer");

    act(() => (viewerProps?.onPlayingChange as (p: boolean) => void)(true));
    expect(mounted.claimPlayback).toHaveBeenCalledTimes(1);
    // The pause the layer calls when another block starts is this player's.
    (vi.mocked(mounted.claimPlayback).mock.calls[0][0] as () => void)();
    expect(viewer.pause).toHaveBeenCalled();

    act(() => (viewerProps?.onPlayingChange as (p: boolean) => void)(false));
    expect(release).toHaveBeenCalled();
  });

  it("takes a marker link's seek through the handle it registers", async () => {
    resolveBlock.mockResolvedValue(vm());
    const mounted = args();
    render(<MediaBlockPanel {...mounted} />);
    await screen.findByTestId("viewer");

    const calls = vi.mocked(mounted.register).mock.calls;
    const handle = calls[calls.length - 1]?.[0];
    act(() => handle?.seekTo(12));

    expect(viewer.seek).toHaveBeenCalledWith(12, false);
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

  it("wakes the player only when the block comes near, and lets it go when it leaves", async () => {
    resolveBlock.mockResolvedValue(vm());
    render(<MediaBlockPanel {...args()} />);
    await screen.findByTestId("viewer");

    expect(viewerProps?.playerAwake).toBe(false);
    act(() => observed?.([{ isIntersecting: true }]));
    expect(viewerProps?.playerAwake).toBe(true);
    act(() => observed?.([{ isIntersecting: false }]));
    expect(viewerProps?.playerAwake).toBe(false);
  });

  it("keeps a playing block's player when it scrolls away", async () => {
    resolveBlock.mockResolvedValue(vm());
    render(<MediaBlockPanel {...args()} />);
    await screen.findByTestId("viewer");
    act(() => observed?.([{ isIntersecting: true }]));
    act(() => (viewerProps?.onPlayingChange as (p: boolean) => void)(true));

    act(() => observed?.([{ isIntersecting: false }]));

    expect(viewerProps?.playerAwake).toBe(true);
  });
});
