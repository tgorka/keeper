/**
 * A `record = "new"` block's panel: what it offers in Preview, while a
 * session records, and what its Start does — link the session to this note,
 * then name the session in this block's own fence and write the note.
 *
 * The session and permission hooks are stand-ins a test sets; the setup
 * cards have tests of their own and are drawn as placeholders here.
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { START_RECORDING_LABEL } from "@/components/layout/recording-pane";
import type * as SessionHook from "@/hooks/use-recording-session";
import { IDLE_RECORDING_STATUS } from "@/hooks/use-recording-session";
import type * as IpcClient from "@/lib/ipc/client";
import type { RecordingStatusVm } from "@/lib/ipc/client";
import {
  MediaRecorderPanel,
  type MediaRecorderPanelProps,
  NOT_RECORDED_SENTENCE,
  RECORDING_IN_LABEL,
  RECORDING_UNNAMED_SENTENCE,
} from "./media-recorder-panel";

const ID = "01KYDKP6SN2HR4SJBJ9JTBVC2Z-01KYDM0000000000000000000A";
const linkedNote = vi.fn<typeof IpcClient.recordingLinkedNote>();
const recordStarted = vi.fn<typeof IpcClient.mediaBlockRecordStarted>();
vi.mock("@/lib/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof IpcClient>()),
  recordingLinkedNote: () => linkedNote(),
  mediaBlockRecordStarted: (source: string, sessionId: string) => recordStarted(source, sessionId),
}));

vi.mock("@/lib/stores/recording-settings", () => ({
  ensureRecordingSettingsHydrated: async () => {},
  useRecordingSettings: () => null,
}));

const start = vi.fn(async (..._args: unknown[]) => true);
let status: RecordingStatusVm = IDLE_RECORDING_STATUS;
vi.mock("@/hooks/use-recording-session", async (importOriginal) => ({
  ...(await importOriginal<typeof SessionHook>()),
  useRecordingSession: () => ({
    status,
    sessionFolders: [],
    elapsed: "0:05",
    start,
    stop: vi.fn(async () => {}),
    acknowledge: vi.fn(async () => {}),
    adoptRetitled: vi.fn(),
  }),
}));

vi.mock("@/hooks/use-recording-permission", () => ({
  useRecordingPermission: () => ({
    permission: {
      screenRecording: "granted",
      microphone: "granted",
      camera: "granted",
      canStart: true,
    },
    refresh: vi.fn(),
  }),
}));

vi.mock("@/components/recording/recording-source-picker", () => ({
  RecordingSourcePicker: () => <div data-testid="source" />,
}));
vi.mock("@/components/recording/recording-audio-controls", () => ({
  RecordingAudioControls: () => <div data-testid="audio" />,
}));
vi.mock("@/components/recording/recording-webcam-controls", () => ({
  RecordingWebcamControls: () => <div data-testid="webcam" />,
}));

const replaceSource = vi.fn((_next: string) => true);
const saveNote = vi.fn();

function props(over: Partial<MediaRecorderPanelProps> = {}): MediaRecorderPanelProps {
  return {
    profileId: "01VAULT",
    source: 'record = "new"\n',
    notePath: () => "meetings/standup.md",
    noteLink: "standup",
    preview: false,
    replaceSource,
    saveNote,
    ...over,
  };
}

const LIVE: RecordingStatusVm = {
  ...IDLE_RECORDING_STATUS,
  state: "recording",
  startedAtEpochMs: 1,
};

describe("MediaRecorderPanel", () => {
  beforeEach(() => {
    status = IDLE_RECORDING_STATUS;
    start.mockClear();
    start.mockResolvedValue(true);
    replaceSource.mockClear();
    saveNote.mockClear();
    recordStarted.mockReset();
    recordStarted.mockResolvedValue(`session = "${ID}"\n`);
    linkedNote.mockReset();
    linkedNote.mockResolvedValue(null);
  });

  it("in Preview says only that nothing is recorded yet", () => {
    render(<MediaRecorderPanel {...props({ preview: true })} />);
    expect(screen.getByText(NOT_RECORDED_SENTENCE)).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
    expect(linkedNote).not.toHaveBeenCalled();
  });

  it("names the session in its own fence once Start answers, and writes the note", async () => {
    render(<MediaRecorderPanel {...props()} />);
    linkedNote.mockResolvedValue({
      profileId: "01VAULT",
      path: "meetings/standup.md",
      sessionId: ID,
      title: "Standup",
    });
    fireEvent.click(await screen.findByRole("button", { name: START_RECORDING_LABEL }));

    await waitFor(() => expect(saveNote).toHaveBeenCalledTimes(1));
    expect(start.mock.calls[0]?.slice(6)).toEqual([
      { title: "standup" },
      { profileId: "01VAULT", path: "meetings/standup.md" },
    ]);
    expect(recordStarted).toHaveBeenCalledWith('record = "new"\n', ID);
    expect(replaceSource).toHaveBeenCalledWith(`session = "${ID}"\n`);
    expect(replaceSource.mock.invocationCallOrder[0]).toBeLessThan(
      saveNote.mock.invocationCallOrder[0] ?? 0,
    );
  });

  it("names nothing when the start was refused", async () => {
    start.mockResolvedValue(false);
    linkedNote.mockResolvedValue({
      profileId: "01VAULT",
      path: "meetings/standup.md",
      sessionId: "SOMEBODY-ELSES",
      title: "Standup",
    });
    render(<MediaRecorderPanel {...props()} />);
    fireEvent.click(await screen.findByRole("button", { name: START_RECORDING_LABEL }));

    await waitFor(() => expect(start).toHaveBeenCalledTimes(1));
    // Every step `begin` could take after the start, settled.
    await act(async () => {
      for (let step = 0; step < 10; step += 1) await Promise.resolve();
    });
    expect(replaceSource).not.toHaveBeenCalled();
    expect(saveNote).not.toHaveBeenCalled();
  });

  it("says it could not take its name when its block moved before the name could land", async () => {
    replaceSource.mockReturnValueOnce(false);
    start.mockImplementationOnce(async () => {
      status = LIVE;
      return true;
    });
    linkedNote.mockResolvedValue({
      profileId: "01VAULT",
      path: "meetings/standup.md",
      sessionId: ID,
      title: "Standup",
    });
    render(<MediaRecorderPanel {...props()} />);
    fireEvent.click(await screen.findByRole("button", { name: START_RECORDING_LABEL }));

    expect(await screen.findByText(RECORDING_UNNAMED_SENTENCE)).toBeInTheDocument();
    expect(replaceSource).toHaveBeenCalledTimes(1);
    expect(saveNote).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Standup" })).toBeNull();
  });

  it("offers no Start while a session records, and names the note by its title", async () => {
    status = LIVE;
    linkedNote.mockResolvedValue({
      profileId: "01VAULT",
      path: "meetings/retro.md",
      sessionId: ID,
      title: "Sprint retro",
    });
    render(<MediaRecorderPanel {...props()} />);
    expect(await screen.findByRole("button", { name: "Sprint retro" })).toBeInTheDocument();
    expect(screen.getByText(RECORDING_IN_LABEL, { exact: false })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: START_RECORDING_LABEL })).toBeNull();
  });

  it("is never the live block, even in the note that records", async () => {
    status = LIVE;
    linkedNote.mockResolvedValue({
      profileId: "01VAULT",
      path: "meetings/standup.md",
      sessionId: ID,
      title: "Standup",
    });
    render(<MediaRecorderPanel {...props()} />);
    expect(await screen.findByRole("button", { name: "Standup" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^stop$/i })).toBeNull();
  });
});
