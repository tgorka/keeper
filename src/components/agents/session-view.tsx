import { Archive, ArchiveRestore, ArrowLeft } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { AgentAnswer } from "@/components/agents/agent-answer";
import { DatabaseApprovalCard } from "@/components/agents/approval-card";
import { SessionQuestionsPanel } from "@/components/agents/decisions-panel";
import { SessionControls } from "@/components/agents/session-controls";
import {
  SessionAnswerDetails,
  SessionLiveTurn,
  SessionTrace,
} from "@/components/agents/session-turn";
import { Composer } from "@/components/chat/composer";
import { Button } from "@/components/ui/button";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { logFailure } from "@/lib/failure";
import { formatMessageTime } from "@/lib/format-time";
import {
  type AgentSessionRef,
  type AgentSessionViewVm,
  agentSend,
  agentSessionArchive,
  agentSessionClose,
  agentSessionOpen,
  agentSessionPageBack,
  agentSessionUnarchive,
} from "@/lib/ipc/client";
import { useAgentViewPrefs } from "@/lib/stores/agent-view-prefs";
import {
  ConversationStoresProvider,
  createConversationStores,
} from "@/lib/stores/conversation-stores";
import { syncErrorMessage } from "@/lib/stores/sync";

/** Mount keyed by (database, sessionId): no old page, draft or action can leak across databases. */
export function SessionView({ target, onBack }: { target: AgentSessionRef; onBack?: () => void }) {
  const [vm, setVm] = useState<AgentSessionViewVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [stores] = useState(createConversationStores);
  const viewport = useRef<HTMLDivElement>(null);
  const backButton = useRef<HTMLButtonElement>(null);
  const { phone } = useShellLayout();
  const { database, sessionId } = target;
  const pinned = useRef(true);
  const previousHeight = useRef<number | null>(null);
  const { value: prefs } = useAgentViewPrefs();

  // biome-ignore lint/correctness/useExhaustiveDependencies: explicit retry restarts the failed subscription
  useEffect(() => {
    let active = true;
    let id: string | null = null;
    setError(null);
    void agentSessionOpen({ database, sessionId }, (next) => {
      if (active) setVm(next);
    })
      .then((subscription) => {
        if (active) id = subscription;
        else void agentSessionClose(subscription).catch(logFailure("agents.session.close"));
      })
      .catch((raw: unknown) => {
        if (active) setError(syncErrorMessage(raw, "keeper could not open this session."));
      });
    return () => {
      active = false;
      if (id !== null) void agentSessionClose(id).catch(logFailure("agents.session.close"));
    };
  }, [database, sessionId, attempt]);

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
          <h2 className="truncate font-semibold text-title" title={vm?.title}>
            {vm?.title ?? "Opening session…"}
          </h2>
          <p className="flex min-w-0 flex-wrap gap-x-2 text-muted-foreground text-xs">
            <span className="break-all">{database}</span>
            {vm && <span className="break-all">{vm.agentName}</span>}
            {vm?.archived && <span>Archived</span>}
            {vm?.untrusted && (
              <span
                className="font-medium text-foreground"
                title="This session has read outside content. Consequential actions need a person."
              >
                Untrusted
              </span>
            )}
          </p>
        </div>
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
      {vm && <SessionControls vm={vm} />}
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
        {!vm?.complete && !error && (
          <p role="status" className="text-muted-foreground text-sm">
            Loading messages…
          </p>
        )}
        {vm?.complete && vm.messages.length === 0 && vm.live.length === 0 && (
          <p className="text-muted-foreground text-sm">No messages in this session yet.</p>
        )}
        <ol aria-label="Messages" className="mx-auto flex w-full max-w-3xl flex-col gap-5">
          {vm?.messages.map((message) => (
            <li key={message.id} className="min-w-0">
              <div className="mb-1 flex flex-wrap items-baseline gap-x-2 text-xs">
                <span className="font-medium">{message.authorName}</span>
                <span className="text-muted-foreground">
                  {formatMessageTime(message.createdAtMs)}
                </span>
                {message.state && <span className="text-muted-foreground">{message.state}</span>}
              </div>
              {message.role === "agent" ? (
                <AgentAnswer body={message.body} />
              ) : (
                <p className="whitespace-pre-wrap break-words text-sm [overflow-wrap:anywhere]">
                  {message.body}
                </p>
              )}
              {vm.readsTrace && (
                <SessionTrace
                  reasoning={prefs.showReasoning ? message.reasoning : undefined}
                  steps={message.steps}
                />
              )}
              {prefs.showDetails && message.details && (
                <SessionAnswerDetails details={message.details} />
              )}
              {message.ask && (
                <fieldset
                  className="mt-2 flex min-w-0 flex-wrap gap-2"
                  aria-label="Question options"
                >
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
                              await agentSend(target, option);
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
          ))}
          {vm?.live.map((turn) => (
            <SessionLiveTurn
              key={turn.turnId}
              turn={turn}
              agentName={vm.agentName}
              readsTrace={vm.readsTrace}
              showReasoning={prefs.showReasoning}
            />
          ))}
        </ol>
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
      {vm?.readOnly ? (
        <p className="border-border border-t px-4 py-3 text-muted-foreground text-sm">
          This archived session is read-only. Unarchive it to continue.
        </p>
      ) : (
        <div data-slot="agent-session-composer" className="shrink-0 border-border border-t p-3">
          <ConversationStoresProvider value={stores}>
            <Composer
              transport="session"
              accountId={database}
              roomId={sessionId}
              disabled={!vm?.complete}
              onSend={async (body) => {
                await agentSend(target, body);
              }}
            />
          </ConversationStoresProvider>
        </div>
      )}
    </section>
  );
}
