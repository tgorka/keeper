/**
 * The Bots empty states (Epic 61, Story 61.4) and what the phone adds to them
 * (Epic 62, Story 62.3, FR-400).
 *
 * The one thing asserted here that nothing else asserts: on the phone tier the
 * empty state says, once and in every state, that the drive tools live on the
 * Mac — and on the desktop tier it does not, because there the grant bar is
 * present and the sentence would be false. The tier comes from the capability
 * mirror, never from a platform sniff.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  BOTS_EMPTY_COPY,
  BOTS_NO_DRIVE_HERE_SENTENCE,
  BotEmptyState,
  type BotsEmptyKind,
} from "@/components/bots/bot-empty-state";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const KINDS: readonly BotsEmptyKind[] = [
  "no-provider",
  "no-bot",
  "no-conversation",
  "secret-missing",
];

/** The desktop tier: the drive half present, so no "on the Mac" sentence. */
const DESKTOP = {
  ...DEFAULT_CAPABILITIES,
  trayIcon: true,
  globalHotkey: true,
  bots: true,
  botTools: true,
  sync: true,
};

afterEach(() => {
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("BotEmptyState", () => {
  it.each(KINDS)("%s renders its own copy and one action", (kind) => {
    capabilitiesStore.getState().applySnapshot(DESKTOP);
    const onAction = vi.fn();
    render(<BotEmptyState kind={kind} onAction={onAction} />);
    const { message, detail, action } = BOTS_EMPTY_COPY[kind];
    expect(screen.getByText(message)).toBeInTheDocument();
    if (detail !== null) {
      expect(screen.getByText(detail)).toBeInTheDocument();
    }
    fireEvent.click(screen.getByRole("button", { name: action }));
    expect(onAction).toHaveBeenCalledTimes(1);
  });

  it.each(KINDS)("%s on the phone says once that the drive tools live on the Mac", (kind) => {
    // The phone tier: every tier-telling flag false, `bots` true (the pane
    // exists there), and hydrated — the predicate behind "On this iPhone".
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: true });
    render(<BotEmptyState kind={kind} onAction={() => {}} />);
    expect(screen.getAllByText(BOTS_NO_DRIVE_HERE_SENTENCE)).toHaveLength(1);
  });

  it.each(KINDS)("%s on the desktop does not claim the drive tools are elsewhere", (kind) => {
    capabilitiesStore.getState().applySnapshot(DESKTOP);
    render(<BotEmptyState kind={kind} onAction={() => {}} />);
    expect(screen.queryByText(BOTS_NO_DRIVE_HERE_SENTENCE)).not.toBeInTheDocument();
  });

  it("does not flash the phone sentence before the capability mirror hydrates", () => {
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
    render(<BotEmptyState kind="no-conversation" onAction={() => {}} />);
    expect(screen.queryByText(BOTS_NO_DRIVE_HERE_SENTENCE)).not.toBeInTheDocument();
  });
});
