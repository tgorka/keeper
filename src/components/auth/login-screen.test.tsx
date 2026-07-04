import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, IpcError, IpcErrorCode } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";

// Mock the typed IPC wrapper so the component never touches Tauri.
const loginPassword = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  loginPassword: (...args: [string, string, string]) => loginPassword(...args),
}));

// Import after the mock is registered.
import { LoginScreen } from "@/components/auth/login-screen";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
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

describe("LoginScreen", () => {
  beforeEach(() => {
    accountsStore.getState().clear();
    loginPassword.mockReset();
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
      expect(accountsStore.getState().currentAccount).toEqual(account);
    });
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
    expect(accountsStore.getState().currentAccount).toBeNull();
  });
});
