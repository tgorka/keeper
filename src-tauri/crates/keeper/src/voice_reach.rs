//! Reach: the shell's call sites for starting a voice turn while keeper is not
//! in front (Epic 63, Story 63.5, FR-420–FR-422, AD-174, AD-179).
//!
//! **No decisions live here.** Whether a surface exists, what a press does
//! against the turn's current face, and which `keeper://voice/…` link asks
//! for what are all [`keeper_core::voice_reach`]. This module has the three
//! things a call site needs: the one gate every surface reads ([`present`]),
//! the one act every surface performs ([`reach`]), and the deep-link and
//! hotkey plumbing that hands the OS's event to them.
//!
//! # Why none of this needs the window
//!
//! Every path here ends in a plain Rust call into `voice_ipc` — the same
//! functions the webview's commands are — on keeper's own threads, with no
//! IPC round trip and no webview in front of it. The OS delivers the press
//! (a global shortcut is registered with the window server, not with a
//! window), the tray click (the menu-bar item is the system's) and the URL
//! (Launch Services hands `keeper://` to the running process) whatever app is
//! frontmost, and the turn moves in `keeper_core::voice` the moment they
//! land. The tray's label follows the turn from `voice_snapshot` on the same
//! 1 Hz tick that renders recording and sync, so it does not wait on the
//! webview either.

use keeper_core::vm::{HotkeyVm, IpcError};
use keeper_core::voice_reach::{reach_present, reach_verb, ReachAsk, ReachVerb, VoiceFace};
use tauri::AppHandle;
#[cfg(desktop)]
use tauri::State;
use tauri_plugin_deep_link::DeepLinkExt;

use crate::ipc::to_ipc_error;
#[cfg(desktop)]
use crate::ipc::AppState;

/// Whether the reach surfaces exist on this build right now (AD-179): the
/// one runtime answer from `voice_availability`, read where the tray menu is
/// built, where the hotkey is installed, and where a deep link arrives. A
/// probe that could not answer is treated as absence — the frontend's
/// `undefined` rule: a surface is never shown on a question not answered.
pub fn present() -> bool {
    match crate::voice_ipc::voice_availability() {
        Ok(availability) => reach_present(availability.as_ref()),
        Err(error) => {
            tracing::warn!(
                error = %error.message,
                "voice reach: availability unanswered; surfaces absent"
            );
            false
        }
    }
}

/// Perform one ask against the turn as it is now: read the face, decide
/// ([`reach_verb`]), act through the same `voice_ipc` function the webview's
/// command is. A talk against an open turn is a no-op, which is what makes a
/// repeated hotkey press or a twice-fired Shortcut start exactly one turn.
///
/// Guarded by [`present`] on every call rather than only at install, so a
/// stored chord or a link on a build whose port answers `unsupported` does
/// nothing at all — the hotkey is not registered and the tray item is not
/// built there, but a deep link is the OS's to deliver.
pub fn reach(ask: ReachAsk) {
    if !present() {
        tracing::debug!(?ask, "voice reach: unsupported here; ignored");
        return;
    }
    let face = VoiceFace::from(&crate::voice_ipc::voice_snapshot());
    let verb = reach_verb(face, ask);
    tracing::info!(?ask, ?face, ?verb, "voice reach");
    let result = match verb {
        Some(ReachVerb::Start) => crate::voice_ipc::voice_start(),
        Some(ReachVerb::Stop) => crate::voice_ipc::voice_stop(),
        Some(ReachVerb::StopSpeaking) => crate::voice_ipc::voice_stop_speaking(),
        None => Ok(()),
    };
    if let Err(error) = result {
        tracing::warn!(error = %error.message, ?verb, "voice reach: the turn refused");
    }
}

