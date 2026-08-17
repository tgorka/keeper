/**
 * The writing tools a markdown buffer gets, wherever that buffer is open
 * (Story 50.3, FR-233).
 *
 * # What moved into here, and why the premise moved with it
 *
 * `text-editor-host.ts` used to record the opposite decision: *"what the note
 * editor adds on top (live preview, wikilinks, the slash menu, the notes store)
 * is markdown-and-a-note specific and deliberately stays there"*. The rule
 * under that sentence — a second editor configuration is how two surfaces end
 * up with different tab behaviour — is right and still holds. Its **premise**
 * is what changed: a session log is markdown a person writes prose into, and
 * FR-233 has promised since phase 7 that a session's text files open with the
 * toolbar, the slash menu and completion. A `kind: "file"` target had none of
 * the three, because the note editor owned the wiring rather than sharing it.
 *
 * So the wiring **moved** rather than being copied. There is one
 * `autocompletion()` in the product over markdown, one place a `/` becomes a
 * menu, one place a `:shortcode:` becomes a character, and one place a toolbar
 * action becomes an edit. A second copy of any of them would be the drift the
 * old sentence existed to prevent, arriving by the other door.
 *
 * # Only the vault-free tools
 *
 * Wikilink and tag completion need vault coordinates, and a sessions zone can
 * never produce them: `keeper-sync`'s own validator refuses a layout in which a
 * sessions zone sits inside a notes vault, in either direction, so
 * `notePathForFile` on a session subpath is `null` in every configuration
 * keeper permits. They arrive as `vaultSources` — the caller's contribution,
 * named at the call site that actually holds a vault id — rather than living
 * here behind a nullable vault argument that one caller could only ever pass
 * `null` to.
 *
 * Live preview and the notes store did **not** move and are not going to. A
 * file has no note id, no subscription and no autosave; a live-preview editor
 * over a manually saved buffer is a different contract, not a shared one.
 *
 * # Why this module is a value import and its callers are lazy
 *
 * Everything here is a runtime `@codemirror/*` value, which is why both callers
 * reach it through their own `import()` — the note editor's boot closure and
 * `mountTextEditor`'s. A static import from either surface would pull the
 * completion sources and the emoji table into the main bundle and defeat the
 * lazy boundary NFR-27 depends on. The file host imports it only when it is
 * actually mounting these tools, so a `.rs` file pays nothing for them.
 */
import { autocompletion, type CompletionSource } from "@codemirror/autocomplete";
import type { Extension } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { emojiCompleteSource, emojiShortcodeCommit } from "./emoji-complete";
import { type FormatAction, formatCommand } from "./format-commands";
import { slashMenuSource } from "./slash-menu";

/**
 * Every writing tool a markdown buffer gets, as one extension.
 *
 * `vaultSources` go **in front of** the shared ones, which is the order the
 * note editor has always offered them in: a wikilink or a tag is a narrower
 * trigger than `/` or `:`, and completion asks the sources in order.
 *
 * One function rather than a list of extensions a caller assembles, because the
 * emoji shortcode filter and the emoji completion source are two halves of one
 * feature (Story 45.11) — a surface that mounted the menu and forgot the filter
 * would offer to complete `:tada:` and then leave the text a person typed in
 * full sitting there as characters.
 */
export function markdownWritingTools(vaultSources: readonly CompletionSource[] = []): Extension {
  return [
    // No keymap of its own: `autocompletion()` installs its own bindings at
    // `Prec.highest`, so Enter accepts a completion and falls through to the
    // host's newline when no menu is open.
    autocompletion({ override: [...vaultSources, slashMenuSource(), emojiCompleteSource()] }),
    // The other half of Story 45.11: a shortcode somebody typed in full becomes
    // its character as the closing colon lands, so `:tada:` never has to be
    // recognised as a menu interaction.
    emojiShortcodeCommit(),
  ];
}

/**
 * Run a toolbar action against a live view.
 *
 * The toolbar sits in the main bundle and speaks in `FormatAction` — plain data
 * — so translating the description into a command is the job of whoever holds
 * the view. That translation is one line, and it is here because two surfaces
 * now need it: the day one of them starts annotating the transaction and the
 * other does not is the day a formatting action is undoable in a note and not
 * in a file.
 *
 * Unannotated, deliberately: a formatting action IS the user's edit, so it
 * belongs in the undo history and it has to reach the host through the update
 * listener that reports edits.
 */
export function runFormatAction(view: EditorView, action: FormatAction): void {
  formatCommand(action)(view);
}
