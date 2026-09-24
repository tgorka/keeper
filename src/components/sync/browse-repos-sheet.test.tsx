import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ForgeReposVm, ForgeRepoVm, ForgeSourceVm } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc/client")>()),
  accountSignIn: vi.fn(),
  forgesList: vi.fn(),
  forgeRepos: vi.fn(),
  forgeConnectStart: vi.fn(),
  forgeConnectWait: vi.fn(),
  forgeConnectOpen: vi.fn(),
  forgeConnectCancel: vi.fn(),
  forgeDisconnect: vi.fn(),
  forgeReposAdd: vi.fn(),
  forgeDefaultBaseFolder: vi.fn(),
  syncProfiles: vi.fn(() => Promise.resolve([])),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import {
  BROWSE_ADD_ONE_LABEL,
  BROWSE_BASE_LABEL,
  BROWSE_BASE_MISSING,
  BROWSE_CODE_COPIED,
  BROWSE_COPY_CODE_LABEL,
  BROWSE_COPY_FAILED,
  BROWSE_DISCONNECT_LABEL,
  BROWSE_DOWNLOAD_ONLY,
  BROWSE_FORKS_LABEL,
  BROWSE_REFRESH_LABEL,
  BROWSE_REPOS_LABEL,
  BROWSE_SEARCH_LABEL,
  BROWSE_SIGN_IN_LABEL,
  BROWSE_SYNCING_HERE,
  BROWSE_TRY_AGAIN_LABEL,
  BrowseReposEntry,
  browseAddDrives,
  browseAddedSentence,
  browseConnectLabel,
  browseDriveNameLabel,
  browseElsewhere,
  browseOpenLabel,
  browseSelected,
  browseWaitingSentence,
} from "@/components/sync/browse-repos-sheet";
import { batchNameTakenSentence } from "@/lib/forge-repos";
import {
  accountSignIn,
  forgeConnectCancel,
  forgeConnectOpen,
  forgeConnectStart,
  forgeConnectWait,
  forgeDefaultBaseFolder,
  forgeRepos,
  forgeReposAdd,
  forgesList,
} from "@/lib/ipc/client";
import { accountStore, NO_ACCOUNT } from "@/lib/stores/account";
import { FORGE_SOURCE_COOKIE, resetForgesStoreForTest } from "@/lib/stores/forges";
import { resetSyncStoreForTest } from "@/lib/stores/sync";
import { accountVm } from "@/test/account-fixture";

const READ_ONLY = "You can only read this repository, so keeper only downloads it.";

function source(patch: Partial<ForgeSourceVm> = {}): ForgeSourceVm {
  return {
    id: "github",
    kind: "github",
    name: "GitHub",
    host: "github.com",
    via: "broker",
    state: "connected",
    login: "tgorka",
    sentence: null,
    credential: "forge:github",
    canConnect: false,
    appsUrl: null,
    ...patch,
  };
}

/** The account's own Forgejo, as a second tab. */
function forgejo(patch: Partial<ForgeSourceVm> = {}): ForgeSourceVm {
  return source({
    id: "account-forge",
    kind: "forgejo",
    name: "Acme",
    host: "git.acme.dev",
    via: "accountForge",
    credential: "account",
    ...patch,
  });
}

function repo(fullName: string, patch: Partial<ForgeRepoVm> = {}): ForgeRepoVm {
  const [owner, name] = fullName.split("/");
  return {
    fullName,
    owner,
    name,
    description: null,
    private: false,
    fork: false,
    archived: false,
    template: false,
    mirror: false,
    defaultBranch: "main",
    cloneUrl: `https://github.com/${fullName}.git`,
    webUrl: `https://github.com/${fullName}`,
    updatedMs: null,
    sizeKb: null,
    canPush: true,
    addedAs: [],
    elsewhere: [],
    pullOnly: false,
    pullOnlySentence: null,
    ...patch,
  };
}

