import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  accountSetupResolve: vi.fn(),
  accountSetupConfirm: vi.fn(),
  accountCancelSignIn: vi.fn(),
}));

import {
  AccountSetupSheet,
  SETUP_CANCEL_SIGN_IN_LABEL,
  SETUP_CONTINUE_LABEL,
  SETUP_DEVICE_LABEL,
  SETUP_DONE_LABEL,
} from "@/components/account/account-setup-sheet";
import type { AccountSetupVm, OrgAccountVm } from "@/lib/ipc/client";
import { accountCancelSignIn, accountSetupConfirm, accountSetupResolve } from "@/lib/ipc/client";
import { accountStore, NO_ACCOUNT } from "@/lib/stores/account";
import { accountVm } from "@/test/account-fixture";

const mockResolve = vi.mocked(accountSetupResolve);
const mockConfirm = vi.mocked(accountSetupConfirm);

function setupVm(over: Partial<AccountSetupVm> = {}): AccountSetupVm {
  return {
    setupId: "setup-1",
    name: "Acme",
    issuerHost: "id.acme.dev",
    repoHost: "git.acme.dev",
    repoMode: "same",
    deviceName: "hesperia",
    deviceClass: "desktop",
    registered: false,
    replaces: null,
    ...over,
  };
}

function open(link = "keeper://setup?descriptor=https%3A%2F%2Fid.acme.dev%2Fk.json") {
  render(<AccountSetupSheet />);
  act(() => accountStore.getState().openSetup(link));
}

beforeEach(() => {
  accountStore.setState({ vm: NO_ACCOUNT, setupLink: null });
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("AccountSetupSheet", () => {
  it("shows both hosts before anything is written, and sends the edited device name", async () => {
    mockResolve.mockResolvedValue(setupVm());
    mockConfirm.mockResolvedValue(accountVm({ sentence: "Up to date." }));
    open();

    expect(await screen.findByText("id.acme.dev")).toBeInTheDocument();
    expect(screen.getByText("git.acme.dev")).toBeInTheDocument();
    expect(mockResolve).toHaveBeenCalledWith(
      "keeper://setup?descriptor=https%3A%2F%2Fid.acme.dev%2Fk.json",
    );
    // Resolving wrote nothing; only Continue does.
    expect(mockConfirm).not.toHaveBeenCalled();

    const device = screen.getByLabelText(SETUP_DEVICE_LABEL);
    expect(device).toHaveValue("hesperia");
    fireEvent.change(device, { target: { value: "  studio-mac " } });
    fireEvent.click(screen.getByRole("button", { name: SETUP_CONTINUE_LABEL }));

    await waitFor(() => expect(mockConfirm).toHaveBeenCalledWith("setup-1", "studio-mac"));
    // The result is Rust's sentence, and the mirror holds the account it answered.
    expect(await screen.findByText("Up to date.")).toBeInTheDocument();
    expect(accountStore.getState().vm.configured).toBe(true);
  });

  it("does not let a registered device's name be edited here", async () => {
    mockResolve.mockResolvedValue(setupVm({ registered: true }));
    open();

    expect(await screen.findByText("hesperia")).toBeInTheDocument();
    expect(screen.queryByLabelText(SETUP_DEVICE_LABEL)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: SETUP_CONTINUE_LABEL }));
    await waitFor(() => expect(mockConfirm).toHaveBeenCalledWith("setup-1", "hesperia"));
  });

  it("renders Rust's refusal verbatim and offers nothing to continue", async () => {
    mockResolve.mockRejectedValue({
      code: "invalidInput",
      message: "This setup link is not https, so keeper will not fetch it.",
    });
    open("http://id.acme.dev/k.json");

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This setup link is not https, so keeper will not fetch it.",
    );
    expect(screen.queryByRole("button", { name: SETUP_CONTINUE_LABEL })).not.toBeInTheDocument();
  });

  it("draws a refused outcome of Continue as a failure, in Rust's words", async () => {
    // `account_setup_confirm` resolves — it does not reject — when the sign-in
    // is refused, cancelled or cannot reach the issuer: the outcome is the state.
    const refusal = "Your Acme account does not have the keeper role. Ask your administrator.";
    mockResolve.mockResolvedValue(setupVm());
    mockConfirm.mockResolvedValue(
      accountVm({ state: "blocked", identity: null, device: null, devices: [], sentence: refusal }),
    );
    open();
    fireEvent.click(await screen.findByRole("button", { name: SETUP_CONTINUE_LABEL }));

    const outcome = await screen.findByRole("alert");
    expect(outcome).toHaveTextContent(refusal);
    expect(outcome).toHaveClass("text-destructive");
    expect(screen.queryByRole("button", { name: SETUP_DONE_LABEL })).not.toBeInTheDocument();
  });

  it("shows only what Rust says after Continue, never the previous account's line", async () => {
    // A re-setup: the mirror still holds the old account's "Up to date.".
    accountStore.getState().setVm(accountVm({ id: "old", name: "Old", sentence: "Up to date." }));
    mockResolve.mockResolvedValue(setupVm());
    mockConfirm.mockReturnValue(new Promise<OrgAccountVm>(() => {}));
    vi.mocked(accountCancelSignIn).mockResolvedValue(undefined);
    open();
    fireEvent.click(await screen.findByRole("button", { name: SETUP_CONTINUE_LABEL }));

    // A spinner and no words until Rust publishes.
    const progress = await screen.findByRole("status");
    expect(progress).toHaveTextContent(/^$/);

    act(() =>
      accountStore.getState().setVm(
        accountVm({
          state: "signingIn",
          identity: null,
          sentence: "Finish signing in to Acme in the browser.",
        }),
      ),
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Finish signing in to Acme in the browser.",
    );
    fireEvent.click(screen.getByRole("button", { name: SETUP_CANCEL_SIGN_IN_LABEL }));
    expect(accountCancelSignIn).toHaveBeenCalledTimes(1);
  });
});
