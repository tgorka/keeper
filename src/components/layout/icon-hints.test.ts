import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

// Deliberately not a tree walk: only the surfaces swept by story 72.2.
const SWEPT_FILES = [
  "notes/note-row.tsx",
  "notes/note-list.tsx",
  "notes/notes-pane.tsx",
  "notes/format-toolbar.tsx",
  "notes/attach-file-button.tsx",
  "notes/note-actions.tsx",
  "notes/note-filter-bar.tsx",
  "notes/notes-phone-pane.tsx",
  "notes/properties-panel.tsx",
  "notes/space-editor.tsx",
  "notes/editor/find-panel.tsx",
  "chat/message-actions.tsx",
  "chat/composer.tsx",
  "chat/reaction-popover.tsx",
  "capture/capture-window.tsx",
  "sessions/session-tree.tsx",
  "sessions/session-actions.tsx",
  "sessions/session-templates.tsx",
  "files/files-phone-pane.tsx",
  "layout/files-pane.tsx",
  "layout/pane-header.tsx",
  "layout/priority-actions.tsx",
  "layout/fold-strip.tsx",
  "layout/spaces-group.tsx",
  "layout/sidebar-pane.tsx",
  "layout/sync-pane.tsx",
  "layout/account-footer.tsx",
  "layout/chat-list-pane.tsx",
  "layout/conversation-pane.tsx",
  "layout/phone-header.tsx",
  "layout/phone-inbox-header.tsx",
  "layout/phone-search-surface.tsx",
  "layout/surface-column.tsx",
  "layout/verify-banner.tsx",
  "export/export-file-button.tsx",
  "recording/recording-meta-fields.tsx",
  "sync/add-folder-form.tsx",
  "viewers/text-file-frame.tsx",
  "bots/bot-attachment.tsx",
  "bots/bot-session-list.tsx",
];

function unhintedControls(text: string): number[] {
  const offenders: number[] = [];
  for (const match of text.matchAll(/<(Button|button)\b[\s\S]*?<\/\1>/g)) {
    const control = match[0];
    const iconOnly =
      /size="icon[^"]*"|FOLD_STRIP\.(?:headControlSize|controlSize|controlClass)/.test(control) ||
      />\s*<\w+\b[^<>]*\/>\s*<\/(?:Button|button)>$/.test(control) ||
      />\s*\{glyph\}\s*<\/button>$/.test(control);
    if (!iconOnly) continue;
    const before = text.slice(0, match.index);
    const lastHint = before.lastIndexOf("<IconHint");
    const wrapped = lastHint > before.lastIndexOf("</IconHint>");
    const exempt = /icon-hint-exempt:[^\n]+\n\s*$/.test(before);
    if (!wrapped && !exempt) offenders.push(before.split("\n").length);
  }
  return offenders;
}

describe("icon-only controls retain a readable hint", () => {
  it("names an unwrapped control by file and line in the explicit swept surfaces", () => {
    // Prove the recognizer itself catches a new icon button rather than passing vacuously.
    expect(unhintedControls('<Button size="icon-sm" aria-label="New"><Plus /></Button>')).toEqual([
      1,
    ]);
    expect(unhintedControls("<button><Plus /></button>")).toEqual([1]);
    expect(
      unhintedControls("// icon-hint-exempt: folded avatar\n<button><Avatar /></button>"),
    ).toEqual([]);
    const offenders = SWEPT_FILES.flatMap((file) => {
      const text = readFileSync(resolve(__dirname, "..", file), "utf8");
      return unhintedControls(text).map((line) => `${file}:${line}`);
    });
    expect(offenders).toEqual([]);
  });
});
