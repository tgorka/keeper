/**
 * What the model was told about your drive before you asked it anything (Epic
 * 61, Story 61.11, FR-390, FR-391, NFR-48).
 *
 * # Why this is a disclosure and not a detail
 *
 * A conversation that answers well because `AGENTS.md` told it your folder
 * conventions, and a conversation that answers badly because `AGENTS.md` was
 * dropped by the context budget, look identical from the outside. FR-391 asks
 * that the context files be shown to the user **as what the model was told**,
 * so this renders the very
 * [`ContextBundle`](../../lib/ipc/gen/BotContextBundleVm.ts) the system prompt
 * was built from: the preamble verbatim, every file in the order the prompt put
 * them, each one's contributed size, and — the part that actually changes an
 * answer — every file that was found and left out.
 *
 * **A silently dropped instruction file is worse than none.** With no
 * `AGENTS.md` a person expects a generic answer; with an `AGENTS.md` the budget
 * dropped they expect a specific one and get a generic one, and nothing on
 * screen explains the difference. So an over-budget skip is rendered under its
 * own heading with `keeper-core`'s own sentence, and never folded in with an
 * empty file, which is news about nothing.
 *
 * # Order is a fact, so it is rendered as one
 *
 * `files` arrives in **render** order — root first, nearest last — which is the
 * order the model met them in and the reason the most specific instruction
 * wins. An ordered list is that claim in the DOM; re-sorting it here for
 * tidiness would make this pane describe a prompt nobody sent.
 *
 * # Absent, not empty, and never guessed
 *
 * `bundle === null` is *unknown* — no turn has been sent, or the shell has not
 * said — and unknown renders nothing (AD-27: a capability keeper could not read
 * is never `false`). A bundle that exists and found nothing is a different
 * fact, and says so in a sentence.
 */
import { ChevronDown, ChevronRight } from "lucide-react";
import { useId, useState } from "react";
import { formatFileSize } from "@/lib/file-size";
import type { BotContextBundleVm } from "@/lib/ipc/client";

/** The disclosure's own name. */
export const BOT_CONTEXT_TITLE = "What the model was told about your drive";

/** What the two orders mean, stated where the order is shown. */
export const BOT_CONTEXT_ORDER_NOTE =
  "In the order the model was given them: the folder root first, the folders closest to your question last.";

/** The heading over the files that made it. */
export const BOT_CONTEXT_FILES_LABEL = "Files included";

/** The heading over the files that did not. */
export const BOT_CONTEXT_SKIPPED_LABEL = "Found and left out";

/** What a bundle that found nothing says. */
export const BOT_CONTEXT_NONE_FOUND =
  "keeper found no instruction files here, so the model was told nothing about how this folder works.";

/** What the sentence separating instruction from data is called on screen. */
export const BOT_CONTEXT_PREAMBLE_LABEL = "And it was told this about them";

/** The count and size of what the prompt carried. */
export function botContextSummary(bundle: BotContextBundleVm): string {
  const files = bundle.files.length === 1 ? "1 drive file" : `${bundle.files.length} drive files`;
  return `${files}, ${formatFileSize(bundle.totalBytes)}`;
}

/** One file's contribution, and whether the budget cut it short. */
export function botContextFileLine(file: {
  bytes: number;
  ofBytes: number;
  truncated: boolean;
}): string {
  if (file.truncated) {
    return `the first ${formatFileSize(file.bytes)} of ${formatFileSize(file.ofBytes)}`;
  }
  return formatFileSize(file.bytes);
}

/** The context disclosure for one turn. */
export function BotContextNote({ bundle }: { bundle: BotContextBundleVm | null }) {
  const [open, setOpen] = useState(false);
  const bodyId = useId();
  if (bundle === null) {
    // Unknown, which is not the same as none — so nothing is claimed.
    return null;
  }
  const empty = bundle.files.length === 0 && bundle.skipped.length === 0;
  return (
    <div className="flex min-w-0 flex-col gap-0.5" data-slot="bot-context-note">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={open ? bodyId : undefined}
        onClick={() => setOpen((shown) => !shown)}
        className="flex min-w-0 items-start gap-1 text-left text-muted-foreground text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
      >
        {open ? (
          <ChevronDown aria-hidden="true" className="mt-0.5 size-3 shrink-0" />
        ) : (
          <ChevronRight aria-hidden="true" className="mt-0.5 size-3 shrink-0" />
        )}
        <span className="min-w-0 break-words">
          {BOT_CONTEXT_TITLE}
          {!empty && <span className="figures"> — {botContextSummary(bundle)}</span>}
        </span>
      </button>
      {open && (
        <div className="flex min-w-0 flex-col gap-1 pl-4" id={bodyId}>
          {empty && <p className="text-muted-foreground text-xs">{BOT_CONTEXT_NONE_FOUND}</p>}
          {bundle.files.length > 0 && (
            <>
              <p className="text-muted-foreground text-xs">{BOT_CONTEXT_FILES_LABEL}</p>
              <p className="text-muted-foreground text-xs">{BOT_CONTEXT_ORDER_NOTE}</p>
              <ol className="flex min-w-0 flex-col gap-0.5" data-slot="bot-context-files">
                {bundle.files.map((file) => (
                  <li className="min-w-0 break-words text-xs" key={file.subpath}>
                    <span className="font-mono">{file.subpath}</span>
                    <span className="figures text-muted-foreground">
                      {" — "}
                      {botContextFileLine(file)}
                    </span>
                  </li>
                ))}
              </ol>
            </>
          )}
          {bundle.skipped.length > 0 && (
            <>
              <p className="text-muted-foreground text-xs">{BOT_CONTEXT_SKIPPED_LABEL}</p>
              <ul className="flex min-w-0 flex-col gap-0.5" data-slot="bot-context-skipped">
                {bundle.skipped.map((skip) => (
                  // `keeper-core`'s own sentence, verbatim, and `data-over-budget`
                  // so a dropped instruction file is distinguishable from an
                  // empty one by more than the words it happens to use.
                  <li
                    className="min-w-0 break-words text-muted-foreground text-xs"
                    data-over-budget={skip.overBudget ? "true" : "false"}
                    key={skip.subpath}
                  >
                    {skip.sentence}
                  </li>
                ))}
              </ul>
            </>
          )}
          {bundle.files.length > 0 && (
            <>
              <p className="text-muted-foreground text-xs">{BOT_CONTEXT_PREAMBLE_LABEL}</p>
              {/* The preamble verbatim: the surface and the prompt quote one
                  `const`, so the two can never say different things. */}
              <p className="break-words text-muted-foreground text-xs">{bundle.preamble}</p>
            </>
          )}
        </div>
      )}
    </div>
  );
}
