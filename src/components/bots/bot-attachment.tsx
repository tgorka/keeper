/**
 * The two things Story 61.12 puts on screen (Epic 61, FR-392, FR-393, AD-160).
 *
 * {@link BotAttachmentStrip} is the composer's tray: the images this message
 * will carry, each with what it is and how big it is, and a way to take one
 * back off. {@link BotDeliverablePaths} is the other direction: the paths an
 * answer named, each either a control or a sentence saying why there is none.
 *
 * # Why the thumbnail is a `blob:` URL and not a protocol URL
 *
 * The house rule is that bytes never cross IPC as base64 (AD-58) and that
 * vault-local assets reach the webview over a custom scheme
 * (`note_protocol.rs:4-6`). Both are honoured here, and neither is what this
 * thumbnail needs: the webview **already holds** the clipboard `File` — that is
 * where the paste came from — so `URL.createObjectURL` shows the exact bytes it
 * is about to hand Rust, at no copy and with no round trip. Adding a fifth
 * protocol handler to serve back a picture the webview is holding would be
 * more machinery for the same pixels, in a crate that does not compile on the
 * development host.
 *
 * What is *not* done here, and is the thing the rule is actually about: no
 * `data:` URI is ever put in a `src`, and no remote URL is ever fetched. The
 * only base64 in this feature is minted in Rust inside the outbound HTTP body
 * (`deliverable::image_data_uri`) and never reaches the DOM.
 *
 * # Why an unmeasured dimension renders as unmeasured
 *
 * `width`/`height` are `null` until the browser has decoded the image, and a
 * decode can fail. An absent number renders as absent — this is the app that
 * refuses to print a number it did not measure (Story 61.8's rule, applied to
 * pixels instead of tokens).
 */
import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { BotPasteContext, BotPasteDecision } from "@/components/bots/bot-paste";
import { botHumanBytes } from "@/components/bots/bot-paste";
import { Button } from "@/components/ui/button";
import type { BotDeliverableVm } from "@/lib/ipc/client";
import {
  botsDeliverablePaths,
  botsImageDiscard,
  botsImagePaste,
  syncOpenEntry,
} from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** One pasted image the composer is holding, as the pane models it. */
export interface BotPendingImage {
  /** The staging id Rust gave back. `null` while the write is in flight. */
  id: string | null;
  /** What it is called. */
  filename: string;
  /** Its MIME. */
  mime: string;
  /** Its size in bytes. */
  size: number;
  /** Pixel width, or `null` while unmeasured or unmeasurable. */
  width: number | null;
  /** Pixel height, or `null` while unmeasured or unmeasurable. */
  height: number | null;
  /** The `blob:` URL the pane made from the clipboard file. */
  previewUrl: string;
}

/** What the tray says about an image whose dimensions are not known. */
export const BOT_DIMENSIONS_UNKNOWN = "size on disk only";

/** What the tray says while Rust is still writing the image. */
export const BOT_ATTACHMENT_STAGING = "Attaching…";

/** The verb that takes one image back off the message. */
export const BOT_ATTACHMENT_REMOVE = "Remove image";

/** How one image's facts read under its thumbnail. */
export function botAttachmentCaption(image: BotPendingImage): string {
  const size = botHumanBytes(image.size);
  if (image.width === null || image.height === null) {
    return `${size}, ${BOT_DIMENSIONS_UNKNOWN}`;
  }
  return `${image.width}×${image.height}, ${size}`;
}

export function BotAttachmentStrip({
  images,
  notice,
  onRemove,
}: {
  /** The images this message will carry, in paste order. */
  images: BotPendingImage[];
  /**
   * One sentence beside the tray — a refusal that just happened, or the
   * unknown-vision warning. `null` when there is nothing to say.
   */
  notice: string | null;
  /** Take image `index` back off the message. */
  onRemove: (index: number) => void;
}) {
  if (images.length === 0 && notice === null) {
    return null;
  }
  return (
    <div className="flex shrink-0 flex-col gap-2">
      {images.length > 0 && (
        <ul aria-label="Attached images" className="flex flex-wrap items-start gap-2">
          {images.map((image, index) => (
            <li
              key={image.previewUrl}
              className="flex w-32 min-w-0 flex-col gap-1 rounded-md border border-border p-1"
            >
              <div className="relative">
                {/* The bytes the webview already holds. Never a data: URI. */}
                <img
                  src={image.previewUrl}
                  alt={image.filename}
                  className="h-20 w-full rounded-sm object-cover"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label={`${BOT_ATTACHMENT_REMOVE}: ${image.filename}`}
                  className="absolute top-0 right-0 size-6 bg-background/80"
                  onClick={() => onRemove(index)}
                >
                  <X className="size-3" aria-hidden="true" />
                </Button>
              </div>
              <p className="truncate text-xs" title={image.filename}>
                {image.filename}
              </p>
              <p
                className={cn(
                  "text-xs",
                  image.id === null ? "text-muted-foreground italic" : "text-muted-foreground",
                )}
              >
                {image.id === null ? BOT_ATTACHMENT_STAGING : botAttachmentCaption(image)}
              </p>
            </li>
          ))}
        </ul>
      )}
      {notice !== null && (
        <p role="status" className="text-muted-foreground text-xs">
          {notice}
        </p>
      )}
    </div>
  );
}

