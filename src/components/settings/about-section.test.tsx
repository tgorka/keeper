import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  egressList: vi.fn(() => Promise.resolve([])),
  // The background-update plan (`update.auto` + the cadence): on by default,
  // exactly as an install that has never answered reads it.
  autoUpdateGet: vi.fn(() =>
    Promise.resolve({
      supported: true,
      enabled: true,
      firstCheckDelayMs: 120_000,
      checkIntervalMs: 21_600_000,
      retryDelayMs: 1_800_000,
      restartCheckIntervalMs: 60_000,
    }),
  ),
  autoUpdateSet: vi.fn((enabled: boolean) =>
    Promise.resolve({
      supported: true,
      enabled,
      firstCheckDelayMs: 120_000,
      checkIntervalMs: 21_600_000,
      retryDelayMs: 1_800_000,
      restartCheckIntervalMs: 60_000,
    }),
  ),
  debugModeGet: vi.fn(() => Promise.resolve(false)),
  debugModeSet: vi.fn(() => Promise.resolve()),
  // Where this device's log is (Story 65.3): the default is the Mac's answer,
  // so the pre-existing sentence assertions hold; the phone case opts in.
  debugLogPath: vi.fn(() => Promise.resolve("/Users/alice/Library/Logs/keeper/keeper.log")),
  // The voice facts `useVoiceFacts` reads on open (Story 63.8, AD-179). The
  // default is every build without a voice port, so the pre-existing
  // assertions see no "On this Mac" block; the cases that care opt in.
  voiceAvailability: vi.fn(() =>
    Promise.resolve({ kind: "unsupported", message: "voice is not available in this build" }),
  ),
  voiceWakeGet: vi.fn(() => Promise.reject(new Error("not answered"))),
  // The daily log upload (`diagnostics.log_upload`): on by default, and the
  // setter echoes the effective value the way Rust re-reads it.
  logUploadGet: vi.fn(() => Promise.resolve(true)),
  logUploadSet: vi.fn((enabled: boolean) => Promise.resolve(enabled)),
  accountSendLog: vi.fn(() => Promise.reject(new Error("not answered"))),
  // The phone tier's release check (102.9): unasked until a case answers it.
  releaseCheck: vi.fn(() => Promise.reject(new Error("not answered"))),
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
import {
  AboutSection,
  autoUpdateSentence,
  debugModeSentence,
  MACOS_DISCLOSURE_LINES,
  RELEASE_CHECK_FAILED,
  selfRestartNote,
} from "@/components/settings/about-section";
import {
  accountSendLog,
  autoUpdateGet,
  autoUpdateSet,
  debugLogPath,
  type EgressEndpointVm,
  egressList,
  logUploadGet,
  logUploadSet,
  type PlatformDisclosureVm,
  releaseCheck,
  voiceAvailability,
} from "@/lib/ipc/client";
import { accountStore, NO_ACCOUNT } from "@/lib/stores/account";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { platformStore } from "@/lib/stores/platform";
import { updateStore } from "@/lib/stores/update";
import { voiceStore } from "@/lib/stores/voice";

const mockEgress = vi.mocked(egressList);
const mockCheck = vi.mocked(check);
const mockRelaunch = vi.mocked(relaunch);
const mockOpenUrl = vi.mocked(openUrl);
const mockGetVersion = vi.mocked(getVersion);
const mockVoiceAvailability = vi.mocked(voiceAvailability);
const mockDebugLogPath = vi.mocked(debugLogPath);
const mockAutoUpdateGet = vi.mocked(autoUpdateGet);
const mockAutoUpdateSet = vi.mocked(autoUpdateSet);
const mockLogUploadGet = vi.mocked(logUploadGet);
const mockLogUploadSet = vi.mocked(logUploadSet);
const mockAccountSendLog = vi.mocked(accountSendLog);
const mockReleaseCheck = vi.mocked(releaseCheck);

/** Signed in, with this device registered as `marta-air`: the log has a place to go. */
const SIGNED_IN = {
  ...NO_ACCOUNT,
  configured: true,
  state: "ready" as const,
  identity: { login: "marta", displayName: "Marta", email: null, roles: [] },
  device: {
    slug: "marta-air",
    name: "marta-air",
    class: null,
    platform: "macos",
    version: null,
    thisDevice: true,
  },
};

