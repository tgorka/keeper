/**
 * The verbs a space row has, and the two words it must never say (Story 51.6,
 * FR-297; matrix rows 9–11).
 *
 * **What is asserted here and what is not.** The rename's *arithmetic* — the
 * stamp that survives, the collision that refuses, the pointers that follow — is
 * `keeper_core::sessions::{files, refs}`'s and is asserted over real strings
 * there, on every machine (AD-56). What can only be got wrong on this side is the
 * argument the menu hands those commands and the words it puts on screen, so those
 * are the cases: the address, the block, and the vocabulary.
 *
 * The vocabulary case is not decoration. The owner's report asked for *"open in
 * new tab"* and this app has never had a tab; a menu that shipped the word would
 * teach a reader a model the rest of keeper does not have, and no other test in
 * the repo would notice.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const sessionsFilePath = vi.fn<(profileId: string, subpath: string) => Promise<string>>();
const sessionsFileRename =
  vi.fn<
    (profileId: string, subpath: string, block: string, nextBlock: string) => Promise<string>
  >();
const sessionsFileDelete =
  vi.fn<(rootId: string, sessionId: string, rel: string) => Promise<void>>();
const syncReadFrontmatter = vi.fn<(profileId: string, subpath: string) => Promise<string>>();
const syncOpenEntry = vi.fn<(profileId: string, subpath: string) => Promise<void>>();
const revealPath = vi.fn<(path: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  sessionsFilePath: (profileId: string, subpath: string) => sessionsFilePath(profileId, subpath),
  sessionsFileRename: (profileId: string, subpath: string, block: string, nextBlock: string) =>
    sessionsFileRename(profileId, subpath, block, nextBlock),
  sessionsFileDelete: (rootId: string, sessionId: string, rel: string) =>
    sessionsFileDelete(rootId, sessionId, rel),
  syncReadFrontmatter: (profileId: string, subpath: string) =>
    syncReadFrontmatter(profileId, subpath),
  syncOpenEntry: (profileId: string, subpath: string) => syncOpenEntry(profileId, subpath),
  revealPath: (path: string) => revealPath(path),
  // Reached by `properties-panel`'s module graph rather than by this menu, and
  // present so importing the splice helpers does not drag a real `invoke` in.
  notesSave: vi.fn(),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(),
  recordingSessionMeta: vi.fn(),
  tagsVocabulary: vi.fn(async () => ({ entries: [] })),
  syncWriteFrontmatter: vi.fn(),
}));

import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import {
  SPACE_ROW_COPY_PATH_LABEL,
  SPACE_ROW_DELETE_LABEL,
  SPACE_ROW_OPEN_BESIDE_LABEL,
  SPACE_ROW_OPEN_HERE_LABEL,
  SPACE_ROW_OPEN_LABEL,
  SPACE_ROW_RENAME_FIELD_LABEL,
  SPACE_ROW_RENAME_LABEL,
  SPACE_ROW_REVEAL_LABEL,
  SpaceRowMenu,
} from "./space-row-menu";

const ROOT = "tgdrive";
const SESSION = "01J4SESSION";
const REL = "2026-08-16-1812-untitled.md";
const SUBPATH = `60-sessions/active/2026-08-16-keeper/${REL}`;
const ABSOLUTE = `/Users/t/tgdrive/${SUBPATH}`;
const BLOCK = "---\ntitle: untitled\n---\n";
const ROW_LABEL = "untitled";

/**
 * What Rust answers a rename with in the Story 52.2 cases below.
 *
 * A different DIRECTORY, and a filename that is neither the old one nor the
 * typed title — so nothing on this side could have composed it from `SUBPATH`
 * plus "Kick Off". An assertion that finds this string got it from the command
 * (AD-65). The `beforeEach` default deliberately stays a plausible sibling path,
 * because the tests that are not about re-pointing should read like real life.
 */
const MOVED_SUBPATH = "60-sessions/archive/2026-02/kick-off-notes.md";

function mount(
  handlers: {
    onOpen?: (subpath: string) => void;
    onChanged?: () => void;
    onNotice?: () => void;
  } = {},
) {
  const onOpen = vi.fn<(subpath: string) => void>(handlers.onOpen);
  const onChanged = vi.fn(handlers.onChanged);
  const onNotice = vi.fn(handlers.onNotice);
  render(
    <SpaceRowMenu
      rootId={ROOT}
      sessionId={SESSION}
      relPath={REL}
      subpath={SUBPATH}
      title={ROW_LABEL}
      onOpen={onOpen}
      onChanged={onChanged}
      onNotice={onNotice}
    >
      <button type="button">{ROW_LABEL}</button>
    </SpaceRowMenu>,
  );
  return { onOpen, onChanged, onNotice, row: screen.getByRole("button", { name: ROW_LABEL }) };
}

