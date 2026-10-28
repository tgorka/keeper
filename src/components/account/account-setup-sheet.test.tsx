import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  accountSetupResolve: vi.fn(),
  accountSetupConfirm: vi.fn(),
  accountCancelSignIn: vi.fn(),
}));

import {
  AccountSetupSheet,
  SETUP_BROKER_LABEL,
  SETUP_CANCEL_SIGN_IN_LABEL,
  SETUP_CONTINUE_LABEL,
  SETUP_DEVICE_LABEL,
  SETUP_DONE_LABEL,
  SETUP_FORGE_HOSTS_LABEL,
  setupClosedSentence,
  setupReplacesSentence,
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
    brokerHost: null,
    forgeHosts: [],
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
    mockConfirm.mockResolvedValue(accountVm({ sentence: "Up to date.", attempt: "done" }));
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

  it("does not let a registered device's name be edited here, and says it will be restored", async () => {
    mockResolve.mockResolvedValue(setupVm({ registered: true }));
    open();

    expect(await screen.findByText("hesperia")).toBeInTheDocument();
    expect(screen.queryByLabelText(SETUP_DEVICE_LABEL)).not.toBeInTheDocument();
    expect(
      screen.getByText(
        "This device is already in your settings repository; keeper will restore its drives, model providers and settings.",
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: SETUP_CONTINUE_LABEL }));
    await waitFor(() => expect(mockConfirm).toHaveBeenCalledWith("setup-1", "hesperia"));
  });

  it("promises no restore for a device the repository does not know yet", async () => {
    mockResolve.mockResolvedValue(setupVm({ registered: false }));
    open();

    expect(await screen.findByLabelText(SETUP_DEVICE_LABEL)).toBeInTheDocument();
    expect(screen.queryByText(/keeper will restore/)).not.toBeInTheDocument();
  });

  it("names the GitHub broker as a host to check, only when the descriptor has one", async () => {
    mockResolve.mockResolvedValue(setupVm({ brokerHost: "broker.acme.dev" }));
    const { unmount } = render(<AccountSetupSheet />);
    act(() => accountStore.getState().openSetup("keeper://setup?a"));
    expect(await screen.findByText("broker.acme.dev")).toBeInTheDocument();
    expect(screen.getByText(SETUP_BROKER_LABEL)).toBeInTheDocument();
    unmount();
    act(() => accountStore.getState().closeSetup());

    mockResolve.mockResolvedValue(setupVm());
    open("keeper://setup?b");
    await screen.findByText("git.acme.dev");
    expect(screen.queryByText(SETUP_BROKER_LABEL)).not.toBeInTheDocument();
  });

  it("lists every other host a repository token would go to, one per line, before Continue", async () => {
    // The case the list exists for: a descriptor whose GitHub source sends
    // its API calls to a host other than GitHub's, which no other fact shows.
    mockResolve.mockResolvedValue(setupVm({ forgeHosts: ["github.com", "api.elsewhere.example"] }));
    const { unmount } = render(<AccountSetupSheet />);
    act(() => accountStore.getState().openSetup("keeper://setup?a"));
    const hosts = await screen.findByRole("list", { name: SETUP_FORGE_HOSTS_LABEL });
    expect(
      within(hosts)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["github.com", "api.elsewhere.example"]);
    expect(mockConfirm).not.toHaveBeenCalled();
    unmount();
    act(() => accountStore.getState().closeSetup());

    // No other host receives a token: no row claims one does.
    mockResolve.mockResolvedValue(setupVm());
    open("keeper://setup?b");
    await screen.findByText("git.acme.dev");
    expect(screen.queryByText(SETUP_FORGE_HOSTS_LABEL)).not.toBeInTheDocument();
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
      accountVm({
        state: "blocked",
        problem: true,
        attempt: "failed",
        identity: null,
        device: null,
        devices: [],
        sentence: refusal,
      }),
    );
    open();
    fireEvent.click(await screen.findByRole("button", { name: SETUP_CONTINUE_LABEL }));

    const outcome = await screen.findByRole("alert");
    expect(outcome).toHaveTextContent(refusal);
    expect(outcome).toHaveClass("text-destructive");
    expect(screen.queryByRole("button", { name: SETUP_DONE_LABEL })).not.toBeInTheDocument();
  });

  it.each([
    [
      // Marta's case: the identity provider answered access_denied. Rust's
      // `problem` sentence names the provider, the account and its reason.
      "the identity provider refused it (R381)",
      accountVm({
        state: "blocked",
        problem: true,
        attempt: "failed",
        identity: null,
        device: null,
        devices: [],
        sentence:
          "id.acme.dev refused to sign you in to Acme: User is not allowed to use this project.",
      }),
    ],
    [
      "connecting the repository could not open its page (R387)",
      accountVm({
        state: "needsSignIn",
        problem: true,
        attempt: "failed",
        repoMode: "oauth",
        forgeConnected: false,
        sentence:
          "Connecting Acme's repository failed (keeper could not open the repository's sign-in page). Sign in again.",
      }),
    ],
    [
      "the repository was unreachable while connecting it (R387)",
      accountVm({
        state: "offline",
        problem: true,
        attempt: "failed",
        repoMode: "oauth",
        forgeConnected: false,
        sentence:
          "keeper could not reach Acme's repository to connect it. Check the connection, then sign in again.",
      }),
    ],
  ])("says why a sign-in failed, as an alert, never a neutral line: %s", async (_case, vm) => {
    mockResolve.mockResolvedValue(setupVm());
    mockConfirm.mockResolvedValue(vm);
    open();
    fireEvent.click(await screen.findByRole("button", { name: SETUP_CONTINUE_LABEL }));

    const outcome = await screen.findByRole("alert");
    expect(outcome).toHaveTextContent(vm.sentence ?? "");
    expect(outcome).toHaveClass("text-destructive");
    expect(screen.queryByText(/to use your settings on this device/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: SETUP_DONE_LABEL })).not.toBeInTheDocument();
  });

  it("says quietly that a sign-in the person closed set nothing up", async () => {
    mockResolve.mockResolvedValue(setupVm());
    mockConfirm.mockResolvedValue(
      accountVm({
        state: "signedOut",
        problem: false,
        attempt: "closed",
        identity: null,
        device: null,
        devices: [],
        sentence: "Sign in to Acme to use your settings on this device.",
      }),
    );
    open();
    fireEvent.click(await screen.findByRole("button", { name: SETUP_CONTINUE_LABEL }));

    expect(await screen.findByText(setupClosedSentence("Acme", false))).toHaveAttribute(
      "role",
      "status",
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("says a closed re-setup of an account signed in here was closed, never Up to date or Done (R387)", async () => {
    // The native sheet closed, or Cancel sign-in was pressed, while the
    // account already had a session here: its state still composes "Up to
    // date.", and only Rust's `attempt` says this attempt was closed.
    mockResolve.mockResolvedValue(setupVm());
    mockConfirm.mockResolvedValue(
      accountVm({ state: "ready", sentence: "Up to date.", attempt: "closed" }),
    );
    open();
    fireEvent.click(await screen.findByRole("button", { name: SETUP_CONTINUE_LABEL }));

    expect(await screen.findByText(setupClosedSentence("Acme", true))).toHaveAttribute(
      "role",
      "status",
    );
    expect(screen.queryByText("Up to date.")).not.toBeInTheDocument();
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

  it("offers the sign-in's link while it waits in the browser and drops it once it moves on", async () => {
    // 102.9 #1, R470: the browser may not come forward; the link is the way in.
    const url = "https://id.acme.dev/authorize?client_id=keeper&state=s1&code_challenge=c";
    mockResolve.mockResolvedValue(setupVm());
    mockConfirm.mockReturnValue(new Promise<OrgAccountVm>(() => {}));
    open();
    fireEvent.click(await screen.findByRole("button", { name: SETUP_CONTINUE_LABEL }));
    act(() =>
      accountStore.getState().setVm(
        accountVm({
          state: "signingIn",
          identity: null,
          sentence: "Finish signing in to Acme in the browser.",
          signInLink: {
            url,
            notOpened:
              "keeper could not open the sign-in page. Press Copy link, paste it into your browser's address bar and finish signing in there — keeper keeps waiting. Or cancel and sign in again.",
          },
        }),
      ),
    );
    expect(screen.getByText(url)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("keeper could not open the sign-in page.");
    expect(screen.getByRole("button", { name: "Copy link" })).toBeInTheDocument();

    act(() =>
      accountStore
        .getState()
        .setVm(accountVm({ state: "syncing", sentence: "Syncing your settings…" })),
    );
    expect(screen.queryByText(url)).not.toBeInTheDocument();
  });

  it("says which account Continue replaces, and only when it replaces one", async () => {
    mockResolve.mockResolvedValue(setupVm({ replaces: "Old Corp" }));
    const { unmount } = render(<AccountSetupSheet />);
    act(() => accountStore.getState().openSetup("keeper://setup?a"));
    expect(await screen.findByText(setupReplacesSentence("Old Corp"))).toBeInTheDocument();
    unmount();
    act(() => accountStore.getState().closeSetup());

    // A first account, or the same account again: nothing is replaced.
    mockResolve.mockResolvedValue(setupVm({ replaces: null }));
    open();
    expect(await screen.findByRole("button", { name: SETUP_CONTINUE_LABEL })).toBeInTheDocument();
    expect(screen.queryByText(/This replaces/)).not.toBeInTheDocument();
  });
});
