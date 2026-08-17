import { act, createEvent, fireEvent, render, screen } from "@testing-library/react";
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

/**
 * Move the real DOM focus, and let React flush the render that focus caused.
 * The `act` is the test runner's requirement, not the control's: jsdom does
 * dispatch `focusin`/`focusout` with a `relatedTarget` from a bare `.focus()`
 * (that is the mechanism this control uses), but an update scheduled outside
 * `act` in a React act-environment is left unflushed. `fireEvent` gets the same
 * wrapping for free, which is why only the focus moves need this.
 */
function focusOn(element: HTMLElement): void {
  act(() => {
    element.focus();
  });
}

/**
 * Render and put the caret in the field, which is what unfolds the list (Story
 * 53.2). Focus, not a press: a chooser you have to open with a control of its
 * own is the shape 44.13 refused, so every flow that reads the list starts by
 * arriving at the field the way a person does.
 */
function browsing(props: Partial<ComponentProps<typeof TagCombobox>> = {}): Mock {
  const onChoose = open(props);
  focusOn(field());
  return onChoose;
}

/** A focus target outside the chooser, so leaving it is a real focus-out with a
 *  `relatedTarget` rather than a synthesised event. */
function elsewhere(): HTMLElement {
  return screen.getByRole("button", { name: "Elsewhere" });
}

/** The chooser plus something to move to. `onPress` is what the outside control
 *  does when it is clicked, so a test can prove a commit still lands. */
function withNeighbour(onPress: () => void = () => {}): Mock {
  const onChoose = vi.fn();
  render(
    <>
      <TagCombobox label="Add a tag" vocabulary={VAULT} onChoose={onChoose} />
      <button type="button" onClick={onPress}>
        Elsewhere
      </button>
    </>,
  );
  focusOn(field());
  return onChoose;
}

/** The list element itself, hidden or not — `getByRole` would refuse a folded
 *  one, and "is it still built" is a separate question from "is it on screen". */
function listbox(): HTMLElement {
  const found = document.querySelector('[role="listbox"]');
  if (found === null) {
    throw new Error("the chooser rendered no listbox at all");
  }
  return found as HTMLElement;
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
    browsing();

    expect(offered()).toEqual(VAULT);
  });

  it("says so rather than rendering an empty box when the vault has no tags", () => {
    browsing({ vocabulary: [] });

    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(screen.getByText(TAG_COMBOBOX_NO_VOCABULARY)).toBeVisible();
  });

  it("reports the state its list is actually in", () => {
    // Re-anchored by Story 53.2. This assertion used to read `aria-expanded` as
    // permanently `true`, which was honest while the list was permanent. It now
    // has a folded state, and a combobox reporting itself expanded over a
    // hidden listbox would tell a screen-reader user the opposite of what is
    // there — the same reason the literal was right before.
    open();

    expect(field()).toHaveAttribute("aria-expanded", "false");
    expect(field()).toHaveAttribute("aria-controls", listbox().id);

    focusOn(field());

    expect(field()).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("listbox")).toBe(listbox());
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
    browsing({ chosen: ["client/acme"] });

    expect(offered()).toEqual(["client", "client/acme/renewal", "standup"]);
  });
});

describe("the whole control works from the keyboard", () => {
  it("arrows to a tag and Enters it without the caret ever leaving the field", () => {
    const onChoose = open();
    focusOn(field());

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
    //
    // Story 53.2 left this alone on purpose: choosing is not "done choosing",
    // so the whole vault is offered again with the list still up. What closes
    // it is the user leaving — asserted in the folding suite below.
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
    focusOn(field());

    const option = screen.getByText("standup");
    const press = createEvent.mouseDown(option);
    fireEvent(option, press);
    fireEvent.click(option);

    expect(press.defaultPrevented).toBe(true);
    expect(onChoose).toHaveBeenCalledWith("standup");
    expect(document.activeElement).toBe(field());
  });
});

/**
 * Folding the list away when the choosing stops (Story 53.2, FR-315).
 *
 * The ask was "fold back the list of tags when I stop choosing (in all views)",
 * and the half at risk here is what "folded" means. These assertions read the
 * accessibility tree and the `hidden` attribute rather than a class, because a
 * class that changes while the box still occupies the column is exactly the
 * defect this story would ship: the owner's complaint is about height. The
 * other half is that folding is not a retraction of 44.13 — the same list, in
 * the same order, is one focus away and is still BUILT while hidden.
 */
