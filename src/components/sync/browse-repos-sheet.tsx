/**
 * Browse repositories (Epic 86, UX-DR120): add drives from the repositories a
 * source can reach — the account's forge, GitHub (through the organization's
 * broker, or a device-flow connection) — instead of pasting remote URLs.
 *
 * # Where it opens
 *
 * {@link BrowseReposEntry} is the `Browse repositories…` button and the sheet
 * behind it. Settings › Sync puts it beside the Add a folder heading and the
 * Sync pane beside its add action (or in its empty state). It renders nothing
 * while `forges_list` is empty or unanswered: an install with no account and
 * no GitHub client id sees keeper exactly as before (AD-27).
 *
 * # What it decides, and what it does not
 *
 * Rust fetches, marks (`addedAs`, `elsewhere`), orders and words everything —
 * every sentence about a source, every notice, every per-repository result.
 * The sheet holds the person's lens on the list (search, owner, forks,
 * archived, sort) and the batch preview; both are pure, in
 * `@/lib/forge-repos`.
 *
 * A single `Add…` hands a prefill to the surface's own add form, which asks
 * for the folder; the batch step adds many at once under one base folder.
 * No token crosses into the webview: a device-flow connection shows the user
 * code, which the person types into GitHub themselves, and the shell opens
 * the verification page (`forge_connect_open`) — nothing here opens it.
 */
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowDownToLine,
  Check,
  ChevronLeft,
  FolderGit2,
  Info,
  LoaderCircle,
  Lock,
  RefreshCw,
} from "lucide-react";
import {
  type KeyboardEvent,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { type AddFolderPrefill, SYNC_CHOOSE_FOLDER_LABEL } from "@/components/sync/add-folder-form";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { IconHint } from "@/components/ui/tooltip";
import { formatFileSize } from "@/lib/file-size";
import {
  batchConflicts,
  batchFolder,
  DEFAULT_REPO_FILTERS,
  filterRepos,
  groupRepos,
  pickedRepos,
  type RepoFilters,
  type RepoSort,
} from "@/lib/forge-repos";
import { formatDraftAge } from "@/lib/format-time";
import {
  accountSignIn,
  type DeviceCodeVm,
  type ForgeAddResultVm,
  type ForgeReposVm,
  type ForgeRepoVm,
  type ForgeSourceVm,
  forgeConnectCancel,
  forgeConnectOpen,
  forgeConnectStart,
  forgeConnectWait,
  forgeDefaultBaseFolder,
  forgeDisconnect,
  forgeRepos,
  forgeReposAdd,
} from "@/lib/ipc/client";
import { accountStore, useAccountStore } from "@/lib/stores/account";
import { useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import {
  putForgeSource,
  readForgeSourceCookie,
  refreshForgeSources,
  rememberForgeSource,
  useForgesStore,
} from "@/lib/stores/forges";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { refreshSyncProfiles, syncErrorMessage, useSyncStore } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

// ---------------------------------------------------------------------------
// Words
// ---------------------------------------------------------------------------

export const BROWSE_REPOS_LABEL = "Browse repositories…";
export const BROWSE_REPOS_TITLE = "Add drives from your repositories";
export const BROWSE_REPOS_NOTE = "Pick repositories to sync as drives on this device.";
export const BROWSE_SOURCE_LABEL = "Source";

export function browseConnectIntro(name: string): string {
  return `See your ${name} repositories and those of your organizations. keeper asks ${name} for read access to them and to your organizations' names.`;
}
export function browseConnectLabel(name: string): string {
  return `Connect ${name}`;
}
export const BROWSE_CODE_LABEL = "Your code";
export const BROWSE_COPY_CODE_LABEL = "Copy code";
export const BROWSE_CODE_COPIED = "Code copied";
/** A clipboard the webview refused or does not have; the code stays selectable. */
export const BROWSE_COPY_FAILED = "keeper couldn't copy the code. Select it above and copy it.";
export function browseOpenLabel(host: string): string {
  return `Open ${host}`;
}
export function browseWaitingSentence(name: string): string {
  return `Waiting for you to approve keeper on ${name}…`;
}
export function browseCodeLifetime(expiresInSeconds: number): string {
  const minutes = Math.max(1, Math.round(expiresInSeconds / 60));
  return `The code works for ${minutes} ${minutes === 1 ? "minute" : "minutes"}.`;
}
export const BROWSE_CANCEL_LABEL = "Cancel";
export const BROWSE_SIGN_IN_LABEL = "Sign in";
export const BROWSE_TRY_AGAIN_LABEL = "Try again";
export function browseConnectedAs(login: string): string {
  return `Connected as ${login}`;
}
export const BROWSE_DISCONNECT_LABEL = "Disconnect";
/**
 * GitHub has no revocation keeper can call without a secret (AD-334), so the
 * note links to keeper's own entry on GitHub (`ForgeSourceVm.appsUrl`, Rust's
 * `…/settings/connections/applications/<client_id>`) when Rust knows it.
 */
export const BROWSE_DISCONNECTED_NOTE =
  "Also remove keeper under GitHub › Settings › Applications if you want.";
export const BROWSE_GITHUB_APPS_LABEL = "Open GitHub settings";

export const BROWSE_SEARCH_LABEL = "Search repositories";
export const BROWSE_OWNER_LABEL = "Owner";
export const BROWSE_OWNER_ALL = "All";
export const BROWSE_OWNER_YOU = "You";
export const BROWSE_FORKS_LABEL = "Forks";
export const BROWSE_ARCHIVED_LABEL = "Archived";
export const BROWSE_SORT_LABEL = "Sort";
export const BROWSE_SORT_LABELS: Record<RepoSort, string> = {
  updated: "Recently updated",
  name: "Name",
};
export const BROWSE_REFRESH_LABEL = "Refresh";
export const BROWSE_LOADING = "Reading repositories…";
export const BROWSE_EMPTY = "No repositories here yet.";
export const BROWSE_NO_MATCH = "No repositories match.";
export const BROWSE_LIST_LABEL = "Repositories";
export const BROWSE_PRIVATE_LABEL = "Private";
export const BROWSE_CHIPS = [
  ["fork", "Fork"],
  ["archived", "Archived"],
  ["template", "Template"],
  ["mirror", "Mirror"],
] as const;
export const BROWSE_ADD_ONE_LABEL = "Add…";
export const BROWSE_SYNCING_HERE = "Syncing here as";
export function browseElsewhere(devices: readonly string[]): string {
  return `On ${devices.join(", ")}`;
}
export function browseSelected(count: number): string {
  return `${count} selected`;
}
export const BROWSE_CLEAR_LABEL = "Clear";
export function browseAddDrives(count: number, ellipsis: boolean): string {
  return `Add ${count} ${count === 1 ? "drive" : "drives"}${ellipsis ? "…" : ""}`;
}

export const BROWSE_BACK_LABEL = "Back";
export const BROWSE_WHERE_TITLE = "Where should they go?";
export const BROWSE_BASE_LABEL = "Base folder";
export const BROWSE_BASE_MISSING = "Choose a folder for them first.";
export function browseDriveNameLabel(fullName: string): string {
  return `Drive name for ${fullName}`;
}
export function browseCredentialLine(name: string): string {
  return `Signs in with your ${name} connection`;
}
export function browseAddedSentence(count: number): string {
  return `Added ${count} ${count === 1 ? "drive" : "drives"}. They start syncing now.`;
}
export const BROWSE_ADDED_ROW = "Added";
/**
 * A batch row whose repository keeper can only read (Rust's `pullOnly`): the
 * drive is added as pull-only, and the preview says so before it is.
 */
export const BROWSE_DOWNLOAD_ONLY = "Download only";
/** A repository Rust's answer left out, which is Rust's bug and still the person's row. */
export const BROWSE_NO_RESULT = "keeper didn't report on this repository. Try again.";
export const BROWSE_DONE_LABEL = "Done";

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/**
 * The `Browse repositories…` button and its sheet. Absent while no source
 * exists; re-reads the sources whenever the account changes, because the
 * account's forge and the broker come and go with it.
 *
 * @param onAddOne - A row's `Add…`: the surface opens its own add form on this
 *   prefill. The sheet closes first, so the form is what the person sees.
 */
export function BrowseReposEntry({
  onAddOne,
  size = "sm",
  className,
}: {
  onAddOne: (prefill: AddFolderPrefill) => void;
  size?: "xs" | "sm";
  className?: string;
}) {
  const sources = useForgesStore((s) => s.sources);
  const accountKey = useAccountStore((s) => `${s.vm.id ?? ""}:${s.vm.state}`);
  // biome-ignore lint/correctness/useExhaustiveDependencies: keyed on the account on purpose — signing in or out changes what sources exist and what state each is in
  useEffect(() => {
    void refreshForgeSources();
  }, [accountKey]);
  const [open, setOpen] = useState(false);
  if (sources === null || sources.length === 0) {
    return null;
  }
  return (
    <>
      <Button
        type="button"
        variant="outline"
        size={size}
        className={cn("shrink-0", className)}
        onClick={() => setOpen(true)}
      >
        <FolderGit2 aria-hidden="true" />
        {BROWSE_REPOS_LABEL}
      </Button>
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent className="gap-0 p-0 data-[side=right]:w-full data-[side=right]:sm:max-w-[720px]">
          {open && (
            <BrowseFlow
              sources={sources}
              onClose={() => setOpen(false)}
              onAddOne={(prefill) => {
                setOpen(false);
                onAddOne(prefill);
              }}
            />
          )}
        </SheetContent>
      </Sheet>
    </>
  );
}

/**
 * A repository as the add form's start (AD-336): the repository's name as the
 * drive's, its clone URL and default branch, the credential its source signs
 * with (Rust's `ForgeSourceVm.credential` — `account` for the account's own
 * forge, `forge:<id>` for any other; the webview does not decide it, AD-40),
 * and pull-only when keeper can only read the repository.
 */
function repoPrefill(source: ForgeSourceVm, repo: ForgeRepoVm): AddFolderPrefill {
  return {
    key: `forge:${source.id}:${repo.fullName}`,
    name: repo.name,
    remoteUrl: repo.cloneUrl,
    branch: repo.defaultBranch,
    credential: source.credential,
    ...(repo.pullOnly ? { direction: "pullOnly" as const } : {}),
    notes: null,
    recordings: null,
    sessions: null,
    tasks: null,
    excludes: [],
    lfsThresholdBytes: null,
    virtualPatterns: null,
    virtualOverBytes: null,
    releaseTtlMs: null,
    tags: [],
    commitSubjectTemplate: null,
    devices: [],
  };
}

// ---------------------------------------------------------------------------
// The sheet's body
// ---------------------------------------------------------------------------

/**
 * What the list shows. An answer carries the source it was asked for, and is
 * shown only under that source: a listing is never drawn under another tab.
 */
type Listing =
  | { kind: "loading" }
  | { kind: "ready"; sourceId: string; vm: ForgeReposVm; refreshing: boolean }
  | { kind: "failed"; sourceId: string; sentence: string };

const LOADING: Listing = { kind: "loading" };

function BrowseFlow({
  sources,
  onClose,
  onAddOne,
}: {
  sources: ForgeSourceVm[];
  onClose: () => void;
  onAddOne: (prefill: AddFolderPrefill) => void;
}) {
  const [chosenId, setChosenId] = useState<string>(
    () => readForgeSourceCookie(typeof document === "undefined" ? "" : document.cookie) ?? "",
  );
  // The remembered source, when it still exists; the first one otherwise.
  const source = sources.find((candidate) => candidate.id === chosenId) ?? sources[0];
  const [view, setView] = useState<"list" | "batch">("list");
  const [selected, setSelected] = useState<readonly string[]>([]);
  const [filters, setFilters] = useState<RepoFilters>(DEFAULT_REPO_FILTERS);
  const [held, setListing] = useState<Listing>(LOADING);
  // Set by Disconnect so the connect view that follows says what keeper could
  // not do itself; cleared on the next source switch.
  const [disconnected, setDisconnected] = useState(false);

  // Whatever made the source change — a tab, or the chosen source leaving
  // Rust's list so the first one stands in — the lens and the ticks were
  // about the other source's repositories, and go with it.
  const [lensFor, setLensFor] = useState(source.id);
  if (lensFor !== source.id) {
    setLensFor(source.id);
    setView("list");
    setSelected([]);
    setFilters(DEFAULT_REPO_FILTERS);
    setDisconnected(false);
  }

  const listing = held.kind !== "loading" && held.sourceId === source.id ? held : LOADING;
  const vm = listing.kind === "ready" ? listing.vm : null;
  // An owner a refresh dropped would leave the owner filter on a choice the
  // menu no longer has — a blank trigger over "No repositories match."
  if (
    vm !== null &&
    filters.owner !== null &&
    !vm.owners.some((owner) => owner.login === filters.owner)
  ) {
    setFilters({ ...filters, owner: null });
  }

  // The source on screen now, for answers that arrive after a switch: each is
  // dropped unless it is for this source, so a slow answer for the previous
  // tab can neither show there nor replace this tab's list.
  const shownId = useRef(source.id);
  useLayoutEffect(() => {
    shownId.current = source.id;
  }, [source.id]);
  const connected = source.state === "connected";
  const load = useCallback(
    async (refresh: boolean, live: () => boolean = () => true) => {
      const sourceId = source.id;
      const current = () => live() && shownId.current === sourceId;
      setListing((was) =>
        was.kind === "ready" && was.sourceId === sourceId && refresh
          ? { ...was, refreshing: true }
          : LOADING,
      );
      try {
        const answer = await forgeRepos(sourceId, refresh);
        if (current()) {
          setListing({ kind: "ready", sourceId, vm: answer, refreshing: false });
        }
      } catch (raw) {
        if (current()) {
          setListing({ kind: "failed", sourceId, sentence: syncErrorMessage(raw) });
        }
        // The shell remembers why a listing failed, and `forges_list` then
        // answers the source in that state (needs connecting, needs sign-in,
        // unreachable) with its sentence — which swaps this list for the view
        // that can fix it.
        void refreshForgeSources();
      }
    },
    [source.id],
  );
  useEffect(() => {
    if (!connected) {
      return;
    }
    let live = true;
    void load(false, () => live);
    return () => {
      live = false;
    };
  }, [connected, load]);

  const choose = (id: string) => {
    setChosenId(id);
    rememberForgeSource(id);
  };

  const picked = vm === null ? [] : pickedRepos(vm.repos, selected);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SheetHeader className="gap-3 border-border border-b pr-12">
        <SheetTitle>{BROWSE_REPOS_TITLE}</SheetTitle>
        <SheetDescription>
          {sources.length === 1 ? `${source.name} · ${source.host}` : BROWSE_REPOS_NOTE}
        </SheetDescription>
        {/* A segment per source, and only when there is a choice to make: a
            single segment is a control that switches to itself. */}
        {sources.length > 1 && (
          <Tabs value={source.id} onValueChange={choose}>
            <TabsList aria-label={BROWSE_SOURCE_LABEL} className="w-full">
              {sources.map((candidate) => (
                <TabsTrigger key={candidate.id} value={candidate.id} className="min-w-0">
                  <span className="truncate">{candidate.name}</span>
                  <span className="truncate text-muted-foreground">· {candidate.host}</span>
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
        )}
      </SheetHeader>
      {source.state === "connected" && view === "list" && (
        <RepoBrowser
          source={source}
          listing={listing}
          filters={filters}
          onFilters={setFilters}
          selected={selected}
          picked={picked.length}
          onSelected={setSelected}
          onRefresh={() => void load(true)}
          // Asked again, never served from memory: the list failed, and
          // only a fresh listing replaces what the shell remembers (#1).
          onRetry={() => void load(true)}
          onAddOne={(repo) => onAddOne(repoPrefill(source, repo))}
          onBatch={() => setView("batch")}
          onOpenDrive={() => {
            primaryViewStore.getState().setView("sync");
            onClose();
          }}
          onDisconnect={async () => {
            const next = await forgeDisconnect(source.id);
            putForgeSource(next);
            setDisconnected(true);
            setSelected([]);
          }}
        />
      )}
      {source.state === "connected" && view === "batch" && (
        <BatchStep
          source={source}
          repos={picked}
          onBack={() => setView("list")}
          onDone={onClose}
          onAdded={(fullNames) => {
            setSelected((held) => held.filter((name) => !fullNames.includes(name)));
            // Marked again: what was just added now reads `Syncing here`.
            void load(false);
          }}
        />
      )}
      {/* Whether a source can be connected here is Rust's answer (`canConnect`:
          a device-flow client id exists), not its `via`: a broker source with
          a fallback client id and no grants is `notConnected` and connectable. */}
      {source.state === "notConnected" && source.canConnect && (
        <ConnectPanel key={source.id} source={source} disconnected={disconnected} />
      )}
      {((source.state === "notConnected" && !source.canConnect) ||
        source.state === "needsSignIn" ||
        source.state === "unreachable") && <SourceProblem source={source} />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Source states other than connected
// ---------------------------------------------------------------------------

/**
 * A source that cannot list: its own sentence and the one thing that can
 * change it — the account's sign-in, or asking again. Never a disabled list.
 *
 * Both ASK the source again (`forge_repos(id, refresh)`) before re-reading
 * the sources: the shell answers a source from the last listing's failure
 * until a listing replaces it, so re-reading alone would show the same
 * failure forever (surface #1). The listing's own rejection is not shown —
 * the source re-read after it carries Rust's sentence for it.
 */
function SourceProblem({ source }: { source: ForgeSourceVm }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const signIn = source.state === "needsSignIn";
  const act = async () => {
    setBusy(true);
    setError(null);
    try {
      if (signIn) {
        accountStore.getState().setVm(await accountSignIn());
      }
      await forgeRepos(source.id, true).catch(() => {});
      await refreshForgeSources();
    } catch (raw) {
      setError(syncErrorMessage(raw));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-col items-start gap-3 p-4">
      {source.sentence !== null && <p className="text-muted-foreground">{source.sentence}</p>}
      <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => void act()}>
        {busy && <LoaderCircle aria-hidden="true" className="animate-spin" />}
        {signIn ? BROWSE_SIGN_IN_LABEL : BROWSE_TRY_AGAIN_LABEL}
      </Button>
      {error !== null && (
        <p role="alert" className="text-destructive text-xs">
          {error}
        </p>
      )}
    </div>
  );
}

type ConnectPhase =
  | { kind: "idle"; sentence: string | null }
  | { kind: "starting" }
  | { kind: "waiting"; code: DeviceCodeVm; copied: boolean; problem: string | null };

/**
 * The device-flow connection (AD-334): Connect asks the shell for a code and
 * starts waiting at once; the person types the code on GitHub — copied for
 * them by `Open github.com` — and the wait resolves with the source. Closing
 * the sheet or switching away mid-wait cancels it, so no wait outlives the
 * surface that started it.
 */
function ConnectPanel({ source, disconnected }: { source: ForgeSourceVm; disconnected: boolean }) {
  const [phase, setPhase] = useState<ConnectPhase>({ kind: "idle", sentence: source.sentence });
  const waiting = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (waiting.current) {
        void forgeConnectCancel(source.id).catch(() => {});
      }
    };
  }, [source.id]);

  const connect = async () => {
    setPhase({ kind: "starting" });
    let code: DeviceCodeVm;
    try {
      code = await forgeConnectStart(source.id);
    } catch (raw) {
      if (mounted.current) {
        setPhase({ kind: "idle", sentence: syncErrorMessage(raw) });
      }
      return;
    }
    if (!mounted.current) {
      void forgeConnectCancel(source.id).catch(() => {});
      return;
    }
    waiting.current = true;
    setPhase({ kind: "waiting", code, copied: false, problem: null });
    try {
      const next = await forgeConnectWait(source.id);
      waiting.current = false;
      // Published whatever it says: `connected` swaps this panel for the list,
      // anything else comes back here with Rust's sentence for why.
      putForgeSource(next);
      if (mounted.current) {
        setPhase({ kind: "idle", sentence: next.sentence });
      }
    } catch (raw) {
      waiting.current = false;
      if (mounted.current) {
        setPhase({ kind: "idle", sentence: syncErrorMessage(raw) });
      }
    }
  };

  // A copy or an open that failed says so beside the buttons (surface #15):
  // pressing `Open github.com` and seeing nothing happen is the one outcome
  // this panel must not have. The code stays on screen, selectable.
  const report = (sentence: string | null, copied?: boolean) => {
    if (mounted.current) {
      setPhase((held) =>
        held.kind === "waiting"
          ? { ...held, problem: sentence, copied: copied ?? held.copied }
          : held,
      );
    }
  };
  const copy = (userCode: string) => {
    const clipboard: Clipboard | undefined = navigator.clipboard;
    if (clipboard === undefined) {
      report(BROWSE_COPY_FAILED, false);
      return;
    }
    clipboard.writeText(userCode).then(
      () => report(null, true),
      () => report(BROWSE_COPY_FAILED, false),
    );
  };

  if (phase.kind !== "waiting") {
    const appsUrl = source.appsUrl;
    return (
      <div className="flex flex-col items-start gap-3 p-4">
        {disconnected && source.kind === "github" && (
          <p className="text-muted-foreground">
            {BROWSE_DISCONNECTED_NOTE}
            {appsUrl !== null && (
              <>
                {" "}
                <Button
                  type="button"
                  variant="link"
                  className="h-auto p-0"
                  onClick={() =>
                    void openUrl(appsUrl).catch((raw) =>
                      setPhase({ kind: "idle", sentence: syncErrorMessage(raw) }),
                    )
                  }
                >
                  {BROWSE_GITHUB_APPS_LABEL}
                </Button>
              </>
            )}
          </p>
        )}
        <p>{browseConnectIntro(source.name)}</p>
        {phase.kind === "idle" && phase.sentence !== null && (
          <p role="status" className="text-muted-foreground">
            {phase.sentence}
          </p>
        )}
        <Button type="button" disabled={phase.kind === "starting"} onClick={() => void connect()}>
          {phase.kind === "starting" && (
            <LoaderCircle aria-hidden="true" className="animate-spin" />
          )}
          {browseConnectLabel(source.name)}
        </Button>
      </div>
    );
  }

  const { code } = phase;
  return (
    <div className="flex flex-col items-start gap-4 p-4">
      <div className="flex flex-col gap-1">
        <span className="label-caps text-faint" id={`${source.id}-code-label`}>
          {BROWSE_CODE_LABEL}
        </span>
        <output
          aria-labelledby={`${source.id}-code-label`}
          className="select-all font-mono text-display"
        >
          {code.userCode}
        </output>
        <span className="text-muted-foreground text-xs">{browseCodeLifetime(code.expiresIn)}</span>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          onClick={() => {
            copy(code.userCode);
            forgeConnectOpen(source.id).catch((raw) => report(syncErrorMessage(raw)));
          }}
        >
          {browseOpenLabel(source.host)}
        </Button>
        <Button type="button" variant="outline" onClick={() => copy(code.userCode)}>
          {BROWSE_COPY_CODE_LABEL}
        </Button>
        {phase.copied && (
          <span role="status" className="flex items-center gap-1 text-muted-foreground text-xs">
            <Check aria-hidden="true" className="size-3.5" />
            {BROWSE_CODE_COPIED}
          </span>
        )}
      </div>
      {phase.problem !== null && (
        <p role="alert" className="text-destructive text-xs">
          {phase.problem}
        </p>
      )}
      <div className="flex items-center gap-2">
        <LoaderCircle aria-hidden="true" className="size-4 animate-spin text-muted-foreground" />
        <span className="text-muted-foreground">{browseWaitingSentence(source.name)}</span>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => void forgeConnectCancel(source.id).catch(() => {})}
        >
          {BROWSE_CANCEL_LABEL}
        </Button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------

const OWNER_ALL = "\u0000all";

function RepoBrowser({
  source,
  listing,
  filters,
  onFilters,
  selected,
  picked,
  onSelected,
  onRefresh,
  onRetry,
  onAddOne,
  onBatch,
  onOpenDrive,
  onDisconnect,
}: {
  source: ForgeSourceVm;
  listing: Listing;
  filters: RepoFilters;
  onFilters: (next: RepoFilters) => void;
  selected: readonly string[];
  /** How many ticks a batch would add ({@link pickedRepos}), which the footer counts. */
  picked: number;
  onSelected: (next: readonly string[]) => void;
  onRefresh: () => void;
  onRetry: () => void;
  onAddOne: (repo: ForgeRepoVm) => void;
  onBatch: () => void;
  onOpenDrive: () => void;
  onDisconnect: () => Promise<void>;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLFieldSetElement>(null);
  const [disconnectError, setDisconnectError] = useState<string | null>(null);
  const vm = listing.kind === "ready" ? listing.vm : null;
  const visible = vm === null ? [] : filterRepos(vm.repos, filters);
  const groups = vm === null ? [] : groupRepos(visible, vm.owners, filters.sort);
  const byName = new Map(visible.map((repo) => [repo.fullName, repo]));
  const toggle = (fullName: string, on: boolean) =>
    onSelected(on ? [...selected, fullName] : selected.filter((name) => name !== fullName));

  // ⌘A picks every visible repository not already here, from anywhere in the
  // sheet — the toolbar, the sheet body, a row — except where it means "select
  // this text" (surface #16). Listened for on the sheet itself, because focus
  // on the sheet's own body or its close button never reaches this subtree.
  const selectAll = useRef<() => void>(() => {});
  useLayoutEffect(() => {
    selectAll.current = () => {
      const addable = visible
        .filter((repo) => repo.addedAs.length === 0)
        .map((repo) => repo.fullName);
      onSelected([...new Set([...selected, ...addable])]);
    };
  });
  useEffect(() => {
    const sheet = rootRef.current?.closest<HTMLElement>('[data-slot="sheet-content"]');
    if (sheet === null || sheet === undefined) {
      return;
    }
    const onSheetKey = (event: globalThis.KeyboardEvent) => {
      const target = event.target;
      const typing =
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        (target instanceof HTMLElement && target.isContentEditable);
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "a" && !typing) {
        event.preventDefault();
        selectAll.current();
      }
    };
    sheet.addEventListener("keydown", onSheetKey);
    return () => sheet.removeEventListener("keydown", onSheetKey);
  }, []);

  // Up/Down move by row from whatever in a row has focus — its checkbox, its
  // `Add…`, the name of the drive it syncs as — to the next row's checkbox, or
  // to its first button where the row is already added (surface #16).
  const onKeyDown = (event: KeyboardEvent<HTMLFieldSetElement>) => {
    const focused = document.activeElement;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const rows = [...(listRef.current?.querySelectorAll<HTMLElement>("[data-repo-row]") ?? [])];
      const index = rows.findIndex((row) => row.contains(focused));
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = rows[index === -1 ? 0 : Math.min(rows.length - 1, Math.max(0, index + step))];
      (
        next?.querySelector<HTMLElement>("[data-repo-check]:not(:disabled)") ??
        next?.querySelector<HTMLElement>("button:not(:disabled)")
      )?.focus();
      return;
    }
    if (
      event.key === "Enter" &&
      focused instanceof HTMLElement &&
      focused.dataset.repoCheck !== undefined
    ) {
      const repo = byName.get(focused.dataset.fullName ?? "");
      if (repo !== undefined) {
        event.preventDefault();
        onAddOne(repo);
      }
    }
  };

  const fetched =
    vm === null || vm.fetchedMs === null
      ? BROWSE_REFRESH_LABEL
      : `Fetched ${formatDraftAge(vm.fetchedMs)}`;

  return (
    <div ref={rootRef} className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-border border-b px-4 py-2">
        <Input
          type="search"
          aria-label={BROWSE_SEARCH_LABEL}
          placeholder={BROWSE_SEARCH_LABEL}
          className="h-8 min-w-40 flex-1"
          value={filters.query}
          onChange={(event) => onFilters({ ...filters, query: event.target.value })}
        />
        <Select
          value={filters.owner ?? OWNER_ALL}
          onValueChange={(owner) =>
            onFilters({ ...filters, owner: owner === OWNER_ALL ? null : owner })
          }
        >
          <SelectTrigger size="sm" aria-label={BROWSE_OWNER_LABEL} className="max-w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={OWNER_ALL}>{BROWSE_OWNER_ALL}</SelectItem>
            {vm?.owners.map((owner) => (
              <SelectItem key={owner.login} value={owner.login}>
                {owner.isYou ? BROWSE_OWNER_YOU : owner.login}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {(
          [
            ["forks", BROWSE_FORKS_LABEL],
            ["archived", BROWSE_ARCHIVED_LABEL],
          ] as const
        ).map(([key, label]) => (
          <Button
            key={key}
            type="button"
            variant="outline"
            size="sm"
            aria-pressed={filters[key]}
            onClick={() => onFilters({ ...filters, [key]: !filters[key] })}
          >
            {/* The shape says "on", not a fill colour alone (DESIGN.md). */}
            {filters[key] && <Check aria-hidden="true" />}
            {label}
          </Button>
        ))}
        <Select
          value={filters.sort}
          onValueChange={(sort) => onFilters({ ...filters, sort: sort as RepoSort })}
        >
          <SelectTrigger size="sm" aria-label={BROWSE_SORT_LABEL}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {(Object.keys(BROWSE_SORT_LABELS) as RepoSort[]).map((sort) => (
              <SelectItem key={sort} value={sort}>
                {BROWSE_SORT_LABELS[sort]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <IconHint label={fetched}>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label={BROWSE_REFRESH_LABEL}
            disabled={
              listing.kind === "loading" || (listing.kind === "ready" && listing.refreshing)
            }
            onClick={onRefresh}
          >
            <RefreshCw
              aria-hidden="true"
              className={cn(
                listing.kind === "ready" && listing.refreshing && "motion-safe:animate-spin",
              )}
            />
          </Button>
        </IconHint>
      </div>

      {/* The list scrolls under its sticky owner headers; toolbar and footer stay. */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {vm !== null && vm.notices.length > 0 && (
          <div className="flex flex-col gap-2 border-border border-b p-4">
            {vm.notices.map((notice) => (
              <Alert key={notice.sentence} role="note">
                <Info aria-hidden="true" />
                <AlertDescription>
                  <span>{notice.sentence}</span>
                  {notice.link !== null && (
                    <Button
                      type="button"
                      variant="link"
                      className="h-auto p-0 text-sm"
                      onClick={() => {
                        const link = notice.link;
                        if (link !== null) {
                          void openUrl(link).catch(() => {});
                        }
                      }}
                    >
                      {browseOpenLabel(hostOf(notice.link))}
                    </Button>
                  )}
                </AlertDescription>
              </Alert>
            ))}
          </div>
        )}
        {listing.kind === "loading" && (
          <div aria-busy="true" className="flex flex-col gap-2 p-4">
            <span role="status" className="sr-only">
              {BROWSE_LOADING}
            </span>
            {[0, 1, 2, 3, 4, 5].map((row) => (
              <Skeleton key={row} className="h-12 w-full" />
            ))}
          </div>
        )}
        {listing.kind === "failed" && (
          <div className="flex flex-col items-start gap-3 p-4">
            <p role="alert" className="text-destructive">
              {listing.sentence}
            </p>
            <Button type="button" variant="outline" size="sm" onClick={onRetry}>
              {BROWSE_TRY_AGAIN_LABEL}
            </Button>
          </div>
        )}
        {vm !== null && vm.repos.length === 0 && (
          <p className="p-4 text-muted-foreground">{BROWSE_EMPTY}</p>
        )}
        {vm !== null && vm.repos.length > 0 && visible.length === 0 && (
          <p className="p-4 text-muted-foreground">{BROWSE_NO_MATCH}</p>
        )}
        {groups.length > 0 && (
          // A group of checkboxes, which is what a fieldset is; the list's
          // keyboard (Up/Down, Enter, ⌘A) is delegated to it from the rows.
          <fieldset
            ref={listRef}
            aria-label={BROWSE_LIST_LABEL}
            className="m-0 min-w-0 border-0 p-0"
            onKeyDown={onKeyDown}
          >
            {groups.map((group) => (
              <section key={group.login} aria-labelledby={`repo-group-${group.login}`}>
                {/* Not `label-caps`: the heading is a login, and an identifier in
                    the wrong case is a wrong identifier. */}
                <h3
                  id={`repo-group-${group.login}`}
                  className="sticky top-0 z-10 flex items-center justify-between gap-2 border-border border-b bg-popover px-4 py-1.5 font-heading font-semibold text-sm"
                >
                  <span className="truncate">
                    {group.login}
                    {group.isYou && (
                      <span className="font-normal text-muted-foreground">
                        {" "}
                        · {BROWSE_OWNER_YOU}
                      </span>
                    )}
                  </span>
                  <span className="font-mono text-muted-foreground text-xs tabular-nums">
                    {group.repos.length}
                  </span>
                </h3>
                <ul>
                  {group.repos.map((repo) => (
                    <RepoRow
                      key={repo.fullName}
                      repo={repo}
                      showOwner={filters.owner === null}
                      checked={selected.includes(repo.fullName)}
                      onChecked={(on) => toggle(repo.fullName, on)}
                      onAddOne={() => onAddOne(repo)}
                      onOpenDrive={onOpenDrive}
                    />
                  ))}
                </ul>
              </section>
            ))}
          </fieldset>
        )}
        {/* A device-flow connection this device holds can be dropped here;
            `canConnect` says one can exist for this source, whatever its `via`
            (a broker source falls back to one when the broker has no grants). */}
        {source.canConnect && (
          <div className="flex items-center gap-2 px-4 py-3 text-muted-foreground text-xs">
            {source.login !== null && <span>{browseConnectedAs(source.login)}</span>}
            <Button
              type="button"
              variant="ghost"
              size="xs"
              onClick={() => {
                setDisconnectError(null);
                void onDisconnect().catch((raw) => setDisconnectError(syncErrorMessage(raw)));
              }}
            >
              {BROWSE_DISCONNECT_LABEL}
            </Button>
            {disconnectError !== null && (
              <span role="alert" className="text-destructive">
                {disconnectError}
              </span>
            )}
          </div>
        )}
      </div>

      {picked > 0 && (
        <div className="flex items-center gap-2 border-border border-t px-4 py-3">
          <span className="font-mono text-xs tabular-nums">{browseSelected(picked)}</span>
          <Button type="button" variant="ghost" size="sm" onClick={() => onSelected([])}>
            {BROWSE_CLEAR_LABEL}
          </Button>
          <Button type="button" size="sm" className="ml-auto" onClick={onBatch}>
            {browseAddDrives(picked, true)}
          </Button>
        </div>
      )}
    </div>
  );
}

function hostOf(link: string): string {
  try {
    return new URL(link).host;
  } catch {
    return link;
  }
}

function RepoRow({
  repo,
  showOwner,
  checked,
  onChecked,
  onAddOne,
  onOpenDrive,
}: {
  repo: ForgeRepoVm;
  showOwner: boolean;
  checked: boolean;
  onChecked: (on: boolean) => void;
  onAddOne: () => void;
  onOpenDrive: () => void;
}) {
  const added = repo.addedAs.length > 0;
  const updated = repo.updatedMs === null ? "" : formatDraftAge(repo.updatedMs);
  return (
    <li
      data-testid="forge-repo-row"
      data-repo-row=""
      className="flex items-start gap-3 border-border border-b px-4 py-2 last:border-b-0"
    >
      <Checkbox
        data-repo-check=""
        data-full-name={repo.fullName}
        aria-label={repo.fullName}
        className="mt-1"
        checked={checked && !added}
        disabled={added}
        onCheckedChange={(next) => onChecked(next === true)}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="truncate" title={repo.fullName}>
            {showOwner && <span className="text-muted-foreground">{repo.owner}/</span>}
            <strong className="font-semibold">{repo.name}</strong>
          </span>
          {repo.private && (
            <Lock
              role="img"
              aria-label={BROWSE_PRIVATE_LABEL}
              className="size-3.5 shrink-0 text-muted-foreground"
            />
          )}
          {BROWSE_CHIPS.map(
            ([flag, label]) =>
              repo[flag] && (
                <Badge key={flag} variant="outline" className="rounded-sm">
                  {label}
                </Badge>
              ),
          )}
        </div>
        {repo.description !== null && repo.description !== "" && (
          <p className="truncate text-muted-foreground text-xs" title={repo.description}>
            {repo.description}
          </p>
        )}
        {/* Rust's one sentence for a repository keeper can only read — no push
            access, archived, or a mirror — so the person knows before adding
            that the drive will only download. Muted: a fact, not a warning. */}
        {repo.pullOnly && repo.pullOnlySentence !== null && (
          <p className="flex min-w-0 items-center gap-1 text-muted-foreground text-xs">
            <ArrowDownToLine aria-hidden="true" className="size-3.5 shrink-0" />
            <span className="truncate" title={repo.pullOnlySentence}>
              {repo.pullOnlySentence}
            </span>
          </p>
        )}
        {(updated !== "" || repo.sizeKb !== null) && (
          <p className="flex gap-2 text-meta text-muted-foreground">
            {updated !== "" && <span>Updated {updated}</span>}
            {repo.sizeKb !== null && (
              <span className="font-mono tabular-nums">{formatFileSize(repo.sizeKb * 1024)}</span>
            )}
          </p>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2 text-xs">
        {added ? (
          <span className="flex items-center gap-1" data-testid="forge-repo-added">
            <Check aria-hidden="true" className="size-3.5" />
            {BROWSE_SYNCING_HERE}{" "}
            {repo.addedAs.map((name, index) => (
              <span key={name}>
                {index > 0 && ", "}
                <Button
                  type="button"
                  variant="link"
                  size="xs"
                  className="h-auto p-0"
                  onClick={onOpenDrive}
                >
                  {name}
                </Button>
              </span>
            ))}
          </span>
        ) : (
          <>
            {repo.elsewhere.length > 0 && (
              <span className="text-muted-foreground">{browseElsewhere(repo.elsewhere)}</span>
            )}
            <Button
              type="button"
              variant="outline"
              size="xs"
              aria-label={`${BROWSE_ADD_ONE_LABEL} ${repo.fullName}`}
              onClick={onAddOne}
            >
              {BROWSE_ADD_ONE_LABEL}
            </Button>
          </>
        )}
      </div>
    </li>
  );
}

// ---------------------------------------------------------------------------
// The batch step
// ---------------------------------------------------------------------------

type RowResult = { kind: "added" } | { kind: "failed"; sentence: string };

/**
 * Many drives at once (AD-336): one base folder, a preview of where each goes
 * with its name editable, and Rust's result per repository. A row that cannot
 * go — a name used twice in this list, a folder another drive already has, or
 * (once Rust has looked) a folder holding other files — says so beside itself
 * and is left out; the rest go.
 */
function BatchStep({
  source,
  repos,
  onBack,
  onDone,
  onAdded,
}: {
  source: ForgeSourceVm;
  repos: ForgeRepoVm[];
  onBack: () => void;
  onDone: () => void;
  onAdded: (fullNames: string[]) => void;
}) {
  // The repositories as they were chosen: the list behind re-reads after an
  // add and drops them from the selection, but this step keeps showing them.
  const [rows] = useState(repos);
  // The phone keeps drives in its container and Rust picks the folder there;
  // every desktop asks, whether or not Rust had a default to offer (#18). A
  // desktop with no default (no drives, no HOME) gets the field empty.
  const phone = useIsReducedCapabilityPlatform();
  const [base, setBase] = useState<string | undefined>(undefined);
  const [names, setNames] = useState<Record<string, string>>(() =>
    Object.fromEntries(repos.map((repo) => [repo.fullName, repo.name])),
  );
  const [results, setResults] = useState<Record<string, RowResult>>({});
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const profiles = useSyncStore((s) => s.profiles);

  useEffect(() => {
    if (phone) {
      return;
    }
    let live = true;
    forgeDefaultBaseFolder().then(
      (folder) => {
        if (live) setBase(folder ?? "");
      },
      // Unanswered: the field is still there, empty, to fill in.
      () => {
        if (live) setBase("");
      },
    );
    return () => {
      live = false;
    };
  }, [phone]);

  const pending = rows.filter((repo) => results[repo.fullName]?.kind !== "added");
  const trimmedBase = phone || base === undefined ? null : base.trim();
  const conflicts = batchConflicts(
    pending.map((repo) => ({ fullName: repo.fullName, driveName: names[repo.fullName] ?? "" })),
    trimmedBase,
    profiles ?? [],
  );
  const baseMissing = trimmedBase === "";
  const sendable =
    (!phone && base === undefined) || baseMissing
      ? []
      : pending.filter((repo) => !conflicts.has(repo.fullName));
  const addedCount = rows.length - pending.length;
  const allAdded = pending.length === 0;

  const run = async () => {
    const sending = sendable;
    setRunning(true);
    setError(null);
    let answer: ForgeAddResultVm[];
    try {
      answer = await forgeReposAdd({
        sourceId: source.id,
        baseFolder: trimmedBase,
        repos: sending.map((repo) => ({
          fullName: repo.fullName,
          driveName: (names[repo.fullName] ?? repo.name).trim(),
          folder: null,
        })),
      });
    } catch (raw) {
      // Rejected only when the source itself failed; its state says why.
      setError(syncErrorMessage(raw));
      setRunning(false);
      void refreshForgeSources();
      return;
    }
    const next: Record<string, RowResult> = {};
    for (const repo of sending) {
      const result = answer.find((candidate) => candidate.fullName === repo.fullName);
      next[repo.fullName] =
        result?.profileId !== null && result?.profileId !== undefined
          ? { kind: "added" }
          : { kind: "failed", sentence: result?.sentence ?? BROWSE_NO_RESULT };
    }
    setResults((held) => ({ ...held, ...next }));
    setRunning(false);
    const added = Object.keys(next).filter((fullName) => next[fullName].kind === "added");
    if (added.length > 0) {
      void refreshSyncProfiles();
      onAdded(added);
    }
  };

  const chooseBase = async () => {
    const selection = await openFolder({ directory: true, defaultPath: trimmedBase ?? undefined });
    if (typeof selection === "string") {
      setBase(selection);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="self-start"
          disabled={running}
          onClick={onBack}
        >
          <ChevronLeft aria-hidden="true" />
          {BROWSE_BACK_LABEL}
        </Button>
        {/* Absent only where Rust picks the folder itself (the phone's container). */}
        {!phone && (
          <div className="flex flex-col gap-2">
            <h3 className="font-heading text-title">{BROWSE_WHERE_TITLE}</h3>
            <div className="flex items-center gap-2">
              <Label htmlFor="forge-batch-base" className="sr-only">
                {BROWSE_BASE_LABEL}
              </Label>
              <Input
                id="forge-batch-base"
                className="min-w-0 flex-1 font-mono"
                value={base ?? ""}
                disabled={base === undefined || running}
                onChange={(event) => setBase(event.target.value)}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={running}
                onClick={() => void chooseBase().catch(() => {})}
              >
                {SYNC_CHOOSE_FOLDER_LABEL}
              </Button>
            </div>
            {baseMissing && <p className="text-destructive text-xs">{BROWSE_BASE_MISSING}</p>}
          </div>
        )}
        <ul aria-label={BROWSE_LIST_LABEL} className="flex flex-col">
          {rows.map((repo) => {
            const result = results[repo.fullName];
            const conflict = conflicts.get(repo.fullName);
            const inFlight = running && sendable.includes(repo);
            const name = names[repo.fullName] ?? "";
            const folder = batchFolder(trimmedBase, name);
            const problem =
              conflict ?? (result?.kind === "failed" && !inFlight ? result.sentence : undefined);
            return (
              <li
                key={repo.fullName}
                data-testid="forge-batch-row"
                className="flex items-start gap-3 border-border border-b py-2 last:border-b-0"
              >
                <div className="flex min-w-0 flex-1 flex-col gap-1">
                  <div className="flex min-w-0 items-center gap-1.5">
                    <span className="truncate text-muted-foreground text-xs">{repo.fullName}</span>
                    {/* Added as pull-only (Rust's direction for it), said before
                        the add rather than discovered after it. */}
                    {repo.pullOnly && (
                      <Badge
                        variant="outline"
                        className="shrink-0 rounded-sm"
                        title={repo.pullOnlySentence ?? undefined}
                      >
                        <ArrowDownToLine aria-hidden="true" />
                        {BROWSE_DOWNLOAD_ONLY}
                      </Badge>
                    )}
                  </div>
                  {result?.kind === "added" ? (
                    <span className="font-medium">{name.trim()}</span>
                  ) : (
                    <Input
                      aria-label={browseDriveNameLabel(repo.fullName)}
                      aria-invalid={problem !== undefined}
                      className="h-8"
                      value={name}
                      disabled={running}
                      onChange={(event) => {
                        setNames((held) => ({ ...held, [repo.fullName]: event.target.value }));
                        // A result was about the name it ran under; renamed,
                        // the row is a new question until the next run (#11).
                        setResults((held) => {
                          const { [repo.fullName]: _stale, ...rest } = held;
                          return rest;
                        });
                      }}
                    />
                  )}
                  {folder !== null && (
                    <span
                      className="truncate font-mono text-muted-foreground text-xs"
                      title={folder}
                    >
                      {folder}
                    </span>
                  )}
                  {problem !== undefined && <p className="text-destructive text-xs">{problem}</p>}
                </div>
                <div className="flex h-8 shrink-0 items-center text-xs">
                  {inFlight && (
                    <LoaderCircle
                      role="img"
                      aria-label={`Adding ${repo.fullName}`}
                      className="size-4 animate-spin text-muted-foreground"
                    />
                  )}
                  {!inFlight && result?.kind === "added" && (
                    <span className="flex items-center gap-1">
                      <Check aria-hidden="true" className="size-3.5" />
                      {BROWSE_ADDED_ROW}
                    </span>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
        <p className="text-muted-foreground text-xs">{browseCredentialLine(source.name)}</p>
        {addedCount > 0 && (
          <p role="status" className="font-medium">
            {browseAddedSentence(addedCount)}
          </p>
        )}
        {error !== null && (
          <p role="alert" className="text-destructive text-xs">
            {error}
          </p>
        )}
      </div>
      <div className="flex items-center justify-end gap-2 border-border border-t px-4 py-3">
        {(addedCount > 0 || Object.keys(results).length > 0) && (
          <Button type="button" variant={allAdded ? "default" : "ghost"} size="sm" onClick={onDone}>
            {BROWSE_DONE_LABEL}
          </Button>
        )}
        {!allAdded && (
          <Button
            type="button"
            size="sm"
            disabled={running || sendable.length === 0}
            onClick={() => void run()}
          >
            {running && <LoaderCircle aria-hidden="true" className="animate-spin" />}
            {browseAddDrives(sendable.length, false)}
          </Button>
        )}
      </div>
    </div>
  );
}
