/**
 * Shared imperative recording control (Story 20.4, FR-48/FR-50).
 *
 * The single entry the palette verbs ("Start Recording" / "Stop Recording") and
 * the global recording hotkey both route through, so the two reachability
 * surfaces provably behave identically. Start reads the SAME module-level
 * capture stores the Recording view's Start button and the banner's Restart
 * read imperatively (source target, system audio, mic, webcam) — the stores
 * outlive view remounts, so a palette/hotkey start can never silently revert a
 * chosen app/mic/webcam/display to the defaults.
 *
 * Error-safe by design: a failed start surfaces through the existing Story 18.4
 * loud-failure pipeline (Rust marks the session failed → tray error + native
 * notification), so rejections here are swallowed rather than crashing the
 * caller; stop is idempotent in Rust; toggle asks Rust for the authoritative
 * live state first (never a frontend guess).
 */
import { isLiveRecording } from "@/hooks/use-recording-session";
import { recordingStart, recordingStatus, recordingStop } from "@/lib/ipc/client";
import { systemAudioEnabled } from "@/lib/stores/recording-audio";
import { micDeviceId, micEnabled } from "@/lib/stores/recording-mic";
import { selectedRecordingTarget } from "@/lib/stores/recording-source";
import { cameraDeviceId, webcamEnabled } from "@/lib/stores/recording-webcam";

/**
 * Start a recording session with the CURRENT capture selections, read
 * imperatively from the module-level stores at call time (the exact call shape
 * of the Recording view's Start button). Best-effort: a rejected start (e.g. a
 * permission-blocked pre-flight) is logged and swallowed — the 18.4 pipeline
 * has already surfaced it loudly; this caller never crashes.
 */
export async function startRecordingWithCurrentSelections(): Promise<void> {
  try {
    await recordingStart(
      selectedRecordingTarget(),
      systemAudioEnabled(),
      micEnabled(),
      micDeviceId(),
      webcamEnabled(),
      cameraDeviceId(),
    );
  } catch (error) {
    console.warn("recording-control: start failed (surfaced via the loud-failure pipeline)", error);
  }
}

/**
 * Request the graceful stop-and-finalize of the live session. Idempotent — Rust
 * treats a stop with no live session as a no-op, never an error; a transport
 * rejection is logged and swallowed.
 */
export async function stopRecording(): Promise<void> {
  try {
    await recordingStop();
  } catch (error) {
    console.warn("recording-control: stop failed", error);
  }
}

/**
 * Toggle capture from the global recording hotkey (Story 20.4): ask Rust for
 * the authoritative session snapshot, then stop a live session or start one
 * with the current selections. A failed status read (no Tauri host / early
 * boot) is a safe no-op — the toggle must never guess.
 */
export async function toggleRecording(): Promise<void> {
  let live: boolean;
  try {
    live = isLiveRecording(await recordingStatus());
  } catch {
    return;
  }
  if (live) {
    await stopRecording();
  } else {
    await startRecordingWithCurrentSelections();
  }
}
