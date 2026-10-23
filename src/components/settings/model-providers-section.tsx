/**
 * Settings › Model providers: the endpoints notes search asks for its
 * embeddings — added, tested, edited and removed here (Story 61.4, FR-379;
 * Story 108.3a, UX-DR-E108-4).
 *
 * # One form, two modes (AD-C7)
 *
 * {@link ProviderForm} is an **add** when `provider` is `undefined` and an
 * **edit** of that row when it is not, revealed inline by the section's
 * header for an add and by a row's disclosure for an edit. Two forms would be
 * two chances to word or validate the same base URL differently, **and the
 * one that is wrong is the one nobody is looking at**. Inline disclosure,
 * never a dialog; a destructive confirm IS a dialog, and its sentence names
 * what happens to which object.
 *
 * # What this form refuses to do
 *
 * **It does not validate the base URL.** The grammar is
 * `keeper_core::providers::parse_base_url` — scheme `http`/`https`, no
 * userinfo, no query, no fragment — and the field is sent as typed and Rust's
 * own sentence is rendered verbatim on a refusal.
 *
 * **It never renders a token, and an empty token field never clears one.**
 * There is no field on `ModelProviderVm` a credential could arrive in, so an
 * empty box means *unchanged*; clearing is its own explicit act.
 *
 * # Disclosure, not a blocklist
 *
 * A loopback or private-network base URL is accepted — disclosure plus an
 * explicit act. The row prints the **host** and whether it is private, the
 * same shape the egress disclosure in About uses, so one destination is named
 * one way.
 *
 * The same section renders on the phone: the row and the header are
 * `flex-wrap` with a floor under the name (`min-w-40`), so the verbs drop to
 * their own line at 390 px rather than squeezing the name.
 */
import { useCallback, useEffect, useRef, useState } from "react";
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
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ACCOUNT_OFFERS_TITLE, offerUrlHost } from "@/lib/account-offers";
import { logFailure } from "@/lib/failure";
import type {
  CredentialSource,
  ModelProviderVm,
  ProviderKind,
  ProviderOfferVm,
  ProviderProbeVm,
} from "@/lib/ipc/client";
import {
  accountOfferAddProvider,
  providerCredentialSourceGet,
  providerCredentialSourceSet,
  providerProbe,
  providerRemove,
  providerSave,
  providersList,
} from "@/lib/ipc/client";
import { accountUsable, useAccountStore } from "@/lib/stores/account";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The section heading, so the dialog and its test cannot disagree about it. */
export const MODEL_PROVIDERS_SECTION_TITLE = "Model providers";

/** The standing explanation, shown whatever is configured. */
export const MODEL_PROVIDERS_SECTION_NOTE =
  "Notes search asks one of these for the meaning of your notes. keeper ships no endpoint of its own: add the address of a model server you run, and its key is stored in this machine's keychain — never in a row and never on a screen.";

/** The verbs. */
export const PROVIDERS_ADD_LABEL = "Add a provider";
export const PROVIDERS_TEST_LABEL = "Test";
export const PROVIDERS_EDIT_LABEL = "Edit";
export const PROVIDERS_REMOVE_LABEL = "Remove";
export const PROVIDERS_SAVE_LABEL = "Save";
export const PROVIDERS_CANCEL_LABEL = "Cancel";
export const PROVIDERS_CLEAR_TOKEN_LABEL = "Forget the stored key";

/** The field labels. Option text is the stored spelling, never a prettified
 *  label — two words for one stored value is the drift AD-C7 forbids. */
export const PROVIDERS_KIND_LABEL = "Kind";
export const PROVIDERS_NAME_LABEL = "Name";
export const PROVIDERS_BASE_URL_LABEL = "Base URL";
export const PROVIDERS_TOKEN_LABEL = "Key";

/** What the key field says about leaving it blank. */
export const PROVIDERS_TOKEN_NOTE =
  "Leave blank to keep the key already stored. keeper cannot show you a stored key.";

