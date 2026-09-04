/**
 * The Bots surface on a phone (Epic 62, Story 62.2, FR-397, FR-398).
 *
 * Rendered through the real `PhoneShell`, because the story is that the
 * surface rides the shell's own stack rather than a navigation of its own.
 * Asserted here and nowhere else:
 *
 * 1. **Absent, not disabled** — with `capabilities.bots` off a stale "bots"
 *    primary view pushes no level at all.
 * 2. **One thing at a time** — the list is level 1, a conversation is level 2,
 *    and back pops one level each time, the last pop returning the view to
 *    the Inbox. A selected room outranks the view.
 * 3. **The transcript gets the height** (Story 61.14, held here) — jsdom lays
 *    nothing out, so this is structural: the conversation column has exactly
 *    one flexible child, it is the scroll region, every other band is
 *    `shrink-0`, and there are three bands in the resting state.
 * 4. **No grant affordance** — `botTools` is false on a phone; a conversation
 *    streams and closes with no grant bar drawn and no grant or deliverable
 *    read ever made.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BOT_COMPOSER_LABEL,
  BOT_COMPOSER_SEND_LABEL,
  botComposerNoBot,
} from "@/components/bots/bot-composer";
import { BOT_CONVERSATION_LABEL } from "@/components/bots/bot-conversation";
import { BOTS_EMPTY_COPY } from "@/components/bots/bot-empty-state";
import { GRANT_ADD_LABEL, GRANT_NONE_HELD } from "@/components/bots/bot-grant-bar";
import { BOT_PICKER_BOT_LABEL } from "@/components/bots/bot-picker";
import { BOT_PINS_LABEL } from "@/components/bots/bot-pins-strip";
import { BOT_SESSION_NEW_LABEL } from "@/components/bots/bot-session-list";
import { VOICE_LOCALE_LABEL } from "@/components/bots/bot-voice-wake";
import { BOTS_PANE_TITLE } from "@/components/bots/bots-pane";
import {
  BOTS_PHONE_BACK_TO_INBOX,
  BOTS_PHONE_BACK_TO_LIST,
  BOTS_PHONE_CONVERSATION_SLOT,
  BOTS_PHONE_PICKER_LABEL,
  BOTS_PHONE_PICKER_PLACE,
} from "@/components/bots/bots-phone-pane";
import { SETTINGS_PANE_TITLE } from "@/components/layout/settings-pane";
import type {
  BotMessageVm,
  BotModelVm,
  BotProviderVm,
  BotSessionVm,
  BotStreamEvent,
  BotVm,
  VoiceUnavailableVm,
  VoiceWakeVm,
} from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { botsStore } from "@/lib/stores/bots";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { detailStore } from "@/lib/stores/detail-ui";
import { leadingDrawerStore } from "@/lib/stores/leading-drawer";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import { voiceStore } from "@/lib/stores/voice";

const botsGrantsList = vi.fn();
const botsDeliverablePaths = vi.fn();
const botsSessionOpen = vi.fn();
const voiceAvailability = vi.fn<() => Promise<VoiceUnavailableVm | null>>();
const voiceWakeGet = vi.fn<() => Promise<VoiceWakeVm>>();
/** The event sink the level handed to `botsChatSend`, driven as Rust would. */
let sink: ((event: BotStreamEvent) => void) | null = null;

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    // The shell's always-mounted chat surfaces (the phone-shell suite's stubs).
    subscribeInbox: vi.fn(async (): Promise<number> => 1),
    unsubscribeInbox: vi.fn(async (): Promise<void> => {}),
    listDrafts: vi.fn(async (): Promise<Array<[string, string]>> => []),
    getFavoritesCollapsed: vi.fn(async (): Promise<boolean> => false),
    subscribeDraftMirror: vi.fn(async (): Promise<number> => 1),
    unsubscribeDraftMirror: vi.fn(async (): Promise<void> => {}),
    subscribeTimeline: vi.fn(async (): Promise<number> => 1),
    unsubscribeTimeline: vi.fn(async (): Promise<void> => {}),
    subscribeTyping: vi.fn(async (): Promise<number> => 1),
    unsubscribeTyping: vi.fn(async (): Promise<void> => {}),
    subscribePaginationStatus: vi.fn(async (): Promise<number> => 1),
    unsubscribePaginationStatus: vi.fn(async (): Promise<void> => {}),
    subscribeOutbox: vi.fn(async (): Promise<number> => 1),
    unsubscribeOutbox: vi.fn(async (): Promise<void> => {}),
    markRoomRead: vi.fn(async (): Promise<void> => {}),
    couplingCaveats: vi.fn(async () => []),
    loadDraft: vi.fn(async (): Promise<string | null> => null),
    encryptionPosture: vi.fn(() => Promise.resolve(false)),
    paletteQuery: vi.fn(async () => ({ contacts: [], chats: [], actions: [] })),
    searchArchive: vi.fn(async () => []),
    // The Bots reads.
    botsProvidersList: () => Promise.resolve([PROVIDER]),
    botsBotsList: () => Promise.resolve([BOT]),
    botsSessionsList: () => Promise.resolve([SESSION]),
    botsSessionsSearch: () =>
      Promise.resolve({
        rows: [{ session: SESSION, messageCount: 2, latestActivityMs: 1, transcript: "local" }],
        total: 1,
      }),
    botsModelsList: () => Promise.resolve([MODEL]),
    botsMessageDetailsGet: () => Promise.resolve(false),
    botsSessionOpen: (id: string) => {
      botsSessionOpen(id);
      return Promise.resolve({
        session: SESSION,
        messages: [QUESTION, ANSWER_DONE],
        transcript: "local",
      });
    },
    botsChatSend: (_req: unknown, onEvent: (event: BotStreamEvent) => void) => {
      sink = onEvent;
      return Promise.resolve(SUBSCRIPTION_ID);
    },
    botsCommandPreview: () =>
      Promise.resolve({ draft: "", verdict: { kind: "prose", text: "" }, escapeHint: "" }),
    // The drive half. Must never be reached from a phone.
    botsGrantsList: () => {
      botsGrantsList();
      return Promise.resolve({ grants: [], unknown: [] });
    },
    botsDeliverablePaths: (sessionId: string, body: string) => {
      botsDeliverablePaths(sessionId, body);
      return Promise.resolve([]);
    },
    // Voice (Stories 62.5/62.6): unanswered by default, so the affordances
    // stay absent and the column below is the resting three bands; the
    // language test answers them.
    voiceAvailability: () => voiceAvailability(),
    voiceWatch: () => Promise.resolve(1),
    voiceUnwatch: () => Promise.resolve(),
    voiceWakeGet: () => voiceWakeGet(),
  };
});

vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => vi.fn(),
}));

vi.mock("@/hooks/use-stale-resume-pill", () => ({
  useStaleResumePill: () => false,
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn((_handler?: (e: unknown) => void) => Promise.resolve(() => {})),
  }),
}));

import { PhoneShell } from "@/components/layout/phone-shell";

const SUBSCRIPTION_ID = "sub-1";

const PROVIDER: BotProviderVm = {
  id: "prov-1",
  kind: "hermes",
  name: "Hermes at home",
  baseUrl: "https://hermes.example",
  host: "hermes.example",
  isPrivate: false,
  createdMs: 1,
  health: "reachable",
  healthCheckedMs: 2,
  healthDetail: null,
  readTimeoutMs: null,
  hasToken: true,
};

const BOT: BotVm = {
  id: "bot-1",
  providerId: "prov-1",
  target: "assistant",
  name: "Hermes",
  pinOrder: 0,
  shape: null,
  colour: null,
  mark: null,
  createdMs: 1,
};

const MODEL: BotModelVm = {
  id: "hermes-3",
  family: null,
  parameterSize: null,
  quantization: null,
  sizeBytes: null,
  contextWindow: null,
  maxOutputTokens: null,
  vision: false,
  tools: true,
  reasoning: false,
  capabilities: ["completion", "tools"],
};

const SESSION: BotSessionVm = {
  id: "sess-1",
  botId: "bot-1",
  providerId: "prov-1",
  title: "What changed?",
  createdMs: 1,
  updatedMs: 1,
  archived: false,
  remoteSessionId: null,
  remoteLastActiveMs: null,
  remoteSource: null,
};

