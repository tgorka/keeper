/**
 * The collapsed Advanced group (Story 19.5, Epic 19): the fps control.
 *
 * A HAND-ROLLED disclosure (Button + `useState` + conditional render) — app
 * code, not a shadcn `ui/` component, and no new dependency — collapsed by
 * default so the frame rate stays out of the way. Expanding reveals an fps
 * `Select` offering exactly {30, 60} (30 the default), bound to the shared
 * `recording-settings` mirror store so the setup card and Settings → Recording
 * stay in lockstep. Edits persist immediately and apply to the next Recording
 * Session only — the sidecar reads fps once at Start.
 */
import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  applyRecordingSettings,
  ensureRecordingSettingsHydrated,
  RECORDING_FPS_ALLOWED,
  recordingSettingsStore,
  useRecordingSettings,
} from "@/lib/stores/recording-settings";

/** The disclosure toggle's label (recording voice: sentence case). */
export const ADVANCED_DISCLOSURE_LABEL = "Advanced options";

/** The fps field label (recording voice: sentence case). */
export const FPS_LABEL = "Frame rate (fps)";

/** Honest scope note: edits never mutate a running session (glossary caps). */
export const FPS_NEXT_SESSION_NOTE = "Applies to the next Recording Session.";

/** Test id for the disclosure toggle button. */
export const ADVANCED_TOGGLE_TESTID = "recording-advanced-toggle";

/** Test id for the fps Select trigger. */
export const FPS_SELECT_TESTID = "recording-fps-select";

export function RecordingAdvancedControls() {
  const settings = useRecordingSettings();
  // Collapsed by default on every mount — fps is deliberately tucked away.
  const [expanded, setExpanded] = useState(false);
  // Lazy shared hydration: whichever surface mounts first triggers the one
  // read; the other (and any remount) reuses the mirrored value.
  useEffect(() => {
    void ensureRecordingSettingsHydrated();
  }, []);

  /** Persist a picked frame rate via the shared optimistic-mirror store. */
  const commitFps = (value: string) => {
    // Read the *live* store value (not the closed-over render snapshot) so
    // this commit never clobbers a co-setting edited concurrently on the
    // sibling surface.
    const live = recordingSettingsStore.getState().settings;
    const fps = Number(value);
    if (live === null || !RECORDING_FPS_ALLOWED.includes(fps) || fps === live.fps) {
      return;
    }
    void applyRecordingSettings({ ...live, fps });
  };

  return (
    <div className="flex flex-col gap-2 text-sm">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="w-fit justify-start gap-1 px-1"
        data-testid={ADVANCED_TOGGLE_TESTID}
        aria-expanded={expanded}
        onClick={() => setExpanded((open) => !open)}
      >
        {expanded ? <ChevronDown aria-hidden /> : <ChevronRight aria-hidden />}
        {ADVANCED_DISCLOSURE_LABEL}
      </Button>
      {expanded && (
        <div className="flex flex-col gap-2 pl-1">
          <div className="flex items-center justify-between gap-2">
            <Label id="recording-fps-label">{FPS_LABEL}</Label>
            <Select
              value={settings === null ? undefined : String(settings.fps)}
              onValueChange={commitFps}
              disabled={settings === null}
            >
              <SelectTrigger
                className="w-24"
                data-testid={FPS_SELECT_TESTID}
                aria-labelledby="recording-fps-label"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {RECORDING_FPS_ALLOWED.map((fps) => (
                  <SelectItem key={fps} value={String(fps)}>
                    {fps}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <p className="text-muted-foreground">{FPS_NEXT_SESSION_NOTE}</p>
        </div>
      )}
    </div>
  );
}