/** All seven capabilities present = the desktop tier (updater block renders). */
const DESKTOP_CAPABILITIES = {
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
  // Story 66.3: the phone's reveal; false on every desktop fixture.
  shareOut: false,
  recording: false,
  sync: false,
  notes: false,
  sessions: false,
  bots: false,
  botTools: false,
  overlayTitleBar: false,
  transcription: false,
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

/**
 * A fleet with one folder-sync profile: the account homeserver, the profile's remote
 * HOST (never its URL — the Rust `compute_egress` reduces it, Story 23.7) and the
 * update endpoint.
 */
const SYNC_EGRESS: EgressEndpointVm[] = [
  { url: "https://matrix.example.org", kind: "homeserver", label: "Matrix homeserver" },
  { url: "github.com", kind: "gitRemote", label: "Folder sync remote" },
  { url: UPDATE_ENDPOINT, kind: "update", label: "Signed app updates" },
];

/**
 * What Rust's `platform_facts` answers on an iPhone, its lines read from
 * `docs/ios.md` (which `keeper_core::disclosure::IPHONE_LINES` is pinned to by
 * `platform_disclosure_mirrors_docs.rs`) rather than restated here.
 */
const IPHONE_DISCLOSURE: PlatformDisclosureVm = {
  heading: "On this iPhone",
  lines: firstBulletRun(
    readFileSync(resolve(process.cwd(), "docs/ios.md"), "utf8"),
    "docs/ios.md",
    "## Limitations",
  ),
  docsUrl: "https://github.com/tgorka/keeper/blob/main/docs/ios.md",
  docsLabel: "What's different on iPhone",
};

beforeEach(() => {
  platformStore.getState().applySnapshot({
    urlForm: "scheme",
    tablet: false,
    disclosure: IPHONE_DISCLOSURE,
  });
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
  mockVoiceAvailability.mockReset();
  mockVoiceAvailability.mockResolvedValue({
    kind: "unsupported",
    message: "voice is not available in this build",
  });
  mockDebugLogPath.mockReset();
  mockDebugLogPath.mockResolvedValue("/Users/alice/Library/Logs/keeper/keeper.log");
  voiceStore.setState({ state: null, unavailable: undefined, wake: null });
  // The update flow lives in a module-level store now (it is shared with the
  // background loop), so a case that left a build "waiting for a restart" must
  // not hand that state to the next one.
  updateStore.getState().reset();
  mockAutoUpdateGet.mockClear();
  mockAutoUpdateSet.mockClear();
  mockLogUploadGet.mockReset();
  mockLogUploadGet.mockResolvedValue(true);
  mockLogUploadSet.mockReset();
  mockLogUploadSet.mockImplementation((enabled: boolean) => Promise.resolve(enabled));
  mockAccountSendLog.mockReset();
  accountStore.setState({ vm: NO_ACCOUNT });
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

  it("renders a folder-sync remote host beside the account destinations", async () => {
    mockEgress.mockResolvedValue(SYNC_EGRESS);
    render(<AboutSection open />);

    await waitFor(() => {
      expect(screen.getByText("github.com")).toBeInTheDocument();
    });
    expect(screen.getByText("Folder sync remote")).toBeInTheDocument();
    expect(screen.getByText("https://matrix.example.org")).toBeInTheDocument();
    expect(screen.getByText(UPDATE_ENDPOINT)).toBeInTheDocument();
  });

  it("drops the folder-sync row once that profile is gone", async () => {
    // The list is re-read on every open and never cached in the component, which is
    // what makes removing a profile remove its entry (Story 23.7 AC). A row that
    // survived the profile would be a destination disclosed for a repository keeper
    // no longer talks to.
    mockEgress.mockResolvedValue(SYNC_EGRESS);
    const withProfile = render(<AboutSection open />);
    await waitFor(() => {
      expect(screen.getByText("github.com")).toBeInTheDocument();
    });
    withProfile.unmount();

    mockEgress.mockResolvedValue(NON_BEEPER_EGRESS);
    render(<AboutSection open />);
    await waitFor(() => {
      expect(screen.getByText("https://matrix.example.org")).toBeInTheDocument();
    });
    expect(screen.queryByText("github.com")).not.toBeInTheDocument();
    expect(screen.queryByText("Folder sync remote")).not.toBeInTheDocument();
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

  it("reports a build the background loop already installed, and restarts on request", async () => {
    // What `use-auto-update` leaves behind while Settings was closed: installed,
    // named, waiting for a restart nobody has been forced into.
    updateStore
      .getState()
      .setPhase({ kind: "installedNeedsRestart", version: "0.9.0", atMs: Date.now() });
    render(<AboutSection open />);
    await waitFor(() => expect(mockEgress).toHaveBeenCalled());

    expect(
      screen.getByText("Update 0.9.0 installed. Restart keeper to finish."),
    ).toBeInTheDocument();
    // Opening About must not discard that state the way it resets a stale check.
    expect(mockRelaunch).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Restart now" }));
    await waitFor(() => expect(mockRelaunch).toHaveBeenCalledTimes(1));
  });

  it("says what the self-restart is waiting for, and names the recording case", async () => {
    const flow = updateStore.getState();
    flow.setPhase({ kind: "installedNeedsRestart", version: "0.9.0", atMs: Date.now() });
    flow.setRestartHold("recording");
    render(<AboutSection open />);
    await waitFor(() => expect(mockEgress).toHaveBeenCalled());

    // The hold that can stand for hours on a machine nobody is touching is the
    // one a person must be able to read off the surface.
    expect(screen.getByText(selfRestartNote("recording"))).toBeInTheDocument();
    expect(selfRestartNote("recording")).toContain("will not restart while a recording is running");
    expect(selfRestartNote("inUse")).toContain("once you are away from keeper");
  });

  it("persists the background-update switch and shows the effective answer a file pinned", async () => {
    render(<AboutSection open />);
    const auto = await screen.findByLabelText("Update automatically");
    // The stored answer, not a compile-time default: on.
    await waitFor(() => expect(auto).toBeChecked());

    // A layer file pins `update.auto`, so the write is echoed back unchanged and
    // the switch refuses to move rather than promising a session that never runs.
    mockAutoUpdateSet.mockResolvedValueOnce({
      supported: true,
      enabled: true,
      firstCheckDelayMs: 120_000,
      checkIntervalMs: 21_600_000,
      retryDelayMs: 1_800_000,
      restartCheckIntervalMs: 60_000,
    });
    fireEvent.click(auto);
    expect(mockAutoUpdateSet).toHaveBeenCalledWith(false);
    await waitFor(() => expect(auto).toBeChecked());

    // With nothing pinning it, the same click sticks.
    fireEvent.click(auto);
    await waitFor(() => expect(auto).not.toBeChecked());
  });

  it("omits the switch where a background install would exit the app, keeping the manual control", async () => {
    mockAutoUpdateGet.mockResolvedValueOnce({
      supported: false,
      enabled: false,
      firstCheckDelayMs: 120_000,
      checkIntervalMs: 21_600_000,
      retryDelayMs: 1_800_000,
      restartCheckIntervalMs: 60_000,
    });
    render(<AboutSection open />);
    await waitFor(() => expect(mockEgress).toHaveBeenCalled());

    // Absent, not disabled — the project's rule for a surface the platform
    // refuses. The two-click path is unaffected.
    expect(screen.queryByLabelText("Update automatically")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeInTheDocument();
  });

  it("names the cadence Rust chose in the switch's sentence", () => {
    const plan = {
      supported: true,
      enabled: true,
      firstCheckDelayMs: 120_000,
      checkIntervalMs: 21_600_000,
      retryDelayMs: 1_800_000,
      restartCheckIntervalMs: 60_000,
    };
    expect(autoUpdateSentence(plan)).toContain("about every 6 hours");
    // And the two things a person needs to predict what happens to their
    // session: that keeper will restart itself, and the one thing that always
    // stops it.
    expect(autoUpdateSentence(plan)).toContain("keeper restarts itself");
    expect(autoUpdateSentence(plan)).toContain("never while a recording is running");
    // Unanswered: no invented number.
    expect(autoUpdateSentence(undefined)).toContain("on a cadence");
  });
});

describe("AboutSection device log", () => {
  it("persists the daily-upload switch and shows the effective answer a file pinned", async () => {
    render(<AboutSection open />);
    const upload = await screen.findByLabelText("Send this device's log to your account daily");
    // The stored answer, absent meaning on.
    await waitFor(() => expect(upload).toBeChecked());

    // A layer file pins `diagnostics.log_upload`: Rust re-reads it and answers
    // on, so the switch refuses to move rather than promising no upload.
    mockLogUploadSet.mockResolvedValueOnce(true);
    fireEvent.click(upload);
    expect(mockLogUploadSet).toHaveBeenCalledWith(false);
    await waitFor(() => expect(upload).toBeChecked());

    // With nothing pinning it, the same click sticks.
    fireEvent.click(upload);
    await waitFor(() => expect(upload).not.toBeChecked());
  });

  // Two quick toggles from a confirmed on, both writes failing: the switch
  // must end on what Rust last confirmed, never on the first toggle's
  // optimistic off, and say why beside it.
  it("takes one write at a time and returns to the value Rust last confirmed", async () => {
    const rejections: Array<(reason: unknown) => void> = [];
    mockLogUploadSet.mockImplementation(
      () =>
        new Promise<boolean>((_, reject) => {
          rejections.push(reject);
        }),
    );
    render(<AboutSection open />);
    const upload = await screen.findByLabelText("Send this device's log to your account daily");
    await waitFor(() => expect(upload).toBeChecked());

    fireEvent.click(upload);
    expect(upload).not.toBeChecked();
    expect(upload).toBeDisabled();
    fireEvent.click(upload);
    expect(mockLogUploadSet).toHaveBeenCalledTimes(1);

    await act(async () => {
      for (const reject of rejections) reject(new Error("database is locked"));
    });
    await waitFor(() => expect(upload).toBeChecked());
    expect(screen.getByText("The switch was not saved: database is locked")).toHaveClass(
      "text-destructive",
    );
    expect(upload).toBeEnabled();
  });

  it("cannot send without an account to send to, and says why", async () => {
    render(<AboutSection open />);
    const button = await screen.findByRole("button", { name: "Send log now" });
    expect(button).toBeDisabled();
    expect(screen.getByText(/Sign in to an organisation account/)).toBeInTheDocument();
  });

  it("shows Rust's sentence under the button once the log is sent", async () => {
    accountStore.setState({ vm: SIGNED_IN });
    const sentence =
      "Sent. This device's log is in the settings repository at marta/logs/marta-air.log.";
    mockAccountSendLog.mockResolvedValueOnce({ outcome: "sent", sentence });
    render(<AboutSection open />);
    const button = await screen.findByRole("button", { name: "Send log now" });
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(await screen.findByText(sentence)).toHaveAttribute("role", "status");
  });

  it("shows a command failure inline, not as a sentence of success", async () => {
    accountStore.setState({ vm: SIGNED_IN });
    mockAccountSendLog.mockRejectedValueOnce({ code: "internal", message: "keeper stopped" });
    render(<AboutSection open />);
    fireEvent.click(await screen.findByRole("button", { name: "Send log now" }));
    expect(await screen.findByText("keeper stopped")).toHaveClass("text-destructive");
    // The button is usable again for another try.
    await waitFor(() => expect(screen.getByRole("button", { name: "Send log now" })).toBeEnabled());
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

  it("iOS: checks for a newer release on a press and says Rust's answer, never the desktop updater", async () => {
    mockEgress.mockResolvedValue(NON_BEEPER_EGRESS);
    mockReleaseCheck.mockResolvedValue({
      outcome: "newer",
      installed: "0.10.5",
      latest: "0.11.0",
      sentence: "keeper 0.11.0 is available — install it from your Mac with `bun run install:ios`.",
    });
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<AboutSection open />);

    // The egress list stays ungated…
    await waitFor(() => {
      expect(screen.getByText("https://matrix.example.org")).toBeInTheDocument();
    });
    // …and nothing is asked before the press (R464: check on demand only).
    expect(mockReleaseCheck).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    expect(
      await screen.findByText(
        "keeper 0.11.0 is available — install it from your Mac with `bun run install:ios`.",
      ),
    ).toBeInTheDocument();
    expect(mockReleaseCheck).toHaveBeenCalledTimes(1);
    // The desktop updater is never touched on the phone, and nothing offers
    // a download or a background install.
    expect(mockCheck).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Download and install" })).not.toBeInTheDocument();
    expect(screen.queryByText("Update automatically")).not.toBeInTheDocument();
    // The list Rust composed for this device renders under its heading, every
    // line in order (story 98.3 #4: the front decides none of it).
    expect(screen.getByText("On this iPhone")).toBeInTheDocument();
    expect(screen.getAllByRole("listitem").map((item) => item.textContent)).toEqual(
      expect.arrayContaining(IPHONE_DISCLOSURE.lines),
    );
  });

  it("iOS: a check Rust could not finish says why, and a command that failed says so", async () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    mockReleaseCheck.mockResolvedValueOnce({
      outcome: "failed",
      installed: "0.10.5",
      latest: null,
      sentence:
        "keeper could not check GitHub for a newer version (it answered 403). Try again later.",
    });
    render(<AboutSection open />);

    fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
    expect(
      await screen.findByText(
        "keeper could not check GitHub for a newer version (it answered 403). Try again later.",
      ),
    ).toHaveClass("text-destructive");

    mockReleaseCheck.mockRejectedValueOnce({ code: "internal", message: "gone" });
    fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(RELEASE_CHECK_FAILED);
    expect(
      screen.queryByText(
        "keeper could not check GitHub for a newer version (it answered 403). Try again later.",
      ),
    ).not.toBeInTheDocument();
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

  it("renders the Debug mode toggle off by default and persists a flip (Story 22.5)", async () => {
    const { debugModeGet, debugModeSet } = await import("@/lib/ipc/client");
    render(<AboutSection open />);
    await waitFor(() => expect(vi.mocked(debugModeGet)).toHaveBeenCalled());
    const toggle = await screen.findByLabelText("Debug mode");
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() => expect(vi.mocked(debugModeSet)).toHaveBeenCalledWith(true));
    expect(toggle).toBeChecked();
  });

  // ── The debug sentence names this device's log (Story 65.3, AD-192) ───────
  it("names the log path Rust answers — the phone's own container, not the Mac's folder", async () => {
    const PHONE_LOG =
      "/private/var/mobile/Containers/Data/Application/1F2E/Library/Logs/keeper/keeper.log";
    mockDebugLogPath.mockResolvedValue(PHONE_LOG);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<AboutSection open />);

    expect(await screen.findByText(debugModeSentence(PHONE_LOG))).toBeInTheDocument();
    // The literal the sentence used to carry is gone: nothing on the phone
    // names `~/Library/Logs`.
    expect(screen.queryByText(/~\/Library\/Logs/)).not.toBeInTheDocument();
  });

  it("names the Mac's log path on the Mac, from the same answer", async () => {
    mockDebugLogPath.mockResolvedValue("/Users/alice/Library/Logs/keeper/keeper.log");
    render(<AboutSection open />);

    expect(
      await screen.findByText(
        /Writes app logs to \/Users\/alice\/Library\/Logs\/keeper\/keeper\.log/,
      ),
    ).toBeInTheDocument();
  });

  it("names the file without a folder while the path is unanswered, never a guessed folder", async () => {
    mockDebugLogPath.mockRejectedValue(new Error("not answered"));
    render(<AboutSection open />);

    expect(await screen.findByText(debugModeSentence(null))).toBeInTheDocument();
    expect(screen.queryByText(/Library\/Logs/)).not.toBeInTheDocument();
  });

  // ── "On this Mac" (Story 63.8, AD-175, AD-179) ────────────────────────────
  it("desktop with a voice port: shows the 'On this Mac' voice disclosure, every line", async () => {
    mockVoiceAvailability.mockResolvedValue(null);
    render(<AboutSection open />);

    expect(await screen.findByText("On this Mac")).toBeInTheDocument();
    for (const line of MACOS_DISCLOSURE_LINES) {
      expect(screen.getByText(line)).toBeInTheDocument();
    }
    // The disclosure names what the Mac cannot do, not only what it does.
    expect(screen.getByText(/does not lower other audio/)).toBeInTheDocument();
    expect(screen.queryByText("On this iPhone")).not.toBeInTheDocument();
  });

  it("desktop with a refusal that is not 'unsupported' (not authorised): the disclosure still stands", async () => {
    // A port exists and is refusing for a reason the person can fix; what
    // keeper will and will not do once it works is the same sentence.
    mockVoiceAvailability.mockResolvedValue({
      kind: "notAuthorized",
      message: "allow the microphone under System Settings",
    });
    render(<AboutSection open />);

    expect(await screen.findByText("On this Mac")).toBeInTheDocument();
  });

  it("desktop without a voice port: no 'On this Mac' block (absent, not disabled)", async () => {
    // beforeEach already answered `unsupported` — every build without a port.
    mockEgress.mockResolvedValue(NON_BEEPER_EGRESS);
    render(<AboutSection open />);

    await waitFor(() => expect(mockVoiceAvailability).toHaveBeenCalled());
    await waitFor(() => expect(voiceStore.getState().unavailable?.kind).toBe("unsupported"));
    expect(await screen.findByText("https://matrix.example.org")).toBeInTheDocument();
    expect(screen.queryByText("On this Mac")).not.toBeInTheDocument();
  });

  it("does not draw the 'On this Mac' block before voice_availability has answered", async () => {
    mockVoiceAvailability.mockReturnValue(new Promise<null>(() => {}));
    mockEgress.mockResolvedValue(NON_BEEPER_EGRESS);
    render(<AboutSection open />);

    expect(await screen.findByText("https://matrix.example.org")).toBeInTheDocument();
    expect(screen.queryByText("On this Mac")).not.toBeInTheDocument();
  });

  it("iOS with a voice port: the phone's disclosure, never the Mac's", async () => {
    mockVoiceAvailability.mockResolvedValue(null);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<AboutSection open />);

    expect(await screen.findByText("On this iPhone")).toBeInTheDocument();
    await waitFor(() => expect(voiceStore.getState().unavailable).toBeNull());
    expect(screen.queryByText("On this Mac")).not.toBeInTheDocument();
  });
});

/**
 * The first bullet run under the heading `headingLine` in `doc`: the list stops
 * at the first line that is not a bullet after the list has begun, so the prose
 * below it is not mistaken for one more item. Throws when the heading is absent,
 * so a renamed section fails the guard rather than passing it vacuously.
 */
function firstBulletRun(doc: string, docPath: string, headingLine: string): string[] {
  const start = doc.indexOf(`${headingLine}\n`);
  if (start < 0) {
    throw new Error(`${docPath} has no '${headingLine}' section`);
  }
  const lines = doc.slice(start + headingLine.length + 1).split("\n");
  const first = lines.findIndex((line) => line.startsWith("- "));
  expect(first).toBeGreaterThanOrEqual(0);
  const bullets: string[] = [];
  for (const line of lines.slice(first)) {
    if (!line.startsWith("- ")) {
      break;
    }
    bullets.push(line.slice(2));
  }
  return bullets;
}

/**
 * The Mac's lines are the egress claim in the person's own words, so they are
 * mirrored into `docs/egress.md` — the canonical record every release diffs —
 * under the same discipline as the phone's (Story 63.8, AD-178).
 */
describe("MACOS_DISCLOSURE_LINES mirrors docs/egress.md (Story 63.8)", () => {
  it("is identical to the 'On this Mac' bullet list, in order", () => {
    const doc = readFileSync(resolve(process.cwd(), "docs/egress.md"), "utf8");
    expect(firstBulletRun(doc, "docs/egress.md", "### On this Mac")).toEqual([
      ...MACOS_DISCLOSURE_LINES,
    ]);
  });

  it("says what the Mac cannot do, not only what it does", () => {
    // AD-175: no `AVAudioSession` on macOS means no ducking, and the person is
    // told so where they read what this build does with their voice.
    expect(MACOS_DISCLOSURE_LINES.some((line) => /does not lower other audio/.test(line))).toBe(
      true,
    );
    expect(MACOS_DISCLOSURE_LINES.some((line) => /never sent to a server/.test(line))).toBe(true);
  });
});
