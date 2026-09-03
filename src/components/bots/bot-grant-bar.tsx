/**
 * The grant bar — what this bot can reach, in a person's words, with the two
 * controls that change it (Epic 61, Story 61.10, FR-386, FR-387, AD-158).
 *
 * # Where the affordance exists at all, and why
 *
 * Four conditions, and every failing one is an AD-27 *absence*, never a
 * disabled control:
 *
 * 1. **`capabilities.botTools` must be on.** A drive tool resolves a path
 *    inside a synced profile through `keeper-sync`, so on a machine with no
 *    folder sync — and on a phone, where `keeper-sync` is not linked at all
 *    (Epic 62) — there is nothing a grant could name. This is the honest split
 *    the epic states: the *pane* is gated on `bots` because a conversation
 *    needs no `git`, and the *grant* is gated on `botTools` because a file
 *    needs the drive. Absent, not disabled: the conversation goes on without
 *    it.
 * 2. **The provider must be Ollama.** Hermes executes its own tools on its own
 *    host under its own permission model, so the pane says that in one sentence
 *    and offers nothing.
 * 3. **The model must state that it takes tools.** `false` is a refusal keeper
 *    was told; `null` is one keeper could not read. Both mean no affordance —
 *    but they are different facts and get different sentences, and the unknown
 *    one is rendered as a warning rather than as a settled state.
 * 4. **A grant is only ever created here or in Settings** (NFR-48). No tool
 *    result, file content or model message can reach the writer.
 *
 * # The sentences are Rust's, letter for letter
 *
 * `keeper-core::bots::grant` holds the three refusals as `const`s
 * (`HERMES_RUNS_ITS_OWN_TOOLS`, `MODEL_HAS_NO_TOOLS`,
 * `TOOLS_CAPABILITY_UNKNOWN`), because the audit log and the pane must not word
 * one fact twice. Those three never cross IPC — nothing sends them, the pane
 * decides locally which applies — so they are restated here **once**, and
 * `bot-grant-bar.test.tsx` reads `grant.rs` and fails if a character drifts.
 * Every sentence that *does* cross IPC (a denial, an ask, a save refusal) is
 * rendered from the payload and is never retyped in TypeScript.
 *
 * # `unknown` offers the grant, with the warning
 *
 * A capability keeper could not read is `unknown`, never `false` (AD-27), and
 * `grant::grant_offer` — the decision home under AD-55/AD-56 — returns
 * `Offered { warning: TOOLS_CAPABILITY_UNKNOWN }` for it. The asymmetry is in
 * the user's favour: a grant on a model that turns out to have no tools costs
 * a tool call that is never made, while refusing on unknown would strand every
 * Ollama old enough to predate the `capabilities` array with no route to the
 * feature. `Some(false)` — a stated refusal — is the only case that hides the
 * affordance. Unknown vision is treated the same way in 61.12, so one flag
 * does not mean two opposite things inside one epic.
 */
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type {
  BotGrantVm,
  BotModelVm,
  BotProviderVm,
  GrantMode,
  GrantScope,
} from "@/lib/ipc/client";
import { botsGrantRevoke, botsGrantSave, botsGrantsList } from "@/lib/ipc/client";
import { syncErrorMessage, useSyncStore } from "@/lib/stores/sync";

// ---------------------------------------------------------------------------
// The three sentences that live only in the UI. Each is `keeper-core`'s const,
// verbatim; the parity test is what keeps them so.
// ---------------------------------------------------------------------------

/** `grant::HERMES_RUNS_ITS_OWN_TOOLS`. */
export const GRANT_HERMES_SENTENCE =
  "Hermes runs its tools on its own host, under its own permissions, so a keeper grant would change nothing here.";

/** `grant::MODEL_HAS_NO_TOOLS`. */
export const GRANT_NO_TOOLS_SENTENCE =
  "This model does not support tool calls, so there is nothing a grant would let it reach.";

/** `grant::TOOLS_CAPABILITY_UNKNOWN` — a warning beside an offered grant. */
export const GRANT_TOOLS_UNKNOWN_SENTENCE =
  "keeper could not read whether this model supports tool calls, so a grant here may reach nothing. Test the provider again to find out.";

/** What the bar says while this bot holds no grant. */
export const GRANT_NONE_HELD =
  "No grant. This bot cannot read or write anything in the folders you sync.";

