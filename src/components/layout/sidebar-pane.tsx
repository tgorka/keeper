import {
  Archive,
  Bot,
  Cable,
  CalendarClock,
  Film,
  FlaskConical,
  FolderSync,
  FolderTree,
  MessageSquare,
  MonitorDot,
  NotebookPen,
  Settings,
  Stamp,
  WifiOff,
} from "lucide-react";
import { AccountFooter } from "@/components/layout/account-footer";
import {
  FOLD_STRIP,
  FOLD_STRIP_SLOT,
  FOLD_STRIP_TITLE_SLOT,
  FoldStripHead,
  FoldStripName,
} from "@/components/layout/fold-strip";
import { NetworksGroup } from "@/components/layout/networks-group";
import { SpacesGroup } from "@/components/layout/spaces-group";
import { Button } from "@/components/ui/button";
import { Lamp } from "@/components/ui/lamp";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { BRIDGE_HEALTH_LABEL, BRIDGE_HEALTH_LAMP } from "@/lib/bridges";
import { useShellOffline } from "@/lib/stores/account-status";
import { useWorstBridgeHealth } from "@/lib/stores/bridge-health";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { usePendingDraftCount } from "@/lib/stores/drafts";
import { type PrimaryView, primaryViewStore, usePrimaryView } from "@/lib/stores/primary-view";
import { cn } from "@/lib/utils";

interface SidebarView {
  label: string;
  icon: typeof MessageSquare;
  /** The primary view this entry switches to.
   *
   * Carried on the entry rather than derived from `label` at the call site: the
   * label-to-view mapping used to be a ten-deep nested ternary duplicated once
   * for the click handler and once for `aria-current`, which meant every new
   * surface had to be spelled in three places and a miss produced a nav row
   * that highlighted the wrong entry. One field, one lookup, no ladder. */
  view: PrimaryView;
}

/** The always-present nav entries, in order. The capability-gated Recording entry
 * (Story 16.3), Sync + Files entries (Story 32.5, Story 43.8) and Notes entry
 * (Story 37.1) are spliced in before Settings only when their capability is on —
 * never a dead button on a platform that cannot record (AD-27), a machine with
 * no usable `git` (AD-41), or a build with no folder sync to hold a vault
 * (FR-122). */
const BASE_VIEWS: SidebarView[] = [
  { label: "Chats", icon: MessageSquare, view: "inbox" },
  { label: "Archive", icon: Archive, view: "archive" },
  // `Stamp`, not `Inbox`. Two things were wrong with the tray. Every other
  // entry here names its CONTENT — messages, an archive, files, notes — while
  // a tray names the box they arrive in, and approving is an act of consent
  // rather than a container. And `inbox` is the view id of *Chats* two lines
  // up, so the glyph and the route name were pointing at different rows.
  { label: "Approvals", icon: Stamp, view: "approval" },
  // `Cable`, not `Radio`. `Radio` was drawn twice in one window — here and on
  // the NETWORKS group header — and one glyph standing for two concepts stands
  // for neither. The header keeps it: arcs radiating from a point are a
  // network's signal, and they survive the 14px that header draws them at.
  // Bridges takes the connector, because a bridge is a link between two
  // systems and up-or-down on that link is exactly what this row's health
  // lamp reports.
  { label: "Bridges", icon: Cable, view: "bridges" },
];

/** The capability-gated Recording nav entry (Story 16.3).
 *
 * `MonitorDot`, not `Video`. A camcorder in a messenger reads "video call", it
 * says nothing about the SCREEN this feature records, and drawn next to
 * Recordings' `Film` it was a rounded rectangle beside a rounded rectangle —
 * a pair told apart at 16px by counting sprocket holes. The two now differ by
 * KIND rather than by detail: a display in the act of capturing (the only
 * glyph on this rail with a stand, plus the universal record dot) against the
 * strip the captures end up on. It is also the same family as the `Monitor`
 * the recording source picker draws for a whole-screen target. */
const RECORDING_VIEW: SidebarView = { label: "Recording", icon: MonitorDot, view: "recording" };

/** The capability-gated Recordings browser entry (Story 42.3), sitting directly
 * after the capture surface it browses the output of and gated on the SAME
 * `recording` flag: a browser for recordings this build cannot make is a puzzle,
 * so it is absent rather than empty. Two entries because the epic calls it a
 * browser, and a browser buried under the capture settings is one nobody opens. */
const RECORDINGS_VIEW: SidebarView = { label: "Recordings", icon: Film, view: "recordings" };