/**
 * The verb on a path keeper may open.
 *
 * "Open", not "Reveal in Files", because the command behind it
 * ({@link syncOpenEntry}) hands the file to the OS's default application —
 * naming it after a thing it does not do is the same lie as a disabled button.
 */
export const BOT_DELIVERABLE_OPEN = "Open";

/** The heading above the paths an answer named. */
export const BOT_DELIVERABLE_LABEL = "Paths in this answer";

export function BotDeliverablePaths({
  paths,
  onReveal,
}: {
  /** What Rust resolved for this reply, in the order the reply named them. */
  paths: BotDeliverableVm[];
  /** Show `subpath` of `profileId` in the Files pane. */
  onReveal: (profileId: string, subpath: string) => void;
}) {
  if (paths.length === 0) {
    return null;
  }
  return (
    <ul aria-label={BOT_DELIVERABLE_LABEL} className="flex flex-col gap-1">
      {paths.map((path) => (
        <li key={`${path.start}-${path.raw}`} className="flex min-w-0 flex-col gap-0.5">
          <code className="truncate font-mono text-xs" title={path.absolute}>
            {path.raw}
          </code>
          {/* The control exists only inside a grant. Where it does not, the
              affordance is ABSENT and the sentence is present — never a
              disabled button, which is the shape AD-27 forbids. */}
          {path.reason === null && path.profileId !== null && path.subpath !== null ? (
            <div className="flex">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => {
                  if (path.profileId !== null && path.subpath !== null) {
                    onReveal(path.profileId, path.subpath);
                  }
                }}
              >
                {BOT_DELIVERABLE_OPEN}
              </Button>
            </div>
          ) : (
            <p className="text-muted-foreground text-xs">{path.reason}</p>
          )}
        </li>
      ))}
    </ul>
  );
}

/** Everything the pane needs to hold pasted images, in one value. */
export interface BotImagePaste {
  /** The images this message will carry, in paste order. */
  images: BotPendingImage[];
  /** The one sentence beside the tray, or `null`. */
  notice: string | null;
  /** What the composer needs to decide a paste, or `null` when there is no bot. */
  context: BotPasteContext | null;
  /** Act on one paste decision the composer handed back. */
  handle: (decision: BotPasteDecision) => void;
  /** Take image `index` back off the message. */
  remove: (index: number) => void;
  /** The staged ids for the send, clearing the tray. */
  take: () => string[];
}

/**
 * Hold the images a message will carry (FR-392, AD-58).
 *
 * A hook rather than pane state so the pane's wiring is five lines: the bytes
 * path, the caps, the object-URL lifetime and the staging round trip all live
 * beside the component that draws them.
 *
 * **Object URLs are revoked.** An image taken off the message, and every image
 * still held when the pane unmounts, releases its URL — a blob URL that is
 * never revoked pins its bytes in the webview for the life of the document,
 * which is `media-viewer.tsx:36-39`'s point about a `<video>` holding a
 * decoder, one layer down.
 */
