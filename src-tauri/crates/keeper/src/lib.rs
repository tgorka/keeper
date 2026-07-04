//! keeper Tauri shell (AD-6) — IPC/plugin/protocol glue only, no business logic.

// The deeply-nested matrix-sdk futures reachable through the async IPC commands
// (e.g. building a room `Timeline` inside `timeline_subscribe`) overflow rustc's
// default type-layout recursion depth; raise it as matrix-sdk recommends.
#![recursion_limit = "256"]

mod ipc;

/// Application entry point. Registers the plugin set and the typed IPC command
/// surface, then runs the Tauri event loop.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ipc::AppState::new())
        .invoke_handler(tauri::generate_handler![
            ipc::app_ping,
            ipc::demo_subscribe,
            ipc::login_password,
            ipc::room_list_subscribe,
            ipc::room_list_unsubscribe,
            ipc::timeline_subscribe,
            ipc::timeline_unsubscribe,
            ipc::connection_status_subscribe,
            ipc::connection_status_unsubscribe,
            ipc::send_text,
            ipc::send_retry
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
