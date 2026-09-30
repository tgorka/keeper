import { useState } from "react";
import {
  TRANSCRIBE_AGAIN_LABEL,
  TranscribeAgainDialog,
} from "@/components/transcription/transcribe-again";
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
  const [asking, setAsking] = useState(false);
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
      {/* A job that stopped at a transcript keeper will not overwrite on its
          own would only stop there again: the way on is to replace it. */}
      {job.phase === "failed" &&
        (job.replaceable ? (
          <Button size="sm" variant="outline" onClick={() => setAsking(true)}>
            {TRANSCRIBE_AGAIN_LABEL}…
          </Button>
        ) : (
          <Button size="sm" variant="outline" onClick={() => void startTranscription(path)}>
            Try again
          </Button>
        ))}
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
      <TranscribeAgainDialog
        open={asking}
        onClose={() => setAsking(false)}
        onConfirm={() => void startTranscription(path, undefined, true)}
      />
    </div>
  );
}
