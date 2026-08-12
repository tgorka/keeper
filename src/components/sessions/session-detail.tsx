/**
 * The session detail (Phase 7, FR-233, UX-DR89): the drill-in a board row
 * opens — header with tags, the properties widget, lineage chips, the
 * rendered activity log, and the mini-file sections.
 *
 * A review surface, so the ordering choices all point one way: the log
 * renders NEWEST FIRST (the file on disk keeps the zone's newest-last
 * convention — only this projection reverses), and every file section sorts
 * by mtime descending. Everything shown is a fresh projection of the zone's
 * files (`sessions_detail`), re-read on the changed event — an agent writing
 * on disk moves this view live.
 *
 * Files open in the panel strip beside the board through the SAME file
 * target the Files pane sets (AD-109, UX-DR91) — one editor, one viewer
 * registry, no second open path. `workspace/` rows carry the zone's own
 * caveat and open read-only; the write fence in Rust is what enforces it.
 */
import { ArrowLeft, FileText, Folder, Lock, Pencil, Pin } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { formatDraftAge } from "@/lib/format-time";
import type { SessionDetailVm, SessionFileVm } from "@/lib/ipc/client";
import { listenSessionsChanged, sessionsDetail } from "@/lib/ipc/client";
import { formatSize } from "@/lib/recording-format";
import { panelsStore } from "@/lib/stores/panels";

/** The way back to the board. */
export const SESSION_DETAIL_BACK_LABEL = "Back to sessions";

/** Section headings — the zone's own vocabulary, not keeper's. */
export const SESSION_DETAIL_LOG_HEADING = "Log";
export const SESSION_DETAIL_ARTIFACTS_HEADING = "Artifacts";
export const SESSION_DETAIL_REFS_HEADING = "Refs";
export const SESSION_DETAIL_PROMPTS_HEADING = "Prompts";
export const SESSION_DETAIL_WORKSPACE_HEADING = "Workspace";
export const SESSION_DETAIL_EXTRAS_HEADING = "Other files";
export const SESSION_DETAIL_PROPERTIES_HEADING = "Properties";

/** The zone's own sentence under the workspace section (FR-237). */
export const SESSION_DETAIL_WORKSPACE_CAVEAT =
  "scratch — not versioned, not synced, dies with the session. Read-only in keeper; promote what matters into artifacts.";

/** What an empty section says, per section, honestly. */
export const SESSION_DETAIL_NO_LOG = "No log entries yet.";
export const SESSION_DETAIL_NO_FILES = "Empty.";

/** Open the session's README in the strip. */
export const SESSION_DETAIL_OPEN_README_LABEL = "Open README";

export interface SessionDetailProps {
  rootId: string;
  /** The zone subfolder ("60-sessions"), for composing file targets. */
  subfolder: string;
  sessionId: string;
  onBack: () => void;
}

/** One file row: name, size, age; click opens in the strip. */
function FileRow({
  file,
  readOnly,
  nowMs,
  onOpen,
}: {
  file: SessionFileVm;
  readOnly: boolean;
  nowMs: number;
  onOpen: (file: SessionFileVm) => void;
}) {
  return (
    <li>
      <button
        type="button"
        onClick={() => onOpen(file)}
        className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-sm hover:bg-accent/50"
      >
        {file.isDir ? (
          <Folder aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
        ) : (
          <FileText aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
        )}
        <span className="min-w-0 flex-1 truncate">{file.name}</span>
        {readOnly && (
          <Lock aria-label="read-only" className="size-3 shrink-0 text-muted-foreground" />
        )}
        {!file.isDir && (
          <span className="figures shrink-0 text-muted-foreground text-xs">
            {formatSize(file.size)}
          </span>
        )}
        {file.mtimeMs > 0 && (
          <span className="figures w-16 shrink-0 text-right text-muted-foreground text-xs">
            {formatDraftAge(file.mtimeMs, nowMs)}
          </span>
        )}
      </button>
    </li>
  );
}

