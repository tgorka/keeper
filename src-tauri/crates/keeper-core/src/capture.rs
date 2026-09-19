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
    /// Whether the window floats above other applications (Story 48.4).
    ///
    /// Carried on the view model rather than read back from the live window by
    /// the chrome, because the chrome cannot read it: a webview may not ask its
    /// own window whether it is on top, and `quick-capture.json` deliberately
    /// grants no window permissions at all. This is the same reason `locked`
    /// rides here — the toggle's pressed state has to come from the same list
    /// the lock's does, or the two buttons could disagree.
    pub always_on_top: bool,
    /// Whether it is on screen. A hidden draft window is still a window.
    pub visible: bool,
    /// The gap, in **logical CSS pixels**, the window's own resize border needs
    /// on the chrome's top and right edges right now (Story 47.5, DW-199; the
    /// chrome is the header's frame group since Story 75.3) — `0` on every
    /// platform and in every state but one.
    ///
    /// Decided in Rust and carried, because the frontend cannot decide it: this
    /// app reads the platform nowhere (`src/test/no-user-agent-gating.test.ts`
    /// enforces that), and the number is a function of the backend, the lock,
    /// the maximized state and the monitor's scale factor. See
    /// [`chrome_edge_inset`].
    pub chrome_inset: u32,
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
pub fn query_encode(value: &str) -> String {
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
/// hold, and re-derived by Story 75.3 when the two header rows became one
/// (AD-260). The old derivation described a 32 px window-chrome strip above a
/// 40 px editor header; that strip no longer exists, and the three window
/// controls it carried now sit in the header's frame group, where they cost
/// width instead of height.
///
/// The row, left to right, at its floor:
///
/// ```text
///   12  left gutter (px-3)
/// + 160  identity, at PANE_HEADER_IDENTITY_MIN_PX — the title's own floor
/// +   8  gap
/// +   0  status caption, squeezed to nothing: AD-260 says it gives first
/// +   8  gap — the slot is IN the row, so it costs its seam even at width 0
/// + 104  actions: attach 32 + properties 32, adjacent, + 8 + overflow 32
/// +   8  gap
/// +  88  frame: three 24 px window controls + two 8 px gaps
/// +  12  right gutter
/// = 400
/// ```
///
/// Three numbers in there are decisions rather than measurements. The leading
/// pair of actions is 64 and not 72: `PriorityActions` renders its `leading`
/// box with no gap between the two, measured in Chromium over the real
/// component, so the group is 104. The window controls are 24 px and not 32:
/// three 32 px controls would put the floor at 424, and a control smaller than
/// a document action is also the distinction a person wants — window chrome is
/// not the note's own verbs. And the DW-199 edge inset no longer adds width:
/// the merged chrome pads only itself, by `max(0, inset − gutter)`, and the
/// row's own 12 px gutter already covers the inset at 1× and 2×. The 3 px that
/// remains at 3× is not folded in here: a 3× display is 400 logical pixels
/// wide only in a thought experiment, and paying it on every machine to
/// protect one that cannot exist is the wrong trade.
///
/// The `0` for the caption is the row at its tightest and not the row always:
/// a caption that is a *refused write* keeps its box and the identity floor is
/// released instead, because that sentence is the only place a capture window
/// says why it will not close. Measured in Chromium over the real component at
/// this floor, that row is 12 + 0 + 8 + 125 + 8 + 104 + 8 + 88 + 12 = 365 of
/// demands — the caption's box is the width of its widest sizer, 125 in this
/// machine's locale — so the identity keeps the remaining 35 and nothing
/// overflows. The floor does not move for it.
///
/// 240 of height is unchanged and is now more generous than it was: the merge
/// gave the body back 32 px that used to be chrome.
///
/// Enforced twice on purpose: here, so a remembered row can never restore a
/// window smaller than this, and as `minWidth`/`minHeight` on the window itself,
/// so the compositor refuses the drag before the user gets there.
pub const CAPTURE_MIN_SIZE: (u32, u32) = (400, 240);

/// The word that introduces a size in a persisted placement.
///
/// Tagged rather than positional, because a size has to be storable *without* a
/// position — a window resized but never moved is an ordinary thing — and three
/// optional trailing integers with no tag cannot say which pair is which.
/// It also keeps the old two-token spelling readable verbatim: a row written by
/// the build before this story is `free 120 -40`, and it still decodes to
/// exactly the placement it always did, with no size.
const SIZE_TAG: &str = "size";

/// The word that introduces the always-on-top flag in a persisted placement.
///
/// Story 48.4, and [`SIZE_TAG`]'s pattern verbatim, for the same reason: the
/// flag has to be storable without a position *and* without a size, and a
/// seventh bare trailing token could not say which of the optional groups it
/// belonged to.
///
/// Written **only when the flag is on** since Story 75.2, which is the inverse
/// of 48.4's rule and inverted with the default it encodes: the tag is the
/// exception, and the exception is now "this window floats". Both spellings
/// are still *read* — `top 0` is the row 48.4 wrote for an un-pinned window and
/// still means un-pinned — so no row changes meaning, but a row written before
/// this story that carries no tag now answers `false` instead of `true`. That
/// is the upgrade AD-259 asks for, and [`Placement`]'s own doc argues why the
/// old spelling had nothing better to say.
const TOP_TAG: &str = "top";

/// Whether `word` introduces one of the optional tagged groups.
///
/// The position is the only group that is *positional*, so it is read only when
/// the next word is not a tag — and that test has to know about every tag, not
/// just the first one. Missing a tag here does not lose the tag's own value: it
/// consumes the tag word as an x-coordinate, which loses the position too.
fn is_tag(word: &str) -> bool {
    word == SIZE_TAG || word == TOP_TAG
}

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
///
/// # Why `always_on_top` defaults to *false* (Story 75.2, AD-259)
///
/// Story 48.4 defaulted it to `true` because that is what every capture window
/// already was, and said in as many words that flipping it "would silently
/// un-pin every capture window on every machine at upgrade, which is a
/// behaviour change nobody asked for". The owner has now asked for it: a panel
/// that decides for itself to float over every other application is the second
/// of the four complaints this epic answers. So the un-pinning that paragraph
/// refused is this story's deliverable, and it is deliberate rather than a
/// regression of it.
///
/// **The persisted spelling had to invert with the default**, and that is the
/// part worth reading twice. 48.4 wrote the `top` tag only when the flag was
/// OFF, precisely so that every row written before it stayed byte-identical —
/// which means "on" is *unrepresentable* in an old row, and there is no way to
/// tell a window the owner pinned from one that was pinned because keeper
/// pinned everything. Leaving [`Placement::decode`]'s absent-tag fallback at
/// `true` would therefore have shipped a new default that no existing machine
/// — including the one the complaint came from — ever sees. The tag is now
/// written when the flag is ON, an absent tag means OFF, and `top 0` still
/// reads as OFF so the rows 48.4 did write keep their meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// `true` — keeper places the window and the user may neither move nor
    /// resize it. `false` — the user moves and resizes it and keeper remembers
    /// both.
    ///
    /// Since Story 75.1 a lock no longer imposes a *size*: it freezes whatever
    /// is there (see [`Placement::window_size`]). It still owns the position,
    /// which is what lets a locked panel follow the pointer between monitors.
    ///
    /// Still stored, and still read by the chrome, but **no longer survives an
    /// open**: [`Placement::opened`] clears it, because a lock is a gesture
    /// about the capture in front of you rather than a preference about the
    /// window (Story 75.2, AD-258).
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
    /// `true` — the window floats above other applications, which is what a
    /// capture panel has always done. `false` — it behaves like an ordinary
    /// window and can be covered.
    ///
    /// Per-window rather than a global setting, and stored here rather than in
    /// the settings table's own namespace, because it is a property of *this*
    /// window in the same sense the size is: two capture windows are two
    /// placements, and a person who pins a note beside what they are reading
    /// has said nothing about the next window they open.
    ///
    /// **A pin survives an open and a lock does not, and that asymmetry is the
    /// decision rather than an oversight** (Story 75.2, AD-258/AD-259): a lock
    /// is a gesture about the capture in front of you, a pin is a preference
    /// about how this window behaves, so discarding one on every open is
    /// respect and discarding the other is the same disrespect in the opposite
    /// direction. [`Placement::opened`] therefore clears exactly one of these
    /// two flags.
    pub always_on_top: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            locked: true,
            position: None,
            size: None,
            // Story 75.2, AD-259. See the struct doc: the flip takes the
            // persisted spelling with it, because 48.4's encoding cannot
            // express an explicit `on`.
            always_on_top: false,
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
        // Written only when the flag is ON (Story 75.2). The tag still costs
        // nothing on the common row — it is just that the common row is now the
        // un-pinned one — and writing the *on* case explicitly is what makes an
        // asked-for pin distinguishable from a pin nobody asked for. Under
        // 48.4's spelling it was not: "on" was the absence of a tag, so an
        // upgrade could not tell the owner's pin from keeper's own default.
        if self.always_on_top {
            encoded.push(' ');
            encoded.push_str(TOP_TAG);
            encoded.push_str(" 1");
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
        // Only look for a position when the next word is not a TAG: `free size
        // 900 600` is a window resized but never moved, and reading its first
        // two words as coordinates would consume the tag and lose the size as
        // well. Every tag counts here, not just the size's — see [`is_tag`].
        let position = if parts.peek().is_some_and(|word| !is_tag(word)) {
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
        // Each tagged group PEEKS for its own tag before consuming anything.
        // The obvious spelling — matching on a tuple of three `next()` calls —
        // consumes three words whether or not the tag was there, so a row with
        // no size but a later tag (`free 1 2 top 0`) loses both: the size
        // branch eats `top` and `0` looking for a size it never finds. That is
        // not a hypothetical; it is what this function did before Story 48.4,
        // and it is why a second tag could not simply be appended.
        let size = if parts.next_if(|word| *word == SIZE_TAG).is_some() {
            match (parts.next(), parts.next()) {
                (Some(width), Some(height)) => {
                    match (width.parse::<u32>(), height.parse::<u32>()) {
                        (Ok(width), Ok(height)) if width > 0 && height > 0 => Some((width, height)),
                        _ => None,
                    }
                }
                _ => None,
            }
        } else {
            None
        };
        // Absent tag, absent value and unreadable value all answer `false`
        // (Story 75.2, AD-259): the fallback follows the default, as it did
        // before, and the default is now an ordinary window. Only the exact
        // word `1` floats one — so `top 0`, the only spelling 48.4 ever wrote,
        // keeps meaning exactly what it meant, and a row keeper half-understands
        // costs the user a pin they can restore with one click rather than a
        // panel sitting over everything they do.
        let always_on_top = if parts.next_if(|word| *word == TOP_TAG).is_some() {
            matches!(parts.next(), Some("1"))
        } else {
            false
        };
        Self {
            locked,
            position,
            size,
            always_on_top,
        }
    }

    /// The size to give this window right now, in logical pixels, or `None` for
    /// "leave it exactly as it is".
    ///
    /// # A lock freezes what is there (Story 75.1, AD-257)
    ///
    /// The locked arm used to answer [`CAPTURE_DEFAULT_SIZE`] for *every*
    /// locked window, whatever the row remembered, under the rule "a locked
    /// window is keeper's to place and keeper's to size". That is a defensible
    /// reading of the word and it is not the one a person pressing a padlock
    /// has: they mean *stay exactly as you are*. It made the single most
    /// reported thing about this window — resize it, lock it, watch it jump
    /// back to 560×340 — a decision rather than a bug, and it was not even a
    /// lossy one: the lock command persists the live size first, so the size
    /// the person chose was in the row the whole time and was simply not asked
    /// for. The rule is rescinded for the size and kept for nothing else; a
    /// locked window still cannot be *moved*, which is [`Self::adopted_position`].
    ///
    /// Four answers now, and the last is still the one that matters:
    ///
    /// - **Locked with a remembered size → that size**, clamped. The freeze.
    /// - **Locked with nothing remembered → keeper's own size.** Not `None`:
    ///   there is nothing to honour, and unlike the unlocked case below there
    ///   is no gesture in flight to undo — a locked window's live extent is
    ///   keeper's own, because that is what being locked has meant for every
    ///   frame up to this one.
    /// - **Unlocked with a remembered size → that size**, clamped.
    /// - **Unlocked with no remembered size → `None`.** Not the default: the
    ///   caller must not touch the window. This is the difference between
    ///   "never resized" and "resized to 560×340", and it is load-bearing — the
    ///   live window may have been dragged to a new size seconds ago by a user
    ///   whose blur has not yet written it down, and re-asserting a size on the
    ///   next open would undo the gesture in front of them.
    ///
    /// The clamp applies to every arm that answers, and applies to the freeze
    /// for a reason the freeze creates: a size remembered on a larger monitor
    /// is now replayed onto a *locked* window too, and a locked capture window
    /// is undecorated and in no task switcher, so an unclamped restore would
    /// put its controls past an edge with nothing left to click.
    ///
    /// `work_area` is the usable area of the monitor the window will appear on,
    /// in logical pixels, or `None` when the platform will not say (a headless
    /// session, or a compositor that does not answer).
    #[must_use]
    pub fn window_size(&self, work_area: Option<(u32, u32)>) -> Option<(u32, u32)> {
        let wanted = match (self.locked, self.size) {
            (true, Some(size)) => size,
            (true, None) => CAPTURE_DEFAULT_SIZE,
            (false, Some(size)) => size,
            (false, None) => return None,
        };
        Some(clamp_size(wanted, work_area))
    }

    /// This placement as it must be applied when the window is **opened**
    /// (Story 75.2, AD-258).
    ///
    /// # One field, and the wipe hazard that makes it a function
    ///
    /// `locked` is forced `false`. Nothing else moves: `position`, `size` and
    /// `always_on_top` ride in the same stored row
    /// (`notes.capture_placement.<key>`), so the obvious spelling of "open it
    /// unlocked" — assigning a fresh [`Placement`] with `locked: false` — would
    /// throw away the geometry this whole module exists to keep, and would do
    /// it silently, on the path every window takes. That is why this is a named
    /// function returning a struct update rather than a literal at a call site:
    /// [`Self::relocked`]'s doc records the identical mistake being made once
    /// already, inline, in the crate that does not compile on every machine.
    ///
    /// # Why the lock is discarded and the pin is not
    ///
    /// A lock is a gesture about the capture in front of you — it freezes the
    /// geometry of *this* thought while you write it down, and until Story 75.1
    /// it also resized the window out from under you. A pin is a preference
    /// about how this window behaves. So a lock that outlived its window is a
    /// window that no longer answers a drag for a reason nobody remembers,
    /// while a pin discarded on every open would be keeper overruling an
    /// explicit answer — the same disrespect in the other direction. The
    /// asymmetry is AD-258 against AD-259 and it is deliberate: do not "fix" it
    /// by clearing both.
    ///
    /// # Why it is here rather than at the two call sites
    ///
    /// Two paths open a capture window and neither builds it: boot adopts the
    /// stored row for the prewarmed panel, and the hotkey **reveals** a window
    /// that has been alive and hidden since startup (NFR-27). Which of the two
    /// a person happened to use cannot be allowed to change whether their
    /// window is locked, and the way to guarantee that is one rule in the crate
    /// that compiles everywhere rather than two copies in the crate that
    /// compiles nowhere but CI (AD-55/AD-56).
    ///
    /// Nothing here is written back to the settings table. The stored row keeps
    /// saying what it said; this is what the *window* gets, and an open is not
    /// a gesture worth a sqlite transaction.
    #[must_use]
    pub fn opened(self) -> Self {
        Self {
            locked: false,
            ..self
        }
    }

    /// The position to give this window at boot, in physical pixels, or `None`
    /// for "keeper places it" (Story 47.5, DW-198).
    ///
    /// **The exact mirror of [`Self::window_size`], and that symmetry is the
    /// whole fix.** Before this, the size was adopted once at boot and survived
    /// every hotkey press while the position was re-centred on each one, so a
    /// person who unlocked the draft panel and dragged it somewhere found that
    /// keeper remembered how big they made it and not where they put it.
    ///
    /// - **Locked → `None`.** A locked panel is keeper's to place, and
    ///   following the pointer between monitors is the whole of what that
    ///   means. Adopting a stored position here would make a locked window stop
    ///   following the pointer, which is the cost DW-198 names and the reason
    ///   the answer is the lock's rather than a new setting's.
    /// - **Unlocked with a remembered position → that position.**
    /// - **Unlocked with nothing remembered → `None`**: never moved, so keeper
    ///   places it exactly as it always did.
    ///
    /// **This is a request and not a promise, and the UI is worded for the
    /// promise.** Applying it is a `set_position`, the one call UX-DR43 says a
    /// Wayland compositor may refuse, so the lock's label still says "so it can
    /// be moved and resized" and never "remembers". A compositor that declines
    /// leaves a window the person can still put wherever they like — the same
    /// recovery as before, one gesture long — rather than a promise that
    /// quietly fails.
    #[must_use]
    pub fn adopted_position(&self) -> Option<(i32, i32)> {
        match (self.locked, self.position) {
            (false, position) => position,
            (true, _) => None,
        }
    }

    /// Fold what the shell just read off the live window into what is stored
    /// (Story 48.2).
    ///
    /// # The lock was a discard button, and 46.15 promised it was not
    ///
    /// Story 46.15's own words are *"locking is not a discard button … the
    /// remembered size is kept, so unlocking restores it"*. It could not. Both
    /// callers used to merge unconditionally — `live.size.or(stored.size)` —
    /// and on the **unlock** click the live window has already been normalised
    /// to [`CAPTURE_DEFAULT_SIZE`] by the *lock*, so the merge wrote keeper's
    /// own 560×340 over the user's 900×600 a moment before
    /// `Placement::window_size` was asked to restore it. It did not even need
    /// an unlock: blur writes the geometry down too, so the first click on
    /// another app after locking was enough to lose the size for good. The same
    /// hole ate the position, where 46.15's *"unlock, drag, lock again is a
    /// person saying keep it **there**"* met a locked window whose live
    /// coordinate is wherever the last hotkey press put it (Story 47.5,
    /// DW-198).
    ///
    /// # The rule, in one sentence
    ///
    /// **keeper remembers only geometry the user could have produced.** A
    /// window keeper itself placed and sized has nothing to report: its
    /// coordinate is `plan_show_position`'s and its extent is
    /// [`CAPTURE_DEFAULT_SIZE`], and writing either down overwrites a fact with
    /// a restatement of keeper's own defaults.
    ///
    /// # Why a guard and not a second remembered size
    ///
    /// The obvious alternative is a `user_size` beside `size`, so the applied
    /// size and the chosen one are different fields. It is more code and it is
    /// less true. There is no moment when both are informative: while the
    /// window is locked the live size is *always* keeper's and carries nothing,
    /// and while it is unlocked the live size *is* the user's. So the second
    /// field would be a copy of the first that is only ever read in one state,
    /// plus a token in the persisted spelling, plus a decode arm, plus a
    /// question about what old rows mean. What is actually missing at the merge
    /// is not *which size was the user's* but *was this window under the user's
    /// control when I read it* — one boolean the shell already holds, as
    /// `is_resizable()`, the very attribute [`plan_show_position`] reads on the
    /// hot path. Storing a derived copy to avoid reading a fact you have is the
    /// bigger change and the weaker one.
    ///
    /// The lock itself is untouched: this decides what is *remembered*, never
    /// who may move the window.
    #[must_use]
    pub fn observing(self, live: Observed) -> Self {
        if !live.user_controlled {
            return self;
        }
        Self {
            position: live.position.or(self.position),
            size: live.size.or(self.size),
            ..self
        }
    }

    /// What the lock toggle writes down (Story 48.2).
    ///
    /// [`Self::observing`] with the new lock state on top, and a named function
    /// rather than a struct literal at the call site **because the struct
    /// literal is what broke**. `notes_capture_set_locked` used to rebuild the
    /// whole placement inline — `position: live.position.or(stored.position)`,
    /// `size: live.size.or(stored.size)` — which put the one rule in this file
    /// that decides whether the lock keeps a promise inside the crate that does
    /// not compile on every machine (AD-55/AD-56). It was wrong for two
    /// releases and no test could have said so. The command now has no geometry
    /// logic left in it at all: it reads, calls this, writes, and applies.
    ///
    /// `locked` is the state being moved **to**; `live.user_controlled` is what
    /// the window was when it was measured. On the unlock click those are
    /// `false` and `false`, and that is exactly the case the old literal got
    /// wrong.
    #[must_use]
    pub fn relocked(self, live: Observed, locked: bool) -> Self {
        Self {
            locked,
            ..self.observing(live)
        }
    }
}

/// What the shell just read off a live capture window (Story 48.2).
///
/// The counterpart of [`Placement`]: a `Placement` is what is stored, an
/// `Observed` is what a window says about itself right now, and
/// [`Placement::observing`] is the only place the second is allowed to become
/// the first.
///
/// The units match [`Placement`]'s and the asymmetry is the same one, for the
/// same reason: **position physical, size logical**.
///
/// # `user_controlled`, and why it is not `locked`
///
/// It reads almost like the lock and it is deliberately a different question.
/// `Placement::locked` is what is *stored*; this is what the *live window* was
/// when the shell measured it, read off `is_resizable()` — the same attribute
/// [`plan_show_position`] reads, so there is no second copy of the lock to
/// drift. The two disagree at exactly the moment that matters: on the unlock
/// click the placement being written says `locked: false` while the window
/// being measured is still keeper's normalised one, and it is that disagreement
/// that used to cost the user their size.
///
/// A window that will not say is **not** user-controlled. That direction costs
/// a remembered geometry and can never overwrite one, and it matches
/// [`chrome_edge_inset`]'s caller, which treats an unanswering window as
/// locked for the same reason.
///
/// Since Story 75.2 a window is unlocked at every open ([`Placement::opened`]),
/// so `false` here has become the narrow case it always described and no longer
/// the common one: it means the person pressed the padlock during *this*
/// session. The consequence is deliberate and worth knowing — keeper's own
/// opening geometry is now remembered on the first blur of a window nobody has
/// touched, because that window genuinely is theirs to move.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Observed {
    /// The top-left in physical pixels, or `None` when the platform will not
    /// say.
    pub position: Option<(i32, i32)>,
    /// The inner size in logical pixels, or `None` when the platform will not
    /// say.
    pub size: Option<(u32, u32)>,
    /// Whether the user, rather than keeper, is what put this window at this
    /// coordinate and this size.
    pub user_controlled: bool,
}

