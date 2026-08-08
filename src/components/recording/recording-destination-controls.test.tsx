import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recordingDestinationProfiles: vi.fn(),
  recordingPathPreview: vi.fn(),
  recordingSettingsGet: vi.fn(),
  recordingSettingsSet: vi.fn(),
}));

// The OS-native directory picker (the export-dialog mock pattern).
const openFolder = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openFolder(...args),
}));

import {
  CHOOSE_FOLDER_LABEL,
  DESTINATION_CHOICE_FOLDER_LABEL,
  DESTINATION_CHOICE_LABEL,
  DESTINATION_CHOICE_PROFILE_LABEL,
  DESTINATION_CHOICE_TESTID,
  DESTINATION_LOCAL_ONLY_NOTE,
  DESTINATION_NEXT_SESSION_NOTE,
  DESTINATION_PATH_TESTID,
  DESTINATION_PROFILE_SELECT_TESTID,
  DESTINATION_SYNCED_NOTE_TESTID,
  DESTINATION_TEMPLATE_FAULT_TESTID,
  DESTINATION_TEMPLATE_PREVIEW_TESTID,
  DESTINATION_TEMPLATE_SAVE_TESTID,
  DESTINATION_TEMPLATE_TESTID,
  DESTINATION_VOLUME_NOTE_TESTID,
  destinationSyncedNote,
  destinationVolumeNote,
  RecordingDestinationControls,
} from "@/components/recording/recording-destination-controls";
import type {
  RecordingPathPreviewVm,
  RecordingProfileVm,
  RecordingSettingsVm,
} from "@/lib/ipc/client";
import {
  recordingDestinationProfiles,
  recordingPathPreview,
  recordingSettingsGet,
  recordingSettingsSet,
} from "@/lib/ipc/client";
import { recordingMetaStore } from "@/lib/stores/recording-meta";
import {
  RECORDING_PATH_TEMPLATE_DEFAULT,
  resetRecordingSettingsForTest,
} from "@/lib/stores/recording-settings";

const mockGet = vi.mocked(recordingSettingsGet);
const mockSet = vi.mocked(recordingSettingsSet);
const mockPreview = vi.mocked(recordingPathPreview);
const mockProfiles = vi.mocked(recordingDestinationProfiles);

const DEFAULTS: RecordingSettingsVm = {
  segmentMb: 500,
  durationCapMinutes: 30,
  destinationDir: "/Users/alice/Movies/keeper",
  destinationKind: "folder",
  destinationProfileId: null,
  destinationProfileName: null,
  destinationVolume: null,
  fps: 30,
  codec: "h264",
  scalePercent: 100,
  echoCancellation: false,
  pathTemplate: RECORDING_PATH_TEMPLATE_DEFAULT,
};

/** Two recordings-flagged synced folders. `recordingsRoot` is Rust-resolved —
 * the card never composes one — so the fixture states it outright. */
const PROFILES: RecordingProfileVm[] = [
  { id: "tgdrive", name: "tgdrive", recordingsRoot: "/Users/alice/tgdrive/recordings" },
  { id: "attic", name: "Attic backup", recordingsRoot: "/Volumes/Attic/keeper/recordings" },
];

/** The settings VM Rust echoes once `tgdrive` is the destination: the kind
 * flips, the id and name are carried, and `destinationDir` is the RESOLVED
 * recordings root — the same field, now answering for the profile. */
const SYNCED: RecordingSettingsVm = {
  ...DEFAULTS,
  destinationDir: "/Users/alice/tgdrive/recordings",
  destinationKind: "profile",
  destinationProfileId: "tgdrive",
  destinationProfileName: "tgdrive",
};

/** Story 41.7: the same synced destination, on the pendrive the field report is
 * about. Rust re-scans the volume on every settings read, so the fixtures differ
 * only in the state the card is handed. */
const SYNCED_ON_ATTACHED_DRIVE: RecordingSettingsVm = {
  ...SYNCED,
  destinationDir: "/Volumes/merope/tgdrive/recordings",
  destinationVolume: { name: "merope", state: "attached" },
};

const SYNCED_ON_DETACHED_DRIVE: RecordingSettingsVm = {
  ...SYNCED_ON_ATTACHED_DRIVE,
  destinationVolume: { name: "merope", state: "absent" },
};

/** What Rust renders for the default template with no session title (the
 * `{slug}` collapse case), at the matrix's 2026-08-05T14:32 local. */
