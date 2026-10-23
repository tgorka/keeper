import { Users } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { logFailure } from "@/lib/failure";
import {
  type AgentMemberVm,
  type AgentSessionMembersVm,
  type AgentSessionRef,
  agentSessionLeave,
  agentSessionMemberAdd,
  agentSessionMemberReaderSet,
  agentSessionMemberRemove,
  agentSessionMembersSubscribe,
  agentSessionMembersUnsubscribe,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** Mirrors Rust's People sheet for one (database, session); decides nothing itself. */
function useSessionMembers(target: AgentSessionRef) {
  const [sheet, setSheet] = useState<AgentSessionMembersVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { database, sessionId } = target;
  useEffect(() => {
    let active = true;
    let id: string | null = null;
    setError(null);
    void agentSessionMembersSubscribe({ database, sessionId }, (next) => {
      if (active) setSheet(next);
    })
      .then((subscription) => {
        if (active) id = subscription;
        else
          void agentSessionMembersUnsubscribe(subscription).catch(
            logFailure("agents.members.unsubscribe"),
          );
      })
      .catch((raw: unknown) => {
        if (active) setError(syncErrorMessage(raw, "keeper could not read this session's people."));
      });
    return () => {
      active = false;
      if (id !== null)
        void agentSessionMembersUnsubscribe(id).catch(logFailure("agents.members.unsubscribe"));
    };
  }, [database, sessionId]);
  return { sheet, error };
}

function roleText(member: AgentMemberVm): string {
  if (member.joining) return "Joining…";
  const role = member.role === "owner" ? "Owner" : member.role === "agent" ? "Agent" : "Person";
  return member.me ? `${role} · you` : role;
}

function Mark({ mark }: { mark: string }) {
  return (
    <span
      aria-hidden="true"
      className="flex size-8 shrink-0 items-center justify-center rounded-full bg-muted font-medium text-sm"
    >
      {mark}
    </span>
  );
}

export function SessionPeople({
  target,
  open: controlled,
  onOpenChange,
}: {
  target: AgentSessionRef;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const [ownOpen, setOwnOpen] = useState(false);
  const open = controlled ?? ownOpen;
  const setOpen = onOpenChange ?? setOwnOpen;
  const { phone } = useShellLayout();
  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetTrigger asChild>
        <Button variant="ghost" size="icon" className="size-11 shrink-0" aria-label="People">
          <Users aria-hidden="true" />
        </Button>
      </SheetTrigger>
      <SheetContent
        side={phone ? "bottom" : "right"}
        showCloseButton={false}
        className={
          phone
            ? "max-h-[85dvh] gap-0 p-0 pb-[var(--safe-bottom)]"
            : "gap-0 p-0 data-[side=right]:w-[min(26rem,100vw)] data-[side=right]:sm:max-w-none"
        }
      >
        {open && <PeopleBody target={target} onClose={() => setOpen(false)} />}
      </SheetContent>
    </Sheet>
  );
}

function PeopleBody({ target, onClose }: { target: AgentSessionRef; onClose: () => void }) {
  const { sheet, error: readError } = useSessionMembers(target);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const keep = useRef<HTMLButtonElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const body = useRef<HTMLDivElement>(null);
  // Where focus goes once a confirmation closes: back to the control that opened it on
  // Cancel, to Close once its subject is gone (that control goes with it).
  const refocus = useRef<string | null>(null);

  useEffect(() => {
    if (confirming !== null) {
      keep.current?.focus();
      return;
    }
    const key = refocus.current;
    refocus.current = null;
    if (key === null) return;
    const opener = Array.from(
      body.current?.querySelectorAll<HTMLElement>("[data-confirm]") ?? [],
    ).find((el) => el.dataset.confirm === key);
    (opener ?? close.current)?.focus();
  }, [confirming]);

  const cancel = () => {
    refocus.current = confirming;
    setConfirming(null);
  };

  const run = async (key: string, action: () => Promise<void>, fallback: string) => {
    setPending(key);
    setError(null);
    try {
      await action();
      if (confirming !== null) refocus.current = "";
      setConfirming(null);
    } catch (raw) {
      setError(syncErrorMessage(raw, fallback));
    } finally {
      setPending(null);
    }
  };

  const busy = pending !== null;
  // A fieldset's minimum width is its content's by default: `min-w-0` lets it follow the sheet,
  // the consequence wraps anywhere (a name may be one unbroken word), and the destructive control
  // shows a short label while its accessible name carries the whole subject.
  const confirmRow = (
    subject: string,
    label: string,
    consequence: string,
    onConfirm: () => void,
  ) => (
    <fieldset
      className="mt-2 flex min-w-0 max-w-full flex-wrap items-center gap-2"
      aria-label={subject}
    >
      <p className="w-full min-w-0 text-sm [overflow-wrap:anywhere]">{consequence}</p>
      <Button
        variant="destructive"
        className="min-h-11"
        aria-label={subject}
        disabled={busy}
        onClick={onConfirm}
      >
        {label}
      </Button>
      <Button ref={keep} variant="outline" className="min-h-11" disabled={busy} onClick={cancel}>
        Cancel
      </Button>
    </fieldset>
  );

  return (
    <div ref={body} className="flex min-h-0 flex-1 flex-col">
      <SheetHeader className="flex-row items-start gap-2 border-border border-b p-4">
        <div className="min-w-0 flex-1">
          <SheetTitle>People</SheetTitle>
          <SheetDescription>
            Who is in this session. Only people of{" "}
            <span className="break-all font-medium">{target.database}</span> can be added.
          </SheetDescription>
        </div>
        <Button ref={close} variant="ghost" className="min-h-11 shrink-0" onClick={onClose}>
          Close
        </Button>
      </SheetHeader>
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4">
        {!sheet && !readError && (
          <p role="status" className="text-muted-foreground text-sm">
            Reading people…
          </p>
        )}
        {(readError || error) && (
          <p role="alert" className="mb-3 text-destructive text-sm [overflow-wrap:anywhere]">
            {error ?? readError}
          </p>
        )}
        {sheet?.notice && (
          <p role="status" className="mb-3 text-destructive text-sm [overflow-wrap:anywhere]">
            {sheet.notice}
          </p>
        )}
        {sheet && (
          <>
            <ul aria-label="Members" className="flex flex-col gap-3">
              {sheet.members.map((member) => (
                <li key={member.actorId} className="min-w-0">
                  <div className="flex min-w-0 items-center gap-3">
                    <Mark mark={member.mark} />
                    <div className="min-w-0 flex-1">
                      <p className="truncate font-medium text-sm" title={member.name}>
                        {member.name}
                      </p>
                      <p className="text-muted-foreground text-xs">{roleText(member)}</p>
                    </div>
                    {member.canRemove && confirming !== member.actorId && (
                      <Button
                        variant="ghost"
                        className="min-h-11 shrink-0"
                        disabled={busy}
                        aria-label={`Remove ${member.name}`}
                        data-confirm={member.actorId}
                        onClick={() => setConfirming(member.actorId)}
                      >
                        Remove
                      </Button>
                    )}
                  </div>
                  {member.role === "person" && !member.joining && (
                    <div className="mt-1 pl-11">
                      {member.canSetReader ? (
                        <Button
                          role="switch"
                          aria-checked={member.readsTrace}
                          aria-label={`${member.name} can read thinking and tool results`}
                          variant={member.readsTrace ? "secondary" : "outline"}
                          className="h-auto min-h-11 max-w-full whitespace-normal text-left"
                          disabled={busy}
                          onClick={() =>
                            void run(
                              `reader:${member.actorId}`,
                              () =>
                                agentSessionMemberReaderSet(
                                  target,
                                  member.actorId,
                                  !member.readsTrace,
                                ),
                              "keeper could not change what they read.",
                            )
                          }
                        >
                          <span>Can read thinking and tool results</span>
                          <span aria-hidden="true" className="text-muted-foreground">
                            {member.readsTrace ? "On" : "Off"}
                          </span>
                        </Button>
                      ) : (
                        <p className="text-muted-foreground text-xs">
                          {member.readsTrace
                            ? "Can read thinking and tool results"
                            : "Sees answers, not thinking or tool results"}
                        </p>
                      )}
                    </div>
                  )}
                  {confirming === member.actorId &&
                    confirmRow(
                      `Remove ${member.name}`,
                      "Remove",
                      `${member.name} leaves this session; it disappears from their devices.`,
                      () =>
                        void run(
                          `remove:${member.actorId}`,
                          () => agentSessionMemberRemove(target, member.actorId),
                          "keeper could not remove them.",
                        ),
                    )}
                </li>
              ))}
            </ul>
            {sheet.owner && (
              <section aria-labelledby="people-add" className="mt-6">
                <h3 id="people-add" className="font-medium text-sm">
                  Add a person
                </h3>
                {sheet.addable.length === 0 ? (
                  <p className="mt-1 text-muted-foreground text-sm">
                    Everyone in {target.database} is already here.
                  </p>
                ) : (
                  <>
                    <p className="mt-1 text-muted-foreground text-xs">
                      They see the answers. Thinking and tool results stay hidden until you turn
                      them on.
                    </p>
                    <ul aria-label="People you can add" className="mt-2 flex flex-col gap-2">
                      {sheet.addable.map((person) => (
                        <li key={person.actorId} className="flex min-w-0 items-center gap-3">
                          <Mark mark={person.mark} />
                          <span className="min-w-0 flex-1 truncate text-sm" title={person.name}>
                            {person.name}
                          </span>
                          <Button
                            variant="outline"
                            className="min-h-11 shrink-0"
                            disabled={busy}
                            aria-label={`Add ${person.name}`}
                            onClick={() =>
                              void run(
                                `add:${person.actorId}`,
                                () => agentSessionMemberAdd(target, person.actorId),
                                "keeper could not add them.",
                              )
                            }
                          >
                            Add
                          </Button>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
              </section>
            )}
            {sheet.canLeave && (
              <div className="mt-6 border-border border-t pt-4">
                {confirming === "leave" ? (
                  confirmRow(
                    "Leave session",
                    "Leave session",
                    "This session and its messages leave your devices.",
                    () =>
                      void run(
                        "leave",
                        async () => {
                          await agentSessionLeave(target);
                          onClose();
                        },
                        "keeper could not leave this session.",
                      ),
                  )
                ) : (
                  <Button
                    variant="outline"
                    className="min-h-11"
                    disabled={busy}
                    data-confirm="leave"
                    onClick={() => setConfirming("leave")}
                  >
                    Leave session
                  </Button>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
