import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  egressList: vi.fn(() => Promise.resolve([])),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(() => Promise.resolve("0.0.0-test")),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(() => Promise.resolve(null)),
}));
vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(() => Promise.resolve()),
}));

import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { AboutSection } from "@/components/settings/about-section";
import { type EgressEndpointVm, egressList } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const mockEgress = vi.mocked(egressList);
const mockCheck = vi.mocked(check);
const mockRelaunch = vi.mocked(relaunch);
const mockOpenUrl = vi.mocked(openUrl);
const mockGetVersion = vi.mocked(getVersion);

/** All seven capabilities present = the desktop tier (updater block renders). */
const DESKTOP_CAPABILITIES = {
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
  recording: false,
};

const UPDATE_ENDPOINT = "https://github.com/tgorka/keeper/releases/latest/download/latest.json";

/** A no-Beeper fleet: one homeserver + the update endpoint. */
const NON_BEEPER_EGRESS: EgressEndpointVm[] = [
  { url: "https://matrix.example.org", kind: "homeserver", label: "Matrix homeserver" },
  { url: UPDATE_ENDPOINT, kind: "update", label: "Signed app updates" },
];

/** A mixed fleet: two homeservers, api.beeper.com once, the update endpoint. */
const MIXED_EGRESS: EgressEndpointVm[] = [
  { url: "https://matrix.example.org", kind: "homeserver", label: "Matrix homeserver" },
  { url: "https://matrix.beeper.com", kind: "homeserver", label: "Matrix homeserver" },
  { url: "https://api.beeper.com", kind: "beeper", label: "Beeper account service" },
  { url: UPDATE_ENDPOINT, kind: "update", label: "Signed app updates" },
];

beforeEach(() => {
  mockEgress.mockReset();
  mockEgress.mockResolvedValue([]);
  mockCheck.mockReset();
  mockCheck.mockResolvedValue(null);
  mockRelaunch.mockReset();
  mockRelaunch.mockResolvedValue(undefined);
  mockOpenUrl.mockReset();
  mockOpenUrl.mockResolvedValue(undefined);
  mockGetVersion.mockReset();
  mockGetVersion.mockResolvedValue("0.0.0-test");
  // Default the mirror to the desktop tier so the software-update block renders for
  // the egress/update-flow assertions; the reduced-platform cases opt in explicitly.
  capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
});

afterEach(() => {
  vi.clearAllMocks();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("AboutSection egress list", () => {
  it("does not load while closed", () => {
    render(<AboutSection open={false} />);
    expect(mockEgress).not.toHaveBeenCalled();
  });

  it("renders every homeserver, api.beeper.com once, and the update endpoint", async () => {
    mockEgress.mockResolvedValue(MIXED_EGRESS);
    render(<AboutSection open />);

    await waitFor(() => {
      expect(screen.getByText("https://matrix.example.org")).toBeInTheDocument();
    });
    expect(screen.getByText("https://matrix.beeper.com")).toBeInTheDocument();
    expect(screen.getByText("https://api.beeper.com")).toBeInTheDocument();
    expect(screen.getByText(UPDATE_ENDPOINT)).toBeInTheDocument();
    // api.beeper.com appears exactly once.
    expect(screen.getAllByText("https://api.beeper.com")).toHaveLength(1);
  });

  it("does not render api.beeper.com when no Beeper account exists", async () => {
    mockEgress.mockResolvedValue(NON_BEEPER_EGRESS);
    render(<AboutSection open />);

    await waitFor(() => {
      expect(screen.getByText("https://matrix.example.org")).toBeInTheDocument();
    });
    expect(screen.queryByText("https://api.beeper.com")).not.toBeInTheDocument();
    expect(screen.getByText(UPDATE_ENDPOINT)).toBeInTheDocument();
  });

  it("renders an honest error line when the egress list cannot load", async () => {
    mockEgress.mockRejectedValue(new Error("registry read failed"));
    render(<AboutSection open />);

    await waitFor(() => {
      expect(screen.getByText("Could not load the egress list.")).toBeInTheDocument();
    });
  });
});

describe("AboutSection installed version", () => {
  it("renders the installed version once open", async () => {
    mockGetVersion.mockResolvedValue("1.2.3");
    render(<AboutSection open />);

    expect(screen.getByText("Installed version")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText("1.2.3")).toBeInTheDocument();
    });
  });

  it("renders an honest 'unknown' when the version read fails", async () => {
    mockGetVersion.mockRejectedValue(new Error("no runtime"));
    render(<AboutSection open />);

    await waitFor(() => {
      expect(screen.getByText("unknown")).toBeInTheDocument();
    });
  });
});

