import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, Provider } from "@/lib/ipc/client";

// Mock the sign-out hook so the footer never touches Tauri; the handler is a spy
// that records the account id it was called with.
const signOutHandler = vi.fn();
vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => signOutHandler,
}));

// The Settings dialog loads the encryption posture on open; stub just that
// wrapper so opening it from the row menu never reaches Tauri.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    encryptionPosture: vi.fn(() => Promise.resolve(false)),
    // Global DND toggle (Story 10.2): default off, capture the set call.
    dndGetGlobal: vi.fn(() => Promise.resolve(false)),
    dndSetGlobal: vi.fn(() => Promise.resolve()),
  };
});

import { AccountFooter } from "@/components/layout/account-footer";
import { TooltipProvider } from "@/components/ui/tooltip";
import { dndGetGlobal, dndSetGlobal } from "@/lib/ipc/client";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";
import { addAccountStore } from "@/lib/stores/add-account";
import { encryptionStatusStore } from "@/lib/stores/encryption-status";
import { primaryViewStore } from "@/lib/stores/primary-view";

function account(id: string, userId: string, hue = 0, provider: Provider = "password"): AccountVm {
  return {
    accountId: id,
    userId,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: hue,
    provider,
  };
}

const alice = account("01ARZ3NDEKTSV4RRFFQ69G5FAV", "@alice:example.org", 0);
const bob = account("01BX5ZZKBKACTAV9WEVGEMMVRZ", "@bob:example.org", 1);

const beeper: AccountVm = {
  accountId: "01CX5ZZKBKACTAV9WEVGEMMVRZ",
  userId: "@carol:beeper.com",
  homeserverUrl: "https://matrix.beeper.com/",
  hueIndex: 2,
  provider: "beeper",
};

function renderFooter(collapsed = false) {
  return render(
    <TooltipProvider>
      <AccountFooter collapsed={collapsed} />
    </TooltipProvider>,
  );
}

/** Open the per-account dropdown menu and return the menu element. Radix opens
 * its menu on pointer-down (not `click` in jsdom); keyboard activation is the
 * reliable path under Testing Library. */
async function openRowMenu(userId: string) {
  const trigger = screen.getByRole("button", { name: `Account menu for ${userId}` });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

beforeEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  accountStatusStore.getState().reset();
  encryptionStatusStore.getState().reset();
  primaryViewStore.getState().setView("inbox");
  addAccountStore.getState().closeAddAccount();
  signOutHandler.mockReset();
  signOutHandler.mockResolvedValue(undefined);
});

afterEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  accountStatusStore.getState().reset();
  addAccountStore.getState().closeAddAccount();
});

