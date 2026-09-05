/**
 * The Files surface on a phone (Epic 66, Story 66.3, FR-464…FR-466, AD-200).
 *
 * The desktop's {@link "@/components/layout/files-pane"} is a tree beside a
 * `PanelStrip`: a folder opens in place, a file opens in a panel to the right,
 * and a row carries a cluster of hover-revealed verbs. None of that shape
 * survives a 390 pt column, and the desktop pane is three thousand lines that
 * assume it. So this is the same READERS in the phone's own stack — one column,
 * one thing at a time: the folders keeper syncs, then one folder's listing,
 * then one file full-screen — and nothing else is forked. The listing is
 * `sync_browse`'s answer rendered as 44 pt rows; a file is drawn by the one
 * viewer registry (`viewerComponentFor`, AD-87), so a markdown file, a
 * photo, a recording and a PDF open here exactly as they open in a panel.
 *
 * **What the phone does that the desktop does not.** A phone profile is fully
 * virtual (AD-199): every large file is a pointer until it is opened. So a tap
 * on a `virtual` row materializes first (`sync_materialize_entry`, the batch
 * client, the same progress word the row's sync mark uses) and opens when the
 * content is here; the second tap finds it `materialized` and reads without a
 * fetch. And where the desktop reveals in Finder, the phone hands the file to
 * the share sheet (`share_out`, `CapabilitiesVm.shareOut`): two verbs, two
 * flags, each absent — never disabled — where its flag is off (AD-27). A
 * pointer is materialized before it is shared, for the same reason it is
 * before it is read.
 *
 * **What the phone does not offer.** Export and Copy path need a destination
 * picker the phone does not have; Open-with needs an application the container
 * cannot hand a file to; Materialize/Release/Pin are the desktop's verbs over a
 * folder whose policy the phone does not edit. None of them is rendered here,
 * and `openWith` is `null` on the file handed to a viewer, so the unknown
 * viewer draws no Open control either.
 *
 * **Pull-to-refresh is `sync_folder_now` and a re-read.** A phone has no
 * watcher (NFR-57): it syncs on open, on foreground and when a person pulls
 * the listing down. The pull zone is the shell's idiom (a thin `touch-none`
 * band, armed only at the top of the scroll), the fetch is the engine's, and
 * the listing is re-browsed afterwards so what the pull brought is what is on
 * screen. The Refresh control beside the title does the same without a
 * gesture, because a control a finger can find is not optional on a surface
 * whose only other trigger is one.
 *
 * **The frontend never composes a path** (AD-65). Every read carries a profile
 * id and a subpath the listing produced, and the absolute path arrives from
 * Rust as an action's argument only — here, for Reveal on a narrow desktop
 * window, and never for Share, which is addressed like every other reader.
 */

