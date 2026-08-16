/**
 * The React panel a `![[…]]` data embed becomes (Story 45.12, FR-186, FR-187).
 *
 * # This is a loader and a mount, and nothing else
 *
 * The panel itself — the Source/Table/Structure toggle, the parse banner, the
 * refusal wording, the four states above a loaded file — is 45.4's
 * `TextFileFrame`, the same component a Files panel mounts. What is here is the
 * two things only a note knows.
 *
 * **The coordinates.** 44.16's CSV commands, and this story's
 * `notes_embed_read` / `notes_embed_write`, are addressed by a **notes vault
 * id** plus a vault-relative target. A Files panel holds a **sync profile id**,
 * which is a different identifier over overlapping bytes; deriving one from the
 * other in the webview is the path arithmetic AD-65 forbids, and the resolution
 * is Story 45.18's. That asymmetry has a visible consequence and it is the
 * right way round: **the same `.csv` is a table in a note and its source in a
 * Files panel**, because only the note can name a vault. 45.4 wrote that
 * consequence down as a sentence the panel shows; this is the other half of it.
 *
 * **The registry row comes from Rust's kind.** `resolveViewer` refuses to
 * answer without one (45.2), and the widget's synchronous guess — made to
 * decide that the embed gets to try at all — is not one. So the row is resolved
 * again here, from the `name` and `kind` `notes_embed_read` returned. When the
 * two disagree, Rust wins: a `.json` that Rust one day calls an image resolves
 * to the image row, which has no rendered half and is not writable, and the
 * panel degrades to a read-only source view with the format's own refusal
 * rather than offering to save bytes it has misread.
 *
 * # Two embeds of one file
 *
 * They are two React roots with two buffers and no common ancestor, so a write
 * through one is invisible to the other. {@link announceEmbedWrite} is how they
 * stay in step; see its doc for why the key is the resolved path.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { TextFileFrame } from "@/components/viewers/text-file-frame";
import type { TextFileSource } from "@/components/viewers/use-text-file";
import { useTextBuffer } from "@/components/viewers/use-text-file";
import type { RecordingNoteTargetKind, TextFileVm } from "@/lib/ipc/client";
import { notesCsvSetCell, notesEmbedRead, notesEmbedWrite } from "@/lib/ipc/client";
import { resolveViewer, UNKNOWN_ENTRY } from "@/lib/viewers";
import { announceEmbedWrite, onEmbedWrite } from "./file-embed";

export interface NoteFileEmbedProps {
  /** The vault the note is in. The editor is built per vault, so this is always
   *  a real id — unlike a Files panel's profile, which can be absent. */
  vaultId: string;
  /** The text between the brackets, verbatim. Never joined to a root (AD-65);
   *  Rust forms the candidates and answers with the path it actually read. */
  target: string;
}

/** What Rust said this embed resolved to, once it has. */
interface Resolved {
  readonly relPath: string;
  readonly name: string;
  readonly kind: RecordingNoteTargetKind;
}

export function NoteFileEmbed({ vaultId, target }: NoteFileEmbedProps): React.ReactElement {
  const [resolved, setResolved] = useState<Resolved | null>(null);
  // This panel's identity on the write bus. An empty object is enough: it is
  // compared by reference and never read, and it is a ref so it survives every
  // re-render — a token that changed would make a panel hear its own writes.
  const token = useRef({});
  // Read by the writer, which needs the path Rust resolved rather than the text
  // in the brackets. A ref because the write closure is memoised on the
  // coordinates and must not be rebuilt — and therefore re-read — every time
  // the resolution lands.
  const relPath = useRef<string | null>(null);

  const source = useMemo<TextFileSource>(
    () => ({
      label: target,
      read: async (): Promise<TextFileVm> => {
        // A rejection propagates and the loader words it, which is what puts
        // Rust's "keeper looked for …" sentence where the embed is. The last
        // resolution is deliberately NOT cleared: nothing stale is drawn from
        // it — the frame renders the sentence instead of the file — and keeping
        // it leaves this panel subscribed to the path it last knew, so a
        // sibling putting the file back brings this one back with it.
        const vm = await notesEmbedRead(vaultId, target);
        relPath.current = vm.relPath;
        setResolved({ relPath: vm.relPath, name: vm.name, kind: vm.kind });
        return vm.file;
      },
      write: async (content: string): Promise<void> => {
        await notesEmbedWrite(vaultId, target, content);
        // Announced only after Rust confirmed the write. A refusal must not
        // make another panel throw away a buffer for a write that did not land.
        announceEmbedWrite(vaultId, relPath.current ?? target, token.current);
      },
      // A note embed always has coordinates. The vault id comes from the editor
      // the note is open in, so there is no "outside every profile" case here.
      unreachable: null,
    }),
    [vaultId, target],
  );

  const state = useTextBuffer(source);

  const { reload } = state;
  useEffect(() => {
    const key = resolved?.relPath;
    if (key === undefined) {
      // Nothing to subscribe to yet: until Rust has answered, this panel does
      // not know which file it is showing, and subscribing on the target would
      // key a bare name apart from the same file's full path.
      return;
    }
    return onEmbedWrite(vaultId, key, token.current, () => {
      void reload();
    });
  }, [vaultId, resolved?.relPath, reload]);

  const csvOptions = useMemo(
    () => ({
      // 44.16's own command, wrapped only to announce. The cells, the revision
      // and the byte-identical splice stay entirely in `keeper-core::notes::csv`
      // — nothing here re-serialises a CSV, which is the whole reason that
      // module exists.
      setCell: async (
        cellVault: string,
        cellTarget: string,
        rev: string,
        row: number,
        column: number,
        value: string,
      ) => {
        const next = await notesCsvSetCell(cellVault, cellTarget, rev, row, column, value);
        announceEmbedWrite(vaultId, relPath.current ?? cellTarget, token.current);
        return next;
      },
    }),
    [vaultId],
  );

  const csv = useMemo(() => ({ vaultId, target }), [vaultId, target]);
  const preview = useMemo(() => ({ vaultId }), [vaultId]);

  // Resolved from Rust's answer once there is one. Before that the frame is
  // showing "opening …" and after a failed read it is showing Rust's sentence,
  // so the fallback row is never drawn — it exists because the frame's props
  // are not optional, and the unknown row is the honest value for "keeper has
  // not been told what this is" (AD-91).
  const entry =
    resolved === null ? UNKNOWN_ENTRY : resolveViewer({ name: resolved.name, kind: resolved.kind });

  return (
    <TextFileFrame
      fileName={resolved?.name ?? target}
      entry={entry}
      state={state}
      csv={csv}
      // Story 50.4: a note embed is addressed by a vault id and a vault-relative
      // target, not by a sync profile — the same asymmetry `csv` documents from
      // the other direction. Deriving one from the other here would be the
      // frontend deciding which folders are profiles (AD-65), so this host
      // offers no properties panel and says so rather than guessing.
      properties={null}
      preview={preview}
      csvOptions={csvOptions}
    />
  );
}

/**
 * Mount the panel into a plain DOM node, for a CodeMirror widget to own.
 *
 * The React boundary lives here rather than in the widget so that
 * `file-embed.ts` — which `live-preview.ts` imports statically — contains no
 * React import at all, and the editor's lazy chunk stays what
 * `gallery-block.ts` describes it as.
 */
export function mountNoteFileEmbed(
  container: HTMLElement,
  args: NoteFileEmbedProps,
): { unmount: () => void } {
  const root = createRoot(container);
  root.render(<NoteFileEmbed vaultId={args.vaultId} target={args.target} />);
  return {
    unmount: () => {
      root.unmount();
    },
  };
}
