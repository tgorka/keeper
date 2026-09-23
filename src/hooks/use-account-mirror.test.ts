import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  accountSubscribe: vi.fn(),
  accountUnsubscribe: vi.fn(() => Promise.resolve()),
  accountSync: vi.fn(),
  listenAccountSetup: vi.fn(),
}));

import { useAccountMirror } from "@/hooks/use-account-mirror";
import { accountSubscribe, accountSync, listenAccountSetup } from "@/lib/ipc/client";
import { accountStore, NO_ACCOUNT } from "@/lib/stores/account";
import { accountVm } from "@/test/account-fixture";

const mockListen = vi.mocked(listenAccountSetup);
const mockSubscribe = vi.mocked(accountSubscribe);
const mockSync = vi.mocked(accountSync);

beforeEach(() => {
  accountStore.setState({ vm: NO_ACCOUNT, setupLink: null });
  mockSubscribe.mockResolvedValue("sub-1");
  mockSync.mockResolvedValue(accountVm());
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("useAccountMirror", () => {
  it("registers the setup-link listener before opening the subscription that emits a held link", async () => {
    // The shell emits the link that started keeper from inside
    // `account_subscribe`; a listener still being registered would miss it.
    let registered: (fn: () => void) => void = () => {};
    let deliver: (link: string) => void = () => {};
    mockListen.mockImplementation((onLink) => {
      deliver = onLink;
      return new Promise((resolve) => {
        registered = resolve;
      });
    });
    mockSubscribe.mockImplementation(async (onVm) => {
      deliver("keeper://setup?d=eyJ9");
      onVm(accountVm({ state: "signedOut", identity: null }));
      return "sub-1";
    });
    renderHook(() => useAccountMirror());

    await Promise.resolve();
    expect(mockSubscribe).not.toHaveBeenCalled();

    await act(async () => registered(() => {}));
    await waitFor(() => expect(mockSubscribe).toHaveBeenCalledTimes(1));
    expect(accountStore.getState().setupLink).toBe("keeper://setup?d=eyJ9");
    expect(accountStore.getState().vm.state).toBe("signedOut");
  });

  it("asks for an unforced sync on focus only while an account is signed in", async () => {
    mockListen.mockResolvedValue(() => {});
    renderHook(() => useAccountMirror());
    await waitFor(() => expect(mockSubscribe).toHaveBeenCalled());

    // No account: focus sends nothing anywhere.
    window.dispatchEvent(new Event("focus"));
    act(() => accountStore.getState().setVm(accountVm({ state: "signedOut", identity: null })));
    window.dispatchEvent(new Event("focus"));
    expect(mockSync).not.toHaveBeenCalled();

    act(() => accountStore.getState().setVm(accountVm()));
    window.dispatchEvent(new Event("focus"));
    expect(mockSync).toHaveBeenCalledWith(false);
  });

  it("asks for an unforced sync when the app comes back to the foreground", async () => {
    // The phone's WKWebView reports a resume as visibilitychange, not focus.
    mockListen.mockResolvedValue(() => {});
    accountStore.getState().setVm(accountVm());
    renderHook(() => useAccountMirror());
    await waitFor(() => expect(mockSubscribe).toHaveBeenCalled());
    const visibility = vi.spyOn(document, "visibilityState", "get");

    visibility.mockReturnValue("hidden");
    document.dispatchEvent(new Event("visibilitychange"));
    expect(mockSync).not.toHaveBeenCalled();

    visibility.mockReturnValue("visible");
    document.dispatchEvent(new Event("visibilitychange"));
    expect(mockSync).toHaveBeenCalledTimes(1);
    expect(mockSync).toHaveBeenCalledWith(false);
    visibility.mockRestore();
  });
});
