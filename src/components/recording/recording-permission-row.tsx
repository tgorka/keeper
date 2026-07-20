/**
 * The honest permission pre-flight row (Story 16.5, FR-67, UX-DR33, AD-36;
 * generalized for the Microphone/Camera legs in Story 20.2).
 *
 * One row per required permission — Screen Recording always, Microphone and
 * Camera only while that source is enabled: permission name + a live status
 * pill + a right-aligned action for the not-green states — "Request
 * permission" while the OS prompt is still available, the System Settings deep
 * link once it is not — plus honest note-lines stating the quirks/framing
 * plainly. Recording voice: sentence case, no exclamation marks.
 *
 * Deliberately no `recording-red` anywhere — that token is reserved for the
 * live record dot (16.6); these pills use the existing neutral / healthy /
 * destructive tokens.
 */
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { ScreenRecordingAccess } from "@/lib/ipc/client";

/** The Screen Recording permission's System Settings name. */
export const SCREEN_RECORDING_PERMISSION_NAME = "Screen Recording";

/** The Microphone permission's System Settings name (Story 20.2). */
export const MICROPHONE_PERMISSION_NAME = "Microphone";

/** The Camera permission's System Settings name (Story 20.2). */
export const CAMERA_PERMISSION_NAME = "Camera";

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

/** Honest quirk note-lines for the Screen Recording row (UX-DR33). */
export const RELAUNCH_NOTE =
  "macOS may require relaunching keeper after granting Screen Recording.";
export const MONTHLY_RECONFIRM_NOTE =
  "On macOS 15 and later, the system may ask you to re-confirm this permission monthly.";
/** The subtle dev-facing caveat (a muted line, not a warning banner). */
export const DEV_BUILD_NOTE =
  "Ad-hoc dev builds may be blocked on macOS 15 and later — sign with an Apple Development " +
  "certificate.";

/** The Screen Recording row's note-lines, in render order. */
export const SCREEN_RECORDING_NOTES: readonly string[] = [
  RELAUNCH_NOTE,
  MONTHLY_RECONFIRM_NOTE,
  DEV_BUILD_NOTE,
];

/** The Microphone row's honest note-line (Story 20.2): needed only while the
 * mic source is on; local-only separate-track framing. */
export const MICROPHONE_ROW_NOTE =
  "Needed only while the microphone source is on. Your voice records locally as its own " +
  "separate track.";

/** The Camera row's honest note-line (Story 20.2): needed only while the
 * webcam is on; local-only separate-file framing. */
export const CAMERA_ROW_NOTE =
  "Needed only while the webcam is on. The camera records locally to its own separate file.";

/** The row container's test id (the names are stable UI copy). */
export function permissionRowTestId(name: string): string {
  return `recording-permission-row-${name}`;
}

interface RecordingPermissionRowProps {
  /** The permission's System Settings name (the row's label). */
  name: string;
  /** The live-detected tri-state for this permission. */
  access: ScreenRecordingAccess;
  /** Honest note-lines rendered under the row, in order (recording voice). */
  notes?: readonly string[];
  /** Trigger the OS request (one real prompt per app lifetime where allowed). */
  onRequest: () => void;
  /** Deep-link to this permission's pane in System Settings. */
  onOpenSettings: () => void;
}

export function RecordingPermissionRow({
  name,
  access,
  notes = [],
  onRequest,
  onOpenSettings,
}: RecordingPermissionRowProps) {
  return (
    <div className="flex flex-col gap-3" data-testid={permissionRowTestId(name)}>
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate font-medium text-sm">{name}</span>
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
      {notes.length > 0 && (
        <div className="flex flex-col gap-1">
          {notes.map((note) => (
            <p key={note} className="text-muted-foreground text-xs">
              {note}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}