/// What a `show` that has read no settings must do about a window's position
/// (Story 47.5, DW-198).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowPosition {
    /// Place the panel on the monitor under the pointer, as keeper always has.
    /// Since Story 75.2 this is the answer for a window that will not say
    /// whether it is resizable, and for no other window.
    Place,
    /// Touch the position not at all — leave the window where the person put
    /// it, which since Story 75.2 is every window that answers.
    Leave,
}

/// Whether the hotkey and tray path re-places the draft panel, given only
/// whether that window **answered** when asked about itself (Story 47.5,
/// DW-198; narrowed by Story 75.2, AD-258).
///
/// **Takes one live window attribute, not a stored placement, and that is what
/// keeps NFR-27 intact.** `show` is the hotkey path: `set_position` → `show` →
/// `set_focus`, a handful of synchronous window calls with no settings read in
/// front of them. The shell reads `is_resizable()` and hands in whether the
/// call succeeded — not what it said. One question, no query, no second copy of
/// anything to drift.
///
/// **Why the answer is no longer the lock's.** Until Story 75.2 a locked window
/// was keeper's to place, so `is_resizable() == false` meant "place it", and
/// [`Placement::adopted_position`] was the other half that put an unlocked one
/// back at boot. Since [`Placement::opened`] unlocks every window at every
/// open, a window that reports itself locked here is one that is about to be
/// unlocked a line later — it is a window the person locked earlier in this
/// session, sitting exactly where they locked it. Placing it would move the
/// window a padlock press asked to keep still, and the next blur would write
/// that coordinate over the row: the position lost for good, by the gesture
/// that was meant to preserve it.
///
/// So the only window keeper still places is one that will not say — a backend
/// whose `is_resizable()` errors gets a placed window rather than one left
/// wherever it happens to be. The panel no longer following the pointer between
/// monitors is AD-258 arriving, not a regression: the following was the
/// compensation for a window nobody could drag.
#[must_use]
pub fn plan_show_position(answers: bool) -> ShowPosition {
    if answers {
        ShowPosition::Leave
    } else {
        ShowPosition::Place
    }
}