/// Route one deep-link event's URLs (Story 63.5, FR-422). Returns whether
/// the event was voice's, so the caller can hand every other URL on to the
/// OAuth-callback registry untouched. One event asks at most once however
/// many `keeper://voice/talk` URLs it carries, and the ask itself is
/// idempotent against an open turn — so a Shortcut that fires twice, or a
/// person who double-clicks it, starts one turn.
pub fn open_voice_link<'a>(urls: impl IntoIterator<Item = &'a str>) -> bool {
    let Some(ask) = keeper_core::voice_reach::voice_link_ask(urls) else {
        return false;
    };
    reach(ask);
    true
}

/// Install the deep-link handler: voice links are performed here, and every
/// other `keeper://` URL goes to `oauth` — the OIDC callback registry that
/// owned the whole handler before this story. One handler, because the
/// plugin keeps one, and a second `on_open_url` would replace the first.
pub fn install_deep_link(app: &AppHandle, oauth: impl Fn(&str) -> bool + Send + Sync + 'static) {
    app.deep_link().on_open_url(move |event| {
        let urls = event.urls();
        if open_voice_link(urls.iter().map(|url| url.as_str())) {
            tracing::debug!("deep-link: received keeper://voice URL");
            return;
        }
        for url in &urls {
            let handled = oauth(url.as_str());
            tracing::debug!(handled, "deep-link: received keeper:// URL");
        }
    });
}

/// Refuse a hotkey command where the surface does not exist (AD-27): the
/// Settings row that would call it is absent on such a build, so this is the
/// honest answer to a caller that reached it anyway.
#[cfg(desktop)]
fn require_present() -> Result<(), IpcError> {
    if present() {
        Ok(())
    } else {
        Err(to_ipc_error(keeper_core::error::CoreError::Unsupported(
            "voice is not available in this build".to_owned(),
        )))
    }
}

/// The soft conflict warning shown against the voice accelerator: the
/// curated macOS system-shortcut list plus the cross-checks against the
/// summon and recording bindings — the same shape as `recording_hotkey_conflict`.
/// Pure over the accelerator strings.
#[cfg(desktop)]
fn voice_hotkey_conflict(accelerator: &str, summon: &str, recording: &str) -> Option<String> {
    crate::hotkey::known_conflict(accelerator).or_else(|| {
        if accelerator.is_empty() {
            return None;
        }
        if accelerator.eq_ignore_ascii_case(summon) {
            return Some("Conflicts with the Summon keeper hotkey.".to_owned());
        }
        if accelerator.eq_ignore_ascii_case(recording) {
            return Some("Conflicts with the Start / stop recording hotkey.".to_owned());
        }
        None
    })
}

/// Validate a voice accelerator before touching registration: the empty
/// string is rejected — clearing is [`voice_hotkey_clear`] — and anything
/// else must parse.
#[cfg(desktop)]
fn validate_voice_hotkey(
    accelerator: &str,
) -> Result<tauri_plugin_global_shortcut::Shortcut, IpcError> {
    if accelerator.is_empty() {
        return Err(to_ipc_error(keeper_core::error::CoreError::Internal(
            "an empty accelerator cannot be assigned; use voice_hotkey_clear to unset".to_owned(),
        )));
    }
    crate::hotkey::parse(accelerator).ok_or_else(|| {
        to_ipc_error(keeper_core::error::CoreError::Internal(format!(
            "invalid accelerator: {accelerator}"
        )))
    })
}

/// The stored summon and recording chords, for the conflict warning.
#[cfg(desktop)]
fn other_hotkeys(data_dir: &std::path::Path) -> Result<(String, String), IpcError> {
    let summon = keeper_core::registry::get_global_hotkey(data_dir).map_err(to_ipc_error)?;
    let recording = keeper_core::registry::get_recording_hotkey(data_dir).map_err(to_ipc_error)?;
    Ok((summon, recording))
}

/// Build the [`HotkeyVm`] for the voice binding: `isDefault` = unset,
/// `active` = a chord that both parses AND is registered with the OS, and the
/// soft `conflict`. Reuses the summon [`HotkeyVm`] shape.
#[cfg(desktop)]
fn voice_hotkey_vm(
    app: &AppHandle,
    accelerator: String,
    summon: &str,
    recording: &str,
) -> HotkeyVm {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let is_default = accelerator.is_empty();
    let active = !accelerator.is_empty()
        && crate::hotkey::parse(&accelerator)
            .map(|shortcut| app.global_shortcut().is_registered(shortcut))
            .unwrap_or(false);
    let conflict = voice_hotkey_conflict(&accelerator, summon, recording);
    HotkeyVm {
        accelerator,
        is_default,
        active,
        conflict,
    }
}

