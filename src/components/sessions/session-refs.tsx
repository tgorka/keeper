/**
 * What a session points at (Phase 7, FR-255, AD-118).
 *
 * The tree above it lists what a session *holds*. This lists what it *names* —
 * a different set on purpose, because the zone's own rule is that big files
 * live in their zone and a session references them by repo-root-relative path.
 * So the thing that breaks is the pointer, and this is the only surface that
 * would ever say so.
 *
 * **Missing is the headline, not a badge.** A broken pointer sorts to the top
 * (Rust decides the order) and carries the sentence naming what keeper looked
 * for — because "keeper could not find it" sends somebody searching four
 * hundred folders, and naming the paths tells them the file is one `mv` away.
 * The heading states the count, so a session with nothing broken says so in a
 * word rather than making a person read thirty green rows to conclude it.
 *
 * **A list, and not the tree.** These rows have no nesting, no expansion and no
 * parent — a reference is flat by nature — and they are far fewer than a
 * session's files. So this is an ordinary list with ordinary tab stops rather
 * than a second ARIA tree with a second roving tabindex to keep in step.
 *
 * Every row's classification and target were decided in Rust: `panelTarget` is
 * the one file target already composed (AD-65, AD-109), `url` opens in the
 * system browser through the same `openUrl` every external link in the app
 * uses, and a `missing` row has neither because there is nothing to open.
 */
import { openUrl } from "@tauri-apps/plugin-opener";
import { AudioLines, ExternalLink, FileQuestion, FileText, Folders, Paperclip } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { SessionReferenceVm } from "@/lib/ipc/client";
import { panelsStore } from "@/lib/stores/panels";

/** The list's accessible name. */
export const SESSION_REFS_LABEL = "Session references";

/** The section heading — the zone's own word for these. */
export const SESSION_REFS_HEADING = "References";

/**
 * What a session that points at nothing says. Not a failure: plenty of good
 * sessions are self-contained, and an empty widget that vanished would read as
 * a surface that had not loaded.
 */
export const SESSION_REFS_EMPTY = "This session does not reference anything yet.";

/**
 * What a session with nothing broken says — stated, because "no missing rows"
 * is the answer a person opened this widget to get, and leaving it implicit
 * makes them read every row to find it.
 */
export const SESSION_REFS_ALL_RESOLVED = "Everything this session points at is where it says.";

/** How the count is worded when something IS broken. */
export function missingSummary(missing: number): string {
  return missing === 1
    ? "1 reference points at something that is not there."
    : `${missing} references point at something that is not there.`;
}

/** What a truncated scan says, naming the cause as the tree's own notice does. */
export const SESSION_REFS_TRUNCATED =
  "Too much text to scan it all — the rest of this session's references are not listed.";

/** One row, for tests that need to find one by target. */
export const SESSION_REFS_ROW_TESTID = "session-ref-row";

/**
 * The icon per kind. A table rather than a `switch`, the viewer registry's own
 * shape: adding a kind is a row.
 *
 * `missing` gets its own glyph rather than a red version of the file icon,
 * because the row's whole point is that keeper does not know what it is — it
 * never resolved to a file, so drawing it as one would be a guess.
 */
const KIND_ICON = {
  note: FileText,
  recording: AudioLines,
  file: Paperclip,
  session: Folders,
  external: ExternalLink,
  missing: FileQuestion,
} as const;

/** What each kind is called on the row — one word, the product's own. */
const KIND_LABEL = {
  note: "note",
  recording: "recording",
  file: "file",
  session: "session",
  external: "link",
  missing: "missing",
} as const;

function iconFor(kind: string) {
  return KIND_ICON[kind as keyof typeof KIND_ICON] ?? KIND_ICON.file;
}

function labelFor(kind: string) {
  return KIND_LABEL[kind as keyof typeof KIND_LABEL] ?? kind;
}

export interface SessionRefsProps {
  refs: SessionReferenceVm[];
  missing: number;
  truncated: boolean;
}

export function SessionRefs({ refs, missing, truncated }: SessionRefsProps) {
  if (refs.length === 0) {
    return <p className="px-2 text-muted-foreground text-xs">{SESSION_REFS_EMPTY}</p>;
  }

  return (
    <div className="flex flex-col gap-1">
      <p
        // A live region: the surface re-reads on the changed event, so an agent
        // moving a file turns this line from "everything resolves" into a count
        // without a keystroke — and that is exactly the moment worth announcing.
        role="status"
        className={
          missing > 0 ? "px-2 text-destructive text-xs" : "px-2 text-muted-foreground text-xs"
        }
      >
        {missing > 0 ? missingSummary(missing) : SESSION_REFS_ALL_RESOLVED}
      </p>
      <ul aria-label={SESSION_REFS_LABEL} className="flex flex-col">
        {refs.map((row) => (
          <SessionRefRow key={`${row.source}:${row.target}`} row={row} />
        ))}
      </ul>
      {truncated && (
        <p role="status" className="px-2 text-muted-foreground text-xs">
          {SESSION_REFS_TRUNCATED}
        </p>
      )}
    </div>
  );
}

function SessionRefRow({ row }: { row: SessionReferenceVm }) {
  const Icon = iconFor(row.kind);
  const missing = row.kind === "missing";
  // A row with nothing to open is not a button. A `missing` row's verb would be
  // "fix it in the file", which is not a click keeper can perform — so it reads
  // as text, and the sentence beneath it says where to go.
  const openable = row.panelTarget !== null || row.url !== null;

  const open = () => {
    if (row.url !== null) {
      // The app's one external-link path — never a raw `window.open`, which in
      // a webview would navigate the app itself.
      void openUrl(row.url).catch(() => {
        /* Best-effort: a browser that will not open is not this widget's error
           to report. */
      });
      return;
    }
    if (row.panelTarget !== null) {
      panelsStore.getState().setActiveTarget(row.panelTarget);
    }
  };

  const body = (
    <>
      <Icon
        aria-hidden="true"
        className={
          missing ? "size-4 shrink-0 text-destructive" : "size-4 shrink-0 text-muted-foreground"
        }
      />
      <span className="min-w-0 flex-1 truncate text-sm">{row.label}</span>
      {/* The kind and the source describe the row; neither is part of its name,
          so a screen reader announces the reference and then the facts about it
          (the tree's own finding). */}
      <span className="shrink-0 text-muted-foreground text-xs">{labelFor(row.kind)}</span>
      <span className="shrink-0 truncate text-muted-foreground text-xs">{row.source}</span>
    </>
  );

  return (
    <li data-testid={`${SESSION_REFS_ROW_TESTID}-${row.target}`} className="flex flex-col">
      {openable ? (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 min-w-0 justify-start gap-2 px-2 font-normal"
          onClick={open}
        >
          {body}
        </Button>
      ) : (
        <div className="flex min-w-0 items-center gap-2 px-2 py-1">{body}</div>
      )}
      {row.notice !== null && (
        <p className="px-2 pb-1 pl-8 text-muted-foreground text-xs">{row.notice}</p>
      )}
    </li>
  );
}
