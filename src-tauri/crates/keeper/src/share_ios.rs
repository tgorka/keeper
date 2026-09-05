//! Share out: the phone's reveal (Story 66.3, FR-466, AD-200).
//!
//! The desktop's Files pane reveals a file in Finder (`reveal_path`, behind
//! `CapabilitiesVm.reveal_in_file_manager`). A phone has no Finder and no
//! default-application opener for a file inside its own container; what it
//! has is the share sheet — `UIActivityViewController` — which is where AirDrop,
//! Mail, Files.app and every other app that accepts a document live. So the
//! Files surface offers Share where the desktop offers Reveal, each behind its
//! own flag (`CapabilitiesVm.share_out`), and this module is the whole of
//! Share.
//!
//! # The address is a profile id plus a subpath, never a path
//!
//! `reveal_path` takes an absolute path because Finder needs one and the
//! frontend only ever echoes one Rust composed. Share is held to the stricter
//! rule every other reader on the Files surface follows (AD-65): the webview
//! names a profile and a profile-relative subpath, and [`share_target`] resolves
//! it through [`keeper_sync::browse::resolve`] — the same function `sync_browse`,
//! `sync_open_entry` and `sync_read_document` use, which refuses `..` lexically
//! and refuses a symlink out of the tree after canonicalisation. A webview that
//! wanted to share `/etc/passwd` would first have to persuade the engine to
//! store a profile rooted there. A directory is refused too: the sheet's
//! activities take documents, and handing a folder URL to Mail is a dialog
//! that says nothing useful.
//!
//! # Only the presentation is iOS
//!
//! [`share_target`] compiles and is tested on every target, because a
//! containment rule that only compiles on the phone is a rule nobody can
//! exercise on Linux (AD-55). [`present`] is the UIKit half: it runs on the
//! main thread (`with_webview`'s contract), takes the window's
//! `UIViewController` from tauri's `PlatformWebview::view_controller`, and
//! presents the sheet. On an iPad the sheet is a popover and UIKit aborts a
//! popover with no anchor, so the controller's own view is named as the
//! source — harmless on an iPhone, where the sheet ignores it.
//!
//! Every objc2 call in [`present`] is `unsafe fn`. This file follows the shell
//! crate's rule: one `#[allow(unsafe_code)]` function, a `// SAFETY:` comment on
//! each block citing the Apple contract it relies on, and a row in the audit
//! inventory in `docs/constraints-and-limitations.md`.

use std::path::{Path, PathBuf};

use keeper_core::vm::IpcError;
use keeper_sync::browse;

use crate::ipc::AppState;
use crate::sync_ipc::{engine_of, find_profile, missing_sentence, open_failure, sync_ipc_error};

/// What a Mac answers: Share is the phone's verb, and the desktop has Reveal.
/// Reached only if the frontend offered Share where `share_out` was false,
/// which the capability gate exists to prevent (AD-27).
#[cfg(any(not(target_os = "ios"), test))]
pub const NOT_A_PHONE_SENTENCE: &str =
    "sharing out is the phone's reveal; on this Mac, reveal the file in Finder instead";

/// The sentence for a subpath that resolved to a directory.
pub fn folder_refusal(subpath: &str) -> String {
    format!("{subpath} is a folder; share one of the files inside it instead")
}

/// Where the share sheet is pointed: `subpath` under `root`, contained.
///
/// `Ok(None)` is "resolved fine, nothing there" — the caller words that with
/// the profile's name, which this function does not have. `Err` carries the
/// refusal's own sentence: the escape, or the folder.
pub fn share_target(root: &Path, subpath: &str) -> Result<Option<PathBuf>, String> {
    let resolved = browse::resolve(root, subpath).map_err(|refusal| refusal.to_string())?;
    match resolved {
        Some(path) if path.is_dir() => Err(folder_refusal(subpath)),
        other => Ok(other),
    }
}

/// Hand one file inside a synced folder to the OS share sheet (Story 66.3).
///
/// The phone's counterpart of `reveal_path`, addressed like `sync_open_entry`:
/// a profile id and a profile-relative subpath, re-resolved through the
/// listing's own containment rule. The frontend materializes a pointer before
/// calling this — a share of a virtual entry would hand the sheet 130 bytes of
/// LFS pointer text — and the capability gate keeps the control off every
/// desktop.
///
/// Rejects with: `unsupported` (not a phone), `internal` (no such profile, a
/// subpath that escapes the root, a folder, a file that is no longer on disk,
/// a window with no webview).
#[tauri::command]
pub async fn share_out(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    id: String,
    subpath: String,
) -> Result<(), IpcError> {
    let engine = engine_of(&state)?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    let profile = find_profile(&profiles, &id)?;
    let target = share_target(&profile.local_path, &subpath)
        .map_err(open_failure)?
        .ok_or_else(|| open_failure(missing_sentence(profile, &subpath)))?;
    // A use keeper can observe (Story 56.5, AD-126), as `sync_open_entry` notes
    // one: the release clock for this path moves.
    engine.note_use(&id, &subpath);
    #[cfg(target_os = "ios")]
    {
        present(&window, &target).map_err(open_failure)
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (window, target);
        Err(IpcError {
            code: keeper_core::vm::IpcErrorCode::Unsupported,
            message: NOT_A_PHONE_SENTENCE.to_owned(),
            account_id: None,
            retriable: false,
        })
    }
}