const DEFAULT_PREVIEW: RecordingPathPreviewVm = {
  relativePath: "2026/2026-08-05 1432",
  absolutePath: "/Users/alice/Movies/keeper/2026/2026-08-05 1432",
  problem: null,
};

/** 40.1's `TemplateError::ParentComponent` sentence, verbatim: the card prints
 * the Rust wording, so the test asserts the Rust wording. */
const TRAVERSAL_REASON = 'a template cannot contain a "." or ".." folder';

beforeEach(() => {
  resetRecordingSettingsForTest();
  // The meta store is a module-level singleton shared with every other test
  // file: a title left behind here would re-root a sibling suite's preview.
  recordingMetaStore.getState().setFields({ title: "" });
  mockGet.mockReset();
  mockGet.mockResolvedValue(DEFAULTS);
  mockSet.mockReset();
  mockSet.mockImplementation((vm) => Promise.resolve(vm));
  openFolder.mockReset();
  mockPreview.mockReset();
  mockPreview.mockResolvedValue(DEFAULT_PREVIEW);
  mockProfiles.mockReset();
  // No flagged profile is the default world: today's card.
  mockProfiles.mockResolvedValue([]);
});

afterEach(() => {
  vi.clearAllMocks();
  recordingMetaStore.getState().setFields({ title: "" });
});

