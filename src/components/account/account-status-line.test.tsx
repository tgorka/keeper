import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { AccountStatusLine } from "@/components/account/account-status-line";
import type { AccountStateVm } from "@/lib/ipc/client";
import { accountStore, NO_ACCOUNT } from "@/lib/stores/account";
import { accountVm } from "@/test/account-fixture";

beforeEach(() => {
  accountStore.setState({ vm: NO_ACCOUNT, setupLink: null });
});

describe("AccountStatusLine", () => {
  it("speaks for offline, sign in again and blocked, in Rust's words", () => {
    const shown: Array<[AccountStateVm, string]> = [
      ["offline", "Offline — using settings from 14:02."],
      ["needsSignIn", "Sign in again to keep your settings in sync."],
      ["blocked", "Your Acme account does not have the keeper role. Ask your administrator."],
    ];
    for (const [state, sentence] of shown) {
      accountStore.getState().setVm(accountVm({ state, sentence }));
      const { unmount } = render(<AccountStatusLine collapsed={false} />);
      expect(screen.getByRole("status")).toHaveTextContent(sentence);
      unmount();
    }
  });

  it("is silent for every other state, including no account at all", () => {
    const silent: AccountStateVm[] = ["none", "signedOut", "signingIn", "syncing", "ready"];
    for (const state of silent) {
      accountStore.getState().setVm(accountVm({ state, sentence: "Up to date." }));
      const { container, unmount } = render(<AccountStatusLine collapsed={false} />);
      expect(container).toBeEmptyDOMElement();
      unmount();
    }
    accountStore.getState().setVm(NO_ACCOUNT);
    const { container } = render(<AccountStatusLine collapsed={false} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("has no words of its own: a VM without Rust's sentence shows nothing", () => {
    for (const state of ["offline", "needsSignIn", "blocked"] as const) {
      accountStore.getState().setVm(accountVm({ state, sentence: null }));
      const { container, unmount } = render(<AccountStatusLine collapsed={false} />);
      expect(container).toBeEmptyDOMElement();
      unmount();
    }
  });

  it("keeps the sentence as the accessible name when the rail is folded", () => {
    accountStore
      .getState()
      .setVm(accountVm({ state: "offline", sentence: "Offline — using settings from 14:02." }));
    render(<AccountStatusLine collapsed />);

    expect(
      screen.getByRole("status", { name: "Offline — using settings from 14:02." }),
    ).toBeInTheDocument();
  });
});
