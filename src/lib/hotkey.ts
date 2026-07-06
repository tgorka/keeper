/**
 * Pure hotkey display + capture helpers (Story 9.4, FR-50).
 *
 * The OS-global summon hotkey is registered and parsed only in Rust; these helpers
 * are the frontend's read-only view of the same accelerator grammar: `formatAccelerator`
 * renders a stored accelerator (e.g. `"Control+Alt+Space"`) as macOS glyphs for the
 * Settings → Shortcuts chips, and `acceleratorFromEvent` turns a captured keydown into
 * an accelerator string the Rust `hotkey_set` command accepts. Both are pure — no IPC,
 * no registration, no ordering logic.
 */

/**
 * The shipped default summon accelerator, mirroring the authoritative Rust constant
 * `keeper_core::registry::DEFAULT_GLOBAL_HOTKEY` (⌃⌥Space). Used by Settings → Shortcuts'
 * "Reset to default" so the literal is not duplicated at the call site; keep it in sync
 * with the Rust default if that ever changes.
 */
export const DEFAULT_GLOBAL_HOTKEY = "Control+Alt+Space";

/** macOS glyph for each modifier token, in canonical display order. */
const MODIFIER_GLYPH: Record<string, string> = {
  control: "⌃",
  ctrl: "⌃",
  alt: "⌥",
  option: "⌥",
  shift: "⇧",
  meta: "⌘",
  super: "⌘",
  command: "⌘",
  cmd: "⌘",
};

/** The canonical macOS render order for modifier glyphs (⌃⌥⇧⌘). */
const GLYPH_ORDER = ["⌃", "⌥", "⇧", "⌘"];

/**
 * Render an accelerator string as macOS key glyphs (Story 9.4). Modifiers become
 * `⌃⌥⇧⌘` in canonical order; the final non-modifier token is title-cased for a
 * single-letter key or shown verbatim otherwise (`"Space"`, `"F4"`). Unknown/empty
 * input renders as-is (best-effort display, never throws).
 *
 * `formatAccelerator("Control+Alt+Space")` → `"⌃⌥Space"`.
 */
export function formatAccelerator(accelerator: string): string {
  const tokens = accelerator.split("+").filter((t) => t.length > 0);
  if (tokens.length === 0) {
    return accelerator;
  }
  const glyphs: string[] = [];
  let key = "";
  for (const token of tokens) {
    const glyph = MODIFIER_GLYPH[token.toLowerCase()];
    if (glyph !== undefined) {
      if (!glyphs.includes(glyph)) {
        glyphs.push(glyph);
      }
    } else {
      // The non-modifier key (last one wins if malformed input has several).
      key = token.length === 1 ? token.toUpperCase() : token;
    }
  }
  glyphs.sort((a, b) => GLYPH_ORDER.indexOf(a) - GLYPH_ORDER.indexOf(b));
  return `${glyphs.join("")}${key}`;
}

/** Map a `KeyboardEvent.code`/`key` to the accelerator key token the Rust grammar
 * accepts, or `null` when the key is a bare modifier (no real key pressed yet). */
function keyToken(event: KeyboardEvent): string | null {
  const { code, key } = event;
  // A bare modifier press has no non-modifier key — reject so capture waits for a full
  // chord (matrix: modifier-only returns null).
  if (
    key === "Control" ||
    key === "Alt" ||
    key === "Shift" ||
    key === "Meta" ||
    key === "AltGraph"
  ) {
    return null;
  }
  // Prefer the physical `code` (layout-independent): `KeyA` → `A`, `Digit1` → `1`,
  // `F1`..`F12` verbatim, `Space` verbatim.
  if (code.startsWith("Key")) {
    return code.slice(3);
  }
  if (code.startsWith("Digit")) {
    return code.slice(5);
  }
  if (/^F\d{1,2}$/.test(code)) {
    return code;
  }
  if (code === "Space") {
    return "Space";
  }
  if (code === "ArrowUp") {
    return "Up";
  }
  if (code === "ArrowDown") {
    return "Down";
  }
  if (code === "ArrowLeft") {
    return "Left";
  }
  if (code === "ArrowRight") {
    return "Right";
  }
  if (code === "Enter" || code === "Tab" || code === "Backspace" || code === "Escape") {
    return code;
  }
  // Fall back to the printable key for anything else (e.g. punctuation), upper-cased
  // for a single character; reject an empty/dead key.
  if (key.length === 1) {
    return key.toUpperCase();
  }
  return null;
}

/**
 * Convert a captured keydown into an accelerator string for `hotkey_set` (Story 9.4),
 * or `null` when the event is not a complete chord: a bare modifier press, an unmappable
 * key, or a modifier-less key (a global hotkey must carry at least one modifier so it
 * cannot hijack a bare key OS-wide). Emits modifier tokens in the same order the Rust
 * grammar renders (`Control+Alt+Shift+Super`) followed by the key token.
 *
 * `acceleratorFromEvent` over `⌃⌥Space` → `"Control+Alt+Space"`; a lone Shift or a bare
 * `K`/`Tab` → `null`.
 */
export function acceleratorFromEvent(event: KeyboardEvent): string | null {
  const key = keyToken(event);
  if (key === null) {
    return null;
  }
  // A global summon hotkey must carry at least one modifier: a modifier-less accelerator
  // (e.g. a bare `Tab`/`Enter`/letter fat-fingered while the capture field is armed)
  // would register OS-wide and hijack that key in every app. Reject it so capture keeps
  // waiting for a real chord rather than binding a bare key (Story 9.4).
  if (!event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey) {
    return null;
  }
  const parts: string[] = [];
  if (event.ctrlKey) {
    parts.push("Control");
  }
  if (event.altKey) {
    parts.push("Alt");
  }
  if (event.shiftKey) {
    parts.push("Shift");
  }
  if (event.metaKey) {
    // The Rust grammar spells the ⌘/Command key `Super` (there is no `Meta` token).
    parts.push("Super");
  }
  parts.push(key);
  return parts.join("+");
}