/** What a grant that covers every bot of the endpoint adds. */
export const GRANT_PROVIDER_WIDE_NOTE = "This grant covers every bot of this endpoint.";

/** The verbs. */
export const GRANT_ADD_LABEL = "Grant a folder";
export const GRANT_CHANGE_LABEL = "Change";
export const GRANT_REVOKE_LABEL = "Revoke";
export const GRANT_SAVE_LABEL = "Save";
export const GRANT_CANCEL_LABEL = "Cancel";

/** The editor's field labels. */
export const GRANT_SCOPE_LABEL = "What it may reach";
export const GRANT_PROFILE_LABEL = "Folder";
export const GRANT_SUBPATH_LABEL = "Inside that folder";
export const GRANT_MODE_LABEL = "What it may do";

/** What the editor says about the folder list it could not read. */
export const GRANT_NO_PROFILES =
  "keeper has not read your synced folders yet, so the only scope it can offer is the whole drive.";

/** What a failed read, save or revoke says when Rust gave no sentence. */
export const GRANT_READ_FAILED = "keeper couldn't read this bot's grants.";
export const GRANT_SAVE_FAILED = "keeper couldn't save that grant.";
export const GRANT_REVOKE_FAILED = "keeper couldn't revoke that grant.";

/** The accessible name of the list of grants in force. */
export const GRANT_LIST_LABEL = "Grants for this bot";

/** The modes and scope kinds, spelled as they are stored (AD-C7's rule). */
const MODES: readonly GrantMode[] = ["none", "read", "write"];
const SCOPE_KINDS: readonly GrantScope["kind"][] = ["drive", "profile", "subtree"];

/** What this bot's pane may show about the drive — `GrantOffer`'s two arms,
 *  plus the case where there is nothing to say at all. */
export type BotGrantOffer =
  /** Nothing at all: no synced folder exists, or no bot is chosen. */
  | { kind: "absent" }
  /** No grant, and the settled reason why. */
  | { kind: "refused"; sentence: string }
  /** A grant is meaningful here. `warning` is set where keeper could not read
   *  whether the model takes tools — offered, and honest about not knowing. */
  | { kind: "offered"; warning: string | null };

/**
 * Decide what this bot's pane may offer — `grant::grant_offer`'s arms, plus
 * the `botTools` gate the shell owns.
 *
 * `model` is `null` while the model list is still being read, which lands in
 * the warned arm on purpose: keeper has not read the capability, and that is
 * precisely what that sentence says.
 */
export function botGrantOffer({
  botTools,
  provider,
  model,
}: {
  botTools: boolean;
  provider: BotProviderVm | null;
  model: BotModelVm | null;
}): BotGrantOffer {
  if (!botTools || provider === null) {
    return { kind: "absent" };
  }
  if (provider.kind === "hermes") {
    return { kind: "refused", sentence: GRANT_HERMES_SENTENCE };
  }
  // The one case that hides the affordance: the endpoint stated the refusal.
  if (model !== null && model.tools === false) {
    return { kind: "refused", sentence: GRANT_NO_TOOLS_SENTENCE };
  }
  const unreadable = model === null || model.tools === null;
  return { kind: "offered", warning: unreadable ? GRANT_TOOLS_UNKNOWN_SENTENCE : null };
}

/**
 * The grants in force for one bot — the provider's own, plus the ones that
 * cover every bot of it.
 *
 * A revoked row is dropped rather than dimmed: this bar answers "what can it
 * reach *now*", and the full history with its revocations is Settings' job.
 */
export function liveGrantsFor(
  grants: BotGrantVm[],
  providerId: string,
  botId: string | null,
): BotGrantVm[] {
  return grants.filter(
    (grant) =>
      grant.revokedMs === null &&
      grant.providerId === providerId &&
      (grant.botId === null || grant.botId === botId),
  );
}

/**
 * What one grant permits, as a sentence.
 *
 * The `write` arm splits on scope because FR-387 does: a `write` grant on the
 * whole drive or a whole profile is a standing permission to *ask*, and only a
 * subtree grant lets a write through unasked. A bar that said "can write" for
 * both would be describing a permission the model does not have.
 */
