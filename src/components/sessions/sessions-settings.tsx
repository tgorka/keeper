/**
 * Settings → Sessions: what a space you have never touched does on arrival
 * (Story 49.3, FR-276).
 *
 * **Why a preference at all, when the fold is already remembered.** Folding one
 * space is an answer about that space; this is the answer for every space
 * nobody has given one about. The owner asked for both — *"moga byc foldable
 * (ustaw w opcjach ze maja byc folder/unfolder by default)"* — and shipping
 * only the mechanism is what `favorites_collapsed` did: a `keeper.toml` key with
 * no control anywhere, which is a setting for people who read the source.
 *
 * **It changes only what nobody has decided.** Flipping this writes
 * `sessions.spaces_folded` and moves the store's fallback; a space the person
 * folded or unfolded by hand keeps their answer, because that answer lives in
 * its own cookie and is consulted first
 * ({@link "@/lib/stores/session-spaces-fold"}). A switch that shut every space
 * including the ones somebody had just opened would be the app overruling them.
 *
 * **Nothing renders without a sessions root.** A fold default is a fact about
 * spaces, spaces live in a zone, and a zone is a synced folder somebody flagged
 * — with none flagged there is nothing here to set, so the section removes
 * itself rather than offering a control that configures nothing. That is
 * `CaptureSettingsSection`'s rule for a vault, and the Recording and Sync
 * sections' rule for a capability they lack.
 *
 * **Load on open, set optimistically, revert on failure** — the
 * `menu_bar_presence` idiom (`settings-dialog.tsx`), with two departures this
 * setting earns. `undefined` while the read is out, so the switch is disabled
 * rather than claiming a state keeper has not looked up yet, and a rejected
 * write puts the switch back where it was rather than leaving it showing
 * something that was never saved.
 *
 * The departures: a FAILED READ reports the fold default the store is already
 * handing the spaces on screen instead of a hard-coded `false`, because unlike
 * a menu-bar preference this one has a live mirror in this document that a
 * detail may already have seeded — and the WRITES are serialised rather than
 * merely guarded, because two `invoke`s in flight are unordered and nothing
 * re-reads afterwards, so an out-of-order pair would leave Rust holding the
 * value the person turned off with no surface saying so.
 */
import { useEffect, useRef, useState } from "react";
import { FileControlled } from "@/components/settings/config-source-section";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { sessionsSpacesFoldedGet, sessionsSpacesFoldedSet } from "@/lib/ipc/client";
import { sessionSpacesFoldStore, setSpacesFoldedDefault } from "@/lib/stores/session-spaces-fold";
import { refreshSessionsRoots, useSessionsRootsStore } from "@/lib/stores/sessions-roots";

/** The section heading, so the dialog and its test cannot disagree about it. */
export const SESSIONS_SECTION_TITLE = "Sessions";

/** The switch's label — what it does, in the vocabulary of the surface it moves. */
export const SESSIONS_SPACES_FOLDED_LABEL = "Start spaces folded";

/**
 * The standing explanation, shown whichever way the switch is set.
 *
 * It names both exceptions, because the exceptions are the whole design:
 * somebody who turns this on and finds three spaces still open needs to know
 * that is their own earlier decision being honoured, or that space's own file
 * saying so (Story 51.3's `keeper.folded`) — not the setting failing. A note
 * that named only the first would send them looking for a bug in the second
 * case.
 */
export const SESSIONS_SPACES_FOLDED_NOTE =
  "New sessions open with their spaces shut, so a long list of saved queries stays out of the way. Spaces you have folded or unfolded yourself keep what you chose, and so do spaces whose own file says how they open.";

/** The Sessions settings, or nothing when no synced folder is a sessions root. */
export function SessionsSettingsSection({ open }: { open: boolean }) {
  const roots = useSessionsRootsStore((state) => state.roots);

  useEffect(() => {
    if (open) {
      void refreshSessionsRoots();
    }
  }, [open]);

  // `null` is "keeper has not looked yet" and `[]` is "no zone is flagged", and
  // neither one has anything to configure. They render the same nothing here on
  // purpose: a section that flickered in while a read resolved would be a
  // section that appears to arrive by itself.
  if (roots === null || roots.length === 0) {
    return null;
  }
  return <SessionsSpacesFoldedRow />;
}

function SessionsSpacesFoldedRow() {
  // `undefined` = still reading; otherwise the stored default.
  const [folded, setFolded] = useState<boolean | undefined>(undefined);
  // Which write is the live one, so an older request's failure cannot revert a
  // newer request's value — the same guard the dock badge and menu bar rows keep.
  const writeId = useRef(0);
  // The tail of the write chain. A person who flips twice quickly has two
  // answers to send and Rust must receive them in the order they gave them, so
  // each write waits for the previous one rather than racing it.
  const pending = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    let cancelled = false;
    void sessionsSpacesFoldedGet()
      .then((value) => {
        if (!cancelled) {
          setFolded(value);
          // The store's fallback is the same fact, and Settings can be the
          // first surface to learn it: a dialog opened before any session
          // detail has mounted would otherwise leave the store on `false`
          // until one did.
          setSpacesFoldedDefault(value);
        }
      })
      .catch(() => {
        // The store's value, not a hard-coded `false`: it is what the spaces on
        // screen are actually obeying, and a switch that claimed the opposite of
        // them because a read failed — enabled, unchecked, and a flip that
        // appears to do nothing — is the exact dishonesty this idiom exists to
        // prevent. Unfolded is still where the store starts (the registry's own
        // default), so reporting it folds nothing on anyone who never asked.
        if (!cancelled) {
          setFolded(sessionSpacesFoldStore.getState().defaultFolded);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const onChange = (next: boolean) => {
    writeId.current += 1;
    const id = writeId.current;
    const previous = folded ?? false;
    setFolded(next);
    // Optimistic here too, and it is what makes the setting visible: a detail
    // open behind the dialog re-renders every space that has nothing recorded.
    setSpacesFoldedDefault(next);
    // Serialised, not merely guarded: `writeId` drops a stale REVERT, but two
    // `sessionsSpacesFoldedSet` calls in flight are unordered, so on-then-off
    // could commit `0` then `1` and leave Rust folded while the switch and the
    // store both show off. Nothing re-reads, so that disagreement would survive
    // until the next document. Chaining makes the last flip the last write.
    pending.current = pending.current
      .then(() => sessionsSpacesFoldedSet(next))
      .catch(() => {
        if (id === writeId.current) {
          setFolded(previous);
          setSpacesFoldedDefault(previous);
        }
      });
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">{SESSIONS_SECTION_TITLE}</p>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor="sessions-spaces-folded">{SESSIONS_SPACES_FOLDED_LABEL}</Label>
        <div className="flex shrink-0 items-center gap-2">
          <FileControlled settingKey="sessions.spaces_folded" />
          <Switch
            id="sessions-spaces-folded"
            checked={folded ?? false}
            disabled={folded === undefined}
            onCheckedChange={onChange}
          />
        </div>
      </div>
      <p className="text-muted-foreground">{SESSIONS_SPACES_FOLDED_NOTE}</p>
    </div>
  );
}
