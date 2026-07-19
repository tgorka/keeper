/**
 * The live Audio card — the System-audio toggle (Story 19.2, FR-69) and the
 * microphone picker (Story 19.3, FR-69, AD-36).
 *
 * The system-audio row is a single default-on `Switch` labelled as
 * content-audio ("the audio the recorded content plays"), not a device pick —
 * there is no output-device chooser here. Inline disclosure states plainly
 * that system audio and microphone are separate tracks (never a mix) and that
 * keeper's own notification sounds are excluded from the file. When the toggle
 * is off, an honest line replaces the "separate track" note: the recording
 * will have no content audio.
 *
 * The mic row (Story 19.3) is a `Switch` (default **off**) plus a device
 * `Select` with "System default input" always the first/default option and
 * each enumerated input device below it; the picker is disabled/greyed with a
 * helper caption while the mic is off. Enabling the Switch is the one trigger
 * for the lazy microphone permission request (`request_microphone_permission`
 * — never requested preemptively, never on render), and the outcome surfaces
 * as an honest inline caption: granted → the voice records as its own track;
 * denied → the mic track will be silent, with the System Settings fix path.
 *
 * Both rows are bound to ephemeral stores ({@link recording-audio.ts},
 * {@link recording-mic.ts}) — per-session, never persisted, never mirrored
 * into Settings → Recording. Destination/fps (19.5) stay out of scope.
 */
import { useRef, useState } from "react";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { requestMicrophonePermission, type TccPermission } from "@/lib/ipc/client";
import { setSystemAudioEnabled, useSystemAudioEnabled } from "@/lib/stores/recording-audio";
import {
  setMicDeviceId,
  setMicEnabled,
  useMicDeviceId,
  useMicEnabled,
} from "@/lib/stores/recording-mic";
import { useRecordingSources } from "@/lib/stores/recording-source";

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

/** The mic Switch's label (Story 19.3). */
export const MIC_LABEL = "Microphone";

/** The separate-track caption under the mic label. */
export const MIC_CAPTION = "Your voice, recorded as its own separate track.";

/** The honest off-state helper (greys the picker) — no voice will be recorded. */
export const MIC_OFF_NOTE = "The microphone is off. The recording will have no voice track.";

/** The device picker's always-first default option. */
export const MIC_DEFAULT_DEVICE_LABEL = "System default input";

/** The honest granted caption after the lazy permission request. */
export const MIC_PERMISSION_GRANTED_NOTE =
  "Microphone access is granted. Your voice records as its own track.";

/** The honest denied caption — the fix path is System Settings (re-prompting
 * is impossible once denied; the rich pre-flight rows are Story 20.2). */
export const MIC_PERMISSION_DENIED_NOTE =
  "Microphone access is denied. The mic track will be silent — allow keeper under " +
  "System Settings → Privacy & Security → Microphone.";

/** Test id for the mic switch control. */
export const MIC_SWITCH_TESTID = "mic-switch";

/** Test id for the mic device Select trigger. */
export const MIC_DEVICE_SELECT_TESTID = "mic-device-select";

/** Sentinel `Select` value for the system default input (`micDeviceId: null`)
 * — Radix Select item values must be non-empty strings, and no real device id
 * collides with this reserved token. */
const MIC_DEFAULT_DEVICE_VALUE = "__default__";

export function RecordingAudioControls() {
  const enabled = useSystemAudioEnabled();
  const micOn = useMicEnabled();
  const deviceId = useMicDeviceId();
  const microphones = useRecordingSources()?.microphones ?? [];
  // The lazy permission outcome (Story 19.3): `null` until the user enables
  // the mic (nothing is probed on render), then the honest tri-state from the
  // one request the enable triggered. Component-local — like the toggle it
  // mirrors, it is ephemeral setup-surface state.
  const [micPermission, setMicPermission] = useState<TccPermission | null>(null);
  // Every toggle bumps this generation counter so a late or out-of-order
  // permission resolution from a superseded enable can never overwrite the
  // caption for the current state (rapid on→off→on fires overlapping requests).
  const micRequestSeq = useRef(0);

  const onMicToggle = (checked: boolean) => {
    setMicEnabled(checked);
    const requestId = micRequestSeq.current + 1;
    micRequestSeq.current = requestId;
    if (checked) {
      // The lazy-permission hinge (FR-69, AD-36): enabling the source is the
      // ONE trigger for the microphone permission request — exactly once per
      // enable, never preemptively, never at Start. A sidecar failure makes
      // no claim either way (no caption) rather than crashing. The generation
      // guard drops any resolution that a newer toggle has already superseded.
      void requestMicrophonePermission()
        .then((status) => {
          if (requestId === micRequestSeq.current) setMicPermission(status);
        })
        .catch(() => {
          if (requestId === micRequestSeq.current) setMicPermission(null);
        });
    } else {
      // Disabling drops any prior outcome; the next enable re-requests fresh.
      setMicPermission(null);
    }
  };

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

      {/* The microphone row (Story 19.3): Switch (default off) + device picker
          ("System default input" first). Off is the honest default — enabling
          is what triggers the one lazy permission request. */}
      <div className="flex items-center justify-between gap-4 border-border border-t pt-4">
        <div className="flex flex-col gap-0.5">
          <Label htmlFor="mic-toggle">{MIC_LABEL}</Label>
          <p className="text-muted-foreground text-xs">{MIC_CAPTION}</p>
        </div>
        <Switch
          id="mic-toggle"
          data-testid={MIC_SWITCH_TESTID}
          checked={micOn}
          onCheckedChange={onMicToggle}
        />
      </div>
      <Select
        value={deviceId ?? MIC_DEFAULT_DEVICE_VALUE}
        onValueChange={(value) => {
          setMicDeviceId(value === MIC_DEFAULT_DEVICE_VALUE ? null : value);
        }}
        disabled={!micOn}
      >
        <SelectTrigger
          className="w-full"
          data-testid={MIC_DEVICE_SELECT_TESTID}
          aria-label="Microphone device"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {/* "System default input" is ALWAYS the first (and default) option;
              the enumerated devices follow. An empty enumeration leaves only
              the default — honest, never an error. */}
          <SelectItem value={MIC_DEFAULT_DEVICE_VALUE}>{MIC_DEFAULT_DEVICE_LABEL}</SelectItem>
          {microphones.map((mic) => (
            <SelectItem key={mic.id} value={mic.id}>
              {mic.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {!micOn && <p className="text-muted-foreground text-xs">{MIC_OFF_NOTE}</p>}
      {micOn && micPermission === "granted" && (
        <p className="text-muted-foreground text-xs" role="status">
          {MIC_PERMISSION_GRANTED_NOTE}
        </p>
      )}
      {micOn && micPermission === "denied" && (
        <p className="text-held text-xs" role="alert">
          {MIC_PERMISSION_DENIED_NOTE}
        </p>
      )}
    </div>
  );
}
