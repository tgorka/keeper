//! keeper Tauri shell (AD-6) — IPC/plugin/protocol glue only, no business logic.

// The deeply-nested matrix-sdk futures reachable through the async IPC commands
// (e.g. building a room `Timeline` inside `timeline_subscribe`) overflow rustc's
// default type-layout recursion depth; raise it as matrix-sdk recommends.
#![recursion_limit = "256"]

mod ipc;
mod media_protocol;

use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;

/// Application entry point. Registers the plugin set and the typed IPC command
/// surface, then runs the Tauri event loop.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        // Native file-picker for the composer attach button (Story 3.7). Returns
        // OS file paths; Rust reads the file — no media bytes cross IPC.
        .plugin(tauri_plugin_dialog::init())
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::app_ping,
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
            ipc::mark_room_unread,
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
            ipc::delete_account_archive
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
