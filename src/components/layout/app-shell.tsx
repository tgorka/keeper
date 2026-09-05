import { type MouseEvent, useCallback, useEffect, useRef } from "react";
import { ApprovalPane } from "@/components/approval/approval-pane";
import { BotsPane } from "@/components/bots/bots-pane";
import { NewChatDialog } from "@/components/chat/new-chat-dialog";
import { CheatSheetOverlay } from "@/components/cheat-sheet/cheat-sheet-overlay";
import { CommandPalette } from "@/components/command-palette/command-palette";
import { ExportDialog } from "@/components/export/export-dialog";
import { BridgesPane } from "@/components/layout/bridges-pane";
import { ChatListPane } from "@/components/layout/chat-list-pane";
import { ConversationPane } from "@/components/layout/conversation-pane";
import { DetailPanel } from "@/components/layout/detail-panel";
import { FilesPane } from "@/components/layout/files-pane";
import { PanelStrip } from "@/components/layout/panel-strip";
import { PhoneShell } from "@/components/layout/phone-shell";
import { RecordingPane } from "@/components/layout/recording-pane";
import { SettingsPane } from "@/components/layout/settings-pane";
import { SIDEBAR_WIDTH_CLASS, SidebarPane } from "@/components/layout/sidebar-pane";
import { SyncPane } from "@/components/layout/sync-pane";
import { TASKS_PANEL_EMPTY_SENTENCE, TasksPane } from "@/components/layout/tasks-pane";
import { VerifyBanner } from "@/components/layout/verify-banner";
import { NotesPane } from "@/components/notes/notes-pane";
import { RecordingsPane } from "@/components/recordings/recordings-pane";
import { SearchOverlay } from "@/components/search/search-overlay";
import { SessionsPane } from "@/components/sessions/sessions-pane";
import { DeviceVerificationDialog } from "@/components/settings/device-verification-dialog";
import { KeyBackupDialog } from "@/components/settings/key-backup-dialog";
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useAccountStatuses } from "@/hooks/use-account-statuses";
import { useApprovalShortcut } from "@/hooks/use-approval-shortcut";
import { useBotsShortcut } from "@/hooks/use-bots-shortcut";
import { useBridgeHealthSubscription } from "@/hooks/use-bridge-health";
import { useBridgesShortcut } from "@/hooks/use-bridges-shortcut";
import { useCheatSheetShortcut } from "@/hooks/use-cheat-sheet-shortcut";
import { useCommandPaletteShortcut } from "@/hooks/use-command-palette-shortcut";
import { useEncryptionStatuses } from "@/hooks/use-encryption-statuses";
import { useGlobalHotkey } from "@/hooks/use-global-hotkey";
import { useKeyBackupStatuses } from "@/hooks/use-key-backup-statuses";
import { useMenuActions } from "@/hooks/use-menu-actions";
import { useNewChatShortcut } from "@/hooks/use-new-chat-shortcut";
import { useNotesShortcut } from "@/hooks/use-notes-shortcut";
import { useQuickSwitcher } from "@/hooks/use-quick-switcher";
import { useRecordingHotkey } from "@/hooks/use-recording-hotkey";
import { useRecordingShortcut } from "@/hooks/use-recording-shortcut";
import { useSearchShortcuts } from "@/hooks/use-search-shortcuts";
import { useSessionsShortcut } from "@/hooks/use-sessions-shortcut";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { useTasksShortcut } from "@/hooks/use-tasks-shortcut";
import { useUnreadJump } from "@/hooks/use-unread-jump";
import { useVerification } from "@/hooks/use-verification";
import { useViewShortcuts } from "@/hooks/use-view-shortcuts";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { hydrateColumnFold } from "@/lib/stores/column-fold";
import { useDetailStore } from "@/lib/stores/detail-ui";
import { hydrateFilesTree } from "@/lib/stores/files-tree";
import { hydratePanels, usePanelsStore } from "@/lib/stores/panels";
import { usePrimaryView } from "@/lib/stores/primary-view";
import { hydrateSidebarFold, sidebarFoldStore, useSidebarFold } from "@/lib/stores/sidebar-fold";
import { beginTitleBarDrag } from "@/lib/titlebar-drag";
import { cn } from "@/lib/utils";