/** The capability-gated Sync nav entry (Story 32.5, AD-S1). */
const SYNC_VIEW: SidebarView = { label: "Sync", icon: FolderSync, view: "sync" };

/** The capability-gated Files nav entry (Story 43.8, FR-153), sitting directly
 * after the Sync entry it browses the folders of and gated on the SAME `sync`
 * flag: where folder sync cannot run there is no synced folder to browse, so
 * the entry is absent rather than empty. Two entries because Sync answers "is
 * this folder working" and Files answers "what is in it", and a browser folded
 * into a diagnostics pane is a browser nobody finds. */
const FILES_VIEW: SidebarView = { label: "Files", icon: FolderTree, view: "files" };

/** The capability-gated Notes nav entry (Story 37.1, FR-122). Absent — not
 * disabled — where the capability is off: the iOS shell and any desktop build
 * without folder sync render no notes surface at all, because a greyed row that
 * answers "unsupported on this platform" is a worse answer than no row. */
const NOTES_VIEW: SidebarView = { label: "Notes", icon: NotebookPen, view: "notes" };

/** The capability-gated Sessions nav entry (Phase 7, FR-223, FR-251). Beside
 * Notes and gated the same way, because it is the same construction — a
 * sessions root is a folder keeper already syncs plus a flag (AD-107) — and a
 * user who knows one gate already knows the other (UX-DR92). */
const SESSIONS_VIEW: SidebarView = { label: "Sessions", icon: FlaskConical, view: "sessions" };

/** The capability-gated Tasks nav entry (Epic 57, FR-351, FR-352, AD-137).
 * Beside Sessions and gated on the same fact, because it is the same substrate:
 * a task record lives in the `sync.db` folder sync opens, and iOS is not a task
 * host at all. Absent rather than disabled, the rule every entry above it
 * follows.
 *
 * Before Settings, which is where the epic's complaint points: the owner could
 * not see schedules anywhere in the app, and this row plus the palette
 * registry's `Tasks` category are the two places that answer him. */
const TASKS_VIEW: SidebarView = { label: "Tasks", icon: CalendarClock, view: "tasks" };

/** The capability-gated Bots nav entry (Epic 61, FR-378).
 *
 * `Bot`, not `Sparkles` or `Brain`. The lucide sparkle is the industry's
 * "magic AI" decoration and this app does not decorate a surface with a claim
 * about it; a brain names the thing on the other side of the wire rather than
 * what this row opens, which is a conversation. `Bot` is also the word the
 * whole feature is spelled with — the sidebar, the command names and the
 * tables all say `bots` — so nobody has to translate between them.
 *
 * Last before Settings, and gated on `bots` rather than on `sessions`: chat
 * needs neither `git` nor `sync.db`, so this is the first entry on this rail
 * that can be present on a machine with no folder sync at all. Absent — not
 * disabled — where the capability is off, the rule every entry above it
 * follows. */
const BOTS_VIEW: SidebarView = { label: "Bots", icon: Bot, view: "bots" };

/** Settings sits last, after every primary-view entry. */
const SETTINGS_VIEW: SidebarView = { label: "Settings", icon: Settings, view: "settings" };

interface SidebarPaneProps {
  collapsed: boolean;
  /**
   * Fold or unfold the whole menu, or `null` where the viewport has already
   * decided.
   *
   * A nullable callback rather than a `foldable` boolean beside a handler: two
   * fields that must agree is a state where "foldable and no handler" compiles,
   * and the symptom is a button that does nothing.
   */
  onToggleFold: (() => void) | null;
}

/** Exact offline-pill copy (UX-DR18) — kept verbatim. Exported so the phone
 * pull-to-refresh (Story 13.6) resolves its spinner into the same pill copy. */
export const OFFLINE_PILL_TEXT =
  "Offline — showing your local archive. Messages queue until you're back.";

/**
 * What the drawer calls itself (Story 48.3).
 *
 * The other three foldable surfaces had a name in {@link SURFACE_COLUMNS} and
 * this one had none at all — its only self-description was the verb on its own
 * fold control, "Expand menu". So the word the control already says is the word
 * the drawer now shows, which is also what keeps the two from disagreeing: the
 * visible label is contained in the control's accessible name (WCAG 2.5.3).
 */
export const SIDEBAR_TITLE = "Menu";

