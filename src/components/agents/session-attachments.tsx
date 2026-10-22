import { FileText, RefreshCw } from "lucide-react";
import { useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import type { AgentAttachmentVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** Metadata only until Open: never mount a thumbnail that downloads another member's file. */
export function SessionAttachments({ attachments }: { attachments: AgentAttachmentVm[] }) {
  return (
    <ul aria-label="Attachments" className="mt-2 grid min-w-0 gap-2 sm:grid-cols-2">
      {attachments.map((attachment) => (
        <AttachmentChip key={attachment.id} attachment={attachment} />
      ))}
    </ul>
  );
}

function AttachmentChip({ attachment }: { attachment: AgentAttachmentVm }) {
  const description = useId();
  const [open, setOpen] = useState(false);
  // `blocked`: the server never judged this file — another file of its message was refused — so
  // it reads "not sent", never like the refused one (R1071C-03).
  const state = attachment.state === "blocked" ? "not sent" : attachment.state;
  return (
    <li className="min-w-0 rounded-lg border border-border bg-muted/30">
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogTrigger asChild>
          <button
            type="button"
            aria-label={`Open ${attachment.name}`}
            aria-describedby={description}
            className="flex min-h-11 w-full min-w-0 items-center gap-2 rounded-lg p-3 text-left hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring"
          >
            <FileText aria-hidden="true" className="size-5 shrink-0 text-muted-foreground" />
            <span className="min-w-0 flex-1">
              <span className="block break-words text-sm [overflow-wrap:anywhere]">
                {attachment.name}
              </span>
              <span id={description} className="block text-muted-foreground text-xs">
                {attachment.sizeLabel} · {state}
              </span>
            </span>
            <span className="shrink-0 text-muted-foreground text-xs">Open</span>
          </button>
        </DialogTrigger>
        <div role="status" aria-live="polite" aria-atomic="true">
          <span className="sr-only">
            {attachment.name}: {state}
          </span>
          {attachment.sentence && (
            <p
              className={cn(
                "px-3 pb-3 text-sm [overflow-wrap:anywhere]",
                attachment.state === "refused" ? "text-destructive" : "text-muted-foreground",
              )}
            >
              {attachment.sentence}
            </p>
          )}
        </div>
        {open && <AttachmentPreview attachment={attachment} />}
      </Dialog>
    </li>
  );
}

/** The scheme owns downloading, authorization and caching; no blob bytes enter React. */
function AttachmentPreview({ attachment }: { attachment: AgentAttachmentVm }) {
  const [attempt, setAttempt] = useState(0);
  const [failed, setFailed] = useState(false);
  const retry = useRef<HTMLButtonElement>(null);
  const url =
    attempt === 0
      ? attachment.url
      : `${attachment.url}${attachment.url.includes("?") ? "&" : "?"}retry=${attempt}`;
  const image = ["image/png", "image/jpeg", "image/webp", "image/gif"].includes(attachment.mime);
  return (
    <DialogContent
      className="min-w-0 sm:max-w-3xl"
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        retry.current?.focus();
      }}
    >
      <DialogHeader className="min-w-0 pr-6">
        <DialogTitle className="[overflow-wrap:anywhere]">{attachment.name}</DialogTitle>
        <DialogDescription>
          {attachment.sizeLabel}. The file loads when opened. If it cannot be previewed, download it
          or retry opening it.
        </DialogDescription>
      </DialogHeader>
      {failed ? (
        <p role="alert" className="text-destructive text-sm [overflow-wrap:anywhere]">
          keeper could not open {attachment.name}. Retry to request the file again.
        </p>
      ) : image ? (
        <img
          src={url}
          alt={attachment.name}
          onError={() => setFailed(true)}
          className="max-h-[60dvh] w-full object-contain"
        />
      ) : (
        <iframe
          key={url}
          title={`Preview ${attachment.name}`}
          src={url}
          sandbox="allow-downloads"
          referrerPolicy="no-referrer"
          onError={() => setFailed(true)}
          className="h-[50dvh] w-full min-w-0 rounded border border-border"
        />
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          ref={retry}
          variant="outline"
          className="min-h-11"
          onClick={() => {
            setFailed(false);
            setAttempt((n) => n + 1);
          }}
        >
          <RefreshCw aria-hidden="true" /> Retry opening
        </Button>
        <Button asChild variant="outline" className="min-h-11">
          <a href={url} download={attachment.name}>
            Download file
          </a>
        </Button>
      </div>
    </DialogContent>
  );
}
