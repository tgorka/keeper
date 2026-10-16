/**
 * The decisions panel above an agent room's composer (Story 101.3, R418, R430):
 * which decisions it offers, that it approves nothing it does not show, that a
 * held answer is no answer, and that its keys send exactly what the card's
 * buttons — or typing the answer — would send, never on one keystroke.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcClient from "@/lib/ipc/client";
import type { AgentAsksVm, AgentAskVm, ApprovalCardVm, ApprovalVm } from "@/lib/ipc/client";

const agentApprovalDecide = vi.fn();
const agentApprovalPayload = vi.fn();
const sendText = vi.fn();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcClient>();
  return {
    ...actual,
    agentApprovalDecide: (...args: unknown[]) => agentApprovalDecide(...args),
    agentApprovalPayload: (...args: unknown[]) => agentApprovalPayload(...args),
    sendText: (...args: unknown[]) => sendText(...args),
  };
});

import {
  DECISION_DEFAULT_MARK,
  DECISION_HELD,
  DECISION_SHOW_RUN_FIRST,
  decisionsOf,
  optionsOf,
  settlingOf,
} from "@/components/agents/decisions";
import {
  DECISION_SEND_FAILED,
  DECISIONS_LABEL,
  DecisionsPanel,
} from "@/components/agents/decisions-panel";

const ACCOUNT = "acc-1";
const ROOM = "!nixi-dm:example.org";
const HARNESS = { user: "@harness:example.org", name: "harness" };
const NIXI = { user: "@nixi:example.org", name: "Nixi" };

function card(over: Partial<ApprovalCardVm> = {}): ApprovalCardVm {
  return {
    id: "01A",
    bindingDigest: "sha256:01a",
    tier: 2,
    tierWord: "T2",
    summary: "Write `notes/reading.md` in tgdrive",
    tool: "drive_write",
    payload: '{\n  "path": "notes/reading.md",\n  "text": "Dune, Solaris"\n}',
    attachment: null,
    approvers: [HARNESS],
    anyone: false,
    chain: [HARNESS, NIXI],
    scopes: [
      { scope: "once", label: "Approve once", detail: "This action once." },
      {
        scope: "session",
        label: "Approve for this session",
        detail: "Every write like this one, until the session closes.",
      },
    ],
    expiresAt: 4_000_000_000_000,
    state: { state: "pending" },
    canDecide: true,
    cannotDecide: null,
    verify: false,
    only: null,
    declassify: null,
    run: null,
    kvm: null,
    gate: null,
    ...over,
  };
}

const RUN = {
  argv: ["git", "-c", "core.hooksPath=/dev/null", "fetch", "origin"],
  program: "/usr/bin/git · 4f1c9a0b7e22",
  wrappers: [],
  wrapped: null,
  cwd: "tgdrive/code/keeper",
  reads: [],
  timeoutS: 120,
  held: [],
  network: null,
};

const request = (...cards: ApprovalCardVm[]): ApprovalVm => ({
  id: cards[0].id,
  cards,
  gate: null,
});

const ASK: AgentAskVm = {
  id: "01ASK",
  roomId: "!tola:example.org",
  askedBy: "Tola Grey",
  question: "Which trip should I plan first?",
  choices: ["Tatry", "Bieszczady", "Mazury"],
  default: "Tatry",
  askedAt: 1,
  state: "open",
};

const asked = (asks: AgentAskVm[], note: string | null = null): AgentAsksVm => ({ asks, note });

function panel(approvals: ApprovalVm[], asks: AgentAsksVm) {
  const scope = createRef<HTMLDivElement>();
  return (
    <div ref={scope}>
      <DecisionsPanel
        accountId={ACCOUNT}
        roomId={ROOM}
        approvals={approvals}
        asks={asks}
        scope={scope}
      />
      <textarea aria-label="Message" />
    </div>
  );
}

function draw(approvals: ApprovalVm[], asks: AgentAsksVm) {
  const view = render(panel(approvals, asks));
  return {
    field: screen.getByLabelText<HTMLTextAreaElement>("Message"),
    redraw: (next: AgentAsksVm) => view.rerender(panel(approvals, next)),
  };
}

const options = () => within(screen.getByRole("listbox")).getAllByRole("option");
const labels = () => options().map((o) => o.textContent);
const selected = () => options().findIndex((o) => o.getAttribute("aria-selected") === "true");
const region = () => screen.getByRole("region", { name: DECISIONS_LABEL });

beforeEach(() => {
  agentApprovalDecide.mockReset().mockResolvedValue(undefined);
  agentApprovalPayload.mockReset();
  sendText.mockReset().mockResolvedValue(undefined);
});

describe("decisionsOf", () => {
  it("offers pending approvals this device may decide, then the questions still open", () => {
    const decided = card({
      id: "01B",
      state: { state: "decided", decision: "approve", scope: "once", by: "x", byName: "x" },
    });
    const elsewhere = card({ id: "01C", canDecide: false });
    const held = { ...ASK, id: "01HELD", state: "held" as const };
    const asks = [ASK, held];
    const decisions = decisionsOf([request(card(), decided, elsewhere)], asks, new Set());
    expect(decisions.map((d) => d.id)).toEqual(["01A", "01ASK"]);
    expect(optionsOf(decisions[0], asks, false).map((o) => o.label)).toEqual([
      "Approve once",
      "Approve for this session",
      "Deny",
    ]);
    expect(optionsOf(decisions[1], asks, false).map((o) => [o.label, o.detail])).toEqual([
      ["Tatry", DECISION_DEFAULT_MARK],
      ["Bieszczady", null],
      ["Mazury", null],
    ]);
    expect(settlingOf(asks).map((s) => [s.id, s.state])).toEqual([["01HELD", "held"]]);
    expect(decisionsOf([request(card())], [ASK], new Set(["01A"])).map((d) => d.id)).toEqual([
      "01ASK",
    ]);
  });

  it("offers an attached run's approval only once its action is shown", () => {
    const run = card({ tool: "run", attachment: "attached sha256:aa", payload: null });
    const [only] = decisionsOf([request(run)], [], new Set());
    expect(optionsOf(only, [], false).map((o) => o.label)).toEqual(["Deny"]);
    expect(optionsOf(only, [], true).map((o) => o.label)).toEqual([
      "Approve once",
      "Approve for this session",
      "Deny",
    ]);
  });
});

describe("DecisionsPanel", () => {
  it("picks with a number key from an empty field, and sends only on Enter", async () => {
    const { field } = draw([], asked([ASK]));
    field.focus();
    fireEvent.keyDown(field, { key: "2" });
    expect(selected()).toBe(1);
    expect(screen.getByRole("listbox")).toHaveFocus();
    expect(sendText).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByRole("listbox"), { key: "Enter" });
    await waitFor(() => expect(sendText).toHaveBeenCalledWith(ACCOUNT, ROOM, "Bieszczady"));
  });

  it("leaves a number key to a field that holds text", () => {
    const { field } = draw([], asked([ASK]));
    fireEvent.change(field, { target: { value: "I think" } });
    field.focus();
    const handled = !fireEvent.keyDown(field, { key: "3" });
    expect(handled).toBe(false);
    expect(selected()).toBe(0);
    expect(field).toHaveFocus();
  });

  it("leaves a number key to an input method choosing a candidate (R430)", () => {
    const { field } = draw([], asked([ASK]));
    field.focus();
    const handled = !fireEvent.keyDown(field, { key: "2", isComposing: true });
    expect(handled).toBe(false);
    expect(selected()).toBe(0);
    expect(field).toHaveFocus();
  });

  it("moves with the arrows and decides the approval the card would", async () => {
    draw([request(card())], asked([]));
    const list = screen.getByRole("listbox");
    fireEvent.keyDown(list, { key: "ArrowUp" });
    expect(selected()).toBe(2);
    fireEvent.keyDown(list, { key: "Enter" });
    await waitFor(() =>
      expect(agentApprovalDecide).toHaveBeenCalledWith(ACCOUNT, ROOM, {
        id: "01A",
        bindingDigest: "sha256:01a",
        decision: "deny",
        scope: "once",
        note: null,
      }),
    );
    await waitFor(() => expect(screen.queryByRole("region", { name: DECISIONS_LABEL })).toBeNull());
  });

  it("shows the action an approval releases above its options (R430 01)", () => {
    const cases: Array<[string, ApprovalCardVm, string[]]> = [
      ["a drive write", card(), ['"text": "Dune, Solaris"']],
      [
        "an inline run",
        card({ tool: "run", payload: null, run: RUN, summary: "Run `git fetch` in keeper" }),
        ["core.hooksPath=/dev/null", "/usr/bin/git · 4f1c9a0b7e22", "tgdrive/code/keeper"],
      ],
      [
        "a KVM action",
        card({
          tool: "kvm_act",
          payload: null,
          kvm: { kvm: "hesperia-kvm", inputs: ["Click at 120, 40", "Type 12 characters"] },
        }),
        ["hesperia-kvm", "Type 12 characters"],
      ],
      [
        "a declassification",
        card({
          tool: "declassify",
          tier: 4,
          tierWord: "T4",
          only: "Only harness decides this, and it cannot be undone.",
          declassify: {
            readers: [{ user: "@lucyna:example.org", name: "Dr Lucyna Novak" }],
            what: "the brief",
            sha256: "ab".repeat(32),
            sentence: "Approving lets Dr Lucyna Novak read the brief: exactly these bytes, once.",
          },
        }),
        [
          "Let this one message reach Dr Lucyna Novak?",
          "exactly these bytes, once",
          "it cannot be undone",
        ],
      ],
    ];
    for (const [name, shown, words] of cases) {
      const view = render(panel([request(shown)], asked([])));
      const body = within(region());
      for (const word of words) {
        expect(body.getByText(word, { exact: false }), `${name}: ${word}`).toBeInTheDocument();
      }
      // The action stands above the options, never below them.
      const action = body.getByText(words[0], { exact: false });
      expect(
        action.compareDocumentPosition(body.getByRole("listbox")) &
          Node.DOCUMENT_POSITION_FOLLOWING,
        name,
      ).toBeTruthy();
      view.unmount();
    }
  });

  it("offers an attached run's approval once its full action is shown in the panel", async () => {
    agentApprovalPayload.mockResolvedValue({ text: "", run: RUN, kvm: null });
    draw(
      [request(card({ tool: "run", payload: null, attachment: "attached sha256:aa" }))],
      asked([]),
    );
    expect(labels()).toEqual(["1Deny"]);
    expect(screen.getByText(DECISION_SHOW_RUN_FIRST)).toBeInTheDocument();
    fireEvent.click(within(region()).getByRole("button", { name: "Show the full action" }));
    await waitFor(() => expect(options()).toHaveLength(3));
    expect(within(region()).getByText("core.hooksPath=/dev/null", { exact: false })).toBeVisible();
    expect(agentApprovalPayload).toHaveBeenCalledWith(ACCOUNT, ROOM, "01A");
  });

  it("shows each attached approval's own action as the panel advances (R447)", async () => {
    const other = {
      ...RUN,
      argv: ["cargo", "publish", "--allow-dirty"],
      cwd: "tgdrive/code/other",
    };
    agentApprovalPayload.mockImplementation(async (_a: string, _r: string, id: string) => ({
      text: "",
      run: id === "01A" ? RUN : other,
      kvm: null,
    }));
    const attached = (id: string, digest: string) =>
      card({
        id,
        bindingDigest: digest,
        tool: "run",
        payload: null,
        attachment: `attached ${digest}`,
      });
    draw([request(attached("01A", "sha256:aa")), request(attached("01B", "sha256:bb"))], asked([]));
    fireEvent.click(within(region()).getByRole("button", { name: "Show the full action" }));
    await waitFor(() => expect(options()).toHaveLength(3));
    expect(within(region()).getByText("core.hooksPath=/dev/null", { exact: false })).toBeVisible();
    fireEvent.click(options()[0]);
    await waitFor(() =>
      expect(agentApprovalDecide).toHaveBeenCalledWith(
        ACCOUNT,
        ROOM,
        expect.objectContaining({ id: "01A", bindingDigest: "sha256:aa" }),
      ),
    );
    // The next approval: none of the first one's action, and deny-only until its own is shown.
    await waitFor(() => expect(within(region()).getByText("attached sha256:bb")).toBeVisible());
    expect(within(region()).queryByText("core.hooksPath=/dev/null", { exact: false })).toBeNull();
    expect(labels()).toEqual(["1Deny"]);
    const show = within(region()).getByRole("button", { name: "Show the full action" });
    expect(show).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(show);
    await waitFor(() => expect(options()).toHaveLength(3));
    expect(agentApprovalPayload).toHaveBeenLastCalledWith(ACCOUNT, ROOM, "01B");
    expect(within(region()).getByText("--allow-dirty", { exact: false })).toBeVisible();
  });

  it("keeps a held answer's question, and gives it back when the answer is undone (R430 05)", async () => {
    const { redraw } = draw([], asked([ASK]));
    fireEvent.keyDown(screen.getByRole("listbox"), { key: "Enter" });
    await waitFor(() => expect(sendText).toHaveBeenCalledWith(ACCOUNT, ROOM, "Tatry"));
    // Still open as far as Rust says: the panel never decides it answered.
    expect(options()).toHaveLength(3);
    redraw(asked([{ ...ASK, state: "held" }]));
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(within(region()).getByText(DECISION_HELD, { exact: false })).toBeInTheDocument();
    // Undo: the held row is gone and Rust reads the question open again.
    redraw(asked([ASK]));
    expect(labels()).toEqual(["1Tatrythe default", "2Bieszczady", "3Mazury"]);
  });

  it("says beside the panel what keeper could not read (R381)", () => {
    const partial = "keeper could not read every room a question for you may wait in.";
    draw([], asked([], partial));
    expect(within(region()).getByRole("status")).toHaveTextContent(partial);
  });

  it("hides to one line on Escape, where number keys no longer pick", () => {
    const { field } = draw([], asked([ASK]));
    fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(screen.getByRole("button", { name: "Show" })).toHaveAttribute("aria-expanded", "false");
    field.focus();
    expect(fireEvent.keyDown(field, { key: "1" })).toBe(true);
  });

  it("says a failed send where the person looks and keeps the decision", async () => {
    sendText.mockRejectedValue({ code: "sendFailed", message: "nope" });
    draw([], asked([ASK]));
    fireEvent.keyDown(screen.getByRole("listbox"), { key: "Enter" });
    expect(await screen.findByRole("alert")).toHaveTextContent(DECISION_SEND_FAILED);
    expect(options()).toHaveLength(3);
  });
});