export function AppShell() {
  // `narrow` is the VIEWPORT's answer, not the user's. Kept under its own name
  // because the two are combined a few lines down and reading one as the other
  // is how a user's fold would silently win over "there is no room".
  const { phone, sidebarCollapsed: narrow, detailFloating } = useShellLayout();
  // Stream every account's connectivity into the per-account status store: the
  // switcher glyphs, the shell offline pill, and the "Queued" send caption are
  // all pure projections of that single map.
  useAccountStatuses();
  // Stream every account's device-verification status into the encryption store:
  // the verify banner and the Settings badge are pure projections of that map.
  useEncryptionStatuses();
  // Subscribe every account's interactive verification flow: an incoming request
  // auto-opens the device-verification modal, and keeper-started flows stream here.
  useVerification();
  // Stream every account's key-backup status into the key-backup store: the
  // Settings backup row is a pure projection of that map.
  useKeyBackupStatuses();
  // Stream live bridge-session health across every account into the bridge-health
  // store: the card dot + state word, the sidebar roll-up, the affected chat-row dot,
  // and the in-conversation re-link banner are pure projections of that map (Story 6.5).
  useBridgeHealthSubscription();
  // Wire the search entry points (⌘⇧F global, ⌘F in-chat) to the search surface.
  useSearchShortcuts();
  // Wire ⌘N to the new-chat dialog (Story 6.6).
  useNewChatShortcut();
  // Wire ⌘4 to the Bridges surface (Story 6.1).
  useBridgesShortcut();
  // Wire ⌘5 to the Recording surface (Story 16.3); a no-op unless the recording
  // capability is on (desktop macOS ≥ 13.0).
  useRecordingShortcut();
  // Wire ⌘6 to the Notes view and the ⌘⌥ notes verb cluster (Story 37.1); every
  // chord self-gates on the `notes` capability, so none of them is a dead key
  // where notes cannot exist.
  useNotesShortcut();
  // Wire ⌘7 to the Sessions board and ⌘⌥L to log-today (Phase 7, FR-251);
  // self-gated on the sessions capability by the same rule.
  useSessionsShortcut();
  // Wire ⌘8 to the Tasks view (Epic 57, FR-351, FR-352), self-gated on the same
  // capability — the third surface over the one `sync.db` (AD-137).
  useTasksShortcut();
  // Wire ⌘9 to the Bots view (Epic 61, FR-378), self-gated on its OWN `bots`
  // capability — the first surface here that needs neither `git` nor `sync.db`.
  useBotsShortcut();
  // Wire ⌘3 to the Approval Pane (Story 7.3).
  useApprovalShortcut();
  // Wire ⌘K to toggle the command palette (Story 9.1).
  useCommandPaletteShortcut();
  // Wire ⌘? to toggle the shortcut cheat sheet (Story 9.3).
  useCheatSheetShortcut();
  // Route native-menu clicks through the shared palette dispatch (Story 9.3).
  useMenuActions();
  // Wire ⌘1/⌘2 to Inbox/Archive (Story 9.2), completing the ⌘1–4 view set.
  useViewShortcuts();
  // Wire ⌃Tab/⌃⇧Tab to cycle the open chat over the rendered window (Story 9.2).
  useQuickSwitcher();
  // Wire ⌥⌘↓/⌥⌘↑ to jump next/previous-unread in the rendered window (Story 9.2).
  useUnreadJump();
  // Listen for the OS-global summon hotkey (Story 9.4): a raise switches to Inbox and
  // moves keyboard focus into the chat list via the focus-request nonce store.
  useGlobalHotkey();
  // Listen for the optional OS-global Start/Stop Recording hotkey (Story 20.4):
  // a press toggles capture through the shared recording-control module. The
  // hook self-gates on the `recording` capability (inert everywhere else).
  useRecordingHotkey();
  // Restore the panel arrangement from the last run (Story 45.1, FR-173).
  //
  // Here rather than at the panel store's module load, and here rather than
  // inside the strip: the notes list can retarget a panel while the Notes
  // surface is up and the strip has never mounted, and an unhydrated store
  // would then overwrite the remembered arrangement with one panel. Mounted at
  // the shell so it runs exactly once per app, whatever surface comes up first
  // — a restore the shell does not mount is a restore no hook-level test can
  // ever see it fail to make (DW-172). `hydratePanels` is idempotent, so
  // React's double-invoked development effects restore once.
  //
  // Not on the phone tier (Epic 65, FR-443). All four cookies below are
  // desktop state — a drawer, a panel strip, a Files tree and surface columns
  // the stack never mounts — and until AD-189 the desktop frame rendered on a
  // rotated iPhone, where a press on its drawer or a column fold wrote them,
  // and the next restore read them back into a phone that had no such
  // controls. Nothing on the phone tier reads these stores, and nothing on it
  // can write them: the sidebar is unmounted, the chat list's column is
  // `enabled: !phone`, and Files, Panels, Tasks and the desktop Bots pane are
  // desktop-only. So the honest thing is to leave the cookies unread there —
  // including one an older build's landscape session left behind. On the
  // desktop the effect runs at mount exactly as before; on a harness window
  // that crosses 768px it runs on the first frame of the desktop frame, and
  // each restore's own once-per-document latch keeps that a single read.
  useEffect(() => {
    if (phone) {
      return;
    }
    hydratePanels(document.cookie);
  }, [phone]);
  // Restore the fold from the last run (Story 45.20, FR-198), in the same place
  // and for the same reason: the drawer is unmounted on the phone tier and can
  // be unmounted for a whole session, so a restore that lived inside it would
  // silently not happen. Idempotent, like `hydratePanels`.
  useEffect(() => {
    if (phone) {
      return;
    }
    hydrateSidebarFold(document.cookie);
  }, [phone]);
  // Restore which folders the Files tree had open (Story 46.3), for the third
  // time and the same reason: `FilesPane` is unmounted by every surface switch,
  // which is exactly the defect — the tree forgot itself whenever you looked at
  // something else. Idempotent, like the two above.
  useEffect(() => {
    if (phone) {
      return;
    }
    hydrateFilesTree(document.cookie);
  }, [phone]);
  // Restore which surface COLUMNS were folded (Story 48.1), for the fourth time
  // and the same reason, doubled: the notes rail, the note list, the Files tree
  // and the chat list live on three different primary views, and every one of
  // them is unmounted whenever another is showing. Four hydration points would
  // be four chances to forget one, and the forgotten one is invisible until
  // somebody switches surfaces twice. Idempotent, like the three above.
  useEffect(() => {
    if (phone) {
      return;
    }
    hydrateColumnFold(document.cookie);
  }, [phone]);
  // The user's fold, and how it composes with the viewport's.
  //
  // OR, not "the user wins": below 1080px there is no room for a 260px drawer
  // and that has been true since long before the fold existed, so the narrow
  // viewport forces the rail and the toggle is withdrawn rather than offered as
  // a control that would lie. Above it, the choice is entirely the user's — and
  // it is remembered, so widening the window later brings back the fold they
  // chose rather than the one the window imposed.
  const menuFolded = useSidebarFold((s) => s.menu);
  const sidebarCollapsed = narrow || menuFolded;
  const toggleFold = useCallback(() => sidebarFoldStore.getState().toggleMenu(), []);
  // Which primary view the shell renders. "bridges" and "approval" each replace the
  // chat-list + conversation cluster with a full-surface pane (Story 6.1 / 7.3).
  const primaryView = usePrimaryView();
  // Whether ANY panel is holding something (Story 59.13).
  //
  // Read here rather than inside `PanelStrip`, because it decides whether the
  // strip is mounted at all in the Tasks view — see that branch for why. A
  // target of any kind counts: a file left open in the Files surface is still a
  // document somebody asked to keep, and the strip's job does not depend on
  // which surface filled it.
  const anyPanelHolds = usePanelsStore((s) => s.panels.some((panel) => panel.target !== null));
  // Detail-open lives in the lifted `detailStore` (Story 13.1) so the desktop
  // frame and the phone stack project one shared signal; the toggle-focus-return
  // on close stays here, wrapping the store's `closeDetail`.
  const detailOpen = useDetailStore((s) => s.open);
  const openDetail = useDetailStore((s) => s.openDetail);
  const storeCloseDetail = useDetailStore((s) => s.closeDetail);
  const toggleDetail = useDetailStore((s) => s.toggleDetail);
  const toggleRef = useRef<HTMLButtonElement>(null);
  // The ⌘? cheat-sheet overlay and the native menu bar are the two projections of
  // the same action registry (Story 9.3), so `nativeMenuBar` is the honest flag:
  // where there's no native menu bar (the phone tier) the cheat sheet is unmounted.
  // The `useCheatSheetShortcut()` hook stays wired above (rules-of-hooks); only the
  // overlay is gated, so an unmounted overlay simply cannot render.
  const nativeMenuBar = useCapabilitiesStore((s) => s.capabilities.nativeMenuBar);
  // Screen recording is a desktop-macOS-≥13 capability (Story 16.3): the ⌘5
  // Recording view renders only when the flag is on, so a stale "recording"
  // primary-view can never show the pane on a platform that cannot record. The
  // Recordings browser (Story 42.3) rides the same flag for the same reason —
  // a browser over sessions this build cannot record is a puzzle, not a surface.
  const recording = useCapabilitiesStore((s) => s.capabilities.recording);
  // Folder sync needs a usable `git` (Story 32.5, AD-41): same rule, so a stale
  // "sync" primary-view can never show the pane where sync cannot run.
  const sync = useCapabilitiesStore((s) => s.capabilities.sync);
  // A notes vault is a folder keeper already syncs (AD-54, FR-122): same rule, so
  // a stale "notes" primary-view can never show the pane where no vault can exist.
  const notes = useCapabilitiesStore((s) => s.capabilities.notes);
  // A sessions root is the same construction (AD-107, FR-223): same rule again.
  const sessions = useCapabilitiesStore((s) => s.capabilities.sessions);
  // Bots is the one gated view whose flag is not the sync gate (Epic 61,
  // FR-378): same rule, different fact, so a stale "bots" primary-view can
  // never show the pane on a build that has no bots surface — and the pane is
  // NOT hidden on a desktop whose `git` is too old, because a conversation
  // does not need one.
  const bots = useCapabilitiesStore((s) => s.capabilities.bots);
  // Where the platform floats the window controls over the webview (desktop macOS,
  // via the macOS-only `titleBarStyle`/`hiddenTitle` keys) the app owes the window
  // its own drag region; under a real title bar the same band would be empty space
  // under chrome the OS already draws, so it is absent there (AD-34-2).
  const overlayTitleBar = useCapabilitiesStore((s) => s.capabilities.overlayTitleBar);

  const closeDetail = useCallback(() => {
    storeCloseDetail();
    // Return focus to the toggle control on close.
    toggleRef.current?.focus();
  }, [storeCloseDetail]);

  const handleSheetOpenChange = useCallback(
    (open: boolean) => {
      if (open) {
        openDetail();
      } else {
        closeDetail();
      }
    },
    [openDetail, closeDetail],
  );

  // The band starts the window drag itself rather than relying only on Tauri's
  // `data-tauri-drag-region` shim, which invokes the same `start_dragging` command
  // and then drops the promise — so a refused drag moves nothing and says nothing.
  // `beginTitleBarDrag` issues the call where its outcome can be recorded (Story
  // 34.3). Both columns keep the attribute: wherever this handler does not run,
  // the shim still behaves exactly as it did before.
  const handleBandMouseDown = useCallback((event: MouseEvent<HTMLDivElement>) => {
    // Primary button; a direct hit on the band itself, which is Tauri's own rule
    // for a bare `data-tauri-drag-region` and keeps a future child of the band
    // from becoming a drag handle; and the opening click only, because on macOS
    // the shim implements double-click-to-zoom on the following `mouseup` and a
    // drag started from the second `mousedown` would swallow that gesture.
    if (event.button !== 0 || event.detail !== 1 || event.target !== event.currentTarget) {
      return;
    }
    // Take the gesture over instead of letting the shim fire as well: one
    // `start_dragging` per mouse-down keeps the recorded outcome attributable to
    // this call, and `preventDefault` is what the shim spends the event on too —
    // suppressing the text cursor. Nothing here reads or writes React state, so
    // the mouse-down cannot re-render the band out from under the drag, and the
    // callback closes over nothing that could go stale.
    event.preventDefault();
    event.stopPropagation();
    void beginTitleBarDrag();
  }, []);

  return (
    <TooltipProvider>
      {/* `h-dvh`, not `h-screen` (Epic 65, AD-189): on a phone `100vh` is the
          LARGEST viewport — the one with the browser chrome retracted — so a
          root sized by it is taller than what is on screen and its bottom is
          off it; `100dvh` is the height the viewport has right now, and it
          follows a rotation, the keyboard's `--kb-inset` aside, and the
          safe-area insets the phone shell already pads for. On the desktop the
          two are the same number.

          Measured on the phone rig (`dev/measure-bots.ts --phone` and
          `--phone-landscape`, hesperia's headless Chrome, iPhone 14 Pro Max
          metrics, 2026-09-05), the Bots conversation open — where a headless
          viewport has no dynamic chrome, so the numbers show the TIER and its
          budget, not the `vh`/`dvh` gap:

            430×932 before  phone tier   transcript 742 of 932px = 79.6%
            932×430 before  DESKTOP tier transcript  48 of 430px = 11.2%
                            (a rail, "MENU", a Conversation list column, the
                            desktop pane's header and voice block; the Talk and
                            Send controls clipped at the bottom edge)
            430×932 → 932×430 rotated, before: desktop tier, the open
                            conversation gone, transcript 24 of 430px = 5.6%
            430×932 after   phone tier   transcript 742 of 932px = 79.6%
            932×430 after   phone tier   transcript 240 of 430px = 55.8%
                            (52px back bar, 37px state line, 102px composer;
                            bottom edge at 430 = inside the viewport)
            rotated either way, after: still the phone tier, the same
                            conversation level still open, 79.6% ⇄ 55.8% */}
      <div className="flex h-dvh flex-col overflow-hidden bg-background text-foreground">
        {/* One drag band, painted per column (AD-34-2, AD-34-3). It is the single
            element that both makes the window movable and clears the floating
            window controls, which is why no pane reserves an inset of its own. The
            band spans two panes with two different backgrounds, so it is painted
            per column to match what sits beneath it: one full-width `bg-background`
            strip above a `bg-sidebar` drawer reads as a seam in light mode and a
            black bar in dark, and that is the whole of the reported black strip. */}
        {overlayTitleBar && (
          <div className="flex h-7 shrink-0">
            {!phone && (
              // biome-ignore lint/a11y/noStaticElementInteractions: the band is window chrome with no accessible semantics — an empty 28px strip whose only affordance is dragging the window with a pointer, which has no keyboard or AT analogue (the OS moves windows through its own Window menu).
              <div
                data-tauri-drag-region
                onMouseDown={handleBandMouseDown}
                className={cn(
                  "h-full shrink-0 bg-sidebar",
                  sidebarCollapsed ? SIDEBAR_WIDTH_CLASS.collapsed : SIDEBAR_WIDTH_CLASS.expanded,
                )}
              />
            )}
            {/* biome-ignore lint/a11y/noStaticElementInteractions: same as the drawer
                column above — pointer-only window chrome, no accessible semantics. */}
            <div
              data-tauri-drag-region
              onMouseDown={handleBandMouseDown}
              className="h-full flex-1 bg-background"
            />
          </div>
        )}
        <VerifyBanner />
        <div className="flex min-h-0 min-w-0 flex-1">
          {phone ? (
            // The single-pane stack replaces the sidebar + panes row (Story
            // 13.1) — on a reduced-capability platform at every width, and
            // elsewhere below 768px (AD-189); the global overlays/dialogs/
            // shortcut hooks below stay mounted in both arrangements.
            <PhoneShell />
          ) : (
            <>
              {/* `onToggleFold` is `null` where the viewport has already made
                  the decision: below 1080px there is no room to unfold into, so
                  the control is absent rather than present-and-inert. A
                  disabled button whose only answer is "your window is too
                  narrow" is a worse answer than no button. */}
              <SidebarPane collapsed={sidebarCollapsed} onToggleFold={narrow ? null : toggleFold} />
              {recording && primaryView === "recording" ? (
                <RecordingPane />
              ) : recording && primaryView === "recordings" ? (
                // Story 42.3: the browser over what capture produced, gated on
                // the SAME `recording` capability as capture itself — a stale
                // "recordings" primary-view can never show a browser for
                // recordings this build cannot make.
                <RecordingsPane />
              ) : sync && primaryView === "sync" ? (
                <SyncPane />
              ) : sync && primaryView === "files" ? (
                // Story 43.8: the browser over what sync holds, gated on the
                // SAME `sync` capability as sync itself — where no folder can be
                // synced there is nothing for a file browser to browse.
                //
                // Story 45.1 puts the panel strip beside it: the tree is the
                // browser and the strip is the document area, which is what
                // turns "the Files pane lists a PDF it will not open" into a
                // click. The tree already carried its own right-hand border for
                // a neighbour it did not have.
                <>
                  <FilesPane />
                  <PanelStrip />
                </>
              ) : notes && primaryView === "notes" ? (
                <NotesPane />
              ) : sessions && primaryView === "sessions" ? (
                // Phase 7: the board over the sessions zones sync holds, gated
                // on the same construction as notes — a stale "sessions"
                // primary-view can never show a board this build cannot fill.
                //
                // The panel strip sits beside it exactly as it does beside
                // Files (Story 45.1): the board is the browser and the strip
                // is the document area, so opening a session's README is a
                // click into the same editor every other surface uses
                // (AD-109, UX-DR91).
                <>
                  <SessionsPane />
                  <PanelStrip />
                </>
              ) : sessions && primaryView === "tasks" ? (
                // Epic 57: the schedules, and which host runs each of them.
                // Gated on the same construction as notes and sessions, so a
                // stale "tasks" primary-view can never render a pane over a
                // record this build has no database for (AD-137).
                //
                // # A retired refusal (Story 59.12)
                //
                // This comment said "no panel strip: a task is not a document,
                // and there is nothing here to open in an editor", and it went
                // on to concede that the refusal "is scope rather than teeth" —
                // it refused `PanelStrip`, whose targets were files opened in
                // the editor, and it did not refuse detail. Story 59.1 kept it
                // and built the pane's own master/detail inside itself instead.
                //
                // The owner has now asked for the declined thing in as many
                // words: he could not open a task in a tab. So the refusal is
                // retired rather than deleted, the way this epic's own refusal
                // of multi-select was overturned at 59.4 — a refusal whose
                // reason has expired is worth keeping on the page, because the
                // next reader is owed the difference between "nobody thought of
                // it" and "it was declined, and then this changed".
                //
                // What makes retiring it safe, in three facts a reader can
                // check. The target is an identity and nothing else — a task id
                // — carrying no cached facts at all, so a panel resolves it
                // against `sync_tasks` and says *this is no longer here* rather
                // than drawing a task the record has lost
                // (`keeper_core::panels`). It resolves on mount, on unfold and
                // when the target changes, and not on a timer: `useTaskResolution`
                // states that boundary out loud, because a panel nobody has
                // touched holds the facts of its last read. The panel draws the
                // pane's OWN `TaskDetail`, so there is one rendering of a task
                // and not two that could word it differently. And the pane
                // remains the only writer: the panel is handed `verbs={null}`,
                // because `formSaving`, `deleting` and `running` are pane-wide
                // precisely so that two write surfaces cannot undo each other.
                //
                // # What retiring it cost, and the correction (Story 59.13)
                //
                // The strip above was mounted unconditionally, and an EMPTY
                // strip is still a claimant: `grow shrink basis-[280px]
                // min-w-[280px]`. Measured in a real browser at three widths, it
                // took 628 of 1024, 702 of 1280 and 837 of 1550 to render one
                // sentence advertising a gesture — and the Tasks pane's detail
                // region, which had no floor of its own, was left 28, 102 and
                // 237px. The add form draws in that region, so at 1024 it was
                // 0px wide with its controls at 22px, and the owner could not
                // create a task. Story 59.12 was verified with a DOM probe that
                // never measured a box, and jsdom lays nothing out, so nothing
                // caught it.
                //
                // Being a claimant is RIGHT for the Files, Sessions and Notes
                // surfaces: there the strip IS the document area, and an empty
                // one advertises the only route to a document at all. It is
                // wrong here alone, because this surface already has a document
                // area — Story 59.1's detail region — so an empty strip beside
                // it is a second host for a rendering that has one, claiming the
                // window to say so. Hence the condition rather than a change to
                // `PanelStrip`: in this view the strip is mounted only while
                // some panel holds a target. The gesture is advertised by
                // `TASKS_OPEN_BESIDE_HINT` under the drawn task instead, where a
                // reader can act on it before ever having opened a panel.
                <>
                  <TasksPane />
                  {anyPanelHolds && <PanelStrip emptySentence={TASKS_PANEL_EMPTY_SENTENCE} />}
                </>
              ) : bots && primaryView === "bots" ? (
                // Epic 61: the conversation surface. No `PanelStrip` beside it,
                // and this is a refusal with a reason rather than scope: the
                // pane already has its own document area — the conversation —
                // and an empty strip is a claimant (`grow shrink basis-[280px]
                // min-w-[280px]`) that would take a third of the window to
                // advertise a gesture for opening files this surface does not
                // list. Story 59.13 measured exactly that cost on the Tasks
                // pane, where it left the add form 0px wide at 1024. If a
                // later story gives a reply a file to open beside it, this is
                // the line that changes.
                <BotsPane />
              ) : primaryView === "bridges" ? (
                <BridgesPane />
              ) : primaryView === "approval" ? (
                <ApprovalPane />
              ) : primaryView === "settings" ? (
                <SettingsPane />
              ) : (
                <>
                  <ChatListPane />
                  <ConversationPane
                    detailOpen={detailOpen}
                    onToggleDetail={toggleDetail}
                    toggleRef={toggleRef}
                  />
                  {detailOpen && !detailFloating && <DetailPanel />}
                </>
              )}
            </>
          )}
        </div>
      </div>

      <DeviceVerificationDialog />
      <KeyBackupDialog />
      <SearchOverlay />
      <ExportDialog />
      <NewChatDialog />
      <CommandPalette />
      {nativeMenuBar && <CheatSheetOverlay />}

      {detailFloating && !phone && (
        <Sheet open={detailOpen} onOpenChange={handleSheetOpenChange}>
          <SheetContent side="right" className="w-[320px] p-0 sm:max-w-[320px]">
            <SheetHeader className="sr-only">
              <SheetTitle>Details</SheetTitle>
            </SheetHeader>
            <DetailPanel floating />
          </SheetContent>
        </Sheet>
      )}
    </TooltipProvider>
  );
}
