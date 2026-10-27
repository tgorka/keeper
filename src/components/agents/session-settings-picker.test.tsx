import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionSettingsPicker } from "@/components/agents/session-settings-picker";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentSessionPickerVm } from "@/lib/ipc/client";
import { AGENT_PICKS_COOKIE, lastAgentPick } from "@/lib/stores/agent-picks";
import { persistFold } from "@/lib/stores/fold-cookie";
import {
  MODEL_FAVORITES_COOKIE,
  modelFavoritesCookie,
  resetModelFavoritesForTest,
} from "@/lib/stores/model-favorites";

const mock = vi.hoisted(() => ({ change: vi.fn() }));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentSessionSettingsChange: mock.change,
}));

const target = { database: "alpha", sessionId: "s.1" };

/** Rust's answer for an agent offering three models and two efforts, nothing saved. */
function picker(patch: Partial<AgentSessionPickerVm> = {}): AgentSessionPickerVm {
  return {
    models: [
      { id: "alpha-large", provider: "Alpha", current: false, default: false },
      { id: "beta-small", provider: "Beta", current: true, default: true },
      { id: "gamma-fast", provider: "Alpha", current: false, default: false },
    ],
    efforts: [
      { effort: "medium", label: "Medium", current: true, default: true },
      { effort: "high", label: "High", current: false, default: false },
    ],
    modelLabel: "beta-small",
    effortLabel: "Medium",
    resettable: false,
    write: { state: "confirmed" },
    ...patch,
  };
}

/** The bar's chips as `settings::footer` reads them from this picker. */
function Picker(
  props: Omit<ComponentProps<typeof SessionSettingsPicker>, "footer" | "picker"> & {
    picker: AgentSessionPickerVm;
  },
) {
  const { picker: p } = props;
  return (
    <SessionSettingsPicker
      {...props}
      footer={{
        model: p.modelLabel,
        effort: p.effortLabel ?? "Default",
        choosesModel: p.models.length > 1,
        choosesEffort: p.efforts.length > 1,
      }}
    />
  );
}

function openModels() {
  fireEvent.click(screen.getByRole("button", { name: /^Model: / }));
  return screen.getByRole("dialog");
}

beforeEach(() => {
  vi.clearAllMocks();
  mock.change.mockResolvedValue({ note: null, pick: { model: "gamma-fast", effort: null } });
  // Clear the favourites cookie and the store that read it.
  persistFold(`${MODEL_FAVORITES_COOKIE}=; path=/; max-age=0`);
  persistFold(`${AGENT_PICKS_COOKIE}=; path=/; max-age=0`);
  resetModelFavoritesForTest();
});

