/**
 * The camera half of the one entry field (Epic 83, AD-317).
 *
 * Loaded lazily by {@link SetupLinkField}, so neither the decoder nor its
 * ~950 KB wasm is in the main chunk. It opens the rear-facing camera into a
 * muted inline preview, decodes a frame every ~150 ms in the webview, and
 * hands the first code's text back — the same road a pasted link takes, so
 * `account_setup_resolve` owns the grammar and the refusal sentence. Frames
 * never leave the webview: nothing is stored and nothing goes over IPC.
 *
 * Every track is stopped on every way out: a decode, a failure (including a
 * track that ends under it) and the page being hidden stop them where they
 * happen — closing keeper's window hides it, so nothing unmounts; Cancel and
 * the surface closing unmount the scanner, and unmount stops them. A stream
 * that arrives after the scanner is already gone is stopped the moment it
 * lands.
 */

import { useEffect, useRef, useState } from "react";
import { prepareZXingModule, type ReaderOptions, readBarcodes } from "zxing-wasm/reader";
import wasmUrl from "zxing-wasm/reader/zxing_reader.wasm?url";
import {
  SCAN_DENIED,
  SCAN_FAILED,
  SCAN_HINT,
  SCAN_NO_CAMERA,
  SCAN_OPEN_CAMERA_SETTINGS_LABEL,
  SCAN_STARTING,
  SETUP_CANCEL_LABEL,
  Waiting,
} from "@/components/account/account-setup-sheet";
import { Button } from "@/components/ui/button";
import { openCameraSettings } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

// zxing's default `locateFile` fetches the wasm from the jsDelivr CDN. That
// would be a network destination keeper never discloses (AD-53), reached the
// moment somebody points a camera at a code — so the bundled copy is served
// instead, and nothing about a scan ever leaves this machine.
prepareZXingModule({
  overrides: {
    locateFile: (path: string, prefix: string) =>
      path.endsWith(".wasm") ? wasmUrl : prefix + path,
  },
});

const FRAME_INTERVAL_MS = 150;
/** Long side of the frame handed to the decoder: enough for a dense code, cheap to scan. */
const MAX_FRAME_SIDE = 1280;
const READER_OPTIONS: ReaderOptions = {
  formats: ["QRCode"],
  tryHarder: true,
  maxNumberOfSymbols: 1,
};

type Phase = "starting" | "scanning" | "denied" | "noCamera" | "failed";

const FAILURE_SENTENCE: Record<Exclude<Phase, "starting" | "scanning">, string> = {
  denied: SCAN_DENIED,
  noCamera: SCAN_NO_CAMERA,
  failed: SCAN_FAILED,
};

/** Which failure a `getUserMedia` rejection is, by its DOMException name. */
function failureOf(raw: unknown): "denied" | "noCamera" | "failed" {
  const name = typeof raw === "object" && raw !== null && "name" in raw ? raw.name : null;
  if (name === "NotAllowedError" || name === "SecurityError") {
    return "denied";
  }
  if (name === "NotFoundError" || name === "OverconstrainedError") {
    return "noCamera";
  }
  return "failed";
}

