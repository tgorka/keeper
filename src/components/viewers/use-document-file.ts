/**
 * Loading one document (Story 45.8, FR-181, FR-182).
 *
 * # Why a hook and not a component
 *
 * The same reasoning `use-text-file.ts` gives for 45.6, and it applies harder
 * here. Two surfaces need a document: the Files pane mounts the registry's
 * `document` viewer, and a note embeds the same file. A component would have
 * made one of them mount the other's chrome. A hook means there is exactly one
 * loader, and the presentational half — `DocumentView` — can be handed a
 * `DocumentVm` that arrived any other way.
 *
 * # It reads no path and joins nothing
 *
 * `profileId` and `subpath` go to Rust as they arrived; Rust re-resolves them
 * through `keeper_sync::browse`'s containment on every call (AD-65). Nothing
 * here composes a path, and `absolutePath` is never touched (FR-145).
 *
 * # There is nothing to save
 *
 * Deliberately, and it is the reason this hook is a third of the size of
 * `useTextFile`. A PDF, a DOCX, a PPTX and an XLSX are read-only because a
 * lossy round trip through a document container is how people lose work (the
 * epic's "what is NOT in this epic"). No dirty tracking, no save, no refusal
 * wording for a save that cannot happen — the absence is the design.
 *
 * # A refusal is a resolution
 *
 * Almost everything that can go wrong with a document — it is not one, it is
 * over a cap, a part is corrupt, it is a decompression bomb — comes back as a
 * `DocumentVm` carrying a sentence, because those are all things the viewer
 * draws. {@link UseDocumentFileResult.error} is only for the command itself
 * failing: no such profile, the drive was unplugged, the file is gone.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { type DocumentVm, syncReadDocument } from "@/lib/ipc/client";

export interface UseDocumentFileArgs {
  /** The sync profile `subpath` is relative to, or `null` when this file is
   *  not inside one — in which case there is nothing to read and the hook says
   *  so rather than calling a command that will refuse. */
  readonly profileId: string | null;
  /** The profile-relative path the listing handed the surface. */
  readonly subpath: string;
}

export interface UseDocumentFileResult {
  /** What Rust made of the document, or `null` while loading or after a
   *  failure. */
  readonly vm: DocumentVm | null;
  /** The command failed. A sentence, already worded, safe to render. */
  readonly error: string | null;
  readonly loading: boolean;
  /** Read it again — after the drive came back, or the file changed. */
  readonly reload: () => void;
}

/** The sentence for a file that is not inside a synced folder.
 *
 *  A panel can VIEW a file outside every profile; the read commands are
 *  profile-scoped, so there is no way to fetch its bytes. Saying so is better
 *  than a spinner that never resolves. */
const NO_PROFILE =
  "this file is not inside a synced folder, so keeper cannot read it — use Open With";

/** The sentence for a command that failed for a reason Rust did not word. */
const UNREADABLE = "keeper could not read this file";

/**
 * The sentence to show for a rejection.
 *
 * Rust's own message when there is one — those are written to be read by a
 * person — and a fallback when the rejection is not the shape this code
 * expects. Never `String(error)` on its own: that renders `[object Object]`
 * for an `IpcError`, which is the error message users report as a bug in
 * itself.
 */
function sentence(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const { message } = error as { message: unknown };
    if (typeof message === "string" && message !== "") {
      return message;
    }
  }
  return UNREADABLE;
}

export function useDocumentFile({
  profileId,
  subpath,
}: UseDocumentFileArgs): UseDocumentFileResult {
  const [vm, setVm] = useState<DocumentVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [nonce, setNonce] = useState(0);

  // Which load is current. A panel whose target changes twice quickly starts
  // two reads, and without this the slower one wins and paints the previous
  // file's contents under the current file's name.
  const generation = useRef(0);

  // nonce is this effect's TRIGGER, not its input: a reload asks the same
  // question again, so the body reads nothing from it. Dropping it to satisfy
  // the rule would make the reload control do nothing.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reason above
  useEffect(() => {
    const mine = ++generation.current;
    setLoading(true);
    setError(null);

    if (profileId === null) {
      setVm(null);
      setError(NO_PROFILE);
      setLoading(false);
      return;
    }

    void syncReadDocument(profileId, subpath)
      .then((loaded) => {
        if (generation.current !== mine) {
          return;
        }
        setVm(loaded);
        setLoading(false);
      })
      .catch((rejection: unknown) => {
        if (generation.current !== mine) {
          return;
        }
        setVm(null);
        setError(sentence(rejection));
        setLoading(false);
      });
  }, [profileId, subpath, nonce]);

  const reload = useCallback(() => setNonce((previous) => previous + 1), []);

  return { vm, error, loading, reload };
}