/// Read the optional OS-global voice hotkey binding (Story 63.5, FR-420):
/// the persisted accelerator (absent ⇒ unset), whether it is registered with
/// the OS, and any soft conflict. `Unsupported` where voice is.
#[cfg(desktop)]
#[tauri::command]
pub fn voice_hotkey_get(app: AppHandle, state: State<'_, AppState>) -> Result<HotkeyVm, IpcError> {
    require_present()?;
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let accelerator = keeper_core::registry::get_voice_hotkey(&data_dir).map_err(to_ipc_error)?;
    let (summon, recording) = other_hotkeys(&data_dir)?;
    Ok(voice_hotkey_vm(&app, accelerator, &summon, &recording))
}

/// Mobile stub for [`voice_hotkey_get`]: no OS-global hotkey on a phone.
#[cfg(not(desktop))]
#[tauri::command]
pub fn voice_hotkey_get() -> Result<HotkeyVm, IpcError> {
    Err(to_ipc_error(keeper_core::error::CoreError::Unsupported(
        "the OS-global voice hotkey is desktop-only".to_owned(),
    )))
}

/// Assign the OS-global voice hotkey (Story 63.5), with `recording_hotkey_set`'s
/// exact validate → unregister-old → register-new → persist → rollback-on-any-
/// failure discipline for the **independent** `hotkey.voice` binding. An empty
/// accelerator is rejected — clearing is [`voice_hotkey_clear`]. Logs carry
/// accelerator strings only.
#[cfg(desktop)]
#[tauri::command]
pub fn voice_hotkey_set(
    app: AppHandle,
    state: State<'_, AppState>,
    accelerator: String,
) -> Result<HotkeyVm, IpcError> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    require_present()?;
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let new_shortcut = validate_voice_hotkey(&accelerator)?;
    let previous = keeper_core::registry::get_voice_hotkey(&data_dir).map_err(to_ipc_error)?;
    let (summon, recording) = other_hotkeys(&data_dir)?;
    let gs = app.global_shortcut();

    if let Some(prev_shortcut) = crate::hotkey::voice_shortcut(&previous) {
        if gs.is_registered(prev_shortcut) {
            if let Err(error) = gs.unregister(prev_shortcut) {
                tracing::warn!(%error, accelerator = %previous, "hotkey: could not unregister old voice binding");
            }
        }
    }

    if let Err(error) = gs.on_shortcut(new_shortcut, crate::hotkey::on_voice_shortcut_event) {
        tracing::warn!(%error, accelerator, "hotkey: OS refused to register voice binding; restoring previous");
        if let Some(prev_shortcut) = crate::hotkey::voice_shortcut(&previous) {
            if let Err(restore_error) =
                gs.on_shortcut(prev_shortcut, crate::hotkey::on_voice_shortcut_event)
            {
                tracing::warn!(%restore_error, accelerator = %previous, "hotkey: could not restore previous voice binding after a failed reassignment");
            }
        }
        return Err(to_ipc_error(keeper_core::error::CoreError::Internal(
            format!("the system refused to register {accelerator}: {error}"),
        )));
    }

    if let Err(error) = keeper_core::registry::set_voice_hotkey(&data_dir, &accelerator) {
        tracing::warn!(%error, accelerator, "hotkey: could not persist voice binding; rolling back to previous");
        if gs.is_registered(new_shortcut) {
            if let Err(unreg_error) = gs.unregister(new_shortcut) {
                tracing::warn!(%unreg_error, accelerator, "hotkey: could not unregister voice binding during rollback");
            }
        }
        if let Some(prev_shortcut) = crate::hotkey::voice_shortcut(&previous) {
            if let Err(restore_error) =
                gs.on_shortcut(prev_shortcut, crate::hotkey::on_voice_shortcut_event)
            {
                tracing::warn!(%restore_error, accelerator = %previous, "hotkey: could not restore previous voice binding after a failed persist");
            }
        }
        return Err(to_ipc_error(error));
    }
    Ok(voice_hotkey_vm(&app, accelerator, &summon, &recording))
}

