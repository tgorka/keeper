import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  setEncryptionPosture: vi.fn(() => Promise.resolve()),
}));

import {
  AtRestEncryptionChoice,
  CHOICE_EXPLANATION,
  CHOICE_SAVE_ERROR,
  CHOICE_SWITCH_LABEL,
  CHOICE_TITLE,
  STORAGE_HONESTY_SENTENCE,
} from "@/components/settings/at-rest-encryption-choice";
import { setEncryptionPosture } from "@/lib/ipc/client";

const mockSetPosture = vi.mocked(setEncryptionPosture);

describe("AtRestEncryptionChoice", () => {
  beforeEach(() => {
    mockSetPosture.mockClear();
    mockSetPosture.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("renders the exact voice-rules copy with no exclamation mark", () => {
    render(<AtRestEncryptionChoice onResolved={() => {}} />);
    expect(screen.getByText(CHOICE_TITLE)).toBeInTheDocument();
    expect(screen.getByText(CHOICE_EXPLANATION)).toBeInTheDocument();
    expect(screen.getByText(CHOICE_SWITCH_LABEL)).toBeInTheDocument();
    expect(screen.getByText(STORAGE_HONESTY_SENTENCE)).toBeInTheDocument();
    // Voice rules (UX-DR10): no exclamation marks anywhere in the rendered text.
    expect(document.body.textContent).not.toContain("!");
  });

  it("defaults the switch to off", () => {
    render(<AtRestEncryptionChoice onResolved={() => {}} />);
    const toggle = screen.getByRole("switch", { name: CHOICE_SWITCH_LABEL });
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  it("Continue with the switch off persists off then resolves", async () => {
    const onResolved = vi.fn();
    render(<AtRestEncryptionChoice onResolved={onResolved} />);

    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    await waitFor(() => expect(mockSetPosture).toHaveBeenCalledWith(false));
    expect(onResolved).toHaveBeenCalledTimes(1);
  });

  it("toggling on then Continue persists on then resolves", async () => {
    const onResolved = vi.fn();
    render(<AtRestEncryptionChoice onResolved={onResolved} />);

    fireEvent.click(screen.getByRole("switch", { name: CHOICE_SWITCH_LABEL }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    await waitFor(() => expect(mockSetPosture).toHaveBeenCalledWith(true));
    expect(onResolved).toHaveBeenCalledTimes(1);
  });

  it("surfaces an error and allows retry when persisting the choice fails", async () => {
    const onResolved = vi.fn();
    mockSetPosture.mockRejectedValueOnce({ code: "internal", message: "boom", retriable: false });
    render(<AtRestEncryptionChoice onResolved={onResolved} />);

    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    // The failure is surfaced (not a silent no-op) and onResolved is not called.
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(CHOICE_SAVE_ERROR));
    expect(onResolved).not.toHaveBeenCalled();

    // Continue is re-enabled so the user can retry; the retry succeeds.
    mockSetPosture.mockResolvedValueOnce(undefined);
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(onResolved).toHaveBeenCalledTimes(1));
  });
});
