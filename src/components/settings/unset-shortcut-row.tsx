/**
 * A Settings → Shortcuts row for an OS-global binding that ships **unset**
 * (Story 20.4's recording chord; Story 63.5's voice chord).
 *
 * One component, two rows, for AD-C7's reason: the capture control, the
 * "Not set" state, the Clear verb and the honest not-registered sentence are
 * the same contract for every unset-by-default chord, and two copies would
 * be two chances to word or gate them differently — with the wrong one being
 * the one nobody is looking at. The row renders the {@link HotkeyVm} as-is:
 * conflict and registration state come from Rust, never derived here.
 *
 * Assigning captures the next chord ({@link acceleratorFromEvent}, so a bare
 * single key can never bind — UX-DR29) and hands it to `set`; Clear hands to
 * `clear`; both adopt the VM the write resolved with, or surface the error
 * without losing the previous binding (the Rust command restored it).
 */
import type React from "react";
import { useEffect, useRef, useState } from "react";
import { FileControlled } from "@/components/settings/config-source-section";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Label } from "@/components/ui/label";
import { acceleratorFromEvent, formatAccelerator } from "@/lib/hotkey";
import type { HotkeyVm } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The unset-state label: the binding ships unset — no chord is registered
 * until the user assigns one. */
export const HOTKEY_NOT_SET_LABEL = "Not set";

export function UnsetShortcutRow({
  open,
  label,
  noun,
  settingKey,
  read,
  set,
  clear,
  notRegistered,
}: {
  /** The section's hydration signal: the binding is read when this flips true. */
  open: boolean;
  /** The row's label, e.g. "Start / stop recording". */
  label: string;
  /** The binding's one-word name for the verbs' accessible names:
   * "Change recording shortcut", "Clear voice shortcut". */
  noun: string;
  /** The settings key the file-controlled marker names, e.g. `hotkey.recording`. */
  settingKey: string;
  read: () => Promise<HotkeyVm>;
  set: (accelerator: string) => Promise<HotkeyVm>;
  clear: () => Promise<HotkeyVm>;
  /** The honest copy when an ASSIGNED chord is not registered with the OS
   * (`active === false` while a chord is set). Never shown for the unset
   * default — nothing is supposed to be registered then. */
  notRegistered: string;
}) {
  // `undefined` = still loading; otherwise the resolved binding VM.
  const [hotkey, setHotkey] = useState<HotkeyVm | undefined>(undefined);
  // Whether the capture control is armed and listening for the next chord.
  const [capturing, setCapturing] = useState(false);
  // The last reassignment/clear error (OS refused / persist failed), or `null`.
  const [error, setError] = useState<string | null>(null);
  const writeId = useRef(0);
  const readRef = useRef(read);
  readRef.current = read;

  useEffect(() => {
    if (!open) {
      return;
    }
    setHotkey(undefined);
    setCapturing(false);
    setError(null);
    let cancelled = false;
    void readRef
      .current()
      .then((vm) => {
        if (!cancelled) {
          setHotkey(vm);
        }
      })
      .catch(() => {
        // On a read failure leave the row in its loading state rather than
        // asserting a (possibly wrong) binding.
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  const applyWrite = (write: Promise<HotkeyVm>) => {
    writeId.current += 1;
    const id = writeId.current;
    setError(null);
    void write
      .then((vm) => {
        if (id === writeId.current) {
          setHotkey(vm);
        }
      })
      .catch((raw: unknown) => {
        if (id !== writeId.current) {
          return;
        }
        setError(syncErrorMessage(raw, "Could not set that shortcut."));
      });
  };

  // While capturing, translate the next complete chord into an accelerator and
  // assign it. A bare modifier or modifier-less key yields `null` and keeps
  // capturing (no single-key verb can ever bind); Escape cancels.
  const onCaptureKeyDown = (event: React.KeyboardEvent) => {
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      setCapturing(false);
      return;
    }
    const accelerator = acceleratorFromEvent(event.nativeEvent);
    if (accelerator === null) {
      return;
    }
    setCapturing(false);
    applyWrite(set(accelerator));
  };

  const unset = hotkey !== undefined && hotkey.accelerator === "";

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <Label>{label}</Label>
        <div className="flex items-center gap-2">
          <FileControlled settingKey={settingKey} />
          {capturing ? (
            <button
              type="button"
              // biome-ignore lint/a11y/noAutofocus: capture is an explicit user action; the field must receive the next keystroke immediately.
              autoFocus
              aria-label={`Press a shortcut for ${label} (Esc to cancel)`}
              onKeyDown={onCaptureKeyDown}
              onBlur={() => setCapturing(false)}
              className="rounded-sm border border-ring px-2 py-0.5 text-muted-foreground text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              Press a shortcut… (Esc to cancel)
            </button>
          ) : unset ? (
            <span className="text-muted-foreground text-xs">{HOTKEY_NOT_SET_LABEL}</span>
          ) : (
            <Kbd aria-label={hotkey === undefined ? "Loading shortcut" : hotkey.accelerator}>
              {hotkey === undefined ? "…" : formatAccelerator(hotkey.accelerator)}
            </Kbd>
          )}
          <Button
            type="button"
            variant="outline"
            size="xs"
            aria-label={`Change ${noun} shortcut`}
            disabled={hotkey === undefined || capturing}
            onClick={() => {
              setError(null);
              setCapturing(true);
            }}
          >
            Change…
          </Button>
          <Button
            type="button"
            variant="outline"
            size="xs"
            aria-label={`Clear ${noun} shortcut`}
            disabled={hotkey === undefined || unset || capturing}
            onClick={() => applyWrite(clear())}
          >
            Clear
          </Button>
        </div>
      </div>
      {hotkey?.conflict != null && (
        <p className="text-held text-xs" role="status">
          {hotkey.conflict}
        </p>
      )}
      {hotkey !== undefined && !unset && !hotkey.active && (
        <p className="text-held text-xs" role="status">
          {notRegistered}
        </p>
      )}
      {error !== null && (
        <p className="text-held text-xs" role="alert">
          {error}
        </p>
      )}
    </>
  );
}
