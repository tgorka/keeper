/**
 * Shared command-palette result rows (Story 9.1 / Story 13.4).
 *
 * The single source of the palette's `CommandItem` rows, extracted verbatim from
 * `command-palette.tsx` so the desktop `CommandPalette` and the phone
 * `PhoneSearchSurface` render byte-identical Chat/Contact and Action rows over the
 * same reused `paletteQuery` engine — no forked or restyled second row set. Pure
 * render: each row only projects its `PaletteChatVm` / `PaletteActionVm` and
 * fires `onSelect`; filtering, scoring, and ordering stay authoritative in Rust
 * (AD-20).
 */
import { Badge } from "@/components/ui/badge";
import { CommandItem } from "@/components/ui/command";
import { Kbd } from "@/components/ui/kbd";
import { accountHueVar } from "@/lib/account-hue";
import type { PaletteActionVm, PaletteChatVm } from "@/lib/ipc/client";

/** One chat/contact result row: type glyph, hue dot, name, network badge. */
export function PaletteChatRow({ chat, onSelect }: { chat: PaletteChatVm; onSelect: () => void }) {
  return (
    <CommandItem value={chat.id} onSelect={onSelect}>
      <span aria-hidden className="text-muted-foreground">
        {chat.isDirect ? "◍" : "◆"}
      </span>
      <span
        aria-hidden
        data-testid="account-hue-dot"
        className="size-2 shrink-0 rounded-full"
        style={{ backgroundColor: accountHueVar(chat.hueIndex) }}
      />
      <span className="truncate">{chat.displayName}</span>
      {chat.network !== null && (
        <Badge variant="secondary" className="ml-auto shrink-0">
          {chat.network}
        </Badge>
      )}
    </CommandItem>
  );
}

/** One action result row: ⚡ glyph, title, optional shortcut chip. */
export function PaletteActionRow({
  action,
  onSelect,
}: {
  action: PaletteActionVm;
  onSelect: () => void;
}) {
  return (
    <CommandItem value={action.id} onSelect={onSelect}>
      <span aria-hidden className="text-muted-foreground">
        ⚡
      </span>
      <span className="truncate">{action.title}</span>
      {action.shortcut !== null && <Kbd className="ml-auto">{action.shortcut}</Kbd>}
    </CommandItem>
  );
}
