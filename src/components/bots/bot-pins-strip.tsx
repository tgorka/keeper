/**
 * The pinned bots, in hand order, each wearing its identity (Story 61.7,
 * FR-383).
 *
 * **The interaction is `layout/pins-strip.tsx`'s, deliberately copied rather
 * than reinvented**, down to the hook and the arithmetic: a press that travels
 * past the slop becomes a drag, the strip previews the reorder while the
 * pointer is down, and the release dispatches the **whole new order**, whose
 * authoritative answer is what the store then holds. HTML5 drag is not an
 * option in this app and the reason is written down in
 * {@link "@/hooks/use-pointer-drag"}: Tauri's wry handler claims
 * `performDragOperation:` in Rust before WebKit can perform it, so the page's
 * `drop` never fires and a strip built on it looks live while being dead.
 *
 * **And the gesture is not the only path, which is where this strip goes
 * further than the one it copies.** The chat pins strip offers `Move up` /
 * `Move down` on the phone only, and its own spec records the gap it left:
 * *"no keyboard-accessible reorder alternative to native HTML5 drag"*. The bots
 * pane is desktop-only, so a phone-gated alternative would be no alternative at
 * all — here `Alt` with an arrow moves the focused bot one place, and the same
 * two moves are in the context menu on every tier. UX-DR28 forbids a gesture
 * being the sole path, and a pointer drag is a gesture.
 *
 * **The strip is bounded and says so.** Nine bots in a horizontally scrolling
 * strip are nine bots you cannot see, silently: this one draws at most
 * {@link BOT_PINS_VISIBLE} and prints how many it is not drawing. The bot
 * currently being talked to is never one of the hidden ones — a strip that
 * hides the open conversation's own bot is worse than a strip that is full.
 *
 * **Identity is shape and mark first, colour second.** The cell comes from
 * {@link "@/components/bots/bot-identity"}, which paints no ink at all without
 * a shape (`DESIGN.md:172`), and every button's accessible name carries the
 * identity in words — so two bots that differ only in their ink are two
 * different bots to a screen reader and to anybody who cannot tell the two inks
 * apart.
 */
import { useRef, useState } from "react";
import { BotIdentityCell, BotIdentityPicker, botPinLabel } from "@/components/bots/bot-identity";
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
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { usePointerDrag } from "@/hooks/use-pointer-drag";
import type { BotVm } from "@/lib/ipc/client";
import { botsBotRemove, botsBotsList, botsBotsReorder } from "@/lib/ipc/client";
import { botsStore } from "@/lib/stores/bots";
import { cn } from "@/lib/utils";

/** The strip's accessible name. */
export const BOT_PINS_LABEL = "Pinned bots";

/**
 * How many bots the strip draws before it starts counting instead.
 *
 * Eight, which is two more than the widest pane at the narrowest supported
 * window fits comfortably and still few enough that the row reads as a hand of
 * cards rather than a list. Past this the strip does not scroll: a scrolling
 * strip hides things without saying it has.
 */
export const BOT_PINS_VISIBLE = 8;

/** Menu items, exported so the tests name them once. */
export const BOT_PINS_MOVE_EARLIER = "Move earlier";
export const BOT_PINS_MOVE_LATER = "Move later";
export const BOT_PINS_EDIT_IDENTITY = "Change identity";
export const BOT_PINS_UNPIN = "Unpin";

/** The confirmation, which names what happens to which object. */
export const botPinsUnpinTitle = (name: string) => `Unpin ${name}?`;
export const BOT_PINS_UNPIN_BODY =
  "keeper forgets the bot and the token you gave it. Your conversations with it stay, and you can pin it again by name.";

/** What the strip says about the bots it is not drawing. */
export const botPinsOverflowNote = (hidden: number) =>
  hidden === 1
    ? "1 more pinned bot than fits here — the picker below lists every one."
    : `${hidden} more pinned bots than fit here — the picker below lists every one.`;

/**
 * The bots the strip draws, and how many it is not drawing.
 *
 * Pure, and separately tested, because the interesting case is the one nobody
 * looks at: an overflowing strip whose *selected* bot is past the bound. The
 * hand order is what the strip is for, so the window is the first N of it — and
 * the one exception is the bot being talked to, which takes the last slot when
 * it would otherwise be invisible. Reordering stays sound either way: the
 * reorder is computed over the full list, never over this window.
 */
export function botPinsWindow(
  bots: BotVm[],
  selectedBotId: string | null,
  limit: number = BOT_PINS_VISIBLE,
): { shown: BotVm[]; hidden: number } {
  if (bots.length <= limit) {
    return { shown: bots, hidden: 0 };
  }
  const shown = bots.slice(0, limit);
  const selected = bots.find((bot) => bot.id === selectedBotId) ?? null;
  if (selected !== null && !shown.includes(selected)) {
    shown[limit - 1] = selected;
  }
  return { shown, hidden: bots.length - limit };
}