function message(overrides: Partial<BotMessageVm> & { id: string; role: string }): BotMessageVm {
  return {
    sessionId: "sess-1",
    seq: 0,
    content: "",
    model: null,
    providerId: null,
    promptTokens: null,
    completionTokens: null,
    totalTokens: null,
    ttftMs: null,
    durationMs: null,
    finishReason: null,
    requestId: null,
    toolCallCount: 0,
    partial: false,
    createdMs: 1,
    ...overrides,
  };
}

const QUESTION = message({ id: "msg-user", role: "user", seq: 0, content: "What changed?" });
const ANSWER = message({ id: "msg-answer", role: "assistant", seq: 1, partial: true });
const ANSWER_DONE = message({
  id: "msg-answer",
  role: "assistant",
  seq: 1,
  content: "Two files.",
});

/** A phone: it can talk to a model and cannot reach the drive. */
const PHONE = { ...DEFAULT_CAPABILITIES, bots: true };

const originalMatchMedia = window.matchMedia;
function mockPhoneViewport() {
  window.matchMedia = vi.fn().mockImplementation((query: string) => {
    const match = query.match(/max-width:\s*(\d+)px/);
    const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
    const matches = query.includes("prefers-reduced-motion") ? true : 390 <= maxWidth;
    return {
      matches,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    };
  });
}

