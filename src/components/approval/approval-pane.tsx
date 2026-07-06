/**
 * The Approval Pane primary view (Story 7.3).
 *
 * The airlock's cross-account review surface: every pending draft (Story 7.1/7.2)
 * across all accounts, grouped by account then chat. Each row renders a silent
 * "You" proposer column, the chat name, a bridged-Network badge, a per-account hue
 * edge, a body preview, and a relative age. A draft whose room/account cannot be
 * resolved is still listed (name = room id, no network) — the pane never hides held
 * text.
 *
 * Bodies are authoritative in Rust: the row list comes from `listPendingDrafts`
 * (re-queried whenever the presence set changes), never from a JS store. Per-row:
 * `Enter` opens an inline editor (save → `saveDraft` + `mirrorDraft`; trimmed-empty →
 * discard), `⌘Enter` approves through the single send gate (`approveDraft`, clearing
 * only on success so a failed send never loses text), `⌘⌫` discards behind a 5 s
 * undo toast. No bulk / select-all / approve-all affordance — approving is strictly
 * per-draft and user-initiated.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { Avatar, AvatarBadge, AvatarFallback } from "@/components/ui/avatar";
import { ScrollArea } from "@/components/ui/scroll-area";
import { accountHueVar } from "@/lib/account-hue";
import { formatDraftAge } from "@/lib/format-time";
import {
  type ApprovalDraftVm,
  approveDraft,
  clearDraft,
  clearDraftMirror,
  listPendingDrafts,
  mirrorDraft,
  saveDraft,
} from "@/lib/ipc/client";
import { draftsStore, usePendingDraftKeys } from "@/lib/stores/drafts";
import { cn } from "@/lib/utils";

/** Exact empty-state copy (Story 7.3 acceptance) — kept verbatim. */
const EMPTY_STATE_TEXT =
  "Nothing waiting. Drafts you write stay here until you approve them — nothing sends without you.";

/** A stable composite key for one draft row (`` `${accountId} ${roomId}` ``). */
function rowKey(accountId: string, roomId: string): string {
  return `${accountId} ${roomId}`;
}

/** Derive up-to-two-letter initials for the room avatar fallback. */
function initials(displayName: string): string {
  const words = displayName.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) {
    return "#";
  }
  if (words.length === 1) {
    return words[0].slice(0, 2).toUpperCase();
  }
  return (words[0][0] + words[words.length - 1][0]).toUpperCase();
}

/** One account's drafts, grouped for the section headers. */
interface AccountGroup {
  accountId: string;
  accountUserId: string;
  hueIndex: number;
  drafts: ApprovalDraftVm[];
}

/** Partition the flat draft list into per-account groups (account then chat). */
function groupByAccount(drafts: ApprovalDraftVm[]): AccountGroup[] {
  const groups = new Map<string, AccountGroup>();
  for (const draft of drafts) {
    let group = groups.get(draft.accountId);
    if (group === undefined) {
      group = {
        accountId: draft.accountId,
        accountUserId: draft.accountUserId,
        hueIndex: draft.hueIndex,
        drafts: [],
      };
      groups.set(draft.accountId, group);
    }
    group.drafts.push(draft);
  }
  return Array.from(groups.values());
}

