import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import {
  COLUMN_COLLAPSE_PREFIX,
  COLUMN_EXPAND_PREFIX,
  SURFACE_COLUMN_FOLDED_WIDTH,
  useSurfaceColumn,
} from "@/components/layout/surface-column";
import { COLUMN_FITTED_VALUE_TEXT, COLUMN_RESIZER_LABEL } from "@/components/ui/resizable-columns";
import {
  COLUMN_KEY_STEP,
  COLUMN_WIDTH_COOKIE,
  MAX_COLUMN_WIDTH,
  readColumnWidths,
  SURFACE_COLUMN_IDS,
  SURFACE_COLUMNS,
  type SurfaceColumnId,
} from "@/lib/column-widths";
import {
  COLUMN_FOLD_COOKIE,
  hydrateColumnFold,
  readColumnFold,
  resetColumnFoldForTest,
} from "@/lib/stores/column-fold";

/**
 * What every surface column does, proved once against a host that is nothing
 * but the contract (Story 48.1).
 *
 * The four real columns each assert that they WIRED this — the fold control is
 * on screen in `notes-pane.test.tsx`, `files-pane.test.tsx`,
 * `chat-list-pane.test.tsx` and `app-shell.test.tsx`. What is here is the
 * behaviour those four share, and it is parametrised over the real id set so a
 * fifth column cannot be added with a floor nobody chose.
 */

/** The body a folded column must not be rendering. */
const BODY = "the column body";

function Host({ id, enabled }: { id: SurfaceColumnId; enabled?: boolean }) {
  const column = useSurfaceColumn(id, { enabled });
  return (
    <div className="flex">
      <div {...column.rootProps} data-testid="column">
        {column.chrome}
        {column.folded ? null : <p>{BODY}</p>}
      </div>
      {column.seam}
    </div>
  );
}

/** What a reload does: the module state goes, the cookie stays. */
function reload(id: SurfaceColumnId, unmount: () => void) {
  unmount();
  resetColumnFoldForTest();
  hydrateColumnFold(document.cookie);
  return render(<Host id={id} />);
}

afterEach(() => {
  resetColumnFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state is this test's subject
  document.cookie = `${COLUMN_FOLD_COOKIE}=; path=/; max-age=0`;
  // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state is this test's subject
  document.cookie = `${COLUMN_WIDTH_COOKIE}=; path=/; max-age=0`;
});