/** Right-click the row, which is the gesture the whole surface exists for. */
async function openMenu(row: HTMLElement): Promise<void> {
  fireEvent.contextMenu(row);
  await screen.findByRole("menuitem", { name: SPACE_ROW_OPEN_HERE_LABEL });
}

beforeEach(() => {
  vi.clearAllMocks();
  resetPanelsStoreForTest();
  // Reveal is platform-gated, and the interesting default here is the platform
  // that HAS it: the one case about its absence turns it back off.
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: true });
  sessionsFilePath.mockResolvedValue(ABSOLUTE);
  sessionsFileRename.mockResolvedValue(
    "60-sessions/active/2026-08-16-keeper/2026-08-16-1812-kick-off.md",
  );
  sessionsFileDelete.mockResolvedValue(undefined);
  syncReadFrontmatter.mockResolvedValue(BLOCK);
  syncOpenEntry.mockResolvedValue(undefined);
  revealPath.mockResolvedValue(undefined);
});

describe("a space row's context menu", () => {
  /** Row 9. Six verbs, and the row keeps the single click it always had. */
  it("offers every verb a file row offers in Files, plus rename and delete", async () => {
    const { row } = mount();

    await openMenu(row);

    for (const label of [
      SPACE_ROW_OPEN_HERE_LABEL,
      SPACE_ROW_OPEN_BESIDE_LABEL,
      SPACE_ROW_OPEN_LABEL,
      SPACE_ROW_REVEAL_LABEL,
      SPACE_ROW_COPY_PATH_LABEL,
      SPACE_ROW_RENAME_LABEL,
      SPACE_ROW_DELETE_LABEL,
    ]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeInTheDocument();
    }
  });

  /**
   * Row 9, the wording half. This app's multi-document model is panels, so the
   * word the owner asked for has no referent — and the two panel items are
   * `files-pane.tsx`'s strings character for character.
   */
  it("says panel, never tab", async () => {
    const { row } = mount();

    await openMenu(row);

    const menu = screen.getByRole("menu");
    expect(menu.textContent).toContain("Open in this panel");
    expect(menu.textContent).toContain("Open in a new panel");
    expect(menu.textContent?.toLowerCase()).not.toContain("tab");
  });

  /** Reveal is the platform's, so it is absent where the platform has no reveal. */
  it("drops Reveal where the platform cannot reveal a path", async () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: false });
    const { row } = mount();

    await openMenu(row);

    expect(screen.queryByRole("menuitem", { name: SPACE_ROW_REVEAL_LABEL })).toBeNull();
  });

  /**
   * Row 11. The only route to five of these verbs is this menu, so a menu a
   * keyboard cannot open is five verbs behind a pointer. `Shift+F10` is the
   * keystroke; Escape is Radix's own.
   */
  it("opens from the keyboard and closes on Escape", async () => {
    const { row } = mount();

    fireEvent.keyDown(row, { key: "F10", shiftKey: true });
    const first = await screen.findByRole("menuitem", { name: SPACE_ROW_OPEN_HERE_LABEL });
    expect(first).toBeInTheDocument();

    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("menuitem", { name: SPACE_ROW_OPEN_HERE_LABEL })).toBeNull(),
    );
  });

  /** A keystroke that is not the menu key is left alone for the row to handle. */
  it("ignores an unrelated keystroke", () => {
    const { row } = mount();

    fireEvent.keyDown(row, { key: "F10" });

    expect(screen.queryByRole("menu")).toBeNull();
  });

  /**
   * The row's own opener, not a second one: the section already has exactly one
   * function that opens a space file, and a menu item calling `setActiveTarget`
   * itself would be a second implementation of the click.
   */
  it("opens in this panel through the surface's own opener", async () => {
    const { row, onOpen } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_OPEN_HERE_LABEL }));

    expect(onOpen).toHaveBeenCalledWith(SUBPATH);
  });

  /** Open beside is the panels store's own verb, on the one file target (AD-109). */
  it("opens a new panel on the file target Rust composed", async () => {
    const { row } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_OPEN_BESIDE_LABEL }));

    await waitFor(() =>
      expect(
        panelsStore.getState().panels.some(({ target }) => {
          if (target === null || target.kind !== "file") {
            return false;
          }
          return target.profileId === ROOT && target.relativePath === SUBPATH;
        }),
      ).toBe(true),
    );
  });

  /**
   * The two verbs that need an absolute path ask for one when they run. AD-65:
   * the path is Rust's, and this side never joins the profile root onto a
   * subpath.
   */
  it("asks Rust where the file is before revealing it", async () => {
    const { row } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_REVEAL_LABEL }));

    await waitFor(() => expect(sessionsFilePath).toHaveBeenCalledWith(ROOT, SUBPATH));
    await waitFor(() => expect(revealPath).toHaveBeenCalledWith(ABSOLUTE));
  });

  it("hands the file to the system opener by its own subpath", async () => {
    const { row } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_OPEN_LABEL }));

    await waitFor(() => expect(syncOpenEntry).toHaveBeenCalledWith(ROOT, SUBPATH));
  });

  /**
   * Row 10. One implementation, and this is the assertion that says so: the menu
   * does not send a bare title, it reads the block, splices `title:` with the
   * panel's own serialiser and calls the panel's own command.
   */
  it("renames through the same block splice and the same command as the properties title", async () => {
    const { row, onChanged } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_RENAME_LABEL }));
    const field = await screen.findByRole("textbox", { name: SPACE_ROW_RENAME_FIELD_LABEL });
    fireEvent.change(field, { target: { value: "Kick Off" } });
    fireEvent.click(screen.getByRole("button", { name: SPACE_ROW_RENAME_LABEL }));

    await waitFor(() => expect(syncReadFrontmatter).toHaveBeenCalledWith(ROOT, SUBPATH));
    await waitFor(() =>
      expect(sessionsFileRename).toHaveBeenCalledWith(
        ROOT,
        SUBPATH,
        BLOCK,
        "---\ntitle: Kick Off\n---\n",
      ),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  /** Enter commits, because a dialog with one field is a field you press Enter on. */
  it("commits the rename on Enter", async () => {
    const { row } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_RENAME_LABEL }));
    const field = await screen.findByRole("textbox", { name: SPACE_ROW_RENAME_FIELD_LABEL });
    fireEvent.change(field, { target: { value: "Kick Off" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(sessionsFileRename).toHaveBeenCalledTimes(1));
  });

  /**
   * Story 52.2, FR-302. The rename's answer is the file's new subpath, and a
   * pane left on the old one renders "is no longer in tgdrive" over a file that
   * merely changed its name — the owner's report, reachable from this menu as
   * well as from the properties panel.
   *
   * Through the panels store's `retargetPanels` and not the section's `onOpen`:
   * the opener moves the ACTIVE pane, which is the right answer for the row's own
   * click and the wrong one for a rename — see the two cases below.
   */
  it("re-points the pane that was showing the file at the subpath the rename answered with", async () => {
    sessionsFileRename.mockResolvedValue(MOVED_SUBPATH);
    // The reader's pane, on the file that is about to be renamed.
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: ROOT, relativePath: SUBPATH });
    const { row } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_RENAME_LABEL }));
    const field = await screen.findByRole("textbox", { name: SPACE_ROW_RENAME_FIELD_LABEL });
    fireEvent.change(field, { target: { value: "Kick Off" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() =>
      expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([
        { kind: "file", profileId: ROOT, relativePath: MOVED_SUBPATH },
      ]),
    );
  });

  /**
   * The gap the `activeId` guard left, and it is the same defect as the banner
   * rather than a smaller one: a pane that is not focused is still a pane on
   * screen, and the panel list is PERSISTED — so the pane nobody re-pointed keeps
   * the emptied address, shows Rust's missing-file sentence, and writes that dead
   * path into the cookie for the next launch to restore.
   *
   * The guard's real requirement is "do not move a pane that is not showing this
   * file". Matching on the target satisfies it without also asking which pane
   * happens to have focus when the round trip answers.
   */
  it("re-points a pane showing the file even when another pane has focus", async () => {
    const OTHER = "60-sessions/active/2026-08-16-keeper/README.md";
    sessionsFileRename.mockResolvedValue(MOVED_SUBPATH);
    // The file is open on the left; the reader is working in the pane on the
    // right, which is where the right-click on the space row happened from.
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: ROOT, relativePath: SUBPATH });
    panelsStore.getState().openPanel({ kind: "file", profileId: ROOT, relativePath: OTHER });
    const right = panelsStore.getState().activeId;
    const { row } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_RENAME_LABEL }));
    const field = await screen.findByRole("textbox", { name: SPACE_ROW_RENAME_FIELD_LABEL });
    fireEvent.change(field, { target: { value: "Kick Off" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() =>
      expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([
        { kind: "file", profileId: ROOT, relativePath: MOVED_SUBPATH },
        { kind: "file", profileId: ROOT, relativePath: OTHER },
      ]),
    );
    // Nothing was navigated: the reader is still in the pane they were in.
    expect(panelsStore.getState().activeId).toBe(right);
  });

  /**
   * The third gap, and the one no arrangement of panels could have shown: the
   * guard ran AFTER a `syncReadFrontmatter` plus a `sessionsFileRename` round
   * trip, so a focus change while the rename was in flight decided which pane
   * followed it. Matching on the target makes the answer independent of when the
   * command answers.
   */
  it("re-points the pane that was showing the file even if focus moved while the rename was in flight", async () => {
    let answer: (subpath: string) => void = () => {};
    sessionsFileRename.mockReturnValue(
      new Promise<string>((resolve) => {
        answer = resolve;
      }),
    );
    const OTHER = "60-sessions/active/2026-08-16-keeper/README.md";
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: ROOT, relativePath: SUBPATH });
    panelsStore.getState().openPanel({ kind: "file", profileId: ROOT, relativePath: OTHER });
    const [holder] = panelsStore.getState().panels;
    if (holder === undefined) {
      throw new Error("expected two panels");
    }
    // Focus back on the pane showing the file, which is what the guard used to
    // require — and then it moves away before the command answers.
    panelsStore.getState().focusPanel(holder.id);
    const { row } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_RENAME_LABEL }));
    const field = await screen.findByRole("textbox", { name: SPACE_ROW_RENAME_FIELD_LABEL });
    fireEvent.change(field, { target: { value: "Kick Off" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(sessionsFileRename).toHaveBeenCalledTimes(1));
    const other = panelsStore.getState().panels[1];
    if (other === undefined) {
      throw new Error("expected two panels");
    }
    panelsStore.getState().focusPanel(other.id);
    answer(MOVED_SUBPATH);

    await waitFor(() =>
      expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([
        { kind: "file", profileId: ROOT, relativePath: MOVED_SUBPATH },
        { kind: "file", profileId: ROOT, relativePath: OTHER },
      ]),
    );
  });

  /**
   * The difference between following a rename and hijacking a pane. A rename of a
   * row nobody has open must leave every pane exactly where it is — a menu verb
   * that replaced the open file would be a worse defect than the banner.
   */
  it("leaves a pane that was showing another file where it was", async () => {
    const OTHER = "60-sessions/active/2026-08-16-keeper/README.md";
    sessionsFileRename.mockResolvedValue(MOVED_SUBPATH);
    panelsStore.getState().setActiveTarget({ kind: "file", profileId: ROOT, relativePath: OTHER });
    const { row, onOpen, onChanged } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_RENAME_LABEL }));
    const field = await screen.findByRole("textbox", { name: SPACE_ROW_RENAME_FIELD_LABEL });
    fireEvent.change(field, { target: { value: "Kick Off" } });
    fireEvent.keyDown(field, { key: "Enter" });

    // The space is still re-read — the pool changed — but nothing moved.
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(onOpen).not.toHaveBeenCalled();
    const { panels, activeId } = panelsStore.getState();
    expect(panels.find((panel) => panel.id === activeId)?.target).toEqual({
      kind: "file",
      profileId: ROOT,
      relativePath: OTHER,
    });
  });

  /**
   * Rust's own sentence, printed rather than re-worded, and printed into the
   * section's live region rather than into a second paragraph of this row's own.
   * The two refusals that matter both arrive this way: a collision naming the
   * file it would have overwritten, and a title that folds to nothing.
   */
  it("reports a refused rename in Rust's own words", async () => {
    const refusal =
      "renaming 2026-08-16-1812-untitled.md would overwrite 2026-08-16-1812-kick-off.md, which is already in this session.";
    sessionsFileRename.mockRejectedValue({ message: refusal });
    const { row, onNotice } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_RENAME_LABEL }));
    const field = await screen.findByRole("textbox", { name: SPACE_ROW_RENAME_FIELD_LABEL });
    fireEvent.change(field, { target: { value: "Kick Off" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(onNotice).toHaveBeenCalledWith(refusal));
  });

  /** Delete is the session tree's command, confirmed, and named in the question. */
  it("deletes only after the confirmation names the file", async () => {
    const { row, onChanged } = mount();
    await openMenu(row);

    fireEvent.click(screen.getByRole("menuitem", { name: SPACE_ROW_DELETE_LABEL }));
    expect(await screen.findByText(new RegExp(REL))).toBeInTheDocument();
    expect(sessionsFileDelete).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: SPACE_ROW_DELETE_LABEL }));

    await waitFor(() => expect(sessionsFileDelete).toHaveBeenCalledWith(ROOT, SESSION, REL));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });
});