/// The resize border tao hit-tests INSIDE an undecorated window's client area,
/// in logical pixels **per unit of scale factor**.
///
/// Read off tao 0.35.3, not guessed: `platform_impl/linux/event_loop.rs` does
/// `let border = window.scale_factor() * 5;` in both the motion handler (line
/// 501) and the button-press handler (line 531), and feeds it to
/// `crate::window::hit_test` as both `border_x` and `border_y`. The comparison
/// is against GDK window coordinates, which on GTK3 are already logical, so the
/// strip is 5 logical pixels on a 1× display and **10 on a 2× one**. A CSS
/// constant of 5 would be exactly half of what a HiDPI GTK window needs, which
/// is the failure mode of fixing this with a number typed into a stylesheet.
pub const TAO_EDGE_BORDER: u32 = 5;

/// The three facts tao's edge hit test actually reads, plus the one platform
/// fact that decides whether it runs at all (Story 47.5, DW-199).
///
/// A struct rather than four positional arguments because every field is a
/// condition in tao's own guard and swapping two booleans at a call site would
/// produce a plausible-looking wrong number rather than a compile error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeResize {
    /// Whether this backend hit-tests the resize edge inside the client area,
    /// so the webview never sees a click that lands in it.
    ///
    /// True on GTK/tao. On macOS and Windows the resize border lives outside
    /// the client area, so the chrome needs no inset there and must not get
    /// one — moving a control on three platforms to fix one is the other way
    /// to get this wrong.
    pub inside_client_area: bool,
    /// Whether the window is resizable right now. tao's guard is
    /// `… && window.is_resizable() && …`, so a LOCKED capture window has no
    /// resize border at all and its close button is already reachable to its
    /// own edge.
    pub resizable: bool,
    /// Whether the window is maximized. tao's guard is `&& !is_maximized()`:
    /// edge dragging is off while maximized, so the border is not there either.
    pub maximized: bool,
    /// The window's scale factor as tao multiplies by it. `0` and `1` both mean
    /// an unscaled display; a platform that will not answer costs the chrome
    /// one border's worth of inset, never a hidden control.
    pub scale: u32,
}

