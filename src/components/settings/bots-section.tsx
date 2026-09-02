/**
 * Settings → Bots: where an endpoint and a bot are added, tested, edited and
 * removed (Epic 61, Story 61.4, FR-379).
 *
 * # One component, two modes (AD-C7)
 *
 * {@link BotProviderForm} is an **add** when `provider` is `undefined` and an
 * **edit** of that row when it is not, revealed inline by the section's own
 * header for an add and by a row's disclosure for an edit. Same for
 * {@link BotForm}. The reason is not tidiness: two forms would be two chances
 * to word or validate the same base URL differently, **and the one that is
 * wrong is the one nobody is looking at**.
 *
 * Inline disclosure, never a dialog — a modal over a list of providers hides
 * the rows whose settings the person is comparing this one against. A
 * destructive confirm IS a dialog, and its sentence names what happens to which
 * object.
 *
 * # What this form refuses to do
 *
 * **It does not validate the base URL.** The grammar is
 * `keeper_core::bots::parse_base_url` — scheme `http`/`https`, no userinfo, no
 * query, no fragment — and re-implementing any of it here would produce a
 * second grammar that disagrees with the one that actually decides. The field
 * is sent as typed and Rust's own sentence is rendered verbatim on a refusal.
 *
 * **It never renders a token, and an empty token field never clears one.**
 * There is no field on `BotProviderVm` a credential could arrive in, so an
 * empty box means *unchanged*; clearing is its own explicit act. A save that
 * treated blank as deletion would unauthenticate a working provider every time
 * somebody fixed a typo in its name.
 *
 * # Disclosure, not a blocklist
 *
 * A loopback or private-network base URL is **accepted** — that is the epic's
 * answer to the SSRF question: disclosure plus an explicit user act. So the row
 * prints the **host** and whether it is private, and never the URL a
 * credential could be smuggled into. The grammar refuses userinfo outright, so
 * there is nothing in the normalized URL to redact — but the row still shows
 * the host, because that is the same shape the egress disclosure uses for a git
 * remote and two surfaces naming one destination differently is how somebody
 * concludes they are two destinations.
 */
