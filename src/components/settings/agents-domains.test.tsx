import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AgentsDomains,
  DOMAINS_EMPTY,
  DOMAINS_READ_FAILED,
  DOMAINS_READING,
  DOMAINS_REMOVED,
  DOMAINS_TITLE,
  LEFT_TITLE,
  leftNote,
  ONLY_YOU,
  offlineNote,
  replacesNote,
} from "@/components/settings/agents-domains";
import { type DomainsVm, type DomainVm, hqDomainsSubscribe } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  hqDomainsSubscribe: vi.fn(),
  hqDomainsUnsubscribe: vi.fn(() => Promise.resolve()),
}));

const me = { actorId: "ana", name: "Ana", me: true };
const bo = { actorId: "bo", name: "Bo", me: false };

const domain = (name: string, kind: DomainVm["kind"], extra: Partial<DomainVm> = {}): DomainVm => ({
  name,
  displayName: name.toUpperCase(),
  kind,
  members: [me, bo],
  ...extra,
});

const LISTED: DomainsVm = {
  state: "current",
  readAtMs: 1_700_000_000_000,
  domains: [
    domain("tgdrive", "personal", {
      members: [me],
      replacedRepository: "https://forge.example/tgdrive.git",
      filesWrites: "shadow",
    }),
    domain("neuradrive", "group"),
    domain("makidrive", "general"),
    domain("crew", "group", { members: null }),
  ],
  left: [],
};

function shows(view: DomainsVm) {
  vi.mocked(hqDomainsSubscribe).mockImplementation((onDomains) => {
    onDomains(view);
    return Promise.resolve("sub");
  });
  return render(<AgentsDomains open />);
}

describe("AgentsDomains (114.5-AC1, AC3, AC4, AC6, AC9)", () => {
  beforeEach(() => vi.clearAllMocks());

  it("lists each domain with its kind and its people, and a one-person domain says so", () => {
    shows(LISTED);
    const list = screen.getByRole("list", { name: DOMAINS_TITLE });
    expect(within(list).getAllByRole("listitem")).toHaveLength(4);
    const tg = within(list).getByRole("listitem", { name: "TGDRIVE" });
    expect(tg).toHaveTextContent("Personal");
    expect(tg).toHaveTextContent(ONLY_YOU);
    expect(tg).toHaveTextContent(replacesNote("https://forge.example/tgdrive.git"));
    expect(within(list).getByRole("listitem", { name: "NEURADRIVE" })).toHaveTextContent("You, Bo");
    expect(within(list).getByRole("listitem", { name: "MAKIDRIVE" })).toHaveTextContent("Everyone");
    expect(within(list).getByRole("listitem", { name: "CREW" })).toHaveTextContent(
      "People not on this device yet",
    );
    // Only a domain that replaces a repository says so; never as cut over.
    expect(screen.getAllByText(/Replaces the git repository/)).toHaveLength(1);
  });

  it("offline, shows the last known with the time it was read", () => {
    shows({ ...LISTED, state: "offline" });
    expect(screen.getByRole("status")).toHaveTextContent(offlineNote(LISTED.readAtMs));
    expect(screen.getByRole("list", { name: DOMAINS_TITLE })).toBeInTheDocument();
  });

  it("removed, keeps the last known and says the copies stay", () => {
    shows({ ...LISTED, state: "removed" });
    expect(screen.getByRole("status")).toHaveTextContent(DOMAINS_REMOVED);
    expect(screen.getAllByRole("listitem")).toHaveLength(4);
  });

  it("says a domain was left, what remains and the writes that went with it", () => {
    const left = {
      name: "scratch",
      displayName: "Scratch",
      kind: "group" as const,
      unsent: 2,
      leftAtMs: 5,
    };
    shows({ ...LISTED, left: [left] });
    const leftList = screen.getByRole("list", { name: LEFT_TITLE });
    const row = within(leftList).getByRole("listitem", { name: "Scratch" });
    expect(row).toHaveTextContent("You left Scratch");
    expect(row).toHaveTextContent("2 writes not yet sent went with it");
    expect(leftNote({ ...left, unsent: 0 })).not.toMatch(/not yet sent/);
  });

  it("reads, is empty, renders nothing unconnected, and says when it cannot read", async () => {
    const { unmount } = shows({ ...LISTED, state: "reading", domains: [] });
    expect(screen.getByRole("status")).toHaveTextContent(DOMAINS_READING);
    unmount();
    const empty = shows({ ...LISTED, domains: [] });
    expect(screen.getByText(DOMAINS_EMPTY)).toBeInTheDocument();
    empty.unmount();
    const none = shows({ ...LISTED, state: "notConnected" });
    expect(none.container).toBeEmptyDOMElement();
    none.unmount();
    vi.mocked(hqDomainsSubscribe).mockRejectedValue(new Error("down"));
    render(<AgentsDomains open />);
    expect(await screen.findByRole("alert")).toHaveTextContent(DOMAINS_READ_FAILED);
  });
});
