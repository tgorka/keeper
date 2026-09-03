/**
 * The Bots surface's empty states (Epic 61, Story 61.4).
 *
 * Four states, and telling them apart is the whole job — the
 * `RecordingsEmptyState` rule, which exists because "nothing recorded yet" and
 * "nothing matches this filter" render the same empty list and mean opposite
 * things.
 *
 * Here the four are: no provider is configured at all; a provider exists but no
 * bot is pinned; a bot is chosen and no conversation is open; and a bot is
 * chosen whose provider has lost its stored credential. The last one is the one
 * this app owes most: a row that has lost its secret says so **before** a send
 * rather than at one (FR-370), and the sentence names the provider so the
 * person knows which of several to fix.
 *
 * Each state carries exactly one action, so the surface never dead-ends —
 * modelled on {@link NotesEmptyState} and {@link RecordingsEmptyState}.
 *
 * On the phone tier every state also carries
 * {@link BOTS_NO_DRIVE_HERE_SENTENCE} (Epic 62, Story 62.3): the grant bar and
 * the tool affordances are absent there rather than disabled (AD-27), and the
 * empty state is where a person looks before the first message — so it is
 * where the missing control is explained, once, instead of in a Settings page
 * nobody opens to find out why a button is not there.
 */
import { Button } from "@/components/ui/button";
import { useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";

/** Which of the four states the surface is in. */
export type BotsEmptyKind = "no-provider" | "no-bot" | "no-conversation" | "secret-missing";

/**
 * The exact copy. Kept in one table so no sentence can be reworded in
 * isolation, and so a test asserts against the same constant the pane renders.
 *
 * `detail` is the second line the empty-state shape allows — used only where
 * the first sentence would otherwise have to carry two facts.
 */
export const BOTS_EMPTY_COPY: Record<
  BotsEmptyKind,
  { message: string; detail: string | null; action: string }
> = {
  "no-provider": {
    message:
      "No model endpoint yet. Add one in Settings — its address and its key stay on this machine.",
    detail: "keeper ships no endpoint of its own and talks to nothing you have not configured.",
    action: "Go to Settings",
  },
  "no-bot": {
    message: "No bot pinned yet. Name one in Settings and keeper will check it exists.",
    detail:
      "On Ollama a bot is a model tag; on Hermes it is a profile. keeper cannot list the profiles on a Hermes endpoint — the key you gave it opens the chat API, and that API has no route that names them.",
    action: "Go to Settings",
  },
  "no-conversation": {
    message: "Nothing asked yet. Type below and the answer lands here.",
    detail: null,
    action: "New conversation",
  },
  "secret-missing": {
    message: "This endpoint has no key stored. Add one in Settings before you send.",
    detail:
      "keeper found the endpoint's row but no credential behind it — the key may have been removed from the system keychain.",
    action: "Go to Settings",
  },
};

/**
 * What the phone says about the half of the surface it cannot have. `keeper-sync`
 * is not linkable on iOS, so a grant would be a promise about files keeper
 * cannot reach from there; the honest sentence names where the tools are.
 * One const, so the empty state and the `/grant` answer cannot word it twice.
 */
export const BOTS_NO_DRIVE_HERE_SENTENCE =
  "On this iPhone a bot can talk but cannot reach the folders you sync — the drive tools live on your Mac.";

/**
 * Render one empty state. It stands where the transcript would, so it takes
 * the transcript's box — `min-h-0 flex-1` — rather than a height of its own:
 * a band that could not shrink would push the composer under the window edge.
 */
export function BotEmptyState({ kind, onAction }: { kind: BotsEmptyKind; onAction: () => void }) {
  const { message, detail, action } = BOTS_EMPTY_COPY[kind];
  // The phone tier, read from the capability mirror rather than the platform:
  // the same predicate that shows "On this iPhone" in Settings → About.
  const reducedPlatform = useIsReducedCapabilityPlatform();
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 overflow-y-auto p-6">
      <p className="max-w-[36ch] text-center text-muted-foreground text-sm">{message}</p>
      {detail !== null && (
        <p className="max-w-[36ch] text-center text-muted-foreground text-xs">{detail}</p>
      )}
      {reducedPlatform && (
        <p className="max-w-[36ch] text-center text-muted-foreground text-xs">
          {BOTS_NO_DRIVE_HERE_SENTENCE}
        </p>
      )}
      <Button type="button" variant="outline" size="sm" onClick={onAction}>
        {action}
      </Button>
    </div>
  );
}
