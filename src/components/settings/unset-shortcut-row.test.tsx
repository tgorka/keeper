import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  frontendErrorReport: vi.fn(async () => undefined),
}));

import { UnsetShortcutRow } from "@/components/settings/unset-shortcut-row";

describe("UnsetShortcutRow", () => {
  it("says a binding it could not read, instead of loading forever (R387)", async () => {
    render(
      <UnsetShortcutRow
        open
        label="Start / stop recording"
        noun="recording"
        settingKey="hotkey.recording"
        read={() =>
          Promise.reject({
            code: "internal",
            message: "keeper could not read config.json.",
            accountId: null,
            retriable: true,
          })
        }
        set={vi.fn()}
        clear={vi.fn()}
        notRegistered="Not registered."
      />,
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "keeper could not read config.json.",
    );
    expect(screen.queryByLabelText("Loading shortcut")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Change recording shortcut" })).toBeDisabled();
  });
});
