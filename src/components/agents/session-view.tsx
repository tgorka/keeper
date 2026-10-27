import { Archive, ArchiveRestore, ArrowLeft, ShieldAlert } from "lucide-react";
import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { AgentAnswer } from "@/components/agents/agent-answer";
import { DatabaseApprovalCard } from "@/components/agents/approval-card";
import { SessionQuestionsPanel } from "@/components/agents/decisions-panel";
import { MINE_ROW, PersonLine } from "@/components/agents/person-line";
import { useSessionAttachmentDraft } from "@/components/agents/session-attachment-draft";
import { SessionAttachments } from "@/components/agents/session-attachments";
import { SessionContext, SessionContextTrigger } from "@/components/agents/session-context";
import { SessionControls } from "@/components/agents/session-controls";
import { SessionDrives } from "@/components/agents/session-drives";
import { SessionInstances } from "@/components/agents/session-instances";
import { SessionChildThread } from "@/components/agents/session-mention";
import { SessionPeople } from "@/components/agents/session-people";
import { SessionSettingsPicker } from "@/components/agents/session-settings-picker";
import { SessionAnswerDetails, SessionLiveTurn, TurnFold } from "@/components/agents/session-turn";
import { SessionUntrusted } from "@/components/agents/session-untrusted";
import { announcedElsewhere, announcedRejection, Composer } from "@/components/chat/composer";
import { Button } from "@/components/ui/button";
import { BAR_CLASS, useBarFold } from "@/hooks/use-bar-fold";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { logFailure } from "@/lib/failure";
import { formatMessageTime } from "@/lib/format-time";
import {
  type AgentMessageVm,
  type AgentSessionRef,
  type AgentSessionViewVm,
  agentAttachBytes,
  agentAttachCommit,
  agentAttachPath,
  agentAttachRelease,
  agentCommandPreview,
  agentMentionPreview,
  agentSend,
  agentSendLine,
  agentSessionArchive,
  agentSessionClose,
  agentSessionOpen,
  agentSessionPageBack,
  agentSessionUnarchive,
  hqStatusSubscribe,
  hqStatusUnsubscribe,
} from "@/lib/ipc/client";
import { runAgentCommand } from "@/lib/stores/agent-commands";
import { agentSessionsStore, useAgentSessionsStore } from "@/lib/stores/agent-sessions";
import { attachmentId, type PendingAttachment } from "@/lib/stores/attachments";
import {
  ConversationStoresProvider,
  createConversationStores,
} from "@/lib/stores/conversation-stores";
import { syncErrorMessage } from "@/lib/stores/sync";

/**
 * Mount keyed by (database, sessionId): no old page, draft or action can leak across databases.
 * `onSession` hears the session a command (`/clean`, `/new`) started, for a host such as the
 * Notes Assistant dock that holds its own choice: given one, the command moves that host and
 * leaves the Agents window's selection where it was (R601-04); without one, it moves the window.
 */