/// Present `UIActivityViewController` for `path` from the window's view
/// controller, on the main thread.
///
/// Returns once the sheet has been *presented*, not once the person has
/// picked an activity: the sheet is modal to the app, and nothing here needs to
/// know what they chose.
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
fn present(window: &tauri::WebviewWindow, path: &Path) -> Result<(), String> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSArray, NSString, NSURL};
    use objc2_ui_kit::{UIActivityViewController, UIViewController};

    let path = path.to_string_lossy().into_owned();
    window
        .with_webview(move |handle| {
            // `with_webview` runs its closure on the main thread — that is its
            // whole contract — so the marker is available rather than asserted.
            // Taken through the checked constructor anyway, as `pdf_export`
            // does: if that contract ever changes, this says so.
            let Some(main) = MainThreadMarker::new() else {
                tracing::warn!("share: with_webview did not run on the main thread");
                return;
            };
            let controller = handle.view_controller();
            if controller.is_null() {
                tracing::warn!("share: the window has no view controller");
                return;
            }
            // SAFETY: on iOS `view_controller()` is the `UIViewController` tao
            // created for this window, alive for as long as the window is —
            // and it is, because the caller holds the window across this
            // call. Null-checked above.
            let controller: &UIViewController = unsafe { &*controller.cast::<UIViewController>() };
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
            let item: Retained<AnyObject> = Retained::into_super(Retained::into_super(url));
            let items = NSArray::from_retained_slice(&[item]);
            // SAFETY: `initWithActivityItems:applicationActivities:` consumes
            // the fresh allocation; the items array holds one `NSURL`, which
            // is a type the sheet documents accepting, and no custom
            // activities are supplied.
            let sheet = unsafe {
                UIActivityViewController::initWithActivityItems_applicationActivities(
                    UIActivityViewController::alloc(main),
                    &items,
                    None,
                )
            };
            // An iPad presents the sheet as a popover, and UIKit aborts a
            // popover with no anchor. The controller's own view is the anchor;
            // an iPhone has no popover controller here and skips this.
            if let Some(popover) = sheet.popoverPresentationController() {
                popover.setSourceView(controller.view().as_deref());
            }
            controller.presentViewController_animated_completion(&sheet, true, None);
        })
        .map_err(|err| format!("the window has no webview to present from: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A profile root with one file, one folder, and a symlink that leaves.
    fn tree() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("folder");
        fs::create_dir_all(root.join("notes")).expect("mkdir");
        fs::write(root.join("notes/plan.md"), b"# plan\n").expect("write");
        fs::write(dir.path().join("outside.txt"), b"secret").expect("write");
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("outside.txt"), root.join("link.txt"))
            .expect("symlink");
        (dir, root)
    }

    #[test]
    fn a_file_inside_the_folder_is_the_target() {
        let (_dir, root) = tree();
        let target = share_target(&root, "notes/plan.md").expect("resolves");
        assert_eq!(
            target,
            Some(
                root.join("notes/plan.md")
                    .canonicalize()
                    .expect("canonical")
            )
        );
    }

    #[test]
    fn a_subpath_that_escapes_is_refused_with_the_listing_sentence() {
        let (_dir, root) = tree();
        let refused = share_target(&root, "../outside.txt").expect_err("escapes");
        assert_eq!(
            refused,
            browse::resolve(&root, "../outside.txt")
                .expect_err("the listing refuses it too")
                .to_string(),
            "share and the listing must word the same escape the same way"
        );
        let absolute = share_target(&root, "/etc/passwd").expect_err("absolute");
        assert!(absolute.contains("/etc/passwd"), "{absolute}");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_folder_is_refused_after_canonicalisation() {
        let (_dir, root) = tree();
        let refused = share_target(&root, "link.txt").expect_err("leaves the tree");
        assert!(refused.contains("link.txt"), "{refused}");
    }

    #[test]
    fn a_folder_is_refused_and_a_missing_file_is_nothing() {
        let (_dir, root) = tree();
        assert_eq!(
            share_target(&root, "notes").expect_err("a folder"),
            folder_refusal("notes")
        );
        assert_eq!(
            share_target(&root, "notes/gone.md").expect("resolves"),
            None
        );
    }

    #[test]
    fn the_desktop_sentence_names_the_verb_it_has_instead() {
        assert!(NOT_A_PHONE_SENTENCE.contains("Finder"));
    }
}
