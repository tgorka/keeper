import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApprovalCardVm, ApprovalVm } from "@/lib/ipc/client";

const agentApprovalDecide = vi.fn();
const agentApprovalPayload = vi.fn();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    agentApprovalDecide: (...args: unknown[]) => agentApprovalDecide(...args),
    agentApprovalPayload: (...args: unknown[]) => agentApprovalPayload(...args),
  };
});

import {
  APPROVAL_SENT,
  ApprovalRequest,
  approvalListName,
} from "@/components/agents/approval-card";
import { verificationStore } from "@/lib/stores/verification";

const ACCOUNT = "acc-1";
const ROOM = "!reading:example.org";
const HARNESS = { user: "@harness:example.org", name: "harness" };
const MARTA = { user: "@marta:example.org", name: "Marta" };
const NIXI = { user: "@nixi:example.org", name: "Nixi" };

function card(over: Partial<ApprovalCardVm> = {}): ApprovalCardVm {
  return {
    id: "01A",
    bindingDigest: "sha256:01a",
    tier: 2,
    tierWord: "T2: it changes something that can be put back",
    summary: "Write `notes/reading.md` in tgdrive (412 bytes)",
    tool: "drive_write",
    payload: '{\n  "path": "notes/reading.md"\n}',
    attachment: null,
    approvers: [HARNESS, MARTA],
    anyone: false,
    chain: [HARNESS, NIXI],
    scopes: [
      { scope: "once", label: "Approve once", detail: "This action once, exactly as shown." },
      {
        scope: "session",
        label: "Approve for this session",
        detail: "drive_write in tgdrive under notes/, until this session closes, at most 24 hours.",
      },
    ],
    expiresAt: 4_000_000_000_000,
    state: { state: "pending" },
    canDecide: true,
    cannotDecide: null,
    verify: false,
    only: null,
    declassify: null,
    ...over,
  };
}

function draw(...cards: ApprovalCardVm[]) {
  const approval: ApprovalVm = { id: cards[0].id, cards };
  return render(<ApprovalRequest approval={approval} accountId={ACCOUNT} roomId={ROOM} />);
}

const theCard = () => screen.getByRole("article", { name: /^Approval request: / });
const decideButtons = () => screen.queryAllByRole("button", { name: /^(Approve|Deny|Send deny)/ });

beforeEach(() => {
  agentApprovalDecide.mockReset();
  agentApprovalPayload.mockReset();
  verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
});

afterEach(() => {
  verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
});

