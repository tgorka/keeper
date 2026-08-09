import {
  Archive,
  Film,
  FolderSync,
  FolderTree,
  Inbox,
  MessageSquare,
  NotebookPen,
  Radio,
  Settings,
  Video,
  WifiOff,
} from "lucide-react";
import { AccountFooter } from "@/components/layout/account-footer";
import { NetworksGroup } from "@/components/layout/networks-group";
import { SpacesGroup } from "@/components/layout/spaces-group";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { BridgeHealth } from "@/lib/ipc/client";
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
  { label: "Approvals", icon: Inbox, view: "approval" },
  { label: "Bridges", icon: Radio, view: "bridges" },
];

/** The capability-gated Recording nav entry (Story 16.3). */
const RECORDING_VIEW: SidebarView = { label: "Recording", icon: Video, view: "recording" };

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

/** Settings sits last, after every primary-view entry. */
const SETTINGS_VIEW: SidebarView = { label: "Settings", icon: Settings, view: "settings" };

/** The `--bridge-*` tint class for a rolled-up worst health (Story 6.5). */
const HEALTH_DOT_CLASS: Record<BridgeHealth, string> = {
  healthy: "bg-bridge-healthy",
  degraded: "bg-bridge-degraded",
  disconnected: "bg-bridge-disconnected",
};

interface SidebarPaneProps {
  collapsed: boolean;
}

/** Exact offline-pill copy (UX-DR18) — kept verbatim. Exported so the phone
 * pull-to-refresh (Story 13.6) resolves its spinner into the same pill copy. */
export const OFFLINE_PILL_TEXT =
  "Offline — showing your local archive. Messages queue until you're back.";

/** The drawer's width, per state. Exported so the drag band's drawer column
 * (`app-shell.tsx`) is painted at exactly the drawer's width: the band and the
 * drawer sit edge to edge, and a desync between them is the visible seam
 * AD-34-3 exists to prevent. */
export const SIDEBAR_WIDTH_CLASS = { collapsed: "w-12", expanded: "w-[260px]" } as const;

export function SidebarPane({ collapsed }: SidebarPaneProps) {
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
    SETTINGS_VIEW,
  ];

  return (
    <nav
      aria-label="Views"
      className={cn(
        "flex h-full min-h-0 shrink-0 flex-col border-border border-r bg-sidebar",
        collapsed ? SIDEBAR_WIDTH_CLASS.collapsed : SIDEBAR_WIDTH_CLASS.expanded,
      )}
    >
      <ScrollArea className="min-h-0 flex-1">
        {/* The primary views and both data-driven groups scroll as one, so the
            footer below stays reachable however many Spaces or Networks the user
            belongs to (AD-34-4). */}
        <div className="flex flex-col">
          <ul className={cn("flex flex-col gap-1 p-2", collapsed && "items-center")}>
            {views.map((view) => {
              const Icon = view.icon;
              // Every entry switches the primary view — Settings included, since
              // it stopped being a dialog — and reflects it as `aria-current`.
              const target = view.view;
              const onClick = () => primaryViewStore.getState().setView(target);
              const active = primaryView === target;
              // The Bridges entry carries the worst-state health roll-up dot (Story
              // 6.1): shown only when at least one bridge reports non-null health.
              const healthDot =
                view.label === "Bridges" && bridgeHealth !== null ? (
                  <span
                    aria-hidden="true"
                    data-slot="bridge-health-rollup"
                    className={cn(
                      "ml-auto size-2 shrink-0 rounded-full",
                      HEALTH_DOT_CLASS[bridgeHealth],
                    )}
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
                  className="ml-auto inline-flex min-w-5 shrink-0 items-center justify-center rounded-full bg-held px-1.5 py-0.5 font-medium text-[11px] text-held-foreground leading-none"
                >
                  {pendingDraftCount}
                </span>
              ) : null;
              if (collapsed) {
                return (
                  <li key={view.label}>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          aria-label={
                            showApprovalBadge
                              ? `${view.label}, ${pendingDraftCount} pending`
                              : view.label
                          }
                          aria-current={active ? "page" : undefined}
                          className={cn(
                            "relative focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
                            active && "bg-accent text-accent-foreground",
                          )}
                          onClick={onClick}
                        >
                          <Icon aria-hidden="true" />
                          {healthDot !== null && (
                            <span
                              aria-hidden="true"
                              data-slot="bridge-health-rollup"
                              className={cn(
                                "absolute top-1.5 right-1.5 size-2 rounded-full",
                                bridgeHealth !== null && HEALTH_DOT_CLASS[bridgeHealth],
                              )}
                            />
                          )}
                          {showApprovalBadge && (
                            <span
                              aria-hidden="true"
                              data-slot="approval-count"
                              className="absolute top-1 right-1 size-2 rounded-full bg-held"
                            />
                          )}
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent side="right">
                        {showApprovalBadge ? `${view.label} (${pendingDraftCount})` : view.label}
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
                    aria-current={active ? "page" : undefined}
                    className={cn(
                      "w-full justify-start gap-2 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
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
          {/* SPACES group (Story 4.5): a single-select list of the Matrix Spaces the
              user belongs to, filtering the Unified Inbox. Rendered after the primary
              views, before the footer. Hidden entirely when there are no Spaces, and
              suppressed on the collapsed rail (it needs labels + names). */}
          {!collapsed && <SpacesGroup />}
          {/* NETWORKS group (Story 4.6): a single-select list of the distinct bridged
              Networks connected across all accounts, filtering the Unified Inbox.
              Rendered immediately after SPACES. Hidden entirely when there are no
              bridged rooms, and suppressed on the collapsed rail (needs labels). */}
          {!collapsed && <NetworksGroup />}
        </div>
      </ScrollArea>
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