export function ApprovalPane() {
  const [rows, setRows] = useState<ApprovalDraftVm[]>([]);
  // A stable serialization of the presence-set CONTENTS (not just its size): a
  // simultaneous add+remove that nets a size change of zero still changes this
  // string, so the re-query below never goes stale (Story 7.3, P2).
  const pendingKeys = usePendingDraftKeys();
  // Which row (composite key) has its inline editor open, or null.
  const [editingKey, setEditingKey] = useState<string | null>(null);
  // Distinguishes a genuinely empty pane from a failed query: on a query error we
  // keep the last-known rows and flag this so the empty branch shows a retry
  // affordance instead of the "nothing waiting" copy (Story 7.3, P6).
  const [queryFailed, setQueryFailed] = useState(false);
  // Approvals in flight, keyed by composite row key: a rapid double ⌘Enter must not
  // dispatch the same draft twice (Story 7.3, P4).
  const inFlight = useRef<Set<string>>(new Set());

  // Re-query the authoritative rows on mount and whenever the presence-set contents
  // change (any draft add/remove/replace anywhere). Depending on the serialized keys
  // (not the size) catches a net-zero add+remove.
  const requery = useCallback(() => {
    listPendingDrafts()
      .then((next) => {
        setRows(next);
        setQueryFailed(false);
      })
      .catch(() => {
        // A query failure leaves the last-known rows in place rather than blanking
        // the airlock, and flags the failure so the empty branch offers a retry.
        setQueryFailed(true);
      });
  }, []);

  useEffect(() => {
    // Reference `pendingKeys` so this effect legitimately re-runs on every presence
    // change (a change means a draft appeared/vanished/moved → re-query Rust).
    void pendingKeys;
    requery();
  }, [pendingKeys, requery]);

  const groups = useMemo(() => groupByAccount(rows), [rows]);

  // Optimistically drop a row from the local list (approve success / discard) so it
  // can't be re-triggered and doesn't flicker back if a fire-and-forget clear races
  // the re-query. The P2 re-query re-adds it if the draft is in fact still pending.
  const removeRow = useCallback((accountId: string, roomId: string) => {
    setRows((prev) => prev.filter((r) => !(r.accountId === accountId && r.roomId === roomId)));
  }, []);

  // Approve a row: dispatch through the single gate; ONLY on success clear the
  // draft locally + mirror + presence marker (a failed send retains everything). An
  // in-flight guard keyed by the composite row key drops a rapid double ⌘Enter so
  // the same draft never dispatches twice (P4). On success the row is optimistically
  // removed so it can't be re-triggered and doesn't flicker back if the clear races
  // the re-query.
  const onApprove = useCallback(
    async (draft: ApprovalDraftVm) => {
      const key = rowKey(draft.accountId, draft.roomId);
      if (inFlight.current.has(key)) {
        return;
      }
      inFlight.current.add(key);
      try {
        try {
          await approveDraft(draft.accountId, draft.roomId, draft.body);
        } catch {
          toast.error("Couldn't send this draft. It's still here — try again.");
          return;
        }
        clearDraft(draft.accountId, draft.roomId).catch(() => {});
        clearDraftMirror(draft.accountId, draft.roomId).catch(() => {});
        draftsStore.getState().mark(draft.accountId, draft.roomId, false);
        removeRow(draft.accountId, draft.roomId);
      } finally {
        inFlight.current.delete(key);
      }
    },
    [removeRow],
  );

  // Discard a row: remove local + mirror + marker immediately (and drop the row
  // optimistically), behind a 5 s undo toast that fully restores the draft (local +
  // marker + mirror). The undo awaits `saveDraft` before re-marking presence so the
  // restore can't race the re-query into a transient absent state.
  const onDiscard = useCallback(
    (draft: ApprovalDraftVm) => {
      clearDraft(draft.accountId, draft.roomId).catch(() => {});
      clearDraftMirror(draft.accountId, draft.roomId).catch(() => {});
      draftsStore.getState().mark(draft.accountId, draft.roomId, false);
      removeRow(draft.accountId, draft.roomId);
      toast("Draft discarded", {
        duration: 5000,
        action: {
          label: "Undo",
          onClick: () => {
            void (async () => {
              await saveDraft(draft.accountId, draft.roomId, draft.body).catch(() => {});
              draftsStore.getState().mark(draft.accountId, draft.roomId, true);
              mirrorDraft(draft.accountId, draft.roomId, draft.body).catch(() => {});
            })();
          },
        },
      });
    },
    [removeRow],
  );

  // Save an inline edit: a trimmed-empty body discards (identical to ⌘⌫), otherwise
  // persist via the normal draft save + mirror path.
  const onSaveEdit = useCallback(
    (draft: ApprovalDraftVm, nextBody: string) => {
      setEditingKey(null);
      if (nextBody.trim().length === 0) {
        onDiscard(draft);
        return;
      }
      if (nextBody === draft.body) {
        return;
      }
      saveDraft(draft.accountId, draft.roomId, nextBody).catch(() => {});
      mirrorDraft(draft.accountId, draft.roomId, nextBody).catch(() => {});
      // Reflect the new body locally so the preview updates before the re-query.
      setRows((prev) =>
        prev.map((r) =>
          r.accountId === draft.accountId && r.roomId === draft.roomId
            ? { ...r, body: nextBody }
            : r,
        ),
      );
    },
    [onDiscard],
  );

  const isEmpty = rows.length === 0;

  return (
    <section
      aria-label="Approvals"
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background"
    >
      <header className="shrink-0 border-border border-b px-6 py-4">
        <h1 className="font-heading font-medium text-lg">Approvals</h1>
        <p className="text-muted-foreground text-sm">
          Review and approve drafts across every account. Nothing sends without you.
        </p>
      </header>

      <ScrollArea className="min-h-0 flex-1">
        {isEmpty && queryFailed ? (
          <div className="flex h-full flex-col items-center justify-center gap-3 p-10">
            <p className="max-w-md text-center text-muted-foreground text-sm">
              Couldn't load pending drafts.
            </p>
            <button
              type="button"
              onClick={requery}
              className="rounded-md border border-border px-3 py-1.5 text-sm outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
            >
              Retry
            </button>
          </div>
        ) : isEmpty ? (
          <div className="flex h-full items-center justify-center p-10">
            <p className="max-w-md text-center text-muted-foreground text-sm">{EMPTY_STATE_TEXT}</p>
          </div>
        ) : (
          <ul className="flex flex-col gap-6 p-6">
            {groups.map((group, groupIndex) => (
              <li key={group.accountId}>
                <h2
                  className="mb-2 flex items-center gap-2 font-medium text-muted-foreground text-xs uppercase tracking-wide"
                  style={{ color: accountHueVar(group.hueIndex) }}
                >
                  <span
                    aria-hidden="true"
                    className="inline-block size-2 rounded-full"
                    style={{ backgroundColor: accountHueVar(group.hueIndex) }}
                  />
                  {group.accountUserId}
                </h2>
                <ul className="flex flex-col overflow-hidden rounded-md border border-border">
                  {group.drafts.map((draft, index) => (
                    <ApprovalRow
                      key={rowKey(draft.accountId, draft.roomId)}
                      draft={draft}
                      editing={editingKey === rowKey(draft.accountId, draft.roomId)}
                      // Roving tabindex: only the very first row in the whole pane
                      // (first row of the first group) is tab-reachable — a single
                      // tab stop for the list, not one per account group.
                      tabbable={groupIndex === 0 && index === 0}
                      onEnterEdit={() => setEditingKey(rowKey(draft.accountId, draft.roomId))}
                      onCancelEdit={() => setEditingKey(null)}
                      onSaveEdit={(next) => onSaveEdit(draft, next)}
                      onApprove={() => {
                        void onApprove(draft);
                      }}
                      onDiscard={() => onDiscard(draft)}
                    />
                  ))}
                </ul>
              </li>
            ))}
          </ul>
        )}
      </ScrollArea>
    </section>
  );
}

