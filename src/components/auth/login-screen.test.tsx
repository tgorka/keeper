import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, IpcError, IpcErrorCode } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";

// Mock the typed IPC wrapper so the component never touches Tauri.
const loginPassword = vi.fn();
const loginOidc = vi.fn();
const cancelOidc = vi.fn();
const beeperRequestCode = vi.fn();
const loginBeeper = vi.fn();
const cancelBeeper = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  loginPassword: (...args: [string, string, string]) => loginPassword(...args),
  loginOidc: (...args: [string]) => loginOidc(...args),
  cancelOidc: (...args: []) => cancelOidc(...args),
  beeperRequestCode: (...args: [string]) => beeperRequestCode(...args),
  loginBeeper: (...args: [string, string]) => loginBeeper(...args),
  cancelBeeper: (...args: []) => cancelBeeper(...args),
}));

// Import after the mock is registered.
import { LoginScreen } from "@/components/auth/login-screen";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

function ipcError(code: IpcErrorCode): IpcError {
  return { code, message: "ignored", accountId: null, retriable: false };
}

function fillAndSubmit() {
  fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "example.org" } });
  fireEvent.change(screen.getByLabelText("Username"), { target: { value: "alice" } });
  fireEvent.change(screen.getByLabelText("Password"), { target: { value: "hunter2" } });
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
}

/** Switch to the Beeper tab and wait for its content to mount. Radix Tabs
 * activate on pointer-down in jsdom, so mouseDown precedes the click. */
async function openBeeperTab() {
  const tab = screen.getByRole("tab", { name: "Beeper" });
  fireEvent.mouseDown(tab);
  fireEvent.click(tab);
  await screen.findByLabelText("Email");
}