/** One mini-file section: heading, count, rows. Absent sections render a
 * quiet "Empty." rather than vanishing — a session missing its artifacts is
 * a fact worth seeing (the delete-instead rule feeds on it). */
function FileSection({
  heading,
  files,
  caveat,
  readOnly,
  nowMs,
  onOpen,
}: {
  heading: string;
  files: SessionFileVm[];
  caveat?: string;
  readOnly?: boolean;
  nowMs: number;
  onOpen: (file: SessionFileVm) => void;
}) {
  return (
    <section aria-label={heading} className="flex flex-col gap-1">
      <h3 className="flex items-baseline gap-2 font-medium text-muted-foreground text-xs uppercase tracking-wide">
        {heading}
        <span className="figures normal-case">{files.length}</span>
      </h3>
      {caveat !== undefined && <p className="text-muted-foreground text-xs">{caveat}</p>}
      {files.length === 0 ? (
        <p className="px-2 text-muted-foreground text-xs">{SESSION_DETAIL_NO_FILES}</p>
      ) : (
        <ul className="flex flex-col">
          {files.map((file) => (
            <FileRow
              key={file.relPath}
              file={file}
              readOnly={readOnly === true}
              nowMs={nowMs}
              onOpen={onOpen}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

export function SessionDetail({ rootId, subfolder, sessionId, onBack }: SessionDetailProps) {
  const [detail, setDetail] = useState<SessionDetailVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const nowMs = Date.now();

  // Read on mount and re-read on the changed event — an agent's write on
  // disk moves this surface without a keystroke (FR-234's detail half).
  useEffect(() => {
    let live = true;
    const read = () => {
      sessionsDetail(rootId, sessionId).then(
        (vm) => {
          if (live) {
            setDetail(vm);
            setError(null);
          }
        },
        (e: unknown) => {
          if (live) {
            setError(e instanceof Error ? e.message : String(e));
          }
        },
      );
    };
    read();
    let unlisten: (() => void) | null = null;
    void listenSessionsChanged((changedRootId) => {
      if (live && changedRootId === rootId) {
        read();
      }
    }).then((stop) => {
      if (live) {
        unlisten = stop;
      } else {
        stop();
      }
    });
    return () => {
      live = false;
      unlisten?.();
    };
  }, [rootId, sessionId]);

  // Files open in the strip through the one file target (AD-109). The
  // workspace fence in Rust keeps read-only honest; the target itself is
  // the same shape for every section.
  const openFile = useCallback(
    (file: SessionFileVm) => {
      if (detail === null || file.isDir) {
        return;
      }
      panelsStore.getState().setActiveTarget({
        kind: "file",
        profileId: rootId,
        relativePath: `${subfolder}/${detail.path}/${file.relPath}`,
      });
    },
    [detail, rootId, subfolder],
  );

  const openReadme = useCallback(() => {
    if (detail === null) {
      return;
    }
    panelsStore.getState().setActiveTarget({
      kind: "file",
      profileId: rootId,
      relativePath: `${subfolder}/${detail.path}/README.md`,
    });
  }, [detail, rootId, subfolder]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-border border-b px-6 py-2">
        <Button type="button" variant="ghost" size="sm" onClick={onBack} className="gap-1">
          <ArrowLeft aria-hidden className="size-3.5" />
          {SESSION_DETAIL_BACK_LABEL}
        </Button>
      </div>
      {error !== null && (
        <div role="alert" className="m-6 rounded-md bg-destructive/10 p-3 text-destructive text-sm">
          {error}
        </div>
      )}
      {detail !== null && (
        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 py-4">
          {/* Header: title, status, pin, tags, lineage, summary. */}
          <header className="flex flex-col gap-1">
            <div className="flex min-w-0 items-center gap-2">
              {detail.pinned && (
                <Pin aria-label="pinned" className="size-3.5 text-muted-foreground" />
              )}
              <h2 className="min-w-0 flex-1 truncate font-heading text-title">{detail.title}</h2>
              <Badge variant="outline">
                {detail.status === "archived" && detail.archivedYear !== null
                  ? `archived ${detail.archivedYear}`
                  : detail.status}
              </Badge>
              <Button type="button" variant="outline" size="sm" onClick={openReadme}>
                <Pencil aria-hidden className="size-3.5" />
                {SESSION_DETAIL_OPEN_README_LABEL}
              </Button>
            </div>
            {detail.summary !== "" && (
              <p className="text-muted-foreground text-sm">{detail.summary}</p>
            )}
            <div className="flex flex-wrap items-center gap-1">
              {detail.tags.map((tag) => (
                <Badge key={tag} variant="secondary">
                  {tag}
                </Badge>
              ))}
              {/* Lineage chips (UX-DR89): navigable directions arrive with the
                  cross-session router; ids render inert but visible for now. */}
              {detail.continues.map((id) => (
                <Badge key={`c-${id}`} variant="outline" title={`continues ${id}`}>
                  ← continues
                </Badge>
              ))}
              {detail.continuedBy.map((id) => (
                <Badge key={`b-${id}`} variant="outline" title={`continued by ${id}`}>
                  continued →
                </Badge>
              ))}
            </div>
          </header>

          {/* The properties widget (FR-227): user-tier frontmatter, read here,
              edited in the README's own properties panel — one writer. */}
          {detail.properties.length > 0 && (
            <section
              aria-label={SESSION_DETAIL_PROPERTIES_HEADING}
              className="rounded-md border border-border px-3 py-2"
            >
              <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
                {detail.properties.map((property) => (
                  <div key={property.key} className="contents">
                    <dt className="text-muted-foreground">{property.key}</dt>
                    <dd className="min-w-0 truncate">{property.value.split("\n").join(", ")}</dd>
                  </div>
                ))}
              </dl>
            </section>
          )}

          {/* The rendered activity log, newest first (the review order). */}
          <section aria-label={SESSION_DETAIL_LOG_HEADING} className="flex flex-col gap-2">
            <h3 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
              {SESSION_DETAIL_LOG_HEADING}
            </h3>
            {detail.log.length === 0 ? (
              <p className="text-muted-foreground text-xs">{SESSION_DETAIL_NO_LOG}</p>
            ) : (
              <ol className="flex flex-col gap-2">
                {detail.log.map((entry) => (
                  <li
                    key={`${entry.date}-${entry.title}`}
                    className="rounded-md border border-border px-3 py-2"
                  >
                    <p className="flex items-baseline gap-2 text-sm">
                      <span className="figures shrink-0 text-muted-foreground">{entry.date}</span>
                      {entry.title !== "" && <span className="font-medium">{entry.title}</span>}
                    </p>
                    {entry.body !== "" && (
                      <p className="mt-1 whitespace-pre-wrap text-muted-foreground text-sm">
                        {entry.body}
                      </p>
                    )}
                  </li>
                ))}
              </ol>
            )}
          </section>

          {/* The mini-file sections, in the zone's own order. */}
          <FileSection
            heading={SESSION_DETAIL_ARTIFACTS_HEADING}
            files={detail.artifacts}
            nowMs={nowMs}
            onOpen={openFile}
          />
          <FileSection
            heading={SESSION_DETAIL_REFS_HEADING}
            files={detail.refs}
            nowMs={nowMs}
            onOpen={openFile}
          />
          <FileSection
            heading={SESSION_DETAIL_PROMPTS_HEADING}
            files={detail.prompts}
            nowMs={nowMs}
            onOpen={openFile}
          />
          {detail.extras.length > 0 && (
            <FileSection
              heading={SESSION_DETAIL_EXTRAS_HEADING}
              files={detail.extras}
              nowMs={nowMs}
              onOpen={openFile}
            />
          )}
          <FileSection
            heading={SESSION_DETAIL_WORKSPACE_HEADING}
            files={detail.workspace}
            caveat={SESSION_DETAIL_WORKSPACE_CAVEAT}
            readOnly
            nowMs={nowMs}
            onOpen={openFile}
          />
        </div>
      )}
    </div>
  );
}
