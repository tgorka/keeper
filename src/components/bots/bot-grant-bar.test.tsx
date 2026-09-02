/**
 * The grant bar: absent where a grant could never be used, warned where keeper
 * could not tell, and a live grant you can change or revoke in one act (Epic
 * 61, Story 61.10, FR-386, FR-387, AD-27, AD-158).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **Absence is absence** — a Hermes bot, and an Ollama bot whose model says
 *    it cannot take tools, get the sentence and **no control**. Make the bar
 *    unconditional and the two `no control` tests fail while the rest pass.
 * 2. **`unknown` is not `false`, and it is not `true` either** — an unreadable
 *    capability renders its own sentence as a live region, and still offers
 *    nothing, because an affordance that may reach nothing is the one AD-27
 *    names.
 * 3. **The three sentences are Rust's, letter for letter** — the parity test
 *    reads `keeper-core/src/bots/grant.rs` and fails on a character of drift,
 *    so the pane and the audit log cannot word one fact twice.
 * 4. **FR-387 is visible in the copy** — a `write` grant on a whole profile
 *    reads as "keeper asks before every write", because that is what `decide`
 *    does; only a subtree grant reads as write-through.
 * 5. **Revoke removes the row and the bar re-reads** — the bar never splices
 *    its own list, so a failed revocation cannot leave a row missing on screen
 *    and present in the store.
 */

import { readFileSync } from "node:fs";
import path from "node:path";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BotGrantBar,
  botGrantOffer,
  GRANT_ADD_LABEL,
  GRANT_CHANGE_LABEL,
  GRANT_HERMES_SENTENCE,
  GRANT_LIST_LABEL,
  GRANT_NO_TOOLS_SENTENCE,
  GRANT_NONE_HELD,
  GRANT_REVOKE_LABEL,
  GRANT_SAVE_LABEL,
  GRANT_TOOLS_UNKNOWN_SENTENCE,
  grantSentence,
  liveGrantsFor,
} from "@/components/bots/bot-grant-bar";
import type {
  BotGrantListVm,
  BotGrantSaveReq,
  BotGrantVm,
  BotModelVm,
  BotProviderVm,
  SyncProfileVm,
} from "@/lib/ipc/client";
import { syncStore } from "@/lib/stores/sync";

const botsGrantsList = vi.fn();
const botsGrantSave = vi.fn();
const botsGrantRevoke = vi.fn();

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsGrantsList: () => botsGrantsList(),
    botsGrantSave: (req: BotGrantSaveReq) => botsGrantSave(req),
    botsGrantRevoke: (grantId: string) => botsGrantRevoke(grantId),
  };
});

const OLLAMA: BotProviderVm = {
  id: "prov-1",
  kind: "ollama",
  name: "Ollama here",
  baseUrl: "http://localhost:11434",
  host: "localhost",
  isPrivate: true,
  createdMs: 1,
  health: "reachable",
  healthCheckedMs: 2,
  healthDetail: null,
  readTimeoutMs: null,
  hasToken: false,
};

const HERMES: BotProviderVm = { ...OLLAMA, id: "prov-2", kind: "hermes", name: "Hermes there" };

const MODEL: BotModelVm = {
  id: "llama4:8b",
  family: "llama4",
  parameterSize: "8.0B",
  quantization: null,
  sizeBytes: null,
  contextWindow: null,
  maxOutputTokens: null,
  vision: false,
  tools: true,
  reasoning: false,
  capabilities: ["completion", "tools"],
};

const GRANT: BotGrantVm = {
  id: "grant-1",
  providerId: "prov-1",
  botId: "bot-1",
  scope: { kind: "subtree", profileId: "p1", subpath: "journal/2026" },
  scopeLabel: "p1/journal/2026",
  mode: "write",
  createdMs: 10,
  revokedMs: null,
};

const PROFILE: SyncProfileVm = {
  id: "p1",
  name: "tgdrive",
} as SyncProfileVm;

function listing(grants: BotGrantVm[]): BotGrantListVm {
  return { grants, unknown: [] };
}

beforeEach(() => {
  botsGrantsList.mockReset();
  botsGrantSave.mockReset();
  botsGrantRevoke.mockReset();
  botsGrantsList.mockResolvedValue(listing([]));
  botsGrantSave.mockResolvedValue(GRANT);
  botsGrantRevoke.mockResolvedValue(undefined);
  syncStore.setState({ profiles: [PROFILE], statuses: {}, hydrated: true, error: null });
});

