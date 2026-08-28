/**
 * The registry's `video`, `image` and `audio` viewer: a media file, playing or
 * shown, in a panel (Story 45.7, FR-180, UX-DR70).
 *
 * **One component branching at the element, not one module per medium.** That
 * is Story 43.5's shape and its reason holds here: three components would mean
 * three loaders, three failure paths and three places to fix the next bug.
 * There is one question — what is this file and how should it be shown — Rust
 * answered it once as a `kind`, the registry turned that into a row, and this
 * branches on `entry.viewer`. Nothing here looks at an extension.
 *
 * **The bytes arrive over `keeper-file://`, and the epic's sentence needed
 * correcting to get there.** Story 45.7 asks for media served "over
 * `keeper-recording://` with its range support". That scheme's coordinates are
 * a SESSION id against the recordings destination; a panel holds a PROFILE id
 * and a profile-relative path, and AD-74 forbids the Files surface reaching for
 * it at all. `keeper-note://` is rooted at the notes SUBFOLDER, narrower than
 * the tree the pane browses. So the range support is reused — it is
 * `note_protocol`'s, called by all four handlers — and the root is the sync
 * profile's own. See `file-asset-url.ts`.
 *
 * **44.1 is why there is a `primeFirstFrame` call in a React component.**
 * `preload="metadata"` settles a `<video>` at `readyState` 1, which the HTML
 * spec defines as having obtained no video data and representing transparent
 * black — measured in a real WKWebView as zero lit pixels, for a lone video
 * with native controls as much as for a pair. A panel showing one video is
 * exactly that lone case. Without the prime this story would ship the defect
 * 44.1 diagnosed, in a new surface, on the same day its fix landed.
 *
 * **A file the platform will not decode says so.** Not a black rectangle and
 * not an empty pane: AD-91's placeholder, with the file's own name, its size
 * and its actions, and a sentence naming which of the four `MediaError` codes
 * came back. That is the unknown viewer reused rather than reimplemented — the
 * two would otherwise come to disagree about what facts a reader gets.
 *
 * **Closing the panel releases the element.** A `<video>` with a `src` holds an
 * open range-request pipeline and a decoder until it is told to let go, and a
 * panel strip a reader opens and closes all afternoon would otherwise
 * accumulate them against files on a volume they then cannot eject.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import {
  primeFirstFrame,
  releaseMediaElement,
} from "@/components/notes/editor/recording-transport";
import { fileAssetUrl } from "@/lib/viewers/file-asset-url";
import type { ViewerProps } from "@/lib/viewers/types";
import { UnknownViewer } from "@/lib/viewers/unknown-viewer";

/** Test id for the whole media surface — the frame, not the element. */
export const MEDIA_VIEWER_TESTID = "media-viewer";

/** Test id for the element itself, whichever of the three it is. */
export const MEDIA_VIEWER_ELEMENT_TESTID = "media-viewer-element";

/** Test id for the line stating the file's name, its size and — once the
 *  platform has read enough of it to know — its intrinsic dimensions. */
export const MEDIA_VIEWER_FACTS_TESTID = "media-viewer-facts";

/**
 * The sentence for a file that is not inside a sync profile.
 *
 * A panel may VIEW a file outside every profile — 45.2 carries `profileId:
 * null` as a fact rather than a gap — and there is no URL that could reach it:
 * every scheme keeper serves is contained to a root, and a media element
 * pointed at an absolute path would be the frontend going around the
 * containment check AD-65 exists to keep. So the reader gets the file's facts
 * and Open With, which does work, rather than a player that never loads.
 */
export const MEDIA_NO_PROFILE_SENTENCE =
  "This file is not inside a synced folder, so keeper cannot stream it here. Reveal it, or hand it to the application that owns it.";

/**
 * What each `MediaError` code means, in a sentence a person can act on.
 *
 * The codes are the platform's own and are the only honest thing keeper knows
 * about a failure: the element does not say which codec was missing. Naming the
 * code's meaning is the difference between "keeper is broken" and "this file
 * needs a decoder this machine does not have" — and the reader still has Open
 * With, which hands it to something that might.
 *
 * Read by number rather than off `MediaError`'s own constants because jsdom
 * does not define that class, and a failure path that only exists in a browser
 * is a failure path nobody tests.
 */
export function mediaErrorSentence(name: string, code: number | null): string {
  switch (code) {
    case 1:
      return `Loading ${name} was aborted before it finished.`;
    case 2:
      return `keeper could not read ${name}. The volume it is on may have gone away.`;
    case 3:
      return `keeper could not decode ${name}. The file is damaged, or its codec is one this machine cannot play.`;
    case 4:
      return `keeper cannot play ${name}. This machine has no decoder for this format.`;
    default:
      // A failure with no `error` object at all — an `<img>` has none, and a
      // media element can fail before one is set. The honest answer is that
      // keeper does not know why, rather than a guess dressed as a fact.
      return `keeper could not open ${name}, and the platform did not say why.`;
  }
}

