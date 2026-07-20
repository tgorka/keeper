/**
 * The live Webcam card (Story 20.1, FR-70, AD-36/AD-37, UX-DR34).
 *
 * A `Switch` (default **off**) plus a flat camera `Select` with "System
 * default camera" always the first/default option and each enumerated camera
 * below it (built-in / external / Continuity Camera — the `localizedName`
 * already distinguishes them, so no device-class grouping); the picker is
 * disabled/greyed with a helper caption while the webcam is off. Enabling the
 * Switch is the one trigger for the lazy camera permission request
 * (`request_camera_permission` — never requested preemptively, never on
 * render), and the outcome surfaces as an honest inline caption: granted →
 * the camera records to its own file; denied → Start is blocked while the
 * webcam stays enabled (Story 20.2 — the pre-flight row names it), with the
 * System Settings fix path.
 *
 * The copy is honest about the shape: the webcam records to a **separate
 * file, synced to the screen** — no picture-in-picture, no self-view bubble,
 * no burn-in (macOS 14+ can composite the camera via the system presenter
 * overlay — an OS behavior, not a keeper feature; UX-DR34).
 *
 * Bound to the ephemeral {@link recording-webcam.ts} store — per-session,
 * never persisted, never mirrored into Settings → Recording (the mic
 * precedent, Story 19.3).
 */
import { useEffect, useRef, useState } from "react";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { requestCameraPermission, type TccPermission } from "@/lib/ipc/client";
import { useRecordingSources } from "@/lib/stores/recording-source";
import {
  isCameraSelectionAvailable,
  setCameraDeviceId,
  setWebcamEnabled,
  useCameraDeviceId,
  useWebcamEnabled,
} from "@/lib/stores/recording-webcam";

/** The webcam Switch's row label (recording voice) — "Camera" under the
 * "Webcam" card title, mirroring the "Microphone" row under "Audio". */
export const WEBCAM_LABEL = "Camera";

/** The separate-file caption under the webcam label — the FR-70 shape. */
export const WEBCAM_CAPTION = "Your camera, recorded to a separate file, synced to the screen.";

/** The honest off-state helper (greys the picker) — no camera file is written. */
export const WEBCAM_OFF_NOTE = "The webcam is off. The recording will have no camera file.";

/** The no-burn-in / presenter-overlay disclosure, shown while on (UX-DR34). */
export const WEBCAM_DISCLOSURE =
  "The camera is never burned into the screen video — no picture-in-picture. " +
  "On macOS 14 and later, the system presenter overlay can composite your camera; " +
  "that is a macOS behavior, not part of the recording.";

/** The camera picker's always-first default option. */
export const CAMERA_DEFAULT_DEVICE_LABEL = "System default camera";

/** The honest granted caption after the lazy permission request. */
export const CAMERA_PERMISSION_GRANTED_NOTE =
  "Camera access is granted. Your camera records to its own file.";

/** The honest denied caption — an enabled webcam that is not granted blocks
 * Start (Story 20.2; the pre-flight row names it), and the fix path is System
 * Settings (re-prompting is impossible once denied). */
export const CAMERA_PERMISSION_DENIED_NOTE =
  "Camera access is denied. Recording can't start while the webcam is on — allow keeper " +
  "under System Settings → Privacy & Security → Camera, or turn the webcam off.";

/** Test id for the webcam switch control. */
export const WEBCAM_SWITCH_TESTID = "webcam-switch";

/** Test id for the camera device Select trigger. */
export const CAMERA_DEVICE_SELECT_TESTID = "camera-device-select";

/** Sentinel `Select` value for the system default camera (`cameraDeviceId:
 * null`) — Radix Select item values must be non-empty strings, and no real
 * device id collides with this reserved token. */
const CAMERA_DEFAULT_DEVICE_VALUE = "__default__";

export interface RecordingWebcamControlsProps {
  /** Whether the setup surface is interactive (mirrors the Audio card):
   * `false` while a session is live, freezing pre-Start reconciliation so a
   * mounted-but-inactive card never rewrites the user's camera selection. */
  active?: boolean;
  /** Fired once the enable-triggered camera permission request has settled
   * (Story 20.2). The pre-flight surface passes its `refresh` here so the Camera
   * pre-flight row and the Start gate re-probe the live grant the moment the OS
   * prompt is answered — rather than waiting on an incidental window focus/return
   * event. A no-op when omitted. */
  onPermissionSettled?: () => void;
}