/** The drawer's width, per state. Exported so the drag band's drawer column
 * (`app-shell.tsx`) is painted at exactly the drawer's width: the band and the
 * drawer sit edge to edge, and a desync between them is the visible seam
 * AD-34-3 exists to prevent.
 *
 * Collapsed is {@link FOLD_STRIP.widthClass} and not a literal of its own: this
 * used to be `w-12` here and `48` in `surface-column.tsx`, each with a comment
 * pointing at the other. */
/**
 * The drawer's two widths.
 *
 * `expanded` was 260px, which was roughly twice what the drawer holds. It went
 * to 130px on a measurement of the navigation rows — the widest is
 * "Recordings", which wants 117px — and 130px was wrong, because the navigation
 * rows are not the tightest thing in here.
 *
 * The tightest is "Add account" in the footer, which sits inside a button with
 * its own horizontal padding rather than in the navigation's inset. Measured
 * against the built stylesheet, space left after the label before the button's
 * own padding:
 *
 *   130px   Add account  0px    Recordings   6px
 *   152px   Add account  4px    Recordings  28px
 *   156px   Add account  8px    Recordings  32px
 *
 * At 130px the footer label ended exactly on its padding — nothing clipped, and
 * nothing between the word and the edge either, which is what reads as a row
 * pushed against one side. 156px gives it 8px, which is the inset the drawer
 * uses everywhere else, so the label sits in the same rhythm as everything
 * above it.
 *
 * The account handle in the footer still truncates, which is a truncation and
 * not a clip: it had an ellipsis at 260px too, and the synced glyph that used
 * to sit beside it is gone (`SyncGlyph`).
 */
export const SIDEBAR_WIDTH_CLASS = {
  collapsed: FOLD_STRIP.widthClass,
  expanded: "w-[156px]",
} as const;

/** The id the fold control's `aria-controls` points at.
 *
 * The list of views, not the whole drawer: the footer and the offline pill stay
 * put and stay reachable while the drawer is folded, so pointing this at the
 * `<nav>` would claim the fold hides them. Naming the region the control
 * genuinely opens and closes is the requirement; a `aria-controls` on the nav
 * would be correct and would also make the relationship untestable by name. */
const VIEWS_LIST_ID = "sidebar-views";

/**
 * Where a folded-rail indicator sits on the button it marks.
 *
 * One constant for the Bridges health lamp and the Approvals count dot,
 * because they had drifted to `top-1.5 right-1.5` and `top-1 right-1` — two
 * corners for one idea, on two rows a person sees at once.
 *
 * The offset is arithmetic rather than taste. A folded button is `size-9` with
 * a 1px border, so its padding box is 34px and the 16px glyph centred in it
 * occupies [10,26] on both axes. Both indicators are 6px — the lamp's own
 * size, which the count dot was ignoring at `size-2`. At 1px from the corner
 * one occupies [28,34] x [2,8]: clear of the glyph by 2px in x AND in y, where
 * the shipped offsets overlapped its top-right corner by 3px in both. It also
 * sits wholly inside the button's 7px `rounded-md` corner — 5.83px from that
 * arc's centre against a 7px radius — so it nests in the corner rather than
 * hanging off it.
 */
const RAIL_INDICATOR = "absolute top-px right-px";

