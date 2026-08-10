//! Quick-capture windows: what one holds, what it is called, and where it sits
//! (Story 45.15, FR-191, FR-192, UX-DR77).
//!
//! # Why several windows, and why that makes this a module
//!
//! Until this story there was exactly one capture window: a static declaration
//! in `tauri.conf.json`, created hidden at startup and never destroyed
//! (NFR-27). Every fact about it was therefore a constant — one label, one
//! position, one buffer. "Several capture windows at once, each holding its own
//! note" turns all of those into functions of a key, and the moment that
//! happens the shell's window code stops being the right place to decide any of
//! it: the shell does not compile on every developer's machine (AD-55/AD-56),
//! and a placement rule nobody can build is a placement rule nobody can check.
//!
//! So this module owns the decisions and `keeper::notes_window` owns the Tauri
//! calls. Everything here is pure and total.
//!
//! # The key, and why the label is a hash of it
//!
//! A capture window is identified by what it holds — [`DRAFT_CAPTURE_KEY`] for
//! the prewarmed hotkey window, `note:<vault>/<note>` for a window opened on an
//! existing note. That key is what survives a restart, so it is what the
//! remembered placement is stored under.
//!
//! A Tauri window *label*, however, is not free-form: it is matched against the
//! `windows` list in a capability file and it appears in generated identifiers,
//! so a note id containing a space, a slash or a colon cannot be one. The label
//! is therefore a fixed prefix plus a hash of the key — deterministic across
//! restarts (so "is this note already open?" is a `get_webview_window` call and
//! not a table nobody keeps in step), always within the legal charset, and
//! always matching [`CAPTURE_LABEL_GLOB`].
//!
//! **That glob is load-bearing and its failure is silent.** Capability files in
//! this app are window-scoped; a capture window whose label the capability does
//! not cover renders normally and can invoke none of the window plugin
//! permissions it needs — it cannot hide, cannot close, cannot be dragged, and
//! cannot follow a link. The file's own words for that are "looks like a
//! frontend bug and is not". The tests below pin the label to the glob, and
//! `src/test/capture-capability.test.ts` pins the glob to the capability file,
//! so the chain has no unchecked link.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The key of the prewarmed, note-less capture window — the one the global
/// hotkey and the tray item raise.
///
/// A constant rather than a target that carries nothing, because this key is
/// also the storage key of that window's draft and placement, and those rows
/// have to be findable by a reader who only has the string.
pub const DRAFT_CAPTURE_KEY: &str = "draft";

/// The static label of the prewarmed capture window. Must match
/// `tauri.conf.json` exactly, and is deliberately unchanged by this story: the
/// NFR-27 startup path — declared hidden, textarea already focused, never
/// destroyed — is byte-for-byte what it was.
pub const DRAFT_CAPTURE_LABEL: &str = "quick-capture";

/// The prefix every *additional* capture window's label carries.
pub const CAPTURE_LABEL_PREFIX: &str = "quick-capture-";

/// The glob the capability file must list beside [`DRAFT_CAPTURE_LABEL`].
///
/// Kept beside the prefix it is built from, and asserted equal to it below, so
/// the two cannot drift into a capability that covers nothing.
pub const CAPTURE_LABEL_GLOB: &str = "quick-capture-*";

/// What one capture window is holding (FR-191).
///
/// Internally tagged on `kind`, matching [`crate::panels::PanelTargetVm`] and
/// every other tagged view model here, so a TypeScript consumer narrows with a
/// `switch` that can be exhaustive.
///
/// `Note` carries the vault as well as the note for
/// [`crate::panels::PanelTargetVm`]'s reason: a note id is unique only inside
/// its vault, and more than one vault is an ordinary setup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum CaptureTargetVm {
    /// The prewarmed window: whatever note it resolves for itself.
    Draft,
    /// A window opened on a note that already exists.
    Note {
        /// The vault the note lives in.
        vault_id: String,
        /// The note's stable id, which survives a rename (FR-97).
        note_id: String,
    },
}

/// One capture window that exists right now, as every surface sees it (FR-191).
///
/// The `key` is included rather than recomputed by the reader, and that is the
/// point: the frontend compares keys and never builds one, so there is exactly
/// one implementation of the keying rule and it is this file's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CaptureWindowVm {
    /// [`capture_key`] of this window's target.
    pub key: String,
    /// What it holds.
    pub target: CaptureTargetVm,
    /// Whether keeper places it (`true`) or the user does (`false`).
    pub locked: bool,
    /// Whether it is on screen. A hidden draft window is still a window.
    pub visible: bool,
}