/** Move one element of `bots` from `from` to `to`, returning the new array. */
function movePin(bots: BotVm[], from: number, to: number): BotVm[] {
  const next = [...bots];
  const [moved] = next.splice(from, 1);
  if (moved === undefined) {
    return bots;
  }
  next.splice(to, 0, moved);
  return next;
}

/**
 * The press a reorder begins with: the bot's index in the authoritative order.
 *
 * Desktop only, so there is no `viaLift` here — the bots pane does not exist on
 * the phone tier (`capabilities.bots` is `cfg!(desktop)`), and a long-press
 * branch nothing can reach would be an affordance that lies.
 */
interface BotPinPress {
  index: number;
}

export function BotPinsStrip({
  bots,
  selectedBotId,
  onSelect,
}: {
  bots: BotVm[];
  selectedBotId: string | null;
  onSelect: (botId: string) => void;
}) {
  const [editing, setEditing] = useState<BotVm | null>(null);
  const [unpinning, setUnpinning] = useState<BotVm | null>(null);
  // The row the slot hit-test measures.
  const rowRef = useRef<HTMLElement>(null);

  /**
   * The full order, rewritten and persisted. The answer is the authoritative
   * list, so it goes straight into the mirror rather than being merged with
   * anything computed here: Rust renumbered the rows and one of them may have
   * changed for another reason since this render.
   */
  const persistOrder = (next: BotVm[]) => {
    void botsBotsReorder(next.map((bot) => bot.id))
      .then((rows) => botsStore.getState().applyBots(rows))
      .catch(() => {});
  };

  const moveBy = (index: number, delta: number) => {
    const target = index + delta;
    if (index < 0 || target < 0 || target >= bots.length) {
      return;
    }
    persistOrder(movePin(bots, index, target));
  };

  /**
   * The slot a release at this x lands in: the nearest drawn cell's midpoint.
   *
   * Measured per call rather than cached at the press, so the preview the drag
   * itself paints resolves where the cells are now instead of where they were.
   * The answer is an index into the DRAWN row, which the release maps back onto
   * the full order through the bot's id.
   */
  const slotAt = (clientX: number): number | null => {
    // Scoped to this strip's own row rather than the document: the query is a
    // hit-test over siblings, and a second strip anywhere on screen would
    // otherwise contribute slots that belong to another list.
    const items = rowRef.current?.querySelectorAll<HTMLElement>("[data-bot-pin-slot]");
    if (items === undefined || items.length === 0) {
      return null;
    }
    let nearest = 0;
    let nearestDist = Number.POSITIVE_INFINITY;
    items.forEach((item, index) => {
      const rect = item.getBoundingClientRect();
      const dist = Math.abs(clientX - (rect.left + rect.width / 2));
      if (dist < nearestDist) {
        nearest = index;
        nearestDist = dist;
      }
    });
    return nearest;
  };

  const visible = botPinsWindow(bots, selectedBotId);

  const drag = usePointerDrag<BotPinPress, number>({
    slopPx: 10,
    resolve: slotAt,
    onRelease: (press, slot, moved) => {
      if (!moved || slot === null) {
        return;
      }
      // The slot is an index into the DRAWN row; the order submitted is over
      // the full list, so it is mapped back through the landing bot's id. Both
      // ends are re-checked against the current list: a refresh landing
      // mid-drag can shrink it, and a stale index would splice the wrong bot —
      // or none, and then submit an order missing a row, which Rust refuses
      // whole rather than half-applying.
      const landing = visible.shown[slot];
      if (landing === undefined || press.index < 0 || press.index >= bots.length) {
        return;
      }
      const to = bots.findIndex((bot) => bot.id === landing.id);
      if (to < 0 || to === press.index) {
        return;
      }
      persistOrder(movePin(bots, press.index, to));
    },
  });

  // Hidden entirely when there is nothing pinned: an empty strip is a band of
  // chrome advertising nothing (UX-DR4).
  if (bots.length === 0) {
    return null;
  }

  // While a bot drags, preview the reordered row; the authoritative order
  // arrives from the reorder's own answer after the release. With no HTML5
  // ghost to lean on, this preview IS the drop cue — `pins-strip.tsx`'s note
  // on why the pressed cell does not ALSO translate under the pointer applies
  // here unchanged: the DOM has already moved it into its target slot.
  const pressed = drag.item;
  const pressedId = pressed === null ? null : (bots[pressed.index]?.id ?? null);
  const pressedSlot =
    pressedId === null ? -1 : visible.shown.findIndex((bot) => bot.id === pressedId);
  const preview =
    drag.dragging && drag.over !== null && pressedSlot >= 0 && drag.over !== pressedSlot
      ? drag.over
      : null;
  const displayed = preview === null ? visible.shown : movePin(visible.shown, pressedSlot, preview);
  return (
    <div className="shrink-0 border-border border-b">
      <nav
        ref={rowRef}
        aria-label={BOT_PINS_LABEL}
        // The move, the release and the cancel are listened for here and
        // nowhere else, `pins-strip.tsx`'s reason verbatim: before the slop
        // crossing there is no capture, and a press near the edge of a cell
        // leaves it well before travelling the 10 px tolerance, so that move
        // has to land on an ancestor or the drag silently never starts.
        onPointerMove={drag.handlers.onPointerMove}
        onPointerUp={drag.handlers.onPointerUp}
        onPointerCancel={drag.handlers.onPointerCancel}
        className="flex flex-nowrap items-center gap-1 px-6 py-2"
      >
        {displayed.map((bot) => {
          const index = bots.findIndex((row) => row.id === bot.id);
          const selected = bot.id === selectedBotId;
          return (
            <ContextMenu key={bot.id}>
              <ContextMenuTrigger asChild>
                <button
                  type="button"
                  data-bot-pin-slot={bot.id}
                  aria-label={botPinLabel(bot)}
                  aria-current={selected ? "true" : undefined}
                  data-dragging={
                    pressed !== null && drag.dragging && pressed.index === index
                      ? "true"
                      : undefined
                  }
                  onClick={() => onSelect(bot.id)}
                  onPointerDown={(event) => {
                    drag.allowNextClick();
                    if (event.button !== 0) {
                      return;
                    }
                    // The press's own default is a selection anchor: a cancelled
                    // `pointerdown` fires no `mousedown`, so the moves that
                    // follow cannot extend a selection across the app
                    // (Story 54.1). `click` is not cancelled by it, so a press
                    // that does not travel still selects the bot.
                    event.preventDefault();
                    drag.begin(
                      { index },
                      {
                        pointerId: event.pointerId,
                        clientX: event.clientX,
                        clientY: event.clientY,
                        target: event.currentTarget,
                      },
                    );
                  }}
                  onClickCapture={drag.handlers.onClickCapture}
                  onKeyDown={(event) => {
                    // The non-gesture reorder. `Alt` because the bare arrows
                    // belong to the strip's own focus movement and `⌘`/`Ctrl`
                    // with an arrow is a text-navigation chord on both
                    // platforms.
                    if (!event.altKey) {
                      return;
                    }
                    if (event.key === "ArrowLeft") {
                      event.preventDefault();
                      moveBy(index, -1);
                      return;
                    }
                    if (event.key === "ArrowRight") {
                      event.preventDefault();
                      moveBy(index, 1);
                    }
                  }}
                  className={cn(
                    "flex shrink-0 items-center gap-2 rounded-md px-2 py-1 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring data-[dragging=true]:opacity-50",
                    selected
                      ? "bg-secondary text-secondary-foreground"
                      : "text-muted-foreground hover:bg-accent",
                  )}
                >
                  <BotIdentityCell identity={bot} />
                  <span className="truncate">{bot.name}</span>
                </button>
              </ContextMenuTrigger>
              <ContextMenuContent>
                <ContextMenuItem onSelect={() => setEditing(bot)}>
                  {BOT_PINS_EDIT_IDENTITY}
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem disabled={index <= 0} onSelect={() => moveBy(index, -1)}>
                  {BOT_PINS_MOVE_EARLIER}
                </ContextMenuItem>
                <ContextMenuItem
                  disabled={index < 0 || index >= bots.length - 1}
                  onSelect={() => moveBy(index, 1)}
                >
                  {BOT_PINS_MOVE_LATER}
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem onSelect={() => setUnpinning(bot)}>
                  {BOT_PINS_UNPIN}
                </ContextMenuItem>
              </ContextMenuContent>
            </ContextMenu>
          );
        })}
      </nav>

      {visible.hidden > 0 && (
        <p className="px-6 pb-2 text-muted-foreground text-meta">
          {botPinsOverflowNote(visible.hidden)}
        </p>
      )}

      {editing !== null && (
        <BotIdentityPicker
          bot={editing}
          open={true}
          onOpenChange={(open) => {
            if (!open) {
              setEditing(null);
            }
          }}
          onSaved={() => {
            // The row that came back is authoritative for that bot, but the
            // mirror holds a list: re-read it rather than splicing, so the
            // order and every other row stay Rust's answer.
            void botsBotsList()
              .then((rows) => botsStore.getState().applyBots(rows))
              .catch(() => {});
          }}
        />
      )}

      <AlertDialog
        open={unpinning !== null}
        onOpenChange={(open) => {
          if (!open) {
            setUnpinning(null);
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{botPinsUnpinTitle(unpinning?.name ?? "")}</AlertDialogTitle>
            <AlertDialogDescription>{BOT_PINS_UNPIN_BODY}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep it</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                const bot = unpinning;
                setUnpinning(null);
                if (bot === null) {
                  return;
                }
                void botsBotRemove(bot.id)
                  .then(() => botsBotsList())
                  .then((rows) => botsStore.getState().applyBots(rows))
                  .catch(() => {});
              }}
            >
              {BOT_PINS_UNPIN}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
