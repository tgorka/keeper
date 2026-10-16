import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { logFailure } from "@/lib/failure";
import { openCameraSettings } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

export interface CapturedPhoto {
  bytes: ArrayBuffer;
  filename: string;
  mime: string;
  size: number;
}

export function photoFilename(): string {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, "0");
  return `photo-${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}.jpg`;
}

type Phase = "starting" | "live" | "capturing" | "denied" | "noCamera" | "failed";
const FAILURES = {
  denied: "keeper can't use the camera. Allow it in System Settings › Privacy & Security › Camera.",
  noCamera: "No camera found.",
  failed: "The camera could not start.",
};

function failureOf(raw: unknown): "denied" | "noCamera" | "failed" {
  const name = typeof raw === "object" && raw !== null && "name" in raw ? raw.name : null;
  if (name === "NotAllowedError" || name === "SecurityError") return "denied";
  if (name === "NotFoundError" || name === "OverconstrainedError") return "noCamera";
  return "failed";
}

/** Mounted only while open: even a late permission answer cannot leave a camera running. */
export function CameraSheet({
  onPhoto,
  onClose,
}: {
  onPhoto: (photo: CapturedPhoto) => void;
  onClose: () => void;
}) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const sessionRef = useRef<{ active: boolean; stop: () => void } | null>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const [deviceId, setDeviceId] = useState("");
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
  const [phase, setPhase] = useState<Phase>("starting");
  const [captureError, setCaptureError] = useState<string | null>(null);

  useEffect(() => {
    let stream: MediaStream | null = null;
    const session = {
      active: true,
      stop: () => {
        for (const track of stream?.getTracks() ?? []) track.stop();
        stream = null;
      },
    };
    sessionRef.current = session;
    setPhase("starting");
    setCaptureError(null);
    const fail = (raw: unknown, kind: "denied" | "noCamera" | "failed") => {
      logFailure("composer.camera")(raw);
      if (!session.active) return;
      session.active = false;
      session.stop();
      setPhase(kind);
    };
    const start = async () => {
      let arrived: MediaStream;
      try {
        arrived = await navigator.mediaDevices.getUserMedia({
          video: { deviceId: deviceId ? { exact: deviceId } : undefined },
          audio: false,
        });
      } catch (raw) {
        fail(raw, failureOf(raw));
        return;
      }
      const video = videoRef.current;
      if (!session.active || !video) {
        for (const track of arrived.getTracks()) track.stop();
        return;
      }
      stream = arrived;
      for (const track of arrived.getTracks()) {
        track.addEventListener("ended", () => fail(new Error("Camera disconnected"), "failed"));
      }
      video.srcObject = arrived;
      await video.play();
      const available = await navigator.mediaDevices.enumerateDevices();
      if (!session.active) return;
      setDevices(available.filter((device) => device.kind === "videoinput"));
      setPhase("live");
    };
    void start().catch((raw: unknown) => fail(raw, "failed"));
    const putAway = () => {
      session.active = false;
      session.stop();
      closeRef.current();
    };
    const onVisibility = () => {
      if (document.visibilityState === "hidden") putAway();
    };
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("pagehide", putAway);
    return () => {
      session.active = false;
      session.stop();
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("pagehide", putAway);
    };
  }, [deviceId]);

  async function capture() {
    const session = sessionRef.current;
    const video = videoRef.current;
    if (phase !== "live" || !session?.active || !video) return;
    setPhase("capturing");
    try {
      const canvas = document.createElement("canvas");
      canvas.width = video.videoWidth;
      canvas.height = video.videoHeight;
      const context = canvas.getContext("2d");
      if (!context || !canvas.width || !canvas.height) throw new Error("No camera frame available");
      context.drawImage(video, 0, 0, canvas.width, canvas.height);
      session.stop();
      const blob = await new Promise<Blob>((resolve, reject) => {
        canvas.toBlob(
          (result) => (result ? resolve(result) : reject(new Error("Photo encoding failed"))),
          "image/jpeg",
          0.9,
        );
      });
      const bytes = await blob.arrayBuffer();
      if (!session.active) return;
      onPhoto({ bytes, filename: photoFilename(), mime: "image/jpeg", size: blob.size });
      onClose();
    } catch (raw) {
      logFailure("composer.camera")(raw);
      session.stop();
      if (!session.active) return;
      setPhase("failed");
      setCaptureError("keeper couldn't capture this photo. Close the camera and try again.");
    }
  }

  const failure = phase === "denied" || phase === "noCamera" || phase === "failed";
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Take photo</DialogTitle>
          <DialogDescription>
            Capture a photo to add to your message. Nothing is sent until you send it.
          </DialogDescription>
        </DialogHeader>
        {devices.length > 0 && (
          <label className="flex min-w-0 flex-col gap-2">
            Camera
            <select
              className="h-9 w-full min-w-0 rounded-md border border-input bg-background px-2"
              value={deviceId}
              disabled={phase === "capturing"}
              onChange={(event) => setDeviceId(event.target.value)}
            >
              <option value="">Default camera</option>
              {devices.map((device, index) => (
                <option key={device.deviceId} value={device.deviceId}>
                  {device.label || `Camera ${index + 1}`}
                </option>
              ))}
            </select>
          </label>
        )}
        <video
          ref={videoRef}
          aria-label="Camera preview"
          muted
          playsInline
          className={cn(
            "aspect-video w-full rounded-md bg-muted object-contain",
            failure && "hidden",
          )}
        />
        {phase === "starting" && <p role="status">Starting camera…</p>}
        {phase === "capturing" && <p role="status">Preparing photo…</p>}
        {(failure || captureError) && (
          <p role="alert">{captureError ?? (failure ? FAILURES[phase] : null)}</p>
        )}
        <DialogFooter>
          {phase === "denied" && (
            <Button
              variant="outline"
              onClick={() =>
                void openCameraSettings().catch((raw: unknown) => {
                  logFailure("composer.camera")(raw);
                  setCaptureError("keeper couldn't open camera settings.");
                })
              }
            >
              Open Camera settings
            </Button>
          )}
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={phase !== "live"} onClick={() => void capture()}>
            Capture
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