describe("AboutSection update flow", () => {
  it("reports up-to-date when no update is available", async () => {
    mockCheck.mockResolvedValue(null);
    render(<AboutSection open />);
    await waitFor(() => expect(mockEgress).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));

    await waitFor(() => {
      expect(screen.getByText("keeper is up to date.")).toBeInTheDocument();
    });
    expect(mockRelaunch).not.toHaveBeenCalled();
  });

  it("surfaces an available update but does not install or relaunch on a mere check", async () => {
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    // The updater's Update object carries `version` + `downloadAndInstall`.
    mockCheck.mockResolvedValue({
      version: "0.2.0",
      downloadAndInstall,
      // biome-ignore lint/suspicious/noExplicitAny: minimal Update stub for the test.
    } as any);
    render(<AboutSection open />);
    await waitFor(() => expect(mockEgress).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));

    await waitFor(() => {
      expect(screen.getByText("Update 0.2.0 available.")).toBeInTheDocument();
    });
    // Consent gate: checking alone must never download, install, or relaunch.
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(mockRelaunch).not.toHaveBeenCalled();
  });

  it("downloads, installs, and relaunches only after the explicit install click", async () => {
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    mockCheck.mockResolvedValue({
      version: "0.2.0",
      downloadAndInstall,
      // biome-ignore lint/suspicious/noExplicitAny: minimal Update stub for the test.
    } as any);
    render(<AboutSection open />);
    await waitFor(() => expect(mockEgress).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Download and install" })).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Download and install" }));

    await waitFor(() => expect(downloadAndInstall).toHaveBeenCalled());
    await waitFor(() => expect(mockRelaunch).toHaveBeenCalled());
    // A real relaunch exits the process; when relaunch resolves without doing so
    // (as the mock does), the flow must not stay stuck on "downloading".
    await waitFor(() => {
      expect(screen.getByText("Update installed. Restart keeper to finish.")).toBeInTheDocument();
    });
  });

  it("reports an installed-but-not-restarted state when relaunch fails", async () => {
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    mockCheck.mockResolvedValue({
      version: "0.2.0",
      downloadAndInstall,
      // biome-ignore lint/suspicious/noExplicitAny: minimal Update stub for the test.
    } as any);
    mockRelaunch.mockRejectedValue(new Error("relaunch not permitted"));
    render(<AboutSection open />);
    await waitFor(() => expect(mockEgress).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Download and install" })).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Download and install" }));

    await waitFor(() => {
      expect(screen.getByText("Update installed. Restart keeper to finish.")).toBeInTheDocument();
    });
  });

  it("surfaces an offline check failure as a rendered error state, never thrown", async () => {
    mockCheck.mockRejectedValue(new Error("network is offline"));
    render(<AboutSection open />);
    await waitFor(() => expect(mockEgress).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));

    await waitFor(() => {
      expect(screen.getByText("Update failed: network is offline")).toBeInTheDocument();
    });
    expect(mockRelaunch).not.toHaveBeenCalled();
  });

  it("surfaces a bad-signature install failure as a rendered error state", async () => {
    const downloadAndInstall = vi.fn(() =>
      Promise.reject(new Error("signature verification failed")),
    );
    mockCheck.mockResolvedValue({
      version: "0.2.0",
      downloadAndInstall,
      // biome-ignore lint/suspicious/noExplicitAny: minimal Update stub for the test.
    } as any);
    render(<AboutSection open />);
    await waitFor(() => expect(mockEgress).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Download and install" })).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Download and install" }));

    await waitFor(() => {
      expect(screen.getByText("Update failed: signature verification failed")).toBeInTheDocument();
    });
    expect(mockRelaunch).not.toHaveBeenCalled();
  });
});

describe("AboutSection capability gating (Story 13.7)", () => {
  it("desktop: renders the software-update block and no 'On this iPhone' disclosure", async () => {
    mockEgress.mockResolvedValue(NON_BEEPER_EGRESS);
    // beforeEach already hydrated the desktop tier.
    render(<AboutSection open />);

    expect(await screen.findByText("Software updates")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeInTheDocument();
    expect(screen.queryByText("On this iPhone")).not.toBeInTheDocument();
    // The egress list is present regardless (never gated).
    expect(screen.getByText("https://matrix.example.org")).toBeInTheDocument();
  });

  it("iOS: hides the software-update block, keeps the egress list, and shows the 'On this iPhone' disclosure", async () => {
    mockEgress.mockResolvedValue(NON_BEEPER_EGRESS);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<AboutSection open />);

    // The egress list stays ungated…
    await waitFor(() => {
      expect(screen.getByText("https://matrix.example.org")).toBeInTheDocument();
    });
    // …but the software-update block is gone (no dead "Check for updates" button).
    expect(screen.queryByText("Software updates")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Check for updates" })).not.toBeInTheDocument();
    // The "On this iPhone" list renders all four honesty lines.
    expect(screen.getByText("On this iPhone")).toBeInTheDocument();
    expect(screen.getByText(/syncs and notifies only while it's open/)).toBeInTheDocument();
    expect(screen.getByText(/No self-hosted bridge runner/)).toBeInTheDocument();
    expect(screen.getByText("No global summon hotkey.")).toBeInTheDocument();
    expect(screen.getByText(/signature renews every 7 days/)).toBeInTheDocument();
  });

  it("iOS: the docs link opens docs/ios.md externally via openUrl (best-effort)", async () => {
    mockEgress.mockResolvedValue(NON_BEEPER_EGRESS);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<AboutSection open />);

    const link = await screen.findByRole("link", { name: /iPhone/i });
    fireEvent.click(link);
    expect(mockOpenUrl).toHaveBeenCalledWith(
      "https://github.com/tgorka/keeper/blob/main/docs/ios.md",
    );
  });

  it("pre-hydration: hides the update block by the safe default but does NOT flash the 'On this iPhone' list", async () => {
    mockEgress.mockResolvedValue(NON_BEEPER_EGRESS);
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
    render(<AboutSection open />);

    await waitFor(() => {
      expect(screen.getByText("https://matrix.example.org")).toBeInTheDocument();
    });
    // Desktop-only updater hidden by the safe default…
    expect(screen.queryByText("Software updates")).not.toBeInTheDocument();
    // …but the iOS-only disclosure must NOT flash before the mirror resolves.
    expect(screen.queryByText("On this iPhone")).not.toBeInTheDocument();
  });
});
