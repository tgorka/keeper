/**
 * The honest Screen Recording permission pre-flight row (Story 16.5, FR-67,
 * UX-DR33, AD-36).
 *
 * One row per required permission (Screen Recording alone in this epic):
 * permission name + a live status pill + a right-aligned action for the
 * not-green states — "Request permission" while the OS prompt is still
 * available, the System Settings deep link once it is not — plus honest
 * note-lines stating the macOS quirks plainly (relaunch after grant, the
 * macOS 15+ monthly re-confirm, and the muted dev-facing ad-hoc-signing
 * caveat). Recording voice: sentence case, no exclamation marks.
 *
 * Deliberately no `recording-red` anywhere — that token is reserved for the
 * live record dot (16.6); these pills use the existing neutral / healthy /
 * destructive tokens.
 */
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { ScreenRecordingAccess } from "@/lib/ipc/client";

/** The macOS permission this row pre-flights (its System Settings name). */
export const SCREEN_RECORDING_PERMISSION_NAME = "Screen Recording";

/** The status pill label per tri-state (recording voice). */
export const ACCESS_LABEL: Record<ScreenRecordingAccess, string> = {
  granted: "Granted",
  notYetRequested: "Not requested yet",
  denied: "Denied",
};

/** The request-affordance label (shown while the OS prompt is available). */
export const REQUEST_PERMISSION_LABEL = "Request permission";

/** The denied fix-path label (deep link — re-prompting is impossible). */
export const OPEN_SETTINGS_LABEL = "Open System Settings";

/** Honest quirk note-lines, stated plainly (UX-DR33). */
export const RELAUNCH_NOTE =
  "macOS may require relaunching keeper after granting Screen Recording.";
export const MONTHLY_RECONFIRM_NOTE =
  "On macOS 15 and later, the system may ask you to re-confirm this permission monthly.";
/** The subtle dev-facing caveat (a muted line, not a warning banner). */
export const DEV_BUILD_NOTE =
  "Ad-hoc dev builds may be blocked on macOS 15 and later — sign with an Apple Development " +
  "certificate.";

interface RecordingPermissionRowProps {
  /** The live-detected tri-state for Screen Recording. */
  access: ScreenRecordingAccess;
  /** Trigger the OS request (one real prompt per app lifetime where allowed). */
  onRequest: () => void;
  /** Deep-link to the Screen Recording pane in System Settings. */
  onOpenSettings: () => void;
}

export function RecordingPermissionRow({
  access,
  onRequest,
  onOpenSettings,
}: RecordingPermissionRowProps) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate font-medium text-sm">{SCREEN_RECORDING_PERMISSION_NAME}</span>
          {access === "granted" ? (
            <Badge variant="secondary">
              <span aria-hidden="true" className="size-1.5 rounded-full bg-bridge-healthy" />
              {ACCESS_LABEL.granted}
            </Badge>
          ) : access === "denied" ? (
            <Badge variant="destructive">{ACCESS_LABEL.denied}</Badge>
          ) : (
            <Badge variant="outline">{ACCESS_LABEL.notYetRequested}</Badge>
          )}
        </div>
        {access === "notYetRequested" ? (
          <Button type="button" size="sm" onClick={onRequest}>
            {REQUEST_PERMISSION_LABEL}
          </Button>
        ) : access === "denied" ? (
          <Button type="button" size="sm" variant="outline" onClick={onOpenSettings}>
            {OPEN_SETTINGS_LABEL}
          </Button>
        ) : null}
      </div>
      <div className="flex flex-col gap-1">
        <p className="text-muted-foreground text-xs">{RELAUNCH_NOTE}</p>
        <p className="text-muted-foreground text-xs">{MONTHLY_RECONFIRM_NOTE}</p>
        <p className="text-muted-foreground/70 text-xs">{DEV_BUILD_NOTE}</p>
      </div>
    </div>
  );
}
