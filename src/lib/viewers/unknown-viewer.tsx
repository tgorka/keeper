/**
 * The viewer for a format keeper cannot render (Story 45.2, FR-174, AD-91).
 *
 * **Unknown is a kind, not a failure.** The difference between a file browser
 * and a demo is what happens when you click the one file nobody anticipated: a
 * demo shows a blank pane or an error, and a browser tells you what the thing
 * is and gets out of your way. So this names the file, names its extension,
 * states its size and offers the two actions that leave keeper entirely —
 * reveal it in the file manager, or hand it to the application that owns it.
 * Nothing here reads a byte of the file.
 *
 * **It is also the fallback for a format keeper knows and cannot yet show.**
 * A wave-2 viewer that has not landed leaves its rows resolving here, and the
 * sentence says which format it was rather than pretending the file is
 * unrecognisable. A surface that declines to act must say what it declined to
 * do (DW-162), and "I know this is a PDF and cannot draw it" is a different
 * fact from "I have never heard of `.sketchpad`".
 *
 * **No absolute path is rendered, ever** (FR-145). The heading is the file's
 * own name and the line under it is the path relative to whatever root the
 * surface is browsing. `absolutePath` is the argument of Reveal and nothing
 * else — the same rule 43.5's attachment chip follows, for the same reason: a
 * screenshot of a note should not carry somebody's home directory.
 *
 * **Reveal is absent, not disabled, where there is no file manager.** Read
 * from the capabilities mirror, which is Rust's answer and the only authority
 * on what this platform has (AD-20). A disabled control is a promise the
 * platform cannot keep.
 */

import { Button } from "@/components/ui/button";
import { revealPath } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { extensionOf } from "./registry";
import type { ViewerEntry, ViewerProps } from "./types";

/** Test id for the whole placeholder. */
export const UNKNOWN_VIEWER_TESTID = "unknown-viewer";

/** Test id for the value cell that names the extension — a slot, so a test
 *  asserts the extension rather than re-deriving the surrounding sentence. */
export const UNKNOWN_VIEWER_EXTENSION_SLOT = "unknown-viewer-extension";

/** Test id for the value cell that states the size. */
export const UNKNOWN_VIEWER_SIZE_SLOT = "unknown-viewer-size";

/** Test id for the value cell that names the format. */
export const UNKNOWN_VIEWER_FORMAT_SLOT = "unknown-viewer-format";

/** The three facts this placeholder is made of. */
export const UNKNOWN_VIEWER_EXTENSION_LABEL = "Extension";
export const UNKNOWN_VIEWER_SIZE_LABEL = "Size";
export const UNKNOWN_VIEWER_FORMAT_LABEL = "Format";

/** What the extension cell says for a name that has none — `Makefile`,
 *  `.gitignore`, a file called `notes`. A word, not an empty cell: an empty
 *  cell reads as a rendering bug rather than as a fact about the file. */
export const UNKNOWN_VIEWER_NO_EXTENSION = "None";

/** What the size cell says when the surface did not supply a size. keeper does
 *  not go and stat the file to fill this in — the size arrives with the
 *  listing or it does not, and inventing a read here would make opening a
 *  placeholder more expensive than opening a real viewer. */
export const UNKNOWN_VIEWER_SIZE_UNKNOWN = "Unknown";

/**
 * The two actions, worded exactly as the Files pane words them.
 *
 * Spelled again rather than imported, on 43.5's terms: `files-pane.tsx` is a
 * layout component and this is a viewer, and one importing the other for a
 * string would couple two modules that share nothing else. One affordance, one
 * wording, said in the places that cannot reach each other.
 *
 * "Open in default app" rather than the epic's phrase "Open With": the command
 * behind it hands the file to the system's DEFAULT handler and offers no
 * chooser. A button labelled "Open With…" that never asks which is a button
 * that lies the first time somebody wants the other application.
 */
export const UNKNOWN_VIEWER_REVEAL_LABEL = "Reveal in Finder";
export const UNKNOWN_VIEWER_OPEN_LABEL = "Open in default app";

