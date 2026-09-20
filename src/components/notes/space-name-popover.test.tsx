import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { SpaceNamePopover } from "./space-name-popover";

function Harness({
  save,
}: {
  save: (name: string, options: { ttlHours: number | null }) => Promise<void>;
}) {
  const [anchor, setAnchor] = useState<HTMLButtonElement | null>(null);
  return (
    <>
      <button type="button" onClick={(event) => setAnchor(event.currentTarget)}>
        Save as space
      </button>
      <button type="button">Other action</button>
      {anchor && (
        <SpaceNamePopover
          anchor={anchor}
          initialName={'client/acme · "budget"'}
          onSave={save}
          onClose={() => setAnchor(null)}
        />
      )}
    </>
  );
}

describe("SpaceNamePopover", () => {
  it("selects the composed name and Enter writes the named draft", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    render(<Harness save={save} />);
    fireEvent.click(screen.getByRole("button", { name: "Save as space" }));
    const input = screen.getByLabelText("Name") as HTMLInputElement;
    expect(input).toHaveFocus();
    expect(input.value.slice(input.selectionStart ?? 0, input.selectionEnd ?? 0)).toBe(
      'client/acme · "budget"',
    );
    fireEvent.change(input, { target: { value: "Trip" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(screen.queryByLabelText("Name")).not.toBeInTheDocument());
    expect(save).toHaveBeenCalledWith("Trip", { ttlHours: null });
    expect(screen.getByRole("button", { name: "Save as space" })).toHaveFocus();
  });

  it("Escape cancels without a write and restores the trigger focus", async () => {
    const save = vi.fn();
    render(<Harness save={save} />);
    fireEvent.click(screen.getByRole("button", { name: "Save as space" }));
    fireEvent.keyDown(screen.getByLabelText("Name"), { key: "Escape" });
    await waitFor(() => expect(screen.queryByLabelText("Name")).not.toBeInTheDocument());
    expect(save).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Save as space" })).toHaveFocus();
  });

  it("allows focus to leave without trapping it or saving the draft", async () => {
    const save = vi.fn();
    render(<Harness save={save} />);
    fireEvent.click(screen.getByRole("button", { name: "Save as space" }));
    const outside = screen.getByRole("button", { name: "Other action" });
    act(() => outside.focus());
    await waitFor(() => expect(screen.queryByLabelText("Name")).not.toBeInTheDocument());
    expect(outside).toHaveFocus();
    expect(save).not.toHaveBeenCalled();
  });

  it("keeps a failed draft editable and validates positive-u32 lifetime", async () => {
    const save = vi
      .fn()
      .mockRejectedValueOnce(new Error("Folder is read-only"))
      .mockResolvedValue(undefined);
    render(<Harness save={save} />);
    fireEvent.click(screen.getByRole("button", { name: "Save as space" }));
    fireEvent.click(screen.getByLabelText("Temporary space"));
    const hours = screen.getByLabelText("Expires after inactivity (hours)");
    fireEvent.change(hours, { target: { value: "0" } });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(hours, { target: { value: "48" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Folder is read-only");
    expect(screen.getByLabelText("Name")).toHaveValue('client/acme · "budget"');
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByLabelText("Name")).not.toBeInTheDocument());
    expect(save).toHaveBeenLastCalledWith('client/acme · "budget"', { ttlHours: 48 });
  });
});
