/**
 * Loading, dirty-tracking and saving one text file (Story 45.6, FR-179, AD-89).
 *
 * # Why a hook and not a component
 *
 * Two surfaces need this and neither is the other: Story 45.4's raw/rendered
 * shell binds the registry's `text` viewer and needs to load, and Story 45.12's
 * note embed loads the same way inside a note. A component would have made one
 * of them mount the other's chrome. A hook lets there be exactly one loader and
 * exactly one save rule under two different frames.
 *
 * # One engine, two coordinate systems
 *
 * Those two surfaces do not address a file the same way. A Files panel holds a
 * **sync profile id** and a profile-relative subpath; a note holds a **notes
 * vault id** and the text between a pair of brackets. Neither is derivable from
 * the other in the webview (AD-65; the resolution is Story 45.18's), so the two
 * commands are genuinely different commands.
 *
 * Which is why the pair the surface DID address comes back out as {@link
 * FileOrigin}: a view over the buffer has to be able to tell new bytes from a
 * new file — an undo stack that spans two files writes one over the other — and
 * the only place that answer cannot go stale is beside the read that produced
 * the bytes.
 *
 * Everything else about them is identical, and everything else is where the
 * bugs live: the generation counter that stops a slow read for the previous
 * file overwriting the current one's buffer, `persisted` being state rather
 * than a ref, the rule that a refused save leaves the buffer dirty and never
 * rolls it back, and the four reasons a save declines out loud. So the commands
 * are a {@link TextFileSource} the caller supplies and {@link useTextBuffer} is
 * the rest — one implementation, and a second surface cannot drift from it by
 * copying it.
 *
 * It is also a separate module from `text-viewer.tsx` so that importing the
 * loader does not import CodeMirror. 45.4's chrome asks whether a file is too
 * big before it decides what to render, and paying several hundred kilobytes
 * for that question would defeat the lazy boundary `note-editor.tsx` set up for
 * NFR-27.
 *
 * # One writer per vault, and neither of them is new
 *
 * AD-89 retired AD-75 and gave the Files surface a write path — through
 * `write_vault_file` + `mark_dirty`, the same path notes use, never a second
 * writer. {@link useTextFile} calls {@link syncWriteEntry} and composes no path
 * of its own: the `subpath` it saves to is the one the listing handed it, and
 * Rust re-resolves it through `keeper_sync::browse`'s containment on every call
 * (AD-65). A note embed's `notes_embed_write` lands on that same
 * `write_vault_file`, after the vault's own containment check.
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
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { syncReadText, syncWriteEntry, type TextFileVm } from "@/lib/ipc/client";

/**
 * Which file a buffer was read from, in whichever coordinates its surface
 * addresses files by.
 *
 * **The two hosts do not identify a file the same way, and this does not
 * pretend they do.** A Files panel holds a sync profile id and a
 * profile-relative subpath; a note embed holds a notes vault id and the
 * vault-relative target Rust answered with. Neither is derivable from the other
 * in the webview (AD-65), so this pair is whichever one the surface can prove —
 * it is never assembled, never joined to a root, and never handed to a command,
 * because the commands in {@link TextFileSource.read} / {@link
 * TextFileSource.write} already hold their own coordinates.
 *
 * It exists because a view has to be able to tell NEW BYTES from a NEW FILE, and
 * a display name cannot: story 51.1 made two markdown files with one basename in
 * two directories an ordinary session layout, and a panel replaces its target in
 * place. It comes out of the loader rather than down a second prop chain so it
 * cannot disagree with the buffer it describes.
 */
export interface FileOrigin {
  /**
   * The sync profile id, or the notes vault id — whichever this surface
   * addresses files by — and `null` for a surface that holds neither.
   *
   * Never a note id: a note is a row in a vault's index and this is the vault
   * (or the profile) itself. Never rendered, either; a profile id is a uuid.
   */
  readonly profileOrVaultId: string | null;
  /** The path the buffer was read from, relative to {@link profileOrVaultId}:
   *  a profile-relative subpath, or a vault-relative target. Never absolute
   *  (FR-145). */
  readonly relativePath: string;
}

/**
 * The two commands one surface addresses one file with, plus what to call it.
 *
 * **Must be referentially stable.** {@link useTextBuffer} re-reads whenever this
 * changes, which is exactly right when the file changes and an infinite loop
 * when it does not — so a caller builds it with `useMemo` over the identifiers
 * it is keyed on. Both facades in this repository do; a caller that forgets
 * gets a read per render, which the tests below catch by counting calls.
 */
