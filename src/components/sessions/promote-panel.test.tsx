import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PromotePanel } from "@/components/sessions/promote-panel";
import { SessionActions } from "@/components/sessions/session-actions";
import { SessionDetail } from "@/components/sessions/session-detail";
import { TooltipProvider } from "@/components/ui/tooltip";
import type * as Client from "@/lib/ipc/client";
import {
  installMockShell,
  type PromoteFixture,
  stagePromoteFixture,
} from "../../../dev/mock-shell";

// The zone's change event, raised by the test as the watcher would raise it.
const changed: ((rootId: string) => void)[] = [];
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Client>()),
  listenSessionsChanged: async (onChanged: (rootId: string) => void) => {
    changed.push(onChanged);
    return () => {
      changed.splice(changed.indexOf(onChanged), 1);
    };
  },
}));

const rootId = "p1";
const sessionId = "01J8SESSIONAAAAAAAAAAAAAAA";
const LEDGER = "artifacts/knowledge/2026-10-05-taxes/the-whole-ledger.md";
type Invoke = (command: string, payload?: Record<string, unknown>) => Promise<unknown>;
let original: Invoke;
function internals() {
  // Tauri's mock installs this typed in-process boundary on Window.
  const mockWindow = window as unknown as { __TAURI_INTERNALS__: { invoke: Invoke } };
  return mockWindow.__TAURI_INTERNALS__;
}
function intercept(
  handler: (command: string, payload: Record<string, unknown>, next: Invoke) => Promise<unknown>,
) {
  internals().invoke = (command, payload = {}) => handler(command, payload, original);
}
function refusal(message: string) {
  return { code: "internal", message, accountId: null, retriable: false };
}
function stage(change: (fixture: PromoteFixture) => void) {
  current = stagePromoteFixture(rootId, sessionId, change);
}
/** Change the fixture in place, as a write on disk would, without resetting it. */
function edit(change: (fixture: PromoteFixture) => void) {
  current = stagePromoteFixture(rootId, sessionId, (fresh) => {
    Object.assign(fresh, current);
    change(fresh);
  });
}
let current: PromoteFixture;
/** The watcher's event for this root, as a write anywhere in it raises it. */
function announce() {
  for (const onChanged of changed) onChanged(rootId);
}
/**
 * A key pressed on `element`, which must have the focus: what a browser does —
 * the keydown, then, unless the control cancelled it, the activation it
 * stands for. Radix's checkbox cancels Enter, as its keyboard contract says.
 */
function press(element: HTMLElement, key: "Enter" | " ") {
  element.focus();
  expect(element).toHaveFocus();
  if (fireEvent.keyDown(element, { key })) {
    fireEvent.keyUp(element, { key });
    fireEvent.click(element);
  }
}
function mount() {
  return render(<PromotePanel rootId={rootId} sessionId={sessionId} />);
}
async function row(name: string) {
  return within(await screen.findByRole("listitem", { name }));
}
async function candidate() {
  return row("The whole ledger");
}
/** The next panel read waits for `release`. */
function holdNextPanelRead() {
  let release = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let once = true;
  intercept(async (command, payload, next) => {
    if (command === "sessions_promote_panel" && once) {
      once = false;
      await held;
    }
    return next(command, payload);
  });
  return () => act(async () => release());
}