function listing(repos: ForgeRepoVm[], sourceId = "github"): ForgeReposVm {
  const logins = [...new Set(repos.map((each) => each.owner))];
  return {
    sourceId,
    repos,
    owners: logins.map((login) => ({
      login,
      isYou: login === "tgorka",
      count: repos.filter((each) => each.owner === login).length,
    })),
    notices: [],
    truncated: false,
    fetchedMs: null,
  };
}

/** Render the entry (with whatever `forgesList` answers), open the sheet, and return the add callback. */
async function openEntry() {
  const onAddOne = vi.fn();
  const view = render(<BrowseReposEntry onAddOne={onAddOne} />);
  fireEvent.click(await screen.findByRole("button", { name: BROWSE_REPOS_LABEL }));
  return { onAddOne, unmount: view.unmount };
}

async function openSheet(sources: ForgeSourceVm[]) {
  vi.mocked(forgesList).mockResolvedValue(sources);
  return (await openEntry()).onAddOne;
}

const row = (fullName: string) =>
  screen
    .getAllByTestId("forge-repo-row")
    .find((candidate) => within(candidate).queryByRole("checkbox", { name: fullName }) !== null);

beforeEach(() => {
  resetForgesStoreForTest();
  resetSyncStoreForTest();
  accountStore.getState().setVm(NO_ACCOUNT);
  // Where Rust puts new drives; a case that is about the folder says which.
  vi.mocked(forgeDefaultBaseFolder).mockResolvedValue(null);
});

afterEach(() => {
  Reflect.deleteProperty(navigator, "clipboard");
  // The sheet remembers the last source in a cookie; one test's tab must not
  // open the next test's sheet.
  // biome-ignore lint/suspicious/noDocumentCookie: clearing the source this suite remembered
  document.cookie = `${FORGE_SOURCE_COOKIE}=; path=/; max-age=0`;
  vi.clearAllMocks();
});

