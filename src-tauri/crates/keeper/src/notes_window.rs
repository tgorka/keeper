//! The quick-capture window (FR-101, NFR-27, AD-60).
//!
//! One extra window, declared statically in `tauri.conf.json`, created hidden at
//! startup and **never destroyed**. That is the whole of NFR-27: the hotkey path
//! is `set_position` → `show` → `set_focus` — three synchronous calls plus one
//! compositor frame — with no webview construction, no bundle load, no React
//! mount and no IPC round trip before the panel is visible. Because the window
//! never unmounts, its `<textarea>` stays focused while hidden, so `show()`
//! reveals an already-focused live DOM node rather than racing an `autoFocus`
//! effect, and a keystroke typed 50 ms after the hotkey has nowhere to go but the
//! buffer.
//!
//! Rust owns show, hide, position and focus. The capture webview asks for nothing
//! about its own window — it has one job and four commands, and its capability
//! file (`capabilities/quick-capture.json`) grants it nothing else.
//!
//! Positioning is monitor-aware and **best-effort by design**: an undecorated
//! always-on-top window cannot place itself on Wayland, so keeper asks and
//! accepts the compositor's answer rather than fighting it (UX-DR43). Nothing in
//! the UI promises a position it cannot deliver.

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewWindow};

/// The static window label. Must match `tauri.conf.json` and the `windows` list
/// in `capabilities/quick-capture.json`, or the panel renders and can invoke
/// nothing.
pub const CAPTURE_LABEL: &str = "quick-capture";

/// Emitted after the panel is shown, so the capture entry point can re-assert
/// focus after a compositor race on Linux. Ids only, per the `keeper://kebab-case`
/// convention — this one carries no payload at all.
pub const CAPTURE_SHOWN_EVENT: &str = "keeper://notes-capture-shown";

/// How far down the focused monitor's work area the panel sits, as a fraction of
/// the monitor height.
///
/// A fifth of the way down rather than centred: a capture panel is a thing you
/// type into and dismiss, and vertical centring puts it exactly where a person's
/// eyes are already busy with whatever they were reading.
const TOP_FRACTION: f64 = 0.2;

/// The quick-capture window, or `None` when it does not exist.
///
/// It is declared statically, so `None` means the config and this module have
/// drifted — logged loudly, because every capture path silently doing nothing is
/// the failure mode that looks like a frontend bug and is not.
fn window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    let window = app.get_webview_window(CAPTURE_LABEL);
    if window.is_none() {
        tracing::warn!(
            label = CAPTURE_LABEL,
            "notes: the quick-capture window is not declared; capture is unavailable"
        );
    }
    window
}

/// Whether the panel is currently visible.
pub fn is_visible<R: Runtime>(app: &AppHandle<R>) -> bool {
    window(app).is_some_and(|window| window.is_visible().unwrap_or(false))
}

/// Position, show and focus the panel (`notes_capture_show`).
///
/// Every step is best-effort and logged: a compositor that refuses the position
/// still gets a visible, focused panel, which is the part that matters.
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = window(app) else {
        return;
    };
    position(&window);
    if let Err(error) = window.show() {
        tracing::warn!(%error, "notes: could not show the capture panel");
        return;
    }
    // `set_focus` after `show`: focusing a hidden window is a no-op on every
    // backend, and the panel exists to receive a keystroke.
    if let Err(error) = window.set_focus() {
        tracing::warn!(%error, "notes: could not focus the capture panel");
    }
    if let Err(error) = app.emit(CAPTURE_SHOWN_EVENT, ()) {
        tracing::warn!(%error, "notes: could not emit the capture-shown event");
    }
}

/// Hide the panel (`notes_capture_hide`). Never destroys it — the window's whole
/// value is that it already exists.
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = window(app) else {
        return;
    };
    if let Err(error) = window.hide() {
        tracing::warn!(%error, "notes: could not hide the capture panel");
    }
}

/// Place the panel horizontally centred on the focused monitor, a fifth of the
/// way down its work area.
///
/// The **focused** monitor, not the primary one: on a two-screen desk the panel
/// has to appear where the user is looking. `current_monitor` reports the monitor
/// the window is on, which for a hidden window is where it was last placed, so
/// the cursor's monitor is preferred when the platform can name it.
fn position<R: Runtime>(window: &WebviewWindow<R>) {
    let monitor = match window.cursor_position() {
        Ok(cursor) => window
            .monitor_from_point(cursor.x, cursor.y)
            .ok()
            .flatten()
            .or_else(|| window.current_monitor().ok().flatten()),
        Err(_) => window.current_monitor().ok().flatten(),
    };
    let Some(monitor) = monitor.or_else(|| window.primary_monitor().ok().flatten()) else {
        // No monitor information at all (a headless session, or a compositor that
        // does not answer). `center: true` in the static config is the fallback,
        // and it is a perfectly good answer.
        tracing::debug!("notes: no monitor geometry; leaving the capture panel where it is");
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    // The work area, not the raw resolution: it excludes the macOS menu bar and
    // a Linux panel, so the offset below is measured against the space a window
    // may actually occupy.
    let area = monitor.work_area();
    let x = area.position.x + centred(area.size.width, size.width);
    let y = area.position.y + offset_from_top(area.size.height, size.height);
    if let Err(error) = window.set_position(PhysicalPosition::new(x, y)) {
        // Wayland refuses this, and that is not a fault: the compositor places
        // the panel and everything else about it is unchanged (UX-DR43).
        tracing::debug!(%error, "notes: the compositor placed the capture panel itself");
    }
}

/// The left edge that centres `window_width` inside `area_width`.
///
/// Saturating and signed, because a panel wider than the monitor — a 560 px panel
/// on a tiny virtual display — must land at the left edge rather than at a
/// negative coordinate that some backends reject outright.
fn centred(area_width: u32, window_width: u32) -> i32 {
    let free = area_width.saturating_sub(window_width);
    i32::try_from(free / 2).unwrap_or(0)
}

/// The top edge, [`TOP_FRACTION`] of the way down, clamped so the panel is always
/// fully on screen.
fn offset_from_top(area_height: u32, window_height: u32) -> i32 {
    let free = area_height.saturating_sub(window_height);
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let wanted = (f64::from(area_height) * TOP_FRACTION) as u32;
    i32::try_from(wanted.min(free)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_is_centred_horizontally_on_its_monitor() {
        assert_eq!(centred(1920, 560), 680);
        assert_eq!(centred(560, 560), 0, "an exact fit sits flush");
    }

    #[test]
    fn a_panel_wider_than_the_monitor_lands_at_the_edge_rather_than_off_it() {
        assert_eq!(
            centred(400, 560),
            0,
            "a negative left edge is refused by some backends"
        );
    }

    #[test]
    fn the_top_offset_is_a_fifth_down_and_never_pushes_the_panel_off_screen() {
        assert_eq!(offset_from_top(1080, 340), 216);
        // A short monitor clamps to the last row that keeps the panel whole
        // rather than placing its title strip below the screen.
        assert_eq!(offset_from_top(400, 340), 60);
        assert_eq!(offset_from_top(340, 340), 0, "an exact fit sits at the top");
        assert_eq!(
            offset_from_top(100, 340),
            0,
            "an impossible fit sits at the top"
        );
    }
}
