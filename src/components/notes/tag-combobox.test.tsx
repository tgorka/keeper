import { createEvent, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { describe, expect, it, type Mock, vi } from "vitest";
import {
  TAG_COMBOBOX_NO_VOCABULARY,
  TagCombobox,
  tagComboboxAlreadyChosen,
  tagComboboxCreate,
  tagComboboxNoMatch,
} from "@/components/notes/tag-combobox";

/**
 * The tag chooser (Story 44.13, FR-169, UX-DR61).
 *
 * These assertions are made through the rendered control and its real DOM
 * focus, not through the matcher underneath it. The matcher has its own suite
 * (`components/tags/tag-match.test.ts`); what is at risk HERE is the part that
 * is not a pure function — whether the list is on screen before anything is
 * typed, whether the arrow keys move a selection Enter can then act on, and
 * where the caret is afterwards. Epic 43 shipped two defects of exactly that
 * shape, both invisible to a test of the pure half.
 */
const VAULT = ["client", "client/acme", "client/acme/renewal", "standup"];

function open(props: Partial<ComponentProps<typeof TagCombobox>> = {}): Mock {
  const onChoose = vi.fn();
  render(<TagCombobox label="Add a tag" vocabulary={VAULT} onChoose={onChoose} {...props} />);
  return onChoose;
}

/** The field, by the accessible role the control claims to have. */
const field = () => screen.getByRole("combobox", { name: "Add a tag" });

/** The tag text of every option on screen, in the order it is offered. */
const offered = () => screen.getAllByRole("option").map((option) => option.textContent);

/** The option the keyboard is currently on, read the way a screen reader does:
 *  through the field's `aria-activedescendant`, not through a CSS class. */
function activeOption(): string | null {
  const id = field().getAttribute("aria-activedescendant");
  return id === null ? null : (document.getElementById(id)?.textContent ?? null);
}

describe("the list is browsable before anything is typed", () => {
  it("renders every tag with an empty query, in the vault's own order", () => {
    open();

    expect(offered()).toEqual(VAULT);
  });

  it("says so rather than rendering an empty box when the vault has no tags", () => {
    open({ vocabulary: [] });

    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(screen.getByText(TAG_COMBOBOX_NO_VOCABULARY)).toBeInTheDocument();
  });

  it("reports itself as a combobox whose options are on screen", () => {
    // The list is permanent, so `aria-expanded` is permanently true. A control
    // that reported itself collapsed while showing options would tell a screen
    // reader the opposite of what is there.
    open();

    expect(field()).toHaveAttribute("aria-expanded", "true");
    expect(field()).toHaveAttribute("aria-controls", screen.getByRole("listbox").id);
  });
});

describe("typing narrows the same list", () => {
  it("filters to what was typed without taking the list away", () => {
    open();

    fireEvent.change(field(), { target: { value: "acme" } });

    expect(offered()).toEqual(["client/acme", "client/acme/renewal"]);
  });

  it("completes a hierarchy at its segment boundary", () => {
    open();

    fireEvent.change(field(), { target: { value: "client/ac" } });

    expect(offered()).toEqual(["client/acme", "client/acme/renewal"]);
  });

  it("gives the whole list back when the query is cleared", () => {
    open();

    fireEvent.change(field(), { target: { value: "acme" } });
    fireEvent.change(field(), { target: { value: "" } });

    expect(offered()).toEqual(VAULT);
  });
});

describe("a tag the vocabulary does not have", () => {
  it("offers to create it where creating is allowed, and hands back what was typed", () => {
    const onChoose = open({ allowCreate: true });

    fireEvent.change(field(), { target: { value: "client/newco" } });
    expect(screen.getByText(tagComboboxCreate("client/newco"))).toBeInTheDocument();

    fireEvent.keyDown(field(), { key: "Enter" });
    expect(onChoose).toHaveBeenCalledWith("client/newco");
  });

  it("says there is no such tag where creating is not allowed", () => {
    const onChoose = open();

    fireEvent.change(field(), { target: { value: "client/newco" } });

    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(screen.getByText(tagComboboxNoMatch("client/newco"))).toBeInTheDocument();

    // The refusal is a refusal, not a shrug: Enter must not invent the tag.
    fireEvent.keyDown(field(), { key: "Enter" });
    expect(onChoose).not.toHaveBeenCalled();
  });

  it("puts the offer to create below the matches, never above them", () => {
    // Otherwise Enter creates a near-duplicate of the tag the user was one
    // keystroke away from picking.
    open({ allowCreate: true });

    fireEvent.change(field(), { target: { value: "client/acm" } });

    expect(offered()).toEqual([
      "client/acme",
      "client/acme/renewal",
      tagComboboxCreate("client/acm"),
    ]);
  });

  it("will not create a second copy of a tag already chosen", () => {
    // Case-folded, because `Standup` and `standup` are one tag and two chips
    // would be a lie about the filter.
    open({ allowCreate: true, chosen: ["standup"] });

    fireEvent.change(field(), { target: { value: "Standup" } });

    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(screen.getByText(tagComboboxAlreadyChosen("Standup"))).toBeInTheDocument();
  });

  it("leaves a chosen tag out of the list it offers", () => {
    open({ chosen: ["client/acme"] });

    expect(offered()).toEqual(["client", "client/acme/renewal", "standup"]);
  });
});

describe("the whole control works from the keyboard", () => {
  it("arrows to a tag and Enters it without the caret ever leaving the field", () => {
    const onChoose = open();
    field().focus();

    fireEvent.keyDown(field(), { key: "ArrowDown" });
    fireEvent.keyDown(field(), { key: "ArrowDown" });

    expect(activeOption()).toBe("client/acme/renewal");
    expect(document.activeElement).toBe(field());

    fireEvent.keyDown(field(), { key: "Enter" });

    expect(onChoose).toHaveBeenCalledWith("client/acme/renewal");
    expect(document.activeElement).toBe(field());
  });

  it("starts on the first option so Enter alone picks the closest match", () => {
    const onChoose = open();

    fireEvent.change(field(), { target: { value: "acme" } });
    expect(activeOption()).toBe("client/acme");

    fireEvent.keyDown(field(), { key: "Enter" });
    expect(onChoose).toHaveBeenCalledWith("client/acme");
  });

  it("wraps at both ends, so ArrowUp from the top reaches the last tag", () => {
    open();

    fireEvent.keyDown(field(), { key: "ArrowUp" });
    expect(activeOption()).toBe("standup");

    fireEvent.keyDown(field(), { key: "ArrowDown" });
    expect(activeOption()).toBe("client");
  });

  it("clears the query and the selection once a tag is taken", () => {
    // Tagging happens in runs. A control that kept the query would make the
    // second tag start by deleting the first one's name.
    open();

    fireEvent.change(field(), { target: { value: "acme" } });
    fireEvent.keyDown(field(), { key: "ArrowDown" });
    fireEvent.keyDown(field(), { key: "Enter" });

    expect(field()).toHaveValue("");
    expect(offered()).toEqual(VAULT);
    expect(activeOption()).toBe("client");
  });

  it("puts the highlight back on the closest match whenever the query changes", () => {
    const onChoose = open();

    fireEvent.keyDown(field(), { key: "ArrowUp" });
    expect(activeOption()).toBe("standup");

    fireEvent.change(field(), { target: { value: "acme" } });
    fireEvent.keyDown(field(), { key: "Enter" });

    expect(onChoose).toHaveBeenCalledWith("client/acme");
  });

  it("keeps Enter on a real row when the list shrinks underneath it", () => {
    // Not a typing case — this is the list changing while the caret sits
    // still, which is what happens when a chip is raised elsewhere in the bar
    // with the chooser open. Without the clamp the highlight points past the
    // end and Enter fires on nothing while a row still looks chosen.
    const onChoose = vi.fn();
    const { rerender } = render(
      <TagCombobox label="Add a tag" vocabulary={VAULT} onChoose={onChoose} />,
    );

    fireEvent.keyDown(field(), { key: "ArrowUp" });
    expect(activeOption()).toBe("standup");

    rerender(
      <TagCombobox
        label="Add a tag"
        vocabulary={VAULT}
        chosen={["client", "client/acme", "client/acme/renewal"]}
        onChoose={onChoose}
      />,
    );

    expect(activeOption()).toBe("standup");
    fireEvent.keyDown(field(), { key: "Enter" });
    expect(onChoose).toHaveBeenCalledWith("standup");
  });

  it("Escape clears the query first and dismisses only once it is empty", () => {
    const onDismiss = vi.fn();
    open({ onDismiss });

    fireEvent.change(field(), { target: { value: "acme" } });
    fireEvent.keyDown(field(), { key: "Escape" });

    expect(field()).toHaveValue("");
    expect(onDismiss).not.toHaveBeenCalled();

    fireEvent.keyDown(field(), { key: "Escape" });
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("keeps Escape to itself, so dismissing does not also drop the chip behind it", () => {
    // The filter bar's Esc walks the chip stack down one press at a time. This
    // control is mounted inside that bar.
    const outer = vi.fn();
    const onDismiss = vi.fn();
    render(
      // A `section` rather than a `div` only so the wrapper is not a static
      // element with a keyboard handler; the real bar's column is a labelled
      // region with exactly this Esc handler on it.
      <section aria-label="Filters" onKeyDown={outer}>
        <TagCombobox
          label="Add a tag"
          vocabulary={VAULT}
          onChoose={vi.fn()}
          onDismiss={onDismiss}
        />
      </section>,
    );

    fireEvent.keyDown(screen.getByRole("combobox", { name: "Add a tag" }), { key: "Escape" });

    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(outer).not.toHaveBeenCalled();
  });

  it("refuses the focus a mouse press would take, so the caret survives a click", () => {
    // jsdom does not move focus on mousedown the way a browser does, so the
    // assertion is on the prevented default rather than on `activeElement`:
    // that IS the mechanism, and it is what disappears if the handler goes.
    const onChoose = open();
    field().focus();

    const option = screen.getByText("standup");
    const press = createEvent.mouseDown(option);
    fireEvent(option, press);
    fireEvent.click(option);

    expect(press.defaultPrevented).toBe(true);
    expect(onChoose).toHaveBeenCalledWith("standup");
    expect(document.activeElement).toBe(field());
  });
});
