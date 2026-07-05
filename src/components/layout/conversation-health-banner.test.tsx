import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BridgeHealth } from "@/lib/ipc/client";
import { bridgeHealthStore } from "@/lib/stores/bridge-health";

// The banner's Re-link action mounts the shipped BridgeLoginSheet, which uses the
// streaming login IPC on start. Stub the client so opening the Sheet never touches a
// real Tauri channel; a never-resolving `startBridgeLogin` keeps it in waiting state.
vi.mock("@/lib/ipc/client", () => ({
  startBridgeLogin: vi.fn(() => new Promise<number>(() => {})),
  submitBridgeLogin: vi.fn(() => Promise.resolve()),
  cancelBridgeLogin: vi.fn(() => Promise.resolve()),
  bridgeBotRoom: vi.fn(() => Promise.resolve("!bot:example.org")),
}));

import { ConversationHealthBanner } from "@/components/layout/conversation-pane";

const ACCOUNT_ID = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/** Seed one session's live health into the store. */
function seedHealth(networkId: string, health: BridgeHealth, networkName = networkId) {
  bridgeHealthStore.getState().applySnapshot({
    sessions: [
      { accountId: ACCOUNT_ID, networkId, networkName, health, lastCheckedMs: 1, detail: null },
    ],
  });
}

beforeEach(() => {
  bridgeHealthStore.getState().reset();
});

afterEach(() => {
  bridgeHealthStore.getState().reset();
});

describe("ConversationHealthBanner", () => {
  it("renders nothing for a native room (no networkId)", () => {
    seedHealth("whatsapp", "disconnected");
    render(<ConversationHealthBanner accountId={ACCOUNT_ID} networkId={null} />);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("renders nothing for a healthy session", () => {
    seedHealth("whatsapp", "healthy");
    render(<ConversationHealthBanner accountId={ACCOUNT_ID} networkId="whatsapp" />);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("renders nothing for an unmonitored session (no store entry)", () => {
    render(<ConversationHealthBanner accountId={ACCOUNT_ID} networkId="whatsapp" />);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows a non-dismissible banner naming the network when the session is unhealthy", () => {
    seedHealth("whatsapp", "disconnected", "WhatsApp");
    render(<ConversationHealthBanner accountId={ACCOUNT_ID} networkId="whatsapp" />);
    const banner = screen.getByRole("alert");
    expect(banner).toHaveTextContent("WhatsApp disconnected — messages may not arrive.");
    // No dismiss control — it is persistent until the session recovers.
    expect(screen.queryByRole("button", { name: /dismiss/i })).not.toBeInTheDocument();
  });

  it("only matches on BOTH accountId and networkId (not the display label)", () => {
    seedHealth("telegram", "disconnected", "Telegram");
    // A whatsapp room must not surface an unhealthy telegram session.
    render(<ConversationHealthBanner accountId={ACCOUNT_ID} networkId="whatsapp" />);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("Re-link opens the login stepper for that exact (accountId, networkId)", async () => {
    seedHealth("whatsapp", "disconnected", "WhatsApp");
    render(<ConversationHealthBanner accountId={ACCOUNT_ID} networkId="whatsapp" />);
    fireEvent.click(screen.getByRole("button", { name: "Re-link" }));
    // The shipped login Sheet opens — its title names the network being re-linked.
    expect(await screen.findByRole("dialog", { name: /Connect WhatsApp/ })).toBeInTheDocument();
  });
});
