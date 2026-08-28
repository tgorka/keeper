/**
 * Content search over notes and sessions (FR-267).
 *
 * The sibling of {@link import("./search-panel").SearchPanel}, not a mode of it.
 * Messages are an FTS query against the archive that returns once; a note or a
 * session file is a bounded walk of a folder that *streams*, so the two have a
 * different shape of state (a sequence guard and one result set vs. a growing
 * list and a running flag) and a different empty state ("no matches in your
 * archive" vs. "no vault is open"). Folding them into one component would mean a
 * union of both states with half of it inert on every render.
 *
 * What they share is the affordance: a query field, a grouped list, the same
 * tinted match (`HighlightedBody`, imported rather than re-implemented), and
 * activation that opens the document and closes the surface.
 *
 * Activation switches the primary view as well as the panel target, because the
 * panel strip only exists beside Files, Notes and Sessions (`app-shell.tsx`):
 * setting a target from a search opened over the inbox would otherwise write to
 * a strip nothing is rendering, and the click would look ignored.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { HighlightedBody } from "@/components/search/search-result-list";
import { InputGroup, InputGroupInput } from "@/components/ui/input-group";
import { useNotesSearch } from "@/hooks/use-notes-search";
import { useSessionsSearch } from "@/hooks/use-sessions-search";
import { useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { panelsStore } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import type { SearchSource } from "@/lib/stores/search";
import { useSessionsRootsStore } from "@/lib/stores/sessions-roots";
import { cn } from "@/lib/utils";

/** Testid prefix for one result row, suffixed with its flat index. */
export const DOCUMENT_SEARCH_ROW_TESTID = "document-search-row";

export interface DocumentSearchPanelProps {
  /** Which store to search. `"messages"` is not this panel's business. */
  source: Exclude<SearchSource, "messages">;
  /**
   * Whether the owning surface is open. A rising edge (`false` → `true`) clears
   * the query so results never leak across opens; while `false` no scan starts,
   * which matters more here than for messages — a walk left running would be a
   * folder read funding an answer nobody can see.
   */
  active: boolean;
  /** Close the owning surface (activation closes it). */
  onClose: () => void;
  /** Extra classes for the results scroll region. */
  resultsClassName?: string;
}

/**
 * One rendered row, flattened out of whichever hit shape produced it.
 *
 * The two searches return different VMs and this panel renders one list, so the
 * projection happens once per source rather than in the JSX: a row is a heading
 * (which document), a line number, a snippet, and the click that opens it.
 */
interface Row {
  key: string;
  /** The document this hit is in — a note title, or `<session> · <file>`. */
  heading: string;
  line: number;
  snippet: string;
  open: () => void;
}

export function DocumentSearchPanel({
  source,
  active,
  onClose,
  resultsClassName,
}: DocumentSearchPanelProps) {
  const [query, setQuery] = useState("");
  const vaultId = useNotesVaultsStore((s) => s.activeVaultId);
  const rootId = useSessionsRootsStore((s) => s.activeRootId);

  // Clear on open, the `SearchPanel` rule: results are never held in a store and
  // must not survive a close. Switching *source* deliberately does not clear —
  // the query is the thing you are carrying from one store to the next.
  useEffect(() => {
    if (active) {
      setQuery("");
    }
  }, [active]);

  // Only the selected source scans. Passing `null` for the other one is what
  // keeps this to one walk: the hooks treat a null id as "nothing to search"
  // and never call, so the inactive source costs a render, not a folder read.
  const notes = useNotesSearch(source === "notes" && active ? vaultId : null, query);
  const sessions = useSessionsSearch(source === "sessions" && active ? rootId : null, query);

  const activateNote = useCallback(
    (noteId: string) => {
      if (vaultId === null) {
        return;
      }
      primaryViewStore.getState().setView("notes");
      panelsStore.getState().setActiveTarget({ kind: "note", vaultId, noteId });
      onClose();
    },
    [vaultId, onClose],
  );

  const activateFile = useCallback(
    (subpath: string) => {
      if (rootId === null) {
        return;
      }
      primaryViewStore.getState().setView("sessions");
      // `subpath` is profile-relative and composed in Rust (AD-65) — the same
      // string every session file row opens with, so a hit and a row reach the
      // one editor by the one path.
      panelsStore
        .getState()
        .setActiveTarget({ kind: "file", profileId: rootId, relativePath: subpath });
      onClose();
    },
    [rootId, onClose],
  );

  const rows = useMemo<Row[]>(() => {
    if (source === "notes") {
      return notes.hits.map((hit) => ({
        key: `${hit.id}:${hit.line}`,
        heading: hit.title,
        line: hit.line,
        snippet: hit.snippet,
        open: () => activateNote(hit.id),
      }));
    }
    return sessions.hits.map((hit) => ({
      key: `${hit.subpath}:${hit.line}`,
      // The session first because that is the question a flat pool makes hard:
      // twelve sessions each hold an `about.md`, and the path alone answers
      // "which file" while leaving "which session" unanswered.
      heading: `${hit.sessionTitle} · ${hit.file}`,
      line: hit.line,
      snippet: hit.snippet,
      open: () => activateFile(hit.subpath),
    }));
  }, [source, notes.hits, sessions.hits, activateNote, activateFile]);

  const running = source === "notes" ? notes.running : sessions.running;
  const error = source === "notes" ? notes.error : sessions.error;
  const scopeId = source === "notes" ? vaultId : rootId;

  // Never an empty box (UX-DR44): each state says which one it is, and the
  // "nothing to search" case names the missing thing rather than showing an
  // empty result list that looks like a search that found nothing.
  let body: React.ReactNode = null;
  if (scopeId === null) {
    body = (
      <p className="p-3 text-muted-foreground text-sm">
        {source === "notes"
          ? "No vault is open — open one to search your notes."
          : "No sessions zone is open — open one to search your sessions."}
      </p>
    );
  } else if (error !== null) {
    body = (
      <div role="alert" className="rounded-md bg-destructive/10 p-3 text-destructive text-sm">
        <p>Search failed: {error}</p>
      </div>
    );
  } else if (rows.length > 0) {
    body = (
      <div className="flex flex-col gap-1" role="listbox" aria-label="Search results">
        {rows.map((row, index) => (
          <button
            type="button"
            key={row.key}
            role="option"
            aria-selected={false}
            data-testid={`${DOCUMENT_SEARCH_ROW_TESTID}-${index}`}
            onClick={row.open}
            className={cn(
              "flex flex-col items-start gap-0.5 rounded-md px-2 py-1.5 text-left text-sm",
              "hover:bg-accent focus:outline-none",
            )}
          >
            <span className="w-full truncate font-medium text-foreground">{row.heading}</span>
            <span className="w-full break-words text-muted-foreground">
              <HighlightedBody body={row.snippet} query={query} />
            </span>
            <span className="text-meta text-muted-foreground">Line {row.line}</span>
          </button>
        ))}
      </div>
    );
  } else if (running) {
    body = <p className="p-3 text-muted-foreground text-sm">Searching…</p>;
  } else if (query.trim() !== "") {
    body = (
      <p className="p-3 text-muted-foreground text-sm">
        {source === "notes" ? "No matches in this vault." : "No matches in this zone."}
      </p>
    );
  }

  return (
    <div className="flex min-h-0 flex-col gap-3">
      <InputGroup>
        <InputGroupInput
          autoFocus
          placeholder={source === "notes" ? "Search notes" : "Search sessions"}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Search query"
        />
      </InputGroup>
      <div className={cn("max-h-[50vh] overflow-y-auto", resultsClassName)}>{body}</div>
    </div>
  );
}
