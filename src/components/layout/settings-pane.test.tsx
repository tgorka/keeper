import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/components/settings/settings-dialog", () => ({
  // The sections themselves are covered by `settings-dialog.test.tsx` and the
  // categories by `settings-categories.test.tsx`; what this file is about is
  // the pane around them — that it renders the shared body at all, hands it an
  // `open` it can hydrate from, picks the arrangement for the tier, and
  // carries no modal.
  SettingsBody: ({ open, layout }: { open: boolean; layout: string }) => (
    <div data-testid="settings-body" data-open={String(open)} data-layout={layout} />
  ),
}));

const shell = vi.hoisted(() => ({ phone: false }));
vi.mock("@/hooks/use-shell-layout", () => ({ useShellLayout: () => shell }));

import { SETTINGS_PANE_TITLE, SettingsPane } from "@/components/layout/settings-pane";

afterEach(() => {
  shell.phone = false;
});

describe("SettingsPane", () => {
  it("renders the shared settings body as an already-open surface", () => {
    render(<SettingsPane />);

    const body = screen.getByTestId("settings-body");
    // A mounted pane is unambiguously open: it exists only while it is the
    // active view. Passing `false` would leave every section unhydrated and the
    // pane permanently blank.
    expect(body).toHaveAttribute("data-open", "true");
  });

  it("is a pane, not a dialog", () => {
    render(<SettingsPane />);

    // The whole point of the change: no focus trap, no overlay over the app.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: SETTINGS_PANE_TITLE })).toBeInTheDocument();
  });

  it("names itself the way the other primary views do", () => {
    render(<SettingsPane />);

    expect(screen.getByRole("heading", { name: SETTINGS_PANE_TITLE })).toBeInTheDocument();
  });
});
