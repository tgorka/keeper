/**
 * The approval sheet — what keeper asks when a grant says a person must decide
 * (Epic 61, Story 61.10, FR-387, AD-158).
 *
 * `keeper-core::bots::grant::decide` answers `Ask` for exactly one shape of
 * call: a write that lands inside a `write`-mode scope wider than a subtree.
 * The shell's tool host turns that into an approval port
 * (`bots_tools.rs:74-96`, "a host built with no approver declines every ask"),
 * and this dialog is what fills it in.
 *
 * # The three answers, and why the third one is a grant and not a memory
 *
 * - **Deny** is the default and the Escape key, because a dialog that consented
 *   by being dismissed is a dialog that consented for you. The port's own
 *   comment already takes this position for the missing-UI case; this is the
 *   same rule where the UI exists.
 * - **Just this once** approves this call and nothing else. keeper remembers
 *   nothing.
 * - **Always for this folder** writes a durable subtree grant through
 *   `bots_grant_save`. It is a *grant edit*, not a remembered click, and the
 *   dialog says so in its own copy: the answer to "what can this bot change?"
 *   stays one list in Settings, never a history of dialogs nobody can re-read.
 *
 * # Where "always" is absent
 *
 * A path directly under a profile root has no folder inside the profile to
 * grant — its parent *is* the profile, and a `write` grant on a whole profile
 * asks every time by FR-387, so an "always" that saved one would not stop the
 * asking. The control is therefore absent with a sentence saying why (AD-27),
 * rather than present and quietly useless.
 *
 * # The sentence at the top is Rust's
 *
 * `reason` is carried verbatim from the verdict (`grant::ASK_WRITE_WIDE_SCOPE`
 * today), so the log and this dialog quote the same words. Nothing here
 * rewrites it.
 */
import { useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import type { Effect } from "@/lib/ipc/client";
import { botsGrantSave } from "@/lib/ipc/client";
import { botsStore, useBotsStore } from "@/lib/stores/bots";
import { syncErrorMessage } from "@/lib/stores/sync";

/**
 * One tool call waiting on a person (Story 61.10).
 *
 * Every field is what the grant check already knew before the effect: the same
 * facts the audit row is born with, so the dialog and the log describe one
 * call in one vocabulary.
 */
export interface BotApprovalRequest {
  /** The pending call's handle, so an answer names which ask it answers. */
  requestId: string;
  /** The provider whose bot asked. */
  providerId: string;
  /** The bot that asked, or `null` where none was named. */
  botId: string | null;
  /** The tool's name, as the model called it. */
  tool: string;
  /** The path a person reads — `profile/sub/path`. */
  path: string;
  /** The profile the path is in. */
  profileId: string;
  /** The profile-relative path. */
  subpath: string;
  /** Read or write. */
  effect: Effect;
  /** The sentence Rust produced, rendered verbatim. */
  reason: string;
}

/** What the person answered. */
export type BotApprovalAnswer = "once" | "always" | "deny";

/** The heading, which names the act rather than asking "are you sure". */
export const APPROVAL_TITLE = "This bot is asking to write";
export const APPROVAL_TITLE_READ = "This bot is asking to read";

/** The three answers. */
export const APPROVAL_DENY_LABEL = "Deny";
export const APPROVAL_ONCE_LABEL = "Just this once";
export const APPROVAL_ALWAYS_LABEL = "Always for this folder";

/** What "always" does, said plainly, because it is a durable permission. */
export const APPROVAL_ALWAYS_NOTE =
  "Always for this folder saves a grant. It is listed in Settings with your other grants and can be revoked there, so what this bot may change stays one list and never a hidden history of clicks.";

/** Why "always" is absent for a file at the top of a profile. */
export const APPROVAL_ALWAYS_ABSENT =
  "This path sits at the top of the folder, so there is no folder inside it to grant. Approve this one call, or grant the folder itself in Settings.";

/** What a failed grant write says when Rust gave no sentence. */
export const APPROVAL_SAVE_FAILED = "keeper couldn't save that grant, so nothing was approved.";

/**
 * The subtree "always" would grant: the folder the path sits in, or `null`
 * when that folder is the profile root.
 *
 * Segment arithmetic on the profile-relative path only — it never joins, never
 * resolves and never touches disk, because turning a path into a location is
 * `keeper-sync`'s containment rule and this is not it (AD-65).
 */
export function approvalSubtree(subpath: string): string | null {
  const cut = subpath.lastIndexOf("/");
  if (cut <= 0) {
    return null;
  }
  return subpath.slice(0, cut);
}

/**
 * The approval sheet. Rendered whenever `request` is set; answers exactly once
 * per request.
 */
export function BotApprovalDialog({
  request,
  onAnswer,
}: {
  request: BotApprovalRequest | null;
  onAnswer: (requestId: string, answer: BotApprovalAnswer) => void;
}) {
  // Both pieces of transient state are **keyed by the ask they belong to**,
  // rather than reset by an effect when the ask changes: a failure from the
  // previous call must never be read as this one's, and a keyed value cannot
  // be shown for one frame before an effect clears it.
  const [failure, setFailure] = useState<{ requestId: string; message: string } | null>(null);
  const [savingId, setSavingId] = useState<string | null>(null);

  if (request === null) {
    return null;
  }

  const subtree = approvalSubtree(request.subpath);
  const error = failure?.requestId === request.requestId ? failure.message : null;
  const saving = savingId === request.requestId;

  const always = () => {
    if (subtree === null) {
      return;
    }
    const { requestId } = request;
    setSavingId(requestId);
    setFailure(null);
    void botsGrantSave({
      id: null,
      providerId: request.providerId,
      botId: request.botId,
      scope: { kind: "subtree", profileId: request.profileId, subpath: subtree },
      // The grant is exactly as wide as the effect it is approving.
      mode: request.effect === "write" ? "write" : "read",
    })
      .then(() => onAnswer(requestId, "always"))
      // The ask stays open on a failed write: answering "always" when no grant
      // was stored would approve a call under a permission that does not exist.
      .catch((raw: unknown) =>
        setFailure({ requestId, message: syncErrorMessage(raw, APPROVAL_SAVE_FAILED) }),
      )
      .finally(() => setSavingId((held) => (held === requestId ? null : held)));
  };

  return (
    <AlertDialog
      open
      onOpenChange={(next) => {
        // Escape, and every other dismissal: a sheet that is closed without an
        // answer has not been consented to.
        if (!next) {
          onAnswer(request.requestId, "deny");
        }
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {request.effect === "write" ? APPROVAL_TITLE : APPROVAL_TITLE_READ}
          </AlertDialogTitle>
          {/* Rust's sentence, verbatim — the audit row stores this same text. */}
          <AlertDialogDescription>{request.reason}</AlertDialogDescription>
        </AlertDialogHeader>
        <p className="text-sm">
          {request.tool} — {request.path}
        </p>
        <p className="text-muted-foreground text-xs">
          {subtree === null ? APPROVAL_ALWAYS_ABSENT : APPROVAL_ALWAYS_NOTE}
        </p>
        {error !== null && (
          <p role="alert" className="text-destructive text-xs">
            {error}
          </p>
        )}
        <AlertDialogFooter>
          {/* Cancel is Deny, and radix focuses it when the sheet opens: the
              safe answer is the one already under the return key. */}
          <AlertDialogCancel onClick={() => onAnswer(request.requestId, "deny")}>
            {APPROVAL_DENY_LABEL}
          </AlertDialogCancel>
          <AlertDialogAction onClick={() => onAnswer(request.requestId, "once")}>
            {APPROVAL_ONCE_LABEL}
          </AlertDialogAction>
          {subtree !== null && (
            <AlertDialogAction
              disabled={saving}
              // Not `AlertDialogAction`'s default close-on-click: this one
              // writes a grant first and stays open if that write failed.
              onClick={(event) => {
                event.preventDefault();
                always();
              }}
            >
              {`${APPROVAL_ALWAYS_LABEL} — ${subtree}`}
            </AlertDialogAction>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/**
 * The pane's mount for the sheet: reads the pending ask from the store,
 * answers it, and clears it (Story 61.10).
 *
 * A wrapper rather than props threaded through the pane, for the reason the
 * store's own doc gives: the pane unmounts on every surface switch, and an ask
 * held in component state would be an approval that vanished because somebody
 * glanced at Files — which the tool call underneath would read as a decline.
 * The store is where a live ask survives that, and this component is the two
 * lines the pane needs.
 */
export function BotApprovalHost() {
  const pending = useBotsStore((state) => state.pendingApproval);
  return (
    <BotApprovalDialog
      request={pending?.request ?? null}
      onAnswer={(_requestId, answer) => {
        // Answer first, clear second: the continuation is what the blocking
        // tool call is waiting on, and dropping it would hang the turn.
        pending?.answer(answer);
        botsStore.getState().clearApproval();
      }}
    />
  );
}
