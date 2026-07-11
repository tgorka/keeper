/**
 * Long-press → context-menu bridge (Story 13.6, touch idioms).
 *
 * Phone-gated pointer handlers that, after a ≥500ms stationary press, dispatch
 * a synthetic `contextmenu` event at the press point on the pressed element.
 * Radix `ContextMenuTrigger`s listen for exactly that event, so spreading these
 * handlers on (or inside) a trigger opens the *identical* menu by touch — no
 * forked menus, no second visual language. Movement past the tolerance, an
 * early lift, a pointer cancel (native scroll taking over), or a second
 * pointer all cancel the press, so a normal tap/scroll never opens a menu.
 * The click that follows a fired long-press is suppressed (capture phase) so
 * lifting the finger never also activates the control under the opened menu.
 *
 * Off the phone tier (`useShellLayout().phone === false`) every handler is a
 * no-op, and mouse pointers are ignored on any tier — right-click already
 * opens context menus natively. One hook instance may be shared across many
 * targets (e.g. every row of a list): the pressed element is captured per
 * press and only one press is tracked at a time.
 */
import * as React from "react";
import { useShellLayout } from "@/hooks/use-shell-layout";

/** Hold duration (ms) before the long-press fires. */
export const LONG_PRESS_MS = 500;
/** Movement past this distance (px) cancels the press (it is a scroll/swipe). */
export const LONG_PRESS_MOVE_TOLERANCE_PX = 10;

/** The press detail handed to a custom `onLongPress` (the Pins drag lift). */
export interface LongPressDetail {
  /** The element the press started on (the handlers' `currentTarget`). */
  target: HTMLElement;
  clientX: number;
  clientY: number;
  pointerId: number;
}

export interface UseLongPressOptions {
  /**
   * When provided, called at the hold threshold INSTEAD of dispatching the
   * synthetic `contextmenu`. The Pins strip uses this to lift a pin into
   * drag-reorder mode and defer the menu decision to release; everyone else
   * omits it and gets the default menu-opening dispatch.
   */
  onLongPress?: (detail: LongPressDetail) => void;
}

export interface LongPressHandlers {
  onPointerDown: (e: React.PointerEvent<HTMLElement>) => void;
  onPointerMove: (e: React.PointerEvent<HTMLElement>) => void;
  onPointerUp: (e: React.PointerEvent<HTMLElement>) => void;
  onPointerCancel: (e: React.PointerEvent<HTMLElement>) => void;
  onClickCapture: (e: React.MouseEvent<HTMLElement>) => void;
}

/**
 * The one in-flight press. Movement/lift/cancel clear it; the fire path clears
 * it too (nothing left to clean up once the menu opened).
 */
interface PressState {
  pointerId: number;
  target: HTMLElement;
  startX: number;
  startY: number;
  timer: number;
}

export function useLongPress(options: UseLongPressOptions = {}): LongPressHandlers {
  const { phone } = useShellLayout();
  // Refs keep the returned handler object referentially stable across renders
  // while always reading the current tier gate and options.
  const phoneRef = React.useRef(phone);
  phoneRef.current = phone;
  const optionsRef = React.useRef(options);
  optionsRef.current = options;

  const pressRef = React.useRef<PressState | null>(null);
  // Set when a long-press fired: the very next click through the target is
  // swallowed so the lift never also taps the row under the opened menu.
  const firedRef = React.useRef(false);

  return React.useMemo(() => {
    const cancel = () => {
      const press = pressRef.current;
      if (press !== null) {
        window.clearTimeout(press.timer);
        pressRef.current = null;
      }
    };

    return {
      onPointerDown: (e: React.PointerEvent<HTMLElement>) => {
        // Off-phone no-op; mouse presses are ignored (native right-click owns
        // the menu there). A second concurrent pointer cancels the press —
        // a pinch/scroll is never a long-press.
        if (!phoneRef.current || e.pointerType === "mouse") {
          return;
        }
        if (pressRef.current !== null) {
          cancel();
          return;
        }
        firedRef.current = false;
        const target = e.currentTarget;
        const { pointerId, clientX, clientY } = e;
        const timer = window.setTimeout(() => {
          const press = pressRef.current;
          if (press === null) {
            return;
          }
          pressRef.current = null;
          firedRef.current = true;
          const custom = optionsRef.current.onLongPress;
          if (custom !== undefined) {
            custom({
              target: press.target,
              clientX: press.startX,
              clientY: press.startY,
              pointerId: press.pointerId,
            });
            return;
          }
          // The bridge itself: Radix ContextMenuTrigger opens on exactly this
          // event, at exactly this point.
          press.target.dispatchEvent(
            new MouseEvent("contextmenu", {
              bubbles: true,
              cancelable: true,
              clientX: press.startX,
              clientY: press.startY,
            }),
          );
        }, LONG_PRESS_MS);
        pressRef.current = { pointerId, target, startX: clientX, startY: clientY, timer };
      },
      onPointerMove: (e: React.PointerEvent<HTMLElement>) => {
        const press = pressRef.current;
        if (press === null || e.pointerId !== press.pointerId) {
          return;
        }
        const dist = Math.hypot(e.clientX - press.startX, e.clientY - press.startY);
        if (dist > LONG_PRESS_MOVE_TOLERANCE_PX) {
          cancel();
        }
      },
      onPointerUp: (e: React.PointerEvent<HTMLElement>) => {
        const press = pressRef.current;
        if (press !== null && e.pointerId === press.pointerId) {
          cancel();
        }
      },
      onPointerCancel: (e: React.PointerEvent<HTMLElement>) => {
        const press = pressRef.current;
        if (press !== null && e.pointerId === press.pointerId) {
          cancel();
        }
      },
      onClickCapture: (e: React.MouseEvent<HTMLElement>) => {
        if (!firedRef.current) {
          return;
        }
        firedRef.current = false;
        e.preventDefault();
        e.stopPropagation();
      },
    };
  }, []);
}
