/**
 * Which primary views the phone stack can show (Story 66.1, AD-197, AD-27).
 *
 * The desktop frame renders every {@link PrimaryView}; the phone stack renders
 * the two chat-list windows at level 0 and a subset of the rest as full-screen
 * *surfaces* at level 1. This table is the one place that subset is written
 * down, and three readers agree through it:
 *
 * - `PhoneShell` derives its level-1 surface from `primaryViewStore.view`
 *   with {@link phoneSurfaceFor} and renders the pane the value names.
 * - `SidebarPane`, when it is the drawer on the phone tier, keeps only the
 *   rows {@link phoneRoutesView} accepts — so a row the phone cannot land
 *   is absent rather than a tap that does nothing (AD-27). The drawer is the
 *   desktop sidebar verbatim, so the capability splice happens first and this
 *   filter second: a row is on the phone only if the capability is on AND the
 *   stack has a surface for it.
 * - The registry test enumerates the drawer's rows through the real shell and
 *   fails on any that opens no level.
 *
 * Adding a surface (66.3 `files`, 66.4 `notes`) is therefore: one member in
 * {@link PhoneSurface}, one arm in {@link phoneSurfaceFor} naming its
 * capability, and one branch in `PhoneShell`'s surface switch rendering the
 * pane. The drawer row and the test follow on their own.
 */
import type { CapabilitiesVm } from "@/lib/ipc/client";
import type { PrimaryView } from "@/lib/stores/primary-view";

/** The full-screen level-1 surfaces the phone stack renders. */
export type PhoneSurface = "bots" | "settings" | "approval" | "bridges" | "sync";

/**
 * The surface a view opens on the phone, or `null` where the phone has none
 * for it. Capability-gated where the desktop is (`bots`, `sync`): absent,
 * never a dead level, where the build cannot do the thing.
 */
export function phoneSurfaceFor(
  view: PrimaryView,
  capabilities: CapabilitiesVm,
): PhoneSurface | null {
  switch (view) {
    case "bots":
      return capabilities.bots ? "bots" : null;
    case "sync":
      return capabilities.sync ? "sync" : null;
    case "approval":
    case "bridges":
    case "settings":
      return view;
    default:
      return null;
  }
}

/**
 * Whether a drawer row for `view` lands somewhere on the phone: the two chat
 * windows are level 0 (the stack always shows one of them), everything else
 * needs a surface.
 */
export function phoneRoutesView(view: PrimaryView, capabilities: CapabilitiesVm): boolean {
  return view === "inbox" || view === "archive" || phoneSurfaceFor(view, capabilities) !== null;
}
