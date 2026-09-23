/**
 * "Add a device or a person" (Epic 82, UX-DR116 (3)).
 *
 * The setup link as a QR code, for a phone's camera, and as text with a Copy
 * button, for a Mac, which cannot scan. The link is the same for everyone —
 * a new person scans it and signs in as themselves — so the sheet says that,
 * rather than implying it invites someone in particular.
 *
 * The QR is Rust's SVG (`bridges::login::qr_svg`) as an `<img>` data URL on the
 * house's mandatory white card, white in both themes because a dark card does
 * not scan. DESIGN.md (QR login panel) sizes the CODE at ≥ 240 px, so the
 * image itself is 240 px and the card's padding is the quiet zone around it —
 * the bridge-login sheet's shape sized the card at 240 px instead, which left
 * the code at 208. A link too long to encode arrives with no SVG, and the
 * sheet says so instead of drawing an empty card.
 *
 * Copy link says when the clipboard refused (or there is none), because the
 * person would otherwise paste whatever was on it before; the link is
 * selectable in one click for copying by hand. "Copied" goes back to "Copy
 * link" after the house's two seconds, as the bot answer's copy button does.
 */
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { type AccountShareVm, accountShare } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

export const SHARE_SHEET_TITLE = "Add a device or a person";
export const SHARE_SHEET_NOTE =
  "Scan this with the camera on another device, or send the link. Everyone who uses it signs in as themselves and gets their own settings.";
export const SHARE_QR_ALT = "QR code of the setup link. Scan it with the camera on another device.";
export const SHARE_NO_QR = "This link is too long for a QR code. Copy it instead.";
export const SHARE_COPY_LABEL = "Copy link";
export const SHARE_COPIED_LABEL = "Copied";
export const SHARE_READING = "Making the setup link…";
export const SHARE_FAILED = "keeper couldn't make a setup link.";
export const SHARE_COPY_FAILED = "keeper couldn't copy the link. Select it above and copy it.";
/** How long "Copied" stays before the button reads "Copy link" again (bot-answer.tsx's value). */
export const SHARE_COPIED_RESET_MS = 2000;
/** The code's own edge, not the card's (DESIGN.md: the QR is at least 240 px). */
const QR_PX = 240;

export function AccountShareSheet({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="flex flex-col gap-4">
        <SheetHeader>
          <SheetTitle>{SHARE_SHEET_TITLE}</SheetTitle>
          <SheetDescription>{SHARE_SHEET_NOTE}</SheetDescription>
        </SheetHeader>
        <div className="flex min-h-0 flex-1 flex-col items-center gap-4 overflow-y-auto px-4 pb-4 text-sm">
          {open && <ShareBody />}
        </div>
      </SheetContent>
    </Sheet>
  );
}

function ShareBody() {
  // `undefined` = still reading; a string = Rust's refusal.
  const [share, setShare] = useState<AccountShareVm | string | undefined>(undefined);
  const [copy, setCopy] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    let abandoned = false;
    void accountShare()
      .then((vm) => {
        if (!abandoned) {
          setShare(vm);
        }
      })
      .catch((raw: unknown) => {
        if (!abandoned) {
          setShare(syncErrorMessage(raw, SHARE_FAILED));
        }
      });
    return () => {
      abandoned = true;
    };
  }, []);

  useEffect(() => {
    if (copy !== "copied") {
      return;
    }
    const timer = setTimeout(() => setCopy("idle"), SHARE_COPIED_RESET_MS);
    return () => clearTimeout(timer);
  }, [copy]);

  if (share === undefined) {
    return (
      <p role="status" className="text-muted-foreground">
        {SHARE_READING}
      </p>
    );
  }
  if (typeof share === "string") {
    return (
      <p role="alert" className="text-destructive">
        {share}
      </p>
    );
  }
  return (
    <>
      {share.qrSvg !== null ? (
        <div className="flex items-center justify-center rounded-lg bg-white p-4">
          <img
            src={`data:image/svg+xml,${encodeURIComponent(share.qrSvg)}`}
            alt={SHARE_QR_ALT}
            width={QR_PX}
            height={QR_PX}
            className="size-60"
          />
        </div>
      ) : (
        <p className="text-muted-foreground">{SHARE_NO_QR}</p>
      )}
      <p className="w-full select-all break-all rounded-md border border-border p-2 font-mono text-xs">
        {share.link}
      </p>
      <Button
        type="button"
        variant="outline"
        onClick={() => {
          setCopy("idle");
          // Typed as always present, but a webview without clipboard access
          // has none at all.
          const clipboard: Clipboard | undefined = navigator.clipboard;
          if (clipboard === undefined) {
            setCopy("failed");
            return;
          }
          void clipboard.writeText(share.link).then(
            () => setCopy("copied"),
            () => setCopy("failed"),
          );
        }}
      >
        {copy === "copied" ? SHARE_COPIED_LABEL : SHARE_COPY_LABEL}
      </Button>
      {copy === "failed" && (
        <p role="alert" className="text-destructive">
          {SHARE_COPY_FAILED}
        </p>
      )}
    </>
  );
}