export function useBotImagePaste(
  botId: string | null,
  model: string | null,
  vision: boolean | null,
): BotImagePaste {
  const [images, setImages] = useState<BotPendingImage[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  // Read by the unmount cleanup, which must see the images as they are then
  // and not as they were when the effect was created.
  const held = useRef<BotPendingImage[]>([]);
  held.current = images;
  useEffect(
    () => () => {
      for (const image of held.current) {
        URL.revokeObjectURL(image.previewUrl);
      }
    },
    [],
  );

  const handle = (decision: BotPasteDecision) => {
    if (decision.kind === "refuse") {
      setNotice(decision.reason);
      return;
    }
    if (decision.kind !== "attachImage" || botId === null || model === null) {
      return;
    }
    setNotice(decision.warning);
    const previewUrl = URL.createObjectURL(decision.file);
    const pending: BotPendingImage = {
      id: null,
      filename: decision.filename,
      mime: decision.mime,
      size: decision.size,
      width: null,
      height: null,
      previewUrl,
    };
    setImages((current) => [...current, pending]);
    // The browser is the only thing here that can say how big the picture is.
    // A decode that never completes leaves both numbers `null`, which the tray
    // renders as unmeasured rather than as zero.
    const probe = new Image();
    probe.onload = () => {
      setImages((current) =>
        current.map((entry) =>
          entry.previewUrl === previewUrl
            ? { ...entry, width: probe.naturalWidth, height: probe.naturalHeight }
            : entry,
        ),
      );
    };
    probe.src = previewUrl;
    // The bytes leave as a raw binary IPC body. `arrayBuffer()` is the last
    // place they exist in the webview as anything but the clipboard's own File.
    void decision.file
      .arrayBuffer()
      .then((bytes) =>
        botsImagePaste(bytes, decision.filename, decision.mime, botId, model, images.length),
      )
      .then((staged) => {
        setImages((current) =>
          current.map((entry) =>
            entry.previewUrl === previewUrl ? { ...entry, id: staged.id } : entry,
          ),
        );
      })
      .catch((raw: unknown) => {
        // Rust refused, and Rust worded it. The half-added row is taken back
        // out so the tray never shows an image the send would silently drop.
        URL.revokeObjectURL(previewUrl);
        setImages((current) => current.filter((entry) => entry.previewUrl !== previewUrl));
        setNotice(ipcMessage(raw));
      });
  };

  const remove = (index: number) => {
    setImages((current) => {
      const going = current[index];
      if (going === undefined) {
        return current;
      }
      URL.revokeObjectURL(going.previewUrl);
      if (going.id !== null) {
        void botsImageDiscard(going.id).catch(() => {});
      }
      return current.filter((_, at) => at !== index);
    });
    setNotice(null);
  };

  const take = () => {
    const ids = images.flatMap((entry) => (entry.id === null ? [] : [entry.id]));
    for (const entry of images) {
      URL.revokeObjectURL(entry.previewUrl);
    }
    setImages([]);
    setNotice(null);
    return ids;
  };

  return {
    images,
    notice,
    context: botId === null || model === null ? null : { vision, model, attached: images.length },
    handle,
    remove,
    take,
  };
}

/** The sentence an IPC rejection carries, or a plain fallback. */
function ipcMessage(raw: unknown): string {
  if (typeof raw === "object" && raw !== null && "message" in raw) {
    const message = raw.message;
    if (typeof message === "string" && message !== "") {
      return message;
    }
  }
  return "keeper could not hold on to that image, so it was not attached.";
}

/**
 * The paths in one finished answer, resolved and rendered (FR-393).
 *
 * Self-fetching, and deliberately: the answer is the only input, the grant is
 * re-read by Rust on every call, and a pane that plumbed this through props
 * would have to hold one resolved list per message and re-resolve them all
 * whenever a grant changed. One row, one question, asked when the row stops
 * moving.
 *
 * Nothing renders while an answer is still arriving. A path in a half-written
 * reply is a path that may still gain three more characters, and offering to
 * open it would be offering to open a file name that does not exist yet.
 */
export function BotReplyPaths({
  sessionId,
  body,
  streaming,
}: {
  /** The conversation the answer belongs to — how Rust finds the grants. */
  sessionId: string;
  /** The answer, exactly as stored. */
  body: string;
  /** Whether this row is still arriving. */
  streaming: boolean;
}) {
  const [paths, setPaths] = useState<BotDeliverableVm[]>([]);
  useEffect(() => {
    if (streaming || body.length === 0) {
      setPaths([]);
      return;
    }
    let live = true;
    void botsDeliverablePaths(sessionId, body)
      .then((resolved) => {
        if (live) {
          setPaths(resolved);
        }
      })
      // A resolution that failed is a row with no controls, which is the same
      // safe direction every other refusal in this story takes.
      .catch(() => {
        if (live) {
          setPaths([]);
        }
      });
    return () => {
      live = false;
    };
  }, [sessionId, body, streaming]);
  return (
    <BotDeliverablePaths
      paths={paths}
      onReveal={(profileId, subpath) => {
        // Best-effort, like every other open verb in the tree: a file manager
        // that refuses is not a reason to put an error on a conversation.
        void syncOpenEntry(profileId, subpath).catch(() => {});
      }}
    />
  );
}
