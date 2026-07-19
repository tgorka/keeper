/**
 * The live Audio card — the System-audio toggle (Story 19.2, FR-69).
 *
 * A single default-on `Switch` labelled as content-audio ("the audio the
 * recorded content plays"), not a device pick — there is no output-device
 * chooser here. Inline disclosure states plainly that system audio and
 * microphone are separate tracks (never a mix) and that keeper's own
 * notification sounds are excluded from the file. When the toggle is off, an
 * honest line replaces the "separate track" note: the recording will have no
 * content audio (no false "audio recorded" claim while off, no false "no
 * audio" claim while on).
 *
 * The toggle is bound to {@link recording-audio.ts}'s ephemeral store — it is
 * per-session (default on each load, never persisted, never mirrored into
 * Settings → Recording). Mic (19.3) and destination/fps (19.5) are out of
 * scope; this card only ever renders the system-audio leg.
 */
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { setSystemAudioEnabled, useSystemAudioEnabled } from "@/lib/stores/recording-audio";

/** The Switch's label (recording voice: content-audio, not a device). */
export const SYSTEM_AUDIO_LABEL = "System audio";

/** The content-audio caption under the label. */
export const SYSTEM_AUDIO_CAPTION = "The audio the recorded content plays.";

/** The separate-tracks / keeper-excluded disclosure, shown while on. */
export const SYSTEM_AUDIO_DISCLOSURE =
  "System audio and microphone are recorded as separate tracks, never mixed. " +
  "keeper's own notification sounds are excluded.";

/** The honest off-state line — no content audio will be recorded. */
export const SYSTEM_AUDIO_OFF_NOTE =
  "System audio is off. The recording will have no content audio.";

/** Test id for the switch control. */
export const SYSTEM_AUDIO_SWITCH_TESTID = "system-audio-switch";

export function RecordingAudioControls() {
  const enabled = useSystemAudioEnabled();

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-4">
        <div className="flex flex-col gap-0.5">
          <Label htmlFor="system-audio-toggle">{SYSTEM_AUDIO_LABEL}</Label>
          <p className="text-muted-foreground text-xs">{SYSTEM_AUDIO_CAPTION}</p>
        </div>
        <Switch
          id="system-audio-toggle"
          data-testid={SYSTEM_AUDIO_SWITCH_TESTID}
          checked={enabled}
          onCheckedChange={setSystemAudioEnabled}
        />
      </div>
      {enabled ? (
        <p className="text-muted-foreground text-xs">{SYSTEM_AUDIO_DISCLOSURE}</p>
      ) : (
        <p className="text-muted-foreground text-xs">{SYSTEM_AUDIO_OFF_NOTE}</p>
      )}
    </div>
  );
}
