import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { DatabaseApprovalCard } from "@/components/agents/approval-card";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentApprovalVm, AgentDriveWriteVm } from "@/lib/ipc/client";

const mock = vi.hoisted(() => ({ decide: vi.fn() }));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentSessionApprovalDecide: mock.decide,
}));

beforeEach(() => {
  mock.decide
    .mockReset()
    .mockResolvedValue({ state: "sent", sentence: "Approved and written as 1a2b3c4." });
});

const EDIT: AgentDriveWriteVm = {
  op: "edit",
  drive: "tgdrive",
  path: "10-notes/inbox.md",
  untrusted: false,
  preview: {
    hunks: [
      {
        oldStart: 4,
        newStart: 4,
        lines: [
          { kind: "context", text: "# Inbox" },
          { kind: "remove", text: "- [ ] milk" },
          { kind: "add", text: "- [x] milk" },
        ],
      },
    ],
  },
};

function card(write: Partial<AgentDriveWriteVm>, patch: Partial<AgentApprovalVm> = {}) {
  const approval: AgentApprovalVm = {
    id: "ap1",
    database: "tgdrive",
    sessionId: "s.1",
    toolName: "drive_write",
    tier: "T3",
    summary: "Edit `10-notes/inbox.md` in tgdrive: 11 bytes become 11 bytes",
    action: "{}",
    bindingDigest: "sha256:ap1",
    expiresAtMs: 1800000000000,
    createdAtMs: 1780000000000,
    state: "pending",
    canDecide: true,
    sentence: "Waiting for you.",
    actions: ["allowOnce", "deny"],
    declinedBySend: true,
    write: { ...EDIT, ...write },
    ...patch,
  };
  render(<DatabaseApprovalCard approval={approval} />);
  return screen.getByRole("region", { name: "The change" });
}

/** The diff's lines, each as its mark-free text and what happened to it. */
function lines(change: HTMLElement) {
  const diff = within(change).getByRole("figure", { name: "Diff" });
  return Array.from(diff.querySelectorAll("[data-kind]")).map((line) => [
    line.getAttribute("data-kind"),
    line.textContent,
  ]);
}

it("an edit and a replace show the line diff, marked for a screen reader too", () => {
  for (const op of ["edit", "replace"] as const) {
    const change = card({ op });
    expect(lines(change)).toEqual([
      ["context", "  unchanged: # Inbox"],
      ["remove", "− removed: - [ ] milk"],
      ["add", "+ added: - [x] milk"],
    ]);
    expect(within(change).getByText("Line 4 → 4")).toBeInTheDocument();
    expect(within(change).queryByRole("figure", { name: "New text" })).toBeNull();
    document.body.innerHTML = "";
  }
});

it("a create and an append show the new text; past what a diff shows, its beginning", () => {
  const created = card({
    op: "create",
    path: "10-notes/new.md",
    preview: {
      hunks: [{ oldStart: 0, newStart: 1, lines: [{ kind: "add", text: "# New" }] }],
    },
  });
  expect(within(created).getByText("Create")).toBeInTheDocument();
  expect(lines(created)).toEqual([["add", "+ added: # New"]]);
  document.body.innerHTML = "";

  const appended = card({
    op: "append",
    preview: { hunks: [], whole: { lines: ["first", "second"], more: 3 } },
  });
  const whole = within(appended).getByRole("figure", { name: "The file after the change" });
  expect(whole).toHaveTextContent("firstsecond");
  expect(whole).toHaveTextContent("… and 3 more lines");
});

it("a move shows from and to, and a delete the deleted text", () => {
  const moved = card({ op: "move", to: "20-done/inbox.md", preview: { hunks: [] } });
  const moves = within(moved).getByLabelText("Moves");
  expect(moves).toHaveTextContent("From10-notes/inbox.mdTo20-done/inbox.md");
  expect(within(moved).queryByRole("figure", { name: "Diff" })).toBeNull();
  document.body.innerHTML = "";

  const deleted = card({
    op: "delete",
    preview: { hunks: [], deleted: { lines: ["# Old"], more: 0 } },
  });
  expect(within(deleted).getByRole("figure", { name: "Deleted text" })).toHaveTextContent("# Old");
});

it("Allow and Deny are buttons in the tab order, and Allow approves the write", () => {
  card({});
  const allow = screen.getByRole("button", { name: "Allow once" });
  const deny = screen.getByRole("button", { name: "Deny" });
  for (const button of [allow, deny]) {
    expect(button.tabIndex).toBe(0);
    button.focus();
    expect(document.activeElement).toBe(button);
  }
  fireEvent.click(allow);
  expect(mock.decide).toHaveBeenCalledWith(
    { database: "tgdrive", sessionId: "s.1" },
    "ap1",
    "sha256:ap1",
    true,
  );
});

it("a write this device cannot carry out offers Deny only and says why; the outcome is shown", () => {
  const reason = '"10-notes/inbox.md" changed on hesperia since the agent read it.';
  const blocked = card({ blocked: reason }, { actions: ["deny"] });
  expect(blocked).toHaveTextContent(`Allow is not offered: ${reason}`);
  expect(screen.queryByRole("button", { name: "Allow once" })).toBeNull();
  expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
  document.body.innerHTML = "";

  const done = card(
    { outcome: "Written as 1a2b3c4 on hesperia." },
    { state: "consumed", canDecide: false, actions: [], sentence: "Approved by Ana." },
  );
  expect(within(done).getByRole("status")).toHaveTextContent("Written as 1a2b3c4 on hesperia.");
});

