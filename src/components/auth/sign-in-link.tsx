import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { logFailure, sayFailure } from "@/lib/failure";
import type { SignInLinkVm } from "@/lib/ipc/client";

/** The offer under a sign-in waiting for its browser. */
export const SIGN_IN_LINK_PROMPT = "Browser didn't open? Copy the sign-in link";
/** What follows keeper's own reason when it could not open the page itself. */
export const SIGN_IN_LINK_OPEN_YOURSELF = "Copy the sign-in link and open it in your browser.";
export const SIGN_IN_LINK_LABEL = "Sign-in link";
export const SIGN_IN_LINK_COPY_LABEL = "Copy link";
export const SIGN_IN_LINK_COPIED_LABEL = "Copied";
const COPY_FAILED = "keeper couldn't copy the sign-in link. Select it and copy it yourself.";
/** How often the waiting sign-in's page is asked for. */
export const SIGN_IN_LINK_POLL_MS = 1000;

interface SignInLinkProps {
  /** The waiting sign-in's page: `accountSignInLink` or `oidcSignInLink`. */
  fetchLink: () => Promise<SignInLinkVm | null>;
}

/**
 * The page of a sign-in waiting for its browser, for the person whose browser
 * did not open it: the address, selectable, and a copy button. Rust keeps the
 * page with the flow and drops it when the flow ends, so this only asks while
 * it is mounted (the sign-in's pending state) and renders nothing until there
 * is a page. The address carries the flow's state, so it is shown, never logged.
 */
export function SignInLink({ fetchLink }: SignInLinkProps) {
  const [link, setLink] = useState<SignInLinkVm | null>(null);
  // The address last copied: a new page (a second leg, a new attempt) has not been.
  const [copiedUrl, setCopiedUrl] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setInterval> | undefined;
    const ask = () => {
      fetchLink()
        .then((next) => {
          if (!live) {
            return;
          }
          setLink((prev) =>
            prev?.url === next?.url && prev?.openFailed === next?.openFailed ? prev : next,
          );
        })
        .catch((raw: unknown) => {
          // Asking again every second would only repeat the failure.
          clearInterval(timer);
          logFailure("signIn.link")(raw);
        });
    };
    ask();
    timer = setInterval(ask, SIGN_IN_LINK_POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [fetchLink]);

  if (link === null) {
    return null;
  }
  return (
    <div className="flex flex-col gap-2 text-sm">
      {link.openFailed === null ? (
        <p className="text-muted-foreground">{SIGN_IN_LINK_PROMPT}</p>
      ) : (
        <p role="alert" className="text-destructive">
          {link.openFailed} {SIGN_IN_LINK_OPEN_YOURSELF}
        </p>
      )}
      <div className="flex gap-2">
        <Input
          readOnly
          aria-label={SIGN_IN_LINK_LABEL}
          className="font-mono text-xs"
          value={link.url}
          onFocus={(event) => event.currentTarget.select()}
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => {
            const failed = sayFailure("signIn.copyLink", COPY_FAILED);
            // A webview without a clipboard says so; the field stays selectable.
            if (navigator.clipboard === undefined) {
              failed(new Error("no clipboard in this webview"));
              return;
            }
            void navigator.clipboard
              .writeText(link.url)
              .then(() => setCopiedUrl(link.url))
              .catch(failed);
          }}
        >
          {copiedUrl === link.url ? SIGN_IN_LINK_COPIED_LABEL : SIGN_IN_LINK_COPY_LABEL}
        </Button>
      </div>
    </div>
  );
}
