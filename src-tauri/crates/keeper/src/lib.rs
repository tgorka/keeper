//! keeper Tauri shell (AD-6) — IPC/plugin/protocol glue only, no business logic.

// The deeply-nested matrix-sdk futures reachable through the async IPC commands
// (e.g. building a room `Timeline` inside `timeline_subscribe`) overflow rustc's
// default type-layout recursion depth; raise it as matrix-sdk recommends.
#![recursion_limit = "256"]

#[cfg(desktop)]
mod copy_ipc;
mod debug_log;
// The `keeper-file://` asset scheme (Story 45.7). Desktop-only for the same
// reason `note_protocol` is: it serves files out of a synced folder, and the
// folder sync it is rooted in is desktop-only.
#[cfg(desktop)]
mod file_protocol;
#[cfg(desktop)]
mod hotkey;
mod ipc;
mod lifecycle;
mod macos_version;
mod media_protocol;
#[cfg(desktop)]
mod menu;
// The `keeper-note://` asset scheme (AD-59). Desktop-only with the rest of notes:
// a vault is a synced folder, and iOS has no folder sync to put one in.
#[cfg(desktop)]
mod note_protocol;
#[cfg(desktop)]
mod notes_ipc;
#[cfg(desktop)]
mod notes_vault;
#[cfg(desktop)]
mod notes_window;
mod recorder;
// The `keeper-recording://` asset scheme (Story 42.4). Desktop-only for the same
// reason `note_protocol` is: it serves the recordings a desktop records into,
// embedded in notes a desktop syncs.
#[cfg(desktop)]
mod recording_protocol;
#[cfg(desktop)]
mod sync;
#[cfg(desktop)]
mod sync_ipc;
#[cfg(desktop)]
mod tray;
// The zero-egress source-scan audit over the `keeper-rec` sidecar's Swift
// sources (Story 20.4, FR-76) — test-only; it ships no code.
#[cfg(test)]
mod zero_egress;

#[cfg(desktop)]
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Manager;
#[cfg(desktop)]
use tauri::WindowEvent;
use tauri_plugin_deep_link::DeepLinkExt;
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

/// Whether the quit-while-recording confirm was accepted (Story 18.2). Set by
/// the dialog callback right before it re-requests the exit, so the second
/// `ExitRequested` pass skips the confirm and runs the stop→finalize→kill-
/// timeout path. Never reset — the process is exiting once it is set.
#[cfg(desktop)]
static QUIT_CONFIRMED: AtomicBool = AtomicBool::new(false);

