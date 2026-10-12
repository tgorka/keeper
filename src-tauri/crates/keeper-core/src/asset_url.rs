//! How keeper's webview addresses keeper's own URI schemes — `keeper-media`,
//! `keeper-file`, `keeper-note`, `keeper-recording` — on this build (Epic 98,
//! story 98.3 #3; codemap Q13).
//!
//! # Two spellings, chosen by Tauri, not by keeper
//!
//! Tauri registers a custom protocol differently per operating system. On
//! macOS, iOS and Linux the webview loads `<scheme>://<host>/<path>` as is.
//! Android's WebView and Windows' WebView2 cannot load a custom scheme at all,
//! so Tauri serves the same protocol at `http://<scheme>.localhost/<path>` and
//! intercepts that host; the data that sat in the host slot moves into the
//! first path segment (`tauri/scripts/core.js`, `convertFileSrc`: the
//! `localhost` form exactly when the OS is `windows` or `android`).
//!
//! A URL in the wrong spelling is a request nothing answers: an image that never
//! loads, with nothing in any log. So every keeper URL of these schemes is
//! composed through [`compose`] with this build's [`UrlForm::HOST`], in Rust
//! (`media::media_url`, the shell's note-asset URLs) and in TypeScript
//! (`src/lib/media-src.ts`, fed this same value by the shell's initialization
//! script), and every parser takes either spelling apart through [`strip`].
//!
//! # The one `cfg!` this module holds
//!
//! [`UrlForm::HOST`] names two operating systems. It sits in `keeper-core`
//! rather than in the shell because the composer it feeds is core's (a media
//! URL is built while a timeline item becomes a view model, far from any shell
//! code), and it is the same compile-time fact `agents::mac_tables` already
//! reads for `may_spawn`. It decides a spelling, never a surface: what a build
//! *offers* stays `CapabilitiesVm`'s, computed in the shell (AD-26).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which spelling this build's webview loads keeper's custom schemes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum UrlForm {
    /// `<scheme>://<host>/<path>` — macOS, iOS, Linux.
    Scheme,
    /// `http://<scheme>.localhost/<host>/<path>` — Android, Windows.
    Localhost,
}

impl UrlForm {
    /// This build's spelling: Tauri's own rule, `localhost` exactly on Android
    /// and Windows.
    pub const HOST: UrlForm = if cfg!(any(target_os = "android", target_os = "windows")) {
        UrlForm::Localhost
    } else {
        UrlForm::Scheme
    };
}

/// The URL for `rest` — `<host>/<path>`, already percent-encoded segment by
/// segment — under `scheme`, in `form`.
///
/// Adds no encoding and checks nothing: the caller encodes its segments, and the
/// handler that serves the URL is the one that refuses. The two spellings carry
/// the same `rest` byte for byte, so [`strip`] hands back what went in.
pub fn compose(form: UrlForm, scheme: &str, rest: &str) -> String {
    match form {
        UrlForm::Scheme => format!("{scheme}://{rest}"),
        UrlForm::Localhost => format!("http://{scheme}.localhost/{rest}"),
    }
}

/// `<host>/<path>[?query][#fragment]` of a URL of `scheme` in either spelling,
/// or `None` for any other scheme or host.
///
/// Both spellings on every build, because the handler cannot choose which one
/// the webview sends: Tauri routes a request to it in the spelling the
/// platform's webview uses (the `localhost` one on Android and Windows), and one
/// parser serves every build. `https://<scheme>.localhost/` is accepted too —
/// Tauri's spelling when a build sets `useHttpsScheme`.
pub fn strip<'a>(raw: &'a str, scheme: &str) -> Option<&'a str> {
    let after_scheme = |prefix: &str| {
        raw.strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix(scheme))
    };
    if let Some(rest) = raw
        .strip_prefix(scheme)
        .and_then(|rest| rest.strip_prefix("://"))
    {
        return Some(rest);
    }
    after_scheme("http://")
        .or_else(|| after_scheme("https://"))?
        .strip_prefix(".localhost/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_form_is_tauris_spelling_of_one_url() {
        assert_eq!(
            compose(UrlForm::Scheme, "keeper-file", "01P/a%20b.png"),
            "keeper-file://01P/a%20b.png"
        );
        assert_eq!(
            compose(UrlForm::Localhost, "keeper-file", "01P/a%20b.png"),
            "http://keeper-file.localhost/01P/a%20b.png"
        );
    }

    #[test]
    fn strip_hands_back_what_compose_was_given_in_either_form() {
        for form in [UrlForm::Scheme, UrlForm::Localhost] {
            let url = compose(form, "keeper-note", "01V/x/y.png?v=2");
            assert_eq!(strip(&url, "keeper-note"), Some("01V/x/y.png?v=2"), "{url}");
        }
        assert_eq!(
            strip("https://keeper-note.localhost/01V/y.png", "keeper-note"),
            Some("01V/y.png")
        );
    }

    #[test]
    fn strip_refuses_another_scheme_or_host() {
        for raw in [
            "keeper-file://01P/a.png",
            "http://keeper-file.localhost/01P/a.png",
            "http://keeper-note.localhostx/01P/a.png",
            "http://keeper-note.example/01P/a.png",
            "ftp://keeper-note.localhost/01P/a.png",
            "keeper-note:/01P/a.png",
            "keeper-notes://01P/a.png",
            "http://keeper-notes.localhost/01P/a.png",
            "",
        ] {
            assert_eq!(strip(raw, "keeper-note"), None, "{raw}");
        }
    }
}
