/**
 * What every free-text field in keeper turns off (Story 101.3, R418): the
 * system rewriting what a person types — a capital letter they did not type at
 * the start of a message, a word "corrected" into another, an inline
 * prediction.
 *
 * Why these four, measured against WebKit's source (WebKit/WebKit `48c073df`),
 * since the Mac app is a WKWebView:
 *
 * - `spellcheck="false"` — on macOS every automatic substitution WebKit runs
 *   while you type (spelling correction, text replacement, smart quotes and
 *   dashes, and any capitalisation AppKit's checker returns as a correction)
 *   goes through `Editor::markAndReplaceFor`, which returns before asking the
 *   checker when `isSpellCheckingEnabledFor` the field is false. This is the
 *   attribute that stops them on the Mac. Whether macOS's own text input
 *   applies a capital outside WebKit's checker is what the hesperia run
 *   (owed, epic 101) settles.
 * - `autocorrect="off"` — WebKit's `canEnableAutomaticSpellingCorrection`
 *   (ENABLE(AUTOCORRECT), on for every Cocoa platform) reads it for the field,
 *   and iOS's keyboard reads it too.
 * - `autocapitalize="off"` — iOS and iPadOS (WebKit compiles
 *   ENABLE(AUTOCAPITALIZE) out on the Mac, so there it is spellcheck's job).
 * - `writingsuggestions="false"` — the inline predictions of macOS 14+ and
 *   iOS 17+ (the HTML attribute WebKit ships).
 *
 * The shadcn `Input`, `Textarea` and `CommandInput` wear these by default and
 * a caller may still override one; a raw `<input>`/`<textarea>` spreads
 * {@link NO_TEXT_SUBSTITUTION}; a CodeMirror editor adds
 * {@link NO_TEXT_SUBSTITUTION_ATTRIBUTES} through `EditorView.contentAttributes`;
 * a field made outside React — the CSV cell's input, an editable run of a
 * rendered file — takes them from {@link withoutTextSubstitution} (R433).
 */

/** The attributes, as the DOM spells them (CodeMirror's `contentAttributes`). */
export const NO_TEXT_SUBSTITUTION_ATTRIBUTES = {
  autocapitalize: "off",
  autocorrect: "off",
  spellcheck: "false",
  writingsuggestions: "false",
} as const;

/** The same attributes, as React props of an `<input>` or a `<textarea>`. */
export const NO_TEXT_SUBSTITUTION = {
  autoCapitalize: NO_TEXT_SUBSTITUTION_ATTRIBUTES.autocapitalize,
  autoCorrect: NO_TEXT_SUBSTITUTION_ATTRIBUTES.autocorrect,
  spellCheck: false,
  writingsuggestions: NO_TEXT_SUBSTITUTION_ATTRIBUTES.writingsuggestions,
} as const;

/**
 * Put the attributes on `element`, a field made outside React (Story 101.3,
 * R433). `setAttribute`, as every engine reads the attribute and jsdom ignores
 * some of the properties.
 */
export function withoutTextSubstitution(element: HTMLElement): void {
  for (const [name, value] of Object.entries(NO_TEXT_SUBSTITUTION_ATTRIBUTES)) {
    element.setAttribute(name, value);
  }
}
