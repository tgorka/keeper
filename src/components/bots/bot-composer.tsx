/**
 * The Bots composer (Epic 61, Stories 61.4 and 61.9).
 *
 * A deliberately small sibling of the Matrix `Composer` rather than a reuse of
 * it: that one is keyed on `(accountId, roomId)`, persists a draft through
 * `notes`-style IPC, mirrors it cross-device, throttles typing notices and
 * carries an attachment tray — every one of which is a Matrix concept with no
 * counterpart here. Sharing it would mean widening seven props with "or null,
 * when this is a bot" and would leave two surfaces able to disagree about what
 * Enter means.
 *
 * # The keyboard model, stated because two of the three rules are refusals
 *
 * - **Enter sends** — or accepts the highlighted command, where the menu is
 *   open. The desktop chat default, and what the Matrix composer does.
 * - **Shift+Enter is a newline**, menu or no menu. The only way to write a
 *   paragraph, and the one chord that must never depend on what is on screen.
 * - **⌘Enter is NOT bound.** It is the "send anyway" chord in tools where
 *   Enter is a newline, so binding it here would mean two chords send and
 *   neither is discoverable from the other. An unbound chord falls through to
 *   the webview, which is AD-27's no-dead-chord rule applied to a key rather
 *   than to a button.
 *
 * A composing IME is guarded first, as every keyboard surface in this tree
 * does: acting on Enter mid-composition sends a half-finished candidate and
 * loses the composition.
 *
 * # The slash path decides nothing
 *
 * Every question about what a draft *means* goes to `keeper_core::bots::
 * commands` through {@link botsCommandPreview}: which command a spelling is,
 * whether it may run, what to say when it may not, and whether a leading slash
 * was escaped. This file asks and obeys. The three verdicts are exhaustive and
 * only one of them sends — which is how an unknown command is structurally
 * unable to reach an endpoint as prose, rather than being kept out by a branch
 * somebody has to remember.
 *
 * Two consequences worth naming:
 *
 * - **The field is never disabled.** `/help` is how somebody with no provider
 *   configured finds out what to do next, so a field that refused to take it
 *   would be a closed door with the instructions behind it. Send is disabled
 *   instead, and the no-bot line says why.
 * - **`/help` is answered here**, because the rows are already in this
 *   component's hands and the pane has nothing to add to them. Every other
 *   command leaves through `onCommand`.
 */
import { type KeyboardEvent, type ReactNode, useCallback, useRef, useState } from "react";
import {
  type BotPasteContext,
  type BotPasteDecision,
  botPasteDecision,
} from "@/components/bots/bot-paste";
import type { BotCommandContext, BotCommandInvocation } from "@/components/bots/bot-slash-menu";
import type { SlashRow } from "@/components/slash-menu/slash-menu";
import { SlashHelp, SlashMenuList } from "@/components/slash-menu/slash-menu-list";
import { Button } from "@/components/ui/button";
import { useSlashMenu } from "@/hooks/use-slash-menu";
import type { BotCommandPreviewVm } from "@/lib/ipc/client";
import { botsCommandPreview } from "@/lib/ipc/client";
import { NO_TEXT_SUBSTITUTION } from "@/lib/text-entry";

/** The textarea's accessible name and its placeholder. */
export const BOT_COMPOSER_LABEL = "Message";
export const BOT_COMPOSER_PLACEHOLDER = "Ask this bot, or type / for a command";

/** The send verb. */
export const BOT_COMPOSER_SEND_LABEL = "Send";

/** The stop verb, shown in place of Send while an answer is arriving. */
export const BOT_COMPOSER_STOP_LABEL = "Stop";

/**
 * What the composer says while it has no bot to ask. One sentence for both
 * tiers; `place` is where THIS tier chooses a bot, because the desktop's
 * picker is a row over the transcript and the phone's is a sheet opened
 * from the header (Story 63.1, FR-411) — "above" on a phone points at a
 * back bar.
 */
export const botComposerNoBot = (place: string) =>
  `Choose a bot ${place} and this will send to it.`;

/** Where the desktop chooses a bot: the picker row over the transcript. */
export const BOT_COMPOSER_PICKER_ABOVE = "above";

/**
 * The whole registry, as `/help` asks for it.
 *
 * A bare slash is the draft that matches everything, so this is the same
 * question the menu asks and not a second command.
 */
const HELP_DRAFT = "/";

