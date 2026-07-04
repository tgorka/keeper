import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { UTD_STUB_TEXT, UtdStub } from "@/components/chat/utd-stub";
import { settingsUiStore } from "@/lib/stores/settings-ui";

beforeEach(() => {
  settingsUiStore.getState().setSettingsOpen(false);
});

afterEach(() => {
  settingsUiStore.getState().setSettingsOpen(false);
});

describe("UtdStub", () => {
  it("renders the honest undecryptable copy (never blank)", () => {
    render(<UtdStub />);
    expect(screen.getByText(UTD_STUB_TEXT)).toBeInTheDocument();
    expect(UTD_STUB_TEXT).toBe("Can't decrypt yet — verify this device or restore key backup");
  });

  it("its inline Verify action opens the shared Settings dialog", () => {
    render(<UtdStub />);
    expect(settingsUiStore.getState().settingsOpen).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    expect(settingsUiStore.getState().settingsOpen).toBe(true);
  });
});
