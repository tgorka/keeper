/**
 * Presentational media attachment renderer (Story 3.6, FR-13, AD-4).
 *
 * Given a {@link MediaVm} — carrying only opaque `keeper-media://` URLs + display
 * metadata — renders the right surface per kind: an image thumbnail, a video
 * poster, an inline `<audio controls>`, or a file chip (icon + name + size). The
 * decrypted bytes are served exclusively over the `keeper-media://` protocol; this
 * component never sees `mxc`, keys, or bytes over IPC.
 *
 * A loading skeleton shows until the media element fires `onLoad`; an `onError`
 * (fetch/decrypt failure → the protocol handler's 404) swaps in a retry affordance
 * that reloads the `src` with a cache-busting suffix (the handler re-fetches on a
 * cache miss). Image/video click or Enter calls `onOpenPreview(key)` to open the
 * Quick-Look overlay. Width/height reserve layout so the thumbnail never reflows.
 */
import { FileIcon, FileVideo, Loader2, RefreshCw } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import type { MediaVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

interface MediaAttachmentProps {
  /** The media view model to render (opaque URLs + display metadata only). */
  media: MediaVm;
  /** The owning message's opaque render key, passed to {@link onOpenPreview}. */
  messageKey: string;
  /**
   * Open the Quick-Look preview overlay for this message (image/video). Wired by
   * the parent; when absent, the attachment is not click-to-open.
   */
  onOpenPreview?: (key: string) => void;
  /**
   * Whether this attachment's own local echo is still uploading (`sendState ===
   * "sending"`, Story 3.7). When `true`, an indeterminate uploading indicator +
   * Cancel affordance overlay the attachment — derived purely from the existing
   * send-state, no new VM field. MVP shows an indeterminate spinner (the send-queue
   * path surfaces no byte-% in matrix-sdk 0.18).
   */
  uploading?: boolean;
  /**
   * Cancel the in-flight upload for {@link messageKey} (Story 3.7). Wired only
   * while {@link uploading}; best-effort — if the send already dispatched it is a
   * no-op and the message stays sent.
   */
  onCancel?: (key: string) => void;
}

/** Format a byte count as a short human-readable size (e.g. `1.2 MB`). */
function formatSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/**
 * Append a cache-busting query suffix to a `keeper-media://` URL so a retry
 * forces the webview to re-request (the protocol handler re-fetches on a cache
 * miss). Reuses `?` vs `&` correctly.
 */
function withCacheBust(url: string, nonce: number): string {
  if (nonce === 0) {
    return url;
  }
  const sep = url.includes("?") ? "&" : "?";
  return `${url}${sep}retry=${nonce}`;
}

export function MediaAttachment({
  media,
  messageKey,
  onOpenPreview,
  uploading = false,
  onCancel,
}: MediaAttachmentProps) {
  const [loaded, setLoaded] = useState(false);
  const [errored, setErrored] = useState(false);
  // Bumped on retry to cache-bust the src and re-trigger load/error.
  const [nonce, setNonce] = useState(0);

  const onRetry = useCallback(() => {
    setErrored(false);
    setLoaded(false);
    setNonce((n) => n + 1);
  }, []);

  const onLoad = useCallback(() => setLoaded(true), []);
  const onError = useCallback(() => setErrored(true), []);

  const openPreview = useCallback(() => {
    onOpenPreview?.(messageKey);
  }, [onOpenPreview, messageKey]);

  // The thumbnail/poster src for image/video; the full src for inline audio.
  const thumbSrc = useMemo(
    () => (media.thumbnailUrl != null ? withCacheBust(media.thumbnailUrl, nonce) : null),
    [media.thumbnailUrl, nonce],
  );
  const fullSrc = useMemo(() => withCacheBust(media.url, nonce), [media.url, nonce]);

  // Reserve layout from intrinsic dimensions so the thumbnail never reflows.
  const aspectStyle =
    media.width != null && media.height != null
      ? { aspectRatio: `${media.width} / ${media.height}` }
      : undefined;

  // The kind-specific surface (image/video/audio/file or the retry affordance).
  // Wrapped below by an uploading overlay + Cancel while an own echo is sending.
  function renderSurface() {
    if (errored) {
      return <MediaRetry onRetry={onRetry} filename={media.filename} />;
    }

    if (media.kind === "image") {
      return (
        <div className="relative max-w-[320px] overflow-hidden rounded-lg" style={aspectStyle}>
          {!loaded && <Skeleton className="absolute inset-0 h-full w-full" />}
          <button
            type="button"
            aria-label={`Open image ${media.filename}`}
            onClick={openPreview}
            disabled={onOpenPreview == null}
            className={cn(
              "block w-full",
              onOpenPreview != null && "cursor-pointer",
              !loaded && "opacity-0",
            )}
          >
            <img
              src={thumbSrc ?? fullSrc}
              alt={media.filename}
              width={media.width ?? undefined}
              height={media.height ?? undefined}
              onLoad={onLoad}
              onError={onError}
              className="block h-auto w-full object-cover"
            />
          </button>
        </div>
      );
    }

    if (media.kind === "video") {
      const hasPoster = thumbSrc != null;
      return (
        <div className="relative max-w-[320px] overflow-hidden rounded-lg" style={aspectStyle}>
          {hasPoster && !loaded && <Skeleton className="absolute inset-0 h-full w-full" />}
          <button
            type="button"
            aria-label={`Open video ${media.filename}`}
            onClick={openPreview}
            disabled={onOpenPreview == null}
            className={cn(
              "block w-full",
              onOpenPreview != null && "cursor-pointer",
              hasPoster && !loaded && "opacity-0",
            )}
          >
            {hasPoster ? (
              <img
                src={thumbSrc}
                alt={media.filename}
                width={media.width ?? undefined}
                height={media.height ?? undefined}
                onLoad={onLoad}
                onError={onError}
                className="block h-auto w-full object-cover"
              />
            ) : (
              // No poster source (e.g. an encrypted video without a thumbnail):
              // render a static placeholder rather than mounting a <video>, which
              // would force the protocol handler to download+decrypt the ENTIRE file
              // just to show a poster frame (matrix-sdk's fetch is atomic). The full
              // video loads only when the user opens the preview overlay.
              <div
                className={cn(
                  "flex w-full items-center justify-center bg-muted",
                  aspectStyle == null && "aspect-video",
                )}
                style={aspectStyle}
              >
                <FileVideo aria-hidden="true" className="size-10 text-muted-foreground" />
              </div>
            )}
          </button>
          {/* Play affordance overlay hint (immediate for the non-fetching placeholder). */}
          {(!hasPoster || loaded) && (
            <span
              aria-hidden="true"
              className="pointer-events-none absolute inset-0 flex items-center justify-center"
            >
              <span className="flex h-12 w-12 items-center justify-center rounded-full bg-black/50 text-white">
                ▶
              </span>
            </span>
          )}
        </div>
      );
    }

    if (media.kind === "audio") {
      return (
        <div className="flex max-w-[320px] flex-col gap-1">
          {/* Inline audio playback over the protocol (AC3). No skeleton needed —
            the controls render immediately; loading is handled by the element. */}
          {/* biome-ignore lint/a11y/useMediaCaption: user-sent voice/audio clips have no caption track. */}
          <audio
            src={fullSrc}
            controls
            preload="metadata"
            onError={onError}
            onLoadedMetadata={onLoad}
            aria-label={`Audio ${media.filename}`}
            className="w-full"
          />
          <span className="truncate text-muted-foreground text-xs">{media.filename}</span>
        </div>
      );
    }

    // File chip: icon + name + human size. No auto-download of bytes over IPC.
    return (
      <div className="flex max-w-[320px] items-center gap-3 rounded-lg border bg-muted/50 p-3">
        <FileIcon aria-hidden="true" className="size-8 shrink-0 text-muted-foreground" />
        <div className="flex min-w-0 flex-col">
          <span className="truncate font-medium text-sm">{media.filename}</span>
          <span className="text-muted-foreground text-xs">
            {media.mimetype ?? "File"}
            {media.size != null ? ` · ${formatSize(media.size)}` : ""}
          </span>
        </div>
      </div>
    );
  }

  // While the own echo is still uploading, overlay an indeterminate indicator +
  // Cancel on top of the surface (Story 3.7). Cancel is best-effort — if the send
  // already dispatched, it is a no-op and the message stays sent.
  if (uploading) {
    return (
      <div className="relative inline-block max-w-[320px]">
        {renderSurface()}
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 rounded-lg bg-black/50">
          <Loader2 aria-label="Uploading" className="size-6 animate-spin text-white" />
          {onCancel != null && (
            <Button
              type="button"
              variant="secondary"
              size="xs"
              aria-label="Cancel upload"
              onClick={() => onCancel(messageKey)}
            >
              Cancel
            </Button>
          )}
        </div>
      </div>
    );
  }

  return renderSurface();
}

interface MediaRetryProps {
  onRetry: () => void;
  filename: string;
}

/**
 * The retry affordance shown when a media fetch fails (the protocol handler's 404
 * → `onError`). Clicking reloads the `src` with a cache-busting suffix.
 */
function MediaRetry({ onRetry, filename }: MediaRetryProps) {
  return (
    <div className="flex max-w-[320px] flex-col items-start gap-2 rounded-lg border border-dashed p-4">
      <span className="text-muted-foreground text-sm">Couldn't load {filename}.</span>
      <Button type="button" variant="outline" size="xs" onClick={onRetry}>
        <RefreshCw aria-hidden="true" className="size-3" />
        Retry
      </Button>
    </div>
  );
}
