/**
 * Loading, dirty-tracking and saving one text file (Story 45.6, FR-179, AD-89).
 *
 * # Why a hook and not a component
 *
 * Two surfaces need this and neither is the other: Story 45.4's raw/rendered
 * shell binds the registry's `text` viewer and needs to load, and Story 45.12's
 * note embed will need to load the same way inside a note. A component would
 * have made one of them mount the other's chrome. A hook lets there be exactly
 * one loader and exactly one save rule under two different frames.
 *
 * It is also a separate module from `text-viewer.tsx` so that importing the
 * loader does not import CodeMirror. 45.4's chrome asks whether a file is too
 * big before it decides what to render, and paying several hundred kilobytes
 * for that question would defeat the lazy boundary `note-editor.tsx` set up for
 * NFR-27.
 *
 * # One writer, and it is Story 45.3's
 *
 * AD-89 retired AD-75 and gave the Files surface a write path — through
 * `write_vault_file` + `mark_dirty`, the same path notes use, never a second
 * writer. This hook calls {@link syncWriteEntry} and composes no path of its
 * own: the `subpath` it saves to is the one the listing handed it, and Rust
 * re-resolves it through `keeper_sync::browse`'s containment on every call
 * (AD-65).
 *
 * # Declining out loud
 *
 * Every reason this hook can refuse to save — nothing changed, the file is a
 * prefix, the bytes are not text, there is no profile to write into — is logged
 * at `console.info` naming the file and the reason. A save that silently does
 * nothing is DW-162 exactly: the surface looks like it worked, the file did not
 * change, and nothing on the machine says why. `console.debug` would not do,
 * for the same reason `tracing::debug!` does not reach a packaged app's log.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { syncReadText, syncWriteEntry, type TextFileVm } from "@/lib/ipc/client";

export interface UseTextFileArgs {
  /**
   * The sync profile the file lives in, or `null` when it lives in none.
   *
   * `null` is a real state and not a loading placeholder: a panel can view a
   * file outside every profile, and the honest answer is that it can be neither
   * read nor written through these commands. The hook reports that as an
   * `error` sentence rather than hanging in `loading` forever.
   */
  profileId: string | null;
  /** The profile-relative path the listing produced. Never composed here. */
  subpath: string;
}

export interface UseTextFileResult {
  /** What Rust said about the file, or `null` until the first read resolves. */
  vm: TextFileVm | null;
  /** The buffer as it stands, `""` before the first read resolves. */
  content: string;
  /** Adopt an edit. Marks the file dirty when it differs from what is on disk. */
  setContent: (next: string) => void;
  /** Whether the buffer differs from what was last persisted. */
  dirty: boolean;
  /** Write the buffer through Story 45.3's command. Declines, loudly, when it
   *  must — see the module doc. */
  save: () => Promise<void>;
  /** Re-read from disk and replace the buffer. For after an outside write. */
  reload: () => Promise<void>;
  /** A whole sentence, already worded by Rust, or `null`. */
  error: string | null;
  loading: boolean;
}

/**
 * The sentence to show for a rejection.
 *
 * `invoke` guarantees an `IpcError` whose `message` is composed in Rust to be
 * rendered verbatim, so that is what a user should see. Narrowed with `in` and
 * `typeof` rather than asserted: an unchecked cast would read `message` off
 * whatever arrived and put `undefined` on the screen if the contract ever
 * changed, which is exactly the failure the fallback exists to prevent.
 *
 * Nothing here inspects `code`. A rejection only reaches this hook when the
 * listing already said the location was writable, so it is a real fault — a
 * permission, a drive that went out mid-edit — and the honest UI is Rust's own
 * sentence rather than a branch on a string.
 */
function sentence(error: unknown, fallback: string): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const { message } = error;
    if (typeof message === "string" && message !== "") {
      return message;
    }
  }
  return fallback;
}

export function useTextFile({ profileId, subpath }: UseTextFileArgs): UseTextFileResult {
  const [vm, setVm] = useState<TextFileVm | null>(null);
  const [content, setContentState] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  // What is on disk, as far as this hook knows. Separate from `vm.text` because
  // a successful save advances it without a re-read: asking Rust to hand back a
  // megabyte to confirm what we just wrote would double every save.
  //
  // **State and not a ref, and that distinction is load-bearing.** `dirty` is
  // derived from it at render time, so advancing a ref after a save would leave
  // the surface showing unsaved-changes chrome over a file that is on disk —
  // React has no reason to re-render for a mutated ref, and the one state
  // update a successful save does make (`setError(null)`) is a no-op when the
  // error was already null. The test named "is clean again after a save" fails
  // on exactly that mutation and on no other.
  const [persisted, setPersisted] = useState("");
  // Bumped by `reload` and by a change of file, and checked after every await:
  // a slow read for the previous file must not overwrite the current one's
  // buffer when it finally lands.
  const generation = useRef(0);

  const read = useCallback(async (): Promise<void> => {
    const mine = ++generation.current;
    setLoading(true);
    if (profileId === null) {
      setVm(null);
      setContentState("");
      setPersisted("");
      setError(
        "This file is not inside a synced folder, so keeper cannot open or save it here. Use Open With to read it.",
      );
      setLoading(false);
      return;
    }
    try {
      const next = await syncReadText(profileId, subpath);
      if (generation.current !== mine) {
        return;
      }
      setVm(next);
      setContentState(next.text ?? "");
      setPersisted(next.text ?? "");
      // `detail` is not an error — an oversize file opened fine, it just cannot
      // be edited — so only the refusal becomes one. The banner reads `detail`
      // off the VM itself.
      setError(next.binary ? next.detail : null);
    } catch (failure) {
      if (generation.current !== mine) {
        return;
      }
      setVm(null);
      setContentState("");
      setPersisted("");
      setError(sentence(failure, `keeper could not read ${subpath}.`));
    } finally {
      if (generation.current === mine) {
        setLoading(false);
      }
    }
  }, [profileId, subpath]);

  useEffect(() => {
    void read();
  }, [read]);

  const save = useCallback(async (): Promise<void> => {
    const declined = (reason: string): void => {
      console.info(`keeper: not saving ${subpath} — ${reason}`);
    };
    if (profileId === null) {
      declined("it is not inside a synced folder.");
      return;
    }
    if (vm === null) {
      declined("it has not finished opening.");
      return;
    }
    if (vm.binary) {
      declined("its bytes are not text, so there is nothing an editor could write back.");
      return;
    }
    if (vm.oversize) {
      declined(
        `it is ${vm.sizeLabel} and only the first part was loaded; saving would truncate it.`,
      );
      return;
    }
    if (content === persisted) {
      declined("nothing changed.");
      return;
    }
    try {
      await syncWriteEntry(profileId, subpath, content);
      // Only now. A rejection must leave the file dirty so the next save tries
      // again, and must never roll the buffer back: losing what someone typed
      // is worse than showing text the disk does not have yet.
      setPersisted(content);
      setError(null);
    } catch (failure) {
      setError(sentence(failure, `keeper could not save ${subpath}.`));
    }
  }, [content, persisted, profileId, subpath, vm]);

  return {
    vm,
    content,
    setContent: setContentState,
    dirty: content !== persisted,
    save,
    reload: read,
    error,
    loading,
  };
}