import { useCallback, useEffect, useState } from "react";
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
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { BotProbeVm, BotProviderVm, BotVm, ProviderKind } from "@/lib/ipc/client";
import {
  botsBotProbe,
  botsBotRemove,
  botsBotSave,
  botsBotsList,
  botsProviderProbe,
  botsProviderRemove,
  botsProviderSave,
  botsProvidersList,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The section heading, so the dialog and its test cannot disagree about it. */
export const BOTS_SECTION_TITLE = "Bots";

/** The standing explanation, shown whatever is configured. */
export const BOTS_SECTION_NOTE =
  "keeper ships no endpoint of its own. Add the address of a model server you run, and its key is stored in this machine's keychain — never in a row and never on a screen.";

/** The verbs. */
export const BOTS_ADD_PROVIDER_LABEL = "Add an endpoint";
export const BOTS_ADD_BOT_LABEL = "Add a bot";
export const BOTS_TEST_LABEL = "Test";
export const BOTS_EDIT_LABEL = "Edit";
export const BOTS_REMOVE_LABEL = "Remove";
export const BOTS_SAVE_LABEL = "Save";
export const BOTS_CANCEL_LABEL = "Cancel";
export const BOTS_CLEAR_TOKEN_LABEL = "Forget the stored key";

/** The field labels. Option text is the stored spelling, never a prettified
 *  label — two words for one stored value is the drift AD-C7 forbids. */
export const BOTS_KIND_LABEL = "Kind";
export const BOTS_NAME_LABEL = "Name";
export const BOTS_BASE_URL_LABEL = "Base URL";
export const BOTS_TOKEN_LABEL = "Key";
export const BOTS_TARGET_LABEL = "Profile or model tag";
export const BOTS_PROVIDER_LABEL = "Endpoint";

/** What the key field says about leaving it blank. */
export const BOTS_TOKEN_NOTE =
  "Leave blank to keep the key already stored. keeper cannot show you a stored key.";

/** What the base-URL field discloses. */
export const BOTS_BASE_URL_NOTE =
  "A loopback or LAN address is fine and is disclosed as a destination in Settings → About.";

/** What a row with no stored key says. Not a fault by itself — a loopback
 *  Ollama legitimately has none — so it states the fact and stops. */
export const BOTS_NO_TOKEN_CAPTION = "No key stored.";

/** What a row whose secret went missing says. This one IS a fault. */
export const BOTS_SECRET_MISSING_CAPTION =
  "The key for this endpoint is gone from the keychain. Add it again before you send.";

/** The empty state. */
export const BOTS_SECTION_EMPTY = "No endpoint yet.";

/** What a failed read says when Rust gave no sentence. */
export const BOTS_SECTION_READ_FAILED = "keeper couldn't read your endpoints.";

/** The two kinds, spelled as they are stored. */
const KINDS: readonly ProviderKind[] = ["ollama", "hermes"];

/**
 * How a probe reads, in the app's own reachability vocabulary.
 *
 * `offline` is the word `keeper-sync` uses for a folder it cannot reach and the
 * word the connection pill uses for a dead homeserver, and a chat endpoint that
 * does not answer is the same event — so it gets the same word rather than a
 * third spelling. Rust's own `reason` comes first wherever it wrote one,
 * because Rust composes the sentence and the frontend renders it verbatim.
 */
export function botProbeSentence(probe: BotProbeVm): string {
  if (probe.reason !== null) {
    return probe.reason;
  }
  if (probe.reach === "offline") {
    return "Offline — nothing answered.";
  }
  const version = probe.version === null ? "" : ` It reports version ${probe.version}.`;
  const presence =
    probe.presence === null
      ? ""
      : probe.presence === "exists"
        ? ` The bot ${probe.bot} is there.`
        : probe.presence === "absent"
          ? ` This endpoint has no bot called ${probe.bot}.`
          : ` keeper could not tell whether ${probe.bot} is there.`;
  return `Reachable.${version}${presence}`;
}

/** The Bots settings section. */
export function BotsSection({ open }: { open: boolean }) {
  const [providers, setProviders] = useState<BotProviderVm[] | null>(null);
  const [bots, setBots] = useState<BotVm[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Which disclosure is revealed: an add, an edit of one row, or none. */
  const [adding, setAdding] = useState<"provider" | "bot" | null>(null);
  const [editingProvider, setEditingProvider] = useState<string | null>(null);
  const [editingBot, setEditingBot] = useState<string | null>(null);
  /** The last probe verdict per provider or bot target, ready to print. */
  const [verdicts, setVerdicts] = useState<Record<string, string>>({});
  /** The row a confirmation is up for, or `null`. */
  const [removing, setRemoving] = useState<{
    kind: "provider" | "bot";
    id: string;
    name: string;
  } | null>(null);

  // A `useCallback` with no dependencies — it closes over nothing but the state
  // setters, which React guarantees are stable — so the effect below can name
  // it as a real dependency rather than suppress the lint. A plain function
  // would be rebuilt every render and would re-read on every keystroke in a
  // form above it: one round trip per character.
  const refresh = useCallback(() => {
    void Promise.allSettled([botsProvidersList(), botsBotsList()]).then(
      ([providerRead, botRead]) => {
        if (providerRead.status === "fulfilled") {
          setProviders(providerRead.value);
        }
        if (botRead.status === "fulfilled") {
          setBots(botRead.value);
        }
        const failure = [providerRead, botRead].find((read) => read.status === "rejected");
        setError(
          failure === undefined || failure.status !== "rejected"
            ? null
            : syncErrorMessage(failure.reason, BOTS_SECTION_READ_FAILED),
        );
      },
    );
  }, []);

  useEffect(() => {
    if (open) {
      refresh();
    }
  }, [open, refresh]);

  const providerList = providers ?? [];
  const botList = bots ?? [];

  const probeProvider = (providerId: string) => {
    void botsProviderProbe(providerId)
      .then((probe) => setVerdicts((held) => ({ ...held, [providerId]: botProbeSentence(probe) })))
      .catch((raw: unknown) => {
        setVerdicts((held) => ({
          ...held,
          [providerId]: syncErrorMessage(raw, BOTS_SECTION_READ_FAILED),
        }));
      })
      // The probe wrote a health verdict against the row, so the row is stale.
      .finally(refresh);
  };

  const probeBot = (bot: BotVm) => {
    void botsBotProbe(bot.providerId, bot.target)
      .then((probe) => setVerdicts((held) => ({ ...held, [bot.id]: botProbeSentence(probe) })))
      .catch((raw: unknown) => {
        setVerdicts((held) => ({
          ...held,
          [bot.id]: syncErrorMessage(raw, BOTS_SECTION_READ_FAILED),
        }));
      });
  };

  const remove = () => {
    if (removing === null) {
      return;
    }
    const target = removing;
    setRemoving(null);
    const call =
      target.kind === "provider" ? botsProviderRemove(target.id) : botsBotRemove(target.id);
    void call
      .catch((raw: unknown) => setError(syncErrorMessage(raw, BOTS_SECTION_READ_FAILED)))
      .finally(refresh);
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <div className="flex items-center justify-between gap-2">
        <p className="font-medium">{BOTS_SECTION_TITLE}</p>
        <div className="flex shrink-0 gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setAdding(adding === "provider" ? null : "provider")}
          >
            {BOTS_ADD_PROVIDER_LABEL}
          </Button>
          {providerList.length > 0 && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setAdding(adding === "bot" ? null : "bot")}
            >
              {BOTS_ADD_BOT_LABEL}
            </Button>
          )}
        </div>
      </div>
      <p className="text-muted-foreground">{BOTS_SECTION_NOTE}</p>

      {error !== null && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}

      {adding === "provider" && (
        <BotProviderForm
          onDone={() => {
            setAdding(null);
            refresh();
          }}
          onCancel={() => setAdding(null)}
        />
      )}

      {providerList.length === 0 ? (
        <p className="text-muted-foreground">{BOTS_SECTION_EMPTY}</p>
      ) : (
        <ul aria-label="Endpoints" className="flex flex-col gap-2">
          {providerList.map((provider) => (
            <li key={provider.id} className="flex flex-col gap-1">
              <div className="flex items-center gap-2">
                <span className="min-w-0 flex-1">
                  {/* The host, never the full URL — the egress disclosure's own
                      shape, so one destination is named one way. */}
                  {provider.name} — {provider.kind} at {provider.host}
                  {provider.isPrivate === true ? " (private)" : ""}
                </span>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => probeProvider(provider.id)}
                >
                  {BOTS_TEST_LABEL}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    setEditingProvider(editingProvider === provider.id ? null : provider.id)
                  }
                >
                  {BOTS_EDIT_LABEL}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    setRemoving({ kind: "provider", id: provider.id, name: provider.name })
                  }
                >
                  {BOTS_REMOVE_LABEL}
                </Button>
              </div>
              {provider.health === "secretMissing" ? (
                <p className="text-destructive text-xs">{BOTS_SECRET_MISSING_CAPTION}</p>
              ) : (
                !provider.hasToken && (
                  <p className="text-muted-foreground text-xs">{BOTS_NO_TOKEN_CAPTION}</p>
                )
              )}
              {verdicts[provider.id] !== undefined && (
                <p role="status" className="text-muted-foreground text-xs">
                  {verdicts[provider.id]}
                </p>
              )}
              {editingProvider === provider.id && (
                <BotProviderForm
                  provider={provider}
                  onDone={() => {
                    setEditingProvider(null);
                    refresh();
                  }}
                  onCancel={() => setEditingProvider(null)}
                />
              )}
            </li>
          ))}
        </ul>
      )}

      {adding === "bot" && providerList.length > 0 && (
        <BotForm
          providers={providerList}
          onDone={() => {
            setAdding(null);
            refresh();
          }}
          onCancel={() => setAdding(null)}
        />
      )}

      {botList.length > 0 && (
        <ul aria-label="Bots" className="flex flex-col gap-2">
          {botList.map((bot) => (
            <li key={bot.id} className="flex flex-col gap-1">
              <div className="flex items-center gap-2">
                <span className="min-w-0 flex-1">
                  {bot.name} — {bot.target}
                </span>
                <Button type="button" variant="outline" size="sm" onClick={() => probeBot(bot)}>
                  {BOTS_TEST_LABEL}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => setEditingBot(editingBot === bot.id ? null : bot.id)}
                >
                  {BOTS_EDIT_LABEL}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => setRemoving({ kind: "bot", id: bot.id, name: bot.name })}
                >
                  {BOTS_REMOVE_LABEL}
                </Button>
              </div>
              {verdicts[bot.id] !== undefined && (
                <p role="status" className="text-muted-foreground text-xs">
                  {verdicts[bot.id]}
                </p>
              )}
              {editingBot === bot.id && (
                <BotForm
                  providers={providerList}
                  bot={bot}
                  onDone={() => {
                    setEditingBot(null);
                    refresh();
                  }}
                  onCancel={() => setEditingBot(null)}
                />
              )}
            </li>
          ))}
        </ul>
      )}

      <AlertDialog open={removing !== null} onOpenChange={(next) => !next && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{BOTS_REMOVE_LABEL}</AlertDialogTitle>
            {/* The confirmation names what happens to which object — the
                house's chain-of-custody rule — and it names the consequence
                rather than asking "are you sure?". */}
            <AlertDialogDescription>{removalSentence(removing)}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{BOTS_CANCEL_LABEL}</AlertDialogCancel>
            <AlertDialogAction onClick={remove}>{BOTS_REMOVE_LABEL}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/**
 * What a removal confirmation says.
 *
 * Two objects, two consequences, and each names the second-order effect the
 * person cannot see from the row: removing an endpoint takes its bots and its
 * key, and removing a bot leaves its conversations alone.
 */
export function removalSentence(target: { kind: "provider" | "bot"; name: string } | null): string {
  if (target === null) {
    return "";
  }
  return target.kind === "provider"
    ? `${target.name} is removed, along with every bot on it and the key stored for it. Conversations you have already had are kept.`
    : `${target.name} is unpinned and its own key is forgotten. Conversations you have already had with it are kept.`;
}

/**
 * The provider form — an add when `provider` is absent, an edit of that row
 * when it is not (AD-C7).
 */
export function BotProviderForm({
  provider,
  onDone,
  onCancel,
}: {
  provider?: BotProviderVm;
  onDone: () => void;
  onCancel: () => void;
}) {
  const [kind, setKind] = useState<ProviderKind>(provider?.kind ?? "ollama");
  const [name, setName] = useState(provider?.name ?? "");
  const [baseUrl, setBaseUrl] = useState(provider?.baseUrl ?? "");
  const [token, setToken] = useState("");
  const [clearToken, setClearToken] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = () => {
    setSaving(true);
    setError(null);
    void botsProviderSave({
      id: provider?.id ?? null,
      kind,
      name,
      baseUrl,
      // Blank means unchanged, never cleared — see the module doc.
      token: token.length === 0 ? null : token,
      clearToken,
    })
      .then(onDone)
      .catch((raw: unknown) => {
        // Rust's sentence verbatim: the base-URL grammar is its decision and
        // its wording names exactly what was wrong.
        setError(syncErrorMessage(raw, BOTS_SECTION_READ_FAILED));
      })
      .finally(() => setSaving(false));
  };

  return (
    <div className="flex flex-col gap-2 rounded-md border border-border p-3">
      <div className="flex flex-wrap items-center gap-1">
        <span className="text-muted-foreground text-xs">{BOTS_KIND_LABEL}</span>
        {KINDS.map((option) => (
          <Button
            key={option}
            type="button"
            size="sm"
            variant={kind === option ? "secondary" : "ghost"}
            aria-pressed={kind === option}
            onClick={() => setKind(option)}
          >
            {/* The stored spelling, not a prettified label. */}
            {option}
          </Button>
        ))}
      </div>
      <Label htmlFor="bots-provider-name">{BOTS_NAME_LABEL}</Label>
      <Input
        id="bots-provider-name"
        value={name}
        onChange={(event) => setName(event.target.value)}
      />
      <Label htmlFor="bots-provider-url">{BOTS_BASE_URL_LABEL}</Label>
      <Input
        id="bots-provider-url"
        value={baseUrl}
        onChange={(event) => setBaseUrl(event.target.value)}
      />
      <p className="text-muted-foreground text-xs">{BOTS_BASE_URL_NOTE}</p>
      <Label htmlFor="bots-provider-token">{BOTS_TOKEN_LABEL}</Label>
      <Input
        id="bots-provider-token"
        type="password"
        value={token}
        onChange={(event) => setToken(event.target.value)}
      />
      <p className="text-muted-foreground text-xs">{BOTS_TOKEN_NOTE}</p>
      {provider?.hasToken === true && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-pressed={clearToken}
          onClick={() => setClearToken(!clearToken)}
        >
          {BOTS_CLEAR_TOKEN_LABEL}
        </Button>
      )}
      {error !== null && (
        <p role="alert" className="text-destructive text-xs">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button type="button" size="sm" disabled={saving} onClick={save}>
          {BOTS_SAVE_LABEL}
        </Button>
        <Button type="button" variant="outline" size="sm" onClick={onCancel}>
          {BOTS_CANCEL_LABEL}
        </Button>
      </div>
    </div>
  );
}

/** The bot form — an add when `bot` is absent, an edit of that row when it is
 *  not (AD-C7). */
export function BotForm({
  providers,
  bot,
  onDone,
  onCancel,
}: {
  providers: BotProviderVm[];
  bot?: BotVm;
  onDone: () => void;
  onCancel: () => void;
}) {
  const [providerId, setProviderId] = useState(bot?.providerId ?? providers[0]?.id ?? "");
  const [target, setTarget] = useState(bot?.target ?? "");
  const [name, setName] = useState(bot?.name ?? "");
  const [token, setToken] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = () => {
    setSaving(true);
    setError(null);
    void botsBotSave({
      id: bot?.id ?? null,
      providerId,
      target,
      name,
      token: token.length === 0 ? null : token,
      clearToken: false,
    })
      .then(onDone)
      .catch((raw: unknown) => setError(syncErrorMessage(raw, BOTS_SECTION_READ_FAILED)))
      .finally(() => setSaving(false));
  };

  return (
    <div className="flex flex-col gap-2 rounded-md border border-border p-3">
      <div className="flex flex-wrap items-center gap-1">
        <span className="text-muted-foreground text-xs">{BOTS_PROVIDER_LABEL}</span>
        {providers.map((option) => (
          <Button
            key={option.id}
            type="button"
            size="sm"
            variant={providerId === option.id ? "secondary" : "ghost"}
            aria-pressed={providerId === option.id}
            onClick={() => setProviderId(option.id)}
          >
            {option.name}
          </Button>
        ))}
      </div>
      <Label htmlFor="bots-bot-target">{BOTS_TARGET_LABEL}</Label>
      <Input
        id="bots-bot-target"
        value={target}
        onChange={(event) => setTarget(event.target.value)}
      />
      <Label htmlFor="bots-bot-name">{BOTS_NAME_LABEL}</Label>
      <Input id="bots-bot-name" value={name} onChange={(event) => setName(event.target.value)} />
      <Label htmlFor="bots-bot-token">{BOTS_TOKEN_LABEL}</Label>
      <Input
        id="bots-bot-token"
        type="password"
        value={token}
        onChange={(event) => setToken(event.target.value)}
      />
      <p className="text-muted-foreground text-xs">{BOTS_TOKEN_NOTE}</p>
      {error !== null && (
        <p role="alert" className="text-destructive text-xs">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button type="button" size="sm" disabled={saving} onClick={save}>
          {BOTS_SAVE_LABEL}
        </Button>
        <Button type="button" variant="outline" size="sm" onClick={onCancel}>
          {BOTS_CANCEL_LABEL}
        </Button>
      </div>
    </div>
  );
}
