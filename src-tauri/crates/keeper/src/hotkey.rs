//! OS-global summon/hide hotkey (Story 9.4, FR-50, AD-25).
//!
//! Registers a single OS-level global shortcut through `tauri-plugin-global-shortcut`
//! so keeper can be raised (or hidden) from any app, even while backgrounded or
//! hidden. All accelerator *parsing* and *registration* live here in the shell crate;
//! `keeper-core` only stores the opaque accelerator string (it stays Tauri-free).
//!
//! The press handler is a pure focus-based toggle: pressed while the main window is
//! focused → `hide()`; otherwise unminimize (if needed) → `show()` → `set_focus()` and
//! emit [`HOTKEY_EVENT`], which the frontend uses to switch to the Inbox view and move
//! keyboard focus into the Unified Inbox chat list. Idempotent and safe to press
//! repeatedly. Logs carry accelerator strings only — never message content.

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

/// The event emitted when the hotkey raises the window (Story 9.4). The frontend
/// `use-global-hotkey` hook listens for it to switch to Inbox + focus the chat list.
pub const HOTKEY_EVENT: &str = "keeper://global-hotkey-activated";

/// The shipped default accelerator, mirroring [`keeper_core::registry::DEFAULT_GLOBAL_HOTKEY`].
pub const DEFAULT_HOTKEY: &str = keeper_core::registry::DEFAULT_GLOBAL_HOTKEY;

/// The label of the main window (matches `tauri.conf.json` / the default capability).
const MAIN_WINDOW_LABEL: &str = "main";

/// Parse an accelerator string into a [`Shortcut`], or `None` when it is malformed.
///
/// Pure over the input string — the single place accelerator *parsing* happens.
/// `keeper-core` never calls this; it only stores the opaque string.
pub fn parse(accelerator: &str) -> Option<Shortcut> {
    accelerator.parse::<Shortcut>().ok()
}

/// Return a soft conflict warning when `accelerator` matches a curated common macOS
/// system shortcut, else `None` (Story 9.4). This is a *soft* signal only — assignment
/// still proceeds; the hard signal is an actual OS `register` failure. Pure and
/// case-insensitive over the normalized accelerator string.
pub fn known_conflict(accelerator: &str) -> Option<String> {
    // Normalize so `control+alt+space` and `Control+Alt+Space` compare equal.
    let key = accelerator.to_ascii_lowercase();
    let message = match key.as_str() {
        "meta+space" | "super+space" | "command+space" | "cmd+space" => {
            "May conflict with Spotlight (⌘Space)."
        }
        "meta+tab" | "super+tab" | "command+tab" | "cmd+tab" => {
            "May conflict with the macOS app switcher (⌘Tab)."
        }
        "meta+shift+tab" | "super+shift+tab" | "command+shift+tab" | "cmd+shift+tab" => {
            "May conflict with the reverse macOS app switcher (⌘⇧Tab)."
        }
        "meta+q" | "super+q" | "command+q" | "cmd+q" => "May conflict with Quit (⌘Q).",
        "control+up" | "ctrl+up" => "May conflict with Mission Control (⌃↑).",
        "control+down" | "ctrl+down" => "May conflict with Application Windows (⌃↓).",
        "control+left" | "ctrl+left" => "May conflict with moving one Space left (⌃←).",
        "control+right" | "ctrl+right" => "May conflict with moving one Space right (⌃→).",
        // ⌃Space toggles the macOS input source — a common real conflict, and one
        // token away from the ⌃⌥Space default, so worth flagging explicitly.
        "control+space" | "ctrl+space" => "May conflict with switching the input source (⌃Space).",
        _ => return None,
    };
    Some(message.to_owned())
}

/// The shared global-shortcut press handler: toggle the main window on `Pressed`.
///
/// Named (not an inline closure) so startup [`install`] and the register/restore paths
/// in `hotkey_set` share one definition and cannot silently diverge — a change to the
/// press semantics lives in exactly one place.
pub(crate) fn on_shortcut_event<R: Runtime>(
    app: &AppHandle<R>,
    _shortcut: &Shortcut,
    event: ShortcutEvent,
) {
    if event.state == ShortcutState::Pressed {
        toggle_main_window(app);
    }
}

