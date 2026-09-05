import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  egressList: vi.fn(() => Promise.resolve([])),
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
  debugModeSentence,
  IOS_DISCLOSURE_LINES,
  MACOS_DISCLOSURE_LINES,
} from "@/components/settings/about-section";
import {
  debugLogPath,
  type EgressEndpointVm,
  egressList,
  voiceAvailability,
} from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { voiceStore } from "@/lib/stores/voice";

const mockEgress = vi.mocked(egressList);
const mockCheck = vi.mocked(check);
const mockRelaunch = vi.mocked(relaunch);
const mockOpenUrl = vi.mocked(openUrl);
const mockGetVersion = vi.mocked(getVersion);
const mockVoiceAvailability = vi.mocked(voiceAvailability);
const mockDebugLogPath = vi.mocked(debugLogPath);

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
  mockVoiceAvailability.mockReset();
  mockVoiceAvailability.mockResolvedValue({
    kind: "unsupported",
    message: "voice is not available in this build",
  });
  mockDebugLogPath.mockReset();
  mockDebugLogPath.mockResolvedValue("/Users/alice/Library/Logs/keeper/keeper.log");
  voiceStore.setState({ state: null, unavailable: undefined, wake: null });
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

  it("says the list is computed from accounts and folder-sync profiles, hosts only", async () => {
    // The disclosure copy has to name both inputs: a bare `github.com` row under a
    // sentence that only mentions accounts reads as a fabricated entry, and the
    // host-only promise is the reason no repository path or token is on screen.
    mockEgress.mockResolvedValue(SYNC_EGRESS);
    render(<AboutSection open />);

    const sentence = await screen.findByText(/These are the servers keeper connects to/);
    expect(sentence).toHaveTextContent("folder-sync profiles");
    expect(sentence).toHaveTextContent("host only");
    expect(sentence).toHaveTextContent("no telemetry, analytics, or crash reporting");
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
    // The "On this iPhone" list renders every honesty line.
    expect(screen.getByText("On this iPhone")).toBeInTheDocument();
    expect(
      screen.getByText(/syncs and notifies about messages only while it's open/),
    ).toBeInTheDocument();
    expect(screen.getByText(/mirrors a remote your Mac already syncs/)).toBeInTheDocument();
    expect(screen.getByText(/Nothing is merged on a phone/)).toBeInTheDocument();
    expect(screen.getByText(/the self-hosted bridge runner/)).toBeInTheDocument();
    expect(screen.getByText(/the global summon hotkey/)).toBeInTheDocument();
    expect(screen.getByText(/signature renews every 7 days/)).toBeInTheDocument();
    expect(screen.getByText(/the drive tools live on your Mac/)).toBeInTheDocument();
    for (const line of IOS_DISCLOSURE_LINES) {
      expect(screen.getByText(line)).toBeInTheDocument();
    }
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
 * `docs/ios.md` says of its Limitations list: "mirrored from `IOS_DISCLOSURE_LINES`
 * … the two must stay identical. Edit both together or neither." A sentence in a
 * doc is a request; this is the guard. The list is read from disk, never
 * restated here, so a third copy cannot drift either.
 */
describe("IOS_DISCLOSURE_LINES mirrors docs/ios.md (Story 62.3, FR-400)", () => {
  it("is identical to the Limitations bullet list, in order", () => {
    const doc = readFileSync(resolve(process.cwd(), "docs/ios.md"), "utf8");
    expect(firstBulletRun(doc, "docs/ios.md", "## Limitations")).toEqual([...IOS_DISCLOSURE_LINES]);
  });

  it("names, in the phone's own disclosure, what Bots does not do there", () => {
    // FR-400: the drive tools are on the Mac, and the phone says so where the
    // person reads what this build lacks — not only in a doc.
    expect(IOS_DISCLOSURE_LINES.some((line) => /drive tools live on your Mac/.test(line))).toBe(
      true,
    );
  });
});

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