/// The storage and lookup key for a capture target.
///
/// Both components are percent-encoded before being joined, so the mapping is
/// injective: without it a vault called `a` holding a note called `b/c` and a
/// vault called `a/b` holding a note called `c` would produce the same key, and
/// two different notes would then share one window, one draft and one
/// remembered position. That is not hypothetical — a note id is derived from a
/// path, and slashes are ordinary in one.
#[must_use]
pub fn capture_key(target: &CaptureTargetVm) -> String {
    match target {
        CaptureTargetVm::Draft => DRAFT_CAPTURE_KEY.to_owned(),
        CaptureTargetVm::Note { vault_id, note_id } => format!(
            "note:{}/{}",
            encode_component(vault_id),
            encode_component(note_id)
        ),
    }
}

/// Percent-encode everything that is not unreserved, matching TypeScript's
/// `encodeURIComponent` for every byte a vault or note id can contain.
///
/// Hand-rolled rather than reaching for `percent_encoding`'s sets because the
/// set has to be *exactly* `encodeURIComponent`'s — the frontend mirror in
/// `src/lib/capture-target.ts` calls that function, and the two are pinned to
/// each other by `capture-key-vectors.json`. A near-miss set produces keys that
/// agree for every ASCII name a developer types by hand and disagree for the
/// first name a user types.
///
/// Byte-wise rather than char-wise for the same reason: `encodeURIComponent`
/// encodes the UTF-8 bytes of a non-ASCII character, one `%XX` each.
fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            out.push(ch);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// The Tauri window label for a capture key.
///
/// The draft keeps its static label; everything else is the prefix plus a
/// 64-bit FNV-1a of the key in lower-case hex. FNV rather than a cryptographic
/// hash because nothing here is a security boundary and a ten-line function
/// with no dependency is easier to keep identical across a decade than a crate
/// version; 64 bits because a person with enough capture windows open to
/// collide has a different problem.
#[must_use]
pub fn capture_label(key: &str) -> String {
    if key == DRAFT_CAPTURE_KEY {
        return DRAFT_CAPTURE_LABEL.to_owned();
    }
    format!("{CAPTURE_LABEL_PREFIX}{:016x}", fnv1a64(key.as_bytes()))
}

/// Whether a window label belongs to the capture family at all.
///
/// Used by the shell's window-event handler, which sees every window's events
/// and must remember the placement of capture windows only.
#[must_use]
pub fn is_capture_label(label: &str) -> bool {
    label == DRAFT_CAPTURE_LABEL || label.starts_with(CAPTURE_LABEL_PREFIX)
}

/// FNV-1a, 64-bit. Constants from the reference specification.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The `capture.html` query string that puts a window on `target`.
///
/// **Composed here and parsed in `src/lib/capture-target.ts`, and there is no
/// composer on that side.** A window is only ever created by Rust, so a second
/// composer in the webview would be a second spelling of a string with one
/// producer — and the two would agree on every ASCII name and disagree on the
/// first one with a space in it, because `URLSearchParams` writes `+` where
/// this writes `%20`. Both decode identically, which is exactly what would make
/// the drift invisible. `capture-key-vectors.json` carries this string for
/// every target and both suites read it.
///
/// Empty for the draft window, so the prewarmed window's URL is byte-for-byte
/// the one `tauri.conf.json` declares and NFR-27's startup path is unchanged.
#[must_use]
pub fn capture_search(target: &CaptureTargetVm) -> String {
    match target {
        CaptureTargetVm::Draft => String::new(),
        CaptureTargetVm::Note { vault_id, note_id } => format!(
            "?vault={}&note={}",
            query_encode(vault_id),
            query_encode(note_id)
        ),
    }
}

/// Percent-encode a query value the way `URLSearchParams` will decode it.
///
/// A narrower unreserved set than [`encode_component`], deliberately: `+` MUST
/// be escaped here, because `URLSearchParams` decodes it as a space. A vault
/// called `a+b` would otherwise open a window holding a note in a vault called
/// `a b`, which resolves to nothing and renders "not found" on a note the
/// person just watched keeper accept.
fn query_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            out.push(ch);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// keeper's own size for a capture window, in **logical** pixels.
///
/// The same numbers `tauri.conf.json` gives the prewarmed window and the same
/// numbers `notes_window::open` builds every other one with, so the second
/// window is the same window and not a differently sized cousin. It lives here
/// rather than in the shell because [`Placement::window_size`] has to answer
/// with it, and that answer is the one thing about a capture window's geometry
/// that can be tested on a machine where the shell does not compile
/// (AD-55/AD-56).
pub const CAPTURE_DEFAULT_SIZE: (u32, u32) = (560, 340);