/** What the base-URL field discloses. */
export const PROVIDERS_BASE_URL_NOTE =
  "A loopback or LAN address is fine and is disclosed as a destination in Settings → About.";

/** What a row with no stored key says. Not a fault by itself — a loopback
 *  Ollama legitimately has none — so it states the fact and stops. */
export const PROVIDERS_NO_TOKEN_CAPTION = "No key stored.";

/**
 * The account as this provider's key (Epic 82, AD-315, UX-DR116 (5)): sent as
 * `Authorization: Bearer` with a fresh account token. Offered only while the
 * account can stand in, or when this provider already uses it.
 */
export function providersAccountCredentialLabel(accountName: string): string {
  return `Use my ${accountName} account`;
}
export const PROVIDERS_ACCOUNT_CREDENTIAL_NOTE =
  "keeper sends a fresh token from your account with every request to this provider, so no key needs to be stored for it.";
export const PROVIDERS_ACCOUNT_SOURCE_FAILED_PREFIX =
  "The provider was saved, but keeper could not record where its key comes from: ";

/** What a row whose secret went missing says. This one IS a fault. */
export const PROVIDERS_SECRET_MISSING_CAPTION =
  "The key for this provider is gone from the keychain. Add it again before notes search asks it.";

/** The empty state. */
export const PROVIDERS_SECTION_EMPTY = "No provider yet.";

/** What a failed read says when Rust gave no sentence. */
export const PROVIDERS_SECTION_READ_FAILED = "keeper couldn't read your model providers.";

/**
 * The providers the account offers from the person's other devices (Epic 84,
 * UX-DR118). One that signs with the account, while the account can stand
 * in, is one tap: Rust adds it with its account key. Any other needs a key
 * typed on this device, so it opens the form prefilled — the ellipsis says a
 * form follows.
 */
export const PROVIDERS_OFFER_ADD_LABEL = "Add";
export const PROVIDERS_OFFER_ADD_FORM_LABEL = "Add…";
/** What a refused one-tap add says when Rust gave no sentence of its own. */
export const PROVIDERS_OFFER_ADD_FAILED = "keeper couldn't add this provider.";
/** Said at the form when an offer's Add… would throw away a draft (fix R20). */
export const PROVIDERS_OFFER_DRAFT_SENTENCE = "Save or cancel the provider you are adding first.";

/**
 * What an offer of a kind this build does not speak says in place of an add
 * (fix R22) — Rust's own refusal for the same offer, word for word.
 */
export function providersOfferUnknownKindSentence(kind: string): string {
  return `This version of keeper cannot talk to a ${kind} provider.`;
}

/** The three kinds, spelled as they are stored. */
const KINDS: readonly ProviderKind[] = ["ollama", "hermes", "openai"];

/**
 * How a probe reads, in the app's own reachability vocabulary: `offline` is
 * the word for a folder keeper cannot reach and for a dead homeserver, so a
 * provider that does not answer gets the same word. Rust's own `reason` comes
 * first wherever it wrote one.
 */
export function providerProbeSentence(probe: ProviderProbeVm): string {
  if (probe.reason !== null) {
    return probe.reason;
  }
  if (probe.reach === "offline") {
    return "Offline — nothing answered.";
  }
  return probe.version === null ? "Reachable." : `Reachable. It reports version ${probe.version}.`;
}

/**
 * What a removal confirmation says: the provider and its key go, and notes
 * search stops asking it for meaning until another is chosen.
 */
export function providerRemovalSentence(name: string | null): string {
  return name === null
    ? ""
    : `${name} is removed, along with the key stored for it. If notes search uses it, search falls back to words until you choose another model.`;
}

