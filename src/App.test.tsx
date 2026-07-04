import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import App from "./App";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
};

describe("App", () => {
  beforeEach(() => {
    accountsStore.getState().clear();
  });

  afterEach(() => {
    accountsStore.getState().clear();
  });

  it("renders the login screen when unauthenticated", () => {
    render(<App />);
    expect(screen.getByRole("button", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
  });

  it("renders the app shell landmarks once an account is set", () => {
    accountsStore.getState().setCurrentAccount(account);
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Views" })).toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
    // The room-list subscribe has not delivered a batch yet, so the chat list
    // sits in its loading state.
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });
});