export function SessionView({
  target,
  onBack,
  onSession,
}: {
  target: AgentSessionRef;
  onBack?: () => void;
  onSession?: (session: AgentSessionRef) => void;
}) {
  const attachmentDraft = useSessionAttachmentDraft(target);
  const [vm, setVm] = useState<AgentSessionViewVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [peopleOpen, setPeopleOpen] = useState(false);
  const [toolsOpen, setToolsOpen] = useState(false);
  const [drivesOpen, setDrivesOpen] = useState(false);
  const [contextOpen, setContextOpen] = useState(false);
  // Rust's sentence while the session is not on this device yet but may still arrive.
  const [arriving, setArriving] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // A send from under the banner that failed: said in the banner too, since its requests
  // may still wait (R10911-06).
  const [bannerFailure, setBannerFailure] = useState<string | null>(null);
  // A request decided from the banner: once Rust moves it to its turn, focus follows (R10911-12).
  const [decidedFromBanner, setDecidedFromBanner] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [stores] = useState(createConversationStores);
  const viewport = useRef<HTMLDivElement>(null);
  const backButton = useRef<HTMLButtonElement>(null);
  /** The archived session's read-only bar, folded as the composer's is (R601-01). */
  const readOnlyBar = useRef<HTMLDivElement>(null);
  useBarFold(readOnlyBar, vm?.readOnly === true);
  const { phone } = useShellLayout();
  const { database, sessionId } = target;
  // Drives… in the list's right-click menu asks for this view's menu. Rust's first view of this
  // session answers it: with drives the menu opens; without (the role or the agent's profile
  // changed since the row was drawn) the request is dropped, so it never opens later on its own.
  const drivesFor = useAgentSessionsStore((state) => state.drivesFor);
  const viewed = vm !== null;
  const hasDrives = vm?.drives !== undefined;
  useEffect(() => {
    if (!viewed || drivesFor?.database !== database || drivesFor.sessionId !== sessionId) return;
    if (hasDrives) setDrivesOpen(true);
    agentSessionsStore.getState().drivesShown();
  }, [viewed, hasDrives, drivesFor, database, sessionId]);
  // Model… in that menu asks the same way for the composer's model picker (A109-05).
  const [modelsOpen, setModelsOpen] = useState(false);
  const modelFor = useAgentSessionsStore((state) => state.modelFor);
  const hasModels = vm?.footer.choosesModel === true;
  useEffect(() => {
    if (!viewed || modelFor?.database !== database || modelFor.sessionId !== sessionId) return;
    if (hasModels) setModelsOpen(true);
    agentSessionsStore.getState().modelShown();
  }, [viewed, hasModels, modelFor, database, sessionId]);
  // Context… asks for the Details sheet, which every member may read: it opens with the first view.
  const contextFor = useAgentSessionsStore((state) => state.contextFor);
  useEffect(() => {
    if (!viewed || contextFor?.database !== database || contextFor.sessionId !== sessionId) return;
    setContextOpen(true);
    agentSessionsStore.getState().contextShown();
  }, [viewed, contextFor, database, sessionId]);
  const pinned = useRef(true);
  const resolveMention = useCallback(
    (before: string, after: string) => agentMentionPreview({ database, sessionId }, before, after),
    [database, sessionId],
  );
  const previousHeight = useRef<number | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: explicit retry restarts the failed subscription
  useEffect(() => {
    let active = true;
    let id: string | null = null;
    setError(null);
    void agentSessionOpen({ database, sessionId }, (next) => {
      if (active) setVm(next);
    })
      .then((subscription) => {
        if (active) {
          id = subscription;
          setArriving(null);
        } else void agentSessionClose(subscription).catch(logFailure("agents.session.close"));
      })
      .catch((raw: unknown) => {
        if (!active) return;
        const message = syncErrorMessage(raw, "keeper could not open this session.");
        // Rust says a refusal is retriable while the session may still arrive here.
        if (typeof raw === "object" && raw !== null && "retriable" in raw && raw.retriable) {
          setArriving(message);
        } else {
          setArriving(null);
          setError(message);
        }
      });
    return () => {
      active = false;
      if (id !== null) void agentSessionClose(id).catch(logFailure("agents.session.close"));
    };
  }, [database, sessionId, attempt]);

  // A session a notification named before this device held it (or before its copies
  // opened) opens when Rust's list brings it. That holds after a refusal too: the
  // session appearing in Rust's list is Rust saying it is here now, so the open is
  // asked again on that transition alone — never on a timer.
  const unopened = vm === null && (arriving !== null || error !== null);
  useEffect(() => {
    if (!unopened) return;
    return agentSessionsStore.subscribe((state, previous) => {
      const now = state.rows.some((r) => r.database === database && r.sessionId === sessionId);
      const before = previous.rows.some(
        (r) => r.database === database && r.sessionId === sessionId,
      );
      if (now && !before) setAttempt((n) => n + 1);
    });
  }, [unopened, database, sessionId]);

  // While Rust says the session may still arrive, its status moving — copies opened,
  // the device removed, the database no longer open — is asked about too, so a
  // session that will not come ends in Rust's sentence, not a wait.
  useEffect(() => {
    if (arriving === null) return;
    let active = true;
    let status: string | null = null;
    void hqStatusSubscribe(() => {
      if (active) setAttempt((n) => n + 1);
    })
      .then((subscription) => {
        if (active) status = subscription;
        else void hqStatusUnsubscribe(subscription).catch(logFailure("agents.session.arrival"));
      })
      .catch(logFailure("agents.session.arrival"));
    return () => {
      active = false;
      if (status !== null)
        void hqStatusUnsubscribe(status).catch(logFailure("agents.session.arrival"));
    };
  }, [arriving]);

  useEffect(() => {
    if (!vm) return;
    const node = viewport.current;
    if (!node) return;
    if (previousHeight.current !== null && !vm.loadingEarlier) {
      node.scrollTop += node.scrollHeight - previousHeight.current;
      previousHeight.current = null;
    } else if (pinned.current) node.scrollTop = node.scrollHeight;
  }, [vm]);

  useEffect(() => {
    if (vm?.complete && !phone) {
      stores.composer.getState().requestFocus();
    }
  }, [vm?.complete, phone, stores]);

  useEffect(() => {
    if (!vm?.complete || vm.readOnly) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          if (!active || event.payload.type !== "drop") return;
          stores.attachments.getState().addMany(
            event.payload.paths.map((path) => ({
              id: attachmentId(),
              kind: "path",
              path,
              filename: path.split(/[/\\]/).pop() || path,
            })),
          );
        }),
      )
      .then((release) => {
        if (active) unlisten = release;
        else release();
      })
      .catch(logFailure("agents.session.drop"));
    return () => {
      active = false;
      unlisten?.();
    };
  }, [vm?.complete, vm?.readOnly, stores]);

  // A decision taken in the banner moves its card to its turn's fold when Rust's next view has it.
  // Focus goes to that fold's toggle (the decided card is folded with the turn), else to the card
  // where Rust drew it in the column, else to the next request still in the banner, else to the
  // composer — never to the page's body.
  useEffect(() => {
    if (decidedFromBanner === null || !vm) return;
    if (vm.banner?.approvals.some((approval) => approval.id === decidedFromBanner)) return;
    setDecidedFromBanner(null);
    const turn = vm.folds.find((fold) =>
      fold.approvals.some((approval) => approval.id === decidedFromBanner),
    )?.turnId;
    const toggle =
      turn === undefined
        ? null
        : viewport.current?.querySelector<HTMLElement>(
            `[data-turn-id="${CSS.escape(turn)}"] [data-slot="turn-fold-toggle"]`,
          );
    const moved = viewport.current?.querySelector<HTMLElement>(
      `[data-approval-id="${CSS.escape(decidedFromBanner)}"]`,
    );
    const next = document.querySelector<HTMLElement>(
      '[data-slot="agent-approval-banner"] [data-approval-id] button',
    );
    const target = toggle ?? moved ?? next;
    if (target) target.focus();
    else stores.composer.getState().requestFocus();
  }, [vm, decidedFromBanner, stores]);
  // An answer that never moves the card (refused, then decided elsewhere) stops waiting for it.
  useEffect(() => {
    if (decidedFromBanner === null) return;
    const giveUp = setTimeout(() => setDecidedFromBanner(null), 10_000);
    return () => clearTimeout(giveUp);
  }, [decidedFromBanner]);

  // What the banner shows now: a message (or `/stop`) sent from under it declines exactly these,
  // at these digests, and nothing that arrives later (Q109-12 a, R10911-01).
  const declines = (vm?.banner?.approvals ?? []).map(({ id, bindingDigest }) => ({
    id,
    bindingDigest,
  }));
  // A refused send names a request that still waits; once the banner's requests change (one is
  // decided, another arrives), that sentence may be false, so it goes (R10911-16).
  const shownKey = JSON.stringify(declines);
  // biome-ignore lint/correctness/useExhaustiveDependencies: the banner's requests changing is the trigger
  useEffect(() => {
    setBannerFailure(null);
  }, [shownKey]);
  /** Run a write that declines what the banner shows, saying in the banner if it failed. */
  async function fromUnderBanner<T>(write: () => Promise<T>): Promise<T> {
    setBannerFailure(null);
    try {
      return await write();
    } catch (raw) {
      if (declines.length === 0) throw raw;
      setBannerFailure(syncErrorMessage(raw, "keeper could not send your message."));
      // Said in the banner; the composer keeps the draft and shows it unannounced.
      throw announcedRejection(raw);
    }
  }

  async function attach(pending: PendingAttachment[], caption?: string) {
    // Rust stages every selection under a handle (paths in one call per run of
    // them, each paste or photo as its raw body), then one commit writes them
    // all with the caption as ONE message. A staging refusal commits nothing:
    // what was staged is released and the whole selection stays in the tray.
    const handles: string[] = [];
    try {
      for (let index = 0; index < pending.length; ) {
        const attachment = pending[index];
        if (attachmentDraft.has(attachment.id)) {
          index += 1;
          handles.push(await attachmentDraft.stage(attachment));
        } else if (attachment.kind === "path") {
          const paths: string[] = [];
          while (index < pending.length) {
            const next = pending[index];
            if (next.kind !== "path" || attachmentDraft.has(next.id)) break;
            paths.push(next.path);
            index += 1;
          }
          handles.push(...(await agentAttachPath(target, paths)));
        } else {
          index += 1;
          handles.push(
            await agentAttachBytes(target, attachment.bytes, attachment.filename, attachment.mime),
          );
        }
      }
    } catch (raw) {
      if (handles.length > 0) {
        void agentAttachRelease(target, handles).catch(logFailure("agents.attach.release"));
      }
      attachmentDraft.consume(
        pending.map((attachment) => attachment.id),
        handles,
        true,
      );
      throw raw;
    }
    try {
      await fromUnderBanner(() => agentAttachCommit(target, handles, caption, declines));
    } catch (raw) {
      attachmentDraft.consume(
        pending.map((attachment) => attachment.id),
        handles,
        true,
      );
      throw raw;
    }
    attachmentDraft.consume(
      pending.map((attachment) => attachment.id),
      handles,
    );
  }

  const pageBack = async () => {
    if (!vm?.hasEarlier || vm.loadingEarlier || busy) return;
    previousHeight.current = viewport.current?.scrollHeight ?? null;
    pinned.current = false;
    setBusy(true);
    setError(null);
    try {
      await agentSessionPageBack(target);
    } catch (raw) {
      previousHeight.current = null;
      setError(syncErrorMessage(raw, "keeper could not load earlier messages."));
    } finally {
      setBusy(false);
    }
  };

  const archive = async () => {
    if (!vm || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (vm.archived) await agentSessionUnarchive(target);
      else await agentSessionArchive(target);
    } catch (raw) {
      setError(syncErrorMessage(raw, "keeper could not change this session."));
    } finally {
      setBusy(false);
    }
  };

  /**
   * Run a `/` line through the composer's one path (`runAgentCommand`), which the session list's
   * menu shares; only `/stop` declines what the banner shows (R10911-07).
   */
  async function runCommand(name: string, line: string) {
    // A host with its own choice (`onSession`) takes the new session; the Agents window keeps its
    // selection (R601-04). Without one, the window follows.
    const result =
      name === "stop"
        ? await fromUnderBanner(() =>
            runAgentCommand(target, vm?.agentActor, line, declines, onSession),
          )
        : await runAgentCommand(target, vm?.agentActor, line, [], onSession);
    if (result.surface === "people") setPeopleOpen(true);
    if (result.surface === "tools") setToolsOpen(true);
    if (result.surface === "drives") setDrivesOpen(true);
    if (result.surface === "context") setContextOpen(true);
    return result;
  }

  const messages = new Map(vm?.messages.map((message) => [message.id, message]));
  const folds = new Map(vm?.folds.map((fold) => [fold.turnId, fold]));
  const live = new Map(vm?.live.map((turn) => [turn.turnId, turn]));
  const children = new Map(vm?.children?.map((child) => [child.threadId, child]));
  const firstChild = vm?.items.flatMap((item) => (item.kind === "child" ? [item.threadId] : []))[0];

  function renderMessage(message: AgentMessageVm) {
    return (
      <li key={message.id} className={message.mine ? `min-w-0 ${MINE_ROW}` : "min-w-0"}>
        <div className="mb-1 flex flex-wrap items-baseline gap-x-2 text-xs">
          <span className="font-medium">{message.authorName}</span>
          <span className="text-muted-foreground">{formatMessageTime(message.createdAtMs)}</span>
          {message.state && <span className="text-muted-foreground">{message.state}</span>}
        </div>
        {message.role === "agent" ? (
          <AgentAnswer body={message.body} />
        ) : (
          <PersonLine body={message.body} mine={message.mine} />
        )}
        {!!message.attachments?.length && <SessionAttachments attachments={message.attachments} />}
        {message.details && <SessionAnswerDetails details={message.details} />}
        {message.ask && (
          <fieldset className="mt-2 flex min-w-0 flex-wrap gap-2" aria-label="Question options">
            {message.ask.open ? (
              <>
                {message.ask.options.map((option) => (
                  <Button
                    key={option}
                    variant="outline"
                    size="sm"
                    className="h-auto min-h-11 max-w-full whitespace-normal break-words text-left [overflow-wrap:anywhere]"
                    disabled={busy || vm?.readOnly}
                    onClick={async () => {
                      setBusy(true);
                      setError(null);
                      try {
                        await fromUnderBanner(() => agentSend(target, option, declines));
                      } catch (raw) {
                        setError(syncErrorMessage(raw, "keeper could not send your answer."));
                      } finally {
                        setBusy(false);
                      }
                    }}
                  >
                    {option}
                  </Button>
                ))}
                <span className="w-full text-muted-foreground text-xs">
                  Choose an option or type your answer below.
                </span>
              </>
            ) : (
              <span className="text-muted-foreground text-xs">Answered</span>
            )}
          </fieldset>
        )}
      </li>
    );
  }

  return (
    <section
      aria-label="Agent session"
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-background"
    >
      <header className="flex shrink-0 items-center gap-2 border-border border-b px-3 py-2">
        {onBack && (
          <Button
            ref={backButton}
            data-slot="agent-screen-back"
            variant="ghost"
            size="icon"
            className="size-11 shrink-0"
            aria-label="Back to sessions"
            onClick={onBack}
          >
            <ArrowLeft aria-hidden="true" />
          </Button>
        )}
        <div className="min-w-0 flex-1">
          <h2 className="truncate font-semibold text-title" title={vm?.displayName}>
            {vm?.displayName ?? "Opening session…"}
          </h2>
          <p className="flex min-w-0 flex-wrap items-center gap-x-2 text-muted-foreground text-xs">
            <span className="break-all">{database}</span>
            <span>{vm?.handle}</span>
            {vm && <span className="break-all">{vm.agentName}</span>}
            {vm?.archived && <span>Archived</span>}
            {vm?.untrusted && (
              <SessionUntrusted
                untrusted={vm.untrusted}
                phone={phone}
                onFreshChat={async () => {
                  try {
                    await runCommand("clean", "/clean");
                  } catch (raw) {
                    throw new Error(syncErrorMessage(raw, "keeper could not start a fresh chat."));
                  }
                }}
              />
            )}
          </p>
          {vm?.drives && (
            <SessionDrives
              target={target}
              agentName={vm.agentName}
              drives={vm.drives}
              open={drivesOpen}
              onOpenChange={setDrivesOpen}
            />
          )}
        </div>
        {vm && <SessionPeople target={target} open={peopleOpen} onOpenChange={setPeopleOpen} />}
        {vm && (
          <Button
            variant="ghost"
            size="icon"
            className="size-11 shrink-0"
            disabled={busy}
            aria-label={vm.archived ? "Unarchive session" : "Archive session"}
            onClick={() => void archive()}
          >
            {vm.archived ? <ArchiveRestore aria-hidden="true" /> : <Archive aria-hidden="true" />}
          </Button>
        )}
      </header>
      {vm?.answering && <SessionInstances answering={vm.answering} agentName={vm.agentName} />}
      {vm && <SessionControls vm={vm} toolsOpen={toolsOpen} onToolsOpenChange={setToolsOpen} />}
      {error && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-2 px-3 py-2 text-destructive text-sm"
        >
          <span>{error}</span>
          {!vm && (
            <Button variant="outline" size="sm" onClick={() => setAttempt((n) => n + 1)}>
              Try again
            </Button>
          )}
        </div>
      )}
      {arriving && !error && (
        <p role="status" className="px-3 py-2 text-muted-foreground text-sm">
          {arriving}
        </p>
      )}
      {vm?.note && (
        <p role="status" className="px-3 py-2 text-muted-foreground text-sm">
          {vm.note}
        </p>
      )}
      <div
        ref={viewport}
        className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-3"
        onScroll={(event) => {
          const node = event.currentTarget;
          pinned.current = node.scrollHeight - node.scrollTop - node.clientHeight < 48;
          if (node.scrollTop < 24 && vm?.complete) void pageBack();
        }}
      >
        {vm?.hasEarlier && (
          <Button
            variant="ghost"
            className="mb-3 w-full"
            disabled={busy || vm.loadingEarlier}
            onClick={() => {
              // The last page removes this button; keep keyboard Back reachable.
              if (phone) backButton.current?.focus();
              void pageBack();
            }}
          >
            {vm.loadingEarlier ? "Loading earlier messages…" : "Earlier messages"}
          </Button>
        )}
        {!vm?.complete && !error && !arriving && (
          <p role="status" className="text-muted-foreground text-sm">
            Loading messages…
          </p>
        )}
        {vm?.complete && vm.items.length === 0 && (
          <p className="text-muted-foreground text-sm">No messages in this session yet.</p>
        )}
        {vm && (
          <ol aria-label="Messages" className="mx-auto flex w-full max-w-3xl flex-col gap-5">
            {vm.items.map((item) => {
              // Rust's reading column, in its order: each entry names what it draws.
              if (item.kind === "message") {
                const message = messages.get(item.messageId);
                return message && renderMessage(message);
              }
              if (item.kind === "child") {
                // 109.24: a thread started here, where it was started; Rust's
                // sentence about threads left out sits above the first one.
                const child = children.get(item.threadId);
                return (
                  child && (
                    <Fragment key={`child:${item.threadId}`}>
                      {vm.childrenNote && item.threadId === firstChild && (
                        <li className="text-muted-foreground text-xs">{vm.childrenNote}</li>
                      )}
                      <li className="min-w-0">
                        <SessionChildThread
                          database={database}
                          child={child}
                          onOpen={(next) => agentSessionsStore.getState().select(next)}
                        />
                      </li>
                    </Fragment>
                  )
                );
              }
              if (item.kind === "fold") {
                const fold = folds.get(item.turnId);
                return (
                  fold && (
                    <TurnFold
                      key={`fold:${item.turnId}`}
                      fold={fold}
                      leadIns={fold.leadInIds.flatMap((id) => messages.get(id) ?? [])}
                      agentName={vm.agentName}
                      verbose={vm.prefs.viewMode === "verbose"}
                    />
                  )
                );
              }
              const turn = live.get(item.turnId);
              return (
                turn && (
                  <SessionLiveTurn
                    key={`live:${item.turnId}`}
                    turn={turn}
                    agentName={vm.agentName}
                  />
                )
              );
            })}
          </ol>
        )}
        {/* Only a request whose turn this view does not show: every other one is in its
            turn's fold, or in the banner. */}
        {!!vm?.approvals.length && (
          <section
            aria-label="Session approvals"
            className="mx-auto mt-5 flex w-full max-w-3xl flex-col gap-3"
          >
            {vm.approvals.map((approval) => (
              <DatabaseApprovalCard
                key={JSON.stringify([approval.id, approval.bindingDigest])}
                approval={approval}
              />
            ))}
          </section>
        )}
      </div>
      {vm && !vm.readOnly && <SessionQuestionsPanel messages={vm.messages} />}
      {vm?.banner && !vm.readOnly && (
        // Q109-12 a: what waits for this person sits above the composer, out of the
        // reading column, and says what a message would do to it.
        <aside
          aria-label="Waiting for your decision"
          data-slot="agent-approval-banner"
          className="max-h-[45dvh] shrink-0 overflow-y-auto border-border border-t bg-muted/30 px-3 py-2"
        >
          <div className="mx-auto flex w-full max-w-3xl flex-col gap-2">
            {vm.untrusted && (
              <p
                data-slot="agent-banner-untrusted"
                className="flex items-start gap-1.5 text-held text-sm [overflow-wrap:anywhere]"
              >
                <ShieldAlert aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
                <span>{vm.untrusted.sentence}</span>
              </p>
            )}
            <p role="note" className="text-sm [overflow-wrap:anywhere]">
              {vm.banner.sentence}
            </p>
            {bannerFailure && (
              <p role="alert" className="text-destructive text-sm [overflow-wrap:anywhere]">
                {bannerFailure}
              </p>
            )}
            {vm.banner.approvals.map((approval) => (
              <DatabaseApprovalCard
                key={JSON.stringify([approval.id, approval.bindingDigest])}
                approval={approval}
                onDecided={(decided) => setDecidedFromBanner(decided.id)}
              />
            ))}
          </div>
        </aside>
      )}
      {vm?.readOnly ? (
        // R598-03: an archived session keeps the bar's reading — model, effort and context —
        // read-only, with no composer; its untrusted chip stays in the header.
        <div
          data-slot="agent-session-readonly-bar"
          className="shrink-0 border-border border-t px-3 py-2"
        >
          <p className="px-1 py-1 text-muted-foreground text-sm">
            This archived session is read-only. Unarchive it to continue.
          </p>
          <div
            ref={readOnlyBar}
            className={`${BAR_CLASS} flex min-w-0 flex-wrap items-center gap-1`}
          >
            <SessionSettingsPicker target={target} agent={vm.agentActor} footer={vm.footer} />
            <div className="ml-auto shrink-0">
              <SessionContext context={vm.context} open={contextOpen} onOpenChange={setContextOpen}>
                <SessionContextTrigger context={vm.context} />
              </SessionContext>
            </div>
          </div>
        </div>
      ) : (
        <div data-slot="agent-session-composer" className="shrink-0 border-border border-t p-3">
          <SessionContext context={vm?.context} open={contextOpen} onOpenChange={setContextOpen}>
            <ConversationStoresProvider value={stores}>
              <Composer
                transport="session"
                accountId={database}
                roomId={sessionId}
                disabled={!vm?.complete}
                onSendAttachments={attach}
                resolveCommand={(draft) => agentCommandPreview(target, draft)}
                onCommand={async ({ name, args }) => {
                  try {
                    const line = `/${name}${args === null ? "" : ` ${args}`}`;
                    const result = await runCommand(name, line);
                    return result.message ? { tone: "done", message: result.message } : null;
                  } catch (raw) {
                    // A refused `/stop` from under the banner is said there, once.
                    if (announcedElsewhere(raw)) return null;
                    return {
                      tone: "failed",
                      message: syncErrorMessage(raw, "keeper could not run this command."),
                    };
                  }
                }}
                resolveMention={resolveMention}
                onSend={async (body) => {
                  await fromUnderBanner(() => agentSendLine(target, body, declines));
                }}
                attachmentPreview={attachmentDraft.preview}
                sendAccessory={vm && <SessionContextTrigger context={vm.context} />}
                // t3.chat's bar (owner ruling 2026-10-27): model and effort, always shown.
                barStart={
                  vm && (
                    <SessionSettingsPicker
                      target={target}
                      agent={vm.agentActor}
                      footer={vm.footer}
                      picker={vm.settings}
                      modelsOpen={modelsOpen}
                      onModelsOpenChange={setModelsOpen}
                    />
                  )
                }
              />
            </ConversationStoresProvider>
          </SessionContext>
        </div>
      )}
    </section>
  );
}