export function BotComposer({
  onSend,
  onStop,
  onCommand,
  commandContext,
  streaming,
  disabled,
  pasteContext = null,
  onPaste,
  resolveDraft = botsCommandPreview,
  accessory,
  pickerPlace = BOT_COMPOSER_PICKER_ABOVE,
}: {
  /** Send the trimmed text. The parent owns the IPC call and the errors. */
  onSend: (text: string) => void;
  /** Stop the answer currently arriving. */
  onStop: () => void;
  /**
   * Run a command keeper resolved. Never called for `/help`, which this
   * component answers, and never called for a command that could not run —
   * those arrive as a refusal and are shown, not forwarded.
   *
   * **Returns `null` when the host acted, or the sentence to show when it did
   * not.** A verb whose surface the host cannot reach is a fact only the host
   * knows, and a command that silently did nothing is the affordance AD-27
   * forbids — so the host answers rather than shrugging, and its sentence is
   * shown exactly where a refusal is shown.
   */
  onCommand: (command: BotCommandInvocation) => string | null;
  /** What the pane knows, which is what decides a command's availability. */
  commandContext: BotCommandContext;
  /** Whether an answer is arriving right now — Stop replaces Send. */
  streaming: boolean;
  /** Whether there is anything to send to. */
  disabled: boolean;
  /**
   * What Story 61.12 needs to decide a paste: the chosen model's vision
   * capability and how many images are already attached. `null` keeps the
   * browser's own behaviour, which is the honest answer where keeper has no
   * model to ask.
   */
  pasteContext?: BotPasteContext | null;
  /**
   * What the paste turned out to be, where it was not the browser's to handle.
   * The tray and the refusal line belong to the pane; this component stores
   * nothing about an image.
   */
  onPaste?: (decision: BotPasteDecision) => void;
  /**
   * How a draft is resolved. The default is the real command; a test passes
   * its own so the surface's obligations can be asserted against each verdict
   * without a shell behind it.
   */
  resolveDraft?: (
    draft: string,
    context: {
      providerKind: BotCommandContext["providerKind"];
      hasProvider: boolean;
      hasBot: boolean;
      hasSession: boolean;
      modelTools: boolean | null;
    },
  ) => Promise<BotCommandPreviewVm>;
  /**
   * A control that shares the field's row — the talk-mode microphone on a
   * phone. Rendered between the field and the verb, absent where undefined.
   */
  accessory?: ReactNode;
  /**
   * Where a bot is chosen on this tier, as the no-bot caption names it. The
   * default is the desktop's row above; the phone names its sheet.
   */
  pickerPlace?: string;
}) {
  const [draft, setDraft] = useState("");
  const [refusal, setRefusal] = useState<string | null>(null);
  // The whole answer to `/help`, held together: the panel outlives the draft
  // that opened it, so it cannot read the escape hint off a preview the
  // cleared field has already discarded.
  const [help, setHelp] = useState<{
    rows: readonly SlashRow[];
    escapeHint: string;
  } | null>(null);

  const { providerKind, hasProvider, hasBot, hasSession, modelTools } = commandContext;
  const resolve = useCallback(
    async (text: string) =>
      await resolveDraft(text, { providerKind, hasProvider, hasBot, hasSession, modelTools }),
    [resolveDraft, providerKind, hasProvider, hasBot, hasSession, modelTools],
  );

  // One submission at a time (R393): taken by the Enter or click that starts
  // it, before the preview is awaited, so a second Enter while Rust is asked
  // does nothing instead of running the line twice.
  const inFlight = useRef(false);

  /** Clear the field only if it still holds `ran`: newer typing is kept (R393). */
  const spend = (ran: string) => setDraft((current) => (current.trim() === ran ? "" : current));

  const submit = (text: string) => {
    inFlight.current = true;
    void resolve(text)
      .then((answer) => {
        switch (answer.verdict.kind) {
          case "prose": {
            // The one path that reaches a model, and the escape arrives here
            // already unwrapped: `//etc` is the text `/etc`.
            if (disabled || streaming) {
              return;
            }
            spend(text);
            setRefusal(null);
            onSend(answer.verdict.text);
            return;
          }
          case "command": {
            spend(text);
            setRefusal(null);
            if (answer.verdict.name === "help") {
              void resolve(HELP_DRAFT)
                .then((all) => setHelp({ rows: all.rows, escapeHint: all.escapeHint }))
                // The narrowed answer is still the truth about this token, and
                // a panel of one row beats a panel that never opened; the
                // failed full list is said and logged once (R381).
                .catch((raw: unknown) => {
                  menu.fail(raw);
                  setHelp({ rows: answer.rows, escapeHint: answer.escapeHint });
                });
              return;
            }
            setRefusal(onCommand({ name: answer.verdict.name, args: answer.verdict.args }));
            return;
          }
          case "refusal": {
            // The draft is left in the field on purpose: it is one keystroke
            // from being right, and clearing it would make a typo cost the
            // whole sentence. A refusal is the answer, not a failure: nothing
            // goes to `keeper.log` (R393).
            setRefusal(answer.verdict.message);
            return;
          }
        }
      })
      .catch((raw: unknown) => {
        // A rejection means the shell is gone, and sending text nobody
        // resolved is the one thing this component must not do.
        menu.fail(raw);
        setRefusal(BOT_COMPOSER_UNRESOLVED);
      })
      .finally(() => {
        inFlight.current = false;
      });
  };

  const send = () => {
    const text = draft.trim();
    if (text.length === 0 || streaming || inFlight.current) {
      return;
    }
    // **A draft with no leading slash is prose, and is sent without asking.**
    // Not a shortcut around the registry: no command and no escape in it can
    // begin with anything but a slash, so this is the same boundary
    // `isCommandLine` already draws locally — and it keeps the ordinary path,
    // which is nearly every message, free of a round trip.
    if (!text.startsWith("/")) {
      if (disabled) {
        return;
      }
      setDraft("");
      setRefusal(null);
      onSend(text);
      return;
    }
    submit(text);
  };

  const menu = useSlashMenu({
    draft,
    enabled: true,
    resolve,
    onComplete: (completed) => {
      setDraft(completed);
      setRefusal(null);
    },
    onRun: send,
    reportAs: "bots-command-preview",
  });

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // A composing IME delivers Enter as a candidate selection; acting on it
    // would lose the composition.
    if (event.nativeEvent.isComposing || menu.onKeyDown(event)) {
      return;
    }
    // Everything the menu did not claim falls through untouched — including
    // Shift+Enter and ⌘Enter, which is why they are not listed here.
    if (event.key !== "Enter" || event.shiftKey || event.metaKey || event.ctrlKey || event.altKey) {
      return;
    }
    event.preventDefault();
    send();
  };

  return (
    <div className="flex shrink-0 flex-col gap-2 border-border border-t px-6 py-3">
      {help !== null && (
        <SlashHelp rows={help.rows} escapeHint={help.escapeHint} onClose={() => setHelp(null)} />
      )}

      {refusal !== null && (
        <p role="alert" className="text-destructive text-sm">
          {refusal}
        </p>
      )}
      {/* One line about the slash path: the refusal when there is one, else why
          the menu could not be asked (R381). */}
      {refusal === null && menu.failure !== null && (
        <p role="alert" className="text-destructive text-sm">
          {menu.failure}
        </p>
      )}

      {disabled && <p className="text-muted-foreground text-xs">{botComposerNoBot(pickerPlace)}</p>}

      {/* The field and its verb share one row (Story 61.14): stacked, the
          verb's row was a 40px band of its own under a three-line field, and
          every band here is height the transcript above does not get. */}
      <div className="relative flex items-end gap-2">
        <SlashMenuList menu={menu} />
        <textarea
          {...NO_TEXT_SUBSTITUTION}
          aria-label={BOT_COMPOSER_LABEL}
          {...menu.fieldProps}
          placeholder={BOT_COMPOSER_PLACEHOLDER}
          value={draft}
          rows={3}
          className="min-h-0 min-w-0 flex-1 resize-none rounded-md border border-border bg-background px-3 py-2 text-sm"
          onChange={(event) => {
            setDraft(event.target.value);
            // A refusal is about a draft, so editing the draft retires it.
            setRefusal(null);
          }}
          onKeyDown={onKeyDown}
          onPaste={(event) => {
            // Ask rather than assume: `passthrough` means the browser's own
            // behaviour is left in place, which is the honest answer where keeper
            // has no model to ask about an image.
            const decision = botPasteDecision(event.clipboardData, pasteContext);
            if (decision.kind !== "passthrough") {
              event.preventDefault();
              onPaste?.(decision);
            }
          }}
        />
        {accessory}
        {streaming ? (
          <Button type="button" variant="outline" size="sm" onClick={onStop}>
            {BOT_COMPOSER_STOP_LABEL}
          </Button>
        ) : (
          <Button
            type="button"
            size="sm"
            disabled={disabled || draft.trim().length === 0}
            onClick={send}
          >
            {BOT_COMPOSER_SEND_LABEL}
          </Button>
        )}
      </div>
    </div>
  );
}

/**
 * What the composer says when it could not find out what a draft was.
 *
 * Not "try again": the honest fact is that keeper could not read its own
 * command list, and sending unresolved text to a model is the one thing this
 * component refuses to do.
 */
export const BOT_COMPOSER_UNRESOLVED =
  "keeper couldn't tell whether that was a command, so nothing was sent.";
