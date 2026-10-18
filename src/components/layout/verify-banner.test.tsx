import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { VERIFY_BANNER_TEXT, VerifyBanner } from "@/components/layout/verify-banner";
import type { AccountVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { encryptionStatusStore } from "@/lib/stores/encryption-status";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { verificationStore } from "@/lib/stores/verification";

function account(accountId: string): AccountVm {
  return {
    accountId,
    userId: `@${accountId}:example.org`,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: 0,
    provider: "password",
  };
}

function reset() {
  encryptionStatusStore.getState().reset();
  accountsStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
  verificationStore.setState({ modalOpen: false, activeAccountId: null, flow: null });
}

beforeEach(reset);
afterEach(reset);

describe("VerifyBanner", () => {
  it("is hidden when no account is unverified", () => {
    accountsStore.getState().hydrateAll([account("a")]);
    encryptionStatusStore.getState().setStatus("a", "verified");
    render(<VerifyBanner />);
    expect(screen.queryByText(VERIFY_BANNER_TEXT)).not.toBeInTheDocument();
  });

  it("is hidden on unknown (no nag before crypto has synced)", () => {
    accountsStore.getState().hydrateAll([account("a")]);
    encryptionStatusStore.getState().setStatus("a", "unknown");
    render(<VerifyBanner />);
    expect(screen.queryByText(VERIFY_BANNER_TEXT)).not.toBeInTheDocument();
  });

  it("shows the verbatim honest copy when an account is unverified", () => {
    accountsStore.getState().hydrateAll([account("a")]);
    encryptionStatusStore.getState().setStatus("a", "unverified");
    render(<VerifyBanner />);
    expect(screen.getByText(VERIFY_BANNER_TEXT)).toBeInTheDocument();
    expect(VERIFY_BANNER_TEXT).toBe("Verify this device to read encrypted history");
  });

  it("its CTA opens verification for the unverified account, not just Settings", () => {
    accountsStore.getState().hydrateAll([account("a"), account("b")]);
    encryptionStatusStore.getState().setStatus("a", "verified");
    encryptionStatusStore.getState().setStatus("b", "unverified");
    render(<VerifyBanner />);
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    expect(verificationStore.getState().modalOpen).toBe(true);
    expect(verificationStore.getState().activeAccountId).toBe("b");
    expect(primaryViewStore.getState().view).toBe("inbox");
  });

  it("dismissing hides it and sets the dismissal flag (collapses to badge)", () => {
    accountsStore.getState().hydrateAll([account("a")]);
    encryptionStatusStore.getState().setStatus("a", "unverified");
    const { rerender } = render(<VerifyBanner />);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(encryptionStatusStore.getState().bannerDismissed).toBe(true);
    rerender(<VerifyBanner />);
    expect(screen.queryByText(VERIFY_BANNER_TEXT)).not.toBeInTheDocument();
  });
});