export function SetupCodeScanner({
  onScanned,
  onCancel,
}: {
  /** The first decoded code's text, verbatim. Every track is already stopped. */
  onScanned: (text: string) => void;
  /** Put the scanner away; its parent unmounts it, which stops the camera. */
  onCancel: () => void;
}) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [phase, setPhase] = useState<Phase>("starting");
  // The parent's callbacks, read when they fire so a re-render does not
  // restart the camera.
  const onScannedRef = useRef(onScanned);
  const onCancelRef = useRef(onCancel);
  useEffect(() => {
    onScannedRef.current = onScanned;
    onCancelRef.current = onCancel;
  });

  useEffect(() => {
    let stopped = false;
    let stream: MediaStream | null = null;
    let timer: number | undefined;
    const stop = () => {
      stopped = true;
      window.clearTimeout(timer);
      for (const track of stream?.getTracks() ?? []) {
        track.stop();
      }
      stream = null;
    };
    const fail = (kind: "denied" | "noCamera" | "failed") => {
      if (!stopped) {
        stop();
        setPhase(kind);
      }
    };
    // Closing keeper's window hides it rather than unmounting anything, so
    // "the surface is gone" has to be heard from the page: a hidden page puts
    // the scanner away exactly as Cancel does.
    const putAway = () => {
      stop();
      onCancelRef.current();
    };
    const onVisibility = () => {
      if (document.visibilityState === "hidden") {
        putAway();
      }
    };
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("pagehide", putAway);

    const canvas = document.createElement("canvas");
    const tick = async () => {
      const video = videoRef.current;
      if (stopped || video === null) {
        return;
      }
      // No frame yet (dimensions arrive with the first one): try again shortly.
      if (video.videoWidth > 0 && video.videoHeight > 0) {
        const scale = Math.min(1, MAX_FRAME_SIDE / Math.max(video.videoWidth, video.videoHeight));
        canvas.width = Math.round(video.videoWidth * scale);
        canvas.height = Math.round(video.videoHeight * scale);
        const context = canvas.getContext("2d", { willReadFrequently: true });
        if (context === null) {
          fail("failed");
          return;
        }
        context.drawImage(video, 0, 0, canvas.width, canvas.height);
        const results = await readBarcodes(
          context.getImageData(0, 0, canvas.width, canvas.height),
          READER_OPTIONS,
        );
        if (stopped) {
          return;
        }
        const text = results.find((result) => result.text !== "")?.text;
        if (text !== undefined) {
          stop();
          onScannedRef.current(text);
          return;
        }
      }
      timer = window.setTimeout(() => void tick().catch(() => fail("failed")), FRAME_INTERVAL_MS);
    };

    void navigator.mediaDevices
      .getUserMedia({
        video: { facingMode: "environment", width: { ideal: 1920 }, height: { ideal: 1080 } },
        audio: false,
      })
      .then(
        async (arrived) => {
          const video = videoRef.current;
          if (stopped || video === null) {
            // Gone before the camera answered: this stream belongs to nobody.
            for (const track of arrived.getTracks()) {
              track.stop();
            }
            return;
          }
          stream = arrived;
          // Unplugged, taken by another app, or its grant revoked mid-scan: a
          // frozen preview that keeps saying "hold it up" would be a lie.
          for (const track of arrived.getTracks()) {
            track.addEventListener("ended", () => fail("failed"));
          }
          video.srcObject = arrived;
          await video.play();
          if (!stopped) {
            setPhase("scanning");
            await tick();
          }
        },
        // Only the camera's own refusal says why by its name. Anything later —
        // playback, the decoder, the canvas — is not about the grant, so it is
        // never worded as one.
        (raw: unknown) => fail(failureOf(raw)),
      )
      .catch(() => fail("failed"));

    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("pagehide", putAway);
      stop();
    };
  }, []);

  const failure = phase === "starting" || phase === "scanning" ? null : FAILURE_SENTENCE[phase];

  return (
    <div className="flex flex-col gap-2">
      {phase === "starting" && <Waiting sentence={SCAN_STARTING} />}
      {/* Always in the tree so the stream has somewhere to land; drawn once it plays. */}
      <video
        ref={videoRef}
        aria-label="Camera preview"
        muted
        playsInline
        className={cn(
          "aspect-video w-full rounded-md bg-muted object-cover",
          phase !== "scanning" && "hidden",
        )}
      />
      {phase === "scanning" && <p className="text-muted-foreground">{SCAN_HINT}</p>}
      {failure !== null && <p role="alert">{failure}</p>}
      <div className="flex flex-wrap gap-2">
        {phase === "denied" && (
          <Button
            type="button"
            variant="outline"
            onClick={() => void openCameraSettings().catch(() => {})}
          >
            {SCAN_OPEN_CAMERA_SETTINGS_LABEL}
          </Button>
        )}
        <Button type="button" variant="outline" onClick={onCancel}>
          {SETUP_CANCEL_LABEL}
        </Button>
      </div>
    </div>
  );
}
