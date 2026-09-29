import { useState } from "react";
import { TranscriptionProgress } from "@/components/transcription/transcription-progress";
import { Button } from "@/components/ui/button";
import { transcriptionCancel } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import {
  startTranscription,
  transcriptionJobLine,
  transcriptionRunning,
  useTranscriptionStore,
} from "@/lib/stores/transcription";

export function TranscriptionJob({
  path,
  onOpen,
}: {
  path: string;
  onOpen: (path: string) => void;
}) {
  const job = useTranscriptionStore((s) => s.jobs[path]);
  const [error, setError] = useState<string | null>(null);
  if (!job) return null;
  const running = transcriptionRunning(job);
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm">
      {running ? (
        <div className="min-w-48 flex-1">
          <TranscriptionProgress job={job} announce />
        </div>
      ) : (
        <p role="status" className="min-w-0 break-words">
          {transcriptionJobLine(job)}
        </p>
      )}
      {running && (
        <Button
          size="sm"
          variant="outline"
          disabled={!job.jobId}
          onClick={() => {
            void transcriptionCancel(job.jobId).catch((cause: unknown) =>
              setError(syncErrorMessage(cause)),
            );
          }}
        >
          Cancel transcription
        </Button>
      )}
      {job.phase === "failed" && (
        <Button size="sm" variant="outline" onClick={() => void startTranscription(path)}>
          Try again
        </Button>
      )}
      {job.transcriptPath && (
        <Button
          size="sm"
          variant="outline"
          onClick={() => {
            if (job.transcriptPath) onOpen(job.transcriptPath);
          }}
        >
          Open transcript
        </Button>
      )}
      {error && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}