/** The Model providers settings section. */
export function ModelProvidersSection({ open }: { open: boolean }) {
  const [providers, setProviders] = useState<ModelProviderVm[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  /** The last probe verdict per provider, ready to print. */
  const [verdicts, setVerdicts] = useState<Record<string, string>>({});
  /** The row a confirmation is up for, or `null`. */
  const [removing, setRemoving] = useState<{ id: string; name: string } | null>(null);
  /**
   * The account's provider offers (Epic 84), and what this open of the section
   * did with them. `offerAdded` hides a row between Rust's yes and the next
   * snapshot, which drops the offer anyway.
   */
  const account = useAccountStore((s) => s.vm);
  const [offerBusy, setOfferBusy] = useState<string | null>(null);
  const [offerErrors, setOfferErrors] = useState<Record<string, string>>({});
  const [offerAdded, setOfferAdded] = useState<readonly string[]>([]);
  const [offerPrefill, setOfferPrefill] = useState<ProviderOfferVm | null>(null);
  const offers = account.offers.providers.filter((offer) => !offerAdded.includes(offer.key));
  const [formPristine, setFormPristine] = useState(true);
  const [offerNotice, setOfferNotice] = useState<string | null>(null);
  const [reveal, setReveal] = useState(0);
  const openOfferForm = (offer: ProviderOfferVm) => {
    if (!adding || formPristine) {
      setOfferPrefill(offer);
      setAdding(true);
      setOfferNotice(null);
    } else {
      setOfferNotice(PROVIDERS_OFFER_DRAFT_SENTENCE);
    }
    setReveal((count) => count + 1);
  };
  const closeForm = () => {
    setAdding(false);
    setOfferPrefill(null);
    setOfferNotice(null);
  };

  const refresh = useCallback(() => {
    void providersList()
      .then((list) => {
        setProviders(list);
        setError(null);
      })
      .catch((raw: unknown) => {
        logFailure("providers.list")(raw);
        setError(syncErrorMessage(raw, PROVIDERS_SECTION_READ_FAILED));
      });
  }, []);

  useEffect(() => {
    if (open) {
      refresh();
    }
  }, [open, refresh]);

  const list = providers ?? [];

  const probe = (providerId: string) => {
    void providerProbe(providerId)
      .then((answer) =>
        setVerdicts((held) => ({ ...held, [providerId]: providerProbeSentence(answer) })),
      )
      .catch((raw: unknown) => {
        logFailure("providers.probe")(raw);
        setVerdicts((held) => ({
          ...held,
          [providerId]: syncErrorMessage(raw, PROVIDERS_SECTION_READ_FAILED),
        }));
      })
      // The probe wrote a health verdict against the row, so the row is stale.
      .finally(refresh);
  };

  const remove = () => {
    if (removing === null) {
      return;
    }
    const target = removing;
    setRemoving(null);
    void providerRemove(target.id)
      .catch((raw: unknown) => {
        logFailure("providers.remove")(raw);
        setError(syncErrorMessage(raw, PROVIDERS_SECTION_READ_FAILED));
      })
      .finally(refresh);
  };

  const addOffer = (offer: ProviderOfferVm) => {
    setOfferBusy(offer.key);
    setOfferErrors(({ [offer.key]: _cleared, ...rest }) => rest);
    void accountOfferAddProvider(offer.key)
      .then(() => setOfferAdded((held) => [...held, offer.key]))
      .catch((raw: unknown) => {
        logFailure("providers.offerAdd")(raw);
        setOfferErrors((held) => ({
          ...held,
          [offer.key]: syncErrorMessage(raw, PROVIDERS_OFFER_ADD_FAILED),
        }));
      })
      .finally(() => {
        setOfferBusy(null);
        refresh();
      });
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="font-medium">{MODEL_PROVIDERS_SECTION_TITLE}</p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          onClick={() => {
            setOfferPrefill(null);
            setOfferNotice(null);
            setAdding(!adding);
          }}
        >
          {PROVIDERS_ADD_LABEL}
        </Button>
      </div>
      <p className="text-muted-foreground">{MODEL_PROVIDERS_SECTION_NOTE}</p>

      {error !== null && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}

      {adding && offerNotice !== null && (
        <p role="status" className="text-xs">
          {offerNotice}
        </p>
      )}
      {adding && (
        <ProviderForm
          key={offerPrefill?.key ?? "blank"}
          prefill={offerPrefill ?? undefined}
          onPristineChange={setFormPristine}
          revealRequest={reveal}
          onDone={() => {
            if (offerPrefill !== null) {
              const added = offerPrefill.key;
              setOfferAdded((held) => [...held, added]);
            }
            closeForm();
            refresh();
          }}
          onCancel={closeForm}
        />
      )}

      {list.length === 0 ? (
        <p className="text-muted-foreground">{PROVIDERS_SECTION_EMPTY}</p>
      ) : (
        <ul aria-label={MODEL_PROVIDERS_SECTION_TITLE} className="flex flex-col gap-2">
          {list.map((provider) => (
            <li key={provider.id} className="flex flex-col gap-1">
              <div className="flex flex-wrap items-center gap-2">
                <span className="min-w-40 flex-1">
                  {/* The host, never the full URL — the egress disclosure's own
                      shape, so one destination is named one way. */}
                  {provider.name} — {provider.kind} at {provider.host}
                  {provider.isPrivate === true ? " (private)" : ""}
                </span>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => probe(provider.id)}
                >
                  {PROVIDERS_TEST_LABEL}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => setEditing(editing === provider.id ? null : provider.id)}
                >
                  {PROVIDERS_EDIT_LABEL}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => setRemoving({ id: provider.id, name: provider.name })}
                >
                  {PROVIDERS_REMOVE_LABEL}
                </Button>
              </div>
              {provider.health === "secretMissing" ? (
                <p className="text-destructive text-xs">{PROVIDERS_SECRET_MISSING_CAPTION}</p>
              ) : (
                !provider.hasToken && (
                  <p className="text-muted-foreground text-xs">{PROVIDERS_NO_TOKEN_CAPTION}</p>
                )
              )}
              {verdicts[provider.id] !== undefined && (
                <p role="status" className="text-muted-foreground text-xs">
                  {verdicts[provider.id]}
                </p>
              )}
              {editing === provider.id && (
                <ProviderForm
                  provider={provider}
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

      {offers.length > 0 && (
        <section aria-labelledby="providers-offers-title" className="flex flex-col gap-2">
          <p id="providers-offers-title" className="font-medium">
            {ACCOUNT_OFFERS_TITLE}
          </p>
          <ul className="flex flex-col gap-2">
            {offers.map((offer) => {
              // A kind this build cannot speak gets Rust's sentence and no add
              // at all (fix R22). Otherwise one tap only where Rust will say
              // yes — the offer signs with the account and the account can
              // stand in now — and the form everywhere else.
              const known = KINDS.some((kind) => kind === offer.kind);
              const oneTap = known && offer.credential === "account" && accountUsable(account);
              return (
                <li key={offer.key} className="flex flex-col gap-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="min-w-40 flex-1">
                      {offer.name} — {offer.kind} at {offerUrlHost(offer.baseUrl)}
                    </span>
                    {oneTap && (
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        aria-label={`${PROVIDERS_OFFER_ADD_LABEL} ${offer.name}`}
                        disabled={offerBusy !== null}
                        onClick={() => addOffer(offer)}
                      >
                        {PROVIDERS_OFFER_ADD_LABEL}
                      </Button>
                    )}
                    {known && !oneTap && (
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        aria-label={`${PROVIDERS_OFFER_ADD_FORM_LABEL} ${offer.name}`}
                        onClick={() => openOfferForm(offer)}
                      >
                        {PROVIDERS_OFFER_ADD_FORM_LABEL}
                      </Button>
                    )}
                  </div>
                  {!known && (
                    <p className="text-muted-foreground text-xs">
                      {providersOfferUnknownKindSentence(offer.kind)}
                    </p>
                  )}
                  {offerErrors[offer.key] !== undefined && (
                    <p role="alert" className="text-destructive text-xs">
                      {offerErrors[offer.key]}
                    </p>
                  )}
                </li>
              );
            })}
          </ul>
        </section>
      )}

      <AlertDialog open={removing !== null} onOpenChange={(next) => !next && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{PROVIDERS_REMOVE_LABEL}</AlertDialogTitle>
            <AlertDialogDescription>
              {providerRemovalSentence(removing?.name ?? null)}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{PROVIDERS_CANCEL_LABEL}</AlertDialogCancel>
            <AlertDialogAction onClick={remove}>{PROVIDERS_REMOVE_LABEL}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/**
 * The provider form — an add when `provider` is absent, an edit of that row
 * when it is not (AD-C7).
 */
export function ProviderForm({
  provider,
  prefill,
  onDone,
  onCancel,
  onPristineChange,
  revealRequest = 0,
}: {
  provider?: ModelProviderVm;
  /**
   * A provider the account offers (Epic 84): an add starts from its kind,
   * name and base URL, and asks for its key as any add does. Read once, on
   * mount — the section remounts the form under the offer's key.
   */
  prefill?: ProviderOfferVm;
  onDone: () => void;
  onCancel: () => void;
  /** Told whether the form still holds only what it opened with (fix R20). */
  onPristineChange?: (pristine: boolean) => void;
  /** Each new value scrolls the form into view and focuses its key field. */
  revealRequest?: number;
}) {
  const [initial] = useState(() => ({
    kind: provider?.kind ?? KINDS.find((known) => known === prefill?.kind) ?? "ollama",
    name: provider?.name ?? prefill?.name ?? "",
    baseUrl: provider?.baseUrl ?? prefill?.baseUrl ?? "",
  }));
  const [kind, setKind] = useState<ProviderKind>(initial.kind);
  const [name, setName] = useState(initial.name);
  const [baseUrl, setBaseUrl] = useState(initial.baseUrl);
  const [token, setToken] = useState("");
  const [clearToken, setClearToken] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  const tokenField = useRef<HTMLInputElement>(null);
  const saveButton = useRef<HTMLButtonElement>(null);
  // Where the key comes from. Read only when an account is configured, and
  // again whenever the account itself changes: Rust answers `account` only
  // for a row bound to the account configured now.
  const account = useAccountStore((s) => s.vm);
  const [source, setSource] = useState<CredentialSource>("keychain");
  const [storedSource, setStoredSource] = useState<CredentialSource>("keychain");
  // An add that stored its provider but not its source: the retry finishes
  // THAT provider instead of creating a second one.
  const [createdId, setCreatedId] = useState<string | null>(null);
  const providerId = provider?.id;
  const accountId = account.configured ? account.id : null;
  const pristine =
    createdId === null &&
    kind === initial.kind &&
    name === initial.name &&
    baseUrl === initial.baseUrl &&
    token === "" &&
    !clearToken &&
    source === storedSource;
  useEffect(() => {
    onPristineChange?.(pristine);
  }, [pristine, onPristineChange]);
  useEffect(() => {
    if (revealRequest === 0) {
      return;
    }
    container.current?.scrollIntoView?.({ block: "nearest" });
    (tokenField.current ?? saveButton.current)?.focus();
  }, [revealRequest]);
  useEffect(() => {
    setStoredSource("keychain");
    setSource("keychain");
    if (providerId === undefined || accountId === null) {
      return;
    }
    let abandoned = false;
    void providerCredentialSourceGet(providerId)
      .then((value) => {
        if (!abandoned) {
          setStoredSource(value);
          setSource(value);
        }
      })
      // The keychain default stands; the stored choice is a refinement.
      .catch(logFailure("providers.readCredentialSource"));
    return () => {
      abandoned = true;
    };
  }, [providerId, accountId]);
  const accountOffered = accountUsable(account) || storedSource === "account";
  const useAccount = accountOffered && source === "account";

  const save = () => {
    setSaving(true);
    setError(null);
    const nextSource: CredentialSource = useAccount ? "account" : "keychain";
    void providerSave({
      id: providerId ?? createdId,
      kind,
      name,
      baseUrl,
      // Blank means unchanged, never cleared — see the module doc. With the
      // account as the key the field is hidden and nothing is sent.
      token: useAccount || token.length === 0 ? null : token,
      clearToken: !useAccount && clearToken,
    })
      .then(async (saved) => {
        if (nextSource !== storedSource) {
          try {
            await providerCredentialSourceSet(saved.id, nextSource);
          } catch (raw) {
            logFailure("providers.credentialSource")(raw);
            if (provider === undefined) {
              setCreatedId(saved.id);
            }
            setError(`${PROVIDERS_ACCOUNT_SOURCE_FAILED_PREFIX}${syncErrorMessage(raw)}`);
            return;
          }
        }
        onDone();
      })
      .catch((raw: unknown) => {
        logFailure("providers.save")(raw);
        // Rust's sentence verbatim: the base-URL grammar is its decision.
        setError(syncErrorMessage(raw, PROVIDERS_SECTION_READ_FAILED));
      })
      .finally(() => setSaving(false));
  };

  return (
    <div ref={container} className="flex flex-col gap-2 rounded-md border border-border p-3">
      <div className="flex flex-wrap items-center gap-1">
        <span className="text-muted-foreground text-xs">{PROVIDERS_KIND_LABEL}</span>
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
      <Label htmlFor="provider-name">{PROVIDERS_NAME_LABEL}</Label>
      <Input id="provider-name" value={name} onChange={(event) => setName(event.target.value)} />
      <Label htmlFor="provider-url">{PROVIDERS_BASE_URL_LABEL}</Label>
      <Input
        id="provider-url"
        value={baseUrl}
        onChange={(event) => setBaseUrl(event.target.value)}
      />
      <p className="text-muted-foreground text-xs">{PROVIDERS_BASE_URL_NOTE}</p>
      {accountOffered && (
        <div className="flex flex-col gap-1">
          <Label className="flex items-center gap-2">
            <Checkbox
              checked={source === "account"}
              onCheckedChange={(next) => setSource(next === true ? "account" : "keychain")}
            />
            {providersAccountCredentialLabel(account.name ?? "organisation")}
          </Label>
          {useAccount && (
            <p className="text-muted-foreground text-xs">{PROVIDERS_ACCOUNT_CREDENTIAL_NOTE}</p>
          )}
        </div>
      )}
      {!useAccount && (
        <>
          <Label htmlFor="provider-token">{PROVIDERS_TOKEN_LABEL}</Label>
          <Input
            ref={tokenField}
            id="provider-token"
            type="password"
            value={token}
            onChange={(event) => setToken(event.target.value)}
          />
          <p className="text-muted-foreground text-xs">{PROVIDERS_TOKEN_NOTE}</p>
          {provider?.hasToken === true && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              aria-pressed={clearToken}
              onClick={() => setClearToken(!clearToken)}
            >
              {PROVIDERS_CLEAR_TOKEN_LABEL}
            </Button>
          )}
        </>
      )}
      {error !== null && (
        <p role="alert" className="text-destructive text-xs">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button ref={saveButton} type="button" size="sm" disabled={saving} onClick={save}>
          {PROVIDERS_SAVE_LABEL}
        </Button>
        <Button type="button" variant="outline" size="sm" onClick={onCancel}>
          {PROVIDERS_CANCEL_LABEL}
        </Button>
      </div>
    </div>
  );
}
