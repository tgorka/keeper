/**
 * Settings → Bots: add, test, edit and remove an endpoint and a bot (Epic 61,
 * Story 61.4, FR-379).
 *
 * What is asserted here, beyond that the controls exist:
 *
 * - **AD-C7's shape** — one component in two modes, revealed as an inline
 *   disclosure and never a dialog. Asserted mechanically, the way
 *   `tasks-pane.test.tsx` does it: no `role="dialog"` while a form is open.
 *   A destructive confirm *is* a dialog, and that is asserted too.
 * - **The base URL is not validated here** — the field is sent as typed and
 *   Rust's own refusal sentence is rendered verbatim. A form that rejected
 *   `ftp://…` itself would be a second grammar.
 * - **A blank key field means unchanged, never cleared** — there is no field a
 *   stored token could arrive in, so an empty box must not unauthenticate a
 *   working endpoint.
 * - **Disclosure, not a blocklist** — a loopback endpoint is accepted and its
 *   host is printed, never its URL.
 * - **The probe speaks the app's own reachability vocabulary** — `offline` for
 *   a remote that did not answer, which is the word `keeper-sync` and the
 *   connection pill already use.
 * - **A lost secret says so** — `secretMissing` renders its own sentence,
 *   which is distinct from "no key stored".
 * - **The phone runs the same component** (Epic 62, Story 62.3, FR-399) — a
 *   Hermes endpoint with its key and a profile-addressed bot go through the
 *   same form and the same probe vocabulary with the capability mirror at the
 *   phone tier, and nothing in this section is gated on the drive half.
 * - **The wake switch lives here on the Mac** (Epic 63, Story 63.5, AD-179):
 *   present on `voice_availability`'s answer alone — absent for `unsupported`,
 *   drawn for a real availability — with no capability flag consulted.
 * - **The language control is reached through Settings on the phone tier**
 *   (Epic 63): the same section, the phone's capability mirror, and the list
 *   the device reported — so a Polish phone with English assets can pick
 *   English here as well as in the sheet.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { VOICE_LOCALE_LABEL, WAKE_SWITCH_LABEL } from "@/components/bots/bot-voice-wake";
import {
  BOTS_ADD_BOT_LABEL,
  BOTS_ADD_PROVIDER_LABEL,
  BOTS_BASE_URL_LABEL,
  BOTS_EDIT_LABEL,
  BOTS_NAME_LABEL,
  BOTS_NO_TOKEN_CAPTION,
  BOTS_REMOVE_LABEL,
  BOTS_SAVE_LABEL,
  BOTS_SECRET_MISSING_CAPTION,
  BOTS_SECTION_EMPTY,
  BOTS_SECTION_TITLE,
  BOTS_TARGET_LABEL,
  BOTS_TEST_LABEL,
  BOTS_TOKEN_LABEL,
  BotsSection,
  botProbeSentence,
  removalSentence,
} from "@/components/settings/bots-section";
import type {
  BotProbeVm,
  BotProviderSaveReq,
  BotProviderVm,
  BotSaveReq,
  BotVm,
} from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { voiceStore } from "@/lib/stores/voice";

const botsProvidersList = vi.fn();
const botsBotsList = vi.fn();
const botsProviderSave = vi.fn();
const botsProviderRemove = vi.fn();
const botsProviderProbe = vi.fn();
const botsBotProbe = vi.fn();
const botsBotSave = vi.fn();
const botsBotRemove = vi.fn();
const voiceAvailability = vi.fn();
const voiceWakeGet = vi.fn();

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsProvidersList: () => botsProvidersList(),
    botsBotsList: () => botsBotsList(),
    botsProviderSave: (req: BotProviderSaveReq) => botsProviderSave(req),
    botsProviderRemove: (id: string) => botsProviderRemove(id),
    botsProviderProbe: (id: string) => botsProviderProbe(id),
    botsBotProbe: (providerId: string, target: string) => botsBotProbe(providerId, target),
    botsBotSave: (req: unknown) => botsBotSave(req),
    botsBotRemove: (id: string) => botsBotRemove(id),
    voiceAvailability: () => voiceAvailability(),
    voiceWakeGet: () => voiceWakeGet(),
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

beforeEach(() => {
  botsProvidersList.mockResolvedValue([PROVIDER]);
  botsBotsList.mockResolvedValue([BOT]);
  botsProviderSave.mockResolvedValue(PROVIDER);
  botsProviderRemove.mockResolvedValue(undefined);
  botsBotSave.mockResolvedValue(BOT);
  botsBotRemove.mockResolvedValue(undefined);
  botsProviderProbe.mockResolvedValue({
    reach: "online",
    status: 200,
    version: "0.33.2",
    roundTripMs: 4,
    bot: null,
    presence: null,
    reason: null,
  } satisfies BotProbeVm);
  botsBotProbe.mockResolvedValue({
    reach: "online",
    status: 200,
    version: null,
    roundTripMs: 6,
    bot: "llama4:8b",
    presence: "exists",
    reason: null,
  } satisfies BotProbeVm);
  // Every build without a voice port: the wake control renders itself away.
  voiceAvailability.mockResolvedValue({
    kind: "unsupported",
    message: "voice is not available in this build",
  });
  voiceWakeGet.mockResolvedValue({
    enabled: false,
    phrase: "hey nixie",
    limits: "limits",
    locale: "en-US",
    localeChosen: null,
    onDeviceLocales: ["en-US"],
  });
  voiceStore.setState({ state: null, unavailable: undefined, wake: null });
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: true });
});

afterEach(() => {
  vi.clearAllMocks();
  voiceStore.setState({ state: null, unavailable: undefined, wake: null });
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("BotsSection", () => {
  it("renders nothing to read until it is opened", () => {
    render(<BotsSection open={false} />);
    expect(screen.getByText(BOTS_SECTION_TITLE)).toBeInTheDocument();
    expect(botsProvidersList).not.toHaveBeenCalled();
    // No rows read yet, so the honest state is the empty one rather than a
    // list that appears to arrive by itself.
    expect(screen.getByText(BOTS_SECTION_EMPTY)).toBeInTheDocument();
  });

  it("shows no wake switch while voice_availability answers unsupported (Story 63.5)", async () => {
    render(<BotsSection open />);
    await waitFor(() => expect(voiceAvailability).toHaveBeenCalled());
    await waitFor(() => expect(voiceStore.getState().unavailable?.kind).toBe("unsupported"));
    // Absent, not disabled (AD-27).
    expect(screen.queryByRole("switch", { name: WAKE_SWITCH_LABEL })).not.toBeInTheDocument();
  });

  it("draws the wake switch and phrase on a real availability, from the runtime answer alone", async () => {
    voiceAvailability.mockResolvedValue(null);
    render(<BotsSection open />);
    // The capability mirror carries no voice field; the section asked
    // `voice_availability` and the control drew itself from the answer.
    expect(await screen.findByRole("switch", { name: WAKE_SWITCH_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL })).not.toBeChecked();
    expect(screen.getByDisplayValue("hey nixie")).toBeInTheDocument();
    // Nothing was streamed: Settings reads the facts and opens no watcher.
    expect(voiceStore.getState().state).toBeNull();
  });

  it("reaches the language control through Settings on the phone tier (Epic 63)", async () => {
    voiceAvailability.mockResolvedValue(null);
    // The phone's mirror: it can talk to a model and cannot reach the drive.
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: true });
    render(<BotsSection open />);
    const control = await screen.findByRole("combobox", { name: VOICE_LOCALE_LABEL });
    expect(control).toHaveValue("");
    expect(screen.getByRole("option", { name: /en-US/ })).toBeInTheDocument();
  });

  it("does not draw the wake switch before voice_availability has answered", () => {
    voiceAvailability.mockReturnValue(new Promise<null>(() => {}));
    render(<BotsSection open />);
    expect(screen.queryByRole("switch", { name: WAKE_SWITCH_LABEL })).not.toBeInTheDocument();
  });

  it("lists an endpoint by HOST and discloses that it is private", async () => {
    render(<BotsSection open />);
    // The host, never the URL: the same shape the egress disclosure uses for a
    // git remote, so one destination is named one way.
    await waitFor(() =>
      expect(screen.getByText(/Ollama here — ollama at localhost \(private\)/)).toBeInTheDocument(),
    );
    expect(screen.queryByText(/11434/)).not.toBeInTheDocument();
  });

  it("says a row has no key stored, which is not a fault by itself", async () => {
    render(<BotsSection open />);
    await waitFor(() => expect(screen.getByText(BOTS_NO_TOKEN_CAPTION)).toBeInTheDocument());
    expect(screen.queryByText(BOTS_SECRET_MISSING_CAPTION)).not.toBeInTheDocument();
  });

  it("says so when the stored key has gone missing, which IS a fault", async () => {
    botsProvidersList.mockResolvedValue([
      { ...PROVIDER, health: "secretMissing", hasToken: false },
    ]);
    render(<BotsSection open />);
    await waitFor(() => expect(screen.getByText(BOTS_SECRET_MISSING_CAPTION)).toBeInTheDocument());
    // The two sentences are different facts and must not both render.
    expect(screen.queryByText(BOTS_NO_TOKEN_CAPTION)).not.toBeInTheDocument();
  });

  it("adds an endpoint through an inline disclosure, never a dialog (AD-C7)", async () => {
    render(<BotsSection open />);
    fireEvent.click(screen.getByRole("button", { name: BOTS_ADD_PROVIDER_LABEL }));
    // The disclosure is inline: a modal over the list would hide the rows whose
    // settings this one is being compared against.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText(BOTS_NAME_LABEL), {
      target: { value: "Work Hermes" },
    });
    fireEvent.change(screen.getByLabelText(BOTS_BASE_URL_LABEL), {
      target: { value: "http://hesperia.local:8642" },
    });
    fireEvent.click(screen.getByRole("button", { name: BOTS_SAVE_LABEL }));

    await waitFor(() => expect(botsProviderSave).toHaveBeenCalledTimes(1));
    expect(botsProviderSave).toHaveBeenCalledWith({
      id: null,
      kind: "ollama",
      name: "Work Hermes",
      // Sent exactly as typed: normalization is Rust's grammar, and a second
      // one here would be a second grammar that could disagree with it.
      baseUrl: "http://hesperia.local:8642",
      token: null,
      clearToken: false,
    });
  });

  it("edits the same row through the same component, with the id present", async () => {
    render(<BotsSection open />);
    await waitFor(() => expect(screen.getByText(/Ollama here/)).toBeInTheDocument());
    fireEvent.click(screen.getAllByRole("button", { name: BOTS_EDIT_LABEL })[0] as HTMLElement);
    // The edit mode is the add form with the row's values in it.
    expect(screen.getByLabelText(BOTS_NAME_LABEL)).toHaveValue("Ollama here");
    expect(screen.getByLabelText(BOTS_BASE_URL_LABEL)).toHaveValue("http://localhost:11434");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText(BOTS_NAME_LABEL), { target: { value: "Renamed" } });
    fireEvent.click(screen.getByRole("button", { name: BOTS_SAVE_LABEL }));
    await waitFor(() => expect(botsProviderSave).toHaveBeenCalledTimes(1));
    const req = botsProviderSave.mock.calls[0]?.[0] as BotProviderSaveReq;
    expect(req.id).toBe("prov-1");
    expect(req.name).toBe("Renamed");
  });

  it("a blank key field means unchanged, never cleared", async () => {
    render(<BotsSection open />);
    await waitFor(() => expect(screen.getByText(/Ollama here/)).toBeInTheDocument());
    fireEvent.click(screen.getAllByRole("button", { name: BOTS_EDIT_LABEL })[0] as HTMLElement);
    // Nothing is typed into the key field, and nothing renders in it either —
    // keeper cannot show a stored key.
    expect(screen.getByLabelText(BOTS_TOKEN_LABEL)).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: BOTS_SAVE_LABEL }));
    await waitFor(() => expect(botsProviderSave).toHaveBeenCalledTimes(1));
    const req = botsProviderSave.mock.calls[0]?.[0] as BotProviderSaveReq;
    expect(req.token).toBeNull();
    expect(req.clearToken).toBe(false);
  });

  it("renders Rust's own refusal sentence for a base URL it will not send to", async () => {
    botsProviderSave.mockRejectedValue({
      code: "internal",
      message: "a base URL must use http or https, got ftp",
      accountId: null,
      retriable: false,
    });
    render(<BotsSection open />);
    fireEvent.click(screen.getByRole("button", { name: BOTS_ADD_PROVIDER_LABEL }));
    fireEvent.change(screen.getByLabelText(BOTS_NAME_LABEL), { target: { value: "Bad" } });
    fireEvent.change(screen.getByLabelText(BOTS_BASE_URL_LABEL), {
      target: { value: "ftp://example.org" },
    });
    fireEvent.click(screen.getByRole("button", { name: BOTS_SAVE_LABEL }));
    // The frontend did not decide this; it rendered what Rust said, verbatim.
    await waitFor(() =>
      expect(screen.getByText("a base URL must use http or https, got ftp")).toBeInTheDocument(),
    );
  });

  it("prints the probe verdict in the app's own reachability vocabulary", async () => {
    render(<BotsSection open />);
    await waitFor(() => expect(screen.getByText(/Ollama here/)).toBeInTheDocument());
    fireEvent.click(screen.getAllByRole("button", { name: BOTS_TEST_LABEL })[0] as HTMLElement);
    await waitFor(() =>
      expect(screen.getByText("Reachable. It reports version 0.33.2.")).toBeInTheDocument(),
    );
  });

  it("confirms a removal by naming what happens to which object", async () => {
    render(<BotsSection open />);
    await waitFor(() => expect(screen.getByText(/Ollama here/)).toBeInTheDocument());
    fireEvent.click(screen.getAllByRole("button", { name: BOTS_REMOVE_LABEL })[0] as HTMLElement);
    // A destructive confirm IS a dialog — the one exception to AD-C7's
    // inline-disclosure rule.
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent(
      "Ollama here is removed, along with every bot on it and the key stored for it.",
    );
    expect(dialog).toHaveTextContent("Conversations you have already had are kept.");
  });
});

/**
 * The phone tier (Epic 62): `bots` true because the pane exists there, every
 * tier-telling flag false, hydrated. The same predicate that shows "On this
 * iPhone" in Settings → About.
 */