describe.each(SURFACE_COLUMN_IDS)("the %s column", (id) => {
  const spec = SURFACE_COLUMNS[id];
  const collapse = `${COLUMN_COLLAPSE_PREFIX} ${spec.label}`;
  const expand = `${COLUMN_EXPAND_PREFIX} ${spec.label}`;
  const seam = `${COLUMN_RESIZER_LABEL} ${spec.label}`;

  it("starts at its own width, showing its body and offering the fold", () => {
    render(<Host id={id} />);

    expect(screen.getByTestId("column")).toHaveStyle({ width: `${spec.defaultWidth}px` });
    expect(screen.getByText(BODY)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: collapse })).toHaveAttribute("aria-expanded", "true");
  });

  it("folds to a strip that still holds the way back", () => {
    render(<Host id={id} />);

    fireEvent.click(screen.getByRole("button", { name: collapse }));

    // The body is gone, not hidden: a folded column that keeps its subtree
    // mounted keeps its subscriptions, which is the cost folding reclaims.
    expect(screen.queryByText(BODY)).not.toBeInTheDocument();
    expect(screen.getByTestId("column")).toHaveStyle({
      width: `${SURFACE_COLUMN_FOLDED_WIDTH}px`,
    });
    // A fold with no handle is a column the user deleted by accident. The
    // control is a real button in the tab order, named for where it goes.
    const back = screen.getByRole("button", { name: expand });
    expect(back).toHaveAttribute("aria-expanded", "false");
    back.focus();
    expect(back).toHaveFocus();

    fireEvent.click(back);
    expect(screen.getByText(BODY)).toBeInTheDocument();
  });

  it("comes back folded after a reload", () => {
    const { unmount } = render(<Host id={id} />);
    fireEvent.click(screen.getByRole("button", { name: collapse }));
    expect(readColumnFold(document.cookie)[id]).toBe(true);

    reload(id, unmount);

    expect(screen.getByRole("button", { name: expand })).toBeInTheDocument();
    expect(screen.queryByText(BODY)).not.toBeInTheDocument();
  });

  it("comes back showing when the last run left it showing", () => {
    // The other arm, written down: "folded" is the state a restore that did
    // nothing could not produce, and "showing" is the state it could.
    const { unmount } = render(<Host id={id} />);
    fireEvent.click(screen.getByRole("button", { name: collapse }));
    fireEvent.click(screen.getByRole("button", { name: expand }));

    reload(id, unmount);

    expect(screen.getByRole("button", { name: collapse })).toBeInTheDocument();
    expect(screen.getByText(BODY)).toBeInTheDocument();
  });

  it("resizes from the keyboard and remembers it across a reload", () => {
    const { unmount } = render(<Host id={id} />);

    const handle = screen.getByRole("separator", { name: seam });
    // A surface column is never fitted to content — it holds a list, and a
    // list is as wide as it is given. The seam reports the real number.
    expect(handle).toHaveAttribute("aria-valuenow", String(spec.defaultWidth));
    expect(handle).not.toHaveAttribute("aria-valuetext", COLUMN_FITTED_VALUE_TEXT);
    expect(handle).toHaveAttribute("aria-valuemin", String(spec.minWidth));
    expect(handle).toHaveAttribute("aria-valuemax", String(MAX_COLUMN_WIDTH));

    fireEvent.keyDown(handle, { key: "ArrowRight" });
    const wider = spec.defaultWidth + COLUMN_KEY_STEP;
    expect(screen.getByTestId("column")).toHaveStyle({ width: `${wider}px` });
    expect(readColumnWidths(document.cookie)[id]).toBe(wider);

    unmount();
    render(<Host id={id} />);
    expect(screen.getByTestId("column")).toHaveStyle({ width: `${wider}px` });
  });

  it("resizes from a drag", () => {
    render(<Host id={id} />);
    const handle = screen.getByRole("separator", { name: seam });

    fireEvent.pointerDown(handle, { pointerId: 1, button: 0, clientX: 100 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 140 });
    fireEvent.pointerUp(handle, { pointerId: 1 });

    expect(screen.getByTestId("column")).toHaveStyle({
      width: `${spec.defaultWidth + 40}px`,
    });
    expect(readColumnWidths(document.cookie)[id]).toBe(spec.defaultWidth + 40);
  });

  it("stops at its own floor rather than the shared one", () => {
    render(<Host id={id} />);
    const handle = screen.getByRole("separator", { name: seam });

    fireEvent.pointerDown(handle, { pointerId: 1, button: 0, clientX: 500 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 0 });
    fireEvent.pointerUp(handle, { pointerId: 1 });

    expect(screen.getByTestId("column")).toHaveStyle({ width: `${spec.minWidth}px` });
    expect(readColumnWidths(document.cookie)[id]).toBe(spec.minWidth);
  });

  /**
   * The interaction rule, and it is the part that would be wrong if it were not
   * deliberate: **a fold suspends a width, it never spends one.**
   *
   * Nothing on the fold path writes `keeper_column_widths`, and nothing on the
   * width path reads the fold. The failure this refuses is the one Story 48.2
   * is fixing a layer down, where a lock wrote the normalised window size over
   * the remembered one: if the strip's 48px could reach the width cookie, one
   * fold would erase a layout the user had arranged and there would be no
   * moment at which anything looked wrong.
   */
  it("keeps the width it was given while folded, and gives it back on unfold", () => {
    render(<Host id={id} />);
    const handle = screen.getByRole("separator", { name: seam });
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    const chosen = spec.defaultWidth + COLUMN_KEY_STEP;

    fireEvent.click(screen.getByRole("button", { name: collapse }));

    // The strip is 48px wide and the remembered width is untouched by it.
    expect(screen.getByTestId("column")).toHaveStyle({
      width: `${SURFACE_COLUMN_FOLDED_WIDTH}px`,
    });
    expect(readColumnWidths(document.cookie)[id]).toBe(chosen);
    // No seam while folded: there is nothing to size, and a drag on a strip
    // would write a width the fold is not showing.
    expect(screen.queryByRole("separator", { name: seam })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: expand }));
    expect(screen.getByTestId("column")).toHaveStyle({ width: `${chosen}px` });
  });

  it("survives a reload folded AND resized, then unfolds to the chosen width", () => {
    const { unmount } = render(<Host id={id} />);
    fireEvent.keyDown(screen.getByRole("separator", { name: seam }), { key: "ArrowRight" });
    fireEvent.click(screen.getByRole("button", { name: collapse }));
    const chosen = spec.defaultWidth + COLUMN_KEY_STEP;

    reload(id, unmount);

    expect(screen.getByTestId("column")).toHaveStyle({
      width: `${SURFACE_COLUMN_FOLDED_WIDTH}px`,
    });
    fireEvent.click(screen.getByRole("button", { name: expand }));
    expect(screen.getByTestId("column")).toHaveStyle({ width: `${chosen}px` });
  });

  it("offers neither control where the arrangement is not a row of columns", () => {
    // The phone stack shows one pane at a time. A fold there hides the whole
    // screen behind a 48px strip, and a seam is a drag target with nothing
    // beside it to trade width with.
    render(<Host id={id} enabled={false} />);

    expect(screen.queryByRole("button", { name: collapse })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: expand })).not.toBeInTheDocument();
    expect(screen.queryByRole("separator", { name: seam })).not.toBeInTheDocument();
    expect(screen.getByText(BODY)).toBeInTheDocument();
  });

  it("shows the body on the phone even when the desktop left this column folded", () => {
    // A remembered desktop fold must not follow the user onto a stack that has
    // no way to undo it.
    hydrateColumnFold(`${COLUMN_FOLD_COOKIE}=${encodeURIComponent(`${id}:1`)}`);

    render(<Host id={id} enabled={false} />);

    expect(screen.getByText(BODY)).toBeInTheDocument();
  });
});

describe("surface columns as a set", () => {
  it("gives every column a distinct name, so no two controls read alike", () => {
    // Two columns on one screen: "Collapse Notes" would name both the rail and
    // the list, and a screen reader user would have no way to tell them apart.
    const labels = SURFACE_COLUMN_IDS.map((id) => SURFACE_COLUMNS[id].label);
    expect(new Set(labels).size).toBe(labels.length);
  });

  it("folds one column without touching another", () => {
    render(
      <div className="flex">
        <Host id="notes-rail" />
        <Host id="notes-list" />
      </div>,
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["notes-rail"].label}`,
      }),
    );

    const fold = readColumnFold(document.cookie);
    expect(fold["notes-rail"]).toBe(true);
    expect(fold["notes-list"]).toBe(false);
    expect(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["notes-list"].label}`,
      }),
    ).toBeInTheDocument();
  });
});
