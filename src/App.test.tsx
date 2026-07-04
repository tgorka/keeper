import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// Mount `App` runs `useSessionRestore`; override just `sessionRestore` with a
// never-resolving stub so the boot hook never mutates the store (each test drives
// the gate directly). Every other wrapper (e.g. the shell's connection-status
// subscribe) keeps its real implementation.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    sessionRestore: () => new Promise(() => {}),
  };
});

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
    accountsStore.setState({ hydrated: false });
  });

  afterEach(() => {
    accountsStore.getState().clear();
    accountsStore.setState({ hydrated: false });
  });

  it("renders a splash while the boot restore is in flight (not hydrated)", () => {
    render(<App />);
    expect(screen.getByRole("status", { name: "Loading keeper" })).toBeInTheDocument();
    // Neither the login screen nor the shell shows behind the splash.
    expect(screen.queryByRole("button", { name: "Sign in" })).not.toBeInTheDocument();
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
  });

  it("renders the login screen when hydrated and unauthenticated", () => {
    accountsStore.getState().markHydrated();
    render(<App />);
    expect(screen.getByRole("button", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
  });

  it("renders the app shell landmarks once hydrated with an account set", () => {
    accountsStore.getState().setCurrentAccount(account);
    accountsStore.getState().markHydrated();
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Views" })).toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
    // The room-list subscribe has not delivered a batch yet, so the chat list
    // sits in its loading state.
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });
});
