import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { incognitoChipLabel } from "@/components/layout/conversation-pane";
import type { IncognitoVm } from "@/lib/ipc/client";

describe("incognitoChipLabel", () => {
  it("labels a per-chat override as overriding the account", () => {
    expect(incognitoChipLabel("chat")).toBe("Incognito — this chat overrides account");
  });

  it("labels a per-account source", () => {
    expect(incognitoChipLabel("account")).toBe("Incognito — account");
  });

  it("labels a global source", () => {
    expect(incognitoChipLabel("global")).toBe("Incognito — global");
  });

  it("covers all three IncognitoVm sources", () => {
    const sources: IncognitoVm["source"][] = ["global", "account", "chat"];
    for (const source of sources) {
      // Every source yields a distinct, non-empty violet-chip label.
      expect(incognitoChipLabel(source)).toMatch(/^Incognito — /);
    }
    render(<span>{incognitoChipLabel("chat")}</span>);
    expect(screen.getByText("Incognito — this chat overrides account")).toBeInTheDocument();
  });
});
