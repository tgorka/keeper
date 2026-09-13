/**
 * Which capture sources the next Recording Session starts with — the persisted
 * half of the three pre-record switches (spec *Recording remembers which sources
 * are on*).
 *
 * The report this exists for was "every restart of the app these settings are
 * coming back to different defaults": system audio, the microphone and the
 * camera were session-scoped module state, so every relaunch threw the person's
 * choice away. The answer now lives in Rust (`recording.system_audio`,
 * `recording.microphone`, `recording.camera`, all default ON) and this module is
 * the seam:
 *
 * - {@link ensureCaptureSourcesHydrated} reads it once per launch and SEEDS the
 *   three session stores, which stay the surface Start reads imperatively (the
 *   palette verb and the global hotkey sample them through `recording-control`).
 * - {@link persistCaptureSources} writes a toggle back, and takes Rust's
 *   effective answer — so a key a config file pins visibly refuses to move
 *   instead of leaving the switch showing a value the next launch will not
 *   honour.
 *
 * Seeding is a one-shot PER SOURCE. A launch-wide flag would mean that toggling
 * one switch before the read lands throws away the stored answers for the other
 * two; a per-source mark means a late read still seeds the sources nobody has
 * answered yet, and can never move one somebody already set.
 *
 * Deliberately NOT part of the `recording-settings` mirror: that VM is submitted
 * whole, and a whole-VM write settles the DESTINATION and rebuilds the
 * recordings index. A camera toggle means none of that.
 */
import {
  type RecordingCaptureSourcesVm,
  recordingCaptureSourcesGet,
  recordingCaptureSourcesSet,
} from "@/lib/ipc/client";
import { recordingAudioStore } from "@/lib/stores/recording-audio";
import { recordingMicStore } from "@/lib/stores/recording-mic";
import { recordingWebcamStore } from "@/lib/stores/recording-webcam";

/** The three sources, by the name the VM carries them under. */
export type CaptureSource = keyof RecordingCaptureSourcesVm;

/** Every source, in card order — the iteration order of seeding and writing. */
const SOURCES: readonly CaptureSource[] = ["systemAudio", "microphone", "camera"];

/** Nothing decided yet — the state every launch and every test starts from. */
const NOTHING_ANSWERED: Record<CaptureSource, boolean> = {
  systemAudio: false,
  microphone: false,
  camera: false,
};

/**
 * The sources whose value this launch has already decided — by seeding them from
 * the stored answer, or by somebody moving the switch. A decided source is never
 * seeded again, so a read that lands late can never undo a click.
 */
let answered: Record<CaptureSource, boolean> = { ...NOTHING_ANSWERED };

/** In-flight read, deduped so concurrent surfaces trigger one IPC hop. */
let hydration: Promise<void> | null = null;

/** Read the live session value of one source. */
function sessionValue(source: CaptureSource): boolean {
  switch (source) {
    case "systemAudio":
      return recordingAudioStore.getState().systemAudioEnabled;
    case "microphone":
      return recordingMicStore.getState().micEnabled;
    case "camera":
      return recordingWebcamStore.getState().webcamEnabled;
  }
}

/** Move the session store of one source. */
function setSessionValue(source: CaptureSource, enabled: boolean): void {
  switch (source) {
    case "systemAudio":
      recordingAudioStore.setState({ systemAudioEnabled: enabled });
      break;
    case "microphone":
      recordingMicStore.setState({ micEnabled: enabled });
      break;
    case "camera":
      recordingWebcamStore.setState({ webcamEnabled: enabled });
      break;
  }
}

/**
 * Apply an answer from Rust to every source nobody has decided yet.
 *
 * `before` is what the switches showed when the read was issued: a source that
 * has MOVED since then was moved by a person, in front of a card that is live
 * from first paint, and seeding it would undo their click and then persist the
 * undo. Per source, not per launch — touching one switch early must not throw
 * away the stored answers for the other two.
 */
function seed(vm: RecordingCaptureSourcesVm, before: Record<CaptureSource, boolean>): void {
  for (const source of SOURCES) {
    if (answered[source]) {
      continue;
    }
    answered[source] = true;
    if (sessionValue(source) === before[source]) {
      setSessionValue(source, vm[source]);
    }
  }
}

/**
 * Read the stored answer once per launch and seed the switches from it.
 *
 * Best-effort and never rejects: an unreadable settings table leaves the shipped
 * defaults standing (all three on) and allows a retry on the next call, because
 * the alternative — refusing to render the cards — would make a broken settings
 * row also a broken recorder.
 */
export async function ensureCaptureSourcesHydrated(): Promise<void> {
  if (SOURCES.every((source) => answered[source])) {
    return;
  }
  const before: Record<CaptureSource, boolean> = {
    systemAudio: sessionValue("systemAudio"),
    microphone: sessionValue("microphone"),
    camera: sessionValue("camera"),
  };
  hydration ??= recordingCaptureSourcesGet()
    .then((vm) => {
      seed(vm, before);
    })
    .catch(() => {
      // Allow a later retry rather than caching the failure forever.
      hydration = null;
    });
  await hydration;
}

/**
 * Remember the sources this patch names, after their switches have already moved
 * their session stores, and take Rust's effective answer back.
 *
 * Only the switch that moved is written, and only it is marked as answered: the
 * other two are seeded from a read that may still be in flight, so writing the
 * whole triple here would persist the shipped defaults over what this device
 * actually chose — the very bug the spec exists to end, re-entered through its
 * own fix.
 *
 * Best-effort: a refused or unreachable write leaves the session value standing,
 * so the switch the person is looking at is still the one this session applies —
 * it simply will not survive the relaunch.
 */
export async function persistCaptureSources(
  patch: Partial<Record<CaptureSource, boolean>>,
): Promise<void> {
  const moved = SOURCES.filter((source) => patch[source] !== undefined);
  // Before the await: a read this write races must not seed over the choice
  // being persisted.
  for (const source of moved) {
    answered[source] = true;
  }
  try {
    // The wire shape names every source, `null` meaning "no answer here" — so a
    // request can never be read as an answer the person did not give.
    const effective = await recordingCaptureSourcesSet({
      systemAudio: patch.systemAudio ?? null,
      microphone: patch.microphone ?? null,
      camera: patch.camera ?? null,
    });
    for (const source of moved) {
      if (effective[source] !== patch[source]) {
        // Rust answered something else — a config-file layer pins this key.
        // Showing its answer is the only honest option: the switch would
        // otherwise promise a next session that will not happen.
        setSessionValue(source, effective[source]);
      }
    }
  } catch {
    // The session value stands; nothing here fails a toggle.
  }
}

/**
 * What a start should send for each source: the session value once that source
 * has been decided this launch, and `undefined` while it has not.
 *
 * `undefined` is not a guess — `recording_start` falls back to the stored
 * preference for an absent flag, so a palette verb or a global hotkey pressed
 * before any card ever rendered starts with what this device last chose, decided
 * in Rust, without the start having to wait on a read that may never answer.
 */
export function captureSourcesForStart(): {
  systemAudio: boolean | undefined;
  microphone: boolean | undefined;
  camera: boolean | undefined;
} {
  return {
    systemAudio: answered.systemAudio ? sessionValue("systemAudio") : undefined,
    microphone: answered.microphone ? sessionValue("microphone") : undefined,
    camera: answered.camera ? sessionValue("camera") : undefined,
  };
}

/** Test-only reset: forget this launch's answers and any in-flight read. */
export function resetCaptureSourcesForTest(): void {
  answered = { ...NOTHING_ANSWERED };
  hydration = null;
}