/**
 * The sentence that says why there is no rendering.
 *
 * Two different facts, so two different sentences: a format with no row at all
 * is one keeper has never claimed to show, and a row whose viewer has not been
 * bound is one keeper recognises and cannot draw yet. Telling a person their
 * PDF is an unknown file would be a small lie that costs them a bug report.
 */
export function unknownViewerSentence(entry: ViewerEntry): string {
  if (entry.viewer === "unknown") {
    return "keeper has no viewer for this format. It is still a file: reveal it, or hand it to the application that owns it.";
  }
  return `keeper recognises this as ${entry.label} but cannot show it here yet. Reveal it, or hand it to the application that owns it.`;
}

/** One fact of the placeholder: a label and the value beside it. */
function Fact({ label, slot, value }: { label: string; slot: string; value: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <dt className="label-caps text-faint">{label}</dt>
      <dd data-testid={slot} className="figures truncate text-sm">
        {value}
      </dd>
    </div>
  );
}

/**
 * What {@link UnknownViewer} renders — {@link ViewerProps} plus one optional
 * override.
 *
 * **Why the sentence became a prop in Story 45.7.** AD-91 makes "unknown" a
 * first-class kind whose viewer names the extension, states the size and offers
 * Reveal and Open With. A media file the platform will not decode wants exactly
 * that placeholder and a different sentence: keeper knows what the format is
 * and has a viewer for it, and the decoder said no. Rendering the stock
 * sentence there would tell the reader their `.mkv` is an unknown file, which
 * is a small lie that costs them a bug report; building a second placeholder
 * beside this one would give the two of them different facts within a release.
 *
 * Optional, so every existing caller is unchanged and the default is still the
 * sentence the row itself implies.
 */
export interface UnknownViewerProps extends ViewerProps {
  /** Why there is nothing to render, when the caller knows better than
   *  {@link unknownViewerSentence} does. */
  readonly reason?: string;
}

/** The placeholder for a file keeper will not — or cannot — render. */
export function UnknownViewer({ file, entry, reason }: UnknownViewerProps) {
  const canReveal = useCapabilitiesStore((state) => state.capabilities.revealInFileManager);
  const extension = extensionOf(file.name);
  const absolutePath = file.absolutePath;

  return (
    <section
      data-testid={UNKNOWN_VIEWER_TESTID}
      data-viewer={entry.viewer}
      data-format={entry.format}
      aria-label={file.name}
      className="flex min-w-0 flex-col gap-4 p-6"
    >
      <div className="min-w-0">
        <h2 className="truncate font-heading text-title">{file.name}</h2>
        {/* The relative path, which is the only path that may be rendered
            (FR-145). Empty for a file at the root of what is being browsed, in
            which case the name above has already said everything. */}
        {file.relativePath !== "" && (
          <p className="truncate text-muted-foreground text-sm">{file.relativePath}</p>
        )}
      </div>

      <dl className="flex flex-wrap gap-6">
        <Fact
          label={UNKNOWN_VIEWER_EXTENSION_LABEL}
          slot={UNKNOWN_VIEWER_EXTENSION_SLOT}
          value={extension === null ? UNKNOWN_VIEWER_NO_EXTENSION : `.${extension}`}
        />
        <Fact
          label={UNKNOWN_VIEWER_SIZE_LABEL}
          slot={UNKNOWN_VIEWER_SIZE_SLOT}
          value={file.sizeLabel ?? UNKNOWN_VIEWER_SIZE_UNKNOWN}
        />
        <Fact
          label={UNKNOWN_VIEWER_FORMAT_LABEL}
          slot={UNKNOWN_VIEWER_FORMAT_SLOT}
          value={entry.label}
        />
      </dl>

      <p className="text-muted-foreground text-sm">{reason ?? unknownViewerSentence(entry)}</p>

      <div className="flex flex-wrap gap-2">
        {file.openWith !== null && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => {
              void file.openWith?.().catch(() => undefined);
            }}
          >
            {UNKNOWN_VIEWER_OPEN_LABEL}
          </Button>
        )}
        {canReveal && absolutePath !== null && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => {
              void revealPath(absolutePath).catch(() => undefined);
            }}
          >
            {UNKNOWN_VIEWER_REVEAL_LABEL}
          </Button>
        )}
      </div>
    </section>
  );
}