describe("BrowseReposEntry", () => {
  it("is absent while Rust lists no repository source", async () => {
    vi.mocked(forgesList).mockResolvedValue([]);
    render(<BrowseReposEntry onAddOne={vi.fn()} />);
    await waitFor(() => expect(forgesList).toHaveBeenCalled());
    await act(async () => {});
    expect(screen.queryByRole("button", { name: BROWSE_REPOS_LABEL })).not.toBeInTheDocument();
  });

  it("connects by device code: shows the code, copies it, opens the page, and cancels", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    vi.mocked(forgeConnectStart).mockResolvedValue({
      userCode: "WDJB-MJHT",
      verificationUri: "https://github.com/login/device",
      expiresIn: 900,
    });
    vi.mocked(forgeConnectWait).mockReturnValue(new Promise(() => {}));
    vi.mocked(forgeConnectOpen).mockResolvedValue(undefined);
    vi.mocked(forgeConnectCancel).mockResolvedValue(undefined);
    await openSheet([
      source({ via: "deviceFlow", state: "notConnected", login: null, canConnect: true }),
    ]);

    fireEvent.click(screen.getByRole("button", { name: browseConnectLabel("GitHub") }));
    expect(await screen.findByText("WDJB-MJHT")).toBeInTheDocument();
    // Waiting starts with the code, not after a click somewhere else.
    expect(forgeConnectWait).toHaveBeenCalledWith("github");
    expect(screen.getByText(browseWaitingSentence("GitHub"))).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: BROWSE_COPY_CODE_LABEL }));
    expect(writeText).toHaveBeenLastCalledWith("WDJB-MJHT");
    expect(await screen.findByText(BROWSE_CODE_COPIED)).toBeInTheDocument();

    // Open copies the code too, and the SHELL opens the page.
    writeText.mockClear();
    fireEvent.click(screen.getByRole("button", { name: browseOpenLabel("github.com") }));
    expect(writeText).toHaveBeenCalledWith("WDJB-MJHT");
    expect(forgeConnectOpen).toHaveBeenCalledWith("github");

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(forgeConnectCancel).toHaveBeenCalledWith("github");
  });

  it("says so when the code could not be copied or the page could not be opened", async () => {
    // No clipboard in this webview, and a shell that refuses to open the page.
    vi.mocked(forgeConnectStart).mockResolvedValue({
      userCode: "WDJB-MJHT",
      verificationUri: "https://github.com/login/device",
      expiresIn: 900,
    });
    vi.mocked(forgeConnectWait).mockReturnValue(new Promise(() => {}));
    // Unmounting mid-wait cancels; answered so the test stands alone.
    vi.mocked(forgeConnectCancel).mockResolvedValue(undefined);
    vi.mocked(forgeConnectOpen).mockRejectedValue({
      code: "internal",
      message: "No code is waiting.",
      accountId: null,
      retriable: false,
    });
    await openSheet([
      source({ via: "deviceFlow", state: "notConnected", login: null, canConnect: true }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: browseConnectLabel("GitHub") }));
    await screen.findByText("WDJB-MJHT");

    fireEvent.click(screen.getByRole("button", { name: BROWSE_COPY_CODE_LABEL }));
    expect(screen.getByRole("alert")).toHaveTextContent(BROWSE_COPY_FAILED);

    fireEvent.click(screen.getByRole("button", { name: browseOpenLabel("github.com") }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("No code is waiting."));
  });

  it("offers Connect GitHub whenever Rust says the source can connect, whatever its via", async () => {
    // A broker that has no grants for this person, with a device-flow client
    // id to fall back to: Rust answers `notConnected` + `canConnect`.
    await openSheet([
      source({ via: "broker", state: "notConnected", login: null, canConnect: true }),
    ]);
    expect(screen.getByRole("button", { name: browseConnectLabel("GitHub") })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: BROWSE_TRY_AGAIN_LABEL })).not.toBeInTheDocument();
  });

  it("shows why a source that cannot connect is not connected, with Try again instead", async () => {
    await openSheet([
      source({
        via: "deviceFlow",
        state: "notConnected",
        login: null,
        canConnect: false,
        sentence: "GitHub isn't set up for this build.",
      }),
    ]);
    expect(screen.getByText("GitHub isn't set up for this build.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: BROWSE_TRY_AGAIN_LABEL })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: browseConnectLabel("GitHub") }),
    ).not.toBeInTheDocument();
  });

  it("offers Disconnect on a connectable source, even one listed through the broker", async () => {
    vi.mocked(forgeRepos).mockResolvedValue(listing([repo("tgorka/keeper")]));
    vi.mocked(forgesList).mockResolvedValue([source({ via: "broker", canConnect: true })]);
    const first = await openEntry();
    await screen.findAllByTestId("forge-repo-row");
    expect(screen.getByRole("button", { name: BROWSE_DISCONNECT_LABEL })).toBeInTheDocument();
    first.unmount();

    resetForgesStoreForTest();
    vi.mocked(forgesList).mockResolvedValue([source({ via: "deviceFlow", canConnect: false })]);
    await openEntry();
    await screen.findAllByTestId("forge-repo-row");
    expect(screen.queryByRole("button", { name: BROWSE_DISCONNECT_LABEL })).not.toBeInTheDocument();
  });

  it("Try again asks the source again, so a remembered failure clears", async () => {
    // The shell answers the source from its last listing's failure until a
    // listing replaces it: re-reading the sources alone would never recover.
    let remembered = true;
    const failing = source({
      state: "unreachable",
      login: null,
      sentence: "Can't reach api.github.com.",
    });
    vi.mocked(forgesList).mockImplementation(async () => [remembered ? failing : source()]);
    vi.mocked(forgeRepos).mockImplementation(async (_id, refresh) => {
      if (refresh) {
        remembered = false;
      }
      return listing([repo("tgorka/keeper")]);
    });
    await openEntry();
    expect(screen.getByText("Can't reach api.github.com.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: BROWSE_TRY_AGAIN_LABEL }));
    expect(await screen.findByRole("checkbox", { name: "tgorka/keeper" })).toBeInTheDocument();
    expect(forgeRepos).toHaveBeenCalledWith("github", true);
  });

  it("Sign in asks the source again once the sign-in has resolved", async () => {
    let remembered = true;
    const signedOut = forgejo({ state: "needsSignIn", sentence: "Sign in to Acme again." });
    vi.mocked(forgesList).mockImplementation(async () => [remembered ? signedOut : forgejo()]);
    vi.mocked(accountSignIn).mockResolvedValue(accountVm());
    vi.mocked(forgeRepos).mockImplementation(async (_id, refresh) => {
      if (refresh) {
        remembered = false;
      }
      return listing([repo("keeper/team-notes")], "account-forge");
    });
    await openEntry();

    fireEvent.click(screen.getByRole("button", { name: BROWSE_SIGN_IN_LABEL }));
    expect(await screen.findByRole("checkbox", { name: "keeper/team-notes" })).toBeInTheDocument();
    expect(forgeRepos).toHaveBeenCalledWith("account-forge", true);
    // Asked after the sign-in, not before it: before, it would fail again.
    expect(vi.mocked(accountSignIn).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(forgeRepos).mock.invocationCallOrder[0],
    );
  });

  it("never shows one source's answer under another source's tab", async () => {
    // The slow answer is a Refresh's: the load the sheet starts by itself is
    // abandoned by its own effect on a switch, a Refresh (or Try again, or the
    // re-mark after an add) is not.
    let answerGithub: (vm: ForgeReposVm) => void = () => {};
    vi.mocked(forgeRepos).mockImplementation((id, refresh) => {
      if (id !== "github") {
        return Promise.resolve(listing([repo("keeper/team-notes")], "account-forge"));
      }
      if (!refresh) {
        return Promise.resolve(listing([repo("tgorka/dotfiles")]));
      }
      return new Promise<ForgeReposVm>((resolve) => {
        answerGithub = resolve;
      });
    });
    await openSheet([source(), forgejo()]);
    await screen.findByRole("checkbox", { name: "tgorka/dotfiles" });
    fireEvent.click(screen.getByRole("button", { name: BROWSE_REFRESH_LABEL }));
    // GitHub's refresh is slow; the person moves on to Acme.
    fireEvent.mouseDown(screen.getByRole("tab", { name: /Acme/ }), { button: 0 });
    expect(await screen.findByRole("checkbox", { name: "keeper/team-notes" })).toBeInTheDocument();

    await act(async () => answerGithub(listing([repo("tgorka/keeper")])));
    expect(screen.queryByRole("checkbox", { name: "tgorka/keeper" })).not.toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "keeper/team-notes" })).toBeInTheDocument();
  });

  it("marks added and elsewhere rows, and an added row cannot be picked or added again", async () => {
    vi.mocked(forgeRepos).mockResolvedValue(
      listing([
        repo("tgorka/dotfiles", { addedAs: ["dotfiles"] }),
        repo("tgorka/notes", { elsewhere: ["iphone-3f2a", "ipad-91c0"] }),
        repo("tgorka/keeper"),
      ]),
    );
    const onAddOne = await openSheet([source()]);
    await screen.findAllByTestId("forge-repo-row");

    const added = row("tgorka/dotfiles");
    expect(added).toBeDefined();
    if (added === undefined) return;
    expect(within(added).getByRole("checkbox")).toBeDisabled();
    expect(within(added).getByText(BROWSE_SYNCING_HERE, { exact: false })).toBeInTheDocument();
    expect(within(added).getByRole("button", { name: "dotfiles" })).toBeInTheDocument();
    expect(within(added).queryByRole("button", { name: /Add…/ })).not.toBeInTheDocument();

    const elsewhere = row("tgorka/notes");
    if (elsewhere === undefined) throw new Error("no notes row");
    expect(
      within(elsewhere).getByText(browseElsewhere(["iphone-3f2a", "ipad-91c0"])),
    ).toBeInTheDocument();
    expect(within(elsewhere).getByRole("checkbox")).toBeEnabled();

    // ⌘A picks every visible repository that is not already here.
    const list = screen.getByRole("group", { name: "Repositories" });
    fireEvent.keyDown(list, { key: "a", metaKey: true });
    expect(screen.getByText(browseSelected(2))).toBeInTheDocument();

    // A single Add… hands the add form the repository and its source's credential.
    fireEvent.click(
      within(elsewhere).getByRole("button", { name: `${BROWSE_ADD_ONE_LABEL} tgorka/notes` }),
    );
    await waitFor(() =>
      expect(onAddOne).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "notes",
          remoteUrl: "https://github.com/tgorka/notes.git",
          branch: "main",
          credential: "forge:github",
        }),
      ),
    );
    expect(onAddOne.mock.calls[0][0]).not.toHaveProperty("direction");
    // No drive folder from Rust (a phone): the form asks for one.
    expect(onAddOne.mock.calls[0][0]).not.toHaveProperty("localPath");
  });

  it("starts a single Add… in <drive folder>/<name>", async () => {
    vi.mocked(forgeRepos).mockResolvedValue(listing([repo("tgorka/notes")]));
    vi.mocked(forgeDefaultBaseFolder).mockResolvedValue("/Users/t/keeper/git/");
    const onAddOne = await openSheet([source()]);
    await screen.findAllByTestId("forge-repo-row");

    fireEvent.click(screen.getByRole("button", { name: `${BROWSE_ADD_ONE_LABEL} tgorka/notes` }));
    await waitFor(() =>
      expect(onAddOne).toHaveBeenCalledWith(
        expect.objectContaining({ localPath: "/Users/t/keeper/git/notes" }),
      ),
    );
  });

  it("selects all with ⌘A from the sheet itself, but leaves ⌘A in the search box to the text", async () => {
    vi.mocked(forgeRepos).mockResolvedValue(listing([repo("tgorka/keeper"), repo("tgorka/notes")]));
    await openSheet([source()]);
    await screen.findAllByTestId("forge-repo-row");

    fireEvent.keyDown(screen.getByLabelText(BROWSE_SEARCH_LABEL), { key: "a", metaKey: true });
    expect(screen.queryByText(browseSelected(2))).not.toBeInTheDocument();

    // Focus on the sheet's own body (a click on nothing in particular) is
    // outside the list and outside the toolbar.
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "a", metaKey: true });
    expect(screen.getByText(browseSelected(2))).toBeInTheDocument();
  });

  it("moves down from a row's Add… to the next row, not back to the first", async () => {
    vi.mocked(forgeRepos).mockResolvedValue(
      listing([repo("tgorka/alpha"), repo("tgorka/bravo"), repo("tgorka/charlie")]),
    );
    await openSheet([source()]);
    await screen.findAllByTestId("forge-repo-row");
    const [alphaAdd] = screen.getAllByRole("button", { name: /^Add… / });
    // Name order is the fixture's here: none has an update time.
    expect(alphaAdd).toHaveAccessibleName(`${BROWSE_ADD_ONE_LABEL} tgorka/alpha`);
    alphaAdd.focus();
    fireEvent.keyDown(alphaAdd, { key: "ArrowDown" });
    expect(screen.getByRole("checkbox", { name: "tgorka/bravo" })).toHaveFocus();
  });

  it("counts and adds only the ticks still listed and not added since", async () => {
    vi.mocked(forgeRepos)
      .mockResolvedValueOnce(
        listing([repo("tgorka/alpha"), repo("tgorka/bravo"), repo("tgorka/charlie")]),
      )
      // A refresh: alpha was added meanwhile (a restore, the other surface),
      // bravo is no longer listed.
      .mockResolvedValueOnce(
        listing([repo("tgorka/alpha", { addedAs: ["alpha"] }), repo("tgorka/charlie")]),
      );
    vi.mocked(forgeDefaultBaseFolder).mockResolvedValue("/Users/t/Drives");
    await openSheet([source()]);
    await screen.findAllByTestId("forge-repo-row");
    for (const name of ["tgorka/alpha", "tgorka/bravo", "tgorka/charlie"]) {
      fireEvent.click(screen.getByRole("checkbox", { name }));
    }
    expect(screen.getByText(browseSelected(3))).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: BROWSE_REFRESH_LABEL }));
    expect(await screen.findByText(browseSelected(1))).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: browseAddDrives(1, true) }));
    const rows = await screen.findAllByTestId("forge-batch-row");
    expect(rows).toHaveLength(1);
    expect(within(rows[0]).getByText("tgorka/charlie")).toBeInTheDocument();
  });

  it("says a read-only repository only downloads, and adds it from Add… as pull-only", async () => {
    vi.mocked(forgeRepos).mockResolvedValue(
      listing([
        repo("makistack/handbook", { canPush: false, pullOnly: true, pullOnlySentence: READ_ONLY }),
        repo("tgorka/keeper"),
      ]),
    );
    const onAddOne = await openSheet([source()]);
    await screen.findAllByTestId("forge-repo-row");

    const readOnly = row("makistack/handbook");
    if (readOnly === undefined) throw new Error("no handbook row");
    expect(within(readOnly).getByText(READ_ONLY)).toBeInTheDocument();
    const writable = row("tgorka/keeper");
    if (writable === undefined) throw new Error("no keeper row");
    expect(within(writable).queryByText(READ_ONLY)).not.toBeInTheDocument();

    fireEvent.click(
      within(readOnly).getByRole("button", {
        name: `${BROWSE_ADD_ONE_LABEL} makistack/handbook`,
      }),
    );
    await waitFor(() =>
      expect(onAddOne).toHaveBeenCalledWith(expect.objectContaining({ direction: "pullOnly" })),
    );
  });

  it("filters by search and shows forks only when asked", async () => {
    vi.mocked(forgeRepos).mockResolvedValue(
      listing([
        repo("tgorka/keeper", { description: "Keeps folders" }),
        repo("tgorka/tauri", { fork: true }),
        repo("makistack/handbook"),
      ]),
    );
    await openSheet([source()]);
    await screen.findAllByTestId("forge-repo-row");
    expect(row("tgorka/tauri")).toBeUndefined();

    fireEvent.click(screen.getByRole("button", { name: BROWSE_FORKS_LABEL }));
    expect(row("tgorka/tauri")).toBeDefined();

    fireEvent.change(screen.getByLabelText(BROWSE_SEARCH_LABEL), { target: { value: "folders" } });
    expect(screen.getAllByTestId("forge-repo-row")).toHaveLength(1);
    expect(row("tgorka/keeper")).toBeDefined();
  });
});

