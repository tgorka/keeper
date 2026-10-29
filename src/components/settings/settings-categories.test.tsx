import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  REVEAL_SETTLE_LIMIT_MS,
  REVEAL_SETTLE_QUIET_MS,
  SETTINGS_BACK_LABEL,
  SETTINGS_CATEGORIES_LABEL,
  SettingsCategories,
  type SettingsLayout,
  type SettingsSectionSlot,
} from "@/components/settings/settings-categories";
import {
  SETTINGS_CATEGORIES,
  SETTINGS_SECTION_IDS,
  type SettingsSectionId,
} from "@/lib/settings-categories";
import { DEVICE_FILE_TITLE, DEVICE_ONLY_TITLE } from "@/lib/settings-device-notes";
import { settingsUiStore } from "@/lib/stores/settings-ui";

/** Every section present, each drawing one line that names it. */
function everySection(): Record<SettingsSectionId, SettingsSectionSlot | null> {
  // Built from the id list, so a cast is the only way to name its keys.
  return Object.fromEntries(
    SETTINGS_SECTION_IDS.map((id) => [id, { node: <p>{`section:${id}`}</p> }]),
  ) as Record<SettingsSectionId, SettingsSectionSlot | null>;
}

function rows() {
  const nav = screen.getByRole("navigation", { name: SETTINGS_CATEGORIES_LABEL });
  return within(nav).getAllByRole("button");
}

/** The sections a person can see right now. */
function visibleSections(): string[] {
  return screen
    .queryAllByText(/^section:/)
    .filter((element) => element.closest("[hidden]") === null)
    .map((element) => (element.textContent ?? "").replace("section:", ""));
}

function choose(layout: SettingsLayout, title: string) {
  if (layout === "drill") {
    const back = screen.queryByRole("button", { name: SETTINGS_BACK_LABEL });
    if (back) fireEvent.click(back);
  }
  const nav = screen.getByRole("navigation", { name: SETTINGS_CATEGORIES_LABEL });
  fireEvent.click(within(nav).getByRole("button", { name: title }));
}

afterEach(() => {
  settingsUiStore.setState({ reveal: null });
});

describe.each<SettingsLayout>(["rail", "drill"])("SettingsCategories (%s)", (layout) => {
  it("shows every section in exactly one category: nothing lost, nothing doubled", () => {
    render(<SettingsCategories layout={layout} sections={everySection()} />);
    const seen: string[] = [];
    for (const category of SETTINGS_CATEGORIES) {
      choose(layout, category.title);
      expect(screen.getByRole("heading", { name: category.title })).toBeVisible();
      const shown = visibleSections();
      // A category is never listed as a link to an empty page.
      expect(shown.length).toBeGreaterThan(0);
      seen.push(...shown);
    }
    expect([...seen].sort()).toEqual([...SETTINGS_SECTION_IDS].sort());
  });

  it("lists no category whose every section this build lacks", () => {
    const sections = everySection();
    sections.shortcuts = null;
    sections.capture = null;
    sections["notes-search"] = null;
    sections["model-providers"] = null;
    render(<SettingsCategories layout={layout} sections={sections} />);
    const titles = rows().map((row) => row.textContent);
    expect(titles).not.toContain("Shortcuts");
    expect(titles).not.toContain("Notes");
    expect(titles).toContain("Account & device");
  });

  it("lists a category only once one of its sections draws something", async () => {
    let draw: (on: boolean) => void = () => {};
    function Late() {
      const [on, setOn] = useState(false);
      draw = setOn;
      return on ? <p>section:capture</p> : null;
    }
    const sections = everySection();
    sections.capture = { node: <Late /> };
    sections["notes-search"] = { node: null };
    sections["model-providers"] = { node: null };
    render(<SettingsCategories layout={layout} sections={sections} />);
    expect(rows().map((row) => row.textContent)).not.toContain("Notes");

    await act(async () => draw(true));
    choose(layout, "Notes");
    expect(visibleSections()).toEqual(["capture"]);
  });
});

describe("SettingsCategories rail", () => {
  it("opens on the first category and swaps the sections when another is chosen", () => {
    render(<SettingsCategories layout="rail" sections={everySection()} />);
    expect(visibleSections()).toEqual(["account", "device-name", "device", "encryption", "setup"]);
    expect(screen.getByRole("button", { name: "Account & device" })).toHaveAttribute(
      "aria-current",
      "page",
    );

    fireEvent.click(screen.getByRole("button", { name: "Sync & drives" }));
    expect(visibleSections()).toEqual(["sync", "tasks", "sync-git"]);
    expect(screen.getByRole("button", { name: "Sync & drives" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByRole("button", { name: "Account & device" })).not.toHaveAttribute(
      "aria-current",
    );
  });
});

describe("SettingsCategories drill", () => {
  it("is the list first, a category with a way back, and back returns focus to its row", () => {
    render(<SettingsCategories layout="drill" sections={everySection()} />);
    expect(visibleSections()).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: "Notifications & dock" }));
    expect(visibleSections()).toEqual(["notifications", "background"]);
    expect(screen.queryByRole("navigation", { name: SETTINGS_CATEGORIES_LABEL })).toBeNull();
    const back = screen.getByRole("button", { name: SETTINGS_BACK_LABEL });
    expect(back).toHaveFocus();

    fireEvent.click(back);
    expect(visibleSections()).toEqual([]);
    expect(screen.getByRole("button", { name: "Notifications & dock" })).toHaveFocus();
  });
});

