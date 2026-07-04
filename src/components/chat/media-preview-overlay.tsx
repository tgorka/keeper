/**
 * Quick-Look-style media preview overlay (Story 3.6, FR-13, AC2/AC3).
 *
 * A shadcn {@link Dialog}-based overlay that shows the full-resolution asset from
 * the `…/full` `keeper-media://` URL: an `<img>` for images, `<video controls
 * autoPlay>` for video (seek plays via the Range protocol → 206), or `<audio
 * controls>` for audio. `Esc` / backdrop close the dialog and radix returns focus
 * to the trigger (the timeline bubble). The parent controls open state by passing
 * the resolved {@link MediaVm} (or `null` to close).
 */
import { RefreshCw } from "lucide-react";
import { useCallback, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import type { MediaVm } from "@/lib/ipc/client";

interface MediaPreviewOverlayProps {
  /** The media to preview, or `null` when the overlay is closed. */
  media: MediaVm | null;
  /** Close the overlay (Esc / backdrop / explicit close). */
  onClose: () => void;
}

export function MediaPreviewOverlay({ media, onClose }: MediaPreviewOverlayProps) {
  const open = media !== null;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          onClose();
        }
      }}
    >
      {media !== null && (
        <DialogContent
          showCloseButton
          className="flex max-h-[90vh] max-w-[90vw] items-center justify-center border-0 bg-transparent p-0 shadow-none sm:max-w-[90vw]"
        >
          {/* Screen-reader-only title/description keep the dialog labeled. */}
          <DialogTitle className="sr-only">{media.filename}</DialogTitle>
          <DialogDescription className="sr-only">
            Media preview for {media.filename}
          </DialogDescription>
          <PreviewBody media={media} />
          <DialogClose className="sr-only">Close preview</DialogClose>
        </DialogContent>
      )}
    </Dialog>
  );
}

/**
 * The full-resolution preview body, chosen by media kind. The `…/full` variant is
 * a heavier fetch than the bubble thumbnail and is the most likely to fail (e.g.
 * the handle became unresolvable after the thumbnail already loaded from cache), so
 * — like the bubble — it must surface an honest retry affordance rather than a
 * silent broken/blank element. `onError` swaps in a retry that re-requests the
 * `src` with a cache-busting suffix (the protocol handler re-fetches on a miss).
 */
function PreviewBody({ media }: { media: MediaVm }) {
  const [errored, setErrored] = useState(false);
  const [nonce, setNonce] = useState(0);
  const onError = useCallback(() => setErrored(true), []);
  const onRetry = useCallback(() => {
    setErrored(false);
    setNonce((n) => n + 1);
  }, []);

  if (errored) {
    return (
      <div className="flex flex-col items-center gap-3 rounded-lg bg-background p-8">
        <span className="text-muted-foreground text-sm">Couldn't load {media.filename}.</span>
        <Button type="button" variant="outline" size="sm" onClick={onRetry}>
          <RefreshCw aria-hidden="true" className="size-4" />
          Retry
        </Button>
      </div>
    );
  }

  const sep = media.url.includes("?") ? "&" : "?";
  const src = nonce === 0 ? media.url : `${media.url}${sep}retry=${nonce}`;

  if (media.kind === "image") {
    return (
      <img
        src={src}
        alt={media.filename}
        onError={onError}
        className="max-h-[85vh] max-w-full rounded-lg object-contain"
      />
    );
  }
  if (media.kind === "video") {
    return (
      <video
        src={src}
        controls
        autoPlay
        onError={onError}
        className="max-h-[85vh] max-w-full rounded-lg"
        aria-label={media.filename}
      >
        <track kind="captions" />
      </video>
    );
  }
  // Audio (and any other kind) plays inline in the overlay.
  return (
    <div className="flex w-[min(480px,90vw)] flex-col gap-2 rounded-lg bg-background p-6">
      <span className="truncate font-medium text-sm">{media.filename}</span>
      {/* biome-ignore lint/a11y/useMediaCaption: user-sent audio clips carry no caption track. */}
      <audio
        src={src}
        controls
        autoPlay
        onError={onError}
        aria-label={media.filename}
        className="w-full"
      />
    </div>
  );
}