describe("LoginScreen", () => {
  beforeEach(() => {
    accountsStore.getState().clear();
    loginPassword.mockReset();
    loginOidc.mockReset();
    cancelOidc.mockReset();
    cancelOidc.mockResolvedValue(undefined);
    beeperRequestCode.mockReset();
    loginBeeper.mockReset();
    cancelBeeper.mockReset();
    cancelBeeper.mockResolvedValue(undefined);
  });

  afterEach(() => {
    accountsStore.getState().clear();
  });

  it("renders the homeserver, username, and password fields", () => {
    render(<LoginScreen />);
    expect(screen.getByLabelText("Homeserver")).toBeInTheDocument();
    expect(screen.getByLabelText("Username")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in" })).toBeInTheDocument();
  });

  it("invokes login_password with the entered values on submit", async () => {
    loginPassword.mockResolvedValue(account);
    render(<LoginScreen />);
    fillAndSubmit();
    await waitFor(() => {
      expect(loginPassword).toHaveBeenCalledWith("example.org", "alice", "hunter2");
    });
  });

  it("records the account and gates the shell on success", async () => {
    loginPassword.mockResolvedValue(account);
    render(<LoginScreen />);
    fillAndSubmit();
    await waitFor(() => {
      expect(accountsStore.getState().accounts).toEqual([account]);
    });
  });

  it("adds (does not replace) an existing account in add mode and calls onDone", async () => {
    const existing: AccountVm = {
      accountId: "01BX5ZZKBKACTAV9WEVGEMMVRZ",
      userId: "@bob:example.org",
      homeserverUrl: "https://matrix.example.org/",
      hueIndex: 1,
      provider: "password",
    };
    accountsStore.getState().addAccount(existing);
    loginPassword.mockResolvedValue(account);
    const onDone = vi.fn();
    render(<LoginScreen addMode onDone={onDone} />);

    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "example.org" } });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "alice" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "hunter2" } });
    fireEvent.click(screen.getByRole("button", { name: "Add account" }));

    await waitFor(() => {
      expect(accountsStore.getState().accounts.map((a) => a.accountId)).toEqual([
        existing.accountId,
        account.accountId,
      ]);
    });
    expect(onDone).toHaveBeenCalledTimes(1);
  });

  it("calls onDone when Cancel is clicked in add mode", () => {
    const onDone = vi.fn();
    render(<LoginScreen addMode onDone={onDone} />);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onDone).toHaveBeenCalledTimes(1);
  });

  it("clears the password field after submit", async () => {
    loginPassword.mockResolvedValue(account);
    render(<LoginScreen />);
    fillAndSubmit();
    await waitFor(() => {
      expect(screen.getByLabelText<HTMLInputElement>("Password").value).toBe("");
    });
  });

  it("shows the bad-credentials message for invalidCredentials", async () => {
    loginPassword.mockRejectedValue(ipcError("invalidCredentials"));
    render(<LoginScreen />);
    fillAndSubmit();
    expect(await screen.findByText("Wrong username or password.")).toBeInTheDocument();
  });

  it("shows the unreachable message for serverUnreachable", async () => {
    loginPassword.mockRejectedValue(ipcError("serverUnreachable"));
    render(<LoginScreen />);
    fillAndSubmit();
    expect(
      await screen.findByText(
        "Couldn't reach that homeserver. Check the address and your connection.",
      ),
    ).toBeInTheDocument();
  });

  it("shows the unsupported-login-type message for unsupportedLoginType", async () => {
    loginPassword.mockRejectedValue(ipcError("unsupportedLoginType"));
    render(<LoginScreen />);
    fillAndSubmit();
    expect(
      await screen.findByText("This homeserver doesn't support password login."),
    ).toBeInTheDocument();
  });

  it("shows the SSS message and a doc link for slidingSyncUnsupported", async () => {
    loginPassword.mockRejectedValue(ipcError("slidingSyncUnsupported"));
    render(<LoginScreen />);
    fillAndSubmit();
    expect(
      await screen.findByText(
        /This homeserver doesn't support Simplified Sliding Sync, which keeper requires\./,
      ),
    ).toBeInTheDocument();
    const link = screen.getByRole("link", { name: /Simplified Sliding Sync/ });
    expect(link).toHaveAttribute("href");
  });

  it("guards blank/whitespace input without calling the backend", async () => {
    render(<LoginScreen />);
    // Whitespace-only homeserver/username and empty password.
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "   " } });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "  " } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(
      await screen.findByText("Enter your homeserver, username, and password."),
    ).toBeInTheDocument();
    expect(loginPassword).not.toHaveBeenCalled();
  });

  it("trims surrounding whitespace before calling the backend", async () => {
    loginPassword.mockResolvedValue(account);
    render(<LoginScreen />);
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "  example.org " } });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: " alice " } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "hunter2" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => {
      expect(loginPassword).toHaveBeenCalledWith("example.org", "alice", "hunter2");
    });
  });

  it("does not set an account when login fails", async () => {
    loginPassword.mockRejectedValue(ipcError("invalidCredentials"));
    render(<LoginScreen />);
    fillAndSubmit();
    await screen.findByText("Wrong username or password.");
    expect(accountsStore.getState().accounts).toEqual([]);
  });

  it("invokes login_oidc with the entered homeserver on the SSO button", async () => {
    loginOidc.mockResolvedValue(account);
    render(<LoginScreen />);
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "  mas.example  " } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in with single sign-on" }));
    await waitFor(() => {
      expect(loginOidc).toHaveBeenCalledWith("mas.example");
    });
  });

  it("records the account and calls onDone on OIDC success", async () => {
    loginOidc.mockResolvedValue(account);
    const onDone = vi.fn();
    render(<LoginScreen addMode onDone={onDone} />);
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "mas.example" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in with single sign-on" }));
    await waitFor(() => {
      expect(accountsStore.getState().accounts).toEqual([account]);
    });
    expect(onDone).toHaveBeenCalledTimes(1);
  });

  it("guards a blank homeserver on the SSO button without calling the backend", async () => {
    render(<LoginScreen />);
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in with single sign-on" }));
    expect(
      await screen.findByText("Enter your homeserver, username, and password."),
    ).toBeInTheDocument();
    expect(loginOidc).not.toHaveBeenCalled();
  });

  it("shows the pending state and a Cancel button while OIDC is in flight", async () => {
    // A never-resolving promise keeps the flow pending.
    loginOidc.mockReturnValue(new Promise(() => {}));
    render(<LoginScreen />);
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "mas.example" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in with single sign-on" }));
    expect(await screen.findByText("Complete sign-in in your browser…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
  });

  it("calls cancel_oidc when Cancel is clicked during a pending OIDC flow", async () => {
    loginOidc.mockReturnValue(new Promise(() => {}));
    render(<LoginScreen />);
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "mas.example" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in with single sign-on" }));
    await screen.findByText("Complete sign-in in your browser…");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(cancelOidc).toHaveBeenCalledTimes(1);
    });
  });

  it("returns quietly to the form (no error) when the OIDC flow is cancelled", async () => {
    loginOidc.mockRejectedValue(ipcError("oauthCancelled"));
    render(<LoginScreen />);
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "mas.example" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in with single sign-on" }));
    // The form (Homeserver field) is back and no destructive alert is shown.
    await waitFor(() => {
      expect(screen.getByLabelText("Homeserver")).toBeInTheDocument();
    });
    expect(screen.queryByText("Couldn't sign in")).not.toBeInTheDocument();
    expect(accountsStore.getState().accounts).toEqual([]);
  });

  it("shows the unsupported message for oauthUnsupported", async () => {
    loginOidc.mockRejectedValue(ipcError("oauthUnsupported"));
    render(<LoginScreen />);
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "mas.example" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in with single sign-on" }));
    expect(
      await screen.findByText("This homeserver doesn't offer single sign-on (OIDC)."),
    ).toBeInTheDocument();
  });

  it("shows the timeout message for oauthTimedOut", async () => {
    loginOidc.mockRejectedValue(ipcError("oauthTimedOut"));
    render(<LoginScreen />);
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "mas.example" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in with single sign-on" }));
    expect(
      await screen.findByText(
        "Single sign-on timed out. It wasn't completed in the browser in time.",
      ),
    ).toBeInTheDocument();
  });

  it("shows the failed message for oauthFailed", async () => {
    loginOidc.mockRejectedValue(ipcError("oauthFailed"));
    render(<LoginScreen />);
    fireEvent.change(screen.getByLabelText("Homeserver"), { target: { value: "mas.example" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in with single sign-on" }));
    expect(
      await screen.findByText("Single sign-on didn't complete. Please try again."),
    ).toBeInTheDocument();
  });

  it("shows the permanent unofficial-API subtitle on the Beeper tab", async () => {
    render(<LoginScreen />);
    await openBeeperTab();
    expect(screen.getByText("Unofficial API — may break without notice")).toBeInTheDocument();
    // Beeper asks only for email at the first step — no homeserver field.
    expect(screen.getByLabelText("Email")).toBeInTheDocument();
    expect(screen.queryByLabelText("Homeserver")).not.toBeInTheDocument();
  });

  it("requests a code and advances to the code step on Send code", async () => {
    beeperRequestCode.mockResolvedValue(undefined);
    render(<LoginScreen />);
    await openBeeperTab();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "  alice@beeper.com " } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    await waitFor(() => {
      expect(beeperRequestCode).toHaveBeenCalledWith("alice@beeper.com");
    });
    // The code step is now shown.
    expect(await screen.findByLabelText("Login code")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Verify" })).toBeInTheDocument();
  });

  it("shows the coverage disclosure after login and does not record the account yet", async () => {
    beeperRequestCode.mockResolvedValue(undefined);
    loginBeeper.mockResolvedValue(account);
    const onDone = vi.fn();
    render(<LoginScreen addMode onDone={onDone} />);
    await openBeeperTab();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "alice@beeper.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    await screen.findByLabelText("Login code");
    fireEvent.change(screen.getByLabelText("Login code"), { target: { value: "424242" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    await waitFor(() => {
      expect(loginBeeper).toHaveBeenCalledWith("alice@beeper.com", "424242");
    });
    // The coverage disclosure gate is shown before the account enters the inbox.
    expect(
      await screen.findByText(
        "WhatsApp connected in the official Beeper app will not appear here.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "I understand" })).toBeInTheDocument();
    // Neither addAccount nor onDone has run yet.
    expect(accountsStore.getState().accounts).toEqual([]);
    expect(onDone).not.toHaveBeenCalled();
  });

  it("records the account and calls onDone once when the disclosure is acknowledged", async () => {
    beeperRequestCode.mockResolvedValue(undefined);
    loginBeeper.mockResolvedValue(account);
    const onDone = vi.fn();
    render(<LoginScreen addMode onDone={onDone} />);
    await openBeeperTab();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "alice@beeper.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    await screen.findByLabelText("Login code");
    fireEvent.change(screen.getByLabelText("Login code"), { target: { value: "424242" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    const acknowledge = await screen.findByRole("button", { name: "I understand" });
    fireEvent.click(acknowledge);
    await waitFor(() => {
      expect(accountsStore.getState().accounts).toEqual([account]);
    });
    expect(onDone).toHaveBeenCalledTimes(1);
  });

  it("clears the code field after submit", async () => {
    beeperRequestCode.mockResolvedValue(undefined);
    // A never-resolving login keeps the flow pending so we can inspect the field.
    loginBeeper.mockReturnValue(new Promise(() => {}));
    render(<LoginScreen />);
    await openBeeperTab();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "alice@beeper.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    await screen.findByLabelText("Login code");
    fireEvent.change(screen.getByLabelText("Login code"), { target: { value: "424242" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    await waitFor(() => {
      expect(screen.getByLabelText<HTMLInputElement>("Login code").value).toBe("");
    });
  });

  it("renders the named failure with Retry and a status link on beeperUnavailable", async () => {
    beeperRequestCode.mockRejectedValue(ipcError("beeperUnavailable"));
    render(<LoginScreen />);
    await openBeeperTab();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "alice@beeper.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    expect(
      await screen.findByText(
        /Beeper login unavailable — this is an unofficial API and may have changed\./,
      ),
    ).toBeInTheDocument();
    const link = screen.getByRole("link", { name: /Beeper status/ });
    expect(link).toHaveAttribute("href", "https://status.beeper.com");
    // Retry returns to the email step.
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByLabelText("Email")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send code" })).toBeInTheDocument();
  });

  it("renders the named failure when verification fails", async () => {
    beeperRequestCode.mockResolvedValue(undefined);
    loginBeeper.mockRejectedValue(ipcError("beeperUnavailable"));
    render(<LoginScreen />);
    await openBeeperTab();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "alice@beeper.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    await screen.findByLabelText("Login code");
    fireEvent.change(screen.getByLabelText("Login code"), { target: { value: "000000" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    expect(
      await screen.findByText(
        /Beeper login unavailable — this is an unofficial API and may have changed\./,
      ),
    ).toBeInTheDocument();
    expect(accountsStore.getState().accounts).toEqual([]);
  });

  it("guards a blank Beeper email without calling the backend", async () => {
    render(<LoginScreen />);
    await openBeeperTab();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    expect(await screen.findByText("Enter your Beeper email.")).toBeInTheDocument();
    expect(beeperRequestCode).not.toHaveBeenCalled();
  });

  it("guards a blank Beeper code without calling the backend", async () => {
    beeperRequestCode.mockResolvedValue(undefined);
    render(<LoginScreen />);
    await openBeeperTab();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "alice@beeper.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    await screen.findByLabelText("Login code");
    fireEvent.change(screen.getByLabelText("Login code"), { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    expect(await screen.findByText("Enter the emailed code.")).toBeInTheDocument();
    expect(loginBeeper).not.toHaveBeenCalled();
  });

  it("calls cancel_beeper when unmounted mid-flow", async () => {
    beeperRequestCode.mockResolvedValue(undefined);
    // A never-resolving login keeps the flow pending across unmount.
    loginBeeper.mockReturnValue(new Promise(() => {}));
    const { unmount } = render(<LoginScreen />);
    await openBeeperTab();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "alice@beeper.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    await screen.findByLabelText("Login code");
    fireEvent.change(screen.getByLabelText("Login code"), { target: { value: "424242" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    // Unmount while the login is still pending.
    unmount();
    await waitFor(() => {
      expect(cancelBeeper).toHaveBeenCalledTimes(1);
    });
  });

  it("calls cancel_beeper when unmounted idle on the code step (no verify pending)", async () => {
    // After Send code the backend holds a request id but the code step sits idle
    // with pending === false. Abandoning here (overlay dismissed / tab switched)
    // must still cancel the backend flow so no registry residue lingers.
    beeperRequestCode.mockResolvedValue(undefined);
    const { unmount } = render(<LoginScreen />);
    await openBeeperTab();
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "alice@beeper.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    // Reach the code step, then unmount without submitting a code.
    await screen.findByLabelText("Login code");
    unmount();
    await waitFor(() => {
      expect(cancelBeeper).toHaveBeenCalledTimes(1);
    });
  });
});