describe("ApprovalRequest — what the card shows (UX-DR136)", () => {
  it("shows the tier, keeper's summary, the exact action, who asked and who decides", () => {
    draw(card());
    const shown = theCard();
    expect(shown).toHaveAccessibleName(
      "Approval request: Write notes/reading.md in tgdrive (412 bytes)",
    );
    expect(within(shown).getByText("T2: it changes something that can be put back")).toBeVisible();
    expect(
      within(shown).getByRole("figure", { name: "What will run: drive_write" }),
    ).toHaveTextContent('"path": "notes/reading.md"');
    expect(within(shown).getByText("harness → Nixi")).toBeInTheDocument();
    expect(within(shown).getByText("harness, Marta")).toBeInTheDocument();
    expect(within(shown).getByText("Waiting")).toBeInTheDocument();
  });

  it("keeps a long action whole: a scroll box that says how many lines, never cut", () => {
    const long = JSON.stringify(
      { content: Array.from({ length: 40 }, (_, n) => `line ${n}`) },
      null,
      2,
    );
    draw(card({ payload: long }));
    const lines = long.split("\n").length;
    const toggle = screen.getByRole("button", { name: `Show all ${lines} lines` });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("figure").querySelector("pre")?.textContent).toBe(long);
    fireEvent.click(toggle);
    expect(screen.getByRole("button", { name: "Show less" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByRole("figure").querySelector("pre")?.textContent).toBe(long);
  });

  it("folds one very long line too, saying how many characters", () => {
    const long = JSON.stringify({ content: "x".repeat(3000) }, null, 2);
    draw(card({ payload: long }));
    expect(
      screen.getByRole("button", { name: `Show all ${long.length} characters` }),
    ).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("figure").querySelector("pre")?.textContent).toBe(long);
  });

  it("draws a short action open, with nothing to show more of", () => {
    draw(card());
    expect(screen.queryByRole("button", { name: /^Show all/ })).toBeNull();
  });

  it("exposes each offered grant boundary and lifetime before approving", async () => {
    const offered = card();
    agentApprovalDecide.mockResolvedValue(undefined);
    draw(offered);
    for (const offer of offered.scopes) {
      expect(screen.getByRole("button", { name: offer.label })).toHaveAccessibleDescription(
        offer.detail,
      );
    }
    fireEvent.click(screen.getByRole("button", { name: offered.scopes[1].label }));
    await screen.findByText(APPROVAL_SENT);
    expect(agentApprovalDecide.mock.calls[0][2].scope).toBe("session");
  });

  it("asks a declassification's own question", () => {
    draw(
      card({
        tier: 3,
        summary: "Let @marta:example.org read the brief (9f2c4be1a7d0)",
        tool: "declassify",
        declassify: {
          readers: [MARTA],
          what: "the brief",
          sha256: "9f2c",
          sentence: "Approving lets Marta read the brief: exactly these bytes, once.",
        },
      }),
    );
    expect(theCard()).toHaveAccessibleName("Approval request: Let this one message reach Marta?");
    expect(
      screen.getByText("Approving lets Marta read the brief: exactly these bytes, once."),
    ).toBeVisible();
  });

  it.each([
    "approve",
    "deny",
  ] as const)("keeps a room's %s decision actionable until the host uses it", async (decision) => {
    agentApprovalDecide.mockResolvedValue(undefined);
    draw(
      card({
        state: { state: "decided", decision, scope: "once", by: MARTA.user, byName: "Marta" },
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Approve once" }));
    await screen.findByText(APPROVAL_SENT);
    expect(agentApprovalDecide.mock.calls[0][2].decision).toBe("approve");
  });

  it("an approval that expires before use closes without erasing the earlier decision", () => {
    const approved = card({
      state: {
        state: "decided",
        decision: "approve",
        scope: "once",
        by: MARTA.user,
        byName: "Marta",
      },
    });
    const { rerender } = draw(approved);
    expect(screen.getByRole("button", { name: "Approve once" })).toBeEnabled();
    rerender(
      <ApprovalRequest
        accountId={ACCOUNT}
        roomId={ROOM}
        approval={{
          id: approved.id,
          cards: [{ ...approved, state: { state: "expired" }, canDecide: false }],
        }}
      />,
    );
    expect(decideButtons()).toEqual([]);
    // The expiry is about use, not whether the person made a decision.
    expect(screen.getByRole("status")).toHaveTextContent(/before.*used/i);
    expect(screen.getByRole("status")).not.toHaveTextContent(/no decision|nobody decided/i);
  });

  it("consumption closes the approval without confirming the effect", () => {
    const pending = card();
    const { rerender } = draw(pending);
    rerender(
      <ApprovalRequest
        accountId={ACCOUNT}
        roomId={ROOM}
        approval={{
          id: pending.id,
          cards: [{ ...pending, state: { state: "consumed" }, canDecide: false }],
        }}
      />,
    );
    expect(decideButtons()).toEqual([]);
    const outcome = screen.getByRole("status");
    expect(outcome).toHaveTextContent(/used once/i);
    expect(outcome).toHaveTextContent(/does not confirm.*outcome/i);
    expect(outcome).toHaveTextContent(/session/i);
    expect(outcome).not.toHaveTextContent(/went ahead|executed|succeeded/i);
  });
});

describe("ApprovalRequest — deciding", () => {
  it.each([
    "You are not one of the people who can decide this.",
    "keeper cannot decide this here: what the card shows is not what was sent for approval.",
  ])("draws no decide controls and preserves Rust's refusal: %s", (cannotDecide) => {
    draw(card({ canDecide: false, cannotDecide }));
    expect(decideButtons()).toEqual([]);
    expect(screen.getByText(cannotDecide)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Verify this device" })).toBeNull();
  });

  it("an approval carries the card's id, digest and the chosen scope", async () => {
    agentApprovalDecide.mockResolvedValue(undefined);
    draw(card());
    fireEvent.click(screen.getByRole("button", { name: "Approve for this session" }));
    expect(await screen.findByText(APPROVAL_SENT)).toBeInTheDocument();
    expect(agentApprovalDecide).toHaveBeenCalledTimes(1);
    expect(agentApprovalDecide).toHaveBeenCalledWith(ACCOUNT, ROOM, {
      id: "01A",
      bindingDigest: "sha256:01a",
      decision: "approve",
      scope: "session",
      note: null,
    });
    // Sent once: the card waits for the room, not for another press.
    expect(decideButtons()).toEqual([]);
  });

  it("a deny carries the person's note, once only", async () => {
    agentApprovalDecide.mockResolvedValue(undefined);
    draw(card());
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    fireEvent.change(screen.getByLabelText("Note for the agent (optional)"), {
      target: { value: "  not this file  " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send deny" }));
    await screen.findByText(APPROVAL_SENT);
    expect(agentApprovalDecide).toHaveBeenCalledWith(ACCOUNT, ROOM, {
      id: "01A",
      bindingDigest: "sha256:01a",
      decision: "deny",
      scope: "once",
      note: "not this file",
    });
  });

  it("a deny with an empty note sends none", async () => {
    agentApprovalDecide.mockResolvedValue(undefined);
    draw(card());
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    fireEvent.click(screen.getByRole("button", { name: "Send deny" }));
    await screen.findByText(APPROVAL_SENT);
    expect(agentApprovalDecide.mock.calls[0][2]).toMatchObject({ decision: "deny", note: null });
  });

  it("shows a refusal in keeper's words and stays decidable", async () => {
    agentApprovalDecide.mockRejectedValueOnce({
      code: "unsupported",
      message: "This approval is no longer waiting for a decision.",
    });
    draw(card());
    fireEvent.click(screen.getByRole("button", { name: "Approve once" }));
    expect(
      await screen.findByText("This approval is no longer waiting for a decision."),
    ).toBeInTheDocument();
    expect(screen.queryByText(APPROVAL_SENT)).toBeNull();
    const again = screen.getByRole("button", { name: "Approve once" });
    expect(again).toBeEnabled();
    agentApprovalDecide.mockResolvedValueOnce(undefined);
    fireEvent.click(again);
    expect(await screen.findByText(APPROVAL_SENT)).toBeInTheDocument();
    expect(agentApprovalDecide).toHaveBeenCalledTimes(2);
  });

  it("a coalesced request decides one row at a time", async () => {
    agentApprovalDecide.mockResolvedValue(undefined);
    draw(
      card({ id: "01G1", bindingDigest: "sha256:g1", summary: "Hand work to Tola (ticket 1)" }),
      card({ id: "01G2", bindingDigest: "sha256:g2", summary: "Hand work to Tola (ticket 2)" }),
      card({
        id: "01G3",
        bindingDigest: "sha256:g3",
        summary: "Hand work to Tola (ticket 3)",
        canDecide: false,
        state: {
          state: "decided",
          decision: "approve",
          scope: "once",
          by: HARNESS.user,
          byName: "harness",
        },
      }),
    );
    const list = screen.getByRole("region", { name: approvalListName(3) });
    const rows = within(list).getAllByRole("article");
    expect(rows).toHaveLength(3);
    fireEvent.click(within(rows[1]).getByRole("button", { name: "Approve once" }));
    expect(await within(rows[1]).findByText(APPROVAL_SENT)).toBeInTheDocument();
    expect(agentApprovalDecide).toHaveBeenCalledTimes(1);
    expect(agentApprovalDecide.mock.calls[0][2]).toMatchObject({
      id: "01G2",
      bindingDigest: "sha256:g2",
    });
    expect(within(rows[0]).getByRole("button", { name: "Approve once" })).toBeEnabled();
    expect(within(rows[2]).getByText("Approved once by harness.")).toBeInTheDocument();
    expect(within(rows[2]).queryByRole("button", { name: "Approve once" })).toBeNull();
  });

  it("keeps a deny draft when the second record arrives", () => {
    const first = card();
    const { rerender } = draw(first);
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    fireEvent.change(screen.getByLabelText("Note for the agent (optional)"), {
      target: { value: "Keep this file" },
    });
    rerender(
      <ApprovalRequest
        accountId={ACCOUNT}
        roomId={ROOM}
        approval={{ id: first.id, cards: [first, card({ id: "02A" })] }}
      />,
    );
    expect(screen.getByLabelText("Note for the agent (optional)")).toHaveValue("Keep this file");
  });

  it("keeps an in-flight refusal on the first record when a second arrives", async () => {
    let refuse!: (error: unknown) => void;
    agentApprovalDecide.mockReturnValue(
      new Promise((_resolve, reject) => {
        refuse = reject;
      }),
    );
    const first = card();
    const { rerender } = draw(first);
    fireEvent.click(screen.getByRole("button", { name: "Approve once" }));
    rerender(
      <ApprovalRequest
        accountId={ACCOUNT}
        roomId={ROOM}
        approval={{ id: first.id, cards: [first, card({ id: "02A" })] }}
      />,
    );
    const row = within(screen.getAllByRole("article")[0]);
    expect(row.getByRole("button", { name: "Approve once" })).toBeDisabled();
    await act(async () => refuse({ message: "The action changed; read the new request." }));
    expect(await row.findByText("The action changed; read the new request.")).toBeVisible();
    expect(agentApprovalDecide).toHaveBeenCalledTimes(1);
  });

  it("does not resubmit a sent decision when the second record arrives", async () => {
    agentApprovalDecide.mockResolvedValue(undefined);
    const first = card();
    const { rerender } = draw(first);
    fireEvent.click(screen.getByRole("button", { name: "Approve once" }));
    await screen.findByText(APPROVAL_SENT);
    rerender(
      <ApprovalRequest
        accountId={ACCOUNT}
        roomId={ROOM}
        approval={{ id: first.id, cards: [first, card({ id: "02A" })] }}
      />,
    );
    const row = within(screen.getAllByRole("article")[0]);
    expect(row.queryByRole("button", { name: "Approve once" })).toBeNull();
    expect(row.getByRole("status")).toHaveTextContent(APPROVAL_SENT);
    expect(agentApprovalDecide).toHaveBeenCalledTimes(1);
  });
});

describe("ApprovalRequest — a device that cannot decide", () => {
  it("an unverified device says why and opens this account's verification", () => {
    const sentence =
      "This device cannot decide: it is not verified. Verify it from another of your devices in Settings › Encryption, then decide here.";
    draw(card({ canDecide: false, cannotDecide: sentence, verify: true }));
    expect(decideButtons()).toEqual([]);
    expect(screen.getByText(sentence)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Verify this device" }));
    expect(verificationStore.getState().modalOpen).toBe(true);
    expect(verificationStore.getState().activeAccountId).toBe(ACCOUNT);
  });

  it("at T4 everyone reads who alone decides, said once", () => {
    draw(
      card({
        tier: 4,
        tierWord: "T4: it cannot be undone",
        chain: [MARTA, NIXI],
        only: "Only Marta can decide this. This cannot be undone.",
        canDecide: false,
        cannotDecide: "Only Marta can decide this.",
      }),
    );
    expect(screen.getByText("Only Marta can decide this. This cannot be undone.")).toBeVisible();
    expect(screen.queryByText("Only Marta can decide this.")).toBeNull();
    expect(decideButtons()).toEqual([]);
  });

  it("at T4 on the host's own app, it says to decide elsewhere", () => {
    const elsewhere =
      "Decide on another device: this app runs the agent that asks, so it cannot be the one that agrees.";
    draw(
      card({
        tier: 4,
        only: "Only harness can decide this. This cannot be undone.",
        canDecide: false,
        cannotDecide: elsewhere,
      }),
    );
    expect(screen.getByText("Only harness can decide this. This cannot be undone.")).toBeVisible();
    expect(screen.getByText(elsewhere)).toBeVisible();
    expect(decideButtons()).toEqual([]);
  });
});

describe("ApprovalRequest — an attached action", () => {
  const attached = () =>
    card({
      payload: null,
      attachment:
        "The action is too large to show here: it is attached to the request, whole. SHA-256 3b7e",
    });

  it("fetches the whole action on request and shows it", async () => {
    const whole = '{\n  "path": "notes/archive.md"\n}';
    agentApprovalPayload.mockResolvedValue(whole);
    draw(attached());
    expect(screen.queryByRole("figure")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show the full action" }));
    await waitFor(() =>
      expect(screen.getByRole("figure").querySelector("pre")?.textContent).toBe(whole),
    );
    expect(agentApprovalPayload).toHaveBeenCalledWith(ACCOUNT, ROOM, "01A");
    expect(screen.getByRole("button", { name: "Hide the full action" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  it("shows why it cannot, and leaves the decision to the card", async () => {
    agentApprovalPayload.mockRejectedValue({
      code: "unsupported",
      message: "keeper could not check the attached action against the request's digest.",
    });
    draw(attached());
    fireEvent.click(screen.getByRole("button", { name: "Show the full action" }));
    expect(
      await screen.findByText(
        "keeper could not check the attached action against the request's digest.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("figure")).toBeNull();
    expect(screen.getByRole("button", { name: "Approve once" })).toBeEnabled();
  });
});