/// Register the persisted-or-default accelerator with the OS at startup (Story 9.4).
///
/// Reads the stored accelerator (absent ⇒ [`DEFAULT_HOTKEY`]), parses it, and attaches
/// the [`toggle_main_window`] press handler through `on_shortcut`. Best-effort: a read,
/// parse, or OS-registration failure is logged via `tracing` and leaves the app running
/// without a global hotkey (the Settings section then reports `active = false`). Never
/// panics — startup must proceed regardless.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let data_dir = match app.state::<crate::ipc::AppState>().platform.data_dir() {
        Ok(dir) => dir,
        Err(error) => {
            tracing::warn!(%error, "hotkey: could not resolve data dir; global hotkey inactive");
            return;
        }
    };
    let accelerator = match keeper_core::registry::get_global_hotkey(&data_dir) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "hotkey: could not read stored accelerator; using default");
            DEFAULT_HOTKEY.to_owned()
        }
    };
    let Some(shortcut) = parse(&accelerator) else {
        tracing::warn!(
            accelerator,
            "hotkey: stored accelerator is malformed; global hotkey inactive"
        );
        return;
    };
    match app
        .global_shortcut()
        .on_shortcut(shortcut, on_shortcut_event)
    {
        Ok(()) => {
            tracing::info!(accelerator, "hotkey: registered global summon shortcut");
        }
        Err(error) => {
            tracing::warn!(%error, accelerator, "hotkey: OS refused to register global shortcut");
        }
    }
}

/// Toggle the main window on a hotkey press (Story 9.4). Focus is the discriminator:
/// pressed while the main window is focused → `hide()`; otherwise unminimize (if
/// minimized) → `show()` → `set_focus()` and emit [`HOTKEY_EVENT`]. Idempotent and safe
/// to press repeatedly; every step is best-effort (a failure is logged, never panics).
pub fn toggle_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        tracing::warn!("hotkey: main window not found; ignoring toggle");
        return;
    };
    // `is_focused()` (not `is_visible()`): a window can be visible-but-not-focused
    // (another app on top), and the AC says raise in that case — see Design Notes.
    let focused = window.is_focused().unwrap_or(false);
    if focused {
        if let Err(error) = window.hide() {
            tracing::warn!(%error, "hotkey: could not hide main window");
        }
        return;
    }
    // Raise path: unminimize first so a minimized window actually restores.
    if window.is_minimized().unwrap_or(false) {
        if let Err(error) = window.unminimize() {
            tracing::warn!(%error, "hotkey: could not unminimize main window");
        }
    }
    if let Err(error) = window.show() {
        tracing::warn!(%error, "hotkey: could not show main window");
    }
    if let Err(error) = window.set_focus() {
        tracing::warn!(%error, "hotkey: could not focus main window");
    }
    if let Err(error) = app.emit(HOTKEY_EVENT, ()) {
        tracing::warn!(%error, "hotkey: could not emit global-hotkey-activated event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_default_and_common_combos() {
        assert!(parse(DEFAULT_HOTKEY).is_some(), "default must parse");
        assert!(parse("Control+Shift+K").is_some());
        // The plugin's accelerator grammar spells the Command/⌘ key `Super` (or
        // `Command`/`Cmd`) — there is no `Meta` token — so the Spotlight combo parses
        // as `Super+Space`, not `Meta+Space`.
        assert!(parse("Super+Space").is_some());
        assert!(parse("Alt+F4").is_some());
    }

    #[test]
    fn parse_rejects_malformed() {
        assert!(parse("Foo+").is_none(), "trailing plus is malformed");
        assert!(parse("").is_none(), "empty is malformed");
        assert!(
            parse("NotAKey").is_none(),
            "bare unknown token is malformed"
        );
        assert!(parse("Control+").is_none(), "modifier-only is malformed");
    }

    #[test]
    fn known_conflict_warns_on_curated_system_shortcuts() {
        assert!(known_conflict("Meta+Space").is_some(), "Spotlight");
        assert!(known_conflict("Meta+Tab").is_some(), "app switcher");
        assert!(known_conflict("Meta+Q").is_some(), "quit");
        assert!(known_conflict("Control+Up").is_some(), "mission control");
        assert!(known_conflict("Control+Left").is_some(), "spaces left");
        assert!(known_conflict("Control+Space").is_some(), "input source");
        // Case-insensitive: a differently-cased spelling still warns.
        assert!(known_conflict("meta+space").is_some());
    }

    #[test]
    fn known_conflict_none_for_novel_combos() {
        assert!(
            known_conflict(DEFAULT_HOTKEY).is_none(),
            "the default is not a conflict"
        );
        assert!(known_conflict("Control+Shift+K").is_none());
        assert!(known_conflict("Alt+F4").is_none());
    }
}
