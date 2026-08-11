/**
 * The Settings primary view.
 *
 * Settings used to be a modal dialog, reached from the sidebar entry, the
 * account-row menu, the verify banner and the UTD stub. It is now a pane like
 * Recording, Sync, Bridges and Approvals, for the reason those are panes: it is a
 * place you go and stay, not a question you answer and dismiss. A dialog also
 * traps focus and covers the app, which is wrong for a surface whose Sync section
 * you read *while* watching a folder work.
 *
 * The sections themselves are {@link SettingsBody}, shared verbatim with the
 * dialog that still exists for the phone tier — one definition, so the two
 * surfaces cannot drift on which settings exist or in what order.
 */
import { SettingsBody } from "@/components/settings/settings-dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { primaryViewStore } from "@/lib/stores/primary-view";

/** The pane heading, and the sentence under it. */
export const SETTINGS_PANE_TITLE = "Settings";
export const SETTINGS_PANE_SUBTITLE = "Archive & Storage";

export function SettingsPane() {
  return (
    <section
      aria-label={SETTINGS_PANE_TITLE}
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-background"
    >
      {/* Same header shape as the Bridges pane, so the primary views read as one
          family rather than as four separately-designed screens. */}
      <header className="flex shrink-0 flex-col gap-0.5 border-border border-b px-4 py-3">
        <h1 className="font-heading text-title">{SETTINGS_PANE_TITLE}</h1>
        <p className="text-muted-foreground text-xs">{SETTINGS_PANE_SUBTITLE}</p>
      </header>
      <ScrollArea className="min-h-0 flex-1">
        <div className="flex min-w-0 flex-col gap-4 p-4">
          {/* `open` is the hydration signal every section takes, and a mounted
              pane is unambiguously open — the pane only exists while it is the
              active view, so there is no closed state to represent.

              `onOpenChange` is only reached by the setup action that replaces
              this surface entirely; in a pane there is nothing to close, so it
              returns to the inbox, which is where that flow used to leave the
              user once the dialog dismissed itself. */}
          <SettingsBody open onOpenChange={() => primaryViewStore.getState().setView("inbox")} />
        </div>
      </ScrollArea>
    </section>
  );
}
