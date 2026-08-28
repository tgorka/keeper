import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  configLayers: vi.fn(),
}));

import {
  CONFIG_SOURCE_FAULTS_TITLE,
  CONFIG_SOURCE_MAIN_FOLDER_LABEL,
  CONFIG_SOURCE_TITLE,
  ConfigSourceSection,
  FILE_CONTROLLED_LABEL,
  FileControlled,
  fileControlledDetail,
} from "@/components/settings/config-source-section";
import type { ConfigLayersVm } from "@/lib/ipc/client";
import { configLayers } from "@/lib/ipc/client";
import { CONFIG_LAYERS_UNREADABLE, configLayersStore } from "@/lib/stores/config-layers";

const mockLayers = vi.mocked(configLayers);

/** A stack in which nothing is set by a file — the normal, healthy install. */
function emptyVm(over: Partial<ConfigLayersVm> = {}): ConfigLayersVm {
  return {
    overrides: [],
    faults: [],
    mainFolder: null,
    summary: "No setting is being set by a file. Everything here is stored by keeper.",
    ...over,
  };
}

/** One key decided by the user-global file. */
function overriddenVm(over: Partial<ConfigLayersVm> = {}): ConfigLayersVm {
  return emptyVm({
    overrides: [
      {
        key: "hotkey.global",
        tier: "userGlobal",
        path: "/Users/t/.keeper/keeper.toml",
        folder: null,
        source: "your settings file, for every machine and folder",
      },
    ],
    summary:
      "1 setting is set by a file. Changing it here will not take effect while the file sets it.",
    ...over,
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  // The store is module-scoped and shared; reset it so one test's stack cannot
  // decide another test's markers.
  configLayersStore.setState({ layers: null, error: null });
  mockLayers.mockResolvedValue(emptyVm());
});

describe("ConfigSourceSection", () => {
  it("renders on an install where no file sets anything, so the mechanism is discoverable", async () => {
    render(<ConfigSourceSection open />);

    expect(await screen.findByText(CONFIG_SOURCE_TITLE)).toBeInTheDocument();
    // Verbatim from Rust: the counts and the consequence are one sentence,
    // asserted in keeper-core, so the pane and the log cannot word the same
    // state two different ways.
    expect(
      screen.getByText("No setting is being set by a file. Everything here is stored by keeper."),
    ).toBeInTheDocument();
  });

  it("names each overridden key with the file that decided it and how far that file reaches", async () => {
    mockLayers.mockResolvedValue(overriddenVm());
    render(<ConfigSourceSection open />);

    // The raw key, because it is what a person types into the file. A
    // prettified label would be unsearchable in the one document this is about.
    expect(await screen.findByText("hotkey.global")).toBeInTheDocument();
    expect(
      screen.getByText("your settings file, for every machine and folder"),
    ).toBeInTheDocument();
    expect(screen.getByText("/Users/t/.keeper/keeper.toml")).toBeInTheDocument();
  });

  it("shows a fault, which is the loud half: a file that did not load sets nothing", async () => {
    mockLayers.mockResolvedValue(
      emptyVm({
        faults: [
          {
            kind: "mainFolderNotAProfile",
            path: "/Volumes/merope/tgdrve",
            summary:
              "/Volumes/merope/tgdrve: mainSyncFolder names a folder keeper does not sync, so the keeper.toml files inside it were not read.",
          },
        ],
      }),
    );
    render(<ConfigSourceSection open />);

    expect(await screen.findByText(CONFIG_SOURCE_FAULTS_TITLE)).toBeInTheDocument();
    expect(
      screen.getByText(
        "/Volumes/merope/tgdrve: mainSyncFolder names a folder keeper does not sync, so the keeper.toml files inside it were not read.",
      ),
    ).toBeInTheDocument();
  });

  it("still names a rejected main folder, because the typo is the thing to fix", async () => {
    mockLayers.mockResolvedValue(
      emptyVm({
        mainFolder: "/Volumes/merope/tgdrve",
        faults: [
          {
            kind: "mainFolderMissing",
            path: "/Volumes/merope/tgdrve",
            summary: "/Volumes/merope/tgdrve: no such folder",
          },
        ],
      }),
    );
    render(<ConfigSourceSection open />);

    expect(
      await screen.findByText(new RegExp(CONFIG_SOURCE_MAIN_FOLDER_LABEL)),
    ).toBeInTheDocument();
    expect(screen.getAllByText("/Volumes/merope/tgdrve").length).toBeGreaterThan(0);
  });

  it("says the list may be incomplete when the read failed, rather than claiming an empty stack", async () => {
    mockLayers.mockRejectedValue(new Error("nope"));
    render(<ConfigSourceSection open />);

    expect(await screen.findByText(CONFIG_LAYERS_UNREADABLE)).toBeInTheDocument();
    // Never the healthy sentence: "nothing overrides anything" is a claim the
    // frontend has not earned when the question went unanswered.
    expect(
      screen.queryByText("No setting is being set by a file. Everything here is stored by keeper."),
    ).not.toBeInTheDocument();
  });

  it("keeps the last good list on screen when a later read fails, and admits it may be behind", async () => {
    mockLayers.mockResolvedValue(overriddenVm());
    const { rerender } = render(<ConfigSourceSection open={false} />);
    rerender(<ConfigSourceSection open />);
    expect(await screen.findByText("hotkey.global")).toBeInTheDocument();

    mockLayers.mockRejectedValue(new Error("nope"));
    rerender(<ConfigSourceSection open={false} />);
    rerender(<ConfigSourceSection open />);

    expect(await screen.findByText(CONFIG_LAYERS_UNREADABLE)).toBeInTheDocument();
    expect(screen.getByText("hotkey.global")).toBeInTheDocument();
  });

  it("does not read the stack until it is open", () => {
    render(<ConfigSourceSection open={false} />);

    expect(mockLayers).not.toHaveBeenCalled();
  });

  it("re-reads on every open, because faults arrive after boot", async () => {
    const { rerender } = render(<ConfigSourceSection open />);
    await waitFor(() => expect(mockLayers).toHaveBeenCalledTimes(1));

    rerender(<ConfigSourceSection open={false} />);
    rerender(<ConfigSourceSection open />);

    // The `mainSyncFolder` fault is pushed after the sync engine opens and the
    // folder tier's faults refresh as profiles are read, so a once-per-lifetime
    // cache would show a stack that had not finished being wrong yet.
    await waitFor(() => expect(mockLayers).toHaveBeenCalledTimes(2));
  });
});

describe("FileControlled", () => {
  it("renders nothing for a key no file decides, which is every key on a plain install", async () => {
    render(
      <>
        <ConfigSourceSection open />
        <FileControlled settingKey="hotkey.global" />
      </>,
    );

    await screen.findByText(CONFIG_SOURCE_TITLE);
    expect(screen.queryByText(FILE_CONTROLLED_LABEL)).not.toBeInTheDocument();
  });

  it("marks a control whose value a file decides, and says which file", async () => {
    mockLayers.mockResolvedValue(overriddenVm());
    render(
      <>
        <ConfigSourceSection open />
        <FileControlled settingKey="hotkey.global" />
      </>,
    );

    const badge = await screen.findByText(FILE_CONTROLLED_LABEL);
    // The badge is two words so it fits inside a settings row; the sentence
    // that explains it rides on the badge for the reader who stops there.
    expect(badge).toHaveAttribute(
      "aria-label",
      fileControlledDetail(
        "hotkey.global",
        "your settings file, for every machine and folder",
        "/Users/t/.keeper/keeper.toml",
      ),
    );
  });

  it("marks only the key the file decides, not its neighbours", async () => {
    mockLayers.mockResolvedValue(overriddenVm());
    render(
      <>
        <ConfigSourceSection open />
        <FileControlled settingKey="hotkey.recording" />
      </>,
    );

    await screen.findByText("hotkey.global");
    expect(screen.queryByText(FILE_CONTROLLED_LABEL)).not.toBeInTheDocument();
  });

  it("says the value is overridden without disabling the control beside it", async () => {
    // AD-98 asks a control to SAY so, not to refuse. `set_setting` still writes
    // the settings table, the table is still the fallback, and what a user sets
    // here is exactly what takes effect the moment the file stops setting the
    // key. A disabled control would make that fallback unreachable and turn a
    // temporary override into a permanent one.
    mockLayers.mockResolvedValue(overriddenVm());
    render(
      <>
        <ConfigSourceSection open />
        <label htmlFor="probe">
          Probe
          <FileControlled settingKey="hotkey.global" />
          <input id="probe" />
        </label>
      </>,
    );

    await screen.findByText(FILE_CONTROLLED_LABEL);
    expect(screen.getByLabelText(/Probe/)).not.toBeDisabled();
  });
});