const PHONE_CAPABILITIES = { ...DEFAULT_CAPABILITIES, bots: true };

const HERMES: BotProviderVm = {
  ...PROVIDER,
  id: "prov-2",
  kind: "hermes",
  name: "Hermes on hesperia",
  baseUrl: "http://hesperia.local:8642",
  host: "hesperia.local",
  isPrivate: true,
  hasToken: true,
};

describe("BotsSection on the phone (Story 62.3, FR-399)", () => {
  beforeEach(() => {
    capabilitiesStore.getState().applySnapshot(PHONE_CAPABILITIES);
  });

  afterEach(() => {
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  });

  it("adds a Hermes endpoint with its base URL and key through the same form", async () => {
    botsProvidersList.mockResolvedValue([]);
    botsBotsList.mockResolvedValue([]);
    render(<BotsSection open />);
    fireEvent.click(screen.getByRole("button", { name: BOTS_ADD_PROVIDER_LABEL }));
    // The kind picker offers both stored spellings — Ollama is neither built
    // for nor blocked on a phone (the epic's DW-221) — and the pick is the
    // stored word.
    expect(screen.getByRole("button", { name: "ollama" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "hermes" }));
    expect(screen.getByRole("button", { name: "hermes" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.change(screen.getByLabelText(BOTS_NAME_LABEL), {
      target: { value: "Hermes on hesperia" },
    });
    fireEvent.change(screen.getByLabelText(BOTS_BASE_URL_LABEL), {
      target: { value: "http://hesperia.local:8642" },
    });
    fireEvent.change(screen.getByLabelText(BOTS_TOKEN_LABEL), {
      target: { value: "hermes-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: BOTS_SAVE_LABEL }));

    await waitFor(() => expect(botsProviderSave).toHaveBeenCalledTimes(1));
    expect(botsProviderSave).toHaveBeenCalledWith({
      id: null,
      kind: "hermes",
      name: "Hermes on hesperia",
      baseUrl: "http://hesperia.local:8642",
      token: "hermes-key",
      clearToken: false,
    } satisfies BotProviderSaveReq);
  });

  it("probes, edits and removes the Hermes row with the desktop's own controls", async () => {
    botsProvidersList.mockResolvedValue([HERMES]);
    botsBotsList.mockResolvedValue([]);
    botsProviderProbe.mockResolvedValue({
      reach: "offline",
      status: null,
      version: null,
      roundTripMs: 30,
      bot: null,
      presence: null,
      reason: null,
    } satisfies BotProbeVm);
    render(<BotsSection open />);
    await waitFor(() =>
      expect(
        screen.getByText(/Hermes on hesperia — hermes at hesperia\.local \(private\)/),
      ).toBeInTheDocument(),
    );
    // The probe answers in the app's reachability vocabulary, same word as the
    // desktop and the connection pill.
    fireEvent.click(screen.getByRole("button", { name: BOTS_TEST_LABEL }));
    await waitFor(() =>
      expect(screen.getByText("Offline — nothing answered.")).toBeInTheDocument(),
    );
    expect(botsProviderProbe).toHaveBeenCalledWith("prov-2");

    fireEvent.click(screen.getByRole("button", { name: BOTS_EDIT_LABEL }));
    expect(screen.getByLabelText(BOTS_BASE_URL_LABEL)).toHaveValue("http://hesperia.local:8642");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: BOTS_REMOVE_LABEL }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Hermes on hesperia is removed");
  });

  it("pins a Hermes bot by its profile, the same target field as a model tag", async () => {
    botsProvidersList.mockResolvedValue([HERMES]);
    botsBotsList.mockResolvedValue([]);
    render(<BotsSection open />);
    fireEvent.click(await screen.findByRole("button", { name: BOTS_ADD_BOT_LABEL }));
    fireEvent.change(screen.getByLabelText(BOTS_TARGET_LABEL), {
      target: { value: "research" },
    });
    fireEvent.change(screen.getByLabelText(BOTS_NAME_LABEL), { target: { value: "Research" } });
    fireEvent.click(screen.getByRole("button", { name: BOTS_SAVE_LABEL }));

    await waitFor(() => expect(botsBotSave).toHaveBeenCalledTimes(1));
    const req = botsBotSave.mock.calls[0]?.[0] as BotSaveReq;
    expect(req.providerId).toBe("prov-2");
    expect(req.target).toBe("research");
  });
});

describe("botProbeSentence", () => {
  const base: BotProbeVm = {
    reach: "online",
    status: 200,
    version: null,
    roundTripMs: 1,
    bot: null,
    presence: null,
    reason: null,
  };

  it("uses the app's word for a remote that did not answer", () => {
    // `offline` is what `keeper-sync` calls an unreachable folder and what the
    // connection pill calls a dead homeserver. A chat endpoint that does not
    // answer is the same event, so it gets the same word.
    expect(botProbeSentence({ ...base, reach: "offline", status: null })).toBe(
      "Offline — nothing answered.",
    );
  });

  it("renders Rust's own sentence whenever Rust wrote one", () => {
    expect(botProbeSentence({ ...base, reason: "the endpoint refused the key it was given" })).toBe(
      "the endpoint refused the key it was given",
    );
  });

  it("distinguishes a bot that is there, absent, and one keeper could not check", () => {
    expect(botProbeSentence({ ...base, bot: "research", presence: "exists" })).toContain(
      "The bot research is there.",
    );
    expect(botProbeSentence({ ...base, bot: "research", presence: "absent" })).toContain(
      "This endpoint has no bot called research.",
    );
    // The honest third arm: "keeper could not ask" is a different sentence from
    // "it is not there" for somebody about to retype a name that was right.
    expect(botProbeSentence({ ...base, bot: "research", presence: "unknown" })).toContain(
      "keeper could not tell whether research is there.",
    );
  });
});

describe("removalSentence", () => {
  it("names the second-order effect the row cannot show", () => {
    expect(removalSentence({ kind: "provider", name: "Work" })).toContain("every bot on it");
    expect(removalSentence({ kind: "bot", name: "Llama" })).toContain(
      "Conversations you have already had with it are kept.",
    );
  });
});