beforeEach(() => {
  vi.spyOn(console, "debug").mockImplementation(() => {});
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  installMockShell();
  original = internals().invoke;
  current = stagePromoteFixture(rootId, sessionId);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("a person's promote review over the mock shell", () => {
  it("offers a copy only where a row can take one and replaces a stale row once", async () => {
    mount();
    const missing = await row("workspace/scratch.md");
    expect(missing.getByText("Source gone")).toBeInTheDocument();
    expect(missing.queryByRole("button", { name: "Re-promote…" })).not.toBeInTheDocument();
    const unreadable = await row("Unreadable line 17");
    expect(unreadable.queryByRole("button")).not.toBeInTheDocument();
    const unknown = await row("workspace/locked.md");
    expect(unknown.getByText("State unknown")).toBeInTheDocument();
    expect(unknown.getByText(/artifacts\/locked.md could not be read/)).toBeInTheDocument();
    for (const name of ["workspace/plan.md", "workspace/figures.csv"]) {
      const item = await row(name);
      fireEvent.click(item.getByRole("button", { name: "Re-promote…" }));
      fireEvent.click(item.getByRole("button", { name: "Promote file" }));
      await waitFor(async () =>
        expect((await row(name)).getByText("Up to date")).toBeInTheDocument(),
      );
      expect(screen.getAllByRole("listitem", { name })).toHaveLength(1);
    }
  });

  it("keeps a refused workspace file editable in place and moves a promoted unlisted file to its row", async () => {
    mount();
    const changing = await row("workspace/notes.md");
    fireEvent.click(changing.getByRole("button", { name: "Promote…" }));
    fireEvent.click(changing.getByRole("button", { name: "Promote file" }));
    expect(await changing.findByRole("alert")).toHaveTextContent("still being written");
    expect(changing.getByLabelText("Artifact target")).toBeEnabled();
    const data = await row("workspace/data/run.json");
    fireEvent.click(data.getByRole("button", { name: "Promote…" }));
    fireEvent.change(data.getByLabelText("Artifact target"), {
      target: { value: "artifacts/results.json" },
    });
    fireEvent.click(data.getByRole("button", { name: "Promote file" }));
    await waitFor(async () =>
      expect(
        (await row("workspace/data/run.json")).getByText(/artifacts\/results.json/),
      ).toBeInTheDocument(),
    );
    expect(screen.getAllByRole("listitem", { name: "workspace/data/run.json" })).toHaveLength(1);
    const unlisted = screen.getByRole("heading", {
      name: "Unlisted workspace files",
    }).parentElement;
    expect(
      within(unlisted as HTMLElement).queryByRole("listitem", { name: "workspace/data/run.json" }),
    ).not.toBeInTheDocument();
  });

  it("opens all 64 KiB and publishes the version read, reviewed, into this drive's vault", async () => {
    mount();
    const note = await candidate();
    const consent = note.getByRole("checkbox", { name: "I reviewed this version" });
    expect(consent).toBeDisabled();
    expect(note.getByRole("button", { name: "Promote to notes…" })).toBeDisabled();
    fireEvent.click(note.getByRole("button", { name: "Read whole note" }));
    const body = await note.findByLabelText("Whole knowledge note");
    expect(new TextEncoder().encode((body as HTMLTextAreaElement).value)).toHaveLength(65536);
    expect((body as HTMLTextAreaElement).value.endsWith("The last line of the ledger.\n")).toBe(
      true,
    );
    await waitFor(() => expect(consent).toBeEnabled());
    fireEvent.click(consent);
    await waitFor(() =>
      expect(note.getByRole("button", { name: "Promote to notes…" })).toBeEnabled(),
    );
    fireEvent.click(note.getByRole("button", { name: "Promote to notes…" }));
    expect(note.queryByRole("button", { name: "Parent folder" })).not.toBeInTheDocument();
    fireEvent.click(await note.findByRole("button", { name: "knowledge" }));
    fireEvent.change(await note.findByLabelText("Note filename"), {
      target: { value: "ledger.md" },
    });
    fireEvent.click(note.getByRole("button", { name: "Promote to this folder" }));
    await waitFor(() => expect(note.getByText("10-notes/knowledge/ledger.md")).toBeInTheDocument());
    expect(note.getByRole("checkbox", { name: "Reviewed by me in notes" })).toBeChecked();
    expect(note.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("binds consent to the version read: a newer candidate clears it and a late read shows stale", async () => {
    mount();
    let note = await candidate();
    fireEvent.click(note.getByRole("button", { name: "Read whole note" }));
    await note.findByLabelText("Whole knowledge note");
    await waitFor(() =>
      expect(note.getByRole("checkbox", { name: "I reviewed this version" })).toBeEnabled(),
    );
    fireEvent.click(note.getByRole("checkbox", { name: "I reviewed this version" }));
    await waitFor(() =>
      expect(note.getByRole("button", { name: "Promote to notes…" })).toBeEnabled(),
    );
    edit((fixture) => {
      fixture.files.set(LEDGER, {
        text: "---\ntitle: The whole ledger\n---\nRewritten.\n",
        changed: 50,
      });
    });
    act(announce);
    note = await candidate();
    expect(await note.findByText(/changed since you opened it/)).toBeInTheDocument();
    expect(note.getByRole("checkbox", { name: "I reviewed this version" })).not.toBeChecked();
    expect(note.getByRole("checkbox", { name: "I reviewed this version" })).toBeDisabled();
    expect(note.getByRole("button", { name: "Promote to notes…" })).toBeDisabled();
    fireEvent.click(note.getByRole("button", { name: "Read the new version" }));
    await waitFor(() =>
      expect(note.getByLabelText("Whole knowledge note")).toHaveValue(
        "---\ntitle: The whole ledger\n---\nRewritten.\n",
      ),
    );
    await waitFor(() =>
      expect(note.getByRole("checkbox", { name: "I reviewed this version" })).toBeEnabled(),
    );

    // A read that answers after the note changed is shown as what it is: old.
    cleanup();
    current = stagePromoteFixture(rootId, sessionId);
    let answer = () => {};
    const late = new Promise<void>((resolve) => {
      answer = resolve;
    });
    intercept(async (command, payload, next) => {
      if (command === "sessions_knowledge_read") {
        const text = await next(command, payload);
        await late;
        return text;
      }
      return next(command, payload);
    });
    mount();
    note = await candidate();
    fireEvent.click(note.getByRole("button", { name: "Read whole note" }));
    edit((fixture) => {
      fixture.files.set(LEDGER, {
        text: "---\ntitle: The whole ledger\n---\nNewer.\n",
        changed: 50,
      });
    });
    act(announce);
    await act(async () => answer());
    note = await candidate();
    expect(await note.findByText(/changed since you opened it/)).toBeInTheDocument();
    expect(note.getByRole("checkbox", { name: "I reviewed this version" })).toBeDisabled();
  });

  it("restores a missing notes copy by promoting the note again, never by reviewing an absent file", async () => {
    stage((fixture) => {
      fixture.vault.clear();
    });
    mount();
    const note = await row("What the tax office wants");
    expect(note.getByText("Target missing")).toBeInTheDocument();
    expect(note.getByText(/notes copy is missing/)).toBeInTheDocument();
    expect(
      note.queryByRole("checkbox", { name: "Reviewed by me in notes" }),
    ).not.toBeInTheDocument();
    fireEvent.click(note.getByRole("button", { name: "Read whole note" }));
    await note.findByLabelText("Whole knowledge note");
    await waitFor(() =>
      expect(note.getByRole("checkbox", { name: "I reviewed this version" })).toBeEnabled(),
    );
    fireEvent.click(note.getByRole("checkbox", { name: "I reviewed this version" }));
    await waitFor(() =>
      expect(note.getByRole("button", { name: "Restore the notes copy" })).toBeEnabled(),
    );
    fireEvent.click(note.getByRole("button", { name: "Restore the notes copy" }));
    await waitFor(() => expect(note.getByText("Up to date")).toBeInTheDocument());
    expect(note.getByRole("checkbox", { name: "Reviewed by me in notes" })).toBeChecked();
  });

  it("reads the candidate and the notes copy separately when they differ", async () => {
    stage((fixture) => {
      const copy = fixture.vault.get("10-notes/knowledge/what-to-bring.md");
      if (copy)
        copy.text = copy.text.replace("Bring the PIT-37", "Bring the PIT-38, edited in notes");
    });
    mount();
    const note = await row("What the tax office wants");
    fireEvent.click(note.getByRole("button", { name: "Read whole note" }));
    expect(
      ((await note.findByLabelText("Whole knowledge note")) as HTMLTextAreaElement).value,
    ).toContain("Bring the PIT-37");
    fireEvent.click(note.getByRole("button", { name: "Read notes copy" }));
    expect(((await note.findByLabelText("Notes copy")) as HTMLTextAreaElement).value).toContain(
      "PIT-38, edited in notes",
    );
  });

  it("reviews only the notes copy as read, and keeps Rust's last saved review when a write is refused", async () => {
    mount();
    const note = await row("What the tax office wants");
    const tick = note.getByRole("checkbox", { name: "Reviewed by me in notes" });
    expect(tick).toBeChecked();
    expect(tick).toBeDisabled();
    fireEvent.click(note.getByRole("button", { name: "Read notes copy" }));
    await note.findByLabelText("Notes copy");
    await waitFor(() => expect(tick).toBeEnabled());
    intercept(async (command, payload, next) => {
      if (command === "sessions_knowledge_review")
        throw refusal("The vault note changed; read it again.");
      return next(command, payload);
    });
    fireEvent.click(tick);
    expect(await note.findByRole("alert")).toHaveTextContent("The vault note changed");
    expect(tick).toBeChecked();
    internals().invoke = original;
    fireEvent.click(tick);
    await waitFor(() => expect(tick).not.toBeChecked());
    // The untick rewrote the copy: a new review needs the copy as it is now.
    expect(tick).toBeDisabled();
    fireEvent.click(note.getByRole("button", { name: "Read the notes copy again" }));
    await waitFor(() => expect(tick).toBeEnabled());
    fireEvent.click(tick);
    await waitFor(() => expect(tick).toBeChecked());
  });

  it("says why a file is not offered, and what the panel could not see, with nothing to click", async () => {
    stage((fixture) => {
      fixture.table = false;
      fixture.rows = [];
      fixture.outRefused = "This note is private to tgorka, but the vault is shared with marta.";
      fixture.problems = ["artifacts/ was listed only up to 4096 files."];
    });
    mount();
    expect(await screen.findByText(/listed only up to 4096 files/)).toBeInTheDocument();
    const draft = await row("workspace/draft.md");
    expect(draft.queryByRole("button", { name: "Promote…" })).not.toBeInTheDocument();
    expect(draft.getByText(/no ## Promote table/)).toBeInTheDocument();
    const note = await candidate();
    expect(note.getByRole("button", { name: "Read whole note" })).toBeEnabled();
    cleanup();
    stage(() => {});
    mount();
    const chart = await row("artifacts/chart.png");
    expect(chart.getByText(/not a text file/)).toBeInTheDocument();
    expect(chart.queryByRole("button")).not.toBeInTheDocument();
    // Valid UTF-8 holding a NUL is text: it is offered.
    expect(
      (await row("artifacts/trace.log")).getByRole("button", {
        name: "Promote artifact to notes…",
      }),
    ).toBeEnabled();
    expect(
      (await row("artifacts/flat-contract.md")).getByRole("button", {
        name: "Promote artifact to notes…",
      }),
    ).toBeEnabled();
  });

  it("does not offer a destination outside the label, or hide a failed panel read behind an empty list", async () => {
    stage((fixture) => {
      fixture.outRefused = "This note is private to tgorka, but the vault is shared with marta.";
    });
    mount();
    const note = await candidate();
    expect(note.queryByRole("button", { name: "Promote to notes…" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Promote artifact to notes…" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/private to tgorka/)).toBeInTheDocument();
    cleanup();
    intercept(async (command, payload, next) => {
      if (command === "sessions_promote_panel") throw refusal("The session is not reachable.");
      return next(command, payload);
    });
    mount();
    expect(await screen.findByRole("alert")).toHaveTextContent("not reachable");
    expect(screen.queryByText("No Promote table yet.")).not.toBeInTheDocument();
    internals().invoke = original;
    fireEvent.click(screen.getByRole("button", { name: "Read again" }));
    expect(await screen.findByRole("listitem", { name: "The whole ledger" })).toBeInTheDocument();
  });

  it("promotes an ordinary artifact without inventing a knowledge review", async () => {
    mount();
    const contract = await row("artifacts/flat-contract.md");
    fireEvent.click(contract.getByRole("button", { name: "Promote artifact to notes…" }));
    fireEvent.click(await contract.findByRole("button", { name: "knowledge" }));
    fireEvent.click(await contract.findByRole("button", { name: "Promote to this folder" }));
    // Its recorded row and its offer, now to the same notes copy, both name it.
    const recorded = await screen.findAllByText(/10-notes\/knowledge\/flat-contract.md/);
    expect(recorded).toHaveLength(2);
    for (const element of recorded)
      expect(element.closest("li")).toHaveAccessibleName("artifacts/flat-contract.md");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("moves focus into each editor and back to its row action", async () => {
    mount();
    const data = await row("workspace/data/run.json");
    const open = data.getByRole("button", { name: "Promote…" });
    open.focus();
    fireEvent.click(open);
    expect(data.getByLabelText("Artifact target")).toHaveFocus();
    fireEvent.click(data.getByRole("button", { name: "Cancel target" }));
    expect(open).toHaveFocus();
    fireEvent.click(open);
    fireEvent.click(data.getByRole("button", { name: "Promote file" }));
    await waitFor(async () =>
      expect(
        (await row("workspace/data/run.json")).getByRole("button", { name: "Re-promote…" }),
      ).toHaveFocus(),
    );

    const artifact = await row("artifacts/flat-contract.md");
    const pick = artifact.getByRole("button", { name: "Promote artifact to notes…" });
    fireEvent.click(pick);
    expect(artifact.getByRole("group", { name: "Choose a notes folder" })).toHaveFocus();
    fireEvent.click(artifact.getByRole("button", { name: "Cancel folder choice" }));
    expect(pick).toHaveFocus();
  });

  it("keeps focus in a knowledge card after its promotion: on its notes-copy reader", async () => {
    async function consentByKeyboard(card: ReturnType<typeof within>) {
      press(card.getByRole("button", { name: "Read whole note" }), "Enter");
      await card.findByLabelText("Whole knowledge note");
      const consent = card.getByRole("checkbox", { name: "I reviewed this version" });
      await waitFor(() => expect(consent).toBeEnabled());
      press(consent, " ");
      await waitFor(() => expect(consent).toBeChecked());
    }

    // A new publication: the trigger is gone once the note is promoted.
    mount();
    let card = await candidate();
    await consentByKeyboard(card);
    press(card.getByRole("button", { name: "Promote to notes…" }), "Enter");
    expect(card.getByRole("group", { name: "Choose a notes folder" })).toHaveFocus();
    press(await card.findByRole("button", { name: "knowledge" }), "Enter");
    press(await card.findByRole("button", { name: "Promote to this folder" }), "Enter");
    await waitFor(async () =>
      expect((await candidate()).getByRole("button", { name: "Read notes copy" })).toHaveFocus(),
    );

    // A missing copy restored, and a newer candidate promoted again: each
    // trigger is disabled, then removed.
    for (const change of [
      (fixture: PromoteFixture) => fixture.vault.clear(),
      (fixture: PromoteFixture) => {
        const bring = "artifacts/knowledge/2026-10-05-taxes/what-to-bring.md";
        const note = fixture.files.get(bring);
        if (note) fixture.files.set(bring, { text: `${note.text}And the ID.\n`, changed: 60 });
      },
    ]) {
      cleanup();
      stage(change);
      mount();
      card = await row("What the tax office wants");
      await consentByKeyboard(card);
      press(
        card.getByRole("button", { name: /Restore the notes copy|Promote this version again/ }),
        "Enter",
      );
      await waitFor(async () =>
        expect(
          (await row("What the tax office wants")).getByRole("button", { name: "Read notes copy" }),
        ).toHaveFocus(),
      );
    }
  });
});

describe("archiving with the promote review", () => {
  function archiveDialog() {
    render(
      <TooltipProvider>
        <SessionActions
          rootId={rootId}
          rootPath="/drive/60-sessions"
          row={{
            id: sessionId,
            path: "active/session",
            title: "Session",
            status: "active",
            pinned: false,
            tags: [],
            archivedYear: null,
            workspaceMs: null,
            recordMs: null,
            lastLogDate: "",
            lastLogLine: "",
            snippet: "",
            unread: false,
            origin: "local",
            headRev: "",
            conflict: false,
            lineage: false,
          }}
        />
      </TooltipProvider>,
    );
    fireEvent.pointerDown(screen.getByRole("button", { name: "Session actions" }), {
      button: 0,
      ctrlKey: false,
      pointerType: "mouse",
    });
  }
  function skipAll(except: string[] = []) {
    const dialog = within(screen.getByRole("alertdialog"));
    for (const item of dialog.getAllByRole("listitem")) {
      const skip = within(item).queryByRole("button", { name: "Skip this row" });
      if (skip && !except.includes(item.getAttribute("aria-label") ?? "")) fireEvent.click(skip);
    }
  }

  it("requires every choice, shows the queued promotion and keeps the dialog on refusal", async () => {
    intercept(async (command, payload, next) => {
      if (command === "sessions_archive") {
        throw refusal("workspace/data/run.json is still being written; try again in a moment.");
      }
      return next(command, payload);
    });
    archiveDialog();
    fireEvent.click(await screen.findByRole("menuitem", { name: "Archive…" }));
    const dialog = within(await screen.findByRole("alertdialog"));
    const archive = dialog.getByRole("button", { name: "Archive session" });
    await screen.findByRole("listitem", { name: "workspace/data/run.json" });
    expect(archive).toBeDisabled();
    skipAll(["workspace/data/run.json"]);
    expect(archive).toBeDisabled();
    const data = await row("workspace/data/run.json");
    fireEvent.click(data.getByRole("button", { name: "Promote…" }));
    fireEvent.click(data.getByRole("button", { name: "Promote before archiving" }));
    await waitFor(() => expect(archive).toBeEnabled());
    expect(
      (await row("workspace/data/run.json")).getByText("Will promote to artifacts/run.json"),
    ).toBeInTheDocument();
    fireEvent.click(archive);
    expect(await dialog.findByRole("alert")).toHaveTextContent("still being written");
    expect(archive).toBeEnabled();
  });

  it("keeps a row written twice as two rows, each archived only on its own choice", async () => {
    stage((fixture) => {
      fixture.rows.push({ source: "workspace/plan.md", target: "artifacts/plan.md", note: "" });
    });
    archiveDialog();
    fireEvent.click(await screen.findByRole("menuitem", { name: "Archive…" }));
    const dialog = within(await screen.findByRole("alertdialog"));
    const archive = dialog.getByRole("button", { name: "Archive session" });
    const twice = await screen.findAllByRole("listitem", { name: "workspace/plan.md" });
    expect(twice).toHaveLength(2);
    expect(within(twice[1]).getByText(/repeats an earlier row/)).toBeInTheDocument();
    skipAll(["workspace/plan.md"]);
    fireEvent.click(within(twice[0]).getByRole("button", { name: "Skip this row" }));
    await waitFor(() =>
      expect(within(twice[0]).getByText("Skipped for this archive")).toBeInTheDocument(),
    );
    expect(within(twice[1]).queryByText("Skipped for this archive")).not.toBeInTheDocument();
    expect(archive).toBeDisabled();
    fireEvent.click(within(twice[1]).getByRole("button", { name: "Skip this row" }));
    await waitFor(() => expect(archive).toBeEnabled());
    fireEvent.click(archive);
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("is not ready while a reread is out, keeps choices an unchanged reread confirms and drops changed ones", async () => {
    archiveDialog();
    fireEvent.click(await screen.findByRole("menuitem", { name: "Archive…" }));
    const dialog = within(await screen.findByRole("alertdialog"));
    const archive = dialog.getByRole("button", { name: "Archive session" });
    await screen.findByRole("listitem", { name: "workspace/data/run.json" });
    skipAll();
    await waitFor(() => expect(archive).toBeEnabled());

    // Activity elsewhere in the root: the reread holds the archive until it
    // answers, then the same rows keep their choices.
    let release = holdNextPanelRead();
    act(announce);
    await waitFor(() => expect(archive).toBeDisabled());
    await release();
    await waitFor(() => expect(archive).toBeEnabled());

    // A file arrives in the workspace while the reread is out: never ready
    // on the old snapshot, and not ready until the new file is decided.
    edit((fixture) => {
      fixture.files.set("workspace/arrived.md", { text: "# new\n", changed: 60 });
    });
    release = holdNextPanelRead();
    act(announce);
    await waitFor(() => expect(archive).toBeDisabled());
    await release();
    const arrived = await row("workspace/arrived.md");
    expect(archive).toBeDisabled();
    fireEvent.click(arrived.getByRole("button", { name: "Skip this row" }));
    await waitFor(() => expect(archive).toBeEnabled());

    // A row whose cells changed loses its choice; the others keep theirs.
    edit((fixture) => {
      const plan = fixture.rows.find(
        (entry) => "source" in entry && entry.source === "workspace/plan.md",
      );
      if (plan && "note" in plan) plan.note = "rewritten";
    });
    act(announce);
    await waitFor(async () =>
      expect((await row("workspace/plan.md")).getByText("rewritten")).toBeInTheDocument(),
    );
    const plan = await row("workspace/plan.md");
    expect(archive).toBeDisabled();
    expect(plan.queryByText("Skipped for this archive")).not.toBeInTheDocument();
    expect(
      (await row("workspace/draft.md")).getByText("Skipped for this archive"),
    ).toBeInTheDocument();
    // Changed back, it is still a row nobody decided about in this snapshot.
    edit((fixture) => {
      const back = fixture.rows.find(
        (entry) => "source" in entry && entry.source === "workspace/plan.md",
      );
      if (back && "note" in back) back.note = "";
    });
    act(announce);
    await waitFor(async () =>
      expect((await row("workspace/plan.md")).queryByText("rewritten")).not.toBeInTheDocument(),
    );
    const again = await row("workspace/plan.md");
    expect(again.queryByText("Skipped for this archive")).not.toBeInTheDocument();
    expect(archive).toBeDisabled();
    fireEvent.click(again.getByRole("button", { name: "Skip this row" }));
    await waitFor(() => expect(archive).toBeEnabled());
    // The checklist as read is what archives: Rust (here the mock) refuses any other.
    fireEvent.click(archive);
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("is not ready while a panel write is out", async () => {
    let finish = () => {};
    const write = new Promise<void>((resolve) => {
      finish = resolve;
    });
    archiveDialog();
    fireEvent.click(await screen.findByRole("menuitem", { name: "Archive…" }));
    const dialog = within(await screen.findByRole("alertdialog"));
    const archive = dialog.getByRole("button", { name: "Archive session" });
    await screen.findByRole("listitem", { name: "workspace/data/run.json" });
    skipAll();
    await waitFor(() => expect(archive).toBeEnabled());
    const note = await row("What the tax office wants");
    fireEvent.click(note.getByRole("button", { name: "Read notes copy" }));
    await note.findByLabelText("Notes copy");
    intercept(async (command, payload, next) => {
      if (command === "sessions_knowledge_review") await write;
      return next(command, payload);
    });
    fireEvent.click(note.getByRole("checkbox", { name: "Reviewed by me in notes" }));
    await waitFor(() => expect(archive).toBeDisabled());
    await act(async () => finish());
    await waitFor(() => expect(archive).toBeEnabled());
  });
});

it("opens the promote review from the session detail", async () => {
  render(
    <TooltipProvider>
      <SessionDetail
        rootId={rootId}
        subfolder="60-sessions"
        sessionId={sessionId}
        onBack={() => {}}
      />
    </TooltipProvider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Promote…" }));
  const dialog = within(await screen.findByRole("dialog"));
  const note = within(await dialog.findByRole("listitem", { name: "The whole ledger" }));
  fireEvent.click(note.getByRole("button", { name: "Read whole note" }));
  expect(
    ((await note.findByLabelText("Whole knowledge note")) as HTMLTextAreaElement).value.endsWith(
      "The last line of the ledger.\n",
    ),
  ).toBe(true);
  await act(async () => {
    fireEvent.click(dialog.getByRole("button", { name: "Close" }));
  });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