export function grantSentence(grant: BotGrantVm): string {
  if (grant.mode === "none") {
    return `This bot is refused ${grant.scopeLabel}, whatever a wider grant says.`;
  }
  if (grant.mode === "read") {
    return `This bot can read ${grant.scopeLabel}, and cannot write there.`;
  }
  if (grant.scope.kind === "subtree") {
    return `This bot can read and write ${grant.scopeLabel}.`;
  }
  return `This bot can read ${grant.scopeLabel}, and keeper asks before every write to a scope this wide.`;
}

/** The grant bar. Absent, or one line per grant with the two controls. */
export function BotGrantBar({
  botTools,
  provider,
  botId,
  model,
}: {
  botTools: boolean;
  provider: BotProviderVm | null;
  botId?: string | null;
  model: BotModelVm | null;
}) {
  const offer = botGrantOffer({ botTools, provider, model });
  const providerId = provider?.id ?? null;
  const offered = offer.kind === "offered";
  const [grants, setGrants] = useState<BotGrantVm[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Which editor is open: a grant being changed, a new one, or none. */
  const [editing, setEditing] = useState<BotGrantVm | "new" | null>(null);

  const refresh = useCallback(() => {
    if (!offered || providerId === null) {
      return;
    }
    void botsGrantsList()
      .then((listing) => {
        setGrants(listing.grants);
        setError(null);
      })
      .catch((raw: unknown) => setError(syncErrorMessage(raw, GRANT_READ_FAILED)));
  }, [offered, providerId]);

  useEffect(refresh, [refresh]);

  if (offer.kind === "absent") {
    return null;
  }
  if (offer.kind === "refused") {
    return (
      <div className="flex shrink-0 flex-col gap-1 border-border border-b px-6 py-2">
        <p className="text-muted-foreground text-xs">{offer.sentence}</p>
      </div>
    );
  }

  const held = providerId === null ? [] : liveGrantsFor(grants ?? [], providerId, botId ?? null);

  const revoke = (grantId: string) => {
    void botsGrantRevoke(grantId)
      .catch((raw: unknown) => setError(syncErrorMessage(raw, GRANT_REVOKE_FAILED)))
      // A re-read rather than a local splice: the store is the truth, and a bar
      // that removed the row itself would keep saying so if the write failed.
      .finally(refresh);
  };

  return (
    <div className="flex shrink-0 flex-col gap-1 border-border border-b px-6 py-2">
      {offer.warning !== null && (
        // `status`, not `alert`: an unreadable capability is a state of the
        // world worth reading, not an error that just happened. The grant is
        // offered anyway — unknown is not no.
        <p role="status" className="text-xs">
          {offer.warning}
        </p>
      )}
      {held.length === 0 ? (
        <div className="flex items-center gap-2">
          <p className="min-w-0 flex-1 text-muted-foreground text-xs">{GRANT_NONE_HELD}</p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setEditing(editing === "new" ? null : "new")}
          >
            {GRANT_ADD_LABEL}
          </Button>
        </div>
      ) : (
        <ul aria-label={GRANT_LIST_LABEL} className="flex flex-col gap-1">
          {held.map((grant) => (
            <li key={grant.id} className="flex flex-col gap-1">
              <div className="flex items-center gap-2">
                <span className="min-w-0 flex-1 text-xs">
                  {grantSentence(grant)}
                  {grant.botId === null ? ` ${GRANT_PROVIDER_WIDE_NOTE}` : ""}
                </span>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    setEditing(editing !== "new" && editing?.id === grant.id ? null : grant)
                  }
                >
                  {GRANT_CHANGE_LABEL}
                </Button>
                <Button type="button" variant="outline" size="sm" onClick={() => revoke(grant.id)}>
                  {GRANT_REVOKE_LABEL}
                </Button>
              </div>
              {editing !== "new" && editing?.id === grant.id && providerId !== null && (
                <BotGrantEditor
                  grant={grant}
                  providerId={providerId}
                  botId={botId ?? null}
                  onDone={() => {
                    setEditing(null);
                    refresh();
                  }}
                  onCancel={() => setEditing(null)}
                />
              )}
            </li>
          ))}
        </ul>
      )}

      {editing === "new" && providerId !== null && (
        <BotGrantEditor
          providerId={providerId}
          botId={botId ?? null}
          onDone={() => {
            setEditing(null);
            refresh();
          }}
          onCancel={() => setEditing(null)}
        />
      )}

      {error !== null && (
        <p role="alert" className="text-destructive text-xs">
          {error}
        </p>
      )}
    </div>
  );
}

