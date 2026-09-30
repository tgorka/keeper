import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { OVERFLOW_PANEL_LABEL, OverflowValue } from "./overflow-value";

const CHAR_PX = 8;
const COLUMN_PX = 120;
const LONG_PATH = "40-media/recordings/2026/2026-09-15 14.29 mounica-sync/camera-0000.mov";

/**
 * WebKit's answers, as measured in the shipped app's engine on 2026-09-30: a
 * `truncate` box is as wide as its column and reports its text as scroll width,
 * while a `<button>` given no width sizes to its content, so its scroll width
 * equals its client width however long the text is. jsdom lays nothing out;
 * this is the smallest model of the two facts the component depends on.
 */
function withWebKitWidths(): () => void {
  const scroll = Object.getOwnPropertyDescriptor(Element.prototype, "scrollWidth");
  const client = Object.getOwnPropertyDescriptor(Element.prototype, "clientWidth");
  const textPx = (element: Element) => (element.textContent ?? "").length * CHAR_PX;
  const contentSized = (element: Element) =>
    element.tagName === "BUTTON" && !element.classList.contains("w-full");
  Object.defineProperty(Element.prototype, "scrollWidth", {
    configurable: true,
    get(this: Element) {
      return contentSized(this) || this.classList.contains("truncate") ? textPx(this) : 0;
    },
  });
  Object.defineProperty(Element.prototype, "clientWidth", {
    configurable: true,
    get(this: Element) {
      if (contentSized(this)) {
        return textPx(this);
      }
      return this.classList.contains("truncate") ? Math.min(textPx(this), COLUMN_PX) : 0;
    },
  });
  return () => {
    if (scroll !== undefined) Object.defineProperty(Element.prototype, "scrollWidth", scroll);
    if (client !== undefined) Object.defineProperty(Element.prototype, "clientWidth", client);
  };
}

describe("OverflowValue in WebKit", () => {
  let restore: () => void = () => {};
  beforeEach(() => {
    restore = withWebKitWidths();
  });
  afterEach(() => restore());

  it("settles on the trigger for a value too long for its column instead of flipping forever", () => {
    render(<OverflowValue name="camera-0000.mov" value={LONG_PATH} monospace />);

    const trigger = screen.getByRole("button", { name: LONG_PATH });
    expect(trigger).toHaveAttribute("data-overflowing", "true");

    fireEvent.click(trigger);
    expect(screen.getByLabelText(`${OVERFLOW_PANEL_LABEL}: camera-0000.mov`)).toHaveTextContent(
      LONG_PATH,
    );
  });

  it("stays inert text when the value fits", () => {
    render(<OverflowValue name="duration" value="55m" />);

    expect(screen.getByText("55m")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
