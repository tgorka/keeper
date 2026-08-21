//! A rendered page, written beside its source as a PDF (macOS).
//!
//! # Why a hidden window and not a library
//!
//! There is no Rust crate that lays out HTML and CSS to the standard a person
//! comparing the two files would accept, and a PDF that does not look like the
//! page is worse than no PDF: the whole point of the button is that the two stay
//! the same document. So the renderer is the one already on this machine and
//! already drawing that page — the webview.
//!
//! The window is off-screen rather than merely hidden. A hidden window on macOS
//! may not be composited at all, and a webview that has never painted produces
//! an empty PDF; positioned outside the visible frame it lays out and paints
//! exactly as it would in front of somebody.
//!
//! # Why the wait is for an event and not for a timer
//!
//! `createPDF` captures what has been laid out. Fonts arrive late, and a sleep
//! long enough to be safe on a slow machine is a sleep everybody else pays. The
//! page itself says when it is done, once, and this waits for that.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_foundation::{NSData, NSError};
use objc2_web_kit::{WKPDFConfiguration, WKWebView};
use tauri::WebviewWindow;

/// # What this does not do yet
///
/// `createPDF` captures the document as ONE page sized to its content. A
/// fifteen-slide deck comes out as a single very tall page rather than fifteen.
/// It is faithful — every slide is there, in its own design — and it is not
/// paginated. Pagination needs `WKPDFConfiguration::rect` driven once per page,
/// or the `NSPrintOperation` path, and both need a decision about where the
/// breaks go that the document itself does not carry. Measured on
/// `2026-08-10-deck-v8-visual.html`: 304 kB, PDF 1.3, one page.

/// How long the render may take before the attempt is abandoned.
///
/// Generous, because this is one press on a document somebody chose, and a deck
/// with a hundred slides on a cold cache is slow once. Bounded, because a
/// webview that never answers must not leave a window off-screen forever.
pub const RENDER_LIMIT: Duration = Duration::from_secs(120);

/// `report.html` → `report.pdf`, in the same directory.
///
/// The same stem on purpose: the request was for a PDF that can be regenerated
/// after an edit, and a name that follows from the source is what makes the
/// second press overwrite the first rather than pile up `report-2.pdf`.
pub fn pdf_beside(source: &Path) -> PathBuf {
    source.with_extension("pdf")
}

/// Ask a loaded webview for its PDF and hand back the bytes.
///
/// Blocking, and deliberately not called from the main thread: `with_webview`
/// dispatches onto the main thread and the completion handler answers there, so
/// a caller that blocked the main thread waiting for this would be waiting for
/// itself.
/// The codebase's second authorized `unsafe` FFI, on the same terms as the
/// first (`IosPlatform::exclude_from_backup`): function-level
/// `#[allow(unsafe_code)]`, a `// Safety:` line on every block, and no safe
/// binding available to reach it instead. `createPDF` is an Objective-C method
/// with a completion handler; objc2-web-kit exposes it and nothing wraps it.
#[allow(unsafe_code)]
pub fn capture(window: &WebviewWindow) -> Result<Vec<u8>, String> {
    let (tx, rx) = mpsc::sync_channel::<Result<Vec<u8>, String>>(1);
    window
        .with_webview(move |handle| {
            // Safety: on macOS `inner()` is the `WKWebView` this window draws
            // with, for as long as the window is alive — and it is, because the
            // caller holds it across this call.
            let webview: &WKWebView = unsafe { &*handle.inner().cast::<WKWebView>() };
            // `with_webview` runs its closure on the main thread — that is its
            // whole contract — so the marker is available rather than asserted.
            // Taken through the checked constructor anyway: if that contract
            // ever changes, this says so instead of being undefined behaviour.
            let main = MainThreadMarker::new()
                .expect("with_webview runs on the main thread, and WKPDFConfiguration needs it");
            // Safety: a plain allocation; the marker above is the thread
            // requirement it carries.
            let config = unsafe { WKPDFConfiguration::new(main) };
            let completion = RcBlock::new(move |data: *mut NSData, error: *mut NSError| {
                let answer = if data.is_null() {
                    // Safety: null-checked, and only read to be described.
                    let said = unsafe { error.as_ref() }
                        .map(|e| e.localizedDescription().to_string())
                        .unwrap_or_else(|| "the webview returned no PDF and no reason".to_owned());
                    Err(said)
                } else {
                    // Safety: non-null per the API's contract that exactly one
                    // of the two arguments is set.
                    Ok(unsafe { &*data }.to_vec())
                };
                // The receiver is gone only if the caller timed out, which is
                // already reported. Dropping the answer is the right thing then.
                let _ = tx.try_send(answer);
            });
            unsafe {
                webview.createPDFWithConfiguration_completionHandler(Some(&config), &completion)
            };
        })
        .map_err(|err| format!("the window has no webview to render: {err}"))?;

    match rx.recv_timeout(RENDER_LIMIT) {
        Ok(answer) => answer,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "the page did not finish rendering within {}s",
            RENDER_LIMIT.as_secs()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("the render was abandoned before it answered".to_owned())
        }
    }
}

/// The window a document is laid out in, off-screen.
///
/// Off-screen rather than hidden: a hidden window on macOS may never be
/// composited, and a webview that has not painted answers `createPDF` with an
/// empty page. Moved far outside any plausible display instead, where it lays
/// out exactly as it would in front of somebody.
pub const OFFSCREEN: (f64, f64) = (-32000.0, -32000.0);

/// A4 at 96 dpi, which is the width the page is laid out at.
///
/// A number rather than the display's size, because the PDF must not depend on
/// which machine made it: the same document exported on a laptop and on a
/// desktop has to paginate identically or the button is not reproducible.
pub const PAGE_SIZE: (f64, f64) = (794.0, 1123.0);

/// The event the print page emits once its document is laid out.
pub const READY_EVENT: &str = "print:ready";
/// The event it emits instead when there is nothing to lay out.
pub const FAILED_EVENT: &str = "print:failed";

#[cfg(test)]
mod tests {
    use super::*;

    /// The name is the contract: a second press overwrites the first, and a
    /// person looking at the folder sees one pair rather than a pile.
    #[test]
    fn the_pdf_takes_its_source_s_name() {
        assert_eq!(
            pdf_beside(Path::new("/vault/30-fundraising/deck-v8.html")),
            Path::new("/vault/30-fundraising/deck-v8.pdf")
        );
    }

    /// A name with dots in it keeps all of them but the last: `2026-08-10` is a
    /// date, not three extensions.
    #[test]
    fn only_the_last_extension_is_replaced() {
        assert_eq!(
            pdf_beside(Path::new("/v/2026-08-10-deck-v8-visual.html")),
            Path::new("/v/2026-08-10-deck-v8-visual.pdf")
        );
    }
}