/**
 * The grant editor — a new grant when `grant` is absent, a rewrite of that row
 * when it is not (AD-C7, one component in two modes).
 *
 * The subtree is sent as typed and Rust's own path grammar refuses it, with the
 * sentence naming what was wrong. A second grammar here would be a second
 * answer to the same question, which is how `notes-old` ends up inside `notes`.
 */
export function BotGrantEditor({
  grant,
  providerId,
  botId,
  onDone,
  onCancel,
}: {
  grant?: BotGrantVm;
  providerId: string;
  botId: string | null;
  onDone: () => void;
  onCancel: () => void;
}) {
  const profiles = useSyncStore((state) => state.profiles);
  const profileList = profiles ?? [];
  const [kind, setKind] = useState<GrantScope["kind"]>(grant?.scope.kind ?? "subtree");
  const [profileId, setProfileId] = useState(
    grant?.scope.kind === "profile" || grant?.scope.kind === "subtree"
      ? grant.scope.profileId
      : (profileList[0]?.id ?? ""),
  );
  const [subpath, setSubpath] = useState(
    grant?.scope.kind === "subtree" ? grant.scope.subpath : "",
  );
  const [mode, setMode] = useState<GrantMode>(grant?.mode ?? "read");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = () => {
    setSaving(true);
    setError(null);
    const scope: GrantScope =
      kind === "drive"
        ? { kind: "drive" }
        : kind === "profile"
          ? { kind: "profile", profileId }
          : { kind: "subtree", profileId, subpath };
    void botsGrantSave({ id: grant?.id ?? null, providerId, botId, scope, mode })
      .then(onDone)
      // Rust's sentence verbatim: the path grammar is its decision and its
      // wording names exactly what was wrong.
      .catch((raw: unknown) => setError(syncErrorMessage(raw, GRANT_SAVE_FAILED)))
      .finally(() => setSaving(false));
  };

  return (
    <div className="flex flex-col gap-2 rounded-md border border-border p-3">
      <div className="flex flex-wrap items-center gap-1">
        <span className="text-muted-foreground text-xs">{GRANT_SCOPE_LABEL}</span>
        {SCOPE_KINDS.map((option) => (
          <Button
            key={option}
            type="button"
            size="sm"
            variant={kind === option ? "secondary" : "ghost"}
            aria-pressed={kind === option}
            onClick={() => setKind(option)}
          >
            {option}
          </Button>
        ))}
      </div>
      {kind !== "drive" &&
        (profileList.length === 0 ? (
          <p className="text-muted-foreground text-xs">{GRANT_NO_PROFILES}</p>
        ) : (
          <div className="flex flex-wrap items-center gap-1">
            <span className="text-muted-foreground text-xs">{GRANT_PROFILE_LABEL}</span>
            {profileList.map((profile) => (
              <Button
                key={profile.id}
                type="button"
                size="sm"
                variant={profileId === profile.id ? "secondary" : "ghost"}
                aria-pressed={profileId === profile.id}
                onClick={() => setProfileId(profile.id)}
              >
                {profile.name}
              </Button>
            ))}
          </div>
        ))}
      {kind === "subtree" && (
        <>
          <Label htmlFor="bot-grant-subpath">{GRANT_SUBPATH_LABEL}</Label>
          <Input
            id="bot-grant-subpath"
            value={subpath}
            onChange={(event) => setSubpath(event.target.value)}
          />
        </>
      )}
      <div className="flex flex-wrap items-center gap-1">
        <span className="text-muted-foreground text-xs">{GRANT_MODE_LABEL}</span>
        {MODES.map((option) => (
          <Button
            key={option}
            type="button"
            size="sm"
            variant={mode === option ? "secondary" : "ghost"}
            aria-pressed={mode === option}
            onClick={() => setMode(option)}
          >
            {option}
          </Button>
        ))}
      </div>
      {error !== null && (
        <p role="alert" className="text-destructive text-xs">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button type="button" size="sm" disabled={saving} onClick={save}>
          {GRANT_SAVE_LABEL}
        </Button>
        <Button type="button" variant="outline" size="sm" onClick={onCancel}>
          {GRANT_CANCEL_LABEL}
        </Button>
      </div>
    </div>
  );
}