/// The smallest a capture window may be, in logical pixels.
///
/// Not a taste judgement — a floor derived from what the window still has to
/// hold. The note editor's header is three groups (AD-104): the identity group
/// collapses to nothing by design, but the status group reserves a measured box
/// for `Saved · HH:MM` (~100 px in a 12-hour locale) and the actions group
/// carries an icon button plus a word-labelled menu (~112 px), on top of the
/// row's padding and gaps (~30 px). Below roughly 250 px the actions start
/// leaving the right-hand edge, which is exactly the defect story 46.5 fixed.
/// 320 keeps a usable margin over that and still leaves the title something to
/// truncate into; 240 keeps the chrome strip, the header and more than one line
/// of text.
///
/// Enforced twice on purpose: here, so a remembered row can never restore a
/// window smaller than this, and as `minWidth`/`minHeight` on the window itself,
/// so the compositor refuses the drag before the user gets there.
pub const CAPTURE_MIN_SIZE: (u32, u32) = (320, 240);

/// The word that introduces a size in a persisted placement.
///
/// Tagged rather than positional, because a size has to be storable *without* a
/// position — a window resized but never moved is an ordinary thing — and three
/// optional trailing integers with no tag cannot say which pair is which.
/// It also keeps the old two-token spelling readable verbatim: a row written by
/// the build before this story is `free 120 -40`, and it still decodes to
/// exactly the placement it always did, with no size.
const SIZE_TAG: &str = "size";

/// Where a capture window sits, how big it is, and who decides (FR-192,
/// UX-DR77).
///
/// # Why `locked` defaults to true
///
/// Locked is what keeper has always done: place the panel a fifth of the way
/// down the monitor holding the pointer, every time it shows. A person who has
/// never touched the lock must see exactly that, so the absent row and the
/// default value have to mean the same thing — which is why [`Placement::decode`]
/// is total and answers [`Placement::default`] for anything it cannot read.
///
/// # Why a locked window can still carry a position
///
/// Unlock, drag, lock again is a person saying "keep it *there*". Throwing the
/// position away on lock would make the lock a discard button, so the two
/// fields are independent: `locked` decides who may move it, `position` is the
/// last place it was put.
///
/// # Why the size is here too, and why it is logical pixels
///
/// The lock's promise is now *both* verbs: unlocked, a window may be moved
/// **and** resized. A size that did not survive a restart would make the resize
/// the only gesture in this app that keeper watches and forgets, so `size`
/// joins `position` under the same key, with the same "locking is not a discard
/// button" rule.
///
/// `position` is physical pixels and `size` is logical ones, and that asymmetry
/// is deliberate rather than an oversight. A position is a point on a desktop
/// that may span monitors of different scale factors, and only the physical
/// coordinate names one unambiguously. A size is a statement about how much
/// *content* fits, which is what a logical pixel measures: restore a physical
/// 1120 on a monitor that has meanwhile gone from 2× to 1× and the person gets
/// a window twice the size they left. The shell converts once, at the window it
/// is reading or writing, and both directions use the same quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// `true` — keeper places and sizes the window and the user can do neither.
    /// `false` — the user moves and resizes it and keeper remembers both.
    pub locked: bool,
    /// The remembered top-left in physical pixels, or `None` for "keeper's
    /// automatic placement", which is where a window that has never been moved
    /// starts.
    pub position: Option<(i32, i32)>,
    /// The remembered inner size in logical pixels, or `None` for "the window
    /// has never been resized", which is not the same as
    /// [`CAPTURE_DEFAULT_SIZE`]: see [`Placement::window_size`], where the
    /// difference decides whether a live window is touched at all.
    pub size: Option<(u32, u32)>,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            locked: true,
            position: None,
            size: None,
        }
    }
}

impl Placement {
    /// The persisted spelling: `locked` or `free`, optionally followed by two
    /// integers, optionally followed by `size` and two more.
    ///
    /// A tiny text encoding rather than JSON because the whole value is five
    /// scalars and the settings table stores strings: JSON here would buy
    /// nothing and would make the "unreadable row falls back to the default"
    /// path depend on a parser with its own opinions about numbers.
    #[must_use]
    pub fn encode(&self) -> String {
        let state = if self.locked { "locked" } else { "free" };
        let mut encoded = match self.position {
            Some((x, y)) => format!("{state} {x} {y}"),
            None => state.to_owned(),
        };
        if let Some((width, height)) = self.size {
            encoded.push(' ');
            encoded.push_str(SIZE_TAG);
            encoded.push_str(&format!(" {width} {height}"));
        }
        encoded
    }

