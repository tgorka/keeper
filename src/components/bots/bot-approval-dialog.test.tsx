/**
 * The approval sheet: three answers, and the safe one is the default (Epic 61,
 * Story 61.10, FR-387, AD-158).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **Deny is the default, and Escape denies.** A sheet dismissed without an
 *    answer has not been consented to — the same position the Rust approval
 *    port takes for the missing-UI case ("a host built with no approver
 *    declines every ask"). Remove the dismissal handler and the Escape test
 *    fails on its own.
 * 2. **Always-for-this-subtree is a durable grant edit.** It calls
 *    `bots_grant_save` with the *parent folder* of the path and the effect's
 *    own mode, and answers only after the write landed. Delete the save and
 *    the "always" test fails; make it answer before the save resolves and the
 *    refused-save test fails.
 * 3. **The always control is absent where it could not work.** A path at the
 *    top of a profile has no folder inside the profile to grant, and a
 *    profile-wide write grant asks every time by FR-387 — so an "always" there
 *    would be an affordance that lies (AD-27).
 * 4. **The reason is Rust's sentence, rendered from the payload.** Nothing in
 *    TypeScript writes the refusal, so the log and the sheet cannot disagree.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  APPROVAL_ALWAYS_ABSENT,
  APPROVAL_ALWAYS_LABEL,
  APPROVAL_ALWAYS_NOTE,
  APPROVAL_DENY_LABEL,
  APPROVAL_ONCE_LABEL,
  APPROVAL_TITLE,
  approvalSubtree,
  BotApprovalDialog,
  type BotApprovalRequest,
} from "@/components/bots/bot-approval-dialog";
import type { BotGrantSaveReq } from "@/lib/ipc/client";

const botsGrantSave = vi.fn();

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsGrantSave: (req: BotGrantSaveReq) => botsGrantSave(req),
  };
});

/** `grant::ASK_WRITE_WIDE_SCOPE`, as it arrives on the verdict. */
const ASK_REASON =
  "This bot may write here, and keeper asks before every write to a scope this wide. Approve this one, or grant the folder itself to stop being asked.";

const REQUEST: BotApprovalRequest = {
  requestId: "ask-1",
  providerId: "prov-1",
  botId: "bot-1",
  tool: "write",
  path: "p1/journal/2026/monday.md",
  profileId: "p1",
  subpath: "journal/2026/monday.md",
  effect: "write",
  reason: ASK_REASON,
};

const onAnswer = vi.fn();

beforeEach(() => {
  onAnswer.mockReset();
  botsGrantSave.mockReset();
  botsGrantSave.mockResolvedValue({ id: "grant-new" });
});

describe("approvalSubtree", () => {
  it("is the folder the path sits in", () => {
    expect(approvalSubtree("journal/2026/monday.md")).toBe("journal/2026");
  });

  it("is absent for a path at the top of a profile", () => {
    expect(approvalSubtree("monday.md")).toBeNull();
  });

  it("is absent for the profile root itself", () => {
    expect(approvalSubtree("")).toBeNull();
  });
});

describe("BotApprovalDialog", () => {
  it("renders nothing while nothing is being asked", () => {
    const { container } = render(<BotApprovalDialog request={null} onAnswer={onAnswer} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("names the tool, the exact path and the effect, and quotes Rust's sentence", () => {
    render(<BotApprovalDialog request={REQUEST} onAnswer={onAnswer} />);
    expect(screen.getByRole("alertdialog")).toHaveTextContent(APPROVAL_TITLE);
    expect(screen.getByText(ASK_REASON)).toBeInTheDocument();
    expect(screen.getByText("write — p1/journal/2026/monday.md")).toBeInTheDocument();
  });

  it("says in its own copy that always-for-this-folder saves a grant", () => {
    render(<BotApprovalDialog request={REQUEST} onAnswer={onAnswer} />);
    expect(screen.getByText(APPROVAL_ALWAYS_NOTE)).toBeInTheDocument();
  });

  it("focuses Deny when it opens", async () => {
    render(<BotApprovalDialog request={REQUEST} onAnswer={onAnswer} />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: APPROVAL_DENY_LABEL })).toHaveFocus(),
    );
  });

  it("denies when Deny is pressed", () => {
    render(<BotApprovalDialog request={REQUEST} onAnswer={onAnswer} />);
    fireEvent.click(screen.getByRole("button", { name: APPROVAL_DENY_LABEL }));
    expect(onAnswer).toHaveBeenCalledWith("ask-1", "deny");
    expect(botsGrantSave).not.toHaveBeenCalled();
  });

  it("denies on Escape, and saves no grant", () => {
    render(<BotApprovalDialog request={REQUEST} onAnswer={onAnswer} />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onAnswer).toHaveBeenCalledWith("ask-1", "deny");
    expect(botsGrantSave).not.toHaveBeenCalled();
  });

  it("approves once without touching the grant table", () => {
    render(<BotApprovalDialog request={REQUEST} onAnswer={onAnswer} />);
    fireEvent.click(screen.getByRole("button", { name: APPROVAL_ONCE_LABEL }));
    expect(onAnswer).toHaveBeenCalledWith("ask-1", "once");
    expect(botsGrantSave).not.toHaveBeenCalled();
  });

  it("saves a write grant on the path's own folder for always-for-this-subtree", async () => {
    render(<BotApprovalDialog request={REQUEST} onAnswer={onAnswer} />);
    fireEvent.click(screen.getByRole("button", { name: /^Always for this folder/ }));
    await waitFor(() =>
      expect(botsGrantSave).toHaveBeenCalledWith({
        id: null,
        providerId: "prov-1",
        botId: "bot-1",
        scope: { kind: "subtree", profileId: "p1", subpath: "journal/2026" },
        mode: "write",
      } satisfies BotGrantSaveReq),
    );
    await waitFor(() => expect(onAnswer).toHaveBeenCalledWith("ask-1", "always"));
  });

  it("names the subtree it would grant on the control itself", () => {
    render(<BotApprovalDialog request={REQUEST} onAnswer={onAnswer} />);
    expect(
      screen.getByRole("button", { name: `${APPROVAL_ALWAYS_LABEL} — journal/2026` }),
    ).toBeInTheDocument();
  });

  it("does not approve when the grant write was refused", async () => {
    botsGrantSave.mockRejectedValue({ message: "keeper could not write the grant" });
    render(<BotApprovalDialog request={REQUEST} onAnswer={onAnswer} />);
    fireEvent.click(screen.getByRole("button", { name: /^Always for this folder/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("keeper could not write the grant");
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it("omits always, and says why, for a path at the top of a profile", () => {
    render(
      <BotApprovalDialog
        request={{ ...REQUEST, path: "p1/monday.md", subpath: "monday.md" }}
        onAnswer={onAnswer}
      />,
    );
    expect(screen.queryByRole("button", { name: /^Always for this folder/ })).toBeNull();
    expect(screen.getByText(APPROVAL_ALWAYS_ABSENT)).toBeInTheDocument();
    // The other two answers are still there: this is a narrowing, not a dead end.
    expect(screen.getByRole("button", { name: APPROVAL_ONCE_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: APPROVAL_DENY_LABEL })).toBeInTheDocument();
  });

  it("grants read, not write, for a read that was asked about", async () => {
    render(
      <BotApprovalDialog
        request={{ ...REQUEST, effect: "read", tool: "read" }}
        onAnswer={onAnswer}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /^Always for this folder/ }));
    await waitFor(() =>
      expect(botsGrantSave).toHaveBeenCalledWith(
        expect.objectContaining({ mode: "read" }) as BotGrantSaveReq,
      ),
    );
  });
});