describe("the batch step", () => {
  async function toBatch(repos: ForgeRepoVm[], base: string | null = "/Users/t/Drives") {
    vi.mocked(forgeRepos).mockResolvedValue(listing(repos));
    vi.mocked(forgeDefaultBaseFolder).mockResolvedValue(base);
    await openSheet([source()]);
    await screen.findAllByTestId("forge-repo-row");
    for (const each of repos) {
      fireEvent.click(screen.getByRole("checkbox", { name: each.fullName }));
    }
    fireEvent.click(screen.getByRole("button", { name: browseAddDrives(repos.length, true) }));
    await waitFor(() => expect(forgeDefaultBaseFolder).toHaveBeenCalled());
    await act(async () => {});
  }

  it("blocks only the row whose name collides, and sends the rest", async () => {
    vi.mocked(forgeReposAdd).mockResolvedValue([
      { fullName: "tgorka/notes", profileId: "p9", sentence: null },
    ]);
    await toBatch([repo("tgorka/notes"), repo("makistack/notes")]);

    const rows = screen.getAllByTestId("forge-batch-row");
    expect(within(rows[0]).queryByText(batchNameTakenSentence("notes"))).not.toBeInTheDocument();
    expect(within(rows[1]).getByText(batchNameTakenSentence("notes"))).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: browseAddDrives(1, false) }));
    await waitFor(() => expect(forgeReposAdd).toHaveBeenCalled());
    expect(vi.mocked(forgeReposAdd).mock.calls[0][0]).toEqual({
      sourceId: "github",
      baseFolder: "/Users/t/Drives",
      repos: [{ fullName: "tgorka/notes", driveName: "notes", folder: null }],
    });

    // Renamed, the blocked row goes too.
    fireEvent.change(screen.getByLabelText(browseDriveNameLabel("makistack/notes")), {
      target: { value: "handbook-notes" },
    });
    expect(screen.queryByText(batchNameTakenSentence("notes"))).not.toBeInTheDocument();
  });

  it("reports each repository's result: added ones, and failures with Rust's sentence", async () => {
    vi.mocked(forgeReposAdd).mockResolvedValue([
      { fullName: "tgorka/notes", profileId: "p9", sentence: null },
      {
        fullName: "tgorka/playground",
        profileId: null,
        sentence: "/Users/t/Drives/playground holds other files.",
      },
    ]);
    await toBatch([repo("tgorka/notes"), repo("tgorka/playground")]);

    fireEvent.click(screen.getByRole("button", { name: browseAddDrives(2, false) }));
    expect(await screen.findByText(browseAddedSentence(1))).toBeInTheDocument();
    const rows = screen.getAllByTestId("forge-batch-row");
    expect(within(rows[0]).getByText("Added")).toBeInTheDocument();
    expect(
      within(rows[1]).getByText("/Users/t/Drives/playground holds other files."),
    ).toBeInTheDocument();
    // The failure stays listed and editable, with a way to try it again.
    expect(screen.getByLabelText(browseDriveNameLabel("tgorka/playground"))).toBeInTheDocument();
    expect(screen.getByRole("button", { name: browseAddDrives(1, false) })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Done" })).toBeInTheDocument();

    // Renamed, the row is a new question: the old folder's sentence goes.
    fireEvent.change(screen.getByLabelText(browseDriveNameLabel("tgorka/playground")), {
      target: { value: "playground-2" },
    });
    expect(
      screen.queryByText("/Users/t/Drives/playground holds other files."),
    ).not.toBeInTheDocument();
  });

  it("marks a read-only repository Download only before it is added", async () => {
    await toBatch([
      repo("makistack/handbook", { canPush: false, pullOnly: true, pullOnlySentence: READ_ONLY }),
      repo("tgorka/keeper"),
    ]);
    const [handbook, keeper] = screen.getAllByTestId("forge-batch-row");
    expect(within(handbook).getByText(BROWSE_DOWNLOAD_ONLY)).toBeInTheDocument();
    expect(within(keeper).queryByText(BROWSE_DOWNLOAD_ONLY)).not.toBeInTheDocument();
  });

  it("asks for the base folder on a desktop Rust had no default for", async () => {
    // No drives and no HOME: Rust answers no default. This is still a
    // desktop, so the field is there, empty, and nothing goes until it is filled.
    await toBatch([repo("tgorka/notes")], null);
    const field = screen.getByLabelText(BROWSE_BASE_LABEL);
    expect(field).toHaveValue("");
    expect(screen.getByText(BROWSE_BASE_MISSING)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: browseAddDrives(0, false) })).toBeDisabled();

    fireEvent.change(field, { target: { value: "/Users/t/Code" } });
    expect(screen.getByText("/Users/t/Code/notes")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: browseAddDrives(1, false) })).toBeEnabled();
  });
});