    /// Read a persisted placement. **Total**: every unreadable value is the
    /// default, because a settings row written by an older build, truncated, or
    /// hand-edited must cost the user their remembered geometry and never their
    /// window.
    ///
    /// A half-readable position — one coordinate that parses and one that does
    /// not — is *no* position rather than a position with a fabricated axis. A
    /// window placed at `(120, 0)` because the `y` was garbage is a window that
    /// moved somewhere the user never put it.
    ///
    /// The size follows the same rule and adds one of its own: **a zero is not
    /// a size**. `size 0 340` parses perfectly and describes a window with no
    /// width — invisible, unfocusable and unclosable, and on some backends not
    /// creatable at all. It degrades to `None`, which is keeper's own size,
    /// rather than to a window the person cannot get rid of. A negative or
    /// out-of-range number never reaches that check, because it fails to parse
    /// as `u32` first.
    #[must_use]
    pub fn decode(raw: &str) -> Self {
        let mut parts = raw.split_whitespace().peekable();
        let locked = !matches!(parts.next(), Some("free"));
        // Only look for a position when the next word is not the size tag:
        // `free size 900 600` is a window resized but never moved, and reading
        // its first two words as coordinates would consume the tag and lose the
        // size as well.
        let position = if parts.peek().is_some_and(|word| *word != SIZE_TAG) {
            match (parts.next(), parts.next()) {
                (Some(x), Some(y)) => match (x.parse::<i32>(), y.parse::<i32>()) {
                    (Ok(x), Ok(y)) => Some((x, y)),
                    _ => None,
                },
                _ => None,
            }
        } else {
            None
        };
        let size = match (parts.next(), parts.next(), parts.next()) {
            (Some(tag), Some(width), Some(height)) if tag == SIZE_TAG => {
                match (width.parse::<u32>(), height.parse::<u32>()) {
                    (Ok(width), Ok(height)) if width > 0 && height > 0 => Some((width, height)),
                    _ => None,
                }
            }
            _ => None,
        };
        Self {
            locked,
            position,
            size,
        }
    }

    /// The size to give this window right now, in logical pixels, or `None` for
    /// "leave it exactly as it is".
    ///
    /// Three answers, and the third is the one that matters:
    ///
    /// - **Locked → keeper's own size, always.** A locked window is keeper's to
    ///   place and keeper's to size; normalising it on every open is what makes
    ///   the lock mean something, and it is the escape hatch from a size the
    ///   person no longer wants.
    /// - **Unlocked with a remembered size → that size**, clamped.
    /// - **Unlocked with no remembered size → `None`.** Not the default: the
    ///   caller must not touch the window. This is the difference between
    ///   "never resized" and "resized to 560×340", and it is load-bearing — the
    ///   live window may have been dragged to a new size seconds ago by a user
    ///   whose blur has not yet written it down, and re-asserting a size on the
    ///   next open would undo the gesture in front of them.
    ///
    /// `work_area` is the usable area of the monitor the window will appear on,
    /// in logical pixels, or `None` when the platform will not say (a headless
    /// session, or a compositor that does not answer).
    #[must_use]
    pub fn window_size(&self, work_area: Option<(u32, u32)>) -> Option<(u32, u32)> {
        let wanted = match (self.locked, self.size) {
            (true, _) => CAPTURE_DEFAULT_SIZE,
            (false, Some(size)) => size,
            (false, None) => return None,
        };
        Some(clamp_size(wanted, work_area))
    }
}

/// Fit a wanted size inside what the screen can actually show.
///
/// **The clamp lives here rather than in the shell because it is the part that
/// can be wrong.** A window restored 3000 px wide on a 1440 px display is not a
/// cosmetic problem: a capture window is undecorated and `skipTaskbar`, so it is
/// in no dock and no task switcher, its close button is at the right-hand edge,
/// and `centred` in the shell puts an oversized window's left edge at zero — so
/// the controls end up past the far edge of the screen with nothing to click
/// and no window list to reach them from. The same applies to a monitor that
/// went away: the remembered size came from a display this machine may no longer
/// have.
///
/// The floor is applied **before** the ceiling, so when the two disagree the
/// screen wins. That order is the whole decision. A work area smaller than
/// [`CAPTURE_MIN_SIZE`] is a strange display, and the two candidate answers are
/// "a window slightly too small to be comfortable" and "a window whose right
/// edge, where every control is, is off the screen". Reachable beats
/// comfortable.
fn clamp_size(size: (u32, u32), work_area: Option<(u32, u32)>) -> (u32, u32) {
    let (mut width, mut height) = (
        size.0.max(CAPTURE_MIN_SIZE.0),
        size.1.max(CAPTURE_MIN_SIZE.1),
    );
    if let Some((area_width, area_height)) = work_area {
        // A zero-sized work area is not information — some backends report one
        // for a monitor that is being reconfigured — and clamping to it would
        // produce the invisible window `decode` refuses to build.
        if area_width > 0 {
            width = width.min(area_width);
        }
        if area_height > 0 {
            height = height.min(area_height);
        }
    }
    (width, height)
}

/// What closing a capture window must actually do (FR-191).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosePlan {
    /// `true` — destroy the window. `false` — hide it and keep it alive.
    pub destroy: bool,
    /// `true` — show the main window afterwards, because nothing else is left
    /// on screen.
    pub raise_main: bool,
}

