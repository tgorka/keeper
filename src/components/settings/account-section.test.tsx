import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  accountState: vi.fn(),
  accountSignIn: vi.fn(),
  accountCancelSignIn: vi.fn(),
  accountSync: vi.fn(),
  accountRenameDevice: vi.fn(),
  accountSignOut: vi.fn(),
  accountForget: vi.fn(),
  accountShare: vi.fn(),
  accountSetupResolve: vi.fn(),
  accountSetupConfirm: vi.fn(),
  voiceWakeGet: vi.fn(),
  voiceWakeToggle: vi.fn(),
  voiceWakeSet: vi.fn(),
  voiceAvailability: vi.fn(),
}));

import { SETUP_LINK_LABEL } from "@/components/account/account-setup-sheet";
import {
  ACCOUNT_FORGET_LABEL,
  ACCOUNT_LISTENING_KEEP_OFF_LABEL,
  ACCOUNT_LISTENING_OFF_SENTENCE,
  ACCOUNT_LISTENING_ON_LABEL,
  ACCOUNT_SECTION_NOTE,
  ACCOUNT_SECTION_TITLE,
  ACCOUNT_SIGN_OUT_LABEL,
  AccountSection,
  accountSyncLine,
} from "@/components/settings/account-section";
import type { AccountRestoreVm, OrgAccountVm, VoiceWakeVm } from "@/lib/ipc/client";
import {
  accountForget,
  accountRenameDevice,
  accountSignIn,
  accountSignOut,
  accountState,
  accountSync,
  voiceAvailability,
  voiceWakeGet,
  voiceWakeSet,
  voiceWakeToggle,
} from "@/lib/ipc/client";
import { accountStore, NO_ACCOUNT } from "@/lib/stores/account";
import { voiceStore } from "@/lib/stores/voice";
import { accountVm, driveOffer, matrixOffer, providerOffer } from "@/test/account-fixture";

const mockState = vi.mocked(accountState);