describe("SettingsCategories deep links", () => {
  it("lands a named section's category at once, and is read only once", () => {
    settingsUiStore.getState().revealSection("tasks");
    render(<SettingsCategories layout="drill" sections={everySection()} />);
    expect(visibleSections()).toEqual(["sync", "tasks", "sync-git"]);
    expect(settingsUiStore.getState().reveal).toBeNull();
  });

  it("moves an open Settings to the named section, and focuses its field once it renders", async () => {
    let draw: (on: boolean) => void = () => {};
    function Search() {
      const [on, setOn] = useState(false);
      draw = setOn;
      return (
        <div>
          <p>section:notes-search</p>
          {on && <input id="notes-embedding-model" aria-label="Model" />}
        </div>
      );
    }
    const sections = everySection();
    sections["notes-search"] = { node: <Search /> };
    render(<SettingsCategories layout="rail" sections={sections} />);
    expect(visibleSections()).toContain("account");

    act(() => settingsUiStore.getState().revealSection("notes-search", "notes-embedding-model"));
    expect(visibleSections()).toEqual(["capture", "notes-search", "model-providers"]);
    await act(async () => draw(true));
    expect(screen.getByRole("textbox", { name: "Model" })).toHaveFocus();
  });

  it("keeps a reveal waiting while its field is disabled, and focuses it once it is enabled", async () => {
    // The real case: Notes search's model field is there at once but disabled
    // until the section's read from Rust answers. Focusing a disabled field
    // does nothing, so the reveal must not count that as landing.
    let hydrate: () => void = () => {};
    function Search() {
      const [loaded, setLoaded] = useState(false);
      hydrate = () => setLoaded(true);
      return (
        <div>
          <p>section:notes-search</p>
          <button type="button" id="notes-embedding-model" disabled={!loaded}>
            Model
          </button>
        </div>
      );
    }
    const sections = everySection();
    sections["notes-search"] = { node: <Search /> };
    settingsUiStore.getState().revealSection("notes-search", "notes-embedding-model");
    render(<SettingsCategories layout="drill" sections={sections} />);
    expect(visibleSections()).toEqual(["capture", "notes-search", "model-providers"]);
    expect(screen.getByRole("button", { name: "Model" })).not.toHaveFocus();

    await act(async () => hydrate());
    expect(screen.getByRole("button", { name: "Model" })).toHaveFocus();
  });

  it("drops a waiting reveal when the person chooses another category", async () => {
    let hydrate: () => void = () => {};
    function Search() {
      const [loaded, setLoaded] = useState(false);
      hydrate = () => setLoaded(true);
      return (
        <div>
          <p>section:notes-search</p>
          <button type="button" id="notes-embedding-model" disabled={!loaded}>
            Model
          </button>
        </div>
      );
    }
    const sections = everySection();
    sections["notes-search"] = { node: <Search /> };
    settingsUiStore.getState().revealSection("notes-search", "notes-embedding-model");
    render(<SettingsCategories layout="rail" sections={sections} />);
    fireEvent.click(screen.getByRole("button", { name: "Sync & drives" }));
    await act(async () => hydrate());

    // Coming back to Notes is a person's choice, not the old request.
    fireEvent.click(screen.getByRole("button", { name: "Notes" }));
    expect(screen.getByRole("button", { name: "Model" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Model" })).not.toHaveFocus();
  });
});

describe("SettingsCategories deep links whose sections above fill in late", () => {
  // jsdom lays nothing out, so the test plays the browser: a ResizeObserver
  // it fires by hand for an element that grew, and a scrollIntoView that
  // records which element was scrolled to.
  let observers: { callback: ResizeObserverCallback; targets: Set<Element> }[];
  let scrolled: Element[];
  let grow: (on: boolean) => void;

  /** Default agent, the first section of Agents & voice, drawn after its read answers. */
  function DefaultAgent() {
    const [on, setOn] = useState(false);
    grow = setOn;
    return <p>{on ? "section:default-agent, now three cards tall" : "section:default-agent"}</p>;
  }

  /** Default agent grows, and the browser tells whoever watches it. */
  async function growAbove() {
    await act(async () => grow(true));
    const above = document.querySelector('[data-settings-section="default-agent"]');
    act(() => {
      for (const { callback, targets } of observers) {
        if (above && targets.has(above)) {
          callback([], {} as ResizeObserver);
        }
      }
    });
  }

  const scrolledToAlwaysAllowed = () =>
    scrolled.filter((element) => element.getAttribute("data-settings-section") === "always-allowed")
      .length;

  function renderAlwaysAllowed() {
    const sections = everySection();
    sections["default-agent"] = { node: <DefaultAgent /> };
    settingsUiStore.getState().revealSection("always-allowed");
    render(<SettingsCategories layout="rail" sections={sections} />);
    expect(visibleSections()).toContain("always-allowed");
    expect(scrolledToAlwaysAllowed()).toBe(1);
  }

  beforeEach(() => {
    observers = [];
    scrolled = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        private readonly entry: (typeof observers)[number];
        constructor(callback: ResizeObserverCallback) {
          this.entry = { callback, targets: new Set() };
          observers.push(this.entry);
        }
        observe(target: Element) {
          this.entry.targets.add(target);
        }
        unobserve(target: Element) {
          this.entry.targets.delete(target);
        }
        disconnect() {
          this.entry.targets.clear();
        }
      },
    );
    vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(function (this: Element) {
      scrolled.push(this);
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("scrolls back to the revealed section when a section above it grows after the first scroll", async () => {
    renderAlwaysAllowed();
    await growAbove();
    expect(scrolledToAlwaysAllowed()).toBe(2);
  });

  it.each([
    ["scrolls", () => fireEvent.wheel(document.body)],
    ["clicks", () => fireEvent.pointerDown(document.body)],
    ["types", () => fireEvent.keyDown(document.body, { key: "ArrowDown" })],
  ])("lets go the moment the person %s", async (_, person) => {
    renderAlwaysAllowed();
    person();
    await growAbove();
    expect(scrolledToAlwaysAllowed()).toBe(1);
  });

  it("lets go once the layout has been still for the quiet window", async () => {
    vi.useFakeTimers();
    renderAlwaysAllowed();
    act(() => vi.advanceTimersByTime(REVEAL_SETTLE_QUIET_MS - 1));
    await growAbove();
    expect(scrolledToAlwaysAllowed()).toBe(2);

    // A growth restarts the quiet window; a full one without any ends it.
    act(() => vi.advanceTimersByTime(REVEAL_SETTLE_QUIET_MS));
    await growAbove();
    expect(scrolledToAlwaysAllowed()).toBe(2);
  });

  it("lets go at the limit even while the sections above keep moving", async () => {
    vi.useFakeTimers();
    renderAlwaysAllowed();
    let held = 1;
    for (let at = 500; at < REVEAL_SETTLE_LIMIT_MS; at += 500) {
      act(() => vi.advanceTimersByTime(500));
      await growAbove();
      held += 1;
      expect(scrolledToAlwaysAllowed()).toBe(held);
    }
    act(() => vi.advanceTimersByTime(500));
    await growAbove();
    expect(scrolledToAlwaysAllowed()).toBe(held);
  });
});

describe("SettingsCategories this-device notes", () => {
  it("says under the category what is backed up for this device and what never leaves it", () => {
    const sections = everySection();
    sections.storage = {
      node: <p>section:storage</p>,
      staysHere: [
        { text: "posture is this device's", keys: ["sdk_encryption"] },
        { text: "passphrase is in the keychain", kept: "device" },
      ],
    };
    sections.privacy = {
      node: <p>section:privacy</p>,
      staysHere: [{ text: "name rides every commit", kept: "account" }],
    };
    sections.diagnostics = { node: null, staysHere: [{ text: "drew nothing", kept: "device" }] };
    render(<SettingsCategories layout="rail" sections={sections} />);
    fireEvent.click(screen.getByRole("button", { name: "Privacy & diagnostics" }));

    const panel = screen.getByRole("region", { name: "Privacy & diagnostics" });
    const group = (title: string) =>
      Array.from(
        within(panel).getByText(title).parentElement?.querySelectorAll("li") ?? [],
        (item) => item.textContent,
      );
    expect(group(DEVICE_FILE_TITLE)).toEqual([
      "name rides every commit",
      "posture is this device's",
    ]);
    expect(group(DEVICE_ONLY_TITLE)).toEqual(["passphrase is in the keychain"]);
    // A section that drew nothing has nothing to say.
    expect(within(panel).queryByText("drew nothing")).toBeNull();
  });

  it("says nothing under a category with nothing of this device's own", () => {
    const sections = everySection();
    sections.sync = {
      node: <p>section:sync</p>,
      staysHere: [{ text: "drive folder", keys: ["sync.drive_folder"] }],
    };
    render(<SettingsCategories layout="rail" sections={sections} />);
    fireEvent.click(screen.getByRole("button", { name: "Shortcuts" }));
    const panel = screen.getByRole("region", { name: "Shortcuts" });
    expect(within(panel).queryByText(DEVICE_FILE_TITLE)).toBeNull();
    expect(within(panel).queryByText(DEVICE_ONLY_TITLE)).toBeNull();
  });
});
