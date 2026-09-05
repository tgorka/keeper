import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// The account footer renders `useSignOut`, which imports the IPC client; mock the
// hook so mounting the sidebar never reaches Tauri.
vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => vi.fn(),
}));

// The Settings dialog loads the encryption posture on open; stub just that
// wrapper so mounting the sidebar never reaches Tauri.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    encryptionPosture: vi.fn(() => Promise.resolve(false)),
  };
});

import { FOLD_STRIP } from "@/components/layout/fold-strip";
import { SidebarPane, sidebarViews } from "@/components/layout/sidebar-pane";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { BridgeHealth } from "@/lib/ipc/client";
import { phoneRoutesView } from "@/lib/phone-surfaces";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";
import { bridgeHealthStore } from "@/lib/stores/bridge-health";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { draftsStore } from "@/lib/stores/drafts";
import { networksStore } from "@/lib/stores/networks";
import { primaryViewStore } from "@/lib/stores/primary-view";
import {
  readSidebarFold,
  resetSidebarFoldForTest,
  sidebarFoldStore,
} from "@/lib/stores/sidebar-fold";
import { spacesStore } from "@/lib/stores/spaces";

const OFFLINE_TEXT = "Offline — showing your local archive. Messages queue until you're back.";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

const other: AccountVm = {
  accountId: "01BX5ZZKBKACTAV9WEVGEMMVRZ",
  userId: "@bob:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 1,
  provider: "password",
};

/**
 * A desktop that can sync. `sync` alone no longer tells the tiers apart (Epic
 * 66: a phone can sync too), so a desktop fixture must carry something the
 * phone's OS refuses — here the native menu bar — or it reads as a phone and
 * the drawer filter (Story 66.1) drops the rows the phone has no surface for.
 */
const DESKTOP_WITH_SYNC = { ...DEFAULT_CAPABILITIES, nativeMenuBar: true, sync: true };

function renderSidebar(collapsed = false, onToggleFold: (() => void) | null = () => {}) {
  return render(
    <TooltipProvider>
      <SidebarPane collapsed={collapsed} onToggleFold={onToggleFold} />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  accountStatusStore.getState().reset();
  accountsStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
  bridgeHealthStore.getState().reset();
  draftsStore.getState().clear();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  resetSidebarFoldForTest();
});

afterEach(() => {
  accountStatusStore.getState().reset();
  accountsStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
  bridgeHealthStore.getState().reset();
  draftsStore.getState().clear();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  resetSidebarFoldForTest();
  spacesStore.getState().clear();
  networksStore.getState().clear();
});

/** Seed one session's live health into the store. */
function seedSession(networkId: string, health: BridgeHealth) {
  const current = bridgeHealthStore.getState().sessions;
  bridgeHealthStore.getState().applySnapshot({
    sessions: [
      ...Object.values(current),
      {
        accountId: account.accountId,
        networkId,
        networkName: networkId,
        health,
        lastCheckedMs: 1,
        detail: null,
      },
    ],
  });
}