beforeEach(() => {
  accountStore.setState({ vm: NO_ACCOUNT, setupLink: null });
  mockState.mockReturnValue(new Promise(() => {}));
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("AccountSection with no account", () => {
  it("is the heading, one sentence and the paste field — nothing else", () => {
    render(<AccountSection open />);

    const section = screen.getByRole("region", { name: ACCOUNT_SECTION_TITLE });
    expect(within(section).getByText(ACCOUNT_SECTION_NOTE)).toBeInTheDocument();
    expect(within(section).getByLabelText(SETUP_LINK_LABEL)).toBeInTheDocument();
    // The one control is the paste field's own submit. No Sign in with nothing
    // to sign in to, no Sync now, no Sign out, no device list.
    expect(
      within(section)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Continue"]);
    // …and nothing in it is disabled: an empty field's Continue is a no-op,
    // not a greyed-out button (UX-DR116 (1)).
    expect(section.querySelectorAll("[disabled]")).toHaveLength(0);
    expect(within(section).queryByRole("list")).not.toBeInTheDocument();
    expect(within(section).queryByRole("status")).not.toBeInTheDocument();
  });

  it("does nothing on Continue with an empty field", () => {
    render(<AccountSection open />);
    fireEvent.change(screen.getByLabelText(SETUP_LINK_LABEL), { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    expect(accountStore.getState().setupLink).toBeNull();
  });

  it("hands a pasted link to the one setup sheet", () => {
    render(<AccountSection open />);
    fireEvent.change(screen.getByLabelText(SETUP_LINK_LABEL), {
      target: { value: "  keeper://setup?d=eyJ9  " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    expect(accountStore.getState().setupLink).toBe("keeper://setup?d=eyJ9");
  });

  it("re-reads the account whenever Settings opens, so a hand-edited account.toml shows", async () => {
    mockState.mockResolvedValue(accountVm());
    render(<AccountSection open />);

    expect(await screen.findByText("Tomasz Gorka")).toBeInTheDocument();
    expect(screen.queryByLabelText(SETUP_LINK_LABEL)).not.toBeInTheDocument();
  });
});

describe("AccountSection signed in", () => {
  beforeEach(() => {
    accountStore.getState().setVm(accountVm());
  });

  it("renders Rust's sentence verbatim and the roles as chips", () => {
    accountStore.getState().setVm(accountVm({ sentence: "Offline — using settings from 14:02." }));
    render(<AccountSection open />);

    expect(screen.getByRole("status")).toHaveTextContent("Offline — using settings from 14:02.");
    const roles = screen.getByRole("list", { name: "Roles" });
    expect(within(roles).getByText("keeper")).toBeInTheDocument();
    expect(within(roles).getByText("staff")).toBeInTheDocument();
  });

  it("says in the sign-out dialog that the repository's files are kept", async () => {
    vi.mocked(accountSignOut).mockResolvedValue(accountVm({ state: "signedOut", identity: null }));
    render(<AccountSection open />);

    fireEvent.click(screen.getByRole("button", { name: ACCOUNT_SIGN_OUT_LABEL }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("The files in the repository are kept");
    // Signing out is not forgetting: the account stays, and so does the way back in.
    expect(dialog).toHaveTextContent(
      "The account stays set up, so you can sign in again without a link.",
    );
    expect(dialog).not.toHaveTextContent("forgets");
    expect(accountSignOut).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "Sign out" }));
    await waitFor(() => expect(accountSignOut).toHaveBeenCalledTimes(1));
    expect(accountForget).not.toHaveBeenCalled();
  });

  it("says in the forget dialog that the server repository is untouched", async () => {
    vi.mocked(accountForget).mockResolvedValue(NO_ACCOUNT);
    render(<AccountSection open />);

    fireEvent.click(screen.getByRole("button", { name: ACCOUNT_FORGET_LABEL }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("The repository on the server is untouched");

    fireEvent.click(within(dialog).getByRole("button", { name: "Forget account" }));
    await waitFor(() => expect(accountForget).toHaveBeenCalledTimes(1));
    // Forgetting lands back on the paste field.
    expect(await screen.findByLabelText(SETUP_LINK_LABEL)).toBeInTheDocument();
    expect(accountSignOut).not.toHaveBeenCalled();
  });

  it("offers Forget but not Sign out once signed out, and Sign in in their place", () => {
    accountStore.getState().setVm(accountVm({ state: "signedOut", identity: null, devices: [] }));
    render(<AccountSection open />);

    expect(screen.getByRole("button", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: ACCOUNT_FORGET_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: ACCOUNT_SIGN_OUT_LABEL })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Sync now" })).not.toBeInTheDocument();
  });

  it("offers Sign in after a sign-in the policy refused, so a fixed role is one click away", async () => {
    vi.mocked(accountSignIn).mockResolvedValue(accountVm());
    accountStore.getState().setVm(
      accountVm({
        state: "blocked",
        identity: null,
        device: null,
        devices: [],
        sentence: "Your Acme account does not have the keeper role. Ask your administrator.",
      }),
    );
    render(<AccountSection open />);

    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(accountSignIn).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("Tomasz Gorka")).toBeInTheDocument();
  });

  it("offers an enabled Sync now while offline, not yet synced, or asked to sign in again", async () => {
    vi.mocked(accountSync).mockResolvedValue(accountVm());
    const states: Array<Partial<OrgAccountVm>> = [
      { state: "offline", sentence: "Offline — using settings from 14:02." },
      { state: "syncing", sentence: "Your settings have not been synced yet." },
      { state: "needsSignIn", sentence: "Sign in again to keep your settings in sync." },
    ];
    for (const over of states) {
      accountStore.getState().setVm(accountVm(over));
      const { unmount } = render(<AccountSection open />);
      const syncNow = screen.getByRole("button", { name: "Sync now" });
      expect(syncNow).toBeEnabled();
      fireEvent.click(syncNow);
      await waitFor(() => expect(accountSync).toHaveBeenLastCalledWith(true));
      unmount();
    }
    expect(accountSync).toHaveBeenCalledTimes(states.length);
  });

  it("keeps the Forget dialog's words while it animates closed", async () => {
    // jsdom runs no CSS, so Radix unmounts a closing dialog at once. Give the
    // content an exit animation, as the real stylesheet does, so it stays
    // mounted for it and what it says during that time can be read.
    const realStyle = window.getComputedStyle.bind(window);
    const styleSpy = vi.spyOn(window, "getComputedStyle").mockImplementation((el, pseudo) => {
      const style = realStyle(el, pseudo);
      if (el instanceof HTMLElement && el.getAttribute("role") === "alertdialog") {
        Object.defineProperty(style, "animationName", {
          // Live, like a real computed style: Radix keeps the object from mount.
          get: () => (el.dataset.state === "closed" ? "exit" : "enter"),
          configurable: true,
        });
      }
      return style;
    });
    try {
      render(<AccountSection open />);
      fireEvent.click(screen.getByRole("button", { name: ACCOUNT_FORGET_LABEL }));
      const dialog = await screen.findByRole("alertdialog");
      fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

      await waitFor(() => expect(dialog).toHaveAttribute("data-state", "closed"));
      expect(dialog).toBeInTheDocument();
      expect(dialog).toHaveTextContent("Forget Acme?");
      expect(within(dialog).getByRole("button", { name: "Forget account" })).toBeInTheDocument();
      expect(dialog).not.toHaveTextContent("Sign out of Acme?");
    } finally {
      styleSpy.mockRestore();
    }
  });

  it("renames inline with a disclosure that says it is open and hands focus back", async () => {
    vi.mocked(accountRenameDevice).mockResolvedValue(accountVm());
    render(<AccountSection open />);
    const toggle = screen.getByRole("button", { name: "Rename" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const field = screen.getByLabelText("Rename hesperia");
    const controlled = toggle.getAttribute("aria-controls");
    expect(controlled).not.toBeNull();
    expect(field.closest("form")).toHaveAttribute("id", controlled ?? "");
    await waitFor(() => expect(field).toHaveFocus());

    // Cancel returns focus to the toggle rather than dropping it on <body>.
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByLabelText("Rename hesperia")).not.toBeInTheDocument();
    expect(toggle).toHaveFocus();
    expect(toggle).toHaveAttribute("aria-expanded", "false");

    // Save sends the new name and does the same.
    fireEvent.click(toggle);
    fireEvent.change(screen.getByLabelText("Rename hesperia"), { target: { value: "studio" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(accountRenameDevice).toHaveBeenCalledWith("studio"));
    expect(toggle).toHaveFocus();
  });

  it("keeps a newer pushed snapshot over an older answer to the open-time read", async () => {
    // Settings opened mid-sign-in: the read was composed while signing in, the
    // subscription has since pushed the sync that followed.
    accountStore
      .getState()
      .setVm(accountVm({ state: "syncing", sentence: "Syncing your settings…", revision: 9 }));
    mockState.mockResolvedValue(
      accountVm({
        state: "signingIn",
        identity: null,
        sentence: "Finish signing in to Acme in the browser.",
        revision: 8,
      }),
    );
    render(<AccountSection open />);
    await waitFor(() => expect(mockState).toHaveBeenCalled());
    await Promise.resolve();

    expect(screen.getByRole("status")).toHaveTextContent("Syncing your settings…");
    expect(screen.queryByRole("button", { name: "Cancel sign-in" })).not.toBeInTheDocument();
  });
});

describe("AccountSection's settings line (Epic 84, UX-DR118)", () => {
  const SYNCED = "Your settings sync with this account.";

  it("says settings sync here, with no time of its own — the status already says when", () => {
    accountStore.getState().setVm(accountVm({ lastSyncedMs: Date.UTC(2026, 8, 24, 12, 0) }));
    render(<AccountSection open />);
    expect(screen.getByText(SYNCED)).toBeInTheDocument();
    expect(screen.queryByText(/Last synced/)).not.toBeInTheDocument();
    expect(screen.queryByText(/On your other devices/)).not.toBeInTheDocument();
  });

  it("stays while offline, where the settings last synced are still the ones in use", () => {
    accountStore
      .getState()
      .setVm(accountVm({ state: "offline", sentence: "Offline — using settings from 14:02." }));
    render(<AccountSection open />);
    expect(screen.getByText(SYNCED)).toBeInTheDocument();
  });

  it("is absent wherever settings do not travel, though an identity is still held", () => {
    for (const state of ["blocked", "needsSignIn", "signedOut"] as const) {
      accountStore.getState().setVm(accountVm({ state, revision: 0 }));
      const view = render(<AccountSection open />);
      expect(screen.queryByText(new RegExp(SYNCED))).not.toBeInTheDocument();
      view.unmount();
    }
  });

  it("counts what the other devices use, leaving out zeros and saying one as one", () => {
    const offers = (drives: number, providers: number, matrix: number) =>
      accountVm({
        offers: {
          drives: Array.from({ length: drives }, (_, i) => driveOffer({ key: `d${i}` })),
          providers: Array.from({ length: providers }, (_, i) => providerOffer({ key: `p${i}` })),
          matrix: Array.from({ length: matrix }, (_, i) => matrixOffer({ key: `m${i}` })),
        },
      });
    expect(accountSyncLine(offers(2, 1, 0))).toBe(
      `${SYNCED} On your other devices: 2 drives, 1 bot provider.`,
    );
    expect(accountSyncLine(offers(1, 2, 3))).toBe(
      `${SYNCED} On your other devices: 1 drive, 2 bot providers, 3 Matrix accounts.`,
    );
    expect(accountSyncLine(offers(0, 0, 1))).toBe(
      `${SYNCED} On your other devices: 1 Matrix account.`,
    );
  });
});

describe("AccountSection after a restore (Epic 85, UX-DR119)", () => {
  const RESTORED = "Restored 2 drives, 1 bot provider and your settings from your account.";
  const WAITING = "Waiting for /Volumes/Field to restore Field recordings.";
  const WAKE_OFF: VoiceWakeVm = {
    enabled: false,
    phrase: "hey nixie",
    limits: "limits",
    locale: "en-US",
    localeChosen: null,
    onDeviceLocales: ["en-US"],
    stopPhrase: "stop",
    voiceTarget: null,
  };
  const WAKE_ON: VoiceWakeVm = { ...WAKE_OFF, enabled: true };

  function restored(restore: Partial<AccountRestoreVm>, revision = 0): OrgAccountVm {
    return accountVm({
      revision,
      restore: { sentence: null, pending: [], listeningOff: false, ...restore },
    });
  }

  beforeEach(() => {
    vi.mocked(voiceAvailability).mockResolvedValue(null);
    voiceStore.setState({ wake: null });
  });

  it("says Rust's restore sentence and each pending one under the status, verbatim", () => {
    const second = "Waiting for /Volumes/Archive to restore archive.";
    accountStore.getState().setVm(restored({ sentence: RESTORED, pending: [WAITING, second] }));
    render(<AccountSection open />);

    const status = screen.getByRole("status");
    const lines = [RESTORED, WAITING, second].map((text) => screen.getByText(text));
    // Under the status, in Rust's order.
    for (const [before, after] of [
      [status, lines[0]],
      [lines[0], lines[1]],
      [lines[1], lines[2]],
    ] as const) {
      expect(before.compareDocumentPosition(after) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
    expect(screen.queryByText(ACCOUNT_LISTENING_OFF_SENTENCE)).not.toBeInTheDocument();
  });

  it("says nothing of a restore when there was none", () => {
    accountStore.getState().setVm(restored({}));
    render(<AccountSection open />);
    expect(screen.queryByText(/^Restored /)).not.toBeInTheDocument();
    expect(screen.queryByText(/^Waiting for /)).not.toBeInTheDocument();
    expect(screen.queryByText(ACCOUNT_LISTENING_OFF_SENTENCE)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: ACCOUNT_LISTENING_ON_LABEL }),
    ).not.toBeInTheDocument();
  });

  it("offers listening but never arms the microphone until the person taps", async () => {
    accountStore.getState().setVm(restored({ listeningOff: true }));
    render(<AccountSection open />);

    expect(screen.getByText(ACCOUNT_LISTENING_OFF_SENTENCE)).toBeInTheDocument();
    const turnOn = screen.getByRole("button", { name: ACCOUNT_LISTENING_ON_LABEL });
    // Rendering, and Settings' open-time read, touch no voice command.
    await waitFor(() => expect(accountState).toHaveBeenCalled());
    await Promise.resolve();
    expect(voiceWakeToggle).not.toHaveBeenCalled();
    expect(voiceWakeSet).not.toHaveBeenCalled();
    expect(voiceWakeGet).not.toHaveBeenCalled();

    vi.mocked(voiceWakeGet).mockResolvedValue(WAKE_OFF);
    vi.mocked(voiceWakeToggle).mockResolvedValue(WAKE_ON);
    vi.mocked(accountState).mockResolvedValue(restored({ listeningOff: false }, 1));
    fireEvent.click(turnOn);

    await waitFor(() => expect(voiceWakeToggle).toHaveBeenCalledTimes(1));
    expect(voiceWakeSet).not.toHaveBeenCalled();
    // What Rust stored is mirrored, and the account read again takes the offer away.
    await waitFor(() => expect(voiceStore.getState().wake).toEqual(WAKE_ON));
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: ACCOUNT_LISTENING_ON_LABEL }),
      ).not.toBeInTheDocument(),
    );
  });

  it("never flips listening off from an offer older than the switch", async () => {
    accountStore.getState().setVm(restored({ listeningOff: true }));
    vi.mocked(voiceWakeGet).mockResolvedValue(WAKE_ON);
    render(<AccountSection open />);
    vi.mocked(accountState).mockResolvedValue(restored({ listeningOff: false }, 1));

    fireEvent.click(screen.getByRole("button", { name: ACCOUNT_LISTENING_ON_LABEL }));
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: ACCOUNT_LISTENING_ON_LABEL }),
      ).not.toBeInTheDocument(),
    );
    expect(voiceWakeToggle).not.toHaveBeenCalled();
    expect(voiceStore.getState().wake).toEqual(WAKE_ON);
  });

  it("says Rust's sentence when listening cannot be turned on, and keeps the offer", async () => {
    accountStore.getState().setVm(restored({ listeningOff: true }));
    vi.mocked(voiceWakeGet).mockResolvedValue(WAKE_OFF);
    vi.mocked(voiceWakeToggle).mockRejectedValue({
      code: "internal",
      message: "the settings table is read-only",
    });
    render(<AccountSection open />);

    fireEvent.click(screen.getByRole("button", { name: ACCOUNT_LISTENING_ON_LABEL }));
    expect(await screen.findByRole("alert")).toHaveTextContent("the settings table is read-only");
    expect(screen.getByRole("button", { name: ACCOUNT_LISTENING_ON_LABEL })).toBeEnabled();
  });

  it("keeps listening off as the person's choice, never arming it, and the offer goes", async () => {
    accountStore.getState().setVm(restored({ listeningOff: true }));
    vi.mocked(voiceWakeGet).mockResolvedValue(WAKE_OFF);
    vi.mocked(voiceWakeSet).mockResolvedValue(WAKE_OFF);
    render(<AccountSection open />);
    vi.mocked(accountState).mockResolvedValue(restored({ listeningOff: false }, 1));

    fireEvent.click(screen.getByRole("button", { name: ACCOUNT_LISTENING_KEEP_OFF_LABEL }));

    // Written as a choice — off, with the phrases Rust holds — so it travels.
    await waitFor(() =>
      expect(voiceWakeSet).toHaveBeenCalledExactlyOnceWith(false, "hey nixie", "stop"),
    );
    expect(voiceWakeToggle).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByText(ACCOUNT_LISTENING_OFF_SENTENCE)).toBeNull());
    expect(screen.queryByRole("button", { name: ACCOUNT_LISTENING_KEEP_OFF_LABEL })).toBeNull();
    expect(screen.queryByRole("button", { name: ACCOUNT_LISTENING_ON_LABEL })).toBeNull();
    expect(voiceStore.getState().wake?.enabled).toBe(false);
  });
});
