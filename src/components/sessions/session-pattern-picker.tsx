/**
 * What the new session is shaped from, and what that means (FR-253, AD-116).
 *
 * Before this, a session was born from the zone's `_template/` and nothing
 * else — unless you happened to find "New like this" on a row's overflow menu,
 * which was a second door to the same room. There is one door now: the create
 * row asks the one question it always asked (the title) and, beside it, what
 * to shape the session from. The zone template sits first because it is the
 * zone's own answer; every session follows, newest change first, because the
 * pattern you want is nearly always the one you were just working in.
 *
 * The preview under the field is the point. Copying a session takes its
 * prompts and its ref pointers and deliberately leaves its artifacts, its
 * workspace and its prose behind — a rule that is correct and, unexplained,
 * looks exactly like a bug ("where did my report go"). So the preview names
 * what travels AND what stays, each with the domain's own sentence, and both
 * halves come from the same `pattern::apply` value the plan is compiled from.
 * The list is not a description of the copy; it IS the copy, projected.
 *
 * Nothing here decides anything: the shell computed both halves, this renders
 * them (AD-7).
 */
import { ArrowRight, Minus } from "lucide-react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { formatDraftAge } from "@/lib/format-time";
import type { SessionPatternVm } from "@/lib/ipc/client";

/** The picker's accessible name — the question it asks. */
export const SESSION_PATTERN_LABEL = "Start from";

/** The preview's two headings, stated as consequences rather than as nouns. */
export const SESSION_PATTERN_COPIES_LABEL = "Copies";
export const SESSION_PATTERN_SKIPS_LABEL = "Leaves behind";

/** A pattern that carries nothing — honest, and not an error. */
export const SESSION_PATTERN_EMPTY_LABEL = "Nothing to copy — the session starts empty.";

/** The one-line state while the root's patterns are being read. */
export const SESSION_PATTERN_LOADING_LABEL = "Reading patterns…";

export interface SessionPatternPickerProps {
  /** Every pattern the root offers, shell-ordered; `null` while loading. */
  patterns: SessionPatternVm[] | null;
  /** The chosen pattern's id, or `null` before the list resolves. */
  value: string | null;
  onChange: (patternId: string) => void;
  /** Injected for tests; the relative ages are cosmetic. */
  nowMs?: number;
}

export function SessionPatternPicker({
  patterns,
  value,
  onChange,
  nowMs = Date.now(),
}: SessionPatternPickerProps) {
  if (patterns === null) {
    return (
      <p role="status" className="text-muted-foreground text-xs">
        {SESSION_PATTERN_LOADING_LABEL}
      </p>
    );
  }
  // A zone with neither a template nor a session has nothing to pick between,
  // and a select with one unchosen option is furniture. Create still works —
  // the empty skeleton is what a first session in a fresh zone should be.
  if (patterns.length === 0) {
    return null;
  }

  const chosen = patterns.find((pattern) => pattern.id === value) ?? null;

  return (
    <div className="flex flex-col gap-2">
      <Select value={chosen?.id} onValueChange={onChange}>
        <SelectTrigger className="w-full" aria-label={SESSION_PATTERN_LABEL}>
          <SelectValue placeholder={SESSION_PATTERN_LABEL} />
        </SelectTrigger>
        <SelectContent>
          {patterns.map((pattern) => (
            <SelectItem key={pattern.id} value={pattern.id}>
              <span className="flex min-w-0 items-baseline gap-2">
                <span className="truncate">{pattern.label}</span>
                <span className="shrink-0 text-muted-foreground text-xs">
                  {pattern.mtimeMs === null
                    ? pattern.detail
                    : formatDraftAge(pattern.mtimeMs, nowMs)}
                </span>
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {chosen !== null && <SessionPatternPreview pattern={chosen} />}
    </div>
  );
}

/** The chosen pattern's consequence, in the zone's own words. */
function SessionPatternPreview({ pattern }: { pattern: SessionPatternVm }) {
  const files = pattern.copies.filter((entry) => !entry.isDir);
  return (
    <div className="flex flex-col gap-1 rounded-md bg-muted/40 px-3 py-2">
      <p className="text-muted-foreground text-xs">{pattern.detail}</p>
      {files.length === 0 && pattern.skips.length === 0 ? (
        <p className="text-muted-foreground text-xs">{SESSION_PATTERN_EMPTY_LABEL}</p>
      ) : (
        <>
          {files.length > 0 && (
            <ul aria-label={SESSION_PATTERN_COPIES_LABEL} className="flex flex-col gap-0.5">
              {files.map((entry) => (
                <li key={entry.relPath} className="flex items-baseline gap-1.5 text-xs">
                  <ArrowRight aria-hidden className="size-3 shrink-0 self-center" />
                  <span className="truncate font-mono">{entry.relPath}</span>
                </li>
              ))}
            </ul>
          )}
          {pattern.skips.length > 0 && (
            <ul
              aria-label={SESSION_PATTERN_SKIPS_LABEL}
              className="flex flex-col gap-0.5 text-muted-foreground"
            >
              {pattern.skips.map((skip) => (
                <li key={skip.relPath} className="flex items-baseline gap-1.5 text-xs">
                  <Minus aria-hidden className="size-3 shrink-0 self-center" />
                  <span className="truncate font-mono">{skip.relPath}</span>
                  <span className="min-w-0 truncate">— {skip.reason}</span>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}