describe("botGrantOffer", () => {
  it("offers nothing at all without folder sync", () => {
    expect(botGrantOffer({ sync: false, provider: OLLAMA, model: MODEL })).toEqual({
      kind: "absent",
    });
  });

  it("refuses a Hermes bot with the one sentence", () => {
    expect(botGrantOffer({ sync: true, provider: HERMES, model: MODEL })).toEqual({
      kind: "refused",
      sentence: GRANT_HERMES_SENTENCE,
    });
  });

  it("refuses a model that stated it cannot take tools", () => {
    expect(
      botGrantOffer({ sync: true, provider: OLLAMA, model: { ...MODEL, tools: false } }),
    ).toEqual({ kind: "refused", sentence: GRANT_NO_TOOLS_SENTENCE });
  });

  it("offers the grant with a warning when the tool capability is unreadable", () => {
    // `unknown` is not `false`: refusing here would strand every endpoint too
    // old to state its capabilities, and every call still goes through
    // `grant::decide` and the approval path.
    expect(
      botGrantOffer({ sync: true, provider: OLLAMA, model: { ...MODEL, tools: null } }),
    ).toEqual({ kind: "offered", warning: GRANT_TOOLS_UNKNOWN_SENTENCE });
  });

  it("offers the grant with no warning for a model that advertises tools", () => {
    expect(botGrantOffer({ sync: true, provider: OLLAMA, model: MODEL })).toEqual({
      kind: "offered",
      warning: null,
    });
  });
});

describe("the sentences are keeper-core's, letter for letter", () => {
  it("quotes grant.rs's three refusal consts verbatim", () => {
    const source = readFileSync(
      path.resolve(process.cwd(), "src-tauri/crates/keeper-core/src/bots/grant.rs"),
      "utf8",
    )
      // Rust joins a `\`-continued string literal by dropping the backslash,
      // the newline and the following indentation.
      .replace(/\\\s*\n\s*/g, "");
    for (const sentence of [
      GRANT_HERMES_SENTENCE,
      GRANT_NO_TOOLS_SENTENCE,
      GRANT_TOOLS_UNKNOWN_SENTENCE,
    ]) {
      expect(source).toContain(sentence);
    }
  });
});

describe("grantSentence", () => {
  it("says a subtree write grant writes", () => {
    expect(grantSentence(GRANT)).toBe("This bot can read and write p1/journal/2026.");
  });

  it("says a profile-wide write grant is asked about every time (FR-387)", () => {
    expect(
      grantSentence({
        ...GRANT,
        scope: { kind: "profile", profileId: "p1" },
        scopeLabel: "p1",
      }),
    ).toBe("This bot can read p1, and keeper asks before every write to a scope this wide.");
  });

  it("says a read grant cannot write", () => {
    expect(grantSentence({ ...GRANT, mode: "read" })).toBe(
      "This bot can read p1/journal/2026, and cannot write there.",
    );
  });

  it("says a none grant refuses, whatever a wider grant says", () => {
    expect(grantSentence({ ...GRANT, mode: "none" })).toBe(
      "This bot is refused p1/journal/2026, whatever a wider grant says.",
    );
  });
});

describe("liveGrantsFor", () => {
  it("keeps the bot's own grants and the provider-wide ones", () => {
    const wide: BotGrantVm = { ...GRANT, id: "grant-2", botId: null };
    const other: BotGrantVm = { ...GRANT, id: "grant-3", botId: "bot-9" };
    expect(liveGrantsFor([GRANT, wide, other], "prov-1", "bot-1").map((g) => g.id)).toEqual([
      "grant-1",
      "grant-2",
    ]);
  });

  it("drops a revoked grant, because the bar answers what it can reach now", () => {
    expect(liveGrantsFor([{ ...GRANT, revokedMs: 99 }], "prov-1", "bot-1")).toEqual([]);
  });
});