describe("SidebarPane offline pill", () => {
  it("hides the pill while online (the default)", () => {
    accountsStore.getState().addAccount(account);
    accountStatusStore.getState().setStatus(account.accountId, "online");
    renderSidebar();
    expect(screen.queryByText(OFFLINE_TEXT)).not.toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("hides the pill while an account is pending (no false flash)", () => {
    accountsStore.getState().addAccount(account);
    // No status batch yet → pending, must not show the pill.
    renderSidebar();
    expect(screen.queryByText(OFFLINE_TEXT)).not.toBeInTheDocument();
  });

  it("shows the persistent pill with the exact text when every account is offline", () => {
    accountsStore.getState().addAccount(account);
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    renderSidebar();
    const pill = screen.getByRole("status");
    expect(pill).toBeInTheDocument();
    expect(screen.getByText(OFFLINE_TEXT)).toBeInTheDocument();
    // Amber `held` tokens.
    expect(pill).toHaveClass("text-held");
    // Rendered in the footer region (the wrapper carries `mt-auto`; the pill
    // itself keeps the `border-t` divider).
    expect(pill).toHaveClass("border-t");
    expect(pill.parentElement).toHaveClass("mt-auto");
  });

  it("hides the pill when one account is offline and another is online (mixed)", () => {
    accountsStore.getState().hydrateAll([account, other]);
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    accountStatusStore.getState().setStatus(other.accountId, "online");
    renderSidebar();
    expect(screen.queryByText(OFFLINE_TEXT)).not.toBeInTheDocument();
  });

  it("hides again when connectivity returns", () => {
    accountsStore.getState().addAccount(account);
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    const { rerender } = renderSidebar();
    expect(screen.getByRole("status")).toBeInTheDocument();

    accountStatusStore.getState().setStatus(account.accountId, "online");
    rerender(
      <TooltipProvider>
        <SidebarPane collapsed={false} onToggleFold={() => {}} />
      </TooltipProvider>,
    );
    expect(screen.queryByText(OFFLINE_TEXT)).not.toBeInTheDocument();
  });

  it("announces the offline status via an accessible label when collapsed", () => {
    accountsStore.getState().addAccount(account);
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    renderSidebar(true);
    expect(screen.getByRole("status", { name: OFFLINE_TEXT })).toBeInTheDocument();
  });
});

describe("SidebarPane account footer", () => {
  it("shows the account switcher row with the signed-in user id when signed in", () => {
    accountsStore.getState().addAccount(account);
    renderSidebar();
    expect(screen.getByText(account.userId)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Account menu for ${account.userId}` }),
    ).toBeInTheDocument();
  });

  it("shows no account row when signed out", () => {
    renderSidebar();
    expect(screen.queryByText(account.userId)).not.toBeInTheDocument();
  });
});

describe("SidebarPane primary view", () => {
  it("switches the primary view to archive when Archive is clicked", () => {
    renderSidebar();
    expect(primaryViewStore.getState().view).toBe("inbox");

    fireEvent.click(screen.getByRole("button", { name: "Archive" }));

    expect(primaryViewStore.getState().view).toBe("archive");
    // The Archive entry reflects the active view.
    expect(screen.getByRole("button", { name: "Archive" })).toHaveAttribute("aria-current", "page");
  });

  it("switches back to the inbox when Chats is clicked", () => {
    primaryViewStore.getState().setView("archive");
    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Chats" }));

    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(screen.getByRole("button", { name: "Chats" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Archive" })).not.toHaveAttribute("aria-current");
  });
});

describe("SidebarPane bridge-health roll-up", () => {
  it("shows no roll-up lamp when nothing is monitored", () => {
    renderSidebar();
    expect(document.querySelector('[data-slot="bridge-health-rollup"]')).not.toBeInTheDocument();
  });

  it("rolls the worst state up to the Bridges lamp (disconnected beats degraded)", () => {
    seedSession("telegram", "degraded");
    seedSession("whatsapp", "disconnected");
    seedSession("signal", "healthy");
    renderSidebar();
    const lamp = document.querySelector('[data-slot="bridge-health-rollup"]');
    expect(lamp).toBeInTheDocument();
    // Worst state is disconnected. Asserted as shape and word rather than as a
    // tint: this roll-up used to be an `aria-hidden` coloured dot, so the
    // health of every bridge at once reached a screen reader not at all, and a
    // dichromat as one of three tints 1.03–1.52 apart in luminance.
    expect(lamp).toHaveAttribute("data-state", "fault");
    // The row's NAME, not just DOM text. The button is named outright rather
    // than from its contents, because a computed name concatenates trimmed
    // text nodes and would announce "BridgesDisconnected" as one token.
    expect(screen.getByRole("button", { name: "Bridges, Disconnected" })).toBeInTheDocument();
  });

  it("shows a different shape, not just a different tint, when the worst state is degraded", () => {
    seedSession("telegram", "degraded");
    seedSession("signal", "healthy");
    renderSidebar();
    const lamp = document.querySelector('[data-slot="bridge-health-rollup"]');
    expect(lamp).toHaveAttribute("data-state", "working");
    expect(screen.getByRole("button", { name: "Bridges, Action needed" })).toBeInTheDocument();
  });

  it("puts the health into the folded rail's accessible name", () => {
    // Folded, the Bridges button carries an explicit `aria-label`, which
    // overrides everything inside it — so the lamp's own word is unreachable
    // there and the state has to be spliced into the name instead. Before
    // this, a folded rail announced plain "Bridges" whether every bridge was
    // up or every bridge was down.
    seedSession("whatsapp", "disconnected");
    renderSidebar(true);
    expect(screen.getByRole("button", { name: "Bridges, Disconnected" })).toBeInTheDocument();
  });
});

describe("SidebarPane approvals", () => {
  it("navigates to the approval pane when Approvals is clicked", () => {
    renderSidebar();
    expect(primaryViewStore.getState().view).toBe("inbox");

    fireEvent.click(screen.getByRole("button", { name: "Approvals" }));

    expect(primaryViewStore.getState().view).toBe("approval");
    expect(screen.getByRole("button", { name: "Approvals" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("shows no count badge when there are no pending drafts", () => {
    renderSidebar();
    expect(document.querySelector('[data-slot="approval-count"]')).not.toBeInTheDocument();
  });

  it("shows the amber count badge with the pending-draft count", () => {
    draftsStore.getState().mark("a1", "!r1:x", true);
    draftsStore.getState().mark("a1", "!r2:x", true);
    draftsStore.getState().mark("a2", "!r3:x", true);
    renderSidebar();
    const badge = document.querySelector('[data-slot="approval-count"]');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveClass("bg-held");
    expect(badge).toHaveTextContent("3");
  });

  it("hides the badge again when the last draft clears", () => {
    draftsStore.getState().mark("a1", "!r1:x", true);
    const { rerender } = renderSidebar();
    expect(document.querySelector('[data-slot="approval-count"]')).toBeInTheDocument();

    draftsStore.getState().mark("a1", "!r1:x", false);
    rerender(
      <TooltipProvider>
        <SidebarPane collapsed={false} onToggleFold={() => {}} />
      </TooltipProvider>,
    );
    expect(document.querySelector('[data-slot="approval-count"]')).not.toBeInTheDocument();
  });
});

describe("SidebarPane recording entry (Story 16.3)", () => {
  it("hides the Recording entry when the recording capability is off (the default)", () => {
    renderSidebar();
    expect(screen.queryByRole("button", { name: "Recording" })).not.toBeInTheDocument();
  });

  it("shows the Recording entry only when the recording capability is on", () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true });
    renderSidebar();
    expect(screen.getByRole("button", { name: "Recording" })).toBeInTheDocument();
  });

  it("switches the primary view to recording when the Recording entry is clicked", () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true });
    renderSidebar();
    expect(primaryViewStore.getState().view).toBe("inbox");

    fireEvent.click(screen.getByRole("button", { name: "Recording" }));

    expect(primaryViewStore.getState().view).toBe("recording");
    expect(screen.getByRole("button", { name: "Recording" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });
});

describe("SidebarPane recordings entry (Story 42.3)", () => {
  it("hides the Recordings entry when the recording capability is off (the default)", () => {
    renderSidebar();
    expect(screen.queryByRole("button", { name: "Recordings" })).not.toBeInTheDocument();
  });

  it("shows the Recordings entry on the same flag as the capture surface", () => {
    // One flag, both entries: a browser for recordings this build cannot make
    // would be a dead row answering a question nobody asked.
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true });
    renderSidebar();
    expect(screen.getByRole("button", { name: "Recording" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Recordings" })).toBeInTheDocument();
  });

  it("switches the primary view to recordings when the Recordings entry is clicked", () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true });
    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Recordings" }));

    expect(primaryViewStore.getState().view).toBe("recordings");
    expect(screen.getByRole("button", { name: "Recordings" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    // The capture entry is a sibling, not the same row — it is not marked active.
    expect(screen.getByRole("button", { name: "Recording" })).not.toHaveAttribute("aria-current");
  });

  it("places Recordings immediately after Recording, before Sync and Settings", () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true, sync: true });
    renderSidebar();

    const labels = screen
      .getAllByRole("button")
      .map((button) => button.textContent)
      .filter(
        (label) =>
          label === "Recording" ||
          label === "Recordings" ||
          label === "Sync" ||
          label === "Settings",
      );
    expect(labels).toEqual(["Recording", "Recordings", "Sync", "Settings"]);
  });
});

describe("SidebarPane sync entry (Story 32.5)", () => {
  it("hides the Sync entry when the sync capability is off (the default)", () => {
    renderSidebar();
    expect(screen.queryByRole("button", { name: "Sync" })).not.toBeInTheDocument();
  });

  it("shows the Sync entry only when the sync capability is on", () => {
    // A machine with no usable `git` gets no sync UI at all (AD-41), never a
    // button whose every command would reject as unsupported.
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, sync: true });
    renderSidebar();
    expect(screen.getByRole("button", { name: "Sync" })).toBeInTheDocument();
  });

  it("switches the primary view to sync when the Sync entry is clicked", () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, sync: true });
    renderSidebar();
    expect(primaryViewStore.getState().view).toBe("inbox");

    fireEvent.click(screen.getByRole("button", { name: "Sync" }));

    expect(primaryViewStore.getState().view).toBe("sync");
    expect(screen.getByRole("button", { name: "Sync" })).toHaveAttribute("aria-current", "page");
  });

  it("keeps both gated entries independent, in order, before Settings", () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true, sync: true });
    renderSidebar();

    const labels = screen
      .getAllByRole("button")
      .map((button) => button.textContent)
      .filter((label) => label === "Recording" || label === "Sync" || label === "Settings");
    expect(labels).toEqual(["Recording", "Sync", "Settings"]);
  });

  it("names the Sync entry on the collapsed rail through its tooltip trigger", () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, sync: true });
    renderSidebar(true);
    // Collapsed, the label survives only as the accessible name.
    expect(screen.getByRole("button", { name: "Sync" })).toBeInTheDocument();
  });
});

describe("SidebarPane files entry (Story 43.8)", () => {
  it("omits the Files entry entirely when the sync capability is off (the default)", () => {
    renderSidebar();
    // Where no folder can be synced there is nothing for a browser to browse,
    // so the row is absent rather than empty (FR-153, AD-27).
    expect(screen.queryByRole("button", { name: "Files" })).not.toBeInTheDocument();
  });

  it("places Files immediately after Sync, before Settings", () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITH_SYNC);
    renderSidebar();

    const labels = screen
      .getAllByRole("button")
      .map((button) => button.textContent)
      .filter((label) => label === "Sync" || label === "Files" || label === "Settings");
    expect(labels).toEqual(["Sync", "Files", "Settings"]);
  });

  it("switches the primary view to files, leaving Sync unmarked", () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_WITH_SYNC);
    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Files" }));

    expect(primaryViewStore.getState().view).toBe("files");
    expect(screen.getByRole("button", { name: "Files" })).toHaveAttribute("aria-current", "page");
    // The diagnostics pane is a sibling, not the same row.
    expect(screen.getByRole("button", { name: "Sync" })).not.toHaveAttribute("aria-current");
  });
});

describe("SidebarPane notes entry (Story 37.1)", () => {
  it("omits the Notes entry entirely when the notes capability is off", () => {
    renderSidebar();
    // Absent, not disabled (FR-122). A greyed row that answers "unsupported on
    // this platform" is a worse answer than no row: there is no vault to reach
    // on a build with no folder sync, so there is nothing to offer.
    expect(screen.queryByRole("button", { name: "Notes" })).not.toBeInTheDocument();
  });

  it("shows the Notes entry only when the notes capability is on", () => {
    capabilitiesStore.getState().applySnapshot({ ...DESKTOP_WITH_SYNC, notes: true });
    renderSidebar();
    expect(screen.getByRole("button", { name: "Notes" })).toBeInTheDocument();
  });

  it("switches the primary view to notes when the Notes entry is clicked", () => {
    capabilitiesStore.getState().applySnapshot({ ...DESKTOP_WITH_SYNC, notes: true });
    renderSidebar();
    expect(primaryViewStore.getState().view).toBe("inbox");

    fireEvent.click(screen.getByRole("button", { name: "Notes" }));

    expect(primaryViewStore.getState().view).toBe("notes");
    expect(screen.getByRole("button", { name: "Notes" })).toHaveAttribute("aria-current", "page");
  });

  it("places Notes after Sync and before Settings", () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true, sync: true, notes: true });
    renderSidebar();

    const labels = screen
      .getAllByRole("button")
      .map((button) => button.textContent)
      .filter(
        (label) =>
          label === "Recording" || label === "Sync" || label === "Notes" || label === "Settings",
      );
    expect(labels).toEqual(["Recording", "Sync", "Notes", "Settings"]);
  });
});

describe("SidebarPane as the phone drawer (Story 66.1, AD-197, AD-27)", () => {
  /** The tier a phone hydrates to once its folder links (Epic 66). */
  const PHONE_WITH_FOLDER = { ...DEFAULT_CAPABILITIES, bots: true, sync: true };

  it("keeps only the rows the phone stack can land, in registry order", () => {
    capabilitiesStore.getState().applySnapshot(PHONE_WITH_FOLDER);
    renderSidebar(false, null);
    const labels = screen
      .getAllByRole("button")
      .map((button) => button.textContent)
      .filter((label) => sidebarViews(PHONE_WITH_FOLDER).some((entry) => entry.label === label));
    // Files rides the same `sync` flag on the desktop, but the phone has no
    // Files surface yet (66.3), so its row is absent rather than dead.
    expect(labels).toEqual([
      "Chats",
      "Archive",
      "Approvals",
      "Bridges",
      "Sync",
      "Bots",
      "Settings",
    ]);
    expect(
      sidebarViews(PHONE_WITH_FOLDER)
        .filter((entry) => phoneRoutesView(entry.view, PHONE_WITH_FOLDER))
        .map((entry) => entry.label),
    ).toEqual(labels);
  });

  it("draws the whole registry off the phone, whatever the flags", () => {
    capabilitiesStore.getState().applySnapshot({ ...DESKTOP_WITH_SYNC, notes: true });
    renderSidebar();
    expect(screen.getByRole("button", { name: "Files" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Notes" })).toBeInTheDocument();
  });
});

describe("SidebarPane settings", () => {
  it("switches to the Settings view rather than opening a dialog", () => {
    primaryViewStore.getState().setView("inbox");
    renderSidebar();
    // No modal, on this click or any other: Settings is a primary view now, and
    // a dialog would trap focus over a surface meant to be read while the app
    // keeps working behind it.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    expect(primaryViewStore.getState().view).toBe("settings");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("marks the Settings entry active while its view is showing, like every other entry", () => {
    primaryViewStore.getState().setView("settings");
    renderSidebar();

    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    primaryViewStore.getState().setView("inbox");
  });
});

describe("SidebarPane reachable footer (Story 34.2)", () => {
  afterEach(() => {
    spacesStore.getState().clear();
  });

  /** Eight Spaces — enough to overflow the drawer at the 600px minimum height. */
  function seedSpaces(count: number) {
    spacesStore.getState().applySnapshot({
      spaces: Array.from({ length: count }, (_, i) => ({
        accountId: account.accountId,
        spaceId: `!space${i}:example.org`,
        name: `Space ${i}`,
        avatarUrl: null,
      })),
    });
  }

  it("scrolls the views and both groups while the account footer stays outside", () => {
    // AD-34-4: the growing content is the only thing inside the scroller, so the
    // `mt-auto` footer cannot be pushed past the `overflow-hidden` root however
    // many Spaces the user belongs to. Putting the footer inside the scroller —
    // or leaving the groups outside it — is what makes "Add account" unreachable.
    accountsStore.getState().addAccount(account);
    seedSpaces(8);
    renderSidebar();

    const viewport = document.querySelector('[data-slot="scroll-area-viewport"]');
    expect(viewport).toBeInTheDocument();
    expect(viewport).toContainElement(screen.getByRole("button", { name: "Chats" }));
    expect(viewport).toContainElement(screen.getByRole("region", { name: "Spaces" }));
    expect(viewport).not.toContainElement(screen.getByRole("button", { name: "Add account" }));
  });

  it("pairs the drawer's own min-h-0 with that scroller", () => {
    // Without `min-h-0` the pane's content sets its floor and the scroller never
    // gets a bounded height to scroll within — the container would be inert.
    renderSidebar();

    const nav = screen.getByRole("navigation", { name: "Views" });
    expect(nav).toHaveClass("min-h-0");
    expect(document.querySelector('[data-slot="scroll-area"]')).toHaveClass("min-h-0", "flex-1");
  });
});

describe("SidebarPane fold (Story 45.20, FR-198, UX-DR81)", () => {
  /** Two Spaces and two Networks. One of each cannot tell "renders the list"
   *  from "renders the first row", and this suite asserts a whole list survives
   *  the fold. */
  function seedGroups() {
    spacesStore.getState().applySnapshot({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!a:example.org",
          name: "Design",
          avatarUrl: null,
        },
        { accountId: account.accountId, spaceId: "!b:example.org", name: "Ops", avatarUrl: null },
      ],
    });
    networksStore.getState().applySnapshot({
      networks: [{ name: "Telegram" }, { name: "Signal" }],
    });
  }

  it("offers a fold control that names which way it goes, in both states", () => {
    renderSidebar(false);
    const collapse = screen.getByRole("button", { name: "Collapse menu" });
    expect(collapse).toHaveAttribute("aria-expanded", "true");
    expect(collapse).toHaveAttribute("aria-controls", "sidebar-views");
    expect(document.getElementById("sidebar-views")).toBeInTheDocument();

    cleanup();
    renderSidebar(true);
    const expand = screen.getByRole("button", { name: "Expand menu" });
    expect(expand).toHaveAttribute("aria-expanded", "false");
    expect(expand).toHaveAttribute("aria-controls", "sidebar-views");
  });

  it("presses the control rather than only offering it", () => {
    // The offer is not the act. `onToggleFold` is what the shell hands down, and
    // a control wired to nothing renders identically to one that works.
    const toggle = vi.fn();
    renderSidebar(false, toggle);
    fireEvent.click(screen.getByRole("button", { name: "Collapse menu" }));
    expect(toggle).toHaveBeenCalledTimes(1);
  });

  it("withdraws the control where the viewport has already decided", () => {
    // `null` is the shell saying "below 1080px there is no room to unfold into".
    // Absent rather than disabled: a button whose only answer is "your window is
    // too narrow" is worse than no button.
    renderSidebar(true, null);
    expect(screen.queryByRole("button", { name: /menu$/ })).not.toBeInTheDocument();
    // …and the rail is still navigable without it.
    expect(screen.getByRole("button", { name: "Chats" })).toBeInTheDocument();
  });

  it("keeps every nav control's accessible name on the folded rail", () => {
    // The requirement in one assertion: icons that keep their names, not a strip
    // of unlabelled glyphs. Every button in the views list is checked, so an
    // entry added later without a name fails here rather than shipping mute.
    capabilitiesStore.setState({
      capabilities: { ...DEFAULT_CAPABILITIES, recording: true, sync: true, notes: true },
      hydrated: true,
    });
    renderSidebar(true);

    const list = document.getElementById("sidebar-views");
    expect(list).toBeInTheDocument();
    const buttons = [...(list?.querySelectorAll("button") ?? [])];
    expect(buttons.length).toBe(10);
    for (const button of buttons) {
      expect(button).toHaveAccessibleName();
    }
    for (const name of [
      "Chats",
      "Archive",
      "Approvals",
      "Bridges",
      "Recording",
      "Recordings",
      "Sync",
      "Files",
      "Notes",
      "Settings",
    ]) {
      expect(screen.getByRole("button", { name }), name).toBeInTheDocument();
    }
  });

  it("keeps the Spaces and Networks submenus on the folded rail, named", () => {
    // They used to be dropped entirely when the drawer collapsed, so folding the
    // menu silently removed a navigation surface. Each row keeps the Space's or
    // Network's own name as its accessible name.
    seedGroups();
    renderSidebar(true);

    expect(screen.getByRole("region", { name: "Spaces" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Networks" })).toBeInTheDocument();
    for (const name of ["Design", "Ops", "Telegram", "Signal"]) {
      expect(screen.getByRole("button", { name }), name).toBeInTheDocument();
    }
    // The names are the accessible ones, not visible text: the rail has none.
    expect(screen.queryByText("Design")).not.toBeInTheDocument();
  });

  it("folds a submenu on its own, on the rail and unfolded alike", () => {
    seedGroups();
    renderSidebar(false);

    fireEvent.click(screen.getByRole("button", { name: "Collapse Spaces" }));

    // The rows are gone from the accessibility tree; the control that brings
    // them back is not, and it now says so.
    expect(screen.queryByRole("button", { name: "Design" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Ops" })).not.toBeInTheDocument();
    const reopen = screen.getByRole("button", { name: "Expand Spaces" });
    expect(reopen).toHaveAttribute("aria-expanded", "false");
    // Its neighbour is untouched — one fold per group, not one for the pair.
    expect(screen.getByRole("button", { name: "Telegram" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse Networks" })).toBeInTheDocument();

    fireEvent.click(reopen);
    expect(screen.getByRole("button", { name: "Design" })).toBeInTheDocument();
  });

  it("writes a folded submenu to the cookie so it survives a restart", () => {
    seedGroups();
    renderSidebar(false);
    fireEvent.click(screen.getByRole("button", { name: "Collapse Networks" }));

    expect(readSidebarFold(document.cookie).groups).toEqual({ spaces: false, networks: true });
    expect(sidebarFoldStore.getState().groups.networks).toBe(true);
  });
});

describe("SidebarPane glyphs", () => {
  /** Everything the menu can show at once: both gated pairs, the Notes entry,
   *  and both data-driven groups. A collision test that renders half the menu
   *  cannot see a collision between the halves. */
  function seedEverything() {
    capabilitiesStore.setState({
      capabilities: { ...DEFAULT_CAPABILITIES, recording: true, sync: true, notes: true },
      hydrated: true,
    });
    spacesStore.getState().applySnapshot({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!a:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });
    networksStore.getState().applySnapshot({ networks: [{ name: "Telegram" }] });
  }

  /** The `lucide-<name>` class every lucide glyph carries, for each svg in the
   *  menu. The name is what the glyph MEANS to a reader, so two rows sharing
   *  one is the defect this asserts against — not a detail of the markup. */
  function glyphNames() {
    const nav = screen.getByRole("navigation", { name: "Views" });
    return [...nav.querySelectorAll("svg")]
      .flatMap((svg) => [...svg.classList])
      .filter((c) => c.startsWith("lucide-"));
  }

  it("draws no glyph twice anywhere in the menu", () => {
    // The shipped set drew `Radio` on both the Bridges row and the NETWORKS
    // header, and `Inbox` on Approvals while `inbox` was the view id of Chats.
    // One glyph standing for two concepts stands for neither, and no test saw
    // it because every test looked at one row at a time.
    seedEverything();
    renderSidebar(false);

    const names = glyphNames();
    const seen = new Set<string>();
    const twice: string[] = [];
    for (const name of names) {
      if (seen.has(name)) {
        twice.push(name);
      }
      seen.add(name);
    }
    expect(twice).toEqual([]);
  });

  it("gives Approvals a verb and the two recording rows different kinds", () => {
    // Approvals is consent, not a container — and it must not wear the glyph
    // whose name is the OTHER row's view id. Recording is a screen being
    // captured; Recordings is the strip they end up on. `Video` and `Film`
    // were both a rounded rectangle at 16px.
    seedEverything();
    renderSidebar(false);

    const names = glyphNames();
    expect(names).toContain("lucide-stamp");
    expect(names).toContain("lucide-cable");
    expect(names).toContain("lucide-monitor-dot");
    expect(names).toContain("lucide-film");
    expect(names).not.toContain("lucide-inbox");
    expect(names).not.toContain("lucide-video");
    // `Radio` survives, on the one row that means a network's signal.
    expect(names).toContain("lucide-radio");
  });
});

describe("SidebarPane rail geometry (the UI around the icons)", () => {
  /** jsdom performs no layout — every rect is zero (see `priority-actions`).
   *  So the proof is the classes that FIX the geometry, and the arithmetic they
   *  stand for is spelled out in each assertion. */
  function seedIndicators() {
    seedSession("whatsapp", "disconnected");
    draftsStore.getState().mark("a1", "!r1:x", true);
    draftsStore.getState().mark("a1", "!r2:x", true);
    draftsStore.getState().mark("a1", "!r3:x", true);
  }

  it("keeps the folded rail's lamp and count dot clear of the glyph they mark", () => {
    // A folded button is `size-9` (36px) with a 1px border, so its padding box
    // is 34px and the 16px glyph centred in it occupies [10,26] on both axes.
    // A 6px indicator 1px from the corner occupies [28,34] x [2,8] — 2px clear
    // in x and 2px clear in y. The shipped `top-1.5 right-1.5` lamp sat at
    // [23,29] x [7,13] and the shipped 8px `top-1 right-1` dot at [23,31] x
    // [5,13]: both overlapped the glyph's top-right corner by 3px in BOTH axes.
    seedIndicators();
    renderSidebar(true);

    const lamp = document.querySelector('[data-slot="bridge-health-rollup"]');
    const dot = document.querySelector('[data-slot="approval-count"]');
    for (const el of [lamp, dot]) {
      expect(el).toBeInTheDocument();
      expect(el).toHaveClass("absolute", "top-px", "right-px");
      expect(el).not.toHaveClass("top-1", "top-1.5", "right-1", "right-1.5");
    }
    // One indicator metric, not two: the count dot is 6px because that is the
    // lamp's size (DESIGN.md → Shapes), where it used to be 8px.
    expect(dot).toHaveClass("size-1.5");
    expect(lamp?.querySelector("svg")).toHaveClass("size-1.5");
  });

  it("pushes the unfolded lamp and badge to the end of the row, away from the glyph", () => {
    // Unfolded the row is a flex line: glyph, label, then `ml-auto` eats every
    // spare pixel, so the indicator lands against the row's trailing padding
    // and the glyph is the row's FIRST child. There is no width at which they
    // can meet.
    seedIndicators();
    renderSidebar(false);

    const bridges = screen.getByRole("button", { name: "Bridges, Disconnected" });
    const approvals = screen.getByRole("button", { name: "Approvals, 3 pending" });
    for (const [row, slot] of [
      [bridges, "bridge-health-rollup"],
      [approvals, "approval-count"],
    ] as const) {
      expect(row.firstElementChild?.tagName).toBe("svg");
      const indicator = row.querySelector(`[data-slot="${slot}"]`);
      expect(indicator).toHaveClass("ml-auto");
      expect(row.lastElementChild).toBe(indicator);
    }
  });

  it("draws every folded rail control at one width", () => {
    // The rail is a 48px column of items that are all one size. A Space or
    // Network row used to reach that size by putting `p-1.5` around a 24px
    // avatar — 24 + 6 + 6 — and under `p-1` before that it was 32px, so its
    // hover and selected pill was 4px narrower than the pill directly above
    // it. The sum is gone: every item asks {@link FOLD_STRIP} for the size,
    // so there is nothing left to get wrong when the avatar changes.
    spacesStore.getState().applySnapshot({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!a:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });
    networksStore.getState().applySnapshot({ networks: [{ name: "Telegram" }] });
    renderSidebar(true);

    expect(screen.getByRole("button", { name: "Chats" })).toHaveClass(FOLD_STRIP.controlClass);
    for (const name of ["Design", "Telegram"]) {
      const row = screen.getByRole("button", { name });
      expect(row, name).toHaveClass(FOLD_STRIP.controlClass);
      // `size="sm"` is the 24px avatar; the size itself is a variant class, so
      // the honest assertion is the variant the row asked for.
      expect(row.querySelector('[data-slot="avatar"]'), name).toHaveAttribute("data-size", "sm");
    }
  });
});