export function SidebarPane({ collapsed, onToggleFold }: SidebarPaneProps) {
  const offline = useShellOffline();
  // Controlled state for the Settings dialog (Story 2.6). Only the Settings view
  // button opens it.
  // The active primary view (Story 4.2 / 6.1): "Chats" switches to the Unified
  // Inbox, "Archive" to the Archive window, "Bridges" to the Bridges surface.
  // Reflected as `aria-current` + accent styling.
  const primaryView = usePrimaryView();
  // The sidebar Bridges health roll-up (Story 6.5): the single worst state across
  // every monitored bridge session, rolled up from the Rust-authoritative
  // bridge-health store. `null` when nothing is monitored (no dot).
  const bridgeHealth = useWorstBridgeHealth();
  // The count of chats with a pending draft across all accounts (Story 7.3). Drives
  // the amber "Approvals" count badge — shown only when at least one draft is held.
  const pendingDraftCount = usePendingDraftCount();
  // Screen recording is a desktop-macOS-≥13 capability (Story 16.3): the Recording
  // nav entry (and its ⌘5) is present only when the flag is on, never a dead button.
  const recording = useCapabilitiesStore((s) => s.capabilities.recording);
  // Folder sync needs a usable `git` (Story 32.5, AD-41): the Sync nav entry is
  // present only when the flag is on, for the same reason.
  const sync = useCapabilitiesStore((s) => s.capabilities.sync);
  // A vault is a folder keeper already syncs (AD-54), so notes exists only where
  // folder sync does (Story 37.1, FR-122) — the entry is absent, not disabled.
  const notes = useCapabilitiesStore((s) => s.capabilities.notes);
  // A sessions root is the same construction (AD-107, FR-223) — same gate.
  const sessions = useCapabilitiesStore((s) => s.capabilities.sessions);
  // Bots is the one entry here that does NOT ride the sync gate (Epic 61,
  // FR-378): a conversation needs no `git` and no `sync.db`, so this row can be
  // present on a machine where Sync, Files, Notes, Sessions and Tasks are all
  // absent.
  const bots = useCapabilitiesStore((s) => s.capabilities.bots);
  // Splice the gated entries in before Settings, each only when supported.
  const views: SidebarView[] = [
    ...BASE_VIEWS,
    // The capture surface and the browser over what it produced ride the one
    // `recording` flag together (Story 42.3): where recordings cannot be made
    // neither entry exists.
    ...(recording ? [RECORDING_VIEW, RECORDINGS_VIEW] : []),
    // The folder's diagnostics and the browser over its contents ride the one
    // `sync` flag together (Story 43.8), for the same reason the two recording
    // entries do.
    ...(sync ? [SYNC_VIEW, FILES_VIEW] : []),
    ...(notes ? [NOTES_VIEW] : []),
    ...(sessions ? [SESSIONS_VIEW] : []),
    // The task record lives in the same `sync.db` (AD-137), so it rides the
    // same gate — and it is last before Settings, which is where a person who
    // cannot find their schedules goes looking.
    ...(sessions ? [TASKS_VIEW] : []),
    // Its own flag, for the reason stated where it is read.
    ...(bots ? [BOTS_VIEW] : []),
    SETTINGS_VIEW,
  ];

  // The way back, built once and placed twice: bare while the drawer is open
  // and its title says which drawer this is, tooltipped while it is folded and
  // the tooltip is the only thing that can.
  //
  // Lowercase, because the name is a sentence and {@link SIDEBAR_TITLE} is the
  // word in it. WCAG 2.5.3 asks that the visible label be IN the accessible
  // name, ignoring case, and "Collapse Menu" mid-sentence is a typo.
  const foldName = `${collapsed ? "Expand" : "Collapse"} ${SIDEBAR_TITLE.toLowerCase()}`;
  const FoldGlyph = collapsed ? FOLD_STRIP.unfoldIcon : FOLD_STRIP.foldIcon;
  const foldControl = (
    <Button
      type="button"
      variant="ghost"
      // A head control: the drawer's head is the same 40px pane-header band as
      // every other foldable surface's, and this is the size a control in one
      // is. See `fold-strip.tsx` — the drawer used to spend 36px here and stand
      // 4px taller than the panel header beside it.
      size={FOLD_STRIP.headControlSize}
      aria-label={foldName}
      aria-expanded={!collapsed}
      aria-controls={VIEWS_LIST_ID}
      data-slot="sidebar-fold"
      className="shrink-0"
      onClick={onToggleFold ?? undefined}
    >
      <FoldGlyph aria-hidden="true" />
    </Button>
  );

  return (
    <nav
      // Not the drawer's display name: this landmark is the LIST of views, and
      // the footer and the offline pill below it are outside what the fold
      // hides. {@link SIDEBAR_TITLE} is what the surface calls itself.
      aria-label="Views"
      data-fold-strip={collapsed ? FOLD_STRIP_SLOT : undefined}
      className={cn(
        "flex h-full min-h-0 shrink-0 flex-col border-border border-r bg-sidebar last:border-r-0",
        collapsed ? SIDEBAR_WIDTH_CLASS.collapsed : SIDEBAR_WIDTH_CLASS.expanded,
      )}
    >
      {/* The drawer's name, and the control that folds it (Story 45.20,
          UX-DR81; Story 48.3).

          A real `<button>` in the tab order with an accessible name that says
          which way it goes, plus `aria-expanded` on the region it controls, so
          the folded rail is navigable by keyboard and announced rather than
          being a strip of glyphs. Absent — not disabled — where the viewport
          has already folded the drawer, because there is nothing it could
          honestly do at that width.

          Folded, the way back keeps the tooltip — it is the only thing that
          says what pressing it DOES — and the drawer's own name moves to the
          spine at the foot of the strip (`FoldStripName`), where it costs the
          controls nothing. */}
      {(!collapsed || onToggleFold !== null) && (
        <FoldStripHead className={collapsed ? "justify-center" : undefined}>
          {!collapsed && (
            <h2 data-slot={FOLD_STRIP_TITLE_SLOT} className={FOLD_STRIP.titleClass}>
              {SIDEBAR_TITLE}
            </h2>
          )}
          {onToggleFold !== null &&
            (collapsed ? (
              <Tooltip>
                <TooltipTrigger asChild>{foldControl}</TooltipTrigger>
                <TooltipContent side="right">{foldName}</TooltipContent>
              </Tooltip>
            ) : (
              foldControl
            ))}
        </FoldStripHead>
      )}
      {/* `shrink` and not `flex-1` while folded: the views take the height they
          need and the spine below takes whatever is left, so a drawer with more
          Spaces than fit loses its name rather than its scroll. Open, there is
          no spine and the scroller is the flexible child again. */}
      <ScrollArea className={cn("min-h-0", collapsed ? "shrink" : "flex-1")}>
        {/* The primary views and both data-driven groups scroll as one, so the
            footer below stays reachable however many Spaces or Networks the user
            belongs to (AD-34-4).

            Folded, this wrapper owns the strip's inset and its rhythm for
            EVERYTHING under the head — the views and both groups — because the
            groups are siblings of the list, not children of it. That is what
            was broken: the list was `p-2`/`gap-1` and the groups were
            `px-1`/`gap-0.5`, so the strip changed inset and spacing halfway
            down at the SPACES boundary. One padded, gapped container cannot. */}
        <div
          data-fold-strip-items={collapsed ? "inset" : undefined}
          className={cn(
            "flex flex-col",
            collapsed && FOLD_STRIP.bodyPadClass,
            collapsed && FOLD_STRIP.gapClass,
          )}
        >
          <ul
            id={VIEWS_LIST_ID}
            data-fold-strip-items={collapsed ? "nested" : undefined}
            className={cn(
              "flex flex-col",
              FOLD_STRIP.gapClass,
              collapsed ? "items-center" : FOLD_STRIP.padClass,
            )}
          >
            {views.map((view) => {
              const Icon = view.icon;
              // Every entry switches the primary view — Settings included, since
              // it stopped being a dialog — and reflects it as `aria-current`.
              const target = view.view;
              const onClick = () => primaryViewStore.getState().setView(target);
              const active = primaryView === target;
              // The Bridges entry carries the worst-state health roll-up lamp
              // (Story 6.1): shown only when at least one bridge reports
              // non-null health. The dot it replaces was `aria-hidden` AND
              // hue-only, so the rolled-up health of every bridge reached a
              // screen reader not at all and a dichromat as one of three
              // near-identical tints. Shape carries it on screen; the word is
              // spliced into the row's own name below.
              const showHealthLamp = view.label === "Bridges" && bridgeHealth !== null;
              const healthWord = bridgeHealth === null ? null : BRIDGE_HEALTH_LABEL[bridgeHealth];
              const healthDot =
                showHealthLamp && bridgeHealth !== null ? (
                  <Lamp
                    state={BRIDGE_HEALTH_LAMP[bridgeHealth]}
                    label={null}
                    data-slot="bridge-health-rollup"
                    className="ml-auto"
                  />
                ) : null;
              // The "Approvals" entry carries an amber count badge (Story 7.3): the
              // number of chats with a pending draft, shown only when > 0 ("written,
              // not sent"). Amber (`--held`) marks the badge — nothing else.
              const showApprovalBadge = view.label === "Approvals" && pendingDraftCount > 0;
              const approvalBadge = showApprovalBadge ? (
                <span
                  data-slot="approval-count"
                  aria-hidden="true"
                  className="ml-auto inline-flex min-w-5 shrink-0 items-center justify-center rounded-full bg-held px-1.5 py-0.5 font-medium text-meta text-held-foreground leading-none"
                >
                  {pendingDraftCount}
                </span>
              ) : null;
              // One name, both rail widths. Folded, the button's `aria-label`
              // replaces its contents outright; unfolded, a name built from
              // contents would concatenate "Bridges" and the lamp's word with
              // no separator between them — the accessible-name algorithm
              // trims each text node before joining, so no amount of padding
              // inside the lamp fixes it and the row would announce
              // "BridgesDisconnected". Naming the row here settles both.
              const rowName = [
                view.label,
                showApprovalBadge ? `${pendingDraftCount} pending` : null,
                showHealthLamp ? healthWord : null,
              ]
                .filter((part) => part !== null)
                .join(", ");
              if (collapsed) {
                return (
                  <li key={view.label}>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          type="button"
                          variant="ghost"
                          size={FOLD_STRIP.controlSize}
                          aria-label={rowName}
                          aria-current={active ? "page" : undefined}
                          className={cn("relative", active && "bg-accent text-accent-foreground")}
                          onClick={onClick}
                        >
                          <Icon aria-hidden="true" />
                          {showHealthLamp && bridgeHealth !== null && (
                            <Lamp
                              state={BRIDGE_HEALTH_LAMP[bridgeHealth]}
                              label={null}
                              data-slot="bridge-health-rollup"
                              className={RAIL_INDICATOR}
                            />
                          )}
                          {showApprovalBadge && (
                            <span
                              aria-hidden="true"
                              data-slot="approval-count"
                              className={cn(RAIL_INDICATOR, "size-1.5 rounded-full bg-held")}
                            />
                          )}
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent side="right">
                        {showApprovalBadge
                          ? `${view.label} (${pendingDraftCount})`
                          : showHealthLamp && healthWord !== null
                            ? `${view.label} — ${healthWord}`
                            : view.label}
                      </TooltipContent>
                    </Tooltip>
                  </li>
                );
              }
              return (
                <li key={view.label}>
                  <Button
                    type="button"
                    variant="ghost"
                    aria-label={rowName}
                    aria-current={active ? "page" : undefined}
                    className={cn(
                      "w-full justify-start gap-2",
                      active && "bg-accent text-accent-foreground",
                    )}
                    onClick={onClick}
                  >
                    <Icon aria-hidden="true" />
                    {view.label}
                    {healthDot}
                    {approvalBadge}
                  </Button>
                </li>
              );
            })}
          </ul>
          {/* SPACES group (Story 4.5): a single-select list of the Matrix Spaces
              the user belongs to, filtering the Unified Inbox. Rendered after
              the primary views, before the footer. Hidden entirely when there
              are no Spaces.

              **It renders folded now** (Story 45.20). It used to be suppressed
              on the rail "because it needs labels + names", which is exactly
              the outcome UX-DR81 refuses: folding the menu silently removed a
              whole navigation surface rather than shrinking it, so a person who
              folded the drawer lost their Spaces until they unfolded it again.
              An avatar with an accessible name is a name; a missing row is not. */}
          <SpacesGroup collapsed={collapsed} />
          {/* NETWORKS group (Story 4.6): a single-select list of the distinct
              bridged Networks connected across all accounts, filtering the
              Unified Inbox. Rendered immediately after SPACES. Hidden entirely
              when there are no bridged rooms, and folded rather than dropped on
              the rail, for the reason above. */}
          <NetworksGroup collapsed={collapsed} />
        </div>
      </ScrollArea>
      {/* The drawer's name, down the spine, in the space between the last view
          and the footer — the same treatment every other folded strip in the
          shell wears (`fold-strip.tsx`). */}
      {collapsed && <FoldStripName name={SIDEBAR_TITLE} />}
      {/* Persistent sidebar-footer region (pushed to the bottom with `mt-auto`):
          the offline pill directly ABOVE the account row, both inside the footer
          region. The account row is always mounted while signed in; the pill
          shows only while disconnected. */}
      <div className="mt-auto flex shrink-0 flex-col">
        {/* Persistent offline pill (UX-DR18): shown only while disconnected, using
            the amber `held` tokens. Non-interactive and keyboard-irrelevant;
            `role="status"` announces the connectivity change without a toast. No
            toasts for connectivity, ever. */}
        {offline &&
          (collapsed ? (
            <div
              role="status"
              aria-label={OFFLINE_PILL_TEXT}
              className="flex shrink-0 items-center justify-center border-border border-t bg-held/10 p-3 text-held"
            >
              <WifiOff aria-hidden="true" className="size-5" />
              {/* Real text content in addition to aria-label so the `role="status"`
                  live region is reliably announced by screen readers that read a
                  live region's *content* (not its label) when the rail is
                  collapsed; visually hidden behind the icon. */}
              <span className="sr-only">{OFFLINE_PILL_TEXT}</span>
            </div>
          ) : (
            <div
              role="status"
              className="flex shrink-0 items-start gap-2 border-border border-t bg-held/10 p-3 text-held text-xs"
            >
              <WifiOff aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
              <span>{OFFLINE_PILL_TEXT}</span>
            </div>
          ))}
        <AccountFooter collapsed={collapsed} />
      </div>
    </nav>
  );
}
