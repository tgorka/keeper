import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { COLUMN_RESIZER_LABEL, ColumnResizer } from "@/components/ui/resizable-columns";

/**
 * Who paints the hairline a resize handle sits on.
 *
 * `DESIGN.md` → Elevation & Depth: a seam has exactly one owner. Every
 * resizable column in the app shipped with two — the column drew its `border-r`
 * and this handle drew a `before:w-px` of its own right beside it, so every
 * seam a user could drag was 2px while every seam they could not was 1px. That
 * is the defect these tests refuse, and the reason the choice is a PROP rather
 * than a constant: the properties grid genuinely has no box on either side of
 * its key/value split, so there the handle is the only candidate owner.
 *
 * Class assertions, and the ceiling is the usual one — jsdom loads no Tailwind,
 * so "the seam is one pixel" is not measurable here. What these prove is the
 * thing that regressed and would regress again: whether this component claims
 * the paint or declines it.
 */

/** The decoration the handle draws — `aria-hidden`, so never the separator. */
function decoration(): HTMLElement {
  const handle = screen.getByRole("separator");
  const found = handle.querySelector<HTMLElement>(":scope > [aria-hidden='true']");
  if (found === null) {
    throw new Error("the resizer drew no hit strip");
  }
  return found;
}

function renderResizer(props: { seam?: "neighbour" | "self" } = {}) {
  return render(
    <ColumnResizer
      label="Property name"
      width={200}
      onWidth={vi.fn()}
      containerLeft={() => 0}
      min={120}
      {...props}
    />,
  );
}

afterEach(cleanup);

describe("a resize handle paints the seam only when nothing else can", () => {
  it("declines the paint by default, because a column owns its own trailing edge", () => {
    renderResizer();

    // Transparent at rest. The 1px on screen is the neighbouring column's
    // `border-r`; a second one here is the 2px seam this rule exists to name.
    expect(decoration()).toHaveClass("before:bg-transparent");
    expect(decoration()).not.toHaveClass("before:bg-border");
    expect(screen.getByRole("separator")).toHaveAttribute("data-seam", "neighbour");
  });

  it("takes the paint where the boundary is between grid cells and not boxes", () => {
    renderResizer({ seam: "self" });

    // The properties grid's key/value split: nothing spans it but the handle,
    // so declining here would delete the seam rather than de-duplicate it.
    expect(decoration()).toHaveClass("before:bg-border");
    expect(decoration()).not.toHaveClass("before:bg-transparent");
    expect(screen.getByRole("separator")).toHaveAttribute("data-seam", "self");
  });

  it("hangs its hover and focus feedback on the element that can actually take focus", () => {
    renderResizer();

    // The decoration is `aria-hidden` and unfocusable, so a bare
    // `focus-visible:` on it could never fire and a keyboard user got no seam
    // feedback at all. The separator is the focusable element, so it is the
    // group.
    expect(screen.getByRole("separator")).toHaveClass("group/column-resizer");
    expect(decoration()).toHaveClass("group-hover/column-resizer:before:bg-ring");
    expect(decoration()).toHaveClass("group-focus-visible/column-resizer:before:bg-ring");
    expect(decoration()).not.toHaveClass("focus-visible:before:bg-ring");
  });

  it("puts the lit pixel on the boundary rather than beside it", () => {
    renderResizer();

    // Lighting a seam must change its colour, not its thickness: the highlight
    // is shifted back one pixel so it lands on the column's own border instead
    // of stacking a second line against it on hover.
    expect(decoration()).toHaveClass("before:-translate-x-px");
  });

  it("still names itself the same way for a screen reader", () => {
    renderResizer({ seam: "self" });

    // The paint changed; the affordance did not.
    expect(
      screen.getByRole("separator", { name: `${COLUMN_RESIZER_LABEL} Property name` }),
    ).toBeInTheDocument();
  });
});