interface ApprovalRowProps {
  draft: ApprovalDraftVm;
  editing: boolean;
  tabbable: boolean;
  onEnterEdit: () => void;
  onCancelEdit: () => void;
  onSaveEdit: (nextBody: string) => void;
  onApprove: () => void;
  onDiscard: () => void;
}

function ApprovalRow({
  draft,
  editing,
  tabbable,
  onEnterEdit,
  onCancelEdit,
  onSaveEdit,
  onApprove,
  onDiscard,
}: ApprovalRowProps) {
  const [editValue, setEditValue] = useState(draft.body);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  // True once an explicit Enter/Escape has committed the editor, so the ensuing
  // unmount `onBlur` doesn't fire a second `onSaveEdit` (double-discard on a
  // trimmed-empty body would toast/clear twice). Reset each time the editor opens.
  const committedRef = useRef(false);
  // Holds the latest row body without making the seed effect re-run on body
  // changes — the editor is seeded strictly on the not-editing→editing transition
  // (see below), never re-seeded mid-edit.
  const bodyRef = useRef(draft.body);
  bodyRef.current = draft.body;
  // Tracks the previous `editing` value so the seed fires only on the
  // not-editing→editing transition, not on every re-render while editing.
  const wasEditingRef = useRef(false);

  // Seed the editor with the current body ONLY when it opens (the
  // not-editing→editing transition), and focus it. Deliberately NOT keyed on
  // `draft.body`: an incoming Story 7.2 cross-device mirror edit landing mid-edit
  // must never re-seed the textarea and clobber the user's in-progress text.
  useEffect(() => {
    if (editing && !wasEditingRef.current) {
      committedRef.current = false;
      setEditValue(bodyRef.current);
      textareaRef.current?.focus();
    }
    wasEditingRef.current = editing;
  }, [editing]);

  const onRowKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      // The row shortcuts must only act when the row div itself is focused. An
      // Enter / ⌘Enter typed inside the inline editor <textarea> bubbles here; act
      // on it and plain Enter would re-open the editor after save, ⌘Enter would fire
      // an unintended approve. Ignore anything that originated from a descendant.
      if (event.target !== event.currentTarget) {
        return;
      }
      const mod = event.metaKey || event.ctrlKey;
      if (mod && event.key === "Enter") {
        event.preventDefault();
        onApprove();
        return;
      }
      // ⌘⌫ discards (Backspace / Delete both fire "Backspace"/"Delete").
      if (mod && (event.key === "Backspace" || event.key === "Delete")) {
        event.preventDefault();
        onDiscard();
        return;
      }
      if (event.key === "Enter") {
        event.preventDefault();
        onEnterEdit();
      }
    },
    [onApprove, onDiscard, onEnterEdit],
  );

  const onEditorKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key === "Escape") {
        event.preventDefault();
        // Mark committed so the ensuing unmount blur is a no-op (no stray save).
        committedRef.current = true;
        onCancelEdit();
        return;
      }
      // Enter saves; Shift+Enter inserts a newline (multiline drafts).
      if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault();
        // Mark committed before saving so the unmount blur doesn't save again.
        committedRef.current = true;
        onSaveEdit(editValue);
      }
    },
    [editValue, onCancelEdit, onSaveEdit],
  );

  // Blur commits the edit — unless an explicit Enter/Escape already committed it,
  // in which case this is the unmount blur and must be a no-op (P3).
  const onEditorBlur = useCallback(() => {
    if (committedRef.current) {
      committedRef.current = false;
      return;
    }
    onSaveEdit(editValue);
  }, [editValue, onSaveEdit]);

  return (
    <li className="border-border border-b last:border-b-0">
      {/* biome-ignore lint/a11y/useSemanticElements: a focusable row hosting an
          inline textarea and multiple actions is a composite widget, not a button;
          Enter opens the editor, ⌘Enter approves, ⌘⌫ discards. */}
      <div
        role="button"
        tabIndex={tabbable ? 0 : -1}
        data-slot="approval-row"
        aria-label={`Draft in ${draft.displayName} on ${draft.accountUserId}. Enter to edit, Cmd+Enter to approve, Cmd+Backspace to discard.`}
        onKeyDown={onRowKeyDown}
        className={cn(
          "relative flex w-full items-start gap-3 py-3 pr-4 pl-4 text-left",
          "outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
          "hover:bg-accent",
        )}
      >
        {/* 3 px per-account hue edge bar (UX-DR3). Decorative. */}
        <span
          aria-hidden="true"
          data-testid="account-hue-bar"
          className="absolute inset-y-0 left-0 w-[3px]"
          style={{ backgroundColor: accountHueVar(draft.hueIndex) }}
        />
        {/* Silent "You" proposer column (Story 7.3): the airlock reserves a leading
            proposer slot; every keeper draft is proposed by the user. */}
        <span data-slot="proposer" className="w-8 shrink-0 pt-1 text-muted-foreground text-xs">
          You
        </span>
        <Avatar size="lg">
          <AvatarFallback>{initials(draft.displayName)}</AvatarFallback>
          {draft.network && (
            <AvatarBadge
              className="size-4! bg-secondary text-[9px] text-secondary-foreground"
              aria-label={`${draft.network} network`}
              title={draft.network}
            >
              {[...draft.network][0]?.toUpperCase() ?? ""}
            </AvatarBadge>
          )}
        </Avatar>
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex items-baseline justify-between gap-2">
            <span className="truncate font-medium text-sm">{draft.displayName}</span>
            <span className="shrink-0 text-muted-foreground text-xs">
              {formatDraftAge(draft.updatedTs)}
            </span>
          </div>
          {editing ? (
            <textarea
              ref={textareaRef}
              data-slot="approval-editor"
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              onKeyDown={onEditorKeyDown}
              onBlur={onEditorBlur}
              aria-label={`Edit draft for ${draft.displayName}`}
              className="flex min-h-16 w-full rounded-md border border-input bg-transparent px-2.5 py-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
            />
          ) : (
            <p className="line-clamp-3 whitespace-pre-wrap text-muted-foreground text-sm">
              {draft.body}
            </p>
          )}
        </div>
      </div>
    </li>
  );
}