/** What a mounted element has told the viewer. */
interface Reported {
  /** The failure sentence, or `null` while nothing has failed. */
  failure: string | null;
  /** `1920 × 1080`, once the platform has read enough of the file to know it. */
  intrinsic: string | null;
}

const NOTHING_REPORTED: Reported = { failure: null, intrinsic: null };

export function MediaViewer({ file, entry }: ViewerProps): React.ReactElement {
  const source = file.profileId === null ? null : fileAssetUrl(file.profileId, file.relativePath);
  const [reported, setReported] = useState<Reported>(NOTHING_REPORTED);

  // A panel retargeted at another file must not inherit the previous one's
  // failure sentence or its dimensions. Reset during render on a changed
  // source rather than in an effect: an effect would paint one frame of the
  // old file's facts under the new file's element.
  const lastSource = useRef(source);
  if (lastSource.current !== source) {
    lastSource.current = source;
    if (reported !== NOTHING_REPORTED) {
      setReported(NOTHING_REPORTED);
    }
  }

  const onFailed = useCallback(
    (code: number | null) => {
      setReported((previous) => ({
        ...previous,
        failure: mediaErrorSentence(file.name, code),
      }));
    },
    [file.name],
  );

  // Zero means "the platform has not decoded enough to know" for `naturalWidth`
  // and for `videoWidth` alike, and it is also what an audio element reports
  // forever. One convention, read once here, so neither element has to know it.
  const onMeasured = useCallback((width: number, height: number) => {
    setReported((previous) => ({
      ...previous,
      intrinsic: width > 0 && height > 0 ? `${width} \u00D7 ${height}` : null,
    }));
  }, []);

  if (source === null) {
    return <UnknownViewer file={file} entry={entry} reason={MEDIA_NO_PROFILE_SENTENCE} />;
  }
  if (reported.failure !== null) {
    // The placeholder, with this story's reason rather than 45.2's stock one:
    // keeper does have a viewer for this format, and the decoder is what said
    // no. Same facts, same actions, one honest sentence.
    return <UnknownViewer file={file} entry={entry} reason={reported.failure} />;
  }

  // **Partition on the field that DECIDES the element, with all three named.**
  //
  // Until this guard, `audio` was the else-of-an-else: anything that was not
  // `image` and not `video` rendered an `<audio>`. The contract that only three
  // ids ever reach this component lives in `VIEWER_COMPONENTS`, in another
  // file, and was enforced nowhere here — so binding a fourth id to this
  // component would have drawn a silent, permanently empty audio bar instead of
  // saying anything. That is Story 45.13's finding applied one level up: a
  // contract stated somewhere else and trusted here is a silent-failure path,
  // and the remedy is to make the bad case unrepresentable rather than to
  // assume the good one. Below this line `entry.viewer` is exactly the three.
  if (entry.viewer !== "video" && entry.viewer !== "image" && entry.viewer !== "audio") {
    return (
      <UnknownViewer
        file={file}
        entry={entry}
        reason={`keeper routed ${file.name} to its media viewer, which draws video, images and audio, and this file is none of those. That is a wiring mistake in keeper rather than a problem with the file.`}
      />
    );
  }

  const facts = [file.name, reported.intrinsic, file.sizeLabel].filter(
    (part) => part !== null && part !== "",
  );

  return (
    <section
      data-testid={MEDIA_VIEWER_TESTID}
      data-viewer={entry.viewer}
      data-format={entry.format}
      aria-label={file.name}
      className="flex min-h-0 min-w-0 flex-col gap-2 p-4"
    >
      {entry.viewer === "image" ? (
        <ImageElement src={source} name={file.name} onFailed={onFailed} onMeasured={onMeasured} />
      ) : (
        <PlayerElement
          src={source}
          name={file.name}
          video={entry.viewer === "video"}
          onFailed={onFailed}
          onMeasured={onMeasured}
        />
      )}
      {/* Never a path. `relativePath` is the panel's own header and an absolute
          one is never rendered anywhere (FR-145). */}
      <p data-testid={MEDIA_VIEWER_FACTS_TESTID} className="truncate text-muted-foreground text-xs">
        {facts.join(" \u00B7 ")}
      </p>
    </section>
  );
}

/**
 * An `<img>` over the profile's own scheme.
 *
 * `decoding="async"` so a large photograph does not block the panel's paint.
 * No `loading="lazy"`, which a note embed does want: a panel's media is the
 * thing the reader just clicked, so deferring it would be a deliberate delay in
 * front of the only content on screen.
 *
 * The intrinsic size is read on `load`, because "how big is this actually" is
 * the question a person opens an image full-pane to answer and a byte count
 * does not answer it.
 */