/// Whether a quit-while-recording confirm sheet is currently up (Story 18.2),
/// so repeated ⌘Q presses never stack a second sheet. Cleared by the dialog
/// callback (on cancel the next ⌘Q asks again).
#[cfg(desktop)]
static QUIT_CONFIRM_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Serve a `lfs clean|smudge` invocation and exit, if that is what this process
/// was started for (DW-121).
///
/// `Engine::open` registers `filter.lfs.clean`/`smudge` as
/// `std::env::current_exe()`, which in a desktop run is **this** binary. Until
/// now only `keeper-syncd` could answer, so every filter invocation from the app
/// failed — silently, because keeper sets `filter.lfs.required=false` on purpose
/// and git cannot tell a filter that failed from one that was never there. The
/// visible symptom was LFS-tracked files reading as permanently modified under
/// plain `git`, differently on macOS and Linux.
///
/// Called first, before the Tauri builder exists, and returns `false` for every
/// ordinary launch. Two rules make that safe: the parse demands `lfs` as the very
/// first argument (Finder passes none, older macOS passes `-psn_…`), and nothing
/// here writes to stdout except object bytes, because git reads stdout as content.
///
/// Desktop-only, like the `keeper-sync` dependency itself: an iOS build has no
/// folder sync and therefore no filter to be.
#[cfg(desktop)]
fn served_as_lfs_filter() -> bool {
    use keeper_sync::lfs::filter;

    // Skipping argv[0]: the parse wants the verb first, and argv[0] is the path
    // git invoked us by.
    let Some((direction, repo)) = filter::parse_args(std::env::args().skip(1)) else {
        return false;
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    if let Err(err) = filter::run(&repo, direction, &mut stdin.lock(), &mut stdout.lock()) {
        // stderr, never stdout. git shows this in `GIT_TRACE` and on a required
        // filter's failure, and it is the only channel that is not content.
        eprintln!("keeper: lfs filter failed: {err}");
        std::process::exit(1);
    }
    true
}

/// The iOS shell links no sync engine, so it can never be a git filter.
#[cfg(not(desktop))]
fn served_as_lfs_filter() -> bool {
    false
}

/// Whether two paths name the same directory on this machine.
///
/// Component equality first, because it is free and answers almost every case
/// (`Path` already ignores a trailing separator and `.` components). The
/// `canonicalize` pass behind it exists so a *correct* `mainSyncFolder` is
/// never reported as wrong: a sync profile's stored `local_path` and the path
/// the owner typed into `~/.keeper/keeper.toml` routinely differ by a symlink
/// macOS inserted — `/tmp` is `/private/tmp`, an external volume can be
/// reached through more than one mount point — and a *false* "your main folder
/// is not a sync folder" would send someone to fix a file that is already
/// right. Both sides get the same treatment, and a failure on either side
/// (a path that vanished between the two calls) falls back to the plain
/// comparison rather than claiming a match.
///
/// Two `stat`-class syscalls at most, on a path already known to exist. Not a
/// walk: nothing in `setup` may touch a tree before the window is up.
#[cfg(desktop)]
fn same_folder(a: &std::path::Path, b: &std::path::Path) -> bool {
    if a == b {
        return true;
    }
    matches!(
        (std::fs::canonicalize(a), std::fs::canonicalize(b)),
        (Ok(left), Ok(right)) if left == right
    )
}

/// Application entry point. Registers the plugin set and the typed IPC command
/// surface, then runs the Tauri event loop.
///
/// Desktop-only surfaces (tray, global hotkey, autostart, updater, native menu,
/// close-to-hide, Reopen) are gated behind `#[cfg(desktop)]` (Story 12.2): the
/// iOS shell registers only notification, deep-link, dialog, opener, the
/// `keeper-media://` protocol, and the IPC `invoke_handler`. The sequential
/// `builder` rebinding below preserves the exact desktop registration order.
///
/// A `lfs clean|smudge` invocation is served and returns before any of that: git
/// starts this binary as a filter, and a filter must not open a window.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if served_as_lfs_filter() {
        return;
    }
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
        });
    // Vault-local assets — pasted images, embedded attachments, linked files —
    // reach the webview only over this scheme and never as base64 over IPC (AD-58,
    // AD-59). A second handler rather than an overload of `keeper-media://`, which
    // resolves against the Matrix media cache: putting user-file path handling
    // inside the code path that serves decrypted message media would make one bug
    // there worse than two handlers.
    #[cfg(desktop)]
    let builder = builder.register_asynchronous_uri_scheme_protocol(
        "keeper-note",
        |ctx, request, responder| {
            note_protocol::handle(ctx.app_handle().clone(), &request, responder);
        },
    );
    // Recording files — the video a recording note embeds — reach the webview
    // only over this scheme (Story 42.4). A third handler rather than a wider
    // `keeper-note://`, because that one's vault containment is what makes it
    // safe and a recording provably lives outside every vault; this one is
    // contained to the effective recordings destination instead.
    #[cfg(desktop)]
    let builder = builder.register_asynchronous_uri_scheme_protocol(
        recording_protocol::SCHEME,
        |ctx, request, responder| {
            recording_protocol::handle(ctx.app_handle().clone(), &request, responder);
        },
    );
    // Files inside a synced folder — the image, audio or video a panel opens
    // (Story 45.7), and the PDF a document viewer draws (Story 45.8) — reach
    // the webview only over this scheme. A fourth handler rather than a wider
    // `keeper-recording://`, whose root is the recordings destination and which
    // AD-74 forbids the Files surface from reaching, and rather than a wider
    // `keeper-note://`, whose root is the notes SUBFOLDER and therefore narrower
    // than the tree the Files pane browses. This one is contained to the sync
    // profile's own root, through the same `browse::resolve` the listing uses.
    #[cfg(desktop)]
    let builder = builder.register_asynchronous_uri_scheme_protocol(
        file_protocol::SCHEME,
        |ctx, request, responder| {
            file_protocol::handle(ctx.app_handle().clone(), &request, responder);
        },
    );
    let builder = builder
        .setup(|app| {
            // Boot-time settings layers, config import and logging init
            // (Stories 22.5/22.6, 46.6/46.7) — first among the setup steps, and
            // for a sharper reason than "it always was".
            //
            // **Everything a person edited by hand must be in force before
            // anything reads a setting**, and the very next call reads one:
            // `debug_log::init` seeds the on-disk logging gate from
            // `debug.mode`. A layer installed after it would apply to the NEXT
            // boot — which is precisely the boot nobody is trying to debug.
            // Hotkeys (below, ~:290), the tray's `system.menu_bar_presence`
            // (~:325) and the engine's `sync.git_path` (~:445) are the same
            // argument one step further out: every one of them is read inside
            // this `setup`, so every one of them is layered only because the
            // stack is installed here. Hence, in order:
            //
            //   1. `config::load_app_layers` + `config::install` — the TOML
            //      stack (AD-98/AD-99). `registry::get_setting` consults it
            //      ahead of the `settings` table, so installing it is what puts
            //      all ~40 typed getters on layered values at once, each still
            //      clamping its own value exactly as before.
            //   2. `install_folder_tier` — AD-101's phase one, not two: the
            //      folder tier resolves each profile's own `.keeper/*.toml` when
            //      the profile is READ, so it needs only the host label and
            //      which folder is main — both known here — and never `sync.db`.
            //      That is what dissolves the cycle AD-101 predicted.
            //   3. `import_config_file` — unchanged, and now the LOWEST layer.
            //      It still writes `config.json`'s keys into the settings rows,
            //      and a TOML layer setting the same key still wins at read
            //      time. Ordered after the install only so this reads in
            //      precedence order; it reads no setting itself.
            //   4. `debug_log::init` — seeds the gate from the layered view.
            //
            // Every outcome is logged AFTER init, so the subscriber exists to
            // carry it. The faults especially: a settings file that did not
            // load is invisible by construction, and this is the only record of
            // it until someone opens Settings.
            if let Ok(data_dir) = app.state::<ipc::AppState>().platform.data_dir() {
                let host = keeper_core::config::read_host_label();
                // No `HOME`, no user-global layer — and therefore no main
                // folder either, since only `~/.keeper/` may name one. Not
                // `temp_dir()` as a fallback the way `debug_log::app_log_path`
                // uses one: a log file in `/tmp` is a lost log, but READING
                // settings out of a world-writable directory would let any
                // local process choose this app's hotkeys and sync paths.
                let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                let layers = match home.as_deref() {
                    Some(home) => keeper_core::config::load_app_layers(home, &host),
                    None => keeper_core::config::AppLayers::default(),
                };
                #[cfg(desktop)]
                keeper_sync::profile::install_folder_tier(keeper_sync::profile::FolderTier::new(
                    host,
                    layers.main_folder.clone(),
                ));
                keeper_core::config::install(layers);
                let imported = keeper_core::registry::import_config_file(&data_dir);
                debug_log::init(&data_dir);
                for fault in keeper_core::config::faults() {
                    // `Display`, not `summary()`: the log form keeps `toml`'s
                    // own caret diagram over the offending line, which is the
                    // only thing that locates a typo. The Settings pane takes
                    // the one-line form instead.
                    tracing::error!(fault = %fault, "settings file: this layer was skipped");
                }
                let layered: Vec<String> = keeper_core::config::overrides()
                    .into_iter()
                    .map(|(key, _)| key)
                    .collect();
                if !layered.is_empty() {
                    tracing::info!(keys = ?layered, "settings: these are set by a file");
                }
                if home.is_none() {
                    tracing::warn!(
                        "settings: HOME is unset, so no ~/.keeper/*.toml was read this boot"
                    );
                }
                match imported {
                    Ok(keys) if keys.is_empty() => {}
                    Ok(keys) => {
                        tracing::info!(?keys, "config.json: imported overrides into settings");
                    }
                    Err(error) => {
                        tracing::error!(%error, "config.json: import failed; file skipped");
                    }
                }
            }

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

            // Register the optional OS-global Start/Stop Recording hotkey (Story
            // 20.4): a SECOND, independent binding under `hotkey.recording` —
            // unset by default (nothing registers), and its press handler emits
            // `keeper://recording-hotkey-toggled` instead of toggling the window.
            // Best-effort exactly like the summon install above.
            #[cfg(desktop)]
            hotkey::install_recording(app.handle());

            // Register the optional OS-global Quick Capture hotkey (Story 36.3,
            // FR-101): a THIRD independent binding under `hotkey.capture`, unset
            // by default. Its press handler raises the pre-created capture panel
            // straight from Rust — NFR-27's 300 ms bar has no room for an IPC
            // round trip in front of `show()` — and never touches the main
            // window. Best-effort like the two above: a compositor that hands out
            // no global shortcuts (Wayland) leaves the tray's Quick Capture item
            // and the in-app ⌘⌥K as the two paths that always work.
            #[cfg(desktop)]
            hotkey::install_capture(app.handle());

            // Give the prewarmed capture window the resizability and size its
            // remembered placement asks for (Story 46.15, FR-192).
            //
            // The window is declared `resizable: false` in `tauri.conf.json`
            // and created before anything has read a setting, so it boots
            // locked whatever the person last chose. Without this, someone who
            // unlocked and resized it yesterday finds it today showing an open
            // padlock, at the size they left it, with edges that do not answer
            // — the lock reduced to a label.
            //
            // Here rather than inside `notes_window::show`, deliberately: the
            // hotkey path is `set_position` → `show` → `set_focus` and NFR-27's
            // 300 ms has no room for a sqlite read in front of it. Done once,
            // at boot, it costs that path nothing and is correct for every
            // press afterwards.
            #[cfg(desktop)]
            {
                let placement = app
                    .state::<ipc::AppState>()
                    .platform
                    .data_dir()
                    .and_then(|dir| {
                        keeper_core::registry::get_capture_placement(
                            &dir,
                            keeper_core::capture::DRAFT_CAPTURE_KEY,
                        )
                    })
                    .unwrap_or_else(|error| {
                        tracing::debug!(
                            %error,
                            "notes: no remembered capture placement; the panel boots locked"
                        );
                        keeper_core::capture::Placement::default()
                    });
                notes_window::adopt_placement(
                    app.handle(),
                    keeper_core::capture::DRAFT_CAPTURE_KEY,
                    placement,
                );
            }

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

            // Drive the tray's recording rendering (Story 18.1): a ~1 Hz tick
            // reads the authoritative `RecordingStatusVm` snapshot and hands it
            // to the tray, which renders record-dot icon + live status line
            // within 1 s of a start and restores the idle tray on any terminal.
            // No explicit `run_on_main_thread` needed — every tray/menu call
            // dispatches to the main thread internally (and the tray module
            // never holds its lock across such a dispatch). With no tray
            // present the tick is a silent no-op; it never forces presence
            // (Story 18.2's concern).
            #[cfg(desktop)]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
                    // A stalled main thread must not queue a burst of catch-up
                    // renders afterwards.
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    loop {
                        tick.tick().await;
                        let snapshot = {
                            let state = handle.state::<ipc::AppState>();
                            ipc::recording_snapshot(&state)
                        };
                        tray::apply_recording_state(&handle, &snapshot);
                        // Sync renders straight after recording and only into
                        // the gap it leaves: `apply_sync_state` returns
                        // immediately while a recording line or error hold is
                        // installed, so the two never fight over one icon.
                        //
                        // The tick still runs at 1 Hz even though no glyph
                        // animates any more: it is what polls the engine for a
                        // state change at all, and `apply_sync_state` writes only
                        // on a transition.
                        let (sync_state, sync_line) = ipc::sync_tray_snapshot(&handle);
                        tray::apply_sync_state(&handle, sync_state, &sync_line);
                        // Notes rides the same clock, deliberately (AD-62): a 1 Hz
                        // tick has exactly the resolution a 2 s idle-commit needs,
                        // and two schedulers over one git repository is how you get
                        // concurrent index locks. The tray labels and the cadence
                        // are both evaluated here, and each writes only on a
                        // change.
                        tray::apply_notes_state(&handle, &notes_ipc::tray_snapshot(&handle));
                        notes_vault::cadence_tick();
                    }
                });
            }

            // Check + request notification permission OFF the main thread. On iOS
            // both calls round-trip through `run_mobile_plugin`, which parks the
            // calling thread on a channel until the Swift side responds — and
            // `request_permission()` only responds once the user answers the OS
            // permission dialog. Running it inline here (setup runs on the UIKit
            // main thread) starved the main run loop, so the WKWebView custom-
            // protocol handler could never serve index.html: the app sat on a
            // white screen until the dialog was answered (first-boot hang).
            // A detached thread keeps boot non-blocking on every platform; the
            // request stays best-effort exactly as before — a failure only means
            // the OS drops notifications, it never blocks startup.
            {
                use tauri_plugin_notification::NotificationExt;
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let notifier = handle.notification();
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
                });
            }

            // Startup recovery of orphaned segments (Story 17.3, FR-73, AD-37).
            // A recorder/keeper/power crash leaves a session's `manifest.json`
            // frozen at `status:"recording"`; this best-effort pass reconciles
            // each such session from its on-disk segments and marks it
            // `recovered` (remux-free) — the durable signal Story 20.3 reads.
            // It runs on a detached thread (like the notification-permission
            // request above) so a slow/large recordings volume NEVER stalls
            // boot; `AppState`'s live-folder reservation + recovery-scan lock
            // make it safe against a concurrent `recording_start` even
            // seconds after boot. A failure is logged only, never fatal.
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let state = handle.state::<ipc::AppState>();
                    ipc::recover_orphaned_recordings(state.inner());
                });
            }

            // Drive folder sync (Epics 26/29). Until this existed the app built
            // the engine but never ran its loop, so a folder configured in
            // Settings sat still until someone pressed "Sync now" and no remote
            // change ever arrived on its own — continuous sync existed only in
            // `keeper-syncd`. Started after the state is managed so the platform
            // port is live; a machine without git has no sync capability and the
            // call returns quietly. Stopped on the quit path so an in-flight push
            // aborts resumably rather than mid-write.
            #[cfg(desktop)]
            {
                let state = app.state::<ipc::AppState>();
                sync::start_supervisor(std::sync::Arc::clone(&state.platform));
            }

            // AD-101's phase two: the one fact about the layer stack that
            // could not be checked until the engine existed.
            //
            // **`mainSyncFolder` is the hinge of the entire shared layer, and
            // a wrong one fails silently.** Phase one can prove the path
            // exists and is a directory; it cannot prove it is a SYNC folder,
            // because that lives in `sync.db`, which the supervisor above has
            // just opened. So a typo that happens to name a real directory
            // reads two files that are not there, finds nothing, and disables
            // `<main>/.keeper/keeper.toml` and `keeper.<host>.toml` whole —
            // looking, from outside, exactly like a configuration that works.
            // That is the worst failure mode this story can have, so it is
            // checked here and said out loud twice: in the log, and in
            // Settings through `config::push_fault`.
            //
            // It names the fault and changes nothing else. The keys that layer
            // did apply stay applied: unwinding them at this point would mean
            // re-seeding the debug gate, the three hotkeys and the tray from a
            // different stack halfway through boot, which is a worse thing
            // than a folder that is honestly reported as wrong.
            #[cfg(desktop)]
            if let Some(main) = keeper_core::config::main_folder() {
                // No engine means no usable `git`, which the `git` report
                // already says far more usefully than a fault blaming the
                // folder for git's absence would. Nothing to check against.
                if let Some(engine) = sync::engine_if_open() {
                    match engine.list_profiles() {
                        // Disabled profiles count. A paused folder is still a
                        // sync folder whose `.keeper/*.toml` phase one read off
                        // the disk, so its layer really is in force and calling
                        // it missing would be a fault about nothing.
                        Ok(profiles) => {
                            if !profiles.iter().any(|p| same_folder(&p.local_path, &main)) {
                                let fault = keeper_core::config::LayerFault::late(
                                    keeper_core::config::LayerFaultKind::MainFolderNotAProfile,
                                    main.clone(),
                                    "mainSyncFolder names a folder keeper does not sync, so the \
                                     keeper.toml files inside it were not read. Add that folder \
                                     in Settings > Sync, or point mainSyncFolder at a folder you \
                                     already sync.",
                                );
                                tracing::error!(
                                    fault = %fault,
                                    "settings: the shared layer is not in force"
                                );
                                keeper_core::config::push_fault(fault);
                            }
                        }
                        Err(error) => {
                            // An unreadable profile list is the engine's
                            // problem, not the folder's. Saying "your main
                            // folder is wrong" here would send someone to edit
                            // a file that is fine.
                            tracing::warn!(
                                %error,
                                "settings: could not check mainSyncFolder against the sync folders"
                            );
                        }
                    }
                }
            }

            // Build the vault registry and tap the engine's watcher (Epic 35,
            // AD-54/AD-56). After the supervisor, because the tap is the engine's
            // and there is nothing to subscribe to before it exists; a machine
            // with no usable `git` has no engine, therefore no vaults, and this
            // returns quietly — `CapabilitiesVm.notes` is already false there.
            #[cfg(desktop)]
            notes_vault::start(app.handle());

            Ok(())
        });

    // ONE handler registration, always. Tauri's `invoke_handler` ASSIGNS its
    // argument (`self.invoke_handler = Box::new(handler)`) rather than adding to
    // it, so registering a second time silently discards everything the first
    // call registered. Folder sync (Epic 29) is desktop-only and
    // `generate_handler!` takes a flat literal that cannot hold a `#[cfg]` entry,
    // so it was registered in a second pass — which left the desktop build with
    // the nine sync commands reachable and every other command in the app
    // answering "Command <name> not found": no account restore, no capability
    // probe, no recording, no bridges. That shipped as v0.4.0-v0.4.2.
    //
    // Splicing the platform's extra commands into the one list keeps the
    // conditional part out of the literal without ever reassigning the handler.
    // `exactly_one_invoke_handler_is_registered` (ipc.rs) fails the build if a
    // second registration is ever added back.
    macro_rules! keeper_with_commands {
        ($builder:expr $(, $extra:path)* $(,)?) => {
            $builder.invoke_handler(tauri::generate_handler![
                $($extra,)*
                ipc::app_ping,
                ipc::capabilities,
                // Registered on every platform, unlike the rest of the sync
                // surface: the report is what tells a desktop user their `git`
                // is unusable, and a phone gets `unsupported` rather than an
                // `invoke` rejection the frontend would have to special-case.
                ipc::sync_git_status,
                ipc::sync_git_path_set,
                // In the shared body rather than spliced in as a desktop
                // `$extra` (Story 46.7): `~/.keeper/*.toml` is read by
                // `keeper_core::config`, which has no desktop gate, so the
                // question "where did this value come from?" has an honest
                // answer on every platform. A phone answering "Command
                // config_layers not found" would force the Settings surface to
                // special-case a call it can always make.
                ipc::config_layers,
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
                ipc::search_recordings,
                ipc::recording_note_targets,
                ipc::export_start,
                ipc::export_cancel,
                ipc::reveal_path,
                ipc::recording_open_path,
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
                ipc::recording_hotkey_get,
                ipc::recording_hotkey_set,
                ipc::recording_hotkey_clear,
                ipc::recording_reveal_folder,
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
                ipc::menu_bar_presence_set,
                ipc::debug_mode_get,
                ipc::debug_mode_set,
                ipc::debug_log_tail,
                ipc::debug_log_path,
                ipc::titlebar_drag_report,
                ipc::recording_permission,
                ipc::request_screen_recording_permission,
                ipc::request_microphone_permission,
                ipc::request_camera_permission,
                ipc::open_screen_recording_settings,
                ipc::open_microphone_settings,
                ipc::open_camera_settings,
                ipc::recording_list_sources,
                ipc::recording_start,
                ipc::recording_stop,
                ipc::recording_status,
                ipc::recording_acknowledge,
                ipc::recording_settings_get,
                ipc::recording_path_preview,
                ipc::recording_destination_profiles,
                ipc::sync_list_settings_get,
                ipc::sync_list_settings_set,
                ipc::recording_settings_set,
                ipc::recording_session_summary,
                ipc::recording_retitle,
                ipc::recording_session_meta,
                ipc::recording_meta_update,
                ipc::recording_note_stub,
                ipc::recording_note_stub_save,
                ipc::recording_note_stub_dismiss,
                ipc::recovered_sessions_list,
                ipc::recovered_session_acknowledge
            ])
        };
    }

    // The macro takes the builder because `generate_handler!` is generic over the
    // runtime: bound to a bare `let` it has nothing to infer `R` from (E0282).
    // Passing the builder through also keeps the registration itself inside the
    // macro, so it is written once no matter how many platform branches exist.
    #[cfg(desktop)]
    let builder = keeper_with_commands!(
        builder,
        sync_ipc::sync_profiles,
        sync_ipc::sync_statuses,
        sync_ipc::sync_profile_save,
        sync_ipc::sync_profile_remove,
        sync_ipc::sync_profile_set_enabled,
        sync_ipc::sync_folder_now,
        sync_ipc::sync_verify,
        sync_ipc::sync_rescan,
        sync_ipc::sync_open_path,
        // The Files tab's listing command and its actions (Story 43.8; the text
        // read added by Story 45.6). Desktop-only with the rest of sync: a build
        // with no folder sync has no synced folder to browse.
        sync_ipc::sync_browse,
        sync_ipc::sync_open_entry,
        sync_ipc::sync_read_text,
        // The document reader (Story 45.8). Returns a bounded projection of a
        // PDF, DOCX, PPTX or XLSX; the bytes themselves never cross IPC.
        sync_ipc::sync_read_document,
        // Export (Story 45.21). Reads inside the profile, writes outside it, so
        // it is here rather than with the write commands below: it needs no
        // vault and refuses nothing for being outside one.
        sync_ipc::sync_export_entry,
        // And the write half (Story 45.3, AD-89, which retired AD-75). Every
        // one of these goes through `notes_vault`'s single writer and refuses
        // anything outside a notes vault; the decisions are in
        // `keeper_sync::files_write`, and these are the call sites.
        sync_ipc::sync_write_entry,
        sync_ipc::sync_delete_plan,
        sync_ipc::sync_delete_entries,
        sync_ipc::sync_create_entry,
        sync_ipc::sync_subscribe_progress,
        sync_ipc::sync_unsubscribe_progress,
        sync_ipc::sync_activity,
        sync_ipc::sync_pending,
        sync_ipc::sync_problems,
        sync_ipc::sync_retry_parked,
        sync_ipc::sync_set_credential,
        sync_ipc::sync_get_credential,
        sync_ipc::sync_clear_credential,
        sync_ipc::sync_device,
        sync_ipc::sync_device_set_label,
        copy_ipc::copy_start,
        copy_ipc::copy_status,
        copy_ipc::copy_cancel,
        notes_ipc::notes_vaults,
        notes_ipc::notes_vault_flag,
        notes_ipc::notes_vault_settings_save,
        notes_ipc::notes_vault_active,
        notes_ipc::notes_vault_set_active,
        notes_ipc::notes_capture_impact,
        notes_ipc::notes_index_rebuild,
        notes_ipc::notes_list,
        notes_ipc::notes_tag_tree,
        notes_ipc::tags_vocabulary,
        notes_ipc::notes_tree,
        notes_ipc::notes_gallery,
        notes_ipc::notes_spaces,
        notes_ipc::notes_space_save,
        notes_ipc::notes_space_validate,
        notes_ipc::notes_space_terms,
        notes_ipc::notes_spaces_restore_defaults,
        notes_ipc::notes_create,
        notes_ipc::notes_journal_today,
        notes_ipc::notes_templates,
        notes_ipc::notes_templates_restore_defaults,
        notes_ipc::notes_open,
        notes_ipc::notes_close,
        notes_ipc::notes_buffer_report,
        notes_ipc::notes_save,
        notes_ipc::notes_rename,
        notes_ipc::notes_set_flag,
        notes_ipc::notes_set_order,
        notes_ipc::notes_delete_plan,
        notes_ipc::notes_delete,
        notes_ipc::notes_search,
        notes_ipc::notes_link_targets,
        notes_ipc::notes_resolve_link,
        notes_ipc::notes_backlinks,
        notes_ipc::notes_history,
        notes_ipc::notes_diff,
        notes_ipc::notes_mark_read,
        notes_ipc::notes_restore_revision,
        notes_ipc::notes_template_update_preview,
        notes_ipc::notes_template_update_apply,
        notes_ipc::notes_conflicts,
        notes_ipc::notes_resolve_conflict,
        notes_ipc::notes_attachment_paste,
        notes_ipc::notes_attach_sources,
        notes_ipc::notes_attach_targets,
        notes_ipc::notes_body_read,
        notes_ipc::notes_body_write,
        notes_ipc::notes_csv_read,
        notes_ipc::notes_csv_set_cell,
        notes_ipc::notes_embed_paths,
        notes_ipc::notes_embed_read,
        notes_ipc::notes_embed_write,
        notes_ipc::notes_capture_draft,
        notes_ipc::notes_subscribe_changes,
        notes_ipc::notes_unsubscribe_changes,
        notes_ipc::notes_subscribe_index,
        notes_ipc::notes_capture_show,
        notes_ipc::notes_capture_hide,
        notes_ipc::notes_capture_open,
        notes_ipc::notes_capture_close,
        notes_ipc::notes_capture_set_locked,
        notes_ipc::notes_capture_windows,
        notes_ipc::notes_reveal,
        notes_ipc::notes_open_file,
        // Export (Story 45.21). Desktop-only with the rest of the Files surface
        // it shares an engine with; iOS has no folder chooser to pick a
        // destination from and `CapabilitiesVm.notes` is false there.
        notes_ipc::notes_export,
    );
    // The notes surface is desktop-only, but the commands that touch a window
    // or a file manager have `Unsupported` twins so the handler list is identical
    // on every target and `cargo check --target aarch64-apple-ios` stays green
    // (AD-27, AD-33). The rest of the notes commands are absent on iOS by
    // construction: `CapabilitiesVm.notes` is false there, so nothing calls them.
    #[cfg(not(desktop))]
    let builder = keeper_with_commands!(
        builder,
        ipc::notes_capture_show,
        ipc::notes_capture_hide,
        ipc::notes_capture_open,
        ipc::notes_capture_close,
        ipc::notes_capture_set_locked,
        ipc::notes_capture_windows,
        ipc::notes_reveal,
        ipc::notes_open_file,
    );
    // Window-close (⌘W / red button) hides the main window instead of destroying it
    // (Story 10.3, FR-53): the process keeps every account's `SyncService` and the
    // notification pipeline alive so background behavior is byte-for-byte identical to
    // foreground. A real quit goes through `RunEvent::ExitRequested` (below), not here.
    // Desktop-only: on iOS the OS owns app lifecycle — there is no window close.
    #[cfg(desktop)]
    let builder = builder.on_window_event(|window, event| {
        if window.label() == "main" {
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        tracing::warn!(%error, "could not hide main window on close");
                    }
                    // The window going away is the strongest available signal that
                    // the other machine wants these bytes, so it force-flushes every
                    // notes vault (FR-115, AD-62). A commit needs no network; a push
                    // that cannot complete is a journal row (AD-49), so nothing here
                    // can block the close.
                    notes_vault::flush();
                }
                // Losing focus is the weaker version of the same signal, and it is
                // the one that fires when the user switches app without hiding
                // anything. `push_on_blur` decides whether it reaches the network.
                WindowEvent::Focused(false) => notes_vault::flush(),
                _ => {}
            }
            return;
        }
        // A capture window losing focus is when its geometry is written down
        // (Story 45.15, FR-192; the size since Story 46.15). Blur rather than
        // `Moved`/`Resized`, which fire once per compositor frame during a
        // gesture; blur rather than only the close button, because a quit, a
        // crash-free force-quit and a click on another app are all ways a
        // window's last position and size become final without anybody pressing
        // anything.
        if matches!(event, WindowEvent::Focused(false))
            && keeper_core::capture::is_capture_label(window.label())
        {
            let app = window.app_handle();
            if let Some(key) = notes_window::key_for_label(window.label()) {
                let geometry = notes_window::geometry_of(app, &key);
                notes_ipc::remember_placement(
                    app.state::<ipc::AppState>().platform.as_ref(),
                    &key,
                    geometry,
                );
            }
        }
    });
    builder
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            // App-quit (⌘Q / native Quit): gracefully stop every account's sync with a
            // bounded wait, then let the process exit. This is the honest-quit
            // guarantee made mechanical — no hidden background process survives a
            // quit. If shutdown exceeds the bound, still exit (log at warn).
            //
            // Quit-while-recording (Story 18.2) warns FIRST: a live session shows a
            // modal OkCancel confirm before anything stops. The plugin's
            // `blocking_show` must never run on this (main) thread — the sheet's
            // completion needs the main runloop the block would freeze — so the
            // confirm is the async twin of blocking: prevent this exit, show the
            // sheet, and have the callback re-request the exit with
            // `QUIT_CONFIRMED` set; the second pass then runs stop → bounded
            // finalize (→ force-kill on timeout) BEFORE the bounded sync shutdown.
            // Cancel simply leaves the app (and recording) running. With no live
            // recording this arm is byte-for-byte the pre-18.2 quit path.
            tauri::RunEvent::ExitRequested { api, .. } => {
                // iOS never records (the recorder is honestly `Unsupported`), so the
                // desktop-only quit guard below is the only consumer of `api`.
                #[cfg(not(desktop))]
                let _ = api;
                let state = app_handle.state::<ipc::AppState>();
                #[cfg(desktop)]
                if ipc::recording_snapshot(&state).state.is_live() {
                    if !QUIT_CONFIRMED.load(Ordering::Relaxed) {
                        api.prevent_exit();
                        if QUIT_CONFIRM_IN_FLIGHT.swap(true, Ordering::Relaxed) {
                            // A confirm sheet is already up — never stack a second.
                            return;
                        }
                        // The confirm attaches to the main window as a sheet — raise
                        // the window first so a quit from the tray (window hidden)
                        // shows a visible dialog, never an invisible stuck quit.
                        tray::show_main_window(app_handle);
                        let handle = app_handle.clone();
                        app_handle
                            .dialog()
                            .message(
                                "Recording in progress — quit will stop and finalize \
                                 the current segment",
                            )
                            .title("Quit keeper?")
                            .buttons(MessageDialogButtons::OkCancel)
                            .show(move |confirmed| {
                                QUIT_CONFIRM_IN_FLIGHT.store(false, Ordering::Relaxed);
                                if confirmed {
                                    QUIT_CONFIRMED.store(true, Ordering::Relaxed);
                                    handle.exit(0);
                                }
                            });
                        return;
                    }
                    // Confirmed quit: graceful stop → await finalize under the
                    // kill-timeout → abort the driver (sidecar force-killed via
                    // `kill_on_drop`) if it hangs. Bounded — quit is never stuck.
                    ipc::finalize_recording_for_quit(&state);
                }
                // Flush every notes vault before the supervisor is signalled
                // (FR-115, AD-62): quitting mid-typing must not lose the note. The
                // wait is bounded, and bounding it is safe precisely because the
                // commit already happened locally and an outstanding push is a
                // journal row the next launch drains — the work is queued, not
                // dropped.
                #[cfg(desktop)]
                notes_vault::flush_for_quit();
                // Ask the sync supervisor to stop before the account teardown
                // below: the engine finishes the unit it holds and leaves its
                // journal row intact, so an interrupted push is re-driven next
                // launch instead of being killed mid-write.
                #[cfg(desktop)]
                sync::stop_supervisor();
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
            //
            // `RunEvent::Reopen` is an Apple-platform variant (there is no dock on
            // Linux/Windows), so this arm is gated on macOS specifically rather than on
            // `desktop` — the wider gate does not compile on the Linux desktop target.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                tray::show_main_window(app_handle);
                ipc::emit_notify_navigate(app_handle);
            }
            _ => {}
        });
}