describe("the model and effort pickers", () => {
  it("has no effort menu at all when the agent offers no effort (Nixi)", () => {
    render(
      <Picker
        target={target}
        agent="nixi"
        picker={picker({ efforts: [], effortLabel: undefined })}
      />,
    );
    expect(screen.getByRole("button", { name: "Model: beta-small" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Effort/ })).toBeNull();
    // Still named, as a chip that opens nothing (owner ruling 2026-10-27).
    expect(screen.getByText("Default")).toBeInTheDocument();
  });

  it("lists only offered models, starred ones first, and never a starred model not offered", () => {
    persistFold(modelFavoritesCookie(["gamma-fast", "foreign-model"]));
    render(<Picker target={target} agent="nixi" picker={picker()} />);
    const dialog = openModels();
    const options = within(dialog).getAllByRole("option");
    // Starred first, then Rust's provider groups; the model in use and the agent's default say
    // so in the option's name.
    expect(options.map((option) => option.getAttribute("aria-label"))).toEqual([
      "gamma-fast",
      "alpha-large",
      "beta-small, in use, default",
    ]);
    expect(within(dialog).getByRole("group", { name: "Alpha" })).toHaveTextContent("alpha-large");
    expect(within(dialog).getByRole("group", { name: "Beta" })).toHaveTextContent("beta-small");
    expect(within(dialog).queryByText("foreign-model")).toBeNull();
    expect(within(options[2]).getByText("Default")).toBeInTheDocument();
  });

  it("searches and picks with the keyboard, writing the model from the next answer", async () => {
    render(<Picker target={target} agent="nixi" picker={picker()} />);
    const dialog = openModels();
    const search = within(dialog).getByRole("combobox", { name: "Search models" });
    fireEvent.change(search, { target: { value: "gamma" } });
    expect(within(dialog).getAllByRole("option")).toHaveLength(1);
    fireEvent.keyDown(search, { key: "Enter" });
    await waitFor(() =>
      expect(mock.change).toHaveBeenCalledWith(target, { kind: "model", model: "gamma-fast" }),
    );
  });

  it("starring a model keeps the popover open and writes nothing", () => {
    render(<Picker target={target} agent="nixi" picker={picker()} />);
    const dialog = openModels();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Star alpha-large as a favourite" }),
    );
    expect(mock.change).not.toHaveBeenCalled();
    expect(within(dialog).getByRole("group", { name: "Favourites" })).toBeInTheDocument();
    expect(document.cookie).toContain(MODEL_FAVORITES_COOKIE);
  });

  it("offers only the agent's efforts with the default's badge, and writes the one picked", async () => {
    render(<Picker target={target} agent="nixi" picker={picker()} />);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Effort: Medium" }), {
      button: 0,
      ctrlKey: false,
    });
    const menu = await screen.findByRole("menu");
    const items = within(menu).getAllByRole("menuitemradio");
    expect(items.map((item) => item.textContent)).toEqual(["MediumDefault", "High"]);
    fireEvent.click(items[1]);
    await waitFor(() =>
      expect(mock.change).toHaveBeenCalledWith(target, { kind: "effort", effort: "high" }),
    );
  });

  it("resets through Rust's reset, shown only when a choice is saved", async () => {
    const { rerender } = render(<Picker target={target} agent="nixi" picker={picker()} />);
    expect(screen.queryByRole("button", { name: /Reset/ })).toBeNull();
    rerender(<Picker target={target} agent="nixi" picker={picker({ resettable: true })} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Reset model and effort to the agent's defaults" }),
    );
    await waitFor(() => expect(mock.change).toHaveBeenCalledWith(target, { kind: "reset" }));
  });

  it("says a change made mid-turn applies from the next answer", async () => {
    const running =
      "This answer keeps its model and effort; your change applies from the next answer.";
    mock.change.mockResolvedValue({ note: running, pick: { model: "alpha-large", effort: null } });
    render(<Picker target={target} agent="nixi" picker={picker({ running })} />);
    fireEvent.click(screen.getByRole("button", { name: /^Model: / }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText(running)).toBeInTheDocument();
    fireEvent.click(within(dialog).getAllByRole("option")[0]);
    expect(await screen.findByRole("status")).toHaveTextContent("from the next answer");
  });

  it("shows the newest answer's fallback by the picker", () => {
    const fallback = "gone-model is no longer offered; using beta-small instead.";
    render(<Picker target={target} agent="nixi" picker={picker({ fallback })} />);
    expect(screen.getByRole("status")).toHaveTextContent(fallback);
  });

  it("puts a later server refusal in plain words on the control", () => {
    const sentence =
      "The server refused your model and effort change: the agent does not offer that model.";
    render(
      <Picker
        target={target}
        agent="nixi"
        picker={picker({ write: { state: "refused", sentence } })}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(sentence);
  });

  it("puts a local refusal in plain words on the control", async () => {
    mock.change.mockRejectedValue({
      code: "internal",
      message: "gamma-fast is not offered here; choose alpha-large or beta-small.",
      accountId: null,
    });
    render(<Picker target={target} agent="nixi" picker={picker()} />);
    fireEvent.click(within(openModels()).getAllByRole("option")[0]);
    expect(await screen.findByRole("alert")).toHaveTextContent("is not offered here");
  });

  it("Enter or Space on a star stars it and never chooses a model (R10918-01)", async () => {
    render(<Picker target={target} agent="nixi" picker={picker()} />);
    const dialog = openModels();
    fireEvent.keyDown(
      within(dialog).getByRole("button", { name: "Star alpha-large as a favourite" }),
      {
        key: "Enter",
      },
    );
    expect(mock.change).not.toHaveBeenCalled();
    expect(within(dialog).getByRole("button", { name: "Unstar alpha-large" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    // The row moved into Favourites; the keyboard's focus moved with it.
    await waitFor(() =>
      expect(document.activeElement).toBe(
        within(dialog).getByRole("button", { name: "Unstar alpha-large" }),
      ),
    );
    fireEvent.keyDown(
      within(dialog).getByRole("button", { name: "Star gamma-fast as a favourite" }),
      {
        key: " ",
      },
    );
    expect(mock.change).not.toHaveBeenCalled();
    expect(within(dialog).getByRole("button", { name: "Unstar gamma-fast" })).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toBe(dialog);
  });

  it("names model and effort as read-only chips where there is nothing to choose", () => {
    render(
      <Picker
        target={target}
        agent="nixi"
        picker={picker({
          models: [{ id: "beta-small", provider: "Beta", current: true, default: true }],
          efforts: [],
          effortLabel: undefined,
          resettable: true,
        })}
      />,
    );
    const bar = screen.getByRole("group", { name: "Model and effort" });
    expect(within(bar).getByText("beta-small")).toBeInTheDocument();
    expect(within(bar).getByText("Default")).toBeInTheDocument();
    // Only Reset, for the saved choice: neither chip opens anything.
    expect(
      within(bar)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Reset"]);
  });

  it("says which model failed and the way back, above everything else", () => {
    const failed =
      "The last answer failed on gamma-fast: provider answered 429. Choose another model, or Reset to the agent's default.";
    render(<Picker target={target} agent="nixi" picker={picker({ failed, resettable: true })} />);
    expect(screen.getByRole("alert")).toHaveTextContent(failed);
    expect(screen.getByRole("button", { name: "Model: beta-small" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Reset model and effort to the agent's defaults" }),
    ).toBeInTheDocument();
  });

  it("keeps what a change wrote as this device's last pick for the agent, and forgets it on reset", async () => {
    mock.change.mockResolvedValue({ note: null, pick: { model: "gamma-fast", effort: "high" } });
    render(<Picker target={target} agent="nixi" picker={picker({ resettable: true })} />);
    fireEvent.click(within(openModels()).getAllByRole("option")[0]);
    await waitFor(() =>
      expect(lastAgentPick("nixi")).toEqual({ model: "gamma-fast", effort: "high" }),
    );
    expect(lastAgentPick("other-agent")).toBeNull();
    mock.change.mockResolvedValue({ note: null, pick: { model: null, effort: null } });
    fireEvent.click(
      screen.getByRole("button", { name: "Reset model and effort to the agent's defaults" }),
    );
    await waitFor(() => expect(lastAgentPick("nixi")).toBeNull());
  });
});
