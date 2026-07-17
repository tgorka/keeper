/**
 * Shared segmentation controls (Story 17.5, FR-72): the segment-size stepper
 * (MB) and the duration-cap fallback field (minutes).
 *
 * Rendered by BOTH settings surfaces — Settings → Recording and the pre-record
 * "Segmenting" setup card — and bound to the one `recording-settings` store, so
 * editing either surface writes the same value and both reflect it live. Each
 * field keeps a local draft while focused, then clamps and persists on blur
 * (Rust clamps again defensively and echoes the effective VM back into the
 * store, so the displayed value is never an unsaved one). Edits apply to the
 * next Recording Session only — a running session is never mutated.
 */
import { useEffect, useId, useState } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  applyRecordingSettings,
  ensureRecordingSettingsHydrated,
  RECORDING_DURATION_CAP_MINUTES_MAX,
  RECORDING_DURATION_CAP_MINUTES_MIN,
  RECORDING_SEGMENT_MB_MAX,
  RECORDING_SEGMENT_MB_MIN,
  recordingSettingsStore,
  useRecordingSettings,
} from "@/lib/stores/recording-settings";

/** Segment-size field label (recording voice: sentence case). */
export const SEGMENT_SIZE_LABEL = "Segment size (MB)";

/** Duration-cap field label (recording voice: sentence case). */
export const DURATION_CAP_LABEL = "Duration cap (minutes)";

/** Honest scope note: edits never mutate a running session (glossary caps). */
export const NEXT_SESSION_NOTE = "Applies to the next Recording Session.";

/** Clamp a committed value into the authored bounds (Rust clamps again). */
function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function RecordingSettingsControls() {
  const settings = useRecordingSettings();
  // Lazy shared hydration: whichever surface mounts first triggers the one
  // read; the other (and any remount) reuses the mirrored value.
  useEffect(() => {
    void ensureRecordingSettingsHydrated();
  }, []);

  // Per-field focus drafts so typing is never clamped mid-edit; `null` means
  // "not editing — render the store value" (which is how a write from the
  // other surface shows up here live).
  const [segmentDraft, setSegmentDraft] = useState<string | null>(null);
  const [durationDraft, setDurationDraft] = useState<string | null>(null);
  const segmentFieldId = useId();
  const durationFieldId = useId();

  /** Commit one field on blur: parse, clamp, persist via the shared store. */
  const commit = (
    field: "segment" | "duration",
    draft: string | null,
    clearDraft: () => void,
    min: number,
    max: number,
  ) => {
    clearDraft();
    // Read the *live* store value (not the closed-over render snapshot) so a
    // single-field commit never clobbers the co-field with a stale value when
    // the sibling surface edited it concurrently.
    const live = recordingSettingsStore.getState().settings;
    if (draft === null || live === null) {
      return;
    }
    const trimmed = draft.trim();
    if (trimmed === "") {
      // An empty/non-numeric entry is discarded — the field falls back to the store value.
      return;
    }
    const parsed = Number(trimmed);
    if (!Number.isFinite(parsed)) {
      // Reject partial-numeric junk ("500abc", "0x10") rather than truncating it.
      return;
    }
    const clamped = clamp(Math.round(parsed), min, max);
    const current = field === "segment" ? live.segmentMb : live.durationCapMinutes;
    if (clamped === current) {
      return;
    }
    const next =
      field === "segment"
        ? { segmentMb: clamped, durationCapMinutes: live.durationCapMinutes }
        : { segmentMb: live.segmentMb, durationCapMinutes: clamped };
    void applyRecordingSettings(next);
  };

  return (
    <div className="flex flex-col gap-2 text-sm">
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={segmentFieldId}>{SEGMENT_SIZE_LABEL}</Label>
        <Input
          id={segmentFieldId}
          type="number"
          min={RECORDING_SEGMENT_MB_MIN}
          max={RECORDING_SEGMENT_MB_MAX}
          className="w-24"
          value={segmentDraft ?? settings?.segmentMb ?? ""}
          disabled={settings === null}
          onChange={(e) => setSegmentDraft(e.target.value)}
          onBlur={() =>
            commit(
              "segment",
              segmentDraft,
              () => setSegmentDraft(null),
              RECORDING_SEGMENT_MB_MIN,
              RECORDING_SEGMENT_MB_MAX,
            )
          }
        />
      </div>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={durationFieldId}>{DURATION_CAP_LABEL}</Label>
        <Input
          id={durationFieldId}
          type="number"
          min={RECORDING_DURATION_CAP_MINUTES_MIN}
          max={RECORDING_DURATION_CAP_MINUTES_MAX}
          className="w-24"
          value={durationDraft ?? settings?.durationCapMinutes ?? ""}
          disabled={settings === null}
          onChange={(e) => setDurationDraft(e.target.value)}
          onBlur={() =>
            commit(
              "duration",
              durationDraft,
              () => setDurationDraft(null),
              RECORDING_DURATION_CAP_MINUTES_MIN,
              RECORDING_DURATION_CAP_MINUTES_MAX,
            )
          }
        />
      </div>
      <p className="text-muted-foreground">{NEXT_SESSION_NOTE}</p>
    </div>
  );
}