/// How much of the chrome's top and right edges the window's own resize border
/// is sitting on, in logical CSS pixels (Story 47.5, DW-199). Since Story 75.3
/// that chrome is the header's frame group rather than a strip above it, and
/// the inset pads only itself: see `CAPTURE_MIN_SIZE`'s ledger.
///
/// **The geometry, not the symptom.** The capture chrome's close button is
/// flush against the top-right corner, which is where two of tao's edge strips
/// overlap, so on GTK an unlocked window turns the top and right few pixels of
/// the close button into a resize handle — aim at close, get a drag, and tao's
/// own FIXME means the cursor does not even change to warn you. The fix is to
/// keep the controls out of the strip, and the strip's width is
/// [`TAO_EDGE_BORDER`] × scale.
///
/// **Zero in every state where tao does not hit-test**, which is most of them:
/// a locked window (not resizable), a maximized one, and every non-GTK
/// backend. That is what keeps this from being a padding that moves a control
/// on three platforms to solve a problem on one.
#[must_use]
pub fn chrome_edge_inset(edge: EdgeResize) -> u32 {
    if !edge.inside_client_area || !edge.resizable || edge.maximized {
        return 0;
    }
    TAO_EDGE_BORDER * edge.scale.max(1)
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

/// How far down the focused monitor's work area an *unplaced* panel sits, as a
/// fraction of the monitor height.
///
/// A fifth of the way down rather than centred: a capture panel is a thing you
/// type into and dismiss, and vertical centring puts it exactly where a
/// person's eyes are already busy with whatever they were reading.
///
/// Moved here from the shell by Story 48.2, with the rest of the placement
/// arithmetic — see [`auto_position`].
const TOP_FRACTION: f64 = 0.2;

/// The rectangle a window may actually occupy on one monitor, in **physical**
/// pixels (Story 48.2).
///
/// The work area rather than the resolution, so the macOS menu bar and a Linux
/// panel are already excluded. Physical throughout, because that is the unit a
/// [`Placement::position`] is remembered in and the unit `set_position` takes,
/// and because only a physical coordinate names a point unambiguously on a desk
/// whose monitors have different scale factors.
///
/// **The origin is a field and not an assumption**, and that is the whole
/// reason this is a struct rather than a `(u32, u32)` like [`clamp_size`]'s
/// argument. A size is measured from nothing; a position is measured from the
/// virtual desktop's origin, and the second monitor's work area does not start
/// at zero. Clamping a coordinate against a bare extent would pull every window
/// on every non-primary monitor onto the primary one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    /// The work area's top-left on the virtual desktop, in physical pixels.
    pub position: (i32, i32),
    /// The work area's extent in physical pixels.
    pub size: (u32, u32),
}

/// Pull a window's top-left back inside the work area it is being placed on
/// (Story 48.2).
///
/// **Nothing clamped a position before this, anywhere**, and [`clamp_size`]'s
/// docs above describe the consequence in full without covering the case: a
/// capture window is undecorated and `skipTaskbar`, so it is in no dock and no
/// task switcher, and a window that is off the screen is a window with nothing
/// left to click. A correctly sized window at an unreachable coordinate is
/// exactly as lost as an oversized one. Two ways in, both reported from a real
/// 0.8.1 desktop:
///
/// - **The lock grows a window from the same top-left.** A 320×240 window
///   parked against the bottom-right corner becomes [`CAPTURE_DEFAULT_SIZE`]
///   the instant it is locked, and nothing used to move it, so 240 px of it
///   went past the edge — including the corner the close button is in. Story
///   75.1 closed that route for a window whose size is remembered — a lock now
///   freezes it rather than growing it — and left this clamp exactly where it
///   was, because the second route below never went through the lock at all.
/// - **A remembered coordinate outlives its monitor.** A position stored on the
///   second display is replayed verbatim on a machine that has since been
///   undocked, putting the window on a rectangle of desktop that no longer has
///   pixels behind it.
///
/// The window is pushed, never shrunk: the caller has already settled the size
/// through [`clamp_size`], and a clamp that changed both would fight it.
///
/// `work_area` is `None` when the platform will not name a monitor (a headless
/// session, or a compositor that does not answer), and then **the position is
/// returned untouched** — the same "invent nothing from nothing" rule
/// [`clamp_size`] follows.
#[must_use]
pub fn clamp_position(
    position: (i32, i32),
    size: (u32, u32),
    work_area: Option<WorkArea>,
) -> (i32, i32) {
    let Some(area) = work_area else {
        return position;
    };
    (
        clamp_axis(position.0, size.0, area.position.0, area.size.0),
        clamp_axis(position.1, size.1, area.position.1, area.size.1),
    )
}

/// One axis of [`clamp_position`]: keep `coordinate` between the work area's
/// near edge and the last coordinate that still leaves `extent` fully inside it.
///
/// A zero span is refused for [`clamp_size`]'s reason, restated for a position:
/// some backends report a zero-sized work area for a monitor mid-reconfiguration,
/// and a window clamped to it is a window pinned to a corner of a screen that is
/// about to stop being that shape. Zero is not a measurement, so nothing is
/// clamped to it.
///
/// A window **larger** than the work area lands flush against the near edge
/// rather than at a negative coordinate — `saturating_sub` gives no free room,
/// so the low and high bounds coincide at the origin. That is
/// [`clamp_size`]'s "reachable beats comfortable" on the other axis of the same
/// problem: the top-left is where the drag strip is, so the part of an
/// unavoidably oversized window a person can still grab is the part kept on
/// screen.
fn clamp_axis(coordinate: i32, extent: u32, origin: i32, span: u32) -> i32 {
    if span == 0 {
        return coordinate;
    }
    let free = span.saturating_sub(extent);
    // Saturating, and the low bound is the origin, so `high >= low` always
    // holds and `clamp` cannot panic on a desktop with an extreme origin.
    let high = origin.saturating_add(i32::try_from(free).unwrap_or(i32::MAX));
    coordinate.clamp(origin, high)
}

