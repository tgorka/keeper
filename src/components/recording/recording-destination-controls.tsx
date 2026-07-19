/**
 * The Destination folder chooser (Story 19.5, Epic 19).
 *
 * Rendered by BOTH settings surfaces — the pre-record "Destination" setup card
 * and Settings → Recording — and bound to the one `recording-settings` mirror
 * store, so choosing a folder on either surface persists the same value and
 * both reflect it live. The displayed path is the EFFECTIVE folder resolved by
 * Rust (`~/Movies/keeper` by default), so the UI always names a concrete
 * destination. The chooser opens only the OS-native directory picker already
 * used by the export dialog (`@tauri-apps/plugin-dialog`) — local folders
 * only, zero network affordance. Edits apply to the next Recording Session
 * only; the folder is validated at Start time (exists/writable/free space),
 * never here.
 */
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { useEffect } from "react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  applyRecordingSettings,
  ensureRecordingSettingsHydrated,
  recordingSettingsStore,
  useRecordingSettings,
} from "@/lib/stores/recording-settings";

/** Field label (recording voice: sentence case). */
export const DESTINATION_FOLDER_LABEL = "Folder";

/** The chooser affordance's label (recording voice). */
export const CHOOSE_FOLDER_LABEL = "Choose folder";

/** Honest scope note: edits never mutate a running session (glossary caps). */
export const DESTINATION_NEXT_SESSION_NOTE = "Applies to the next Recording Session.";

/** Honest local-only disclosure — recording adds zero network destinations. */
export const DESTINATION_LOCAL_ONLY_NOTE =
  "Recordings save to this folder on this Mac. Nothing uploads.";

/** Test id for the truncated effective-path display. */
export const DESTINATION_PATH_TESTID = "recording-destination-path";

export function RecordingDestinationControls() {
  const settings = useRecordingSettings();
  // Lazy shared hydration: whichever surface mounts first triggers the one
  // read; the other (and any remount) reuses the mirrored value.
  useEffect(() => {
    void ensureRecordingSettingsHydrated();
  }, []);

  /** Open the OS-native directory picker; persist a confirmed selection. */
  const pickFolder = async () => {
    try {
      const selection = await openFolder({ directory: true });
      // Read the *live* store value (not the closed-over render snapshot) so
      // the commit never clobbers a co-setting edited concurrently on the
      // sibling surface while the picker was open.
      const live = recordingSettingsStore.getState().settings;
      if (typeof selection === "string" && live !== null) {
        void applyRecordingSettings({ ...live, destinationDir: selection });
      }
    } catch {
      // Picker cancellation / failure → keep the current folder (no write).
    }
  };

  return (
    <div className="flex flex-col gap-2 text-sm">
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-0.5">
          <Label>{DESTINATION_FOLDER_LABEL}</Label>
          {/* The effective folder is always concrete (Rust resolves the
              default), truncated to the card width with the full path on
              hover; empty only while hydration is still in flight. */}
          <p
            className="truncate font-mono text-muted-foreground text-xs"
            data-testid={DESTINATION_PATH_TESTID}
            title={settings?.destinationDir ?? undefined}
          >
            {settings?.destinationDir ?? ""}
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          disabled={settings === null}
          onClick={() => {
            void pickFolder();
          }}
        >
          {CHOOSE_FOLDER_LABEL}
        </Button>
      </div>
      <p className="text-muted-foreground">{DESTINATION_NEXT_SESSION_NOTE}</p>
      <p className="text-muted-foreground text-xs">{DESTINATION_LOCAL_ONLY_NOTE}</p>
    </div>
  );
}
