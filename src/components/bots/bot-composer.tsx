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
import { type KeyboardEvent, type ReactNode, useCallback, useEffect, useState } from "react";
import {
  type BotPasteContext,
  type BotPasteDecision,
  botPasteDecision,
} from "@/components/bots/bot-paste";
import {
  acceptedDraft,
  argumentHint,
  type BotCommandContext,
  type BotCommandInvocation,
  isCommandLine,
  menuKeyAction,
  nextIndex,
  slashToken,
} from "@/components/bots/bot-slash-menu";
import { Button } from "@/components/ui/button";
import type { BotCommandPreviewVm, BotCommandRowVm } from "@/lib/ipc/client";
import { botsCommandPreview } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** The textarea's accessible name and its placeholder. */
export const BOT_COMPOSER_LABEL = "Message";
export const BOT_COMPOSER_PLACEHOLDER = "Ask this bot, or type / for a command";

/** The send verb. */
export const BOT_COMPOSER_SEND_LABEL = "Send";

/** The stop verb, shown in place of Send while an answer is arriving. */
export const BOT_COMPOSER_STOP_LABEL = "Stop";

/** What the composer says while it has no bot to ask. */
export const BOT_COMPOSER_NO_BOT = "Choose a bot above and this will send to it.";

/** The command menu's accessible name. */
export const BOT_COMPOSER_MENU_LABEL = "Commands";

