import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { SessionThumbnail } from "@/components/agents/session-thumbnail";
import { logFailure } from "@/lib/failure";
import {
  type AgentSessionRef,
  agentAttachBytes,
  agentAttachPath,
  agentAttachRelease,
} from "@/lib/ipc/client";
import type { PendingAttachment } from "@/lib/stores/attachments";

/** The same Rust snapshot feeds the tray and Send; a removed draft releases it,
 * including a slow stage that resolves after removal/unmount. No file URL reads. */
export function useSessionAttachmentDraft(target: AgentSessionRef) {
  const staged = useRef(new Map<string, Promise<string>>());
  const [generation, setGeneration] = useState(0);
  const { database, sessionId } = target;
  const stage = useCallback(
    (attachment: PendingAttachment) => {
      const existing = staged.current.get(attachment.id);
      if (existing) return existing;
      const key = { database, sessionId };
      const promise =
        attachment.kind === "path"
          ? agentAttachPath(key, [attachment.path]).then((handles) => handles[0])
          : agentAttachBytes(key, attachment.bytes, attachment.filename, attachment.mime);
      staged.current.set(attachment.id, promise);
      return promise;
    },
    [database, sessionId],
  );
  const release = useCallback(
    (id: string) => {
      const promise = staged.current.get(id);
      staged.current.delete(id);
      if (promise)
        void promise
          .then((handle) => agentAttachRelease({ database, sessionId }, [handle]))
          .catch(logFailure("agents.attach.release"));
    },
    [database, sessionId],
  );
  useEffect(
    () => () => {
      for (const id of staged.current.keys()) release(id);
    },
    [release],
  );
  return {
    stage,
    // Send owns these handles now. On failure the tray restages for retry.
    has(id: string) {
      return staged.current.has(id);
    },
    consume(ids: string[], transferred: string[], retry = false) {
      for (const id of ids) {
        const promise = staged.current.get(id);
        staged.current.delete(id);
        if (promise)
          void promise
            .then((handle) => {
              if (!transferred.includes(handle))
                return agentAttachRelease({ database, sessionId }, [handle]);
            })
            .catch(logFailure("agents.attach.release"));
      }
      if (retry) setGeneration((value) => value + 1);
    },
    preview(attachment: PendingAttachment) {
      const isImage =
        attachment.kind === "bytes"
          ? /^image\/(png|jpeg|gif|webp)$/.test(attachment.mime)
          : /\.(png|jpe?g|gif|webp)$/i.test(attachment.filename);
      return isImage ? (
        <DraftThumbnail
          key={`${attachment.id}:${generation}`}
          attachment={attachment}
          stage={stage}
          release={release}
        />
      ) : null;
    },
  };
}

function DraftThumbnail({
  attachment,
  stage,
  release,
}: {
  attachment: PendingAttachment;
  stage: (attachment: PendingAttachment) => Promise<string>;
  release: (id: string) => void;
}) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    void stage(attachment)
      .then((handle) => {
        if (active) setSrc(`${convertFileSrc(handle, "keeper-hq-blob")}?thumbnail=1`);
      })
      .catch(logFailure("agents.attach.preview"));
    return () => {
      active = false;
      release(attachment.id);
    };
  }, [attachment, stage, release]);
  return src ? <SessionThumbnail src={src} name={attachment.filename} /> : null;
}
