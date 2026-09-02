/**
 * Settings → Grants: one list that answers "what can this bot change?", and a
 * log whose reader is a human (Epic 61, Story 61.10, FR-386, FR-388, NFR-47).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **A pending audit row renders as pending** — with its own sentence saying
 *    the row was written before the effect. Print it as a success and NFR-47's
 *    only observable property is gone.
 * 2. **A revoked grant is still a row, and says so** — the answer to "what can
 *    it change?" is a list with states, never a list that quietly shortens.
 * 3. **A grant row this build cannot read is shown, not skipped** — a
 *    permission the user can see and keeper ignores is a permission they
 *    believe they have.
 * 4. **Revoke removes the row by re-reading** — the section never splices its
 *    own list.
 * 5. **A grant outliving its endpoint is named honestly** rather than
 *    rendered as a blank group.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GRANT_REVOKE_LABEL } from "@/components/bots/bot-grant-bar";
import {
  AUDIT_EMPTY,
  AUDIT_PENDING_CAPTION,
  AUDIT_TITLE,
  auditLine,
  BotGrantsSection,
  GRANT_REVOKED_CAPTION,
  GRANT_UNREADABLE_CAPTION,
  GRANTS_EVERY_BOT,
  GRANTS_PROVIDER_GONE,
  GRANTS_SECTION_EMPTY,
  GRANTS_SECTION_TITLE,
} from "@/components/settings/bot-grants-section";
import type {
  BotAuditRowVm,
  BotGrantVm,
  BotProviderVm,
  BotVm,
  UnknownBotGrantVm,
} from "@/lib/ipc/client";

const botsGrantsList = vi.fn();
const botsGrantRevoke = vi.fn();
const botsProvidersList = vi.fn();
const botsBotsList = vi.fn();
const botsAuditList = vi.fn();

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsGrantsList: () => botsGrantsList(),
    botsGrantRevoke: (grantId: string) => botsGrantRevoke(grantId),
    botsProvidersList: () => botsProvidersList(),
    botsBotsList: () => botsBotsList(),
    botsAuditList: () => botsAuditList(),
  };
});

const PROVIDER: BotProviderVm = {
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

const BOT: BotVm = {
  id: "bot-1",
  providerId: "prov-1",
  target: "llama4:8b",
  name: "Llama",
  pinOrder: 0,
  shape: null,
  colour: null,
  mark: null,
  createdMs: 1,
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

const WIDE_GRANT: BotGrantVm = {
  ...GRANT,
  id: "grant-2",
  botId: null,
  scope: { kind: "drive" },
  scopeLabel: "the whole drive",
  mode: "read",
};

const UNKNOWN_GRANT: UnknownBotGrantVm = {
  id: "grant-9",
  providerId: "prov-1",
  scopeKind: "constellation",
  mode: "sideways",
};

const PENDING: BotAuditRowVm = {
  id: 3,
  startedMs: 300,
  finishedMs: null,
  providerId: "prov-1",
  botId: "bot-1",
  sessionId: "sess-1",
  messageId: null,
  tool: "write",
  path: "p1/journal/2026/monday.md",
  profileId: "p1",
  subpath: "journal/2026/monday.md",
  effect: "write",
  verdict: "allow",
  reason: null,
  grantId: "grant-1",
  outcome: "pending",
  bytes: null,
  truncated: false,
};

const REFUSED: BotAuditRowVm = {
  ...PENDING,
  id: 1,
  startedMs: 100,
  finishedMs: 190,
  tool: "read",
  path: "p2/2026/raw",
  profileId: "p2",
  subpath: "2026/raw",
  effect: "read",
  verdict: "deny",
  reason: "No grant covers this folder, so nothing was read or written.",
  grantId: null,
  outcome: "refused",
};

beforeEach(() => {
  botsGrantsList.mockReset();
  botsGrantRevoke.mockReset();
  botsProvidersList.mockReset();
  botsBotsList.mockReset();
  botsAuditList.mockReset();
  botsGrantsList.mockResolvedValue({ grants: [GRANT], unknown: [] });
  botsGrantRevoke.mockResolvedValue(undefined);
  botsProvidersList.mockResolvedValue([PROVIDER]);
  botsBotsList.mockResolvedValue([BOT]);
  botsAuditList.mockResolvedValue([PENDING, REFUSED]);
});

describe("auditLine", () => {
  it("names the path, the tool, the effect, the verdict and the outcome", () => {
    expect(auditLine(REFUSED)).toBe("p2/2026/raw — read (read), deny, refused");
  });

  it("prints an unreadable verdict as unreadable, never as one of the three", () => {
    expect(auditLine({ ...REFUSED, verdict: null, effect: null })).toBe(
      "p2/2026/raw — read (unreadable), unreadable, refused",
    );
  });
});

describe("BotGrantsSection", () => {
  it("reads nothing while the dialog is closed", () => {
    render(<BotGrantsSection open={false} />);
    expect(botsGrantsList).not.toHaveBeenCalled();
  });

  it("groups a grant under its endpoint and its bot", async () => {
    render(<BotGrantsSection open />);
    const list = await screen.findByRole("list", { name: GRANTS_SECTION_TITLE });
    expect(list).toHaveTextContent("Ollama here — ollama at localhost");
    expect(list).toHaveTextContent("Llama");
    expect(list).toHaveTextContent("This bot can read and write p1/journal/2026.");
  });

  it("names a provider-wide grant as covering every bot of the endpoint", async () => {
    botsGrantsList.mockResolvedValue({ grants: [WIDE_GRANT], unknown: [] });
    render(<BotGrantsSection open />);
    expect(await screen.findByText(GRANTS_EVERY_BOT)).toBeInTheDocument();
  });

  it("names an endpoint keeper no longer holds instead of rendering a blank group", async () => {
    botsProvidersList.mockResolvedValue([]);
    render(<BotGrantsSection open />);
    expect(await screen.findByText(`${GRANTS_PROVIDER_GONE} (prov-1)`)).toBeInTheDocument();
  });

  it("keeps a revoked grant as a row that says it is revoked, with nothing to revoke", async () => {
    botsGrantsList.mockResolvedValue({ grants: [{ ...GRANT, revokedMs: 99 }], unknown: [] });
    render(<BotGrantsSection open />);
    expect(await screen.findByText(GRANT_REVOKED_CAPTION)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: GRANT_REVOKE_LABEL })).toBeNull();
  });

  it("shows a grant row it cannot read, with revoke as its one action", async () => {
    botsGrantsList.mockResolvedValue({ grants: [], unknown: [UNKNOWN_GRANT] });
    render(<BotGrantsSection open />);
    expect(await screen.findByText(GRANT_UNREADABLE_CAPTION)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: GRANT_REVOKE_LABEL }));
    expect(botsGrantRevoke).toHaveBeenCalledWith("grant-9");
  });

  it("revokes a live grant by id and re-reads the list", async () => {
    botsGrantsList
      .mockResolvedValueOnce({ grants: [GRANT], unknown: [] })
      .mockResolvedValue({ grants: [], unknown: [] });
    render(<BotGrantsSection open />);
    fireEvent.click(await screen.findByRole("button", { name: GRANT_REVOKE_LABEL }));
    expect(botsGrantRevoke).toHaveBeenCalledWith("grant-1");
    await waitFor(() => expect(screen.getByText(GRANTS_SECTION_EMPTY)).toBeInTheDocument());
  });

  it("renders a pending row as pending, with the sentence saying what that means", async () => {
    render(<BotGrantsSection open />);
    const log = await screen.findByRole("list", { name: AUDIT_TITLE });
    expect(log).toHaveTextContent(auditLine(PENDING));
    expect(screen.getByText(AUDIT_PENDING_CAPTION)).toBeInTheDocument();
  });

  it("keeps the log newest first, as Rust returned it", async () => {
    render(<BotGrantsSection open />);
    const rows = await screen.findAllByRole("listitem");
    const log = rows.filter((row) => row.textContent?.includes(" — "));
    const newest = log.findIndex((row) => row.textContent?.includes("monday.md"));
    const oldest = log.findIndex((row) => row.textContent?.includes("p2/2026/raw"));
    expect(newest).toBeLessThan(oldest);
  });

  it("quotes a refusal sentence from the row rather than rewriting it", async () => {
    render(<BotGrantsSection open />);
    expect(
      await screen.findByText("No grant covers this folder, so nothing was read or written."),
    ).toBeInTheDocument();
  });

  it("says plainly when nothing has been granted and nothing has been checked", async () => {
    botsGrantsList.mockResolvedValue({ grants: [], unknown: [] });
    botsAuditList.mockResolvedValue([]);
    render(<BotGrantsSection open />);
    expect(await screen.findByText(GRANTS_SECTION_EMPTY)).toBeInTheDocument();
    expect(screen.getByText(AUDIT_EMPTY)).toBeInTheDocument();
  });

  it("prints Rust's sentence when a read was refused", async () => {
    botsGrantsList.mockRejectedValue({ message: "keeper couldn't open the grant table" });
    render(<BotGrantsSection open />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "keeper couldn't open the grant table",
    );
  });
});