export function RecordingWebcamControls({
  active = true,
  onPermissionSettled,
}: RecordingWebcamControlsProps = {}) {
  const webcamOn = useWebcamEnabled();
  const deviceId = useCameraDeviceId();
  const sources = useRecordingSources();
  const cameras = sources?.cameras ?? [];

  // Pre-Start reconciliation (the 19.4 mic pattern): a specifically-selected
  // camera that vanished from the live enumeration falls back to "System
  // default camera" (`null`), so Start never ships a dead device id (the
  // sidecar would fall back to the default anyway — this keeps the UI honest
  // about it). The default/`null` selection is never touched, a never-polled
  // `null` sources list reconciles nothing, and the reset is a store write
  // only — no permission re-request, no Start gating. Gated on `active`: a
  // live-session poll must never silently reset the selection mid-recording.
  useEffect(() => {
    if (active && !isCameraSelectionAvailable(deviceId, sources)) {
      setCameraDeviceId(null);
    }
  }, [active, deviceId, sources]);
  // The lazy permission outcome (Story 20.1): `null` until the user enables
  // the webcam (nothing is probed on render), then the honest tri-state from
  // the one request the enable triggered. Component-local ephemeral state.
  const [cameraPermission, setCameraPermission] = useState<TccPermission | null>(null);
  // Every toggle bumps this generation counter so a late or out-of-order
  // permission resolution from a superseded enable can never overwrite the
  // caption for the current state (rapid on→off→on fires overlapping requests).
  const cameraRequestSeq = useRef(0);

  const onWebcamToggle = (checked: boolean) => {
    setWebcamEnabled(checked);
    const requestId = cameraRequestSeq.current + 1;
    cameraRequestSeq.current = requestId;
    if (checked) {
      // The lazy-permission hinge (FR-70, AD-36): enabling the source is the
      // ONE trigger for the camera permission request — exactly once per
      // enable, never preemptively, never at Start. A sidecar failure makes
      // no claim either way (no caption) rather than crashing. The generation
      // guard drops any resolution that a newer toggle has already superseded.
      void requestCameraPermission()
        .then((status) => {
          if (requestId === cameraRequestSeq.current) setCameraPermission(status);
        })
        .catch(() => {
          if (requestId === cameraRequestSeq.current) setCameraPermission(null);
        })
        .finally(() => {
          // Re-probe the pre-flight now the OS prompt has resolved (Story 20.2):
          // the enable-time probe read `NotDetermined` while the prompt was on
          // screen, so the Camera row + Start would otherwise stay stale until an
          // unrelated focus event. Fired regardless of the generation guard — a
          // live re-probe is idempotent and always reflects the truth.
          onPermissionSettled?.();
        });
    } else {
      // Disabling drops any prior outcome; the next enable re-requests fresh.
      setCameraPermission(null);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-4">
        <div className="flex flex-col gap-0.5">
          <Label htmlFor="webcam-toggle">{WEBCAM_LABEL}</Label>
          <p className="text-muted-foreground text-xs">{WEBCAM_CAPTION}</p>
        </div>
        <Switch
          id="webcam-toggle"
          data-testid={WEBCAM_SWITCH_TESTID}
          checked={webcamOn}
          onCheckedChange={onWebcamToggle}
        />
      </div>
      <Select
        value={deviceId ?? CAMERA_DEFAULT_DEVICE_VALUE}
        onValueChange={(value) => {
          setCameraDeviceId(value === CAMERA_DEFAULT_DEVICE_VALUE ? null : value);
        }}
        disabled={!webcamOn}
      >
        <SelectTrigger
          className="w-full"
          data-testid={CAMERA_DEVICE_SELECT_TESTID}
          aria-label="Camera device"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {/* "System default camera" is ALWAYS the first (and default)
              option; the enumerated cameras follow as a flat name list —
              localizedName already tells built-in / external / Continuity
              Camera apart. An empty enumeration leaves only the default —
              honest, never an error. */}
          <SelectItem value={CAMERA_DEFAULT_DEVICE_VALUE}>{CAMERA_DEFAULT_DEVICE_LABEL}</SelectItem>
          {cameras.map((camera) => (
            <SelectItem key={camera.id} value={camera.id}>
              {camera.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {!webcamOn && <p className="text-muted-foreground text-xs">{WEBCAM_OFF_NOTE}</p>}
      {webcamOn && <p className="text-muted-foreground text-xs">{WEBCAM_DISCLOSURE}</p>}
      {webcamOn && cameraPermission === "granted" && (
        <p className="text-muted-foreground text-xs" role="status">
          {CAMERA_PERMISSION_GRANTED_NOTE}
        </p>
      )}
      {webcamOn && cameraPermission === "denied" && (
        <p className="text-held text-xs" role="alert">
          {CAMERA_PERMISSION_DENIED_NOTE}
        </p>
      )}
    </div>
  );
}
