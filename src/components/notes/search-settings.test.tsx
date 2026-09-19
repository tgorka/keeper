import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { notesSearchStateStore } from "@/lib/stores/notes-search-state";
import { EMBEDDING_UNKNOWN, SearchSettingsSection } from "./search-settings";

const mocks = vi.hoisted(() => ({
  names: ["index.md", "agents.md"],
  setNames: vi.fn(),
  setModel: vi.fn(),
  failProvider: false,
}));
vi.mock("@/components/settings/config-source-section", () => ({ FileControlled: () => null }));
vi.mock("@/lib/ipc/client", () => ({
  notesServiceFileNamesGet: async () => mocks.names,
  notesServiceFileNamesSet: async (names: string[]) => {
    mocks.setNames(names);
    mocks.names = names.length ? ["custom.md", "log.md"] : [];
  },
  notesEmbeddingModelGet: async () => ({ provider: "p", model: "unknown" }),
  notesEmbeddingModelSet: async (model: unknown) => {
    mocks.setModel(model);
  },
  botsProvidersList: async () => [
    { id: "p", name: "Local" },
    { id: "offline", name: "Offline" },
  ],
  botsModelsList: async (provider: string) => {
    if (mocks.failProvider && provider === "p") throw new Error("offline");
    if (provider === "offline") throw new Error("offline");
    return [
      { id: "yes", embedding: true },
      { id: "no", embedding: false },
      { id: "unknown", embedding: null },
    ];
  },
}));
beforeEach(() => {
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, trayIcon: true });
  mocks.failProvider = false;
  mocks.names = ["index.md", "agents.md"];
  vi.clearAllMocks();
  notesSearchStateStore.setState({ byVault: {} });
});
it("round-trips comma-separated names, dropping empty and padded segments", async () => {
  render(<SearchSettingsSection open />);
  const input = await screen.findByDisplayValue("index.md, agents.md");
  fireEvent.change(input, { target: { value: " log.md , , custom.md, " } });
  fireEvent.blur(input);
  await waitFor(() => expect(input).toHaveValue("custom.md, log.md"));
  expect(mocks.setNames).toHaveBeenCalledWith(["log.md", "custom.md"]);
  fireEvent.change(input, { target: { value: "  " } });
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(mocks.setNames).toHaveBeenLastCalledWith([]));
});
it("offers unknown with a warning, hides refused capabilities, and clears with None", async () => {
  render(<SearchSettingsSection open />);
  expect(await screen.findByText(EMBEDDING_UNKNOWN)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("combobox", { name: "Embedding model" }));
  expect(await screen.findByRole("option", { name: "Local · yes" })).toBeInTheDocument();
  expect(screen.queryByRole("option", { name: "Local · no" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("option", { name: "None — search by words only" }));
  await waitFor(() => expect(mocks.setModel).toHaveBeenCalledWith(null));
});
it("shows provider refusal sentences", () => {
  notesSearchStateStore.getState().apply({
    vaultId: "v",
    phase: "refused",
    indexed: 1,
    total: 1,
    embedded: 0,
    embeddable: 1,
    model: "unknown",
    sentence: "This provider cannot embed notes.",
  });
  render(<SearchSettingsSection open />);
  expect(screen.getByText("This provider cannot embed notes.")).toBeInTheDocument();
});
it("keeps reachable providers and names each failed provider", async () => {
  render(<SearchSettingsSection open />);
  expect(
    await screen.findByText(/Embedding models from Offline could not be read/),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("combobox", { name: "Embedding model" }));
  expect(await screen.findByRole("option", { name: "Local · yes" })).toBeInTheDocument();
});
it("names the configured model even when its provider is unreachable", async () => {
  mocks.failProvider = true;
  render(<SearchSettingsSection open />);
  expect(
    await screen.findByText("Configured embedding model p · unknown is unavailable."),
  ).toBeInTheDocument();
});
it("does not offer the embedding picker on the phone capability tier", async () => {
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, notes: true, bots: true });
  render(<SearchSettingsSection open />);
  expect(await screen.findByLabelText("Service files")).toBeInTheDocument();
  expect(screen.queryByRole("combobox", { name: "Embedding model" })).not.toBeInTheDocument();
});