it("lines are text, never markup", () => {
  const change = card({
    preview: {
      hunks: [{ oldStart: 1, newStart: 1, lines: [{ kind: "add", text: "<b>bold</b>" }] }],
    },
  });
  expect(change.querySelector("b")).toBeNull();
  expect(lines(change)).toEqual([["add", "+ added: <b>bold</b>"]]);
});

it("a change of line endings alone says so, beside lines whose text looks the same", () => {
  const change = card({
    preview: {
      hunks: [
        {
          oldStart: 1,
          newStart: 1,
          lines: [
            { kind: "remove", text: "- [ ] milk" },
            { kind: "add", text: "- [ ] milk" },
          ],
        },
      ],
      lineEndings: "Line endings change: CRLF → LF.",
    },
  });
  expect(within(change).getByText("Line endings change: CRLF → LF.")).toBeInTheDocument();
});

it("a change holding hidden characters shows them escaped and offers Deny only", () => {
  const change = card(
    {
      preview: {
        hunks: [
          {
            oldStart: 1,
            newStart: 1,
            lines: [{ kind: "add", text: "pay to ⟨U+202E⟩tnuocca-lacol⟨U+202C⟩ now" }],
          },
        ],
      },
      blocked: "This change contains hidden or reordering characters.",
    },
    { actions: ["deny"] },
  );
  expect(change).toHaveTextContent("pay to ⟨U+202E⟩tnuocca-lacol⟨U+202C⟩ now");
  expect(change).toHaveTextContent(
    "Allow is not offered: This change contains hidden or reordering characters.",
  );
  expect(screen.queryByRole("button", { name: "Allow once" })).toBeNull();
  expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
});

it("a right-to-left line is drawn in stored order, under the card's note", () => {
  const change = card({
    preview: {
      hunks: [
        {
          oldStart: 1,
          newStart: 1,
          lines: [{ kind: "add", text: "transfer א 900 100", rtl: true }],
        },
      ],
      rtlNote: "Right-to-left text is shown in stored order.",
    },
  });
  expect(within(change).getByText("Right-to-left text is shown in stored order.")).toBeVisible();
  const text = change.querySelector("[data-slot=drive-write-line-text]");
  expect(text).toHaveTextContent("transfer א 900 100");
  expect(text?.className).toContain("[unicode-bidi:bidi-override]");
  expect(text?.className).toContain("[direction:ltr]");
});

it("a whitespace-only change says so under its line; a too-large one says its size", () => {
  const change = card({
    preview: {
      hunks: [
        {
          oldStart: 1,
          newStart: 1,
          lines: [
            { kind: "remove", text: "-·[·]·milk" },
            { kind: "add", text: "-·[·]·milk·", note: "Only whitespace changes on this line." },
          ],
        },
      ],
    },
  });
  expect(within(change).getByText("Only whitespace changes on this line.")).toBeVisible();
  document.body.innerHTML = "";
  const large = card(
    {
      preview: { hunks: [], tooLarge: "It takes 131072 lines; a card shows at most 5000." },
      blocked: "This change is too large to show in full.",
    },
    { actions: ["deny"] },
  );
  expect(large).toHaveTextContent("It takes 131072 lines; a card shows at most 5000.");
  expect(within(large).queryByRole("figure", { name: "Diff" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Allow once" })).toBeNull();
});

it("markers are their own named spans; content beside them is plain text", () => {
  const change = card({
    preview: {
      hunks: [
        {
          oldStart: 1,
          newStart: 1,
          lines: [
            {
              kind: "remove",
              text: "a·⟨U+00B7⟩b",
              runs: [{ text: "a" }, { text: "·", mark: "space" }, { text: "⟨U+00B7⟩b" }],
            },
          ],
        },
      ],
    },
  });
  const marks = within(change).getAllByRole("img", { name: "space" });
  expect(marks).toHaveLength(1);
  expect(marks[0]).toHaveTextContent("·");
  expect(change.querySelector("[data-slot=drive-write-line-text]")).toHaveTextContent(
    "a·⟨U+00B7⟩b",
  );
});

it("compacted marker runs announce the exact count without losing visible glyphs", () => {
  const spaces = "·".repeat(9999);
  const change = card({
    preview: {
      hunks: [
        {
          oldStart: 1,
          newStart: 1,
          lines: [
            { kind: "add", text: spaces, runs: [{ text: spaces, mark: "space" }] },
            {
              kind: "add",
              text: "→→⏎",
              runs: [
                { text: "→→", mark: "tab" },
                { text: "⏎", mark: "carriage return" },
              ],
            },
          ],
        },
      ],
    },
  });
  expect(within(change).getByRole("img", { name: "9999 × space" }).textContent).toBe(spaces);
  expect(within(change).getByRole("img", { name: "2 × tab" })).toHaveTextContent("→→");
  expect(within(change).getByRole("img", { name: "carriage return" })).toHaveTextContent("⏎");
});

it("a request whose words hold right-to-left text draws its path, to, summary and action in stored order", () => {
  card(
    {
      op: "move",
      path: "10-notes/inbox.md",
      to: "10-notes/א 900 100.md",
      preview: { hunks: [], rtlNote: "Right-to-left text is shown in stored order." },
      rtl: true,
    },
    { summary: "Move `10-notes/inbox.md` to `10-notes/א 900 100.md` in tgdrive" },
  );
  const stored = (el: Element | null) => el?.className.includes("[unicode-bidi:bidi-override]");
  expect(stored(screen.getByText("10-notes/א 900 100.md"))).toBe(true);
  expect(stored(screen.getByText(/^Move `10-notes/))).toBe(true);
  expect(stored(screen.getByRole("figure", { name: "Exact action" }).querySelector("pre"))).toBe(
    true,
  );
});