/** The help panel's accessible name, and its close verb. */
export const BOT_COMPOSER_HELP_LABEL = "Commands keeper knows";
export const BOT_COMPOSER_HELP_CLOSE = "Close";

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
  heard = null,
  accessory,
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
   * What talk mode heard (Story 62.6, FR-407): lands in the field as the
   * draft, where it can be read and edited before it goes, and is sent only
   * by the same Enter or Send as typed text. `seq` distinguishes hearing the
   * same words twice from nothing new: each turn bumps it. A transcriber
   * that sent what it mis-heard would be worse than one that asks, so this
   * component never sends on its own account.
   */
  heard?: { text: string; seq: number } | null;
  /**
   * A control that shares the field's row — the talk-mode microphone on a
   * phone. Rendered between the field and the verb, absent where undefined.
   */
  accessory?: ReactNode;
}) {
  const [draft, setDraft] = useState("");
  const [preview, setPreview] = useState<BotCommandPreviewVm | null>(null);
  const [highlight, setHighlight] = useState(0);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  // The whole answer to `/help`, held together: the panel outlives the draft
  // that opened it, so it cannot read the escape hint off a preview the
  // cleared field has already discarded.
  const [help, setHelp] = useState<{
    rows: readonly BotCommandRowVm[];
    escapeHint: string;
  } | null>(null);

  const { providerKind, hasProvider, hasBot, hasSession, modelTools } = commandContext;
  const resolve = useCallback(
    async (text: string) =>
      await resolveDraft(text, { providerKind, hasProvider, hasBot, hasSession, modelTools }),
    [resolveDraft, providerKind, hasProvider, hasBot, hasSession, modelTools],
  );

  // Asked only while the draft could be a command, so a keystroke of prose
  // costs nothing. The echoed `draft` is compared before anything is stored:
  // replies can land out of order, and a menu drawn for text the field no
  // longer holds is worse than no menu.
  useEffect(() => {
    if (!isCommandLine(draft)) {
      setPreview(null);
      return;
    }
    let live = true;
    void resolve(draft)
      .then((answer) => {
        if (live && answer.draft === draft) {
          setPreview(answer);
          setHighlight(0);
        }
      })
      // A preview that failed leaves the menu closed. The draft is still
      // sendable — the submit path asks again and reports what happens there,
      // where a person is waiting for an answer rather than typing.
      .catch(() => {
        if (live) {
          setPreview(null);
        }
      });
    return () => {
      live = false;
    };
  }, [draft, resolve]);

  // A heard transcript replaces the draft rather than appending: the field
  // is where it is checked, and a half-typed line under it would be two
  // messages nobody meant. Editing after that is ordinary typing.
  useEffect(() => {
    if (heard !== null) {
      setDraft(heard.text);
      setRefusal(null);
    }
  }, [heard]);

  const token = slashToken(draft);
  const rows = preview === null ? [] : preview.rows;
  const menuOpen = rows.length > 0 && token !== null && token !== dismissed;
  const active = rows[Math.min(highlight, rows.length - 1)];

  const submit = (text: string) => {
    void resolve(text)
      .then((answer) => {
        switch (answer.verdict.kind) {
          case "prose": {
            // The one path that reaches a model, and the escape arrives here
            // already unwrapped: `//etc` is the text `/etc`.
            if (disabled || streaming) {
              return;
            }
            setDraft("");
            setRefusal(null);
            onSend(answer.verdict.text);
            return;
          }
          case "command": {
            setDraft("");
            setRefusal(null);
            if (answer.verdict.name === "help") {
              void resolve(HELP_DRAFT)
                .then((all) => setHelp({ rows: all.rows, escapeHint: all.escapeHint }))
                // The narrowed answer is still the truth about this token, and
                // a panel of one row beats a panel that never opened.
                .catch(() => setHelp({ rows, escapeHint: answer.escapeHint }));
              return;
            }
            setRefusal(onCommand({ name: answer.verdict.name, args: answer.verdict.args }));
            return;
          }
          case "refusal": {
            // The draft is left in the field on purpose: it is one keystroke
            // from being right, and clearing it would make a typo cost the
            // whole sentence.
            setRefusal(answer.verdict.message);
            return;
          }
        }
      })
      .catch(() => {
        // Resolution is a pure, local call; a rejection means the shell is
        // gone, and sending text nobody resolved is the one thing this
        // component must not do.
        setRefusal(BOT_COMPOSER_UNRESOLVED);
      });
  };

  const send = () => {
    const text = draft.trim();
    if (text.length === 0 || streaming) {
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

  const accept = (row: BotCommandRowVm) => {
    const completed = acceptedDraft(row);
    // Completing first and running on the second press is Claude Code's model
    // (R5 §2) and the one that works for a command with an argument: accepting
    // `/bot` must leave the caret where the name goes, not run a command with
    // nothing after it.
    if (draft !== completed) {
      setDraft(completed);
      setRefusal(null);
      return;
    }
    send();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // A composing IME delivers Enter as a candidate selection; acting on it
    // would lose the composition.
    if (event.nativeEvent.isComposing) {
      return;
    }
    const action = menuKeyAction(event.key, menuOpen, {
      shiftKey: event.shiftKey,
      metaKey: event.metaKey,
      ctrlKey: event.ctrlKey,
      altKey: event.altKey,
    });
    if (action !== null) {
      event.preventDefault();
      if (action === "dismiss") {
        // Dismissed for this token only: Escape means "not this one", never
        // "never again".
        setDismissed(token);
        return;
      }
      if (action === "accept") {
        if (active !== undefined) {
          accept(active);
        }
        return;
      }
      setHighlight(nextIndex(highlight, rows.length, action));
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
        <section
          aria-label={BOT_COMPOSER_HELP_LABEL}
          className="flex flex-col gap-2 rounded-md border border-border p-3"
        >
          <ul className="flex flex-col gap-1 text-sm">
            {help.rows.map((row) => (
              <li key={row.name} className="flex min-w-0 gap-2">
                <span className="font-mono">
                  /{row.name}
                  {argumentHint(row) === null ? "" : ` ${argumentHint(row)}`}
                </span>
                <span className="min-w-0 flex-1 text-muted-foreground">{row.description}</span>
              </li>
            ))}
          </ul>
          <p className="text-muted-foreground text-xs">{help.escapeHint}</p>
          <div className="flex">
            <Button type="button" variant="outline" size="sm" onClick={() => setHelp(null)}>
              {BOT_COMPOSER_HELP_CLOSE}
            </Button>
          </div>
        </section>
      )}

      {menuOpen && (
        <ul aria-label={BOT_COMPOSER_MENU_LABEL} className="flex flex-col gap-1 text-sm">
          {rows.map((row, index) => (
            <li
              key={row.name}
              aria-current={row === active}
              className={cn(
                "flex min-w-0 flex-col rounded px-2 py-1",
                index === Math.min(highlight, rows.length - 1) && "bg-accent",
              )}
            >
              <span className="flex min-w-0 gap-2">
                <span className="font-mono">
                  /{row.name}
                  {argumentHint(row) === null ? "" : ` ${argumentHint(row)}`}
                </span>
                <span className="min-w-0 flex-1 text-muted-foreground">{row.description}</span>
              </span>
              {/* A reason says why Enter will not run it; a warning says what to
                  expect when Enter does. Both are the registry's words, printed
                  where the decision is made rather than restated here. */}
              {row.reason !== null && (
                <span className="text-muted-foreground text-xs">{row.reason}</span>
              )}
              {row.warning !== null && (
                <span className="text-muted-foreground text-xs">{row.warning}</span>
              )}
            </li>
          ))}
        </ul>
      )}

      {menuOpen && preview !== null && preview.note !== null && (
        <p className="text-muted-foreground text-xs">{preview.note}</p>
      )}

      {/* Taught where it is relevant: the moment a leading slash is on screen
          is the moment somebody may have meant a path. */}
      {menuOpen && preview !== null && (
        <p className="text-muted-foreground text-xs">{preview.escapeHint}</p>
      )}

      {refusal !== null && (
        <p role="alert" className="text-destructive text-sm">
          {refusal}
        </p>
      )}

      {disabled && <p className="text-muted-foreground text-xs">{BOT_COMPOSER_NO_BOT}</p>}

      {/* The field and its verb share one row (Story 61.14): stacked, the
          verb's row was a 40px band of its own under a three-line field, and
          every band here is height the transcript above does not get. */}
      <div className="flex items-end gap-2">
        <textarea
          aria-label={BOT_COMPOSER_LABEL}
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
