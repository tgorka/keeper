import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { UTD_STUB_TEXT, UtdStub } from "@/components/chat/utd-stub";
import { accountsStore } from "@/lib/stores/accounts";
import { encryptionStatusStore } from "@/lib/stores/encryption-status";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { verificationStore } from "@/lib/stores/verification";

function reset() {
  primaryViewStore.getState().setView("inbox");
  accountsStore.getState().clear();
  encryptionStatusStore.getState().reset();
  verificationStore.setState({ modalOpen: false, activeAccountId: null, flow: null });
}

beforeEach(reset);
afterEach(reset);

describe("UtdStub", () => {
  it("renders the honest undecryptable copy (never blank)", () => {
    render(<UtdStub />);
    expect(screen.getByText(UTD_STUB_TEXT)).toBeInTheDocument();
    expect(UTD_STUB_TEXT).toBe("Can't decrypt yet — verify this device or restore key backup");
  });

  // Inside a room on the phone a view change is invisible — the room outranks
  // it — so the action has to open the dialog itself (kalypso, 2026-10-18).
  it("its inline Verify action opens verification for the unverified device", () => {
    accountsStore.getState().hydrateAll([
      {
        accountId: "a",
        userId: "@a:example.org",
        homeserverUrl: "https://matrix.example.org/",
        hueIndex: 0,
        provider: "password",
      },
    ]);
    encryptionStatusStore.getState().setStatus("a", "unverified");
    render(<UtdStub />);
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    expect(verificationStore.getState().modalOpen).toBe(true);
    expect(verificationStore.getState().activeAccountId).toBe("a");
    expect(primaryViewStore.getState().view).toBe("inbox");
  });

  it("falls back to Settings when no device is known to be unverified", () => {
    render(<UtdStub />);
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    expect(verificationStore.getState().modalOpen).toBe(false);
    expect(primaryViewStore.getState().view).toBe("settings");
  });

  it("is not an aria-live region, so a batch of historical UTD rows is not announced", () => {
    render(<UtdStub />);
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("note")).toHaveTextContent(UTD_STUB_TEXT);
  });
});