/// Mobile stub for [`voice_hotkey_set`]: desktop-only. Nothing is
/// validated, registered, or persisted.
#[cfg(not(desktop))]
#[tauri::command]
pub fn voice_hotkey_set(accelerator: String) -> Result<HotkeyVm, IpcError> {
    let _ = accelerator;
    Err(to_ipc_error(keeper_core::error::CoreError::Unsupported(
        "the OS-global voice hotkey is desktop-only".to_owned(),
    )))
}

/// Clear the OS-global voice hotkey back to unset (Story 63.5): unregister
/// the current binding (best-effort) and persist the empty string.
#[cfg(desktop)]
#[tauri::command]
pub fn voice_hotkey_clear(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<HotkeyVm, IpcError> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    require_present()?;
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let previous = keeper_core::registry::get_voice_hotkey(&data_dir).map_err(to_ipc_error)?;
    let (summon, recording) = other_hotkeys(&data_dir)?;
    let gs = app.global_shortcut();
    if let Some(prev_shortcut) = crate::hotkey::voice_shortcut(&previous) {
        if gs.is_registered(prev_shortcut) {
            if let Err(error) = gs.unregister(prev_shortcut) {
                tracing::warn!(%error, accelerator = %previous, "hotkey: could not unregister voice binding on clear");
            }
        }
    }
    keeper_core::registry::set_voice_hotkey(&data_dir, "").map_err(to_ipc_error)?;
    Ok(voice_hotkey_vm(&app, String::new(), &summon, &recording))
}

/// Mobile stub for [`voice_hotkey_clear`]: desktop-only.
#[cfg(not(desktop))]
#[tauri::command]
pub fn voice_hotkey_clear() -> Result<HotkeyVm, IpcError> {
    Err(to_ipc_error(keeper_core::error::CoreError::Unsupported(
        "the OS-global voice hotkey is desktop-only".to_owned(),
    )))
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    #[test]
    fn voice_hotkey_conflict_names_the_binding_it_clashes_with() {
        assert_eq!(
            voice_hotkey_conflict("Control+Alt+Space", "Control+Alt+Space", ""),
            Some("Conflicts with the Summon keeper hotkey.".to_owned())
        );
        // Case-insensitive, like `known_conflict`: one binding to the OS.
        assert_eq!(
            voice_hotkey_conflict("control+shift+r", "Control+Alt+Space", "Control+Shift+R"),
            Some("Conflicts with the Start / stop recording hotkey.".to_owned())
        );
        // The curated system list wins over the cross-checks.
        assert!(voice_hotkey_conflict("Meta+Space", "Meta+Space", "").is_some());
        // Unset clashes with nothing, not even an unset recording chord.
        assert_eq!(voice_hotkey_conflict("", "Control+Alt+Space", ""), None);
        assert_eq!(
            voice_hotkey_conflict("Control+Alt+V", "Control+Alt+Space", ""),
            None
        );
    }

    #[test]
    fn voice_hotkey_set_refuses_empty_and_malformed_before_registration() {
        assert!(
            validate_voice_hotkey("").is_err(),
            "empty is a clear, not a set"
        );
        assert!(validate_voice_hotkey("Foo+").is_err());
        assert!(validate_voice_hotkey("Control+Alt+V").is_ok());
    }

    #[test]
    fn only_the_talk_link_is_voices() {
        // Every other `keeper://` URL is handed on to the OAuth registry.
        assert!(!open_voice_link(["keeper://oauth/callback?state=abc"]));
        assert!(!open_voice_link(std::iter::empty()));
    }
}