/// Decide how to close the capture window `key`, given whether any *other*
/// window is currently visible.
///
/// Two rules, and both exist to prevent a specific stranding:
///
/// - **The draft window is hidden, never destroyed.** Its entire value is that
///   it already exists: destroying it would turn the next hotkey press into a
///   webview construction and a bundle parse, which is the 300 ms NFR-27 is
///   about. Story 45.15 adds a close button, and a close button that quietly
///   costs the next capture its speed is a regression wearing a feature's
///   clothes.
/// - **Closing the last visible window raises the main one.** A capture window
///   is undecorated and `skipTaskbar`, so it is in no dock and no task
///   switcher, and Story 10.3 makes "main window hidden to the tray" an
///   ordinary state. Destroy the only thing on screen in that state and the app
///   is still running with no visible surface — and on a desktop with no system
///   tray there is nothing left to click. Raising main is the recovery, and
///   doing it *only* when nothing else is visible is what stops it stealing
///   focus every time a person closes one of three open captures.
#[must_use]
pub fn plan_close(key: &str, other_windows_visible: bool) -> ClosePlan {
    ClosePlan {
        destroy: key != DRAFT_CAPTURE_KEY,
        raise_main: !other_windows_visible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS_JSON: &str = include_str!("capture-key-vectors.json");

    #[derive(Deserialize)]
    struct Vector {
        target: CaptureTargetVm,
        key: String,
        search: String,
    }

    /// Story 45.15: the key is built here and mirrored in
    /// `src/lib/capture-target.ts`, and the two never meet at runtime. This
    /// table is the only thing that can stop them drifting — and a drift is not
    /// cosmetic: the frontend would ask about a window Rust stores under a
    /// different name, so every remembered position would be silently lost and
    /// nothing would look broken until someone tried to reproduce it.
    #[test]
    fn every_shared_vector_keys_exactly_as_the_typescript_mirror_expects() {
        let vectors: Vec<Vector> = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(
            vectors.len() >= 6,
            "a table someone empties makes both suites pass while the two languages agree about nothing"
        );
        for vector in vectors {
            assert_eq!(capture_key(&vector.target), vector.key);
            // The window URL travels the same seam and has the same failure:
            // Rust writes it, the webview reads it, and a mismatch shows up as
            // a window that renders "not found" on a note keeper just accepted.
            assert_eq!(capture_search(&vector.target), vector.search);
        }
    }

    /// The reason the components are encoded at all. Without it these two
    /// distinct notes produce one key, and one of them silently inherits the
    /// other's window, draft and remembered position.
    #[test]
    fn a_slash_inside_an_id_cannot_impersonate_the_separator() {
        let ambiguous = capture_key(&CaptureTargetVm::Note {
            vault_id: "a".into(),
            note_id: "b/c".into(),
        });
        let other = capture_key(&CaptureTargetVm::Note {
            vault_id: "a/b".into(),
            note_id: "c".into(),
        });
        assert_ne!(ambiguous, other);
        assert_eq!(ambiguous, "note:a/b%2Fc");
        assert_eq!(other, "note:a%2Fb/c");
    }

    /// `+` is the character whose mis-encoding is invisible: it survives a
    /// naive encoder and `URLSearchParams` then decodes it as a space.
    #[test]
    fn a_query_value_survives_url_search_params_decoding_it() {
        assert_eq!(query_encode("a+b"), "a%2Bb");
        assert_eq!(query_encode("a b"), "a%20b");
        assert_eq!(query_encode("a&b=c"), "a%26b%3Dc");
        assert_eq!(query_encode("plain-Name_1.md~"), "plain-Name_1.md~");
        assert_eq!(capture_search(&CaptureTargetVm::Draft), "");
    }

    #[test]
    fn the_draft_key_is_the_draft_label_and_nothing_else_is() {
        assert_eq!(capture_key(&CaptureTargetVm::Draft), DRAFT_CAPTURE_KEY);
        assert_eq!(capture_label(DRAFT_CAPTURE_KEY), DRAFT_CAPTURE_LABEL);
        assert!(is_capture_label(DRAFT_CAPTURE_LABEL));
        assert!(!is_capture_label("main"));
        assert!(!is_capture_label("quick-captureX"));
        // The prefix EMBEDDED in a foreign label is not a capture window. A
        // substring test here would make the shell's window-event handler write
        // a capture placement row for somebody else's window, under a capture
        // key that window does not have.
        assert!(!is_capture_label("main-quick-capture-0000000000000000"));
        assert!(!is_capture_label("settings/quick-capture-1"));
    }

    /// The silent failure this whole naming scheme exists to avoid: a label the
    /// capability file does not cover renders a window that can invoke no
    /// window permission at all.
    #[test]
    fn every_derived_label_is_covered_by_the_capability_glob() {
        assert_eq!(
            CAPTURE_LABEL_GLOB,
            format!("{CAPTURE_LABEL_PREFIX}*"),
            "the glob and the prefix must be built from the same string"
        );
        for key in [
            "note:vault-a/note-1",
            "note:%C3%A9/%20",
            "note:a/b%2Fc",
            "something-nobody-planned",
        ] {
            let label = capture_label(key);
            assert!(
                label.starts_with(CAPTURE_LABEL_PREFIX),
                "{label} is not covered by {CAPTURE_LABEL_GLOB}"
            );
            assert!(is_capture_label(&label));
            assert!(
                label[CAPTURE_LABEL_PREFIX.len()..]
                    .chars()
                    .all(|ch| ch.is_ascii_hexdigit()),
                "{label} carries a character a Tauri label may not"
            );
        }
    }

    /// Two windows are two windows: different keys must not share a label, or
    /// the second `open` finds the first and retargets it.
    #[test]
    fn two_different_notes_get_two_different_labels() {
        let first = capture_label(&capture_key(&CaptureTargetVm::Note {
            vault_id: "vault-a".into(),
            note_id: "note-1".into(),
        }));
        let second = capture_label(&capture_key(&CaptureTargetVm::Note {
            vault_id: "vault-a".into(),
            note_id: "note-2".into(),
        }));
        assert_ne!(first, second);
        assert_ne!(first, DRAFT_CAPTURE_LABEL);
        assert_ne!(second, DRAFT_CAPTURE_LABEL);
    }

    /// A label has to survive a quit: it is how "is this note already open?" is
    /// answered after a restart, so it may not depend on process state.
    #[test]
    fn a_label_is_the_same_in_every_process() {
        assert_eq!(capture_label("note:v/n"), capture_label("note:v/n"));
        // Pinned to a LITERAL, and deliberately not recomputed from `fnv1a64`
        // here: the contract is that this label is the same after a REBUILD,
        // and an assertion that recomputes the hash moves with any change to it
        // and therefore asserts nothing. Change this number and every window's
        // remembered placement is orphaned under a name nothing looks up.
        assert_eq!(capture_label("note:v/n"), "quick-capture-da85850f1ff52f94");
        assert_eq!(
            capture_label("draft-but-not-the-draft"),
            "quick-capture-d2698e7277fe5846"
        );
    }

    #[test]
    fn a_placement_nobody_has_touched_is_keepers_own_placement() {
        let default = Placement::default();
        assert!(default.locked);
        assert_eq!(default.position, None);
        assert_eq!(default.size, None);
        assert_eq!(Placement::decode(""), default);
    }

    #[test]
    fn a_placement_round_trips_through_its_persisted_form() {
        for placement in [
            Placement {
                locked: true,
                position: None,
                size: None,
            },
            Placement {
                locked: false,
                position: None,
                size: None,
            },
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: None,
            },
            // Unlock, drag, lock again: the position is kept, because locking
            // means "keep it there" and not "forget where I put it".
            Placement {
                locked: true,
                position: Some((0, 0)),
                size: None,
            },
            // Resized but never moved — the reason the size is tagged rather
            // than a third and fourth trailing integer.
            Placement {
                locked: false,
                position: None,
                size: Some((900, 600)),
            },
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: Some((900, 600)),
            },
            // Resize, then lock: the size is kept for the same reason the
            // position is. Locking is not a discard button.
            Placement {
                locked: true,
                position: Some((-15, 900)),
                size: Some((1_280, 800)),
            },
        ] {
            assert_eq!(Placement::decode(&placement.encode()), placement);
        }
        assert_eq!(
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: None,
            }
            .encode(),
            "free 120 -40"
        );
        assert_eq!(
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: Some((900, 600)),
            }
            .encode(),
            "free 120 -40 size 900 600"
        );
        assert_eq!(
            Placement {
                locked: false,
                position: None,
                size: Some((900, 600)),
            }
            .encode(),
            "free size 900 600"
        );
        assert_eq!(Placement::default().encode(), "locked");
    }

    /// The row this story did not write. Every capture window on every machine
    /// that has ever been unlocked already has a placement in the settings
    /// table, in the two-token spelling, and it must keep meaning exactly what
    /// it meant — a size appearing out of nowhere would resize a window nobody
    /// resized.
    #[test]
    fn a_placement_written_before_this_story_still_reads_as_itself() {
        assert_eq!(
            Placement::decode("free 120 -40"),
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: None,
            }
        );
        assert_eq!(Placement::decode("locked"), Placement::default());
    }

    /// Every unreadable spelling falls back to keeper's own placement, and a
    /// half-readable position is no position rather than a fabricated axis.
    #[test]
    fn an_unreadable_placement_costs_the_position_and_never_the_window() {
        for raw in [
            "",
            "banana",
            "locked x y",
            "free 12",
            "free 12 y",
            "free y 12",
        ] {
            let decoded = Placement::decode(raw);
            assert_eq!(decoded.position, None, "{raw} invented a position");
        }
        // `free` is the only word that unlocks; anything else — including a
        // truncated row and a row from a build that spelled it differently —
        // leaves the window under keeper's control.
        assert!(Placement::decode("banana").locked);
        assert!(Placement::decode("Free 1 2").locked);
        assert!(!Placement::decode("free").locked);
        assert!(!Placement::decode("free 1 2").locked);
    }

    /// The degradation that matters most: an unreadable size must become
    /// keeper's own size, and never a window with no width, no height, or a
    /// dimension invented from the half of the pair that happened to parse.
    #[test]
    fn an_unreadable_size_costs_the_size_and_never_the_window() {
        for raw in [
            // A zero is not a size. It parses; it describes a window that
            // cannot be seen, focused or closed.
            "free 1 2 size 0 340",
            "free 1 2 size 560 0",
            "free size 0 0",
            // Half-readable, in each direction.
            "free 1 2 size 560 tall",
            "free 1 2 size wide 340",
            // Truncated.
            "free 1 2 size 560",
            "free 1 2 size",
            // Negative, and larger than a u32.
            "free 1 2 size -560 -340",
            "free 1 2 size 99999999999999 340",
            // A tag nobody writes.
            "free 1 2 dimensions 560 340",
            // A row from a build that spelled it some other way.
            "written by a later build",
        ] {
            let decoded = Placement::decode(raw);
            assert_eq!(decoded.size, None, "{raw} invented a size");
            // And the fallback is a real window, not a zero-sized one.
            assert_eq!(
                decoded.window_size(None),
                if decoded.locked {
                    Some(CAPTURE_DEFAULT_SIZE)
                } else {
                    None
                },
                "{raw} degraded to something other than keeper's own size"
            );
        }
        // The unreadable size costs the size and nothing else: a position in
        // the same row is still honoured.
        assert_eq!(
            Placement::decode("free 1 2 size 0 340").position,
            Some((1, 2))
        );
        // Trailing words nobody wrote are ignored rather than fatal, which is
        // what the two-token spelling already did with `free 1 2 whatever`. A
        // readable size followed by junk is a readable size: refusing it would
        // discard information the row plainly carries.
        assert_eq!(
            Placement::decode("free size 560 340 1 2"),
            Placement {
                locked: false,
                position: None,
                size: Some((560, 340)),
            }
        );
    }

    /// Which of the three answers each state gets, and the one that is `None`.
    #[test]
    fn a_locked_window_is_normalised_and_an_unsized_one_is_left_alone() {
        let screen = Some((1_920u32, 1_080u32));

        // Locked: keeper's own size, whatever the row remembers. This is the
        // escape hatch from a size the person no longer wants.
        assert_eq!(
            Placement::default().window_size(screen),
            Some(CAPTURE_DEFAULT_SIZE)
        );
        assert_eq!(
            Placement {
                locked: true,
                position: None,
                size: Some((1_400, 900)),
            }
            .window_size(screen),
            Some(CAPTURE_DEFAULT_SIZE)
        );

        // Unlocked and never resized: do not touch the window. NOT the
        // default — the live window may hold a size the user chose seconds ago
        // that no blur has written down yet.
        assert_eq!(
            Placement {
                locked: false,
                position: Some((10, 10)),
                size: None,
            }
            .window_size(screen),
            None
        );

        // Unlocked and resized: the size the person left it at.
        assert_eq!(
            Placement {
                locked: false,
                position: None,
                size: Some((900, 600)),
            }
            .window_size(screen),
            Some((900, 600))
        );
    }

    /// A window restored wider than the screen has its close button past the
    /// far edge, and a capture window is in no dock and no task switcher — so
    /// there is nothing left to click. The clamp is the whole of the answer.
    #[test]
    fn a_remembered_size_is_cut_down_to_the_screen_it_is_restored_on() {
        let remembered = |size| Placement {
            locked: false,
            position: None,
            size: Some(size),
        };

        // 3000 px wide on a 1440 px display — the story's own example.
        assert_eq!(
            remembered((3_000, 2_000)).window_size(Some((1_440, 900))),
            Some((1_440, 900))
        );
        // The monitor it was sized on has gone away and the laptop panel is
        // what is left.
        assert_eq!(
            remembered((2_400, 1_300)).window_size(Some((1_512, 945))),
            Some((1_512, 945))
        );
        // One axis over, one under: only the offending axis is cut.
        assert_eq!(
            remembered((3_000, 500)).window_size(Some((1_440, 900))),
            Some((1_440, 500))
        );
        // Exactly the work area is not oversized.
        assert_eq!(
            remembered((1_440, 900)).window_size(Some((1_440, 900))),
            Some((1_440, 900))
        );
        // A size that fits is not touched at all.
        assert_eq!(
            remembered((800, 500)).window_size(Some((1_920, 1_080))),
            Some((800, 500))
        );
    }

    #[test]
    fn a_remembered_size_is_never_smaller_than_the_window_can_hold() {
        let remembered = |size| Placement {
            locked: false,
            position: None,
            size: Some(size),
        };

        // Below the floor on both axes, and on one.
        assert_eq!(
            remembered((1, 1)).window_size(Some((1_920, 1_080))),
            Some(CAPTURE_MIN_SIZE)
        );
        assert_eq!(
            remembered((200, 700)).window_size(Some((1_920, 1_080))),
            Some((CAPTURE_MIN_SIZE.0, 700))
        );
        // Exactly the floor stays there.
        assert_eq!(
            remembered(CAPTURE_MIN_SIZE).window_size(Some((1_920, 1_080))),
            Some(CAPTURE_MIN_SIZE)
        );
        // The floor is below keeper's own size, or normalising a locked window
        // would enlarge it.
        assert!(CAPTURE_MIN_SIZE.0 < CAPTURE_DEFAULT_SIZE.0);
        assert!(CAPTURE_MIN_SIZE.1 < CAPTURE_DEFAULT_SIZE.1);
    }

    /// When the floor and the screen disagree, the screen wins: a window
    /// slightly too small to be comfortable beats a window whose right-hand
    /// edge — where every control is — is off the display.
    #[test]
    fn a_display_smaller_than_the_floor_still_gets_a_window_it_can_show() {
        let remembered = |size| Placement {
            locked: false,
            position: None,
            size: Some(size),
        };
        assert_eq!(
            remembered((900, 600)).window_size(Some((300, 200))),
            Some((300, 200))
        );
        // Including the locked window's normalisation, which asks for 560×340
        // on a display that cannot show it.
        assert_eq!(
            Placement::default().window_size(Some((300, 200))),
            Some((300, 200))
        );
    }

    /// A headless session, or a compositor that will not name a monitor. The
    /// floor still applies; there is no ceiling to apply.
    #[test]
    fn an_unknown_display_clamps_what_it_can_and_invents_nothing() {
        let remembered = |size| Placement {
            locked: false,
            position: None,
            size: Some(size),
        };
        assert_eq!(remembered((1, 1)).window_size(None), Some(CAPTURE_MIN_SIZE));
        assert_eq!(
            remembered((4_000, 3_000)).window_size(None),
            Some((4_000, 3_000))
        );
        // A zero-sized work area is a monitor mid-reconfiguration, not a
        // measurement. Clamping to it would build the invisible window
        // `decode` refuses to build.
        assert_eq!(
            remembered((900, 600)).window_size(Some((0, 0))),
            Some((900, 600))
        );
        assert_eq!(
            remembered((900, 600)).window_size(Some((800, 0))),
            Some((800, 600))
        );
    }

    #[test]
    fn closing_the_draft_hides_it_and_closing_any_other_destroys_it() {
        assert!(!plan_close(DRAFT_CAPTURE_KEY, true).destroy);
        assert!(!plan_close(DRAFT_CAPTURE_KEY, false).destroy);
        assert!(plan_close("note:v/n", true).destroy);
        assert!(plan_close("note:v/n", false).destroy);
    }

    #[test]
    fn closing_the_last_visible_window_raises_the_main_one() {
        // Nothing else on screen: the app would be running invisibly, in no
        // dock and no task switcher.
        assert!(plan_close("note:v/n", false).raise_main);
        assert!(plan_close(DRAFT_CAPTURE_KEY, false).raise_main);
        // Something else is still up — never steal its focus.
        assert!(!plan_close("note:v/n", true).raise_main);
        assert!(!plan_close(DRAFT_CAPTURE_KEY, true).raise_main);
    }

    #[test]
    fn the_window_view_model_wire_shape_is_camel_case() {
        let vm = CaptureWindowVm {
            key: "note:vault-a/note-1".into(),
            target: CaptureTargetVm::Note {
                vault_id: "vault-a".into(),
                note_id: "note-1".into(),
            },
            locked: false,
            visible: true,
        };
        assert_eq!(
            serde_json::to_string(&vm).expect("serialize window"),
            r#"{"key":"note:vault-a/note-1","target":{"kind":"note","vaultId":"vault-a","noteId":"note-1"},"locked":false,"visible":true}"#
        );
        assert_eq!(
            serde_json::to_string(&CaptureTargetVm::Draft).expect("serialize draft"),
            r#"{"kind":"draft"}"#
        );
    }
}