import type { LucideIcon } from "lucide-react";
import { ChevronLeft, ChevronRight, RefreshCw, Share, WifiOff } from "lucide-react";
import {
  type ReactNode,
  type PointerEvent as ReactPointerEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  FILES_ALL_PAUSED_SENTENCE,
  FILES_EMPTY_FOLDER_SENTENCE,
  FILES_NO_PROFILES_SENTENCE,
  FILES_PANE_TITLE,
  FILES_READING_SENTENCE,
  FILES_REFRESH_LABEL,
  FILES_REVEAL_LABEL,
} from "@/components/layout/files-pane";
import { OFFLINE_PILL_TEXT } from "@/components/layout/sidebar-pane";
import { FILES_SYNC_MARK_LABEL, SyncStatusMark } from "@/components/layout/sync-status-mark";
import { Button } from "@/components/ui/button";
import type { FilesEntryVm, FilesListingVm, SyncProfileVm } from "@/lib/ipc/client";
import {
  revealPath,
  shareOut,
  syncBrowse,
  syncFolderNow,
  syncMaterializeEntry,
  syncProfiles,
} from "@/lib/ipc/client";
import { useShellOffline } from "@/lib/stores/account-status";
import { useCapabilitiesStore, useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";
import { resolveViewer, VIEWER_ICON, type ViewerFile, viewerComponentFor } from "@/lib/viewers";

/** The share control — the phone's reveal (FR-466). */
export const FILES_PHONE_SHARE_LABEL = "Share";

/** What the listing says while a pull's fetch is in flight. */
export const FILES_PHONE_REFRESHING_SENTENCE = "Fetching from the remote…";

/**
 * What the surface says while a pointer's content is on its way: the sync
 * mark's own word for the state (`materializing`), so the phone and the
 * desktop row describe one thing with one vocabulary.
 */
export function filesPhoneArrivingSentence(name: string): string {
  return `${FILES_SYNC_MARK_LABEL.materializing}: ${name}`;
}

/** How far a pull has to travel before release fetches (the shell's refresh threshold). */
export const FILES_PHONE_PULL_THRESHOLD_PX = 64;

/** Test ids: the pull zone, one row (suffixed with its relative path), the status line. */
export const FILES_PHONE_PULL_TESTID = "files-phone-pull";
export const FILES_PHONE_ROW_TESTID = "files-phone-row";
export const FILES_PHONE_STATUS_TESTID = "files-phone-status";

/** Names the three bodies, so a test and the measure rig find each. */
export const FILES_PHONE_PROFILES_SLOT = "files-phone-profiles";
export const FILES_PHONE_LISTING_SLOT = "files-phone-listing";
export const FILES_PHONE_DOCUMENT_SLOT = "files-phone-document";

/** Where the column is: the folders, one folder, or one file. */
type Place =
  | { readonly kind: "profiles" }
  | { readonly kind: "folder"; readonly profileId: string; readonly subpath: string }
  | {
      readonly kind: "file";
      readonly profileId: string;
      /** The folder the file was opened from, so Back lands there. */
      readonly subpath: string;
      readonly entry: FilesEntryVm;
    };

/** The folder above `subpath`, or `""` for a first-level folder. */
function parentOf(subpath: string): string {
  const cut = subpath.lastIndexOf("/");
  return cut === -1 ? "" : subpath.slice(0, cut);
}

/** The last segment of a subpath — what a folder level is titled. */
function nameOf(subpath: string): string {
  const cut = subpath.lastIndexOf("/");
  return cut === -1 ? subpath : subpath.slice(cut + 1);
}

/**
 * The file a viewer is handed. `openWith` is `null` on purpose: the phone
 * has no application to hand a file to (`sync_open_entry` refuses there with
 * a sentence), so the control is absent rather than a refusal (AD-27).
 */
function viewerFileFor(profileId: string, entry: FilesEntryVm): ViewerFile {
  return {
    name: entry.name,
    kind: entry.kind,
    relativePath: entry.relativePath,
    profileId,
    absolutePath: entry.absolutePath,
    sizeLabel: entry.size?.label ?? null,
    openWith: null,
    writeCaveat: entry.write.caveat,
    writeCaveatShort: entry.write.caveatShort,
    writeRefusal: entry.write.writable ? null : entry.write.reason,
  };
}

/** One 44 pt row: a glyph, a name, what the trailing cell says. */
function Row({
  label,
  icon: Icon,
  trailing,
  testId,
  onPress,
}: {
  label: string;
  icon: LucideIcon;
  trailing?: ReactNode;
  testId?: string;
  onPress: () => void;
}) {
  return (
    <li>
      <button
        type="button"
        data-testid={testId}
        onClick={onPress}
        className="flex min-h-11 w-full min-w-0 items-center gap-3 px-4 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Icon className="size-5 shrink-0 text-muted-foreground" aria-hidden />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {trailing}
      </button>
    </li>
  );
}

export function FilesPhonePane() {
  const canShare = useCapabilitiesStore((s) => s.capabilities.shareOut);
  const canReveal = useCapabilitiesStore((s) => s.capabilities.revealInFileManager);
  const offline = useShellOffline();
  const reduced = useIsReducedCapabilityPlatform();

  const [profiles, setProfiles] = useState<SyncProfileVm[] | null>(null);
  const [place, setPlace] = useState<Place>({ kind: "profiles" });
  const [listing, setListing] = useState<FilesListingVm | null>(null);
  const [reading, setReading] = useState(false);
  /** Rust's sentence for the last thing that went wrong: one sink, one line. */
  const [notice, setNotice] = useState<string | null>(null);
  /** The name whose content is on its way, while a materialize is in flight. */
  const [arriving, setArriving] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  // A stale-read token: a folder answered after the person has left it must
  // not replace the one they are looking at.
  const readToken = useRef(0);

  const load = useCallback(
    async (profileId: string, subpath: string): Promise<FilesListingVm | null> => {
      readToken.current += 1;
      const token = readToken.current;
      setReading(true);
      try {
        const answer = await syncBrowse(profileId, subpath);
        if (token === readToken.current) {
          setListing(answer);
        }
        return answer;
      } catch (raw: unknown) {
        if (token === readToken.current) {
          setListing(null);
          setNotice(syncErrorMessage(raw));
        }
        return null;
      } finally {
        if (token === readToken.current) {
          setReading(false);
        }
      }
    },
    [],
  );

  // The folders, once. Exactly one browsable folder — the owner's phone —
  // opens itself: a list of one is a tap that buys nothing.
  useEffect(() => {
    let cancelled = false;
    void syncProfiles()
      .then((answer) => {
        if (cancelled) {
          return;
        }
        setProfiles(answer);
        const browsable = answer.filter((p) => p.enabled);
        if (browsable.length === 1) {
          setPlace((current) =>
            current.kind === "profiles"
              ? { kind: "folder", profileId: browsable[0].id, subpath: "" }
              : current,
          );
        }
      })
      .catch((raw: unknown) => {
        if (!cancelled) {
          setProfiles([]);
          setNotice(syncErrorMessage(raw));
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // A folder reads itself when it is entered. The dependency is the FOLDER,
  // which a file level shares with the listing it was opened from: opening a
  // file and coming back are not re-reads, so Back is instant and the row the
  // materialize refreshed is the row that is there. (`place` itself changes
  // identity on every navigation, including into a file and back.)
  const folderProfileId = place.kind === "profiles" ? null : place.profileId;
  const folderSubpath = place.kind === "profiles" ? null : place.subpath;
  useEffect(() => {
    if (folderProfileId !== null && folderSubpath !== null) {
      void load(folderProfileId, folderSubpath);
    }
  }, [folderProfileId, folderSubpath, load]);

  const profileName = useCallback(
    (profileId: string) => profiles?.find((p) => p.id === profileId)?.name ?? FILES_PANE_TITLE,
    [profiles],
  );

  /** What this level is called, and what the level under it is called. */
  const title =
    place.kind === "profiles"
      ? FILES_PANE_TITLE
      : place.kind === "folder"
        ? place.subpath === ""
          ? profileName(place.profileId)
          : nameOf(place.subpath)
        : place.entry.name;
  const beneath =
    place.kind === "profiles"
      ? null
      : place.kind === "folder"
        ? place.subpath === ""
          ? FILES_PANE_TITLE
          : parentOf(place.subpath) === ""
            ? profileName(place.profileId)
            : nameOf(parentOf(place.subpath))
        : place.subpath === ""
          ? profileName(place.profileId)
          : nameOf(place.subpath);

  const back = () => {
    setNotice(null);
    if (place.kind === "folder") {
      setPlace(
        place.subpath === ""
          ? { kind: "profiles" }
          : { kind: "folder", profileId: place.profileId, subpath: parentOf(place.subpath) },
      );
    } else if (place.kind === "file") {
      setPlace({ kind: "folder", profileId: place.profileId, subpath: place.subpath });
    }
  };

  /**
   * The entry with its bytes here. A row whose content is already on this
   * phone (`materialized`, or an ordinary file) is handed back as it is —
   * that is the second open reading without a fetch. A pointer, or one
   * already on its way, is materialized first, and the folder is re-read so
   * the row that comes back says `materialized`. Resolves to `null` after a
   * refusal — which is on screen by then, in Rust's words.
   */
  const withContent = useCallback(
    async (
      profileId: string,
      subpath: string,
      entry: FilesEntryVm,
    ): Promise<FilesEntryVm | null> => {
      if (entry.sync.status !== "virtual" && entry.sync.status !== "materializing") {
        return entry;
      }
      setNotice(null);
      setArriving(entry.name);
      try {
        await syncMaterializeEntry(profileId, entry.relativePath);
      } catch (raw: unknown) {
        setNotice(syncErrorMessage(raw));
        return null;
      } finally {
        setArriving(null);
      }
      const fresh = await load(profileId, subpath);
      return fresh?.entries?.find((e) => e.relativePath === entry.relativePath) ?? entry;
    },
    [load],
  );

  const openEntry = async (profileId: string, subpath: string, entry: FilesEntryVm) => {
    if (entry.kind === "folder") {
      setNotice(null);
      setPlace({ kind: "folder", profileId, subpath: entry.relativePath });
      return;
    }
    const ready = await withContent(profileId, subpath, entry);
    if (ready !== null) {
      setPlace({ kind: "file", profileId, subpath, entry: ready });
    }
  };

  const share = async () => {
    if (place.kind !== "file") {
      return;
    }
    const ready = await withContent(place.profileId, place.subpath, place.entry);
    if (ready === null) {
      return;
    }
    try {
      await shareOut(place.profileId, ready.relativePath);
    } catch (raw: unknown) {
      setNotice(syncErrorMessage(raw));
    }
  };

  const refresh = useCallback(async () => {
    if (place.kind === "profiles" || refreshing) {
      return;
    }
    setRefreshing(true);
    setNotice(null);
    try {
      await syncFolderNow(place.profileId);
    } catch (raw: unknown) {
      setNotice(syncErrorMessage(raw));
    } finally {
      setRefreshing(false);
    }
    if (place.kind === "folder") {
      await load(place.profileId, place.subpath);
    }
  }, [place, refreshing, load]);

  // The pull zone (the shell's idiom, Story 13.6): armed only with the
  // listing scrolled to its top, tracked while captured, and a release past
  // the threshold is the refresh above. Below it, nothing.
  const scrollRef = useRef<HTMLDivElement>(null);
  const pullRef = useRef<{ pointerId: number; startY: number } | null>(null);
  const [pullDy, setPullDy] = useState<number | null>(null);
  const onPullDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (pullRef.current !== null || (scrollRef.current?.scrollTop ?? 0) > 0) {
      return;
    }
    pullRef.current = { pointerId: e.pointerId, startY: e.clientY };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onPullMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const pull = pullRef.current;
    if (pull === null || e.pointerId !== pull.pointerId) {
      return;
    }
    setPullDy(Math.max(e.clientY - pull.startY, 0));
  };
  const onPullEnd = (e: ReactPointerEvent<HTMLDivElement>) => {
    const pull = pullRef.current;
    if (pull === null || e.pointerId !== pull.pointerId) {
      return;
    }
    pullRef.current = null;
    setPullDy(null);
    if (e.type === "pointerup" && e.clientY - pull.startY >= FILES_PHONE_PULL_THRESHOLD_PX) {
      void refresh();
    }
  };

  const enabled = (profiles ?? []).filter((p) => p.enabled);

  let body: ReactNode;
  if (place.kind === "profiles") {
    body = (
      <div data-slot={FILES_PHONE_PROFILES_SLOT} className="min-h-0 flex-1 overflow-y-auto">
        {profiles === null ? (
          <p className="px-4 py-3 text-muted-foreground text-sm">{FILES_READING_SENTENCE}</p>
        ) : enabled.length === 0 ? (
          <p className="px-4 py-3 text-muted-foreground text-sm">
            {profiles.length === 0 ? FILES_NO_PROFILES_SENTENCE : FILES_ALL_PAUSED_SENTENCE}
          </p>
        ) : (
          <ul className="divide-y divide-border">
            {enabled.map((profile) => (
              <Row
                key={profile.id}
                label={profile.name}
                icon={VIEWER_ICON.folder}
                testId={`${FILES_PHONE_ROW_TESTID}-${profile.id}`}
                trailing={
                  <ChevronRight className="size-4 shrink-0 text-muted-foreground" aria-hidden />
                }
                onPress={() => {
                  setNotice(null);
                  setPlace({ kind: "folder", profileId: profile.id, subpath: "" });
                }}
              />
            ))}
          </ul>
        )}
      </div>
    );
  } else if (place.kind === "folder") {
    const entries = listing?.entries ?? null;
    body = (
      <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
        <div
          aria-hidden="true"
          data-testid={FILES_PHONE_PULL_TESTID}
          className="absolute top-0 right-0 left-5 z-10 h-6 touch-none"
          onPointerDown={onPullDown}
          onPointerMove={onPullMove}
          onPointerUp={onPullEnd}
          onPointerCancel={onPullEnd}
          onLostPointerCapture={onPullEnd}
        />
        {(refreshing || (pullDy !== null && pullDy >= FILES_PHONE_PULL_THRESHOLD_PX)) && (
          <div
            role="status"
            className="flex shrink-0 items-center justify-center gap-2 py-1 text-muted-foreground text-xs"
          >
            <RefreshCw
              aria-hidden="true"
              className={cn("size-4", refreshing && "motion-safe:animate-spin")}
            />
            <span>{FILES_PHONE_REFRESHING_SENTENCE}</span>
          </div>
        )}
        <div
          ref={scrollRef}
          data-slot={FILES_PHONE_LISTING_SLOT}
          className="min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-contain"
        >
          {listing === null && reading ? (
            <p className="px-4 py-3 text-muted-foreground text-sm">{FILES_READING_SENTENCE}</p>
          ) : listing !== null && entries === null ? (
            // Not `listed`: the drive is out, the folder moved. Rust's sentence,
            // never "empty" (the distinction is the story).
            <p className="px-4 py-3 text-muted-foreground text-sm">{listing.detail}</p>
          ) : entries !== null && entries.length === 0 ? (
            <p className="px-4 py-3 text-muted-foreground text-sm">
              {listing?.detail ?? FILES_EMPTY_FOLDER_SENTENCE}
            </p>
          ) : entries !== null ? (
            <>
              <ul className="divide-y divide-border">
                {entries.map((entry) => {
                  const folder = entry.kind === "folder";
                  const icon = folder
                    ? VIEWER_ICON.folder
                    : VIEWER_ICON[resolveViewer({ name: entry.name, kind: entry.kind }).icon];
                  return (
                    <Row
                      key={entry.relativePath}
                      label={entry.name}
                      icon={icon}
                      testId={`${FILES_PHONE_ROW_TESTID}-${entry.relativePath}`}
                      trailing={
                        <>
                          {!folder && <SyncStatusMark sync={entry.sync} />}
                          {entry.size !== null && (
                            <span className="shrink-0 text-muted-foreground text-xs tabular-nums">
                              {entry.size.label}
                            </span>
                          )}
                          {folder && (
                            <ChevronRight
                              className="size-4 shrink-0 text-muted-foreground"
                              aria-hidden
                            />
                          )}
                        </>
                      }
                      onPress={() => void openEntry(place.profileId, place.subpath, entry)}
                    />
                  );
                })}
              </ul>
              {listing?.detail !== null && listing?.detail !== undefined && (
                <p className="px-4 py-3 text-muted-foreground text-xs">{listing.detail}</p>
              )}
            </>
          ) : null}
        </div>
      </div>
    );
  } else {
    const file = viewerFileFor(place.profileId, place.entry);
    const { Component, entry } = viewerComponentFor(file);
    body = (
      <div
        data-slot={FILES_PHONE_DOCUMENT_SLOT}
        className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
      >
        <Component file={file} entry={entry} />
      </div>
    );
  }

  return (
    <section
      aria-label={FILES_PANE_TITLE}
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-background"
    >
      <header className="flex min-h-11 shrink-0 items-center gap-1 border-border border-b pr-2 pl-1">
        {beneath !== null ? (
          <button
            type="button"
            aria-label={`Back to ${beneath}`}
            onClick={back}
            className="flex h-11 min-w-11 shrink-0 items-center gap-0.5 pr-2 pl-1 text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <ChevronLeft className="size-5" aria-hidden="true" />
            <span className="max-w-24 truncate text-sm">{beneath}</span>
          </button>
        ) : null}
        <h1 className="min-w-0 flex-1 truncate px-2 font-heading text-title">{title}</h1>
        {place.kind === "file" && canShare && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="min-h-11"
            onClick={() => void share()}
          >
            <Share aria-hidden="true" />
            {FILES_PHONE_SHARE_LABEL}
          </Button>
        )}
        {place.kind === "file" && canReveal && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="min-h-11"
            onClick={() => {
              void revealPath(place.entry.absolutePath).catch(() => undefined);
            }}
          >
            {FILES_REVEAL_LABEL}
          </Button>
        )}
        {place.kind === "folder" && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-11"
            aria-label={FILES_REFRESH_LABEL}
            title={FILES_REFRESH_LABEL}
            disabled={refreshing}
            onClick={() => void refresh()}
          >
            <RefreshCw
              aria-hidden="true"
              className={cn(refreshing && "motion-safe:animate-spin")}
            />
          </Button>
        )}
      </header>
      {reduced && offline && (
        <div
          role="status"
          data-testid="offline-pill"
          className="flex shrink-0 items-start gap-2 border-border border-b bg-held/10 px-3 py-2 text-held text-xs"
        >
          <WifiOff aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
          <span>{OFFLINE_PILL_TEXT}</span>
        </div>
      )}
      {arriving !== null && (
        <p
          role="status"
          data-testid={FILES_PHONE_STATUS_TESTID}
          className="shrink-0 border-border border-b px-4 py-2 text-muted-foreground text-sm"
        >
          {filesPhoneArrivingSentence(arriving)}
        </p>
      )}
      {notice !== null && (
        <p
          role="alert"
          data-testid={FILES_PHONE_STATUS_TESTID}
          className="shrink-0 border-border border-b bg-destructive/10 px-4 py-2 text-destructive text-sm"
        >
          {notice}
        </p>
      )}
      {body}
    </section>
  );
}