describe("the list folds away when the choosing stops (Story 53.2)", () => {
  it("is folded before anybody has come to the field", () => {
    open();

    expect(screen.queryByRole("listbox")).toBeNull();
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(listbox()).not.toBeVisible();
  });

  it("folds once a tag is taken and the focus moves on, without unmounting the list", () => {
    const onChoose = withNeighbour();

    fireEvent.change(field(), { target: { value: "acme" } });
    fireEvent.keyDown(field(), { key: "Enter" });
    expect(onChoose).toHaveBeenCalledWith("client/acme");
    // Choosing alone is not stopping: the list is still up for the next tag.
    expect(screen.getByRole("listbox")).toBeVisible();

    focusOn(elsewhere());

    expect(screen.queryByRole("listbox")).toBeNull();
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(listbox()).toHaveAttribute("hidden");
    expect(listbox()).not.toBeVisible();
    // Hidden, not unmounted: the rows are still there to come back to.
    expect(listbox().querySelectorAll('[role="option"]')).toHaveLength(VAULT.length);
    expect(field()).toHaveAttribute("aria-expanded", "false");
  });

  it("brings the same list back when the focus returns", () => {
    withNeighbour();

    focusOn(elsewhere());
    expect(screen.queryByRole("listbox")).toBeNull();

    focusOn(field());

    expect(offered()).toEqual(VAULT);
    expect(field()).toHaveAttribute("aria-expanded", "true");
  });

  it("holds the fold through a press on an outside control, so that press still lands", () => {
    // The trap. The rows prevent the mousedown default precisely so a click on
    // THEM cannot blur the field — but a press on anything else does blur it,
    // and folding a list that sits above a dialog's Save button between
    // mousedown and mouseup moves that button out from under the cursor, so the
    // press the user meant lands on nothing. The fold therefore waits for the
    // click, by which time the browser has settled what was hit.
    const pressed = vi.fn();
    withNeighbour(pressed);

    fireEvent.pointerDown(elsewhere());
    // What the browser does as the mousedown's default action, and the moment a
    // naive `onBlur` would have folded the list under the cursor.
    focusOn(elsewhere());

    expect(screen.getByRole("listbox")).toBeVisible();

    fireEvent.pointerUp(elsewhere());
    fireEvent.click(elsewhere());

    expect(pressed).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("folds on a click outside it", () => {
    withNeighbour();

    fireEvent.click(document.body);

    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("stays up for a click inside it, including the one that takes a tag", () => {
    const onChoose = withNeighbour();

    // The label is part of the control, and pressing it is not leaving.
    fireEvent.click(screen.getByText("Add a tag"));
    expect(offered()).toEqual(VAULT);

    fireEvent.click(screen.getByText("standup"));

    expect(onChoose).toHaveBeenCalledWith("standup");
    expect(offered()).toEqual(VAULT);
  });

  it("folds on Escape with no host to dismiss, which is the space editors' whole close", () => {
    // Both space editors mount this control unconditionally and pass no
    // `onDismiss`, because there is nothing of theirs to unmount. Before this
    // story that left those two surfaces with no close path in the product.
    browsing();

    fireEvent.keyDown(field(), { key: "Escape" });

    expect(screen.queryByRole("listbox")).toBeNull();
    // The caret never left, so the next keystroke is enough to bring it back.
    expect(document.activeElement).toBe(field());

    fireEvent.change(field(), { target: { value: "acme" } });

    expect(offered()).toEqual(["client/acme", "client/acme/renewal"]);
  });

  it("still gives a host its one Escape, and folds in the same press", () => {
    const onDismiss = vi.fn();
    browsing({ onDismiss });

    fireEvent.keyDown(field(), { key: "Escape" });

    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("takes nothing on Enter while it is folded, and still swallows the key", () => {
    // There is no highlight to act on when there is no list on screen, and a
    // commit nobody could see is not what Enter means. It is still swallowed,
    // because the dialog behind this control would otherwise save.
    const onChoose = open();

    const press = createEvent.keyDown(field(), { key: "Enter" });
    fireEvent(field(), press);

    expect(onChoose).not.toHaveBeenCalled();
    expect(press.defaultPrevented).toBe(true);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("unfolds and moves the highlight in the one arrow press", () => {
    const onChoose = open();

    fireEvent.keyDown(field(), { key: "ArrowUp" });

    expect(offered()).toEqual(VAULT);
    expect(activeOption()).toBe("standup");

    fireEvent.keyDown(field(), { key: "Enter" });

    expect(onChoose).toHaveBeenCalledWith("standup");
  });
});