/// Where keeper puts a panel nobody has placed: horizontally centred on the
/// work area, [`TOP_FRACTION`] of the way down it, and never off the edge.
///
/// **Extracted from the shell's `position` by Story 48.2 rather than invented
/// here.** It is the same arithmetic the shell has always done — the old
/// `centred` and `offset_from_top` — and moving it buys the thing this module
/// exists for: it is now checked on a machine where the shell does not compile
/// (AD-55/AD-56), and the clamp that keeps a *restored* window on screen is the
/// same code as the clamp that keeps a *placed* one there, so the two cannot
/// disagree.
///
/// `size` is the window's **physical** outer size, matching [`WorkArea`].
#[must_use]
pub fn auto_position(size: (u32, u32), area: WorkArea) -> (i32, i32) {
    let free_width = area.size.0.saturating_sub(size.0);
    let centred = i32::try_from(free_width / 2).unwrap_or(0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let down = (f64::from(area.size.1) * TOP_FRACTION) as u32;
    let wanted = (
        area.position.0.saturating_add(centred),
        area.position
            .1
            .saturating_add(i32::try_from(down).unwrap_or(0)),
    );
    // The clamp is what used to be `offset_from_top`'s `.min(free)`: a panel
    // taller than a fifth of the space left sits higher up rather than hanging
    // its bottom edge off the screen.
    clamp_position(wanted, size, Some(area))
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
        // Still locked, and this is the *stored* default rather than the one a
        // window opens with: `locked` is what makes an unreadable or absent row
        // fall back to "keeper places this", and [`Placement::opened`] is what
        // hands the window to the user a moment later (Story 75.2, AD-258).
        assert!(default.locked);
        assert_eq!(default.position, None);
        assert_eq!(default.size, None);
        // Story 75.2, AD-259: nothing floats until it is asked to. If this line
        // ever reads `true` again, every machine that has never touched the pin
        // gets a panel sitting over every other application.
        assert!(!default.always_on_top);
        assert_eq!(Placement::decode(""), default);
    }

    /// The fresh-machine row of Story 75.2's matrix, end to end: no stored row
    /// at all, which is the state every install starts in.
    ///
    /// Asserted through `decode("")` rather than through `Placement::default()`
    /// because that is literally what the registry hands the shell when the
    /// settings table has never heard of this window, and the two are allowed
    /// to drift only over this test's dead body.
    #[test]
    fn a_machine_that_has_never_seen_this_window_opens_unlocked_and_unpinned() {
        let fresh = Placement::decode("").opened();
        assert!(!fresh.locked, "a fresh window opened locked");
        assert!(
            !fresh.always_on_top,
            "a fresh window floated over everything"
        );
        // And keeper still places and sizes it, because nothing is remembered:
        // `opened` unlocks, it does not invent a geometry.
        assert_eq!(fresh.position, None);
        assert_eq!(fresh.size, None);
        assert_eq!(fresh.adopted_position(), None);
        assert_eq!(fresh.window_size(Some((1_920, 1_080))), None);
    }

    #[test]
    fn a_placement_round_trips_through_its_persisted_form() {
        for placement in [
            Placement {
                locked: true,
                position: None,
                size: None,
                always_on_top: false,
            },
            Placement {
                locked: false,
                position: None,
                size: None,
                always_on_top: false,
            },
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: None,
                always_on_top: false,
            },
            // Unlock, drag, lock again: the position is kept, because locking
            // means "keep it there" and not "forget where I put it".
            Placement {
                locked: true,
                position: Some((0, 0)),
                size: None,
                always_on_top: false,
            },
            // Resized but never moved — the reason the size is tagged rather
            // than a third and fourth trailing integer.
            Placement {
                locked: false,
                position: None,
                size: Some((900, 600)),
                always_on_top: false,
            },
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: Some((900, 600)),
                always_on_top: false,
            },
            // Resize, then lock: the size is kept for the same reason the
            // position is. Locking is not a discard button.
            Placement {
                locked: true,
                position: Some((-15, 900)),
                size: Some((1_280, 800)),
                always_on_top: false,
            },
            // …and the same seven shapes are not worth writing twice, but the
            // pinned one is: since Story 75.2 the flag is spelled only when it
            // is ON, so `true` is the case that now adds bytes.
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: Some((900, 600)),
                always_on_top: true,
            },
        ] {
            assert_eq!(Placement::decode(&placement.encode()), placement);
        }
        assert_eq!(
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: None,
                always_on_top: false,
            }
            .encode(),
            "free 120 -40"
        );
        assert_eq!(
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: Some((900, 600)),
                always_on_top: false,
            }
            .encode(),
            "free 120 -40 size 900 600"
        );
        assert_eq!(
            Placement {
                locked: false,
                position: None,
                size: Some((900, 600)),
                always_on_top: false,
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
                // The flag is the one fact such a row does NOT carry forward
                // unchanged — see
                // `a_row_from_before_the_toggle_no_longer_floats_unasked`,
                // which is where that cost is argued rather than restated.
                always_on_top: false,
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
                always_on_top: false,
            }
        );
    }

    /// Story 75.2's default, asserted from the outside: what a row written by
    /// **every build before this one** decodes to now.
    ///
    /// This inverts Story 48.4's test of the same shape, and the inversion is
    /// the story. 48.4 wrote the `top` tag only when the flag was OFF, so an
    /// untagged row was "on top" and a pinned window was indistinguishable from
    /// a window nobody had ever pinned. AD-259 says nothing floats until it is
    /// asked to, and under the old spelling no existing row could express
    /// having been asked — so the spelling inverted with the default: the tag
    /// is written when the flag is ON, `top 0` still reads off, and every row
    /// on every machine today comes back unpinned.
    ///
    /// The cost is stated plainly because it is real and it is one-way: a
    /// person who pinned the window before this build loses that pin once, on
    /// upgrade, and pins it again. The alternative — keeping the absent tag
    /// meaning "on" — is a default that changes nothing for anybody who already
    /// has a row, which is everybody the owner was speaking for.
    ///
    /// Spelled against literal rows rather than against `Placement::default()`,
    /// for 48.4's reason and unchanged by the flip: comparing decode to the
    /// default keeps passing if someone moves both at once, which is exactly
    /// the change this test exists to catch.
    #[test]
    fn a_row_from_before_the_toggle_no_longer_floats_unasked() {
        for raw in [
            "",
            "locked",
            "free",
            "free 120 -40",
            "locked 0 0",
            "free size 900 600",
            "free 120 -40 size 900 600",
            // The spelling 48.4 actually wrote for "off". It meant off then and
            // it means off now — the flip cost this row nothing.
            "free 120 -40 top 0",
            // Unreadable rows too: a row keeper cannot parse costs the user
            // their geometry, and the flag it cannot read is the flag nobody
            // asked for.
            "banana",
            "written by a later build",
            "free 1 2 dimensions 560 340",
        ] {
            assert!(
                !Placement::decode(raw).always_on_top,
                "{raw} floated a window nobody asked to float"
            );
        }

        // And the one row that does float: the spelling this build writes. A
        // pin the owner set is a preference and survives every open (AD-259) —
        // the asymmetry with the lock, in one assertion.
        assert!(Placement::decode("free 120 -40 top 1").always_on_top);
        assert!(
            Placement::decode("free 120 -40 top 1")
                .opened()
                .always_on_top,
            "opening the window threw away a pin the owner asked for"
        );
    }

    /// The flag survives the trip through the settings table, in every
    /// combination of the two optional groups it has to coexist with.
    ///
    /// The `free 1 2 top 0` case is the one that matters and the reason
    /// `decode` peeks per tag instead of consuming a fixed triple: a window
    /// that was MOVED but never RESIZED has a position, no size, and a flag,
    /// and the previous spelling read `top` and `0` as the size it was looking
    /// for and threw both away.
    #[test]
    fn the_flag_round_trips_beside_a_position_and_a_size() {
        for placement in [
            Placement {
                locked: true,
                position: None,
                size: None,
                always_on_top: false,
            },
            Placement {
                locked: false,
                position: None,
                size: None,
                always_on_top: false,
            },
            // Moved, never resized — the case the old decode could not carry.
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: None,
                always_on_top: false,
            },
            // Resized, never moved.
            Placement {
                locked: false,
                position: None,
                size: Some((900, 600)),
                always_on_top: false,
            },
            // Both, and locked on top of it.
            Placement {
                locked: true,
                position: Some((-15, 900)),
                size: Some((1_280, 800)),
                always_on_top: false,
            },
        ] {
            assert_eq!(
                Placement::decode(&placement.encode()),
                placement,
                "{} did not survive its own encoding",
                placement.encode()
            );
        }

        // The exact persisted spelling, pinned. Since Story 75.2 the tag is
        // written ONLY when the flag is ON, because that is the case a row has
        // to be able to state — off is what every row that says nothing means.
        assert_eq!(
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: None,
                always_on_top: true,
            }
            .encode(),
            "free 120 -40 top 1"
        );
        assert_eq!(
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: Some((900, 600)),
                always_on_top: true,
            }
            .encode(),
            "free 120 -40 size 900 600 top 1"
        );
        // Not on top: no tag at all, and therefore the same bytes 46.15 wrote
        // for a window that had no flag to write.
        assert_eq!(Placement::default().encode(), "locked");
        assert_eq!(
            Placement {
                locked: false,
                position: Some((120, -40)),
                size: None,
                always_on_top: false,
            }
            .encode(),
            "free 120 -40"
        );
    }

    /// A row carrying the flag is read for the flag and for everything else in
    /// it. Two separate failures hide here, and both were real before the
    /// per-tag peek: losing the flag, and losing the *position* because the
    /// size branch consumed the flag's two words looking for a size.
    #[test]
    fn reading_the_flag_costs_the_row_none_of_its_other_facts() {
        let moved_and_pinned = Placement::decode("free 120 -40 top 1");
        assert!(moved_and_pinned.always_on_top);
        assert_eq!(moved_and_pinned.position, Some((120, -40)));
        assert_eq!(moved_and_pinned.size, None);
        assert!(!moved_and_pinned.locked);

        let resized_and_pinned = Placement::decode("free size 900 600 top 1");
        assert!(resized_and_pinned.always_on_top);
        assert_eq!(resized_and_pinned.position, None);
        assert_eq!(resized_and_pinned.size, Some((900, 600)));

        let everything = Placement::decode("locked -15 900 size 1280 800 top 1");
        assert!(everything.always_on_top);
        assert!(everything.locked);
        assert_eq!(everything.position, Some((-15, 900)));
        assert_eq!(everything.size, Some((1_280, 800)));

        // …and the flag does not disturb an unreadable size in the same row.
        let bad_size = Placement::decode("free 1 2 size 0 340 top 1");
        assert!(bad_size.always_on_top);
        assert_eq!(bad_size.size, None);
        assert_eq!(bad_size.position, Some((1, 2)));

        // The off spelling 48.4 wrote is still read for everything else in its
        // row too — the flip changed what an ABSENT tag means, not how a
        // present one is consumed, and a row that still carries `top 0` must
        // not lose its position to the words `top` and `0`.
        let pre_flip = Placement::decode("free 120 -40 top 0");
        assert!(!pre_flip.always_on_top);
        assert_eq!(pre_flip.position, Some((120, -40)));
    }

    /// Every unreadable spelling of the flag answers `false`, because floating
    /// over every other application is a thing keeper does only when it is
    /// asked, and a row keeper half-understands is not an ask (Story 75.2,
    /// AD-259). Only the exact word `1` floats a window.
    ///
    /// The direction reversed with the default, and that is the point: under
    /// 48.4 an unreadable flag could not cost a pin, and now it cannot invent
    /// one. Both readings protect the same thing — the state the user did not
    /// choose is never the state a damaged row produces.
    #[test]
    fn an_unreadable_flag_never_floats_a_window_nobody_floated() {
        // Explicitly off, which `encode` no longer writes but every pre-75.2
        // row may still carry.
        assert!(!Placement::decode("free 1 2 top 0").always_on_top);
        // Truncated, misspelled, and a value nobody writes.
        assert!(!Placement::decode("free 1 2 top").always_on_top);
        assert!(!Placement::decode("free 1 2 top on").always_on_top);
        assert!(!Placement::decode("free 1 2 top true").always_on_top);
        assert!(!Placement::decode("free 1 2 ontop 1").always_on_top);
        // A tag nobody writes does not eat the position, either.
        assert_eq!(Placement::decode("free 1 2 ontop 1").position, Some((1, 2)));
        // Only this floats it.
        assert!(Placement::decode("free 1 2 top 1").always_on_top);
    }

    /// The flag is a property of the window, not of the lock or the size, so
    /// changing either must not disturb it. This is the "flag survives a size
    /// change" contract: `window_size` and `adopted_position` read the geometry
    /// and answer about geometry, and a `Placement` re-encoded after a resize
    /// carries the same flag out that it carried in.
    #[test]
    fn the_flag_is_untouched_by_locking_and_by_resizing() {
        let unpinned = Placement {
            locked: false,
            position: Some((10, 20)),
            size: Some((900, 600)),
            always_on_top: false,
        };

        // Resized: only the size moves.
        let resized = Placement {
            size: Some((1_024, 768)),
            ..unpinned
        };
        assert!(!Placement::decode(&resized.encode()).always_on_top);
        assert_eq!(
            Placement::decode(&resized.encode()).size,
            Some((1_024, 768))
        );

        // Locked: since Story 75.1 the lock FREEZES the size rather than
        // normalising it, and it still says nothing about the flag — which is
        // the claim this test is actually making, and the one arm change did
        // not touch.
        let locked = Placement {
            locked: true,
            ..unpinned
        };
        assert_eq!(locked.window_size(None), Some((900, 600)));
        assert!(!Placement::decode(&locked.encode()).always_on_top);
        // The remembered size is still in the row, exactly as before 48.4.
        assert_eq!(Placement::decode(&locked.encode()).size, Some((900, 600)));
    }

    /// Which of the four answers each state gets, and the one that is `None`.
    ///
    /// This test used to be called
    /// `a_locked_window_is_normalised_and_an_unsized_one_is_left_alone`, and
    /// the first half of that name is now a lie: Story 75.1 (AD-257) rescinded
    /// the normalisation. It is renamed rather than deleted because the second
    /// half is still load-bearing and the arms it covers are still the arms.
    #[test]
    fn a_lock_freezes_the_size_it_finds_and_an_unsized_window_is_left_alone() {
        let screen = Some((1_920u32, 1_080u32));

        // Locked with nothing remembered: keeper's own size. There is nothing
        // else to honour, and a locked window has to have a size to be locked
        // at — this is the arm that keeps a never-resized window from opening
        // at whatever the webview happened to lay out.
        assert_eq!(
            Placement::default().window_size(screen),
            Some(CAPTURE_DEFAULT_SIZE)
        );

        // Locked WITH a remembered size: that size. This is the whole of Story
        // 75.1 — the owner resizes, presses the padlock, and the window stays
        // where it was instead of snapping back to 560×340. Delete the
        // `(true, Some(size))` arm and this is the assertion that fails.
        assert_eq!(
            Placement {
                locked: true,
                position: None,
                size: Some((1_400, 900)),
                always_on_top: false,
            }
            .window_size(screen),
            Some((1_400, 900))
        );

        // Unlocked and never resized: do not touch the window. NOT the
        // default — the live window may hold a size the user chose seconds ago
        // that no blur has written down yet.
        assert_eq!(
            Placement {
                locked: false,
                position: Some((10, 10)),
                size: None,
                always_on_top: false,
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
                always_on_top: false,
            }
            .window_size(screen),
            Some((900, 600))
        );
    }

    /// The clamp did not move with the arm, and it now guards a case it never
    /// used to see: before Story 75.1 a locked window asked for
    /// [`CAPTURE_DEFAULT_SIZE`], which fits every display anybody works on, so
    /// the ceiling only ever had unlocked placements to cut down. Now a locked
    /// row can carry 3000 px from the desk monitor and be restored on the
    /// laptop panel — with the lock making it the one window the user cannot
    /// drag back into view.
    #[test]
    fn a_locked_window_restored_on_a_smaller_screen_is_still_cut_down() {
        let from_the_big_monitor = Placement {
            locked: true,
            position: None,
            size: Some((3_000, 2_000)),
            always_on_top: false,
        };
        assert_eq!(
            from_the_big_monitor.window_size(Some((1_440, 900))),
            Some((1_440, 900))
        );
        // The floor applies to a locked row too: a size below what the window
        // can hold is raised, not honoured.
        assert_eq!(
            Placement {
                locked: true,
                position: None,
                size: Some((1, 1)),
                always_on_top: false,
            }
            .window_size(Some((1_920, 1_080))),
            Some(CAPTURE_MIN_SIZE)
        );
    }

    /// Story 75.2, AD-258: opening the window clears the lock and **nothing
    /// else**.
    ///
    /// All four fields are asserted, deliberately. A test that checked only
    /// `locked` would pass against the obvious wrong implementation — replacing
    /// the stored row with a fresh `Placement` — which is exactly the mistake
    /// the story's "the reset is one field, not a wipe" sentence is about: it
    /// would throw away the position and the size that ride in the same row,
    /// and the pin the owner explicitly asked for.
    #[test]
    fn opening_the_window_clears_the_lock_and_keeps_everything_else() {
        let stored = Placement {
            locked: true,
            position: Some((120, -40)),
            size: Some((900, 600)),
            always_on_top: true,
        };
        let opened = stored.opened();

        assert!(!opened.locked, "the window came back locked");
        assert_eq!(
            opened.position,
            Some((120, -40)),
            "opening the window forgot where it was"
        );
        assert_eq!(
            opened.size,
            Some((900, 600)),
            "opening the window forgot how big it was"
        );
        assert!(
            opened.always_on_top,
            "opening the window un-pinned a window the owner pinned"
        );

        // And the geometry is not merely carried — it is ANSWERED with, which
        // is what the shell actually asks for. Unlocking changes neither
        // answer, because both were already the user's.
        assert_eq!(opened.adopted_position(), Some((120, -40)));
        assert_eq!(opened.window_size(Some((1_920, 1_080))), Some((900, 600)));

        // Idempotent, because both shell paths may reach it and one of them
        // runs on a row the other already opened: boot adopts the stored row
        // and the hotkey reveals the same window later in the same session.
        assert_eq!(opened.opened(), opened);

        // An unlocked row is untouched in every field, so routing a path that
        // did not need the reset through it costs nothing.
        let already_free = Placement {
            locked: false,
            position: Some((10, 10)),
            size: None,
            always_on_top: false,
        };
        assert_eq!(already_free.opened(), already_free);
    }

    /// DW-198: the position must answer the same shape of question the size
    /// does, because the whole complaint was that the two disagreed — "it
    /// remembers how big I made it but not where I put it".
    #[test]
    fn an_unlocked_window_is_put_back_and_a_locked_one_still_follows_the_pointer() {
        // Locked with nothing remembered: keeper places it, as it always has.
        assert_eq!(Placement::default().adopted_position(), None);

        // Locked but carrying a position — unlock, drag, lock again. The
        // position is KEPT (locking is not a discard button) and deliberately
        // NOT adopted: a locked panel follows the pointer between monitors, and
        // that is the cost DW-198 weighs and the lock's own promise.
        assert_eq!(
            Placement {
                locked: true,
                position: Some((120, -40)),
                size: None,
                always_on_top: true,
            }
            .adopted_position(),
            None
        );

        // Unlocked and never moved: keeper places it. "Never moved" is not
        // "moved to the default", exactly as it is not for the size.
        assert_eq!(
            Placement {
                locked: false,
                position: None,
                size: Some((900, 600)),
                always_on_top: true,
            }
            .adopted_position(),
            None
        );

        // Unlocked and moved: back where the person put it, negative
        // coordinates included — a second monitor to the left of the primary
        // one is an ordinary desk.
        assert_eq!(
            Placement {
                locked: false,
                position: Some((-1_400, 220)),
                size: None,
                always_on_top: true,
            }
            .adopted_position(),
            Some((-1_400, 220))
        );
    }

    /// DW-198's other half, as Story 75.2 narrowed it. Adopting the position at
    /// boot buys nothing if the next hotkey press re-centres the window, so
    /// `show` leaves a window where it is — and since every window is unlocked
    /// at every open, the only one keeper still places is one whose backend
    /// will not say whether it is resizable. Decided from a live window
    /// attribute either way, with no settings read in front of the hot path
    /// (NFR-27).
    #[test]
    fn the_hotkey_leaves_a_window_that_answers_and_places_only_one_that_does_not() {
        assert_eq!(plan_show_position(true), ShowPosition::Leave);
        assert_eq!(plan_show_position(false), ShowPosition::Place);
    }

    /// DW-199: the inset is a number the platform decides, and it is `0` in
    /// every state where tao does not hit-test an edge. All four states, because
    /// a test that only checked unlocked-GTK would ship a permanent gutter on
    /// macOS — a control moved on three platforms to fix a problem on one.
    #[test]
    fn the_chrome_is_inset_only_where_the_resize_border_actually_is() {
        let gtk = EdgeResize {
            inside_client_area: true,
            resizable: true,
            maximized: false,
            scale: 1,
        };

        // Unlocked GTK window: tao hit-tests a 5 px strip and the close button
        // is flush into the corner where two of them overlap.
        assert_eq!(chrome_edge_inset(gtk), TAO_EDGE_BORDER);

        // …and 10 on a 2× display, which is the whole reason this is not a CSS
        // constant. `scale_factor() * 5`, straight out of tao.
        assert_eq!(
            chrome_edge_inset(EdgeResize { scale: 2, ..gtk }),
            2 * TAO_EDGE_BORDER
        );

        // Locked: tao's guard is `&& is_resizable()`, so there is no border and
        // an inset would be a gap over nothing.
        assert_eq!(
            chrome_edge_inset(EdgeResize {
                resizable: false,
                ..gtk
            }),
            0
        );

        // Maximized: tao's guard is `&& !is_maximized()`. Same reasoning, and
        // it is the state a person is most likely to be in when they reach for
        // close.
        assert_eq!(
            chrome_edge_inset(EdgeResize {
                maximized: true,
                ..gtk
            }),
            0
        );

        // macOS and Windows: the resize border is OUTSIDE the client area, so
        // the webview owns every pixel of its own corner.
        assert_eq!(
            chrome_edge_inset(EdgeResize {
                inside_client_area: false,
                ..gtk
            }),
            0
        );
        assert_eq!(
            chrome_edge_inset(EdgeResize {
                inside_client_area: false,
                resizable: true,
                maximized: false,
                scale: 2,
            }),
            0,
            "a retina Mac is the case a scale-only rule would get wrong"
        );

        // A platform that will not name a scale factor costs one border, never
        // a hidden control: `0` and `1` mean the same unscaled display.
        assert_eq!(
            chrome_edge_inset(EdgeResize { scale: 0, ..gtk }),
            TAO_EDGE_BORDER
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
            always_on_top: true,
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
            always_on_top: true,
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
        // The floor is below keeper's own size, or the answer for a locked
        // window that remembers no size would be one the clamp then has to
        // raise — a default nobody could actually receive.
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
            always_on_top: true,
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
            always_on_top: true,
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
            // Deliberately the NON-default value: serialized as `true` this
            // assertion would pass just as well against a field that was
            // hard-coded, and it would stop being a test of the flag.
            always_on_top: false,
            visible: true,
            chrome_inset: chrome_edge_inset(EdgeResize {
                inside_client_area: true,
                resizable: true,
                maximized: false,
                scale: 2,
            }),
        };
        assert_eq!(
            serde_json::to_string(&vm).expect("serialize window"),
            r#"{"key":"note:vault-a/note-1","target":{"kind":"note","vaultId":"vault-a","noteId":"note-1"},"locked":false,"alwaysOnTop":false,"visible":true,"chromeInset":10}"#
        );
        assert_eq!(
            serde_json::to_string(&CaptureTargetVm::Draft).expect("serialize draft"),
            r#"{"kind":"draft"}"#
        );
    }

    // -----------------------------------------------------------------------
    // Story 48.2 — the lock stops discarding a size, and nothing leaves the
    // screen
    // -----------------------------------------------------------------------

    /// The size a person dragged the window to, and keeper's own.
    const CHOSEN: (u32, u32) = (900, 600);

    /// What an unlocked window reports about itself.
    fn dragged(position: (i32, i32), size: (u32, u32)) -> Observed {
        Observed {
            position: Some(position),
            size: Some(size),
            user_controlled: true,
        }
    }

    /// What a *locked* window reports — the same three readings, from a window
    /// keeper rather than the user is placing.
    ///
    /// The size became a parameter in Story 75.1. It used to be
    /// [`CAPTURE_DEFAULT_SIZE`] unconditionally, because locking normalised the
    /// window to keeper's size; a lock now freezes whatever is there, so the
    /// honest reading from a locked window is the size it was locked at. The
    /// fact that makes these readings discardable was never the size anyway —
    /// it is `user_controlled: false`.
    fn placed(position: (i32, i32), size: (u32, u32)) -> Observed {
        Observed {
            position: Some(position),
            size: Some(size),
            user_controlled: false,
        }
    }

    /// The headline defect of Story 48.2, and the sentence Story 46.15's spec
    /// promised and could not keep: *"the remembered size is kept, so unlocking
    /// restores it"*.
    ///
    /// Walked as the three commands walk it, because no single call is wrong —
    /// the lock click is correct, the unlock click is correct in isolation, and
    /// the loss only exists in the sequence.
    #[test]
    fn resizing_then_locking_then_unlocking_gives_the_user_their_size_back() {
        // Unlocked, dragged to 900×600 at (120, -40).
        let stored = Placement {
            locked: false,
            position: Some((120, -40)),
            size: Some(CHOSEN),
            ..Placement::default()
        };

        // Click the padlock. The window is still the user's when it is
        // measured, so its geometry is worth writing down.
        let locked = stored.relocked(dragged((120, -40), CHOSEN), true);
        assert_eq!(
            locked.size,
            Some(CHOSEN),
            "locking must keep the size, not discard it"
        );
        assert_eq!(
            locked.position,
            Some((120, -40)),
            "'unlock, drag, lock again' is a person saying keep it THERE"
        );
        // ...and since Story 75.1 the live window is still 900×600: the whole
        // sequence this test walks no longer changes the size at any step,
        // which is what the owner meant by the padlock.
        assert_eq!(locked.window_size(Some((1_920, 1_080))), Some(CHOSEN));

        // Click it again. THIS is where the size used to die: the live window
        // was the normalised one, and merging its size over the stored one
        // overwrote 900×600 with 560×340 a moment before `window_size` was
        // asked to restore it. 75.1 removed the normalisation, so the reading
        // is no longer wrong — but the guard is still what stops a
        // keeper-placed reading from overwriting the row, and a window locked
        // before it was ever resized still reports keeper's size.
        let unlocked = locked.relocked(placed((120, -40), CHOSEN), false);
        assert_eq!(
            unlocked.size,
            Some(CHOSEN),
            "unlocking must restore the size the user chose, not keeper's"
        );
        assert_eq!(
            unlocked.window_size(Some((1_920, 1_080))),
            Some(CHOSEN),
            "and the window it puts on screen is that size"
        );
    }

    /// The same loss without anybody pressing the padlock twice. Blur writes
    /// the geometry down too, so one click on another app after locking was
    /// enough — which is why the guard cannot live in the lock command.
    #[test]
    fn a_blur_while_locked_does_not_cost_the_size_or_the_position() {
        let locked = Placement {
            locked: true,
            position: Some((120, -40)),
            size: Some(CHOSEN),
            ..Placement::default()
        };

        // The hotkey has since re-placed this locked panel a fifth of the way
        // down the pointer's monitor (Story 47.5, DW-198), so its live
        // coordinate is keeper's and its live size is keeper's.
        let after_blur = locked.observing(placed((680, 216), CAPTURE_DEFAULT_SIZE));

        assert_eq!(after_blur, locked, "a locked window has nothing to report");
        assert_eq!(
            after_blur
                .relocked(placed((680, 216), CAPTURE_DEFAULT_SIZE), false)
                .size,
            Some(CHOSEN),
            "so the later unlock still finds the user's size"
        );
    }

    /// The other half of the guard, and the one a too-eager fix would break:
    /// an unlocked window's geometry IS the user's and must still be recorded,
    /// on blur and on close alike.
    #[test]
    fn an_unlocked_window_still_writes_down_everything_it_reports() {
        let stored = Placement {
            locked: false,
            position: Some((120, -40)),
            size: Some(CHOSEN),
            ..Placement::default()
        };
        assert_eq!(
            stored.observing(dragged((300, 20), (1_024, 768))),
            Placement {
                locked: false,
                position: Some((300, 20)),
                size: Some((1_024, 768)),
                ..stored
            },
            "a resize and a drag both survive the blur that follows them"
        );
    }

    /// A platform that answers one question and not the other still gets the
    /// readable half remembered — Story 46.15's rule, unchanged, and worth
    /// pinning because the guard sits on the same expression.
    #[test]
    fn a_half_answer_keeps_the_readable_half_and_invents_nothing() {
        let stored = Placement {
            locked: false,
            position: Some((120, -40)),
            size: Some(CHOSEN),
            ..Placement::default()
        };
        let size_only = Observed {
            position: None,
            size: Some((1_024, 768)),
            user_controlled: true,
        };
        assert_eq!(stored.observing(size_only).position, Some((120, -40)));
        assert_eq!(stored.observing(size_only).size, Some((1_024, 768)));

        let says_nothing = Observed {
            user_controlled: true,
            ..Observed::default()
        };
        assert_eq!(
            stored.observing(says_nothing),
            stored,
            "a window that answers neither question changes neither field"
        );
    }

    /// A window that will not say whether it is resizable is treated as
    /// keeper's, so an unanswering backend costs a remembered geometry and can
    /// never overwrite one. `Observed::default()` is that window, and it is
    /// what the shell returns for a window that is not open at all.
    #[test]
    fn a_window_that_will_not_say_is_never_taken_for_the_users() {
        let stored = Placement {
            locked: false,
            position: Some((120, -40)),
            size: Some(CHOSEN),
            ..Placement::default()
        };
        assert!(!Observed::default().user_controlled);
        assert_eq!(
            stored.observing(Observed {
                position: Some((0, 0)),
                size: Some((320, 240)),
                user_controlled: false,
            }),
            stored
        );
    }

    /// The lock is still a lock. The guard decides what is *remembered* and
    /// must not touch who may move the window.
    #[test]
    fn the_toggle_still_sets_the_lock_whatever_the_window_reports() {
        let unlocked = Placement {
            locked: false,
            ..Placement::default()
        };
        assert!(unlocked.relocked(dragged((1, 2), CHOSEN), true).locked);
        assert!(
            !unlocked
                .relocked(placed((1, 2), CAPTURE_DEFAULT_SIZE), false)
                .locked
        );
        assert!(
            !Placement::default()
                .relocked(Observed::default(), false)
                .locked,
            "a window that reports nothing still unlocks"
        );
    }

    /// The 1920×1080 primary display, at the desktop origin.
    const PRIMARY: WorkArea = WorkArea {
        position: (0, 0),
        size: (1_920, 1_080),
    };

    /// The owner's second sentence — *"moze wyjsc poza monitor"* — in its first
    /// reachable form: a window GROWS from the same top-left, and nothing used
    /// to move it afterwards.
    ///
    /// Story 75.1 closed the route this test was named for — a lock freezes the
    /// remembered size instead of growing it to keeper's — and left the test,
    /// because the clamp is what the story was ever about and the growth still
    /// happens: a window locked before anything was ever written down has no
    /// remembered size, so it still gets [`CAPTURE_DEFAULT_SIZE`], and every
    /// other caller of `clamp_position` (the hotkey's re-placement, a restore
    /// onto a monitor that has gone) hands it a size the position was not
    /// chosen for.
    #[test]
    fn locking_a_small_window_in_the_corner_does_not_push_it_off_the_screen() {
        // 320×240 parked hard against the bottom-right corner.
        let parked = (1_600, 840);
        assert_eq!(
            clamp_position(parked, (320, 240), Some(PRIMARY)),
            parked,
            "where it already fits, nothing moves"
        );
        // Grown to 560×340 without repositioning, 240 px of window — including
        // the corner the close button is in — went past the edge.
        assert_eq!(
            clamp_position(parked, CAPTURE_DEFAULT_SIZE, Some(PRIMARY)),
            (1_360, 740),
            "the grown window is pulled back until its far corner is on screen"
        );
    }

    /// The second reachable form: a coordinate remembered on a display this
    /// machine no longer has, replayed verbatim onto a rectangle of desktop
    /// with no pixels behind it. The window is undecorated and `skipTaskbar`,
    /// so there is nothing left to click.
    #[test]
    fn a_position_from_a_monitor_that_is_gone_lands_on_one_that_is_not() {
        assert_eq!(
            clamp_position((3_400, 1_500), CAPTURE_DEFAULT_SIZE, Some(PRIMARY)),
            (1_360, 740)
        );
        assert_eq!(
            clamp_position((-2_000, -900), CAPTURE_DEFAULT_SIZE, Some(PRIMARY)),
            (0, 0),
            "a monitor that used to be to the LEFT is the same defect mirrored"
        );
    }

    /// The work area's origin is a field, not an assumption. Without it every
    /// window on every non-primary monitor is dragged onto the primary one,
    /// which is a worse bug than the one being fixed.
    #[test]
    fn a_second_monitors_origin_is_not_the_desktops() {
        let right_hand = WorkArea {
            position: (1_920, 0),
            size: (2_560, 1_440),
        };
        assert_eq!(
            clamp_position((2_400, 300), CAPTURE_DEFAULT_SIZE, Some(right_hand)),
            (2_400, 300),
            "a window sitting happily on the second monitor is not touched"
        );
        assert_eq!(
            clamp_position((0, 0), CAPTURE_DEFAULT_SIZE, Some(right_hand)),
            (1_920, 0),
            "and one clamped INTO it lands at that monitor's near edge"
        );
        assert_eq!(
            clamp_position((9_000, 9_000), CAPTURE_DEFAULT_SIZE, Some(right_hand)),
            (1_920 + 2_560 - 560, 1_440 - 340)
        );
    }

    /// `clamp_size`'s zero-work-area guard, restated for a position: a monitor
    /// mid-reconfiguration reports one, and pinning a window to a corner of a
    /// screen that is about to stop being that shape is not an improvement on
    /// leaving it alone. Each axis is guarded separately, exactly as the size
    /// clamp guards them.
    #[test]
    fn a_zero_work_area_moves_a_window_not_at_all() {
        let nothing = WorkArea {
            position: (0, 0),
            size: (0, 0),
        };
        assert_eq!(
            clamp_position((3_400, 1_500), CAPTURE_DEFAULT_SIZE, Some(nothing)),
            (3_400, 1_500)
        );
        let half = WorkArea {
            position: (0, 0),
            size: (1_920, 0),
        };
        assert_eq!(
            clamp_position((3_400, 1_500), CAPTURE_DEFAULT_SIZE, Some(half)),
            (1_360, 1_500),
            "a readable width still clamps; an unreadable height does not"
        );
    }

    /// No monitor at all — a headless session, or a compositor that will not
    /// answer. Nothing is invented from nothing, matching `clamp_size`'s
    /// `None` arm.
    #[test]
    fn an_unknown_display_leaves_a_position_exactly_where_it_was() {
        assert_eq!(
            clamp_position((3_400, 1_500), CAPTURE_DEFAULT_SIZE, None),
            (3_400, 1_500)
        );
    }

    /// A window bigger than the space it is on cannot be fully inside it, and
    /// the clamp pushes rather than shrinks — the size is `clamp_size`'s
    /// business and a clamp that changed both would fight it. The near edge is
    /// the answer because that is where the drag strip is.
    #[test]
    fn a_window_bigger_than_the_work_area_stays_reachable_at_the_near_edge() {
        let tiny = WorkArea {
            position: (40, 20),
            size: (400, 300),
        };
        assert_eq!(clamp_position((300, 200), (560, 340), Some(tiny)), (40, 20));
        assert_eq!(
            clamp_position((-500, -500), (560, 340), Some(tiny)),
            (40, 20)
        );
    }

    /// keeper's own placement, moved out of the shell by this story so that it
    /// is checked on a machine the shell does not build on (AD-55/AD-56).
    /// These are the shell's own numbers: `centred(1920, 560) == 680` and
    /// `offset_from_top(1080, 340) == 216`.
    #[test]
    fn an_unplaced_panel_is_centred_a_fifth_of_the_way_down() {
        assert_eq!(auto_position(CAPTURE_DEFAULT_SIZE, PRIMARY), (680, 216));
        assert_eq!(
            auto_position(
                CAPTURE_DEFAULT_SIZE,
                WorkArea {
                    position: (0, 0),
                    size: CAPTURE_DEFAULT_SIZE
                }
            ),
            (0, 0),
            "an exact fit sits flush at the top-left"
        );
    }

    /// The clamp inside `auto_position` is what used to be `offset_from_top`'s
    /// `.min(free)`: a panel taller than a fifth of the room left sits higher
    /// up rather than hanging its bottom edge off the screen.
    #[test]
    fn an_unplaced_panel_never_hangs_off_the_monitor_it_is_placed_on() {
        let short = WorkArea {
            position: (0, 0),
            size: (400, 400),
        };
        assert_eq!(
            auto_position(CAPTURE_DEFAULT_SIZE, short),
            (0, 60),
            "a fifth of 400 is 80, but only 60 px of room is left below"
        );
        let cramped = WorkArea {
            position: (0, 0),
            size: (400, 100),
        };
        assert_eq!(
            auto_position(CAPTURE_DEFAULT_SIZE, cramped),
            (0, 0),
            "an impossible fit sits at the top rather than at a negative edge"
        );
    }

    /// A panel placed on the monitor the pointer is on, when that is not the
    /// primary one. The shell's old arithmetic added the work area's origin by
    /// hand; dropping it here would centre every panel on the wrong screen.
    #[test]
    fn an_unplaced_panel_is_centred_on_the_monitor_it_belongs_on() {
        let right_hand = WorkArea {
            position: (1_920, 100),
            size: (1_920, 1_080),
        };
        assert_eq!(
            auto_position(CAPTURE_DEFAULT_SIZE, right_hand),
            (1_920 + 680, 100 + 216)
        );
    }
}