/** The stack-level wrapper for the given level, or `null` when unmounted. */
function stackLevel(level: 0 | 1 | 2): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-level="${level}"]`);
}

/** Push the Bots view and wait for the list level to hold its rows. */
async function openBots() {
  act(() => {
    primaryViewStore.getState().setView("bots");
  });
  await screen.findByRole("button", { name: BOTS_PHONE_BACK_TO_INBOX });
  await screen.findByRole("button", { name: /^What changed\?/ });
}

/** From the list, open the one conversation and wait for the level to land. */
async function openConversation() {
  fireEvent.click(screen.getByRole("button", { name: /^What changed\?/ }));
  await screen.findByRole("button", { name: BOTS_PHONE_BACK_TO_LIST });
  await waitFor(() => expect(botsStore.getState().selectedModel).toBe(MODEL.id));
}

beforeEach(() => {
  mockPhoneViewport();
  sink = null;
  botsGrantsList.mockClear();
  botsDeliverablePaths.mockClear();
  botsSessionOpen.mockClear();
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  detailStore.setState({ open: false });
  leadingDrawerStore.getState().close();
  primaryViewStore.getState().setView("inbox");
  botsStore.getState().reset();
  voiceStore.getState().reset();
  voiceStore.setState({ unavailable: undefined, wake: null });
  voiceAvailability.mockReset();
  voiceWakeGet.mockReset();
  voiceAvailability.mockRejectedValue(new Error("not answered"));
  voiceWakeGet.mockRejectedValue(new Error("not answered"));
  capabilitiesStore.getState().applySnapshot(PHONE);
});

afterEach(() => {
  window.matchMedia = originalMatchMedia;
  primaryViewStore.getState().setView("inbox");
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  botsStore.getState().reset();
  vi.restoreAllMocks();
});

describe("the Bots view on the phone stack", () => {
  it("pushes nothing when the capability is off", () => {
    // Absent, not disabled: a stale "bots" view on a build with no surface
    // leaves the stack at the Inbox with no level above it.
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });
    primaryViewStore.getState().setView("bots");
    render(<PhoneShell />);
    expect(stackLevel(1)).toBeNull();
    expect(
      screen.queryByRole("button", { name: BOTS_PHONE_BACK_TO_INBOX }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: BOTS_PANE_TITLE })).not.toBeInTheDocument();
  });

  it("is the list at level 1, the conversation at level 2, and back pops one level at a time", async () => {
    render(<PhoneShell />);
    expect(stackLevel(1)).toBeNull();
    await openBots();

    // Level 1: the list, with the shell's back bar and the desktop list's
    // own controls — and no conversation level yet.
    const list = screen.getByRole("region", { name: BOTS_PANE_TITLE });
    expect(stackLevel(1)).toContainElement(list);
    expect(within(list).getByRole("button", { name: BOT_SESSION_NEW_LABEL })).toBeInTheDocument();
    expect(stackLevel(2)).toBeNull();
    expect(screen.queryByRole("textbox", { name: BOT_COMPOSER_LABEL })).not.toBeInTheDocument();

    // Open the conversation: the read lands first, then level 2 pushes over
    // the still-mounted list.
    await openConversation();
    expect(botsSessionOpen).toHaveBeenCalledWith(SESSION.id);
    const conversation = document.querySelector(`[data-slot="${BOTS_PHONE_CONVERSATION_SLOT}"]`);
    expect(stackLevel(2)).toContainElement(conversation as HTMLElement);
    expect(screen.getByRole("list", { name: BOT_CONVERSATION_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: BOT_COMPOSER_LABEL })).toBeInTheDocument();
    expect(stackLevel(1)).toHaveAttribute("inert");

    // Back once: the conversation level goes, the list is live again, the
    // view is still Bots.
    fireEvent.click(screen.getByRole("button", { name: BOTS_PHONE_BACK_TO_LIST }));
    await waitFor(() => expect(stackLevel(2)).toBeNull());
    expect(stackLevel(1)).not.toHaveAttribute("inert");
    expect(primaryViewStore.getState().view).toBe("bots");

    // Back again: the view returns to the Inbox and level 1 unmounts.
    fireEvent.click(screen.getByRole("button", { name: BOTS_PHONE_BACK_TO_INBOX }));
    expect(primaryViewStore.getState().view).toBe("inbox");
    await waitFor(() => expect(stackLevel(1)).toBeNull());
  });

  it("re-enters on the list, never on the conversation left open", async () => {
    render(<PhoneShell />);
    await openBots();
    await openConversation();
    // Leave by the view (as the drawer would) rather than by back.
    act(() => {
      primaryViewStore.getState().setView("inbox");
    });
    await waitFor(() => expect(stackLevel(1)).toBeNull());
    await openBots();
    expect(stackLevel(2)).toBeNull();
    expect(screen.queryByRole("button", { name: BOTS_PHONE_BACK_TO_LIST })).not.toBeInTheDocument();
  });

  it("pushes the conversation from New with nothing asked yet", async () => {
    render(<PhoneShell />);
    await openBots();
    fireEvent.click(screen.getByRole("button", { name: BOT_SESSION_NEW_LABEL }));
    await screen.findByRole("button", { name: BOTS_PHONE_BACK_TO_LIST });
    expect(screen.getByText(BOTS_EMPTY_COPY["no-conversation"].message)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: BOT_COMPOSER_LABEL })).toBeInTheDocument();
  });

  it("yields to a selected room: a notification tap lands on the Room, not on Bots", async () => {
    render(<PhoneShell />);
    await openBots();
    act(() => {
      roomsStore.getState().selectRoom({ accountId: "acc", roomId: "!r:example.org" });
    });
    await screen.findByRole("main");
    expect(screen.queryByRole("region", { name: BOTS_PANE_TITLE })).not.toBeInTheDocument();
    // Popping the room returns to the Bots list that was under it.
    fireEvent.click(screen.getByRole("button", { name: BOTS_PHONE_BACK_TO_INBOX }));
    await screen.findByRole("region", { name: BOTS_PANE_TITLE });
    expect(primaryViewStore.getState().view).toBe("bots");
  });

  it("opens the bot and model choice as a sheet from the conversation header", async () => {
    render(<PhoneShell />);
    await openBots();
    await openConversation();
    // The header names what is chosen, and no picker row sits above the
    // transcript.
    expect(screen.queryByRole("combobox", { name: BOT_PICKER_BOT_LABEL })).not.toBeInTheDocument();
    const trigger = screen.getByRole("button", { name: BOTS_PHONE_PICKER_LABEL });
    expect(trigger).toHaveTextContent(`${BOT.name} · ${MODEL.id}`);
    fireEvent.click(trigger);
    const sheet = await screen.findByRole("dialog", { name: BOTS_PHONE_PICKER_LABEL });
    expect(within(sheet).getByRole("combobox", { name: BOT_PICKER_BOT_LABEL })).toBeInTheDocument();
  });

  /**
   * Epic 63: the language control is reached through the sheet, beside the
   * wake phrase, with the list the phone reported — never the model's.
   */
  it("reaches the language control through the Bot and model sheet", async () => {
    voiceAvailability.mockResolvedValue(null);
    voiceWakeGet.mockResolvedValue({
      enabled: false,
      phrase: "nixie",
      limits: "limits",
      locale: "en-US",
      localeChosen: null,
      onDeviceLocales: ["en-US"],
    });
    render(<PhoneShell />);
    await openBots();
    await openConversation();
    fireEvent.click(screen.getByRole("button", { name: BOTS_PHONE_PICKER_LABEL }));
    const sheet = await screen.findByRole("dialog", { name: BOTS_PHONE_PICKER_LABEL });
    const control = await within(sheet).findByRole("combobox", { name: VOICE_LOCALE_LABEL });
    expect(control).toHaveValue("");
    expect(within(control).getAllByRole("option")).toHaveLength(2);
  });

  /**
   * Story 63.1, FR-412: the pinned bots are reachable at level 1. They sit on
   * the list level, above the rows, and a tap is "talk to this bot": the bot
   * is chosen and a fresh conversation is pushed. Not on the conversation
   * level, where the band would be transcript height (see the height contract
   * below, which still counts three bands there).
   */
  it("reaches the pinned bots at level 1, and a pin starts a conversation with that bot", async () => {
    render(<PhoneShell />);
    await openBots();
    const list = screen.getByRole("region", { name: BOTS_PANE_TITLE });
    const pins = within(list).getByRole("navigation", { name: BOT_PINS_LABEL });
    expect(stackLevel(1)).toContainElement(pins);
    expect(stackLevel(2)).toBeNull();
    // The strip is a bounded band on the list level; the rows keep the height.
    expect(pins.parentElement).toHaveClass("shrink-0");

    act(() => {
      botsStore.getState().selectBot(null);
    });
    fireEvent.click(within(pins).getByRole("button", { name: new RegExp(`^${BOT.name}`) }));
    await screen.findByRole("button", { name: BOTS_PHONE_BACK_TO_LIST });
    expect(botsStore.getState().selectedBotId).toBe(BOT.id);
    expect(botsStore.getState().conversation).toBeNull();
    expect(screen.getByRole("textbox", { name: BOT_COMPOSER_LABEL })).toBeInTheDocument();
  });

  /**
   * Story 63.1, FR-411: the composer's no-bot caption names the sheet, because
   * "above" on this column is a back bar. Asserted with the model unchosen,
   * which is the disabled state the caption is shown in.
   */
  it("names the sheet, not 'above', while the composer has nothing to send to", async () => {
    render(<PhoneShell />);
    await openBots();
    fireEvent.click(screen.getByRole("button", { name: BOT_SESSION_NEW_LABEL }));
    await screen.findByRole("button", { name: BOTS_PHONE_BACK_TO_LIST });
    act(() => {
      botsStore.getState().selectBot(null);
    });
    expect(screen.getByText(botComposerNoBot(BOTS_PHONE_PICKER_PLACE))).toBeInTheDocument();
    expect(screen.getByText(/in the Bot and model sheet/)).toBeInTheDocument();
    expect(screen.queryByText(/Choose a bot above/)).not.toBeInTheDocument();
  });
});

describe("the phone conversation's height contract (Story 61.14)", () => {
  it("is one flexible scroll region between bounded bands, three bands at rest", async () => {
    // jsdom lays nothing out: the pixels were measured on the desktop pane
    // through `dev/mock-shell.ts`, and this guards the classes those pixels
    // were measured over. Not asserted here: that any band is 52px, or that
    // the composer clears the keyboard — only a browser can say.
    render(<PhoneShell />);
    await openBots();
    await openConversation();
    const column = document.querySelector<HTMLElement>(
      `[data-slot="${BOTS_PHONE_CONVERSATION_SLOT}"]`,
    );
    expect(column).not.toBeNull();
    expect(column).toHaveClass("flex", "flex-col", "flex-1", "min-h-0");

    const children = [...(column as HTMLElement).children];
    const flexible = children.filter((child) => child.classList.contains("flex-1"));
    expect(flexible).toHaveLength(1);
    // The flexible child IS the transcript's scroll region.
    expect(flexible[0]).toHaveClass("min-h-0", "overflow-y-auto");
    expect(flexible[0]).toContainElement(
      screen.getByRole("list", { name: BOT_CONVERSATION_LABEL }),
    );
    for (const child of children) {
      if (child !== flexible[0]) {
        expect(child).toHaveClass("shrink-0");
      }
    }
    // At rest — no error, no voice caption — the column is the header, the
    // transcript and the composer: the composer plus at most one caption.
    expect(children).toHaveLength(3);
    expect(children[0]).toContainElement(
      screen.getByRole("button", { name: BOTS_PHONE_BACK_TO_LIST }),
    );
    expect(children[2]).toContainElement(screen.getByRole("textbox", { name: BOT_COMPOSER_LABEL }));
  });
});

describe("what a phone does not have", () => {
  it("has NO grant affordance, and the conversation still streams and closes", async () => {
    render(<PhoneShell />);
    await openBots();
    await openConversation();

    // The desktop's `botTools` gate, proven from the phone side: no bar text,
    // and the grant table is never read.
    expect(screen.queryByText(GRANT_NONE_HELD)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: GRANT_ADD_LABEL })).not.toBeInTheDocument();
    expect(botsGrantsList).not.toHaveBeenCalled();

    // Send, stream, close — the same events the desktop pane handles.
    fireEvent.change(screen.getByRole("textbox", { name: BOT_COMPOSER_LABEL }), {
      target: { value: "And now?" },
    });
    fireEvent.click(screen.getByRole("button", { name: BOT_COMPOSER_SEND_LABEL }));
    await waitFor(() => expect(sink).not.toBeNull());
    const question = message({ id: "msg-2", role: "user", seq: 2, content: "And now?" });
    const answer = message({ id: "msg-3", role: "assistant", seq: 3, partial: true });
    act(() => {
      sink?.({
        kind: "opened",
        session: SESSION,
        user: question,
        assistant: answer,
        subscriptionId: SUBSCRIPTION_ID,
      });
      sink?.({ kind: "delta", text: "Three files." });
      sink?.({
        kind: "closed",
        message: { ...answer, content: "Three files.", partial: false },
        reason: null,
      });
    });
    await screen.findByText("Three files.");

    // A closed answer on the desktop asks for its deliverable paths; on a
    // phone that read does not exist and is never made.
    expect(botsDeliverablePaths).not.toHaveBeenCalled();
    expect(botsGrantsList).not.toHaveBeenCalled();
    expect(screen.queryByText(GRANT_NONE_HELD)).not.toBeInTheDocument();
  });
});

describe("the Settings view on the phone stack", () => {
  it("is a level 1 with a back bar, so the empty state's action lands somewhere", async () => {
    render(<PhoneShell />);
    act(() => {
      primaryViewStore.getState().setView("settings");
    });
    await screen.findByRole("region", { name: SETTINGS_PANE_TITLE });
    expect(stackLevel(1)).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: BOTS_PHONE_BACK_TO_INBOX }));
    expect(primaryViewStore.getState().view).toBe("inbox");
    await waitFor(() => expect(stackLevel(1)).toBeNull());
  });
});

describe("the phone stack with the flag off, for the record", () => {
  it("does not mount the list or the conversation for any store state", () => {
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });
    botsStore.getState().applySessions([SESSION]);
    botsStore.getState().openConversation({
      session: SESSION,
      messages: [QUESTION, ANSWER],
      transcript: "local",
    });
    primaryViewStore.getState().setView("bots");
    render(<PhoneShell />);
    expect(screen.queryByRole("list", { name: BOT_CONVERSATION_LABEL })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: BOT_SESSION_NEW_LABEL })).not.toBeInTheDocument();
  });
});
