/**
 * The collapsed Advanced group (Story 19.5 + 21.1/21.2): fps, codec, and
 * capture-scale controls.
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
import type { RecordingSourcesVm, RecordingTargetVm } from "@/lib/ipc/client";
import {
  applyRecordingSettings,
  ensureRecordingSettingsHydrated,
  RECORDING_CODEC_ALLOWED,
  RECORDING_FPS_ALLOWED,
  RECORDING_SCALE_ALLOWED,
  recordingSettingsStore,
  useRecordingSettings,
} from "@/lib/stores/recording-settings";
import { useRecordingSources, useSelectedRecordingTarget } from "@/lib/stores/recording-source";

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

/** The codec field label (Story 21.1; recording voice). */
export const CODEC_LABEL = "Video codec";

/** Honest per-codec display labels (compatibility vs size trade-off). */
export const CODEC_OPTION_LABELS: Record<string, string> = {
  h264: "H.264 (compatible)",
  hevc: "HEVC (smaller files)",
};

/** Test id for the codec Select trigger. */
export const CODEC_SELECT_TESTID = "recording-codec-select";

/** The capture-scale field label (Story 21.2; recording voice). */
export const SCALE_LABEL = "Capture resolution";

/** Honest per-scale display labels. */
export const SCALE_OPTION_LABELS: Record<number, string> = {
  100: "Full (100%)",
  75: "3/4 (75%)",
  50: "Half (50%)",
  25: "1/4 (25%)",
};

/** Round a scaled dimension DOWN to even pixels — mirrors the sidecar rule. */
function scaledEven(size: number, percent: number): number {
  return Math.max(2, Math.floor((size * percent) / 100) & ~1);
}

/** The live effective-resolution hint (Story 22.1): the selected display's
 * true pixel size → the scaled output size (even-rounded like the encoder).
 * Null when it cannot be honestly computed (audio-only, no enumeration yet,
 * or an older sidecar without pixel dims). App-scoped capture renders on the
 * main display's canvas, so it uses the main display's dims. */
export function effectiveResolutionHint(
  target: RecordingTargetVm,
  sources: RecordingSourcesVm | null,
  scalePercent: number,
): string | null {
  if (target.kind === "audioOnly" || sources === null) {
    return null;
  }
  const display =
    target.kind === "display" && target.displayId !== null
      ? sources.displays.find((d) => d.id === target.displayId)
      : sources.displays.find((d) => d.isMain);
  if (display === undefined || display.pixelWidth === 0 || display.pixelHeight === 0) {
    return null;
  }
  const w = scaledEven(display.pixelWidth, scalePercent);
  const h = scaledEven(display.pixelHeight, scalePercent);
  if (scalePercent === 100) {
    return `${display.pixelWidth}×${display.pixelHeight}`;
  }
  return `${display.pixelWidth}×${display.pixelHeight} → ${w}×${h}`;
}

/** Test id for the scale Select trigger. */
export const SCALE_SELECT_TESTID = "recording-scale-select";

export function RecordingAdvancedControls() {
  const settings = useRecordingSettings();
  // Story 22.1: the live effective-resolution hint tracks the picker's
  // selected target and the enumerated displays' true pixel dims.
  const selectedTarget = useSelectedRecordingTarget();
  const sources = useRecordingSources();
  const resolutionHint =
    settings === null
      ? null
      : effectiveResolutionHint(selectedTarget, sources, settings.scalePercent);
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

  /** Persist a picked codec via the shared optimistic-mirror store. */
  const commitCodec = (value: string) => {
    const live = recordingSettingsStore.getState().settings;
    if (live === null || !RECORDING_CODEC_ALLOWED.includes(value) || value === live.codec) {
      return;
    }
    void applyRecordingSettings({ ...live, codec: value });
  };

  /** Persist a picked capture scale via the shared optimistic-mirror store. */
  const commitScale = (value: string) => {
    const live = recordingSettingsStore.getState().settings;
    const scalePercent = Number(value);
    if (
      live === null ||
      !RECORDING_SCALE_ALLOWED.includes(scalePercent) ||
      scalePercent === live.scalePercent
    ) {
      return;
    }
    void applyRecordingSettings({ ...live, scalePercent });
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
          <div className="flex items-center justify-between gap-2">
            <Label id="recording-codec-label">{CODEC_LABEL}</Label>
            <Select
              value={settings === null ? undefined : settings.codec}
              onValueChange={commitCodec}
              disabled={settings === null}
            >
              <SelectTrigger
                className="w-48"
                data-testid={CODEC_SELECT_TESTID}
                aria-labelledby="recording-codec-label"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {RECORDING_CODEC_ALLOWED.map((codec) => (
                  <SelectItem key={codec} value={codec}>
                    {CODEC_OPTION_LABELS[codec] ?? codec}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center justify-between gap-2">
            <Label id="recording-scale-label">{SCALE_LABEL}</Label>
            <Select
              value={settings === null ? undefined : String(settings.scalePercent)}
              onValueChange={commitScale}
              disabled={settings === null}
            >
              <SelectTrigger
                className="w-40"
                data-testid={SCALE_SELECT_TESTID}
                aria-labelledby="recording-scale-label"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {RECORDING_SCALE_ALLOWED.map((scale) => (
                  <SelectItem key={scale} value={String(scale)}>
                    {SCALE_OPTION_LABELS[scale] ?? `${scale}%`}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {resolutionHint !== null && (
            <p className="text-muted-foreground text-xs" data-testid="recording-resolution-hint">
              Output resolution: <span className="font-mono">{resolutionHint}</span>
            </p>
          )}
          <p className="text-muted-foreground">{FPS_NEXT_SESSION_NOTE}</p>
        </div>
      )}
    </div>
  );
}
