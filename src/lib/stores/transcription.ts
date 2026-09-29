import { open as openFile } from "@tauri-apps/plugin-dialog";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import {
  type DictionaryTermVm,
  dictionaryTerms,
  type PersonVm,
  type TranscriptionLanguage,
  type TranscriptionProgressVm,
  type TranscriptionStatusVm,
  transcriptionSettingsSet,
  transcriptionStart,
  transcriptionStatus,
  voicesPeople,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

interface TranscriptionState {
  status: TranscriptionStatusVm | null;
  error: string | null;
  saving: boolean;
  jobs: Record<string, TranscriptionProgressVm>;
  people: Record<string, PersonVm[]>;
  dictionary: Record<string, DictionaryTermVm[]>;
}
export const transcriptionStore = createStore<TranscriptionState>()(() => ({
  status: null,
  error: null,
  saving: false,
  jobs: {},
  people: {},
  dictionary: {},
}));
export function useTranscriptionStore<T>(selector: (state: TranscriptionState) => T): T {
  return useStore(transcriptionStore, selector);
}
let hydration: Promise<void> | null = null;
export function refreshTranscription(): Promise<void> {
  if (hydration) return hydration;
  hydration = transcriptionStatus()
    .then((status) => {
      transcriptionStore.setState({ status, error: null });
    })
    .catch((cause: unknown) => {
      transcriptionStore.setState({ error: syncErrorMessage(cause) });
    })
    .finally(() => {
      hydration = null;
    });
  return hydration;
}
export async function saveTranscriptionSettings(patch: {
  language?: TranscriptionLanguage;
  afterRecording?: boolean;
}): Promise<void> {
  if (transcriptionStore.getState().saving) return;
  transcriptionStore.setState({ saving: true, error: null });
  try {
    const status = await transcriptionSettingsSet(
      patch.language ?? null,
      patch.afterRecording ?? null,
    );
    transcriptionStore.setState({ status });
  } catch (cause) {
    transcriptionStore.setState({ error: syncErrorMessage(cause) });
  } finally {
    transcriptionStore.setState({ saving: false });
  }
}
export async function refreshVoicesDrive(profileId: string): Promise<void> {
  const [people, dictionary] = await Promise.all([
    voicesPeople(profileId),
    dictionaryTerms(profileId),
  ]);
  transcriptionStore.setState((s) => ({
    people: { ...s.people, [profileId]: people },
    dictionary: { ...s.dictionary, [profileId]: dictionary },
  }));
}
export function transcriptionRunning(job: TranscriptionProgressVm | undefined): boolean {
  return job !== undefined && !["done", "failed", "cancelled"].includes(job.phase);
}
/** What a job is doing, in the words the job strip and the menu verb's toast both show. */
export function transcriptionJobLine(job: TranscriptionProgressVm): string {
  return `${job.message ?? job.phase}${job.parts > 0 ? ` · Part ${job.part} of ${job.parts}` : ""}`;
}
/**
 * The native picker Settings › Transcription and the Transcribe a File… verb
 * both open. Resolves with the chosen path, or null when the person cancelled.
 */
export async function pickFileToTranscribe(): Promise<string | null> {
  const picked = await openFile({ directory: false, multiple: false, title: "Transcribe a file" });
  return typeof picked === "string" ? picked : null;
}
/** Jobs outlive the surface that started them. The generation rejects a late batch from an older attempt. */
const generations = new Map<string, number>();
/**
 * What the surface that started a path's job wants done when it ends (re-read a
 * listing, never open a dialog on its own). Kept per path so the job strip's
 * Try again, which knows only the path, repeats it.
 */
const followUps = new Map<string, () => void>();
export async function startTranscription(path: string, onDone?: () => void): Promise<void> {
  if (transcriptionRunning(transcriptionStore.getState().jobs[path])) return;
  if (onDone) followUps.set(path, onDone);
  const generation = (generations.get(path) ?? 0) + 1;
  generations.set(path, generation);
  const put = (progress: TranscriptionProgressVm) => {
    if (generations.get(path) !== generation) return;
    transcriptionStore.setState((s) => ({ jobs: { ...s.jobs, [path]: progress } }));
    if (progress.phase === "done") followUps.get(path)?.();
  };
  put({ jobId: "", phase: "queued", part: 0, parts: 0, message: null, transcriptPath: null });
  try {
    const jobId = await transcriptionStart(path, put);
    const current = transcriptionStore.getState().jobs[path];
    if (current && !current.jobId && generations.get(path) === generation)
      put({ ...current, jobId });
  } catch (cause) {
    put({
      jobId: "",
      phase: "failed",
      part: 0,
      parts: 0,
      message: syncErrorMessage(cause),
      transcriptPath: null,
    });
  }
}