describe("RecordingDestinationControls", () => {
  it("shows the effective folder with the next-session note and local-only copy", async () => {
    render(<RecordingDestinationControls />);

    // The Rust-resolved EFFECTIVE default is always a concrete folder.
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
        "/Users/alice/Movies/keeper",
      ),
    );
    expect(screen.getByText(DESTINATION_NEXT_SESSION_NOTE)).toBeInTheDocument();
    expect(screen.getByText(DESTINATION_LOCAL_ONLY_NOTE)).toBeInTheDocument();
    // Local-only: no share/cloud/network affordance anywhere in the card.
    expect(screen.queryByText(/share|cloud|network|http/i)).not.toBeInTheDocument();
  });

  it("opens the native directory picker and persists a confirmed selection", async () => {
    openFolder.mockResolvedValue("/Users/alice/Recordings");
    render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() => expect(openFolder).toHaveBeenCalledWith({ directory: true }));
    await waitFor(() =>
      expect(mockSet).toHaveBeenCalledWith({
        ...DEFAULTS,
        destinationDir: "/Users/alice/Recordings",
      }),
    );
    // The card reflects the effective persisted folder.
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
        "/Users/alice/Recordings",
      ),
    );
  });

  it("keeps the current folder when the picker is cancelled", async () => {
    // A cancelled native picker resolves `null` — no write, no change.
    openFolder.mockResolvedValue(null);
    render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() => expect(openFolder).toHaveBeenCalled());
    expect(mockSet).not.toHaveBeenCalled();
    expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper",
    );
  });

  it("keeps the current folder when the picker throws", async () => {
    openFolder.mockRejectedValue(new Error("picker unavailable"));
    render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() => expect(openFolder).toHaveBeenCalled());
    expect(mockSet).not.toHaveBeenCalled();
  });

  it("disables the chooser until the shared store hydrates", () => {
    // Never-resolving hydration: the affordance must not pretend to work.
    mockGet.mockImplementation(() => new Promise(() => {}));
    render(<RecordingDestinationControls />);

    expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeDisabled();
  });

  it("previews a typed template through Rust and writes nothing", async () => {
    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    // The field seeds from the EFFECTIVE template the store hydrated with.
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));

    mockPreview.mockResolvedValue({
      relativePath: "2026/08/05",
      absolutePath: "/Users/alice/Movies/keeper/2026/08/05",
      problem: null,
    });
    fireEvent.change(field, { target: { value: "{yyyy}/{mm}/{dd}" } });

    // The typed text goes to the backend verbatim; the empty meta title is
    // sent as `null` so Rust renders its untitled collapse.
    await waitFor(() => expect(mockPreview).toHaveBeenCalledWith("{yyyy}/{mm}/{dd}", null));
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
        "/Users/alice/Movies/keeper/2026/08/05",
      ),
    );
    // Typing is not a write.
    expect(mockSet).not.toHaveBeenCalled();
  });

  it("prints the Rust reason for an unparseable template and disables the save", async () => {
    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));

    mockPreview.mockResolvedValue({
      relativePath: null,
      absolutePath: null,
      problem: TRAVERSAL_REASON,
    });
    fireEvent.change(field, { target: { value: "../{yyyy}" } });

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_FAULT_TESTID)).toHaveTextContent(
        TRAVERSAL_REASON,
      ),
    );
    expect(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID)).toBeDisabled();
    // Never both: a template that cannot name a folder shows no path.
    expect(screen.queryByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).not.toBeInTheDocument();
  });

  it("saves a cleared template and repopulates from the effective default", async () => {
    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));

    // Clearing is a save, not a fault: Rust previews a blank as the default.
    fireEvent.change(field, { target: { value: "" } });
    await waitFor(() => expect(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID)).toBeEnabled());
    // The stored key is cleared, but the EFFECTIVE echo is never blank.
    mockSet.mockResolvedValue({ ...DEFAULTS, pathTemplate: RECORDING_PATH_TEMPLATE_DEFAULT });

    fireEvent.click(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID));

    await waitFor(() => expect(mockSet).toHaveBeenCalledWith({ ...DEFAULTS, pathTemplate: "" }));
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));
  });

  it("re-asks Rust for the preview when the destination folder changes", async () => {
    render(<RecordingDestinationControls />);
    // The first preview is rooted at the hydrated folder.
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
        "/Users/alice/Movies/keeper/2026/2026-08-05 1432",
      ),
    );
    const beforeFolderChange = mockPreview.mock.calls.length;

    // The command resolves the root itself, so the answer it already gave is
    // stale the moment the folder moves: Rust must be asked again.
    openFolder.mockResolvedValue("/Volumes/Pendrive");
    mockPreview.mockResolvedValue({
      relativePath: "2026/2026-08-05 1432",
      absolutePath: "/Volumes/Pendrive/2026/2026-08-05 1432",
      problem: null,
    });
    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent("/Volumes/Pendrive"),
    );
    await waitFor(() => expect(mockPreview.mock.calls.length).toBeGreaterThan(beforeFolderChange));
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
        "/Volumes/Pendrive/2026/2026-08-05 1432",
      ),
    );
  });

  it("applies only the newest preview response when replies land out of order", async () => {
    // Manually controlled replies, so the test decides the landing order.
    const pending: Array<{
      promise: Promise<RecordingPathPreviewVm>;
      resolve: (vm: RecordingPathPreviewVm) => void;
      reject: (reason: unknown) => void;
    }> = [];
    mockPreview.mockImplementation(() => {
      let resolve!: (vm: RecordingPathPreviewVm) => void;
      let reject!: (reason: unknown) => void;
      const promise = new Promise<RecordingPathPreviewVm>((res, rej) => {
        resolve = res;
        reject = rej;
      });
      pending.push({ promise, resolve, reject });
      return promise;
    });

    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));
    // [0] is the seed preview; it is left in flight and failed last, below.
    await waitFor(() => expect(pending).toHaveLength(1));

    fireEvent.change(field, { target: { value: "{yyyy}/a" } });
    await waitFor(() => expect(pending).toHaveLength(2));
    fireEvent.change(field, { target: { value: "{yyyy}/b" } });
    await waitFor(() => expect(pending).toHaveLength(3));

    // The newest reply lands first.
    await act(async () => {
      pending[2].resolve({
        relativePath: "2026/b",
        absolutePath: "/Users/alice/Movies/keeper/2026/b",
        problem: null,
      });
    });
    expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper/2026/b",
    );
    expect(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID)).toBeEnabled();

    // The loser of the race answers late with a different path: it is dropped.
    await act(async () => {
      pending[1].resolve({
        relativePath: "2026/a",
        absolutePath: "/Users/alice/Movies/keeper/2026/a",
        problem: null,
      });
    });
    expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper/2026/b",
    );
    expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).not.toHaveTextContent(
      "/Users/alice/Movies/keeper/2026/a",
    );

    // A stale round trip that FAILS late is dropped by the same token, so it
    // cannot blank the newest path or re-disable the save.
    await act(async () => {
      pending[0].reject(new Error("preview round trip lost"));
    });
    expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper/2026/b",
    );
    // The save's verdict is the newest response's, not the stragglers'.
    expect(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID)).toBeEnabled();
  });

  it("previews untitled on the Settings surface even with a title pending", async () => {
    // A title typed on the pre-record pane outlives that card's mount; the
    // Settings dialog must not inherit it, or `{slug}`'s collapse never shows.
    act(() => {
      recordingMetaStore.getState().setFields({ title: "Standup" });
    });
    render(<RecordingDestinationControls />);

    await waitFor(() => expect(mockPreview).toHaveBeenCalled());
    for (const call of mockPreview.mock.calls) {
      expect(call[1]).toBeNull();
    }
  });

  it("previews against the pending title on the pre-record surface", async () => {
    act(() => {
      recordingMetaStore.getState().setFields({ title: "Standup" });
    });
    render(<RecordingDestinationControls withNextSessionTitle />);

    await waitFor(() =>
      expect(mockPreview).toHaveBeenCalledWith(RECORDING_PATH_TEMPLATE_DEFAULT, "Standup"),
    );

    // Whitespace is not a title: it collapses to `null` so Rust renders the
    // untitled form rather than a folder named with blanks.
    act(() => {
      recordingMetaStore.getState().setFields({ title: "   " });
    });
    await waitFor(() =>
      expect(mockPreview).toHaveBeenLastCalledWith(RECORDING_PATH_TEMPLATE_DEFAULT, null),
    );
  });

  it("keeps the typed template and prints a refused write, then clears it on success", async () => {
    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));

    mockPreview.mockResolvedValue({
      relativePath: "2026/notes",
      absolutePath: "/Users/alice/Movies/keeper/2026/notes",
      problem: null,
    });
    fireEvent.change(field, { target: { value: "{yyyy}/notes" } });
    await waitFor(() => expect(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID)).toBeEnabled());

    // The preview said fine and the write still refused: an IPC rejection is a
    // `{ code, message }` envelope, and its sentence is what must print.
    const WRITE_REFUSAL = "keeper could not write the recording settings.";
    mockSet.mockRejectedValueOnce({
      code: "internal",
      message: WRITE_REFUSAL,
      retriable: true,
    });
    fireEvent.click(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID));

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_FAULT_TESTID)).toHaveTextContent(
        WRITE_REFUSAL,
      ),
    );
    // The mirror reverted behind the refusal; the field did NOT — the text the
    // sentence is about stays put, ready to be corrected or retried.
    expect(field).toHaveValue("{yyyy}/notes");

    fireEvent.click(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID));

    await waitFor(() =>
      expect(mockSet).toHaveBeenLastCalledWith({ ...DEFAULTS, pathTemplate: "{yyyy}/notes" }),
    );
    // Confirmed: the stale refusal is gone and the field holds the effective
    // template Rust echoed back.
    await waitFor(() =>
      expect(screen.queryByTestId(DESTINATION_TEMPLATE_FAULT_TESTID)).not.toBeInTheDocument(),
    );
    expect(field).toHaveValue("{yyyy}/notes");
  });

  // ── Story 41.2: the destination is a decision, not a path ────────────────

  it("is today's card when no profile is flagged, and asks for the list once", async () => {
    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));
    await waitFor(() => expect(mockProfiles).toHaveBeenCalledTimes(1));

    // No choice, no picker, no consequence line — and not a disabled radio or an
    // empty select either: the affordances are absent, not inert.
    expect(screen.queryByTestId(DESTINATION_CHOICE_TESTID)).not.toBeInTheDocument();
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    expect(screen.queryByTestId(DESTINATION_PROFILE_SELECT_TESTID)).not.toBeInTheDocument();
    expect(screen.queryByTestId(DESTINATION_SYNCED_NOTE_TESTID)).not.toBeInTheDocument();
    // Today's controls and today's copy, unchanged.
    expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeEnabled();
    expect(screen.getByText(DESTINATION_LOCAL_ONLY_NOTE)).toBeInTheDocument();
    expect(screen.getByText(DESTINATION_NEXT_SESSION_NOTE)).toBeInTheDocument();

    // The list is read ONCE per mount: typing re-previews per keystroke, and
    // the profiles command must not ride along with it.
    fireEvent.change(field, { target: { value: "{yyyy}/{mm}" } });
    await waitFor(() => expect(mockPreview).toHaveBeenCalledWith("{yyyy}/{mm}", null));
    expect(mockProfiles).toHaveBeenCalledTimes(1);
  });

  it("persists a chosen synced folder and states the consequence at its resolved root", async () => {
    mockProfiles.mockResolvedValue(PROFILES);
    mockSet.mockResolvedValue(SYNCED);
    render(<RecordingDestinationControls />);

    // The house two-way choice: one radiogroup, named, carrying the card's id.
    const choice = await screen.findByRole("radiogroup", { name: DESTINATION_CHOICE_LABEL });
    expect(screen.getByTestId(DESTINATION_CHOICE_TESTID)).toBe(choice);
    const syncedRadio = screen.getByRole("radio", { name: DESTINATION_CHOICE_PROFILE_LABEL });
    await waitFor(() => expect(syncedRadio).toBeEnabled());

    fireEvent.click(syncedRadio);

    // The write carries the kind and the id; `destinationDir` is an output
    // under this kind, so the card sends the one it was holding untouched.
    await waitFor(() =>
      expect(mockSet).toHaveBeenCalledWith({
        ...DEFAULTS,
        destinationKind: "profile",
        destinationProfileId: "tgdrive",
      }),
    );
    // The rendered root is the one the VM returned — nothing was joined here.
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
        "/Users/alice/tgdrive/recordings",
      ),
    );
    expect(screen.getByTestId(DESTINATION_SYNCED_NOTE_TESTID)).toHaveTextContent(
      destinationSyncedNote("tgdrive"),
    );
    // "Nothing uploads." would now be a lie, so it is gone rather than joined.
    expect(screen.queryByText(DESTINATION_LOCAL_ONLY_NOTE)).not.toBeInTheDocument();
    // The picker replaces the folder chooser, showing the profile by name.
    expect(screen.getByTestId(DESTINATION_PROFILE_SELECT_TESTID)).toHaveTextContent("tgdrive");
  });

  it("shows the profile answer the read resolved without persisting anything", async () => {
    // The matrix's default row (one flagged profile on a fresh install) is a
    // RESOLUTION, not a preselection: Rust answers `kind: "profile"` and the
    // card renders whatever the effective answer is. Mounting a surface must
    // never write, least of all a write that redirects recordings into a folder
    // something else will push.
    mockProfiles.mockResolvedValue(PROFILES);
    mockGet.mockResolvedValue(SYNCED);
    render(<RecordingDestinationControls />);

    await waitFor(() =>
      expect(screen.getByRole("radio", { name: DESTINATION_CHOICE_PROFILE_LABEL })).toBeChecked(),
    );
    expect(screen.getByTestId(DESTINATION_PROFILE_SELECT_TESTID)).toHaveTextContent("tgdrive");
    expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
      "/Users/alice/tgdrive/recordings",
    );
    expect(screen.getByTestId(DESTINATION_SYNCED_NOTE_TESTID)).toHaveTextContent(
      destinationSyncedNote("tgdrive"),
    );
    expect(screen.queryByText(DESTINATION_LOCAL_ONLY_NOTE)).not.toBeInTheDocument();
    expect(mockSet).not.toHaveBeenCalled();
  });

  it("says a synced destination is on removable media before Record is pressed", async () => {
    // Matrix row "removable destination, attached". Nothing has failed and
    // nothing has been asked; the card volunteers that this folder lives on a
    // drive, which is the whole point of putting it here rather than in an error.
    mockProfiles.mockResolvedValue(PROFILES);
    mockGet.mockResolvedValue(SYNCED_ON_ATTACHED_DRIVE);
    render(<RecordingDestinationControls />);

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_VOLUME_NOTE_TESTID)).toHaveTextContent(
        destinationVolumeNote({ name: "merope", state: "attached" }),
      ),
    );
    expect(screen.getByTestId(DESTINATION_VOLUME_NOTE_TESTID)).toHaveTextContent("merope");
    // The consequence sentence is a different fact and stays put beside it.
    expect(screen.getByTestId(DESTINATION_SYNCED_NOTE_TESTID)).toHaveTextContent(
      destinationSyncedNote("tgdrive"),
    );
  });

  it("says the drive is not attached, unprompted, while still naming the chosen folder", async () => {
    // Matrix row "removable destination, detached, card open". Rust does NOT
    // degrade to the plain folder here, so the card must keep naming the synced
    // root — and say why a recording would not start.
    mockProfiles.mockResolvedValue(PROFILES);
    mockGet.mockResolvedValue(SYNCED_ON_DETACHED_DRIVE);
    render(<RecordingDestinationControls />);

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_VOLUME_NOTE_TESTID)).toHaveTextContent(
        destinationVolumeNote({ name: "merope", state: "absent" }),
      ),
    );
    expect(screen.getByTestId(DESTINATION_VOLUME_NOTE_TESTID)).toHaveTextContent("isn't attached");
    expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
      "/Volumes/merope/tgdrive/recordings",
    );
    expect(screen.getByTestId(DESTINATION_PATH_TESTID)).not.toHaveTextContent(
      "/Users/alice/Movies/keeper",
    );
    expect(mockSet).not.toHaveBeenCalled();
  });

  it("says nothing about drives for a synced folder that is not on removable media", async () => {
    // Matrix row "non-removable profile": no removable wording ANYWHERE. A
    // sentence about drives on an ordinary folder is noise that teaches people
    // to stop reading this card.
    mockProfiles.mockResolvedValue(PROFILES);
    mockGet.mockResolvedValue(SYNCED);
    render(<RecordingDestinationControls />);

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_SYNCED_NOTE_TESTID)).toBeInTheDocument(),
    );
    expect(screen.queryByTestId(DESTINATION_VOLUME_NOTE_TESTID)).not.toBeInTheDocument();
    expect(screen.queryByText(/removable/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/attached/i)).not.toBeInTheDocument();
  });

  it("describes an unnamed drive rather than inventing a name for it", async () => {
    // A drive that has been out since launch cannot be named: its name lives in
    // a marker on the drive. The card says so instead of slicing "merope" out of
    // the mountpoint, which is the guess Story 27.3 exists to forbid.
    mockProfiles.mockResolvedValue(PROFILES);
    mockGet.mockResolvedValue({
      ...SYNCED_ON_DETACHED_DRIVE,
      destinationVolume: { name: null, state: "absent" },
    });
    render(<RecordingDestinationControls />);

    const note = await screen.findByTestId(DESTINATION_VOLUME_NOTE_TESTID);
    expect(note).toHaveTextContent("This folder's drive isn't attached");
    expect(note).not.toHaveTextContent("merope");
  });

  it("stops saying the drive is missing once it comes back", async () => {
    // Matrix row "volume returns": the card is a live read of a re-scanned
    // state, not a latch that something has to clear.
    mockProfiles.mockResolvedValue(PROFILES);
    mockGet.mockResolvedValue(SYNCED_ON_DETACHED_DRIVE);
    const { unmount } = render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_VOLUME_NOTE_TESTID)).toHaveTextContent(
        "isn't attached",
      ),
    );

    unmount();
    resetRecordingSettingsForTest();
    mockGet.mockResolvedValue(SYNCED_ON_ATTACHED_DRIVE);
    render(<RecordingDestinationControls />);

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_VOLUME_NOTE_TESTID)).toHaveTextContent(
        destinationVolumeNote({ name: "merope", state: "attached" }),
      ),
    );
    expect(screen.getByTestId(DESTINATION_VOLUME_NOTE_TESTID)).not.toHaveTextContent(
      "isn't attached",
    );
  });

  it("switches the destination between two synced folders through the picker", async () => {
    mockProfiles.mockResolvedValue(PROFILES);
    mockGet.mockResolvedValue(SYNCED);
    render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PROFILE_SELECT_TESTID)).toHaveTextContent("tgdrive"),
    );

    // Open the Radix select via keyboard (jsdom has no real pointer stack).
    fireEvent.keyDown(screen.getByTestId(DESTINATION_PROFILE_SELECT_TESTID), { key: "Enter" });
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual(["tgdrive", "Attic backup"]);

    mockSet.mockResolvedValue({
      ...SYNCED,
      destinationDir: "/Volumes/Attic/keeper/recordings",
      destinationProfileId: "attic",
      destinationProfileName: "Attic backup",
    });
    fireEvent.keyDown(await screen.findByRole("option", { name: "Attic backup" }), {
      key: "Enter",
    });

    await waitFor(() =>
      expect(mockSet).toHaveBeenCalledWith({ ...SYNCED, destinationProfileId: "attic" }),
    );
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
        "/Volumes/Attic/keeper/recordings",
      ),
    );
    expect(screen.getByTestId(DESTINATION_SYNCED_NOTE_TESTID)).toHaveTextContent(
      destinationSyncedNote("Attic backup"),
    );
  });

  it("prints a refused destination verbatim and leaves the previous choice on screen", async () => {
    mockProfiles.mockResolvedValue(PROFILES);
    render(<RecordingDestinationControls />);
    const syncedRadio = await screen.findByRole("radio", {
      name: DESTINATION_CHOICE_PROFILE_LABEL,
    });
    await waitFor(() => expect(syncedRadio).toBeEnabled());

    // The setter's refusal names the profile and what it lacks; the card owns
    // no wording of its own here. `recordingDestinationRefused` is the code
    // both destination refusals carry — the card prints the sentence, not the
    // code, which is why the same slot serves the template's thirteen too.
    const REFUSAL = "tgdrive is not set up to hold recordings.";
    mockSet.mockRejectedValueOnce({
      code: "recordingDestinationRefused",
      message: REFUSAL,
      retriable: false,
    });
    fireEvent.click(syncedRadio);

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_FAULT_TESTID)).toHaveTextContent(REFUSAL),
    );
    // Nothing was written, so the plain folder is still the decision in force —
    // the card never shows a choice the database declined.
    expect(screen.getByRole("radio", { name: DESTINATION_CHOICE_FOLDER_LABEL })).toBeChecked();
    expect(syncedRadio).not.toBeChecked();
    expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper",
    );
    expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeInTheDocument();
    expect(screen.getByText(DESTINATION_LOCAL_ONLY_NOTE)).toBeInTheDocument();
  });

  it("clears the profile id when the choice goes back to a plain folder", async () => {
    mockProfiles.mockResolvedValue(PROFILES);
    mockGet.mockResolvedValue(SYNCED);
    render(<RecordingDestinationControls />);
    const folderRadio = await screen.findByRole("radio", {
      name: DESTINATION_CHOICE_FOLDER_LABEL,
    });
    await waitFor(() => expect(folderRadio).toBeEnabled());

    // "A folder" opens the picker: a blank submission means "no opinion", which
    // on a machine with one flagged profile resolves straight back to that
    // profile, and the profile's own resolved root is the spec's "unambiguous
    // exception". Cancelling therefore writes nothing and changes nothing.
    openFolder.mockResolvedValue(null);
    fireEvent.click(folderRadio);

    await waitFor(() => expect(openFolder).toHaveBeenCalledWith({ directory: true }));
    expect(mockSet).not.toHaveBeenCalled();
    expect(screen.getByRole("radio", { name: DESTINATION_CHOICE_PROFILE_LABEL })).toBeChecked();

    // A named folder is the one submission that always means what it says, and
    // it carries a null profile id.
    openFolder.mockResolvedValue("/Users/alice/Recordings");
    mockSet.mockResolvedValue({ ...DEFAULTS, destinationDir: "/Users/alice/Recordings" });
    fireEvent.click(folderRadio);

    await waitFor(() =>
      expect(mockSet).toHaveBeenCalledWith({
        ...SYNCED,
        destinationKind: "folder",
        destinationProfileId: null,
        destinationDir: "/Users/alice/Recordings",
      }),
    );
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
        "/Users/alice/Recordings",
      ),
    );
    expect(screen.queryByTestId(DESTINATION_SYNCED_NOTE_TESTID)).not.toBeInTheDocument();
    expect(screen.getByText(DESTINATION_LOCAL_ONLY_NOTE)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeInTheDocument();
  });

  it("still previews and saves the 40.2 template while a synced folder is selected", async () => {
    mockProfiles.mockResolvedValue(PROFILES);
    mockGet.mockResolvedValue(SYNCED);
    mockPreview.mockResolvedValue({
      relativePath: "2026/2026-08-05 1432",
      absolutePath: "/Users/alice/tgdrive/recordings/2026/2026-08-05 1432",
      problem: null,
    });
    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));

    mockPreview.mockResolvedValue({
      relativePath: "2026/notes",
      absolutePath: "/Users/alice/tgdrive/recordings/2026/notes",
      problem: null,
    });
    fireEvent.change(field, { target: { value: "{yyyy}/notes" } });

    // The preview is rooted at the profile's resolved root, because Rust roots
    // it at the EFFECTIVE destination whichever kind is in force.
    await waitFor(() => expect(mockPreview).toHaveBeenCalledWith("{yyyy}/notes", null));
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
        "/Users/alice/tgdrive/recordings/2026/notes",
      ),
    );

    fireEvent.click(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID));

    // Saving a template carries the destination decision through untouched.
    await waitFor(() =>
      expect(mockSet).toHaveBeenLastCalledWith({ ...SYNCED, pathTemplate: "{yyyy}/notes" }),
    );
    expect(screen.getByTestId(DESTINATION_SYNCED_NOTE_TESTID)).toHaveTextContent(
      destinationSyncedNote("tgdrive"),
    );
  });
});
