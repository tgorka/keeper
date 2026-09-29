/**
 * Transcribe a File… from the menu bar, the palette and the recording panes.
 *
 * The verb starts the same job the Settings button does, from wherever the
 * person is, so there is no pane on screen to hold the job strip. What a
 * background job started outside its pane shows in keeper is a toast (the
 * export's pattern): one Sonner toast per file, updated in place as the job
 * moves — the progress bar, the phase and the elapsed time — with Cancel while
 * it runs, Try again when it fails and Open transcript when it is done. The
 * transcript opens in the one dialog this module mounts with the shell.
 */
import { toast } from "sonner";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { TranscriptDialog } from "@/components/transcription/transcript-viewer";
import { TranscriptionProgress } from "@/components/transcription/transcription-progress";
import { transcriptionCancel } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import {
  pickFileToTranscribe,
  startTranscription,
  transcriptionJobLine,
  transcriptionRunning,
  transcriptionStore,
} from "@/lib/stores/transcription";

/** The button the Recordings and Recording panes carry for the verb. */
export const TRANSCRIBE_A_FILE_LABEL = "Transcribe a file…";

/** The transcript the verb's toast opened, if any. */
const openTranscriptStore = createStore<{ path: string | null }>()(() => ({ path: null }));

/** Follow one file's job in a toast until it ends, then start it. */
async function transcribeWithToast(path: string): Promise<void> {
  const id = `transcribe:${path}`;
  const unsubscribe = transcriptionStore.subscribe((state, previous) => {
    const job = state.jobs[path];
    if (!job || job === previous.jobs[path]) return;
    const name = path.split(/[\\/]/).pop() ?? path;
    if (transcriptionRunning(job)) {
      toast.loading(name, {
        id,
        description: <TranscriptionProgress job={job} className="mt-1 w-full" />,
        action: job.jobId
          ? {
              label: "Cancel transcription",
              onClick: () => {
                void transcriptionCancel(job.jobId).catch((cause: unknown) =>
                  toast.error(syncErrorMessage(cause), { id }),
                );
              },
            }
          : undefined,
      });
      return;
    }
    unsubscribe();
    const transcriptPath = job.transcriptPath;
    if (job.phase === "done" && transcriptPath)
      toast.success(name, {
        id,
        description: "The transcript is ready.",
        action: {
          label: "Open transcript",
          onClick: () => openTranscriptStore.setState({ path: transcriptPath }),
        },
      });
    else if (job.phase === "failed")
      toast.error(name, {
        id,
        description: transcriptionJobLine(job),
        action: { label: "Try again", onClick: () => void transcribeWithToast(path) },
      });
    else toast(name, { id, description: "Transcription cancelled." });
  });
  await startTranscription(path);
}

/** The registry verb: the Settings picker, then the job in a toast. */
export async function transcribeAFile(): Promise<void> {
  let picked: string | null;
  try {
    picked = await pickFileToTranscribe();
  } catch (cause) {
    toast.error(syncErrorMessage(cause));
    return;
  }
  if (picked) await transcribeWithToast(picked);
}

/** Mounted once with the shell: where the toast's Open transcript lands. */
export function TranscribeAFileHost() {
  const path = useStore(openTranscriptStore, (s) => s.path);
  return (
    <TranscriptDialog path={path} onClose={() => openTranscriptStore.setState({ path: null })} />
  );
}
