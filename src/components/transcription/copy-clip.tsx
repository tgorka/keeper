/**
 * Copy a clip: a `keeper-media` block for the same meeting, with a window, that
 * Rust composes and the person pastes into any note.
 *
 * *From* and *To* are `hh:mm:ss`, and Rust reads them as they are typed: its
 * sentence stands under the fields, and *Copy* waits for a composition of
 * exactly what the fields say. Both empty is the whole meeting.
 */
import { useEffect, useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { MediaClipVm } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { timestamp } from "./transcript-lines";

/** What a Copy clip opens on: a window in seconds, or `null` for the whole meeting. */
export interface ClipRequest {
  from: number | null;
  to: number | null;
}

export const COPY_CLIP_TITLE = "Copy a clip";
export const COPIED_SENTENCE = "Copied. Paste it into any note.";
export const WORDS_LABEL = "Include the words, for Obsidian and other apps";
/** Long enough to let a time be typed, short enough to read as checking as you type. */
const CHECK_DELAY_MS = 250;

export function CopyClipDialog({
  request,
  compose,
  onClose,
}: {
  /** Open while not null; each request starts the fields afresh. */
  request: ClipRequest | null;
  compose: (from: string | null, to: string | null, words: boolean) => Promise<MediaClipVm>;
  onClose: () => void;
}) {
  return (
    <Dialog
      open={request !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{COPY_CLIP_TITLE}</DialogTitle>
          <DialogDescription>
            A block for this meeting, to paste into any note. Leave both times empty for the whole
            meeting.
          </DialogDescription>
        </DialogHeader>
        {request && <ClipForm request={request} compose={compose} />}
      </DialogContent>
    </Dialog>
  );
}

function ClipForm({
  request,
  compose,
}: {
  request: ClipRequest;
  compose: (from: string | null, to: string | null, words: boolean) => Promise<MediaClipVm>;
}) {
  const id = useId();
  const [from, setFrom] = useState(request.from === null ? "" : timestamp(request.from));
  const [to, setTo] = useState(request.to === null ? "" : timestamp(request.to));
  const [words, setWords] = useState(true);
  const [clip, setClip] = useState<MediaClipVm | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const generation = useRef(0);
  const composeRef = useRef(compose);
  composeRef.current = compose;
  useEffect(() => {
    const mine = ++generation.current;
    setClip(null);
    setCopied(null);
    const timer = setTimeout(() => {
      composeRef
        .current(from.trim() || null, to.trim() || null, words)
        .then((next) => {
          if (generation.current !== mine) return;
          setClip(next);
          setRefusal(null);
        })
        .catch((cause: unknown) => {
          if (generation.current === mine) setRefusal(syncErrorMessage(cause));
        });
    }, CHECK_DELAY_MS);
    return () => clearTimeout(timer);
  }, [from, to, words]);
  return (
    <form
      className="space-y-3"
      onSubmit={(event) => {
        event.preventDefault();
        if (!clip) return;
        const markdown = clip.markdown;
        void navigator.clipboard
          ?.writeText(markdown)
          .then(() => setCopied(COPIED_SENTENCE))
          .catch((cause: unknown) => setRefusal(syncErrorMessage(cause, "keeper could not copy.")));
      }}
    >
      <div className="flex flex-wrap gap-3">
        <div className="min-w-0 flex-1 basis-28 space-y-1">
          <Label htmlFor={`${id}-from`}>From</Label>
          <Input
            id={`${id}-from`}
            className="font-mono"
            placeholder="start"
            value={from}
            onChange={(event) => setFrom(event.target.value)}
          />
        </div>
        <div className="min-w-0 flex-1 basis-28 space-y-1">
          <Label htmlFor={`${id}-to`}>To</Label>
          <Input
            id={`${id}-to`}
            className="font-mono"
            placeholder="end"
            value={to}
            onChange={(event) => setTo(event.target.value)}
          />
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Checkbox
          id={`${id}-words`}
          checked={words}
          onCheckedChange={(next) => setWords(next === true)}
        />
        <Label htmlFor={`${id}-words`}>{WORDS_LABEL}</Label>
      </div>
      {refusal ? (
        <p role="alert" className="break-words text-destructive">
          {refusal}
        </p>
      ) : (
        <p className="text-muted-foreground text-xs" aria-live="polite">
          {clip ? `${clip.lines} ${clip.lines === 1 ? "line" : "lines"}` : "Checking…"}
        </p>
      )}
      <DialogFooter className="items-center">
        {copied && (
          <p role="status" className="mr-auto text-muted-foreground">
            {copied}
          </p>
        )}
        <Button type="submit" disabled={!clip}>
          Copy
        </Button>
      </DialogFooter>
    </form>
  );
}
