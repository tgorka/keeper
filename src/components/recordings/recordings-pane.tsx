/**
 * The Recordings primary view — a browser over every session keeper has ever
 * recorded (Story 42.3, Epic 42).
 *
 * 42.1 made a session a row and 42.2 made it findable; neither was reachable by
 * a person. This is the surface that closes that: one query into 42.2's
 * `search_recordings` engine, and rows that get you out of keeper and into the
 * file.
 *
 * **Search first.** The filter row sits above the fold, not behind a
 * disclosure, because the answer to "where is that session" is a query, not a
 * scroll. An empty query is a legitimate query here — the engine reads it as
 * "no text predicate at all" — so the surface lists on mount and narrows as you
 * type, rather than making you type something before it will admit the archive
 * exists.
 *
 * **Debounced, and stale-guarded.** {@link DEBOUNCE_MS} of quiet before a
 * keystroke reaches Rust (each surface declares its own constant; there is no
 * shared hook), and a monotonic sequence so a slow query resolving after a
 * newer one is discarded rather than allowed to win. Both guards are the ones
 * `search-panel.tsx` established, for the same two reasons: one round trip per
 * word instead of per keystroke, and a result list that always answers the
 * question currently on screen.
 *
 * **Two empty states, two sentences.** "Nothing recorded yet" and "nothing
 * matches this filter" produce the same empty list and mean opposite things;
 * {@link RecordingsEmptyState} keeps them apart, and the filter chips stay on
 * screen above the second one so the one chip that went too far is still there
 * to remove.
 *
 * **Capability gating is absence.** This pane is rendered only where the
 * `recording` capability is on — gated upstream at the nav entry
 * (`sidebar-pane.tsx`) and at the render chain (`app-shell.tsx`), the same
 * three layers `RecordingPane` is gated at. A browser for recordings you cannot
 * make is not a surface, it is a puzzle. Nothing here sniffs the platform; the
 * flag is Rust's answer, mirrored in the capabilities store.
 *
 * This surface reads. There is no write path of any kind, no media player (Play
 * hands the file to the system handler and stops caring), and no tag
 * normalisation — Story 42.5 put that in Rust, at the boundary where a
 * recording's tags enter the index, so what arrives here is already the one
 * vocabulary and re-shaping it would only be a way to disagree with the tree.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { RecordingRow } from "@/components/recordings/recording-row";
import {
  type RecordingsEmptyKind,
  RecordingsEmptyState,
} from "@/components/recordings/recordings-empty-state";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { useWindowedRows } from "@/components/ui/window-list";
import { countLabel, SESSIONS } from "@/lib/count-label";
import type { IpcError, RecordingFilterVm, RecordingHitVm } from "@/lib/ipc/client";
import { recordingOpenPath, revealPath, searchRecordings } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { primaryViewStore } from "@/lib/stores/primary-view";

/** Debounce (ms) before a filter change fires `searchRecordings`. */
const DEBOUNCE_MS = 200;

/**
 * The height a recordings row is assumed to be until it has been mounted once,
 * and the space between two of them.
 *
 * An assumption, not a fact, and that is the point: a row grows a third line
 * when it has enough tags to wrap the badges, and a fourth where the platform
 * has no Finder and the path renders as text instead. The window measures what
 * a row really is on first mount; this is only what it starts from.
 */
const RECORDING_ROW_ESTIMATE = 60;
const RECORDING_ROW_GAP = 8;

/** The pane's heading, and the accessible name of the surface itself. */
export const RECORDINGS_PANE_TITLE = "Recordings";

/** The one honest sentence under the heading: what this searches, and where. */
export const RECORDINGS_PANE_SUBTITLE =
  "Every session you have recorded on this Mac, searchable offline.";

/** The accessible name of the result list (distinct from the pane's own name). */
export const RECORDINGS_LIST_LABEL = "Recording sessions";

/** The header control that re-runs the current query against the archive. */
export const RECORDINGS_REFRESH_LABEL = "Refresh";

/**
 * Test id for the line that says how many sessions the filter found (Story
 * 44.11). A slot, so a test asserts the number rather than the sentence.
 */