export interface TextFileSource {
  /**
   * The path this surface addresses the file by, relative to {@link
   * profileOrVaultId}. Never a path that could be absolute (FR-145).
   *
   * Rendered in a sentence and in a log line — and, with the id below, it is
   * also the file's identity ({@link FileOrigin}), which is why it is a path
   * and not a display name.
   */
  readonly label: string;
  /** The profile or vault {@link label} is relative to, or `null` when this
   *  surface has neither. See {@link FileOrigin}. */
  readonly profileOrVaultId: string | null;
  /** Read it, or reject with Rust's sentence. */
  readonly read: () => Promise<TextFileVm>;
  /** Write the whole buffer exactly, or reject with Rust's sentence. */
  readonly write: (content: string) => Promise<void>;
  /**
   * Why this surface cannot address this file at all, or `null`.
   *
   * Not a loading placeholder and not an error from a command: it is the
   * surface admitting up front that it holds no coordinates for this file — a
   * Files panel showing something outside every profile. Non-null
   * short-circuits both halves, so **no command is called**, which matters
   * because the alternative route (reading through an absolute path) would go
   * around Rust's containment check.
   *
   * Two wordings because they appear in two places: `notice` is rendered in
   * place of the file, `reason` completes "not saving <label> — …" in the log.
   */
  readonly unreachable: { readonly notice: string; readonly reason: string } | null;
}

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
  /**
   * Which file these bytes came from — {@link FileOrigin}, carried out of the
   * loader so a view can rebuild on a new FILE without rebuilding on new BYTES.
   */
  loadedFrom: FileOrigin;
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

/**
 * Load, track and save one file, whatever commands address it.
 *
 * The whole of this module's behaviour lives here; {@link useTextFile} and a
 * note embed's own facade differ only in the {@link TextFileSource} they build.
 */
export function useTextBuffer(source: TextFileSource): UseTextFileResult {
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
    if (source.unreachable !== null) {
      setVm(null);
      setContentState("");
      setPersisted("");
      setError(source.unreachable.notice);
      setLoading(false);
      return;
    }
    try {
      const next = await source.read();
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
      setError(sentence(failure, `keeper could not read ${source.label}.`));
    } finally {
      if (generation.current === mine) {
        setLoading(false);
      }
    }
  }, [source]);

  useEffect(() => {
    void read();
  }, [read]);

  const save = useCallback(async (): Promise<void> => {
    const declined = (reason: string): void => {
      console.info(`keeper: not saving ${source.label} — ${reason}`);
    };
    if (source.unreachable !== null) {
      declined(source.unreachable.reason);
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
      await source.write(content);
      // Only now. A rejection must leave the file dirty so the next save tries
      // again, and must never roll the buffer back: losing what someone typed
      // is worse than showing text the disk does not have yet.
      setPersisted(content);
      setError(null);
    } catch (failure) {
      setError(sentence(failure, `keeper could not save ${source.label}.`));
    }
  }, [content, persisted, source, vm]);

  // One object per file rather than one per render, so a consumer may put it in
  // a dependency array without rebuilding on every keystroke — which is exactly
  // what the views downstream do with it.
  const loadedFrom = useMemo<FileOrigin>(
    () => ({ profileOrVaultId: source.profileOrVaultId, relativePath: source.label }),
    [source.profileOrVaultId, source.label],
  );

  return {
    vm,
    content,
    setContent: setContentState,
    dirty: content !== persisted,
    save,
    reload: read,
    error,
    loading,
    loadedFrom,
  };
}

/**
 * The Files surface's file: a sync profile id and a profile-relative subpath
 * (Story 45.6, FR-179, AD-89).
 *
 * Nothing is composed here. The subpath is the one the listing produced, and
 * Rust re-resolves it through `keeper_sync::browse`'s containment on every call
 * (AD-65) — which is also why a file outside every profile is `unreachable`
 * rather than read through its `absolutePath`: that route would go around the
 * containment check the profile commands exist to enforce.
 */
export function useTextFile({ profileId, subpath }: UseTextFileArgs): UseTextFileResult {
  const source = useMemo<TextFileSource>(() => {
    if (profileId === null) {
      // A rejection rather than a call with `""` for the profile: if
      // `unreachable` ever stopped short-circuiting, this surfaces as a failed
      // read rather than as a command quietly aimed at a profile that does not
      // exist.
      const refuse = async (): Promise<never> => {
        throw new Error(`${subpath} is not inside a synced folder`);
      };
      return {
        label: subpath,
        profileOrVaultId: null,
        read: refuse,
        write: refuse,
        unreachable: {
          notice:
            "This file is not inside a synced folder, so keeper cannot open or save it here. Use Open With to read it.",
          reason: "it is not inside a synced folder.",
        },
      };
    }
    return {
      label: subpath,
      profileOrVaultId: profileId,
      read: () => syncReadText(profileId, subpath),
      write: (content) => syncWriteEntry(profileId, subpath, content),
      unreachable: null,
    };
  }, [profileId, subpath]);
  return useTextBuffer(source);
}