describe("BotGrantBar", () => {
  it("is absent entirely on a machine with no folder sync", () => {
    const { container } = render(
      <BotGrantBar sync={false} provider={OLLAMA} botId="bot-1" model={MODEL} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the Hermes sentence and offers no control", async () => {
    render(<BotGrantBar sync provider={HERMES} botId="bot-1" model={MODEL} />);
    expect(await screen.findByText(GRANT_HERMES_SENTENCE)).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByText(GRANT_NONE_HELD)).toBeNull();
    // Nothing was even read: there is no grant for a Hermes bot to hold.
    expect(botsGrantsList).not.toHaveBeenCalled();
  });

  it("offers no control when the model stated it cannot take tools", async () => {
    render(<BotGrantBar sync provider={OLLAMA} botId="bot-1" model={{ ...MODEL, tools: false }} />);
    expect(await screen.findByText(GRANT_NO_TOOLS_SENTENCE)).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("warns, and still offers the control, when the capability is unknown", async () => {
    render(<BotGrantBar sync provider={OLLAMA} botId="bot-1" model={{ ...MODEL, tools: null }} />);
    const warning = await screen.findByRole("status");
    expect(warning).toHaveTextContent(GRANT_TOOLS_UNKNOWN_SENTENCE);
    // Offered anyway: unknown is not no, and a grant here reaches nothing at
    // worst rather than stranding an endpoint that never stated its
    // capabilities.
    expect(screen.getByRole("button", { name: GRANT_ADD_LABEL })).toBeInTheDocument();
  });

  it("shows no warning where the endpoint stated the capability", async () => {
    render(<BotGrantBar sync provider={OLLAMA} botId="bot-1" model={MODEL} />);
    expect(await screen.findByText(GRANT_NONE_HELD)).toBeInTheDocument();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("says no grant is held, and offers the one control that creates one", async () => {
    render(<BotGrantBar sync provider={OLLAMA} botId="bot-1" model={MODEL} />);
    expect(await screen.findByText(GRANT_NONE_HELD)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: GRANT_ADD_LABEL })).toBeInTheDocument();
  });

  it("names what a held grant reaches, with a control to change it and one to revoke", async () => {
    botsGrantsList.mockResolvedValue(listing([GRANT]));
    render(<BotGrantBar sync provider={OLLAMA} botId="bot-1" model={MODEL} />);
    const list = await screen.findByRole("list", { name: GRANT_LIST_LABEL });
    expect(list).toHaveTextContent("This bot can read and write p1/journal/2026.");
    expect(screen.getByRole("button", { name: GRANT_CHANGE_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: GRANT_REVOKE_LABEL })).toBeInTheDocument();
  });

  it("revokes by id, and the bar re-reads instead of splicing its own list", async () => {
    botsGrantsList.mockResolvedValueOnce(listing([GRANT])).mockResolvedValue(listing([]));
    render(<BotGrantBar sync provider={OLLAMA} botId="bot-1" model={MODEL} />);
    fireEvent.click(await screen.findByRole("button", { name: GRANT_REVOKE_LABEL }));
    expect(botsGrantRevoke).toHaveBeenCalledWith("grant-1");
    await waitFor(() => expect(screen.getByText(GRANT_NONE_HELD)).toBeInTheDocument());
    expect(screen.queryByRole("list", { name: GRANT_LIST_LABEL })).toBeNull();
  });

  it("saves a changed grant as a rewrite of the same row, subtree as typed", async () => {
    botsGrantsList.mockResolvedValue(listing([GRANT]));
    render(<BotGrantBar sync provider={OLLAMA} botId="bot-1" model={MODEL} />);
    fireEvent.click(await screen.findByRole("button", { name: GRANT_CHANGE_LABEL }));
    fireEvent.change(screen.getByLabelText("Inside that folder"), {
      target: { value: "journal/2026/" },
    });
    fireEvent.click(screen.getByRole("button", { name: "read" }));
    fireEvent.click(screen.getByRole("button", { name: GRANT_SAVE_LABEL }));
    await waitFor(() =>
      expect(botsGrantSave).toHaveBeenCalledWith({
        id: "grant-1",
        providerId: "prov-1",
        botId: "bot-1",
        // Sent as typed: normalizing a permission path here would be a second
        // grammar beside Rust's.
        scope: { kind: "subtree", profileId: "p1", subpath: "journal/2026/" },
        mode: "read",
      } satisfies BotGrantSaveReq),
    );
  });

  it("creates a new grant with no id", async () => {
    render(<BotGrantBar sync provider={OLLAMA} botId="bot-1" model={MODEL} />);
    fireEvent.click(await screen.findByRole("button", { name: GRANT_ADD_LABEL }));
    fireEvent.change(screen.getByLabelText("Inside that folder"), {
      target: { value: "journal" },
    });
    fireEvent.click(screen.getByRole("button", { name: GRANT_SAVE_LABEL }));
    await waitFor(() =>
      expect(botsGrantSave).toHaveBeenCalledWith({
        id: null,
        providerId: "prov-1",
        botId: "bot-1",
        scope: { kind: "subtree", profileId: "p1", subpath: "journal" },
        mode: "read",
      } satisfies BotGrantSaveReq),
    );
  });

  it("renders Rust's refusal verbatim when a save is refused", async () => {
    botsGrantSave.mockRejectedValue({
      message: 'a tool path may not contain ".."',
    });
    render(<BotGrantBar sync provider={OLLAMA} botId="bot-1" model={MODEL} />);
    fireEvent.click(await screen.findByRole("button", { name: GRANT_ADD_LABEL }));
    fireEvent.change(screen.getByLabelText("Inside that folder"), {
      target: { value: "../etc" },
    });
    fireEvent.click(screen.getByRole("button", { name: GRANT_SAVE_LABEL }));
    expect(await screen.findByRole("alert")).toHaveTextContent('a tool path may not contain ".."');
  });
});
