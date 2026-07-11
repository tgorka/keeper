//! keeper Tauri shell (AD-6) — IPC/plugin/protocol glue only, no business logic.

// The deeply-nested matrix-sdk futures reachable through the async IPC commands
// (e.g. building a room `Timeline` inside `timeline_subscribe`) overflow rustc's
// default type-layout recursion depth; raise it as matrix-sdk recommends.
#![recursion_limit = "256"]

#[cfg(desktop)]
mod hotkey;
mod ipc;
mod lifecycle;
mod media_protocol;
#[cfg(desktop)]
mod menu;
#[cfg(desktop)]
mod tray;

use tauri::Manager;
#[cfg(desktop)]
use tauri::WindowEvent;
use tauri_plugin_deep_link::DeepLinkExt;

/// Application entry point. Registers the plugin set and the typed IPC command
/// surface, then runs the Tauri event loop.
///
/// Desktop-only surfaces (tray, global hotkey, autostart, updater, native menu,
/// close-to-hide, Reopen) are gated behind `#[cfg(desktop)]` (Story 12.2): the
/// iOS shell registers only notification, deep-link, dialog, opener, the
/// `keeper-media://` protocol, and the IPC `invoke_handler`. The sequential
/// `builder` rebinding below preserves the exact desktop registration order.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        // Native file-picker for the composer attach button (Story 3.7). Returns
        // OS file paths; Rust reads the file — no media bytes cross IPC.
        .plugin(tauri_plugin_dialog::init());
    // OS-level global summon/hide hotkey (Story 9.4, FR-50). The single accelerator
    // is registered in `setup()` via `hotkey::install`; its press handler toggles
    // the main window on focus.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    // Native OS notifications from the sync loop (Story 10.1, FR-51). The
    // `Platform::notify` port posts through this plugin; the app handle is stored in
    // `setup()` and notification permission is requested best-effort there.
    let builder = builder.plugin(tauri_plugin_notification::init());
    // Opt-in launch-at-login (Story 10.3, FR-53, AD-25). The autostart plugin owns
    // the LaunchAgent state authoritatively; it is off by default and only ever
    // toggled by an explicit user action via the `launch_at_login_set` command.
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // Signed auto-updates (Story 11.2, NFR-12). The updater checks the
        // GitHub-releases `latest.json` endpoint (config in `tauri.conf.json`
        // `plugins.updater`) and verifies every downloaded artifact against the
        // committed minisign public key before installing; `tauri-plugin-process`
        // supplies `relaunch()` for the in-app update flow to restart into the new
        // build. The in-app "check for updates" control drives both from the webview.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());
    let builder = builder
        .manage(ipc::AppState::new())
        // The exclusive decrypted-media transport (Story 3.6, AD-4): decrypted
        // bytes reach the webview only over this Range-capable `keeper-media://`
        // protocol, served from the SDK media cache — never as base64/JSON over
        // IPC. The handler runs the async SDK fetch off-thread.
        .register_asynchronous_uri_scheme_protocol("keeper-media", |ctx, request, responder| {
            media_protocol::handle(ctx.app_handle().clone(), &request, responder);
        })
        .setup(|app| {
            // Forward every incoming `keeper://oauth/callback` deep link to the
            // OAuth-callback registry, which matches it to its in-flight OIDC
            // flow by the `state` query param (Story 2.2). An unmatched / spurious
            // callback is ignored inside `resolve`. The registry lives in the
            // managed `AppState` and is cloned into the `'static` handler.
            let flows = app.state::<ipc::AppState>().oauth_flows.clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let handled = flows.resolve(url.as_str());
                    tracing::debug!(handled, "deep-link: received keeper:// URL");
                }
            });

            // Build + install the native menu bar from the action registry (Story
            // 9.3): standard macOS App/Edit/Window submenus plus one generated
            // submenu per registry category, derived from the same
            // `registry_sections()` the ⌘? cheat sheet renders. No accelerators are
            // bound (the JS hooks own every binding); the shortcut is display-only
            // label text. A menu click routes through `on_menu_event` below.
            #[cfg(desktop)]
            {
                let native_menu = menu::build_menu(app.handle())?;
                app.set_menu(native_menu)?;
                app.on_menu_event(|app, event| {
                    menu::handle_menu_event(app, event.id().as_ref());
                });
            }

            // Register the OS-global summon/hide hotkey (Story 9.4): the persisted-or-
            // default accelerator, whose press handler toggles the main window on focus
            // and emits `keeper://global-hotkey-activated`. Best-effort — a registration
            // failure leaves the app running with `hotkey_get().active = false`.
            #[cfg(desktop)]
            hotkey::install(app.handle());

            // Store the app handle for the desktop notifier port (Story 10.1) so
            // `Platform::notify` can post native notifications from the sync loop, and
            // request notification permission best-effort. A permission failure only
            // means the OS will drop notifications — it never blocks startup.
            ipc::set_notify_app_handle(app.handle().clone());

            // Store the app handle for the desktop dock-badge port (Story 10.3) so
            // `Platform::set_badge_count` can drive the main window's OS dock badge from
            // the Rust-computed cross-account unread/mention aggregate while the window is
            // hidden. Unset before this point → an honest no-op (never a panic).
            ipc::set_badge_app_handle(app.handle().clone());

            // Build the menu-bar (tray) icon at startup only when the persisted opt-in
            // toggle is on (Story 10.3, FR-53). Off by default → no tray. A read failure
            // defaults off (the tray is a convenience, never load-bearing).
            #[cfg(desktop)]
            {
                let state = app.state::<ipc::AppState>();
                let present = state
                    .platform
                    .data_dir()
                    .and_then(|dir| keeper_core::registry::get_menu_bar_presence(&dir))
                    .unwrap_or_else(|error| {
                        tracing::warn!(%error, "tray: could not read menu-bar presence; defaulting off");
                        false
                    });
                if present {
                    tray::set_tray_presence(app.handle(), true);
                }
            }

            {
                use tauri_plugin_notification::NotificationExt;
                let notifier = app.notification();
                match notifier.permission_state() {
                    Ok(tauri_plugin_notification::PermissionState::Granted) => {}
                    Ok(_) => {
                        if let Err(e) = notifier.request_permission() {
                            tracing::warn!(error = %e, "could not request notification permission");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "could not read notification permission state");
                    }
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::app_ping,
            ipc::capabilities,
            ipc::bridge_catalog,
            ipc::bridge_discover,
            ipc::bridge_login_start,
            ipc::bridge_login_submit,
            ipc::bridge_login_cancel,
            ipc::bridge_bot_room,
            ipc::bbctl_availability,
            ipc::bbctl_run_start,
            ipc::bbctl_run_cancel,
            ipc::bridge_resolve_support,
            ipc::resolve_bridge_identifier,
            ipc::bridge_subscribe_health,
            ipc::bridge_unsubscribe_health,
            ipc::demo_subscribe,
            ipc::egress_list,
            ipc::login_password,
            ipc::login_oidc,
            ipc::cancel_oidc,
            ipc::beeper_request_code,
            ipc::login_beeper,
            ipc::cancel_beeper,
            ipc::set_encryption_posture,
            ipc::encryption_posture,
            ipc::edit_history_get,
            ipc::honor_remote_deletions,
            ipc::set_honor_remote_deletions,
            ipc::set_draft,
            ipc::get_draft,
            ipc::delete_draft,
            ipc::list_drafts,
            ipc::mirror_draft,
            ipc::clear_draft_mirror,
            ipc::load_remote_draft,
            ipc::list_pending_drafts,
            ipc::approve_draft,
            ipc::draft_mirror_subscribe,
            ipc::draft_mirror_unsubscribe,
            ipc::search_archive,
            ipc::export_start,
            ipc::export_cancel,
            ipc::reveal_path,
            ipc::room_list_subscribe,
            ipc::room_list_unsubscribe,
            ipc::timeline_subscribe,
            ipc::timeline_unsubscribe,
            ipc::connection_status_subscribe,
            ipc::connection_status_unsubscribe,
            ipc::encryption_status_subscribe,
            ipc::encryption_status_unsubscribe,
            ipc::verification_subscribe,
            ipc::verification_unsubscribe,
            ipc::verification_start,
            ipc::verification_accept,
            ipc::verification_start_sas,
            ipc::verification_confirm,
            ipc::verification_mismatch,
            ipc::verification_cancel,
            ipc::backup_status_subscribe,
            ipc::backup_status_unsubscribe,
            ipc::backup_enable,
            ipc::backup_restore,
            ipc::backup_save_recovery_key,
            ipc::backup_saved_recovery_key,
            ipc::send_text,
            ipc::undo_send_window,
            ipc::set_undo_send_window,
            ipc::hotkey_get,
            ipc::hotkey_set,
            ipc::cancel_held_send,
            ipc::subscribe_outbox,
            ipc::unsubscribe_outbox,
            ipc::send_reply,
            ipc::edit_message,
            ipc::toggle_reaction,
            ipc::resolve_timeline_event_key,
            ipc::delete_message,
            ipc::room_network_label,
            ipc::send_retry,
            ipc::send_attachment_path,
            ipc::send_attachment_bytes,
            ipc::cancel_send,
            ipc::mark_room_read,
            ipc::sync_now,
            lifecycle::app_lifecycle_changed,
            ipc::palette_query,
            ipc::cheat_sheet_sections,
            ipc::release_receipt,
            ipc::coupling_caveats,
            ipc::mark_room_unread,
            ipc::notify_get_preview_enabled,
            ipc::notify_set_preview_enabled,
            ipc::dnd_get_global,
            ipc::dnd_set_global,
            ipc::network_mute_get,
            ipc::network_mute_set,
            ipc::chat_notify_mode_get,
            ipc::chat_notify_mode_set,
            ipc::incognito_get,
            ipc::incognito_get_global,
            ipc::incognito_set_global,
            ipc::incognito_get_account,
            ipc::incognito_set_account,
            ipc::incognito_set_chat,
            ipc::archive_room,
            ipc::unarchive_room,
            ipc::favourite_room,
            ipc::unfavourite_room,
            ipc::get_favorites_collapsed,
            ipc::set_favorites_collapsed,
            ipc::pin_room,
            ipc::unpin_room,
            ipc::reorder_pins,
            ipc::set_space_filter,
            ipc::set_network_filter,
            ipc::set_typing,
            ipc::paginate_backwards,
            ipc::typing_subscribe,
            ipc::typing_unsubscribe,
            ipc::pagination_status_subscribe,
            ipc::pagination_status_unsubscribe,
            ipc::session_restore,
            ipc::inbox_subscribe,
            ipc::inbox_unsubscribe,
            ipc::sign_out,
            ipc::delete_account_archive,
            ipc::dock_badge_mode_get,
            ipc::dock_badge_mode_set,
            ipc::active_chat_set,
            ipc::nav_state_set,
            ipc::nav_state_clear,
            ipc::nav_state_get,
            ipc::notification_permission_state,
            ipc::ios_open_app_settings,
            ipc::ios_sync_disclosure_shown_get,
            ipc::ios_sync_disclosure_shown_set,
            ipc::launch_at_login_get,
            ipc::launch_at_login_set,
            ipc::menu_bar_presence_get,
            ipc::menu_bar_presence_set
        ]);
    // Window-close (⌘W / red button) hides the main window instead of destroying it
    // (Story 10.3, FR-53): the process keeps every account's `SyncService` and the
    // notification pipeline alive so background behavior is byte-for-byte identical to
    // foreground. A real quit goes through `RunEvent::ExitRequested` (below), not here.
    // Desktop-only: on iOS the OS owns app lifecycle — there is no window close.
    #[cfg(desktop)]
    let builder = builder.on_window_event(|window, event| {
        if window.label() == "main" {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    tracing::warn!(%error, "could not hide main window on close; leaving it open");
                }
            }
        }
    });
    builder
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            // App-quit (⌘Q / native Quit): gracefully stop every account's sync with a
            // bounded wait, then let the process exit (never `prevent_exit`). This is the
            // honest-quit guarantee made mechanical — no hidden background process
            // survives a quit. If shutdown exceeds the bound, still exit (log at warn).
            tauri::RunEvent::ExitRequested { .. } => {
                let state = app_handle.state::<ipc::AppState>();
                // A short, bounded graceful shutdown: `shutdown_all` awaits each
                // account's `sync.stop()`. Bounding it keeps quit responsive even if a
                // network teardown hangs.
                let shutdown = async {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        state.accounts.shutdown_all(),
                    )
                    .await
                };
                if tauri::async_runtime::block_on(shutdown).is_err() {
                    tracing::warn!("shutdown_all exceeded the quit bound; exiting anyway");
                }
            }
            // macOS dock-icon click while the window is hidden re-shows + focuses it.
            // Following a notification click (which activates the app), this is the
            // coarse click-through seam (Story 10.4, Option B): summon+focus, then emit a
            // navigate event derived from the KIND of the last recorded notification
            // target so the webview lands on the Inbox (Message) or Bridges (Bridge)
            // view. The kept notification backend has NO per-notification click callback,
            // so this is deliberately coarse — never exact-message routing (deferred to
            // Epic 11).
            #[cfg(desktop)]
            tauri::RunEvent::Reopen { .. } => {
                tray::show_main_window(app_handle);
                ipc::emit_notify_navigate(app_handle);
            }
            _ => {}
        });
}
