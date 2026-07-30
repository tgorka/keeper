import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { UTD_STUB_TEXT, UtdStub } from "@/components/chat/utd-stub";
import { primaryViewStore } from "@/lib/stores/primary-view";

beforeEach(() => {
  primaryViewStore.getState().setView("inbox");
});

afterEach(() => {
  primaryViewStore.getState().setView("inbox");
});

describe("UtdStub", () => {
  it("renders the honest undecryptable copy (never blank)", () => {
    render(<UtdStub />);
    expect(screen.getByText(UTD_STUB_TEXT)).toBeInTheDocument();
    expect(UTD_STUB_TEXT).toBe("Can't decrypt yet — verify this device or restore key backup");
  });

  it("its inline Verify action goes to the Settings view", () => {
    primaryViewStore.getState().setView("inbox");
    render(<UtdStub />);
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    // Settings is a primary view now, not a dialog — the action still has to
    // land the user on the surface that can actually verify the device.
    expect(primaryViewStore.getState().view).toBe("settings");
  });

  it("is not an aria-live region, so a batch of historical UTD rows is not announced", () => {
    render(<UtdStub />);
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("note")).toHaveTextContent(UTD_STUB_TEXT);
  });
});