describe("AccountFooter", () => {
  it("shows only the Add Account button when there are no accounts", () => {
    renderFooter();
    expect(screen.getByRole("button", { name: "Add account" })).toBeInTheDocument();
    expect(screen.queryByText(alice.userId)).not.toBeInTheDocument();
  });

  it("lists every signed-in account with a switcher row, homeserver and menu", () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    renderFooter();
    expect(screen.getByText(alice.userId)).toBeInTheDocument();
    expect(screen.getByText(bob.userId)).toBeInTheDocument();
    // The homeserver host is rendered on each row.
    expect(screen.getAllByText("matrix.example.org")).toHaveLength(2);
    expect(
      screen.getByRole("button", { name: `Account menu for ${alice.userId}` }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Account menu for ${bob.userId}` }),
    ).toBeInTheDocument();
  });

  it("the Add Account button opens the add-account overlay and is never count-gated", () => {
    // No accounts at all: Add Account is still present.
    renderFooter();
    expect(addAccountStore.getState().open).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Add account" }));
    expect(addAccountStore.getState().open).toBe(true);
  });

  it("shows a syncing spinner when no status batch has arrived yet", () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    expect(screen.getByLabelText("Syncing")).toBeInTheDocument();
  });

  // Online is the state the account is in almost all the time, and a glyph that
  // is present whenever nothing is wrong says nothing to the person reading it.
  // The two states that are not fine still draw one; this one draws nothing.
  it("draws no glyph when the account is online, because that is the boring case", () => {
    accountsStore.getState().hydrateAll([alice]);
    accountStatusStore.getState().setStatus(alice.accountId, "online");
    renderFooter();
    expect(screen.queryByLabelText("Synced")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Syncing")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Offline")).not.toBeInTheDocument();
  });

  it("shows the offline glyph when the account is offline", () => {
    accountsStore.getState().hydrateAll([alice]);
    accountStatusStore.getState().setStatus(alice.accountId, "offline");
    renderFooter();
    expect(screen.getByLabelText("Offline")).toBeInTheDocument();
  });

  /**
   * Story 55.10 reverses Story 49's answer, at the owner's second asking.
   *
   * 49 was asked for the same thing — the `⋮` gone, the row opening the menu —
   * and declined it, for a reason worth keeping: the row was the inbox account
   * filter, so putting a menu on its click deleted one-click filtering to buy
   * back one glyph. That reasoning was right about the cost and wrong about the
   * only way to pay it. The filter is now the menu's first item, so it is still
   * one control away and its state is still visible; what it costs is a second
   * click, which is the trade the owner asked for twice.
   *
   * What must not regress: the filter still exists, still toggles, and still
   * shows whether it is on.
   */
  it("filters the inbox from the row's menu, and says whether the filter is on", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    renderFooter();

    await openRowMenu(alice.userId);
    fireEvent.click(
      screen.getByRole("menuitemcheckbox", { name: `Filter inbox to ${alice.userId}` }),
    );
    expect(accountsStore.getState().filterAccountId).toBe(alice.accountId);

    await openRowMenu(alice.userId);
    const clear = screen.getByRole("menuitemcheckbox", {
      name: `Clear filter for ${alice.userId}`,
    });
    // Checked, not merely relabelled: the row used to carry `aria-pressed` and
    // something has to keep saying the filter is on.
    expect(clear).toHaveAttribute("aria-checked", "true");
    fireEvent.click(clear);
    expect(accountsStore.getState().filterAccountId).toBeNull();
  });

  /**
   * One control per account in the expanded footer.
   *
   * The `⋮` sat in a reserved gutter beside a row that did something else, which
   * is two controls and one of them 24px wide. The row is the control now, and
   * the gutter is width the account's own name gets back — so a second button
   * reappearing here is the regression, not a detail.
   */
  it("gives an expanded row one control, not a row plus a three-dot button", () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();

    const trigger = screen.getByRole("button", { name: `Account menu for ${alice.userId}` });
    // The row itself: it carries the account's name, so it is not a glyph in a
    // gutter.
    expect(trigger).toHaveTextContent(alice.userId);
    expect(screen.getAllByRole("button", { name: /alice/ })).toHaveLength(1);
    // A trigger is not a toggle and must not claim to be one; the filter's
    // state lives on the menu item now.
    expect(trigger).not.toHaveAttribute("aria-pressed");
  });

  it("keeps the collapsed row's menu reachable from the keyboard even though it is quiet", () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter(true);

    // Folded, the `⋮` is painted only on hover/focus — but "quiet" is opacity,
    // not absence: it is in the accessible tree, it is not `aria-hidden`, and
    // it takes focus. A `hidden`-scoped query would pass on a control that had
    // been removed from the tree, so this asserts the default (visible-only)
    // query still finds it and that focus lands on it.
    const menu = screen.getByRole("button", { name: `Account menu for ${alice.userId}` });
    expect(menu).not.toHaveAttribute("aria-hidden");
    menu.focus();
    expect(document.activeElement).toBe(menu);
  });

  it("the row menu opens the keep-archive sign-out dialog and confirming signs out", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    renderFooter();

    const menu = await openRowMenu(bob.userId);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Sign out…" }));

    const dialog = await screen.findByRole("alertdialog");
    // The dialog title frames the keep-local-archive default (UX-DR20).
    expect(
      within(dialog).getByRole("heading", { name: "Sign out, keep local archive" }),
    ).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign out, keep local archive" }));

    await waitFor(() => {
      expect(signOutHandler).toHaveBeenCalledWith(bob.accountId);
    });
  });

  it("cancelling the sign-out dialog does not sign out and closes it", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();

    const menu = await openRowMenu(alice.userId);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Sign out…" }));

    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
    expect(signOutHandler).not.toHaveBeenCalled();
  });

  it("the row menu goes to the Settings view", async () => {
    primaryViewStore.getState().setView("inbox");
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();

    const menu = await openRowMenu(alice.userId);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Settings" }));

    // No dialog to find any more: the footer routes to the pane, so the app
    // stays visible behind whatever the user came here to change.
    expect(primaryViewStore.getState().view).toBe("settings");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("offers a Beeper coverage menu item that opens the disclosure for a Beeper account", async () => {
    accountsStore.getState().hydrateAll([beeper]);
    renderFooter();

    const menu = await openRowMenu(beeper.userId);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Beeper coverage" }));

    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText(
        "WhatsApp connected in the official Beeper app will not appear here.",
      ),
    ).toBeInTheDocument();
  });

  it("does not offer a Beeper coverage item for a non-Beeper account", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();

    const menu = await openRowMenu(alice.userId);
    expect(
      within(menu).queryByRole("menuitem", { name: "Beeper coverage" }),
    ).not.toBeInTheDocument();
  });

  /** Open the sign-out dialog for an account and return the alertdialog element. */
  async function openSignOutDialog(userId: string) {
    const menu = await openRowMenu(userId);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Sign out…" }));
    return await screen.findByRole("alertdialog");
  }

  it("the default dialog states the unsynced-content caveat", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    const dialog = await openSignOutDialog(alice.userId);
    expect(
      within(dialog).getByText(
        /never synced and decrypted before you sign out is not recoverable/i,
      ),
    ).toBeInTheDocument();
  });

  it("arming the destructive path reveals the identity field and gates confirm on exact trimmed identity", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    const dialog = await openSignOutDialog(alice.userId);

    // The arming control is a secondary (non-destructive) button.
    fireEvent.click(
      within(dialog).getByRole("button", { name: "…and delete this Account's archive" }),
    );

    const field = within(dialog).getByLabelText(`Type ${alice.userId} to confirm deletion`);
    const confirm = within(dialog).getByRole("button", {
      name: "Sign out and delete archive",
    });
    // Disabled until the identity is typed exactly.
    expect(confirm).toBeDisabled();
    fireEvent.change(field, { target: { value: "@wrong:example.org" } });
    expect(confirm).toBeDisabled();
    // Extra surrounding whitespace still matches (trimmed-equals).
    fireEvent.change(field, { target: { value: `  ${alice.userId}  ` } });
    expect(confirm).toBeEnabled();
  });

  it("the armed dialog uses destructive framing, not the keep-archive copy", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    const dialog = await openSignOutDialog(alice.userId);

    fireEvent.click(
      within(dialog).getByRole("button", { name: "…and delete this Account's archive" }),
    );

    expect(
      within(dialog).getByRole("heading", { name: "Delete this Account's archive" }),
    ).toBeInTheDocument();
    // The keep-archive copy must NOT be present once armed.
    expect(
      within(dialog).queryByRole("heading", { name: "Sign out, keep local archive" }),
    ).not.toBeInTheDocument();
    expect(within(dialog).queryByText(/stays on this Mac/i)).not.toBeInTheDocument();
  });

  it("arming is reversible without closing the dialog", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    const dialog = await openSignOutDialog(alice.userId);

    fireEvent.click(
      within(dialog).getByRole("button", { name: "…and delete this Account's archive" }),
    );
    expect(
      within(dialog).getByRole("heading", { name: "Delete this Account's archive" }),
    ).toBeInTheDocument();

    // A control returns to the keep-archive choice in place (dialog stays open).
    fireEvent.click(within(dialog).getByRole("button", { name: "Keep archive instead" }));
    expect(
      within(dialog).getByRole("heading", { name: "Sign out, keep local archive" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).toBeInTheDocument();
  });

  it("confirming the armed destructive path runs the delete-archive sign-out", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    renderFooter();
    const dialog = await openSignOutDialog(bob.userId);

    fireEvent.click(
      within(dialog).getByRole("button", { name: "…and delete this Account's archive" }),
    );
    fireEvent.change(within(dialog).getByLabelText(`Type ${bob.userId} to confirm deletion`), {
      target: { value: bob.userId },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign out and delete archive" }));

    await waitFor(() => {
      expect(signOutHandler).toHaveBeenCalledWith(bob.accountId, { deleteArchive: true });
    });
  });

  it("renders avatar-only rows with a menu when collapsed", () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter(true);
    expect(screen.queryByText(alice.userId)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Filter inbox to ${alice.userId}` }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Account menu for ${alice.userId}` }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add account" })).toBeInTheDocument();
  });

  // ── Global Do-Not-Disturb toggle (Story 10.2) ──────────────────────────────
  it("shows a Do not disturb row and reads the global DND state on open", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    await openRowMenu(alice.userId);
    expect(await screen.findByText("Do not disturb")).toBeInTheDocument();
    await waitFor(() => {
      expect(dndGetGlobal).toHaveBeenCalled();
    });
  });

  it("toggling Do not disturb writes the new global state via dndSetGlobal", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    await openRowMenu(alice.userId);
    const item = await screen.findByText("Do not disturb");
    // Initial read resolved to false; the toggle flips it on.
    await waitFor(() => {
      expect(dndGetGlobal).toHaveBeenCalled();
    });
    fireEvent.click(item);
    await waitFor(() => {
      expect(dndSetGlobal).toHaveBeenCalledWith(true);
    });
  });

  it("Do not disturb reports its state as a checked menu item, not as a drawn tick", async () => {
    accountsStore.getState().hydrateAll([alice]);
    renderFooter();
    await openRowMenu(alice.userId);

    // An app-wide switch that silences every notification on every account used
    // to say which way it was set with a tick glyph and nothing else — a
    // picture, and so nothing at all to anyone not looking at this menu.
    const dnd = await screen.findByRole("menuitemcheckbox", {
      name: "Do not disturb",
      checked: false,
    });
    fireEvent.click(dnd);

    await waitFor(() => {
      expect(dndSetGlobal).toHaveBeenCalledWith(true);
    });
    // The menu stays open and the item now says it is on, in place.
    expect(
      await screen.findByRole("menuitemcheckbox", { name: "Do not disturb", checked: true }),
    ).toBeInTheDocument();
  });
});
