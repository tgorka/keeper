import { Check } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { logFailure } from "@/lib/failure";
import type { SignInLinkVm } from "@/lib/ipc/client";

export const SIGN_IN_LINK_CAPTION = "If your browser did not open, copy this link into it";
export const SIGN_IN_LINK_COPY_LABEL = "Copy link";
export const SIGN_IN_LINK_COPIED = "Link copied";
/** A clipboard the webview refused or does not have; the link stays selectable. */
export const SIGN_IN_LINK_COPY_FAILED =
  "keeper couldn't copy the link. Select it above and copy it.";

/** Where a failed copy is logged (R381): a fixed name, never content. */
export const SIGN_IN_LINK_COPY_SOURCE = "signIn.copyLink";
/**
 * Why a copy failed, as logged: fixed words. Neither the link nor the
 * clipboard's own error (which may quote what it was handed) reaches the log.
 */
export const SIGN_IN_LINK_NO_CLIPBOARD = "this webview has no clipboard";
export const SIGN_IN_LINK_CLIPBOARD_REFUSED = "the clipboard refused the link";

/**
 * The authorization page a waiting sign-in is on (story 102.9, R470), in the
 * shape the forge device flow ships (`browse-repos-sheet`): the link as
 * selectable text, a Copy button, and — when Rust could not open the browser
 * or the sign-in sheet — Rust's sentence saying so. Drawn only while the
 * sign-in waits: Rust takes the link away when it ends (R480). A failed copy
 * is said here and logged with a fixed reason (R381).
 */
export function SignInLink({ link }: { link: SignInLinkVm }) {
  const id = useId();
  // Which link the last copy was of, and whether it worked: a later sign-in's
  // link has not been copied, whatever happened to the one before.
  const [lastCopy, setLastCopy] = useState<{ url: string; ok: boolean } | null>(null);
  const copied = lastCopy !== null && lastCopy.url === link.url ? lastCopy.ok : null;
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const copy = () => {
    const url = link.url;
    const said = (ok: boolean) => {
      if (mounted.current) {
        setLastCopy({ url, ok });
      }
    };
    // The sentence below is where the person looks; this is the log line.
    const failed = (reason: string) => {
      logFailure(SIGN_IN_LINK_COPY_SOURCE)(reason);
      said(false);
    };
    const clipboard: Clipboard | undefined = navigator.clipboard;
    if (clipboard === undefined) {
      failed(SIGN_IN_LINK_NO_CLIPBOARD);
      return;
    }
    clipboard.writeText(url).then(
      () => said(true),
      () => failed(SIGN_IN_LINK_CLIPBOARD_REFUSED),
    );
  };

  return (
    <div className="flex min-w-0 flex-col gap-2">
      {link.notOpened !== null && (
        <p role="alert" className="text-destructive text-sm">
          {link.notOpened}
        </p>
      )}
      <span className="text-muted-foreground text-xs" id={`${id}-caption`}>
        {SIGN_IN_LINK_CAPTION}
      </span>
      <output
        aria-labelledby={`${id}-caption`}
        className="max-h-24 select-all overflow-y-auto break-all rounded-md border border-border bg-muted/40 p-2 font-mono text-xs"
      >
        {link.url}
      </output>
      <div className="flex flex-wrap items-center gap-2">
        <Button type="button" variant="outline" size="sm" className="w-fit" onClick={copy}>
          {SIGN_IN_LINK_COPY_LABEL}
        </Button>
        {copied === true && (
          <span role="status" className="flex items-center gap-1 text-muted-foreground text-xs">
            <Check aria-hidden="true" className="size-3.5" />
            {SIGN_IN_LINK_COPIED}
          </span>
        )}
      </div>
      {copied === false && (
        <p role="alert" className="text-destructive text-xs">
          {SIGN_IN_LINK_COPY_FAILED}
        </p>
      )}
    </div>
  );
}