export const RECORDINGS_COUNT_SLOT = "recordings-count";

/**
 * The durability words the archive column can hold, and how the filter names
 * them. Epic 41's wire spelling goes to Rust; epic 41's own words go on screen,
 * so the filter and the row's glyph cannot describe the same state differently.
 */
const DURABILITY_CHOICES: { value: string; label: string }[] = [
  { value: "local", label: "on this Mac" },
  { value: "committed", label: "committed" },
  { value: "pushed", label: "on the drive" },
  { value: "verified", label: "verified" },
];

/** Structural guard for the IpcError envelope surfaced on a search rejection. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  // Narrowed to a plain object above; the assertion only says "read its keys as
  // unknown", which is exactly what the two `typeof` checks below then verify.
  const v = value as Record<string, unknown>;
  return typeof v.code === "string" && typeof v.message === "string";
}

export function RecordingsPane() {
  // A platform with no user-visible file manager gets no Reveal affordance —
  // the row renders the path as inert text instead (Story 42.3 matrix).
  const canReveal = useCapabilitiesStore((s) => s.capabilities.revealInFileManager);

  const [query, setQuery] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [participant, setParticipant] = useState("");
  const [startDate, setStartDate] = useState<string | null>(null);
  const [endDate, setEndDate] = useState<string | null>(null);
  const [durability, setDurability] = useState<string | null>(null);
  const [hits, setHits] = useState<RecordingHitVm[]>([]);
  // How many sessions the filter matches in the whole archive, which is NOT
  // `hits.length` (Story 44.11): the engine's page stops at 200, so an archive
  // of nine thousand and one of exactly two hundred both hand back two hundred
  // rows. Zero until a query lands, and shown only once one has.
  const [total, setTotal] = useState(0);
  const [error, setError] = useState<IpcError | null>(null);
  // Whether a query has actually landed. An empty list is not yet "empty" while
  // the first answer is in flight, and neither empty-state sentence is true of
  // an archive nobody has read.
  const [loaded, setLoaded] = useState(false);

  // Monotonic request sequence: a response is applied only if it is the newest
  // dispatched — an older (superseded) response is discarded.
  const seqRef = useRef(0);

  const filter = useMemo<RecordingFilterVm>(
    () => ({
      query: query.trim(),
      tags,
      participant: participant.trim() === "" ? null : participant.trim(),
      // A calendar day is a day in the user's timezone, and the bound is
      // inclusive at both ends: "to the 4th" means through the end of the 4th,
      // not up to midnight at its start.
      startTs: startDate === null ? null : new Date(`${startDate}T00:00:00`).getTime(),
      endTs: endDate === null ? null : new Date(`${endDate}T23:59:59.999`).getTime(),
      durability,
      // No profile filter on this surface: the browser is about sessions, and
      // which destination root holds one is not a question anyone browsing asks.
      profileId: null,
      // `null` is the engine's DEFAULT_LIMIT. The surface does not second-guess it.
      limit: null,
    }),
    [query, tags, participant, startDate, endDate, durability],
  );

  // One dispatch, shared by the debounced effect and the Refresh control, so
  // both go through the same stale guard. `useCallback` with no dependencies:
  // it closes over nothing but the setters and the ref, and the filter it is to
  // run arrives as an argument.
  const runSearch = useCallback((current: RecordingFilterVm) => {
    seqRef.current += 1;
    const seq = seqRef.current;
    searchRecordings(current)
      .then((result) => {
        // Discard a superseded (out-of-order) response.
        if (seq !== seqRef.current) {
          return;
        }
        setHits(result.rows);
        setTotal(result.total);
        setError(null);
        setLoaded(true);
      })
      .catch((e: unknown) => {
        if (seq !== seqRef.current) {
          return;
        }
        setHits([]);
        setTotal(0);
        setError(
          isIpcError(e)
            ? e
            : { code: "internal", message: String(e), accountId: null, retriable: false },
        );
        setLoaded(true);
      });
  }, []);

  useEffect(() => {
    const handle = window.setTimeout(() => runSearch(filter), DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [filter, runSearch]);

  // Tag choices are seeded from the current result set, the way the message
  // search seeds its sender suggestions: the tags that co-occur with what is on
  // screen are exactly the tags that can narrow it further, and a global list
  // would offer chips that can only ever empty the list.
  const tagChoices = useMemo(
    () => [...new Set(hits.flatMap((h) => h.tags))].filter((t) => !tags.includes(t)).sort(),
    [hits, tags],
  );

  const filtered =
    query.trim() !== "" ||
    tags.length > 0 ||
    participant.trim() !== "" ||
    startDate !== null ||
    endDate !== null ||
    durability !== null;

  const clearFilters = useCallback(() => {
    setQuery("");
    setTags([]);
    setParticipant("");
    setStartDate(null);
    setEndDate(null);
    setDurability(null);
  }, []);

  // "Nothing recorded yet" and "nothing matches this filter" are opposite facts
  // about the same empty list, and neither is true until a query has landed.
  let emptyKind: RecordingsEmptyKind | null = null;
  if (loaded && error === null && hits.length === 0) {
    emptyKind = filtered ? "no-matches" : "no-recordings";
  }

  // Keyed by session id, so a re-query that re-orders the archive carries each
  // row's measured height with the row rather than leaving the previous
  // occupant's height at that position.
  const getKey = useCallback((index: number) => hits[index]?.sessionId ?? String(index), [hits]);
  const list = useWindowedRows({
    count: hits.length,
    getKey,
    rowHeight: RECORDING_ROW_ESTIMATE,
    gap: RECORDING_ROW_GAP,
  });

  return (
    <section
      aria-label={RECORDINGS_PANE_TITLE}
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background"
    >
      <header className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-6 py-4">
        <div className="min-w-0">
          <h1 className="font-heading font-medium text-lg">{RECORDINGS_PANE_TITLE}</h1>
          <p className="text-muted-foreground text-sm">{RECORDINGS_PANE_SUBTITLE}</p>
          {/* How many sessions the filter found (Story 44.11, FR-166).

              In the header, which is rendered in every state, so an archive
              that matches nothing says `0 sessions` instead of dropping the
              count exactly when the reader most wants to know it was asked.
              Suppressed only before the first answer lands: `0` before a query
              has run is a claim nobody has checked yet. */}
          {loaded && (
            <p
              role="status"
              data-slot={RECORDINGS_COUNT_SLOT}
              className="text-muted-foreground text-xs"
            >
              {countLabel(total, SESSIONS)}
            </p>
          )}
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          // An explicit press asks now, undebounced: a session recorded while
          // this pane was open lands in `archive.db` without telling anyone, and
          // every query opens a fresh read-only connection — so re-asking is the
          // whole of "it appears without a restart".
          onClick={() => runSearch(filter)}
        >
          {RECORDINGS_REFRESH_LABEL}
        </Button>
      </header>

      <div className="flex shrink-0 flex-col gap-2 border-border border-b px-6 py-3">
        {/* The filter row, above the fold. Query and the date range share the
            input group the way message search does; the enumerable choices are
            menus; everything selected becomes a removable chip below. */}
        <InputGroup>
          <InputGroupInput
            placeholder="Search recordings"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label="Search recordings"
          />
          <InputGroupAddon align="inline-end">
            <input
              type="date"
              aria-label="Start date"
              className="bg-transparent text-muted-foreground text-xs outline-none"
              value={startDate ?? ""}
              onChange={(e) => setStartDate(e.target.value === "" ? null : e.target.value)}
            />
            <input
              type="date"
              aria-label="End date"
              className="bg-transparent text-muted-foreground text-xs outline-none"
              value={endDate ?? ""}
              onChange={(e) => setEndDate(e.target.value === "" ? null : e.target.value)}
            />
          </InputGroupAddon>
        </InputGroup>

        <div className="flex flex-wrap items-center gap-2">
          <input
            type="text"
            placeholder="Participant"
            aria-label="Participant"
            className="h-7 rounded-md border border-input bg-transparent px-2 text-xs outline-none"
            value={participant}
            onChange={(e) => setParticipant(e.target.value)}
          />

          {tagChoices.length > 0 && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="h-7 rounded-md border border-input px-2 text-muted-foreground text-xs"
                >
                  Tag
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent className="max-h-64 overflow-y-auto">
                {tagChoices.map((tag) => (
                  <DropdownMenuItem
                    key={tag}
                    // Several tags AND together, the way the notes surface's tag
                    // chips do: two chips narrow, they do not widen.
                    onSelect={() => setTags((current) => [...current, tag])}
                  >
                    {tag}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          )}

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                className="h-7 rounded-md border border-input px-2 text-muted-foreground text-xs"
              >
                Durability
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent>
              {DURABILITY_CHOICES.map((choice) => (
                <DropdownMenuItem key={choice.value} onSelect={() => setDurability(choice.value)}>
                  {choice.label}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {/* Active filter chips, each one-tap removable. They stay on screen over
            the "nothing matches" state — the way out of it is here. */}
        {filtered && (
          <div className="flex flex-wrap items-center gap-1.5">
            {tags.map((tag) => (
              <RemovableChip
                key={tag}
                label={`Tag: ${tag}`}
                onRemove={() => setTags((current) => current.filter((t) => t !== tag))}
              />
            ))}
            {participant.trim() !== "" && (
              <RemovableChip
                label={`Participant: ${participant}`}
                onRemove={() => setParticipant("")}
              />
            )}
            {startDate !== null && (
              <RemovableChip label={`From: ${startDate}`} onRemove={() => setStartDate(null)} />
            )}
            {endDate !== null && (
              <RemovableChip label={`To: ${endDate}`} onRemove={() => setEndDate(null)} />
            )}
            {durability !== null && (
              <RemovableChip
                label={`Durability: ${
                  DURABILITY_CHOICES.find((c) => c.value === durability)?.label ?? durability
                }`}
                onRemove={() => setDurability(null)}
              />
            )}
          </div>
        )}
      </div>

      <div {...list.viewportProps} className="min-h-0 flex-1 overflow-y-auto">
        <div className="flex min-h-0 flex-col gap-2 p-6">
          {error !== null ? (
            <div role="alert" className="rounded-md bg-destructive/10 p-3 text-destructive text-sm">
              <p>Could not read your recordings: {error.message}</p>
              {error.retriable && (
                <p className="text-muted-foreground text-xs">
                  This is usually temporary — try again.
                </p>
              )}
            </div>
          ) : emptyKind !== null ? (
            <RecordingsEmptyState
              kind={emptyKind}
              onAction={
                emptyKind === "no-recordings"
                  ? () => primaryViewStore.getState().setView("recording")
                  : clearFilters
              }
            />
          ) : (
            <ul
              aria-label={RECORDINGS_LIST_LABEL}
              className="relative w-full"
              style={{ height: `${list.totalSize}px` }}
            >
              {list.rows.map((row) => {
                const hit = hits[row.index];
                if (hit === undefined) {
                  return null;
                }
                return (
                  <li key={hit.sessionId} {...list.rowProps(row)}>
                    <RecordingRow
                      hit={hit}
                      canReveal={canReveal}
                      onReveal={(h) => {
                        // The absolute path Rust resolved for this session as it
                        // is stored RIGHT NOW: story 40.4 moves folders on a
                        // retitle and 42.1's row follows the session, so the
                        // row's path is the current one and Reveal never points
                        // at where it used to be.
                        void revealPath(h.absolutePath).catch(() => {});
                      }}
                      onPlay={(h) => {
                        if (h.playablePath === null) {
                          return;
                        }
                        void recordingOpenPath(h.playablePath).catch(() => {});
                      }}
                    />
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </div>
    </section>
  );
}

/** A one-tap-removable filter chip (the message-search surface's, in its shape). */
function RemovableChip({ label, onRemove }: { label: string; onRemove: () => void }) {
  return (
    <Badge variant="secondary" className="gap-1">
      {label}
      <button
        type="button"
        onClick={onRemove}
        aria-label={`Remove ${label}`}
        className="ml-0.5 rounded-full text-muted-foreground hover:text-foreground"
      >
        ×
      </button>
    </Badge>
  );
}