function ImageElement({
  src,
  name,
  onFailed,
  onMeasured,
}: {
  src: string;
  name: string;
  onFailed: (code: number | null) => void;
  onMeasured: (width: number, height: number) => void;
}) {
  const ref = useRef<HTMLImageElement>(null);

  return (
    <img
      ref={ref}
      data-testid={MEDIA_VIEWER_ELEMENT_TESTID}
      src={src}
      // The file's own name. An empty `alt` would tell a screen reader the
      // image is decorative, and it is the entire content of the panel.
      alt={name}
      decoding="async"
      className="max-h-full min-h-0 w-auto max-w-full self-start object-contain"
      onLoad={() => {
        const node = ref.current;
        onMeasured(node?.naturalWidth ?? 0, node?.naturalHeight ?? 0);
      }}
      // An `<img>` carries no `MediaError`, so there is no code to name.
      onError={() => onFailed(null)}
    />
  );
}

/**
 * A `<video>` or an `<audio>`, with the platform's own controls.
 *
 * **Native `controls`, which is 43.6's rule rather than an omission.** The
 * shared transport exists for two tracks of one session that must agree about
 * one clock; a panel shows one file, and a hand-built bar for one track would
 * be a worse `<video controls>` — no fullscreen, no picture-in-picture, no
 * captions menu, no platform keyboard conventions. 44.1 measured that a lone
 * video with native controls was frameless too, so what a panel inherits from
 * that story is the prime, not the bar.
 *
 * **`src` is assigned last, in an effect, rather than as a JSX prop.**
 * Assigning it is what starts the load, and every listener — the failure
 * handler, the measurement and 44.1's frame prime — has to already be
 * registered when it does. React sets props during commit and runs effects
 * after, so a `src` prop would invert that order.
 */
function PlayerElement({
  src,
  name,
  video,
  onFailed,
  onMeasured,
}: {
  src: string;
  name: string;
  video: boolean;
  onFailed: (code: number | null) => void;
  onMeasured: (width: number, height: number) => void;
}) {
  // `HTMLMediaElement`, not `HTMLVideoElement`, because this component mounts
  // BOTH. Typed as the video element it was a lie the compiler could not see
  // through, and the lie had two live consequences: `node.videoWidth ?? 0` read
  // as pointless defence against a `number` while being the only thing stopping
  // `undefined` reaching the facts line for an audio element, and
  // `instanceof HTMLVideoElement` read as always-true while being genuinely
  // false half the time. A guard the compiler believes is redundant is a guard
  // the next reader deletes.
  const ref = useRef<HTMLMediaElement>(null);

  useEffect(() => {
    const node = ref.current;
    if (node === null) {
      return;
    }
    const failed = () => onFailed(node.error?.code ?? null);
    const measured = () => {
      if (node instanceof HTMLVideoElement) {
        onMeasured(node.videoWidth, node.videoHeight);
        return;
      }
      // An audio element has no intrinsic size. Reporting zeroes is what makes
      // the facts line say nothing rather than `0 × 0`.
      onMeasured(0, 0);
    };
    node.addEventListener("error", failed);
    node.addEventListener("loadedmetadata", measured);
    if (node instanceof HTMLVideoElement) {
      // Audio is asked for nothing: there is no frame to buy and the range
      // request would be spent for nothing.
      primeFirstFrame(node);
    }
    node.src = src;

    return () => {
      node.removeEventListener("error", failed);
      node.removeEventListener("loadedmetadata", measured);
      // The panel is closing, or the target changed. Removing the node is not
      // enough: the element holds its selected resource — an open range-request
      // pipeline and a decoder — until it is told to let go.
      releaseMediaElement(node);
    };
  }, [src, onFailed, onMeasured]);

  const shared = {
    "data-testid": MEDIA_VIEWER_ELEMENT_TESTID,
    controls: true,
    // Metadata only: these are files that may be hundreds of megabytes on a
    // removable volume. Opening a panel costs a duration and — for video, via
    // the prime — one keyframe, not a download.
    preload: "metadata",
    "aria-label": name,
  } as const;

  // A callback ref rather than the object: `<video>` wants a
  // `Ref<HTMLVideoElement>` and `<audio>` a `Ref<HTMLAudioElement>`, and one
  // `RefObject<HTMLMediaElement>` satisfies neither. A function taking the
  // supertype does satisfy both, which is how the honest type reaches both
  // elements without a cast.
  const attach = (node: HTMLMediaElement | null) => {
    ref.current = node;
  };

  // No caption track on either: a screen recording and its microphone sidecar
  // carry none, and keeper has none to offer. `useMediaCaption` does not fire
  // through the spread, so there is no suppression here to go stale.
  if (!video) {
    return <audio {...shared} ref={attach} className="w-full" />;
  }
  return <video {...shared} ref={attach} className="max-h-full min-h-0 w-full self-start" />;
}
