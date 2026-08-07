//! Where a recording lands, as a template the user owns (Epic 40, Story 40.1).
//!
//! A session folder is no longer a fixed name — it is a **relative path**
//! rendered beneath the destination root from a template the user can edit.
//! Nesting by year, by month, by client, or not at all is then a template edit
//! rather than a checkbox, and the default sorts chronologically in Finder,
//! `ls`, the Files app and `git log` with no metadata read anywhere.
//!
//! # Tokens
//!
//! | Token     | Renders                                                             | Example        |
//! |-----------|---------------------------------------------------------------------|----------------|
//! | `{yyyy}`  | four-digit year                                                     | `2026`         |
//! | `{yy}`    | two-digit year                                                      | `26`           |
//! | `{mm}`    | **month**, zero-padded                                              | `08`           |
//! | `{dd}`    | day of the month, zero-padded                                       | `05`           |
//! | `{HH}`    | hour on a 24-hour clock, zero-padded                                | `14`           |
//! | `{MM}`    | **minute**, zero-padded                                             | `32`           |
//! | `{SS}`    | second, zero-padded                                                 | `07`           |
//! | `{title}` | the title: illegal characters removed, whitespace runs collapsed to one space, trimmed, **at most 80 characters** | `Café Standup` |
//! | `{slug}`  | the title, folded to a slug: lower-case, diacritics dropped, every other character a single `-`, **at most 60 characters** | `cafe-standup` |
//! | `{seq}`   | collision ordinal — nothing, or ` (2)`                              | ` (2)`         |
//!
//! `{mm}` is the **month** and `{MM}` is the **minute** — case-sensitive, and
//! the one pair worth reading twice. `{seq}` belongs to the **last** folder and
//! is refused anywhere else: it exists to give a colliding recording a sibling,
//! and a `{seq}` further up would rename a folder that holds other recordings.
//!
//! The two title tokens cut a long title at **different** lengths, and on
//! purpose: 80 characters is what a folder name can show and stay a name, while
//! `{slug}`'s 60 is `notes::naming`'s cap, inherited whole so a recording folder
//! and a note filename fold one title the same way. Both caps count
//! *characters*, never bytes, and cut on a character boundary. A third cut can
//! still follow: the finished folder is capped at 255 **bytes**, so a title in a
//! script whose characters are three or four bytes wide may lose more than these
//! two rules alone would take.
//!
//! This is deliberately the same vocabulary the journal template already
//! publishes for notes (AD-65): `{yyyy}`, `{yy}`, `{mm}` and `{dd}` mean here
//! exactly what they mean in
//! [`crate::notes::naming::journal_path`] — its sibling, and the file to change
//! in step with this one. A user who has written one template has learned both,
//! and two vocabularies that drift apart would be a bug nobody files.
//!
//! # Guarantees
//!
//! [`PathTemplate::parse`] **validates, never sanitises**: an illegal template
//! is refused with a typed reason the settings UI can print, and is never
//! quietly rewritten into a path the user did not ask for. A folder whose name
//! **does not depend on the title** is therefore decided entirely at parse: its
//! rendered name is knowable here, so one that is too long for a directory entry
//! and one that spells an MS-DOS device name are refused rather than shortened
//! or suffixed behind the user's back. The *last* such folder is measured as it
//! renders at the **widest** collision ordinal, because the ordinal is the one
//! text `render` may still have to add to it — and measuring the seq-1 form and
//! subtracting a constant is not the same thing, since a `{seq}` of the user's
//! own brings back the separators standing beside it as well. A folder
//! holding `{title}` or `{slug}` keeps the render-time rules instead, because
//! there the name depends on a title that only arrives with the recording — and
//! a title is data, which is filtered rather than refused.
//! [`PathTemplate::render`] is then **infallible** — every decision was made at
//! parse time — and its output holds unconditionally, for every template that
//! parsed and every title that exists:
//!
//! - no `:` anywhere, and nothing else from the illegal set
//!   (`< > : " / \ | ? *`, `NUL`, control characters and Unicode *format*
//!   characters — the union of what APFS, exFAT and NTFS refuse, so a FAT
//!   pendrive stays a legal destination, plus the ones they accept but no
//!   human can see: an invisible folder name is not a folder anyone can find,
//!   and a right-to-left override makes Finder draw a name backwards);
//! - never absolute, no leading or trailing separator;
//! - no component that is empty, `.`, `..` or an MS-DOS device name;
//! - no component with a leading or trailing space or `.`;
//! - the **final** component always renders, because it *is* the session
//!   folder: a template whose last folder is built only from tokens that may
//!   collapse is refused at parse ([`TemplateError::OptionalLeaf`]) rather than
//!   allowed to promote the year directory into a session folder;
//! - **every** component, the collision ordinal included, is at most 255 bytes
//!   — `NAME_MAX` on APFS, ext4 and exFAT — because every component is a
//!   directory that has to be created, and a caller retrying with a longer name
//!   never chases a length the filesystem will refuse forever. The ordinal is
//!   never what gets cut to reach that length — two ordinals always name two
//!   folders — though a folder long enough to be clamped has it re-attached at
//!   the **end**, wherever in the component an explicit `{seq}` had written it:
//!   the clamp keeps a prefix, so the only place an ordinal can survive one is
//!   after the cut;
//! - the collision ordinal only ever renames the final component, so a retry is
//!   always a sibling of the same depth: `{seq}` is refused anywhere but the
//!   last folder ([`TemplateError::SeqOutsideLeaf`]);
//! - brackets **pair**: a token that renders nothing takes a matching `()` or
//!   `[]` with it, never one half of one, so the brackets a rendered name owes
//!   to the *template* are exactly the ones the template wrote, minus whole
//!   pairs. A **title** is data and is quoted, not parsed: a title that arrives
//!   holding a lone `(` renders it, exactly as it renders any other legal
//!   character, and it is never claimed as half of a pair the template wrote;
//! - and a title can never introduce more components than the template's own
//!   `/` count — a hostile title cannot deepen or escape the path.
//!
//! # Depth
//!
//! A template writes at most [`crate::recording::RECOVERY_MAX_DEPTH`] folders,
//! and that number is **imported rather than repeated**: it is the depth the
//! recovery walks descend. A session recorded deeper than they walk is one
//! neither the salvage pass nor the recovered-card scan can ever reach, so the
//! template that would have put it there is refused
//! ([`TemplateError::TooDeep`]) instead — a legal-looking template whose only
//! symptom is a recording lost by the crash it was supposed to survive is not a
//! trade worth the extra folder. Raising the cap is one edit, in
//! `recording.rs`, and the walks and this parser move together.
//!
//! # Purity
//!
//! No clock, no filesystem, no ambient state: the civil date-time, the title
//! and the collision ordinal all arrive in a [`RenderCtx`]. The shell owns the
//! clock and the retry loop that discovers the ordinal.

use std::fmt;

use crate::notes::naming::{slug_stem, RESERVED_DEVICE_NAMES};

/// The template a fresh install records with: nested by year, date-first, and
/// titled when there is a title.
pub const DEFAULT_TEMPLATE: &str = "{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}";

/// Character cap on a rendered `{title}`. Long enough for a real meeting name,
/// short enough that a title in any ordinary script reads as a folder name
/// rather than as a paragraph. It is a legibility rule and cannot also be the
/// byte rule — 80 four-byte codepoints are 320 bytes — so [`NAME_MAX_BYTES`] is
/// enforced separately, on each finished component, where the date prefix and
/// the collision ordinal are finally known.
const TITLE_MAX_CHARS: usize = 80;

/// Byte cap on **every** rendered component, the collision ordinal included:
/// `NAME_MAX` on APFS, ext4 and exFAT alike, and the only one of the three
/// limits a title can realistically reach.
///
/// Every component is a directory `mkdir` has to create, so a 320-byte first
/// component fails exactly as hard as a 320-byte leaf — and worse, because
/// story 40.3's collision loop only ever varies the *last* component and only
/// ever *lengthens* it. A component that is already too long would make every
/// retry fail the same way, forever.
const NAME_MAX_BYTES: usize = 255;

/// What a component that folded onto an MS-DOS device name gains, so `nul`
/// becomes `nul-rec` and the folder can exist on Windows at all.
const RESERVED_SUFFIX: &str = "-rec";

/// Why a template was refused.
///
/// Each message stands alone as a sentence and names the offending character or
/// token: the settings field prints it inline, next to the input, with no
/// heading and no modal around it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    /// A `.` or `..` component — a template must not be able to walk out of the
    /// destination root. Surrounding spaces do not make it a different folder,
    /// so `  ..  ` is refused exactly like `..`.
    #[error("a template cannot contain a \".\" or \"..\" folder")]
    ParentComponent,
    /// A folder with nothing in it: a doubled `/`, a trailing `/`, or a folder
    /// written out of characters that a folder name may not begin or end with,
    /// which would leave nothing behind.
    #[error(
        "a folder in a template cannot be empty: look for a doubled \"/\", a \"/\" at the end, or a folder made only of spaces and dots"
    )]
    EmptyComponent,
    /// A folder that begins or ends in text the user typed, whose leading or
    /// trailing spaces and dots a folder name cannot keep. Rendering it would
    /// have to strip them, and a template is validated rather than rewritten — a
    /// preview that disagrees with the field above it, with no error, explains
    /// nothing. A token elsewhere in the folder buys those edges no exemption:
    /// `{yyyy}/..{mm}` loses exactly what `{yyyy}/  ..  /{mm}` is refused for.
    #[error(
        "the folder \"{0}\" starts or ends with a space or a \".\", which a folder name cannot: write it without them"
    )]
    PaddedComponent(
        /// The offending component, exactly as the user wrote it.
        String,
    ),
    /// A folder whose name owes nothing to the title, longer than a directory
    /// entry may be. Its rendered name is knowable here, so the bytes are
    /// countable here — and a clamp at render time would delete characters the
    /// user typed without saying so, beside a preview that would then disagree
    /// with the field above it. The last folder is measured against a smaller
    /// budget than the rest, because it is the one `render` may still have to
    /// append a collision ordinal to.
    #[error(
        "the folder \"{0}\" is longer than a folder name can be: a name holds at most 255 bytes, and the last folder keeps a few of them back for a collision ordinal"
    )]
    OverlongComponent(
        /// The offending component, exactly as the user wrote it.
        String,
    ),
    /// A folder whose name owes nothing to the title, spelling a name MS-DOS
    /// took first. Windows refuses it in every directory, with or without an
    /// extension, and the `-rec` a *title* would gain here is a rewrite of a
    /// specification rather than of data — so it is refused instead.
    #[error(
        "the folder \"{0}\" is a device name Windows keeps for itself (con, prn, aux, nul, com1-com9, lpt1-lpt9), so no folder can be called that"
    )]
    ReservedComponent(
        /// The offending component, exactly as the user wrote it.
        String,
    ),
    /// The template starts at the filesystem root.
    #[error("a template is a path inside the destination folder, so it cannot start with \"/\"")]
    Absolute,
    /// A character no folder name may contain on macOS, Windows or a FAT
    /// pendrive.
    #[error("the character {ch:?} cannot be used in a folder name")]
    IllegalCharacter {
        /// The offending character.
        ch: char,
    },
    /// A `{…}` that is not one of the documented tokens.
    #[error("{{{0}}} is not one of the tokens a template understands")]
    UnknownToken(String),
    /// A `{` with no closing `}`.
    #[error("a token is missing its closing \"}}\"")]
    Unterminated,
    /// Nothing, or nothing but whitespace.
    #[error("a template cannot be empty")]
    Empty,
    /// Every folder in the template is optional, so an untitled recording would
    /// have nowhere to go. Inventing an "Untitled" placeholder is exactly what
    /// this epic refuses, so the template is refused instead.
    #[error(
        "this template can render to nothing at all: at least one folder must contain a date or time token, or some text"
    )]
    MayRenderEmpty,
    /// The *last* folder is built only from tokens that may collapse, so an
    /// untitled recording would have no folder of its own and would be written
    /// into its parent — which the collision ordinal would then rename.
    ///
    /// "Some text of its own" is advice that can be followed literally, brackets
    /// included: a bracket is text, and a pair leaves only when it encloses the
    /// token that collapsed. `({slug})` is refused, and `(x{slug})` and
    /// `({slug}` both render — one character, anywhere in the folder, is enough.
    #[error(
        "the last folder, \"{0}\", can render to nothing, which would put the recording in its parent folder: give it a date or time token, or some text of its own"
    )]
    OptionalLeaf(
        /// The offending component, exactly as the user wrote it.
        String,
    ),
    /// A `{seq}` above the last folder. The ordinal exists to give a colliding
    /// recording a *sibling*; upstream of the leaf it renames a folder that
    /// holds other recordings (`2026`, `2026 (2)`, …) and, worse, suppresses
    /// the leaf's own ordinal, so the second recording of a minute can land
    /// *inside* the first.
    #[error(
        "the folder \"{0}\" uses {{seq}}, which belongs in the last folder only: a second recording has to become a folder beside the first, not a rename of the folder they share"
    )]
    SeqOutsideLeaf(
        /// The offending component, exactly as the user wrote it.
        String,
    ),
    /// More folders than a recovery walk descends. A session recorded below
    /// [`crate::recording::RECOVERY_MAX_DEPTH`] folders is one neither the
    /// salvage pass nor the card scan can reach, so a crash there loses it
    /// silently and forever — the one template fault whose cost is invisible
    /// until the day it matters. Refused at parse, where saying so is free.
    #[error(
        "a template can be at most {max} folders deep, and this one is {0}: a recording nested any deeper is one keeper could not find again after a crash",
        max = crate::recording::RECOVERY_MAX_DEPTH
    )]
    TooDeep(
        /// How many folders the template writes.
        usize,
    ),
}

/// A rendered path, relative to the destination root, `/`-separated on every
/// platform.
///
/// Only [`PathTemplate::render`] can build one, which is what makes the
/// guarantees in the module doc a property of the type rather than of a call
/// site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelativePath(String);

impl RelativePath {
    /// The path as written, e.g. `2026/2026-08-05 1432 standup`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path's components, in order. Never empty, and never yields an empty
    /// component.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Display for RelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Everything a template is allowed to know about the recording being named.
///
/// The date-time is *civil* — already resolved to the user's wall clock by the
/// shell, exactly like `notes::templates::Stamp` — because a core that read the
/// clock could not be tested.
///
/// Every range below is checked by a `debug_assert!` in [`PathTemplate::render`]
/// rather than left as prose. The fields are `pub` because the shell builds one
/// field by field, and two of the module's guarantees quietly depend on the
/// ranges holding: `{yyyy}` is a *fixed*-width four digits only inside
/// `0..=9999`, which is what lets `parse` decide a folder's length before the
/// recording exists; and `seq` is 1-based, so a caller counting from zero would
/// render its second recording into the first one's folder with nothing to say
/// so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderCtx {
    /// Full year, `0..=9999` — the range in which it renders as four digits and
    /// `{yy}` as two.
    pub year: i32,
    /// Month, `1..=12`.
    pub month: u32,
    /// Day of the month, `1..=31`.
    pub day: u32,
    /// Hour on a 24-hour clock, `0..=23`.
    pub hour: u32,
    /// Minute, `0..=59`.
    pub minute: u32,
    /// Second, `0..=59`.
    pub second: u32,
    /// The user's title for this recording, if it has one.
    pub title: Option<String>,
    /// This folder's ordinal among siblings that rendered to the same path.
    /// **1-based**: `1` is the first and adds nothing, so the caller can write
    /// `for seq in 1..` with no off-by-one, and the second folder keeps the
    /// familiar ` (2)`.
    pub seq: u32,
}

/// A parsed, validated template. Parse once when the setting is edited, render
/// once per recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTemplate {
    raw: String,
    /// One entry per `/`-separated folder, each a run of literals and tokens.
    components: Vec<Vec<Segment>>,
    /// Whether the user placed `{seq}` themselves — always in the final
    /// component, because `parse` refuses it anywhere else. If they did not,
    /// the collision ordinal is appended to that component instead.
    has_seq: bool,
}

impl PathTemplate {
    /// Validate `input`, or say precisely what is wrong with it.
    pub fn parse(input: &str) -> Result<Self, TemplateError> {
        // Both of these questions are about the path the user *meant*, so both
        // are asked of the trimmed input. Asking `Absolute` of the raw one made
        // `"  /Users/x/{yyyy}"` an `EmptyComponent` — true of the padding, and
        // advice that sends the reader hunting for a doubled slash that is not
        // there. The padding itself is still refused below, by the rule that is
        // actually about padding.
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(TemplateError::Empty);
        }
        if trimmed.starts_with('/') {
            return Err(TemplateError::Absolute);
        }

        let mut components: Vec<Vec<Segment>> = Vec::new();
        let mut current: Vec<Segment> = Vec::new();
        let mut literal = String::new();
        let mut seq_in: Option<usize> = None;
        let mut chars = input.chars();

        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    let mut name = String::new();
                    let mut closed = false;
                    for c in chars.by_ref() {
                        if c == '}' {
                            closed = true;
                            break;
                        }
                        // A token name is quoted straight back at the user by
                        // `UnknownToken`, and that message is 40.2's inline UI
                        // copy — so `{\u{202e}gpj}` would have put a
                        // right-to-left override into the settings pane, where
                        // it reverses the rest of the sentence. The character is
                        // refused here for the same reason it is refused
                        // anywhere else in the template.
                        if is_illegal(c) {
                            return Err(TemplateError::IllegalCharacter { ch: c });
                        }
                        name.push(c);
                    }
                    if !closed {
                        return Err(TemplateError::Unterminated);
                    }
                    let Some(token) = Token::from_name(&name) else {
                        return Err(TemplateError::UnknownToken(name));
                    };
                    // The *first* folder that mentions `{seq}`: if that one is
                    // not the leaf the template is refused, so a second `{seq}`
                    // in the leaf can never mask a misplaced one above it.
                    if token == Token::Seq && seq_in.is_none() {
                        seq_in = Some(components.len());
                    }
                    push_literal(&mut current, &mut literal);
                    current.push(Segment::Token(token));
                }
                // The one character that is legal in a template and can never
                // survive inside a rendered component: it *is* the separator.
                '/' => {
                    push_literal(&mut current, &mut literal);
                    components.push(std::mem::take(&mut current));
                }
                c if is_illegal(c) => return Err(TemplateError::IllegalCharacter { ch: c }),
                c => literal.push(c),
            }
        }
        push_literal(&mut current, &mut literal);
        components.push(current);

        // What the user typed is a specification, not data: a folder `render`
        // would have had to rewrite is refused here instead of quietly
        // rewritten. The rule is about *typed literals*, and a literal does not
        // stop being typed because a token shares its folder — scoping the
        // check to token-free folders left `{yyyy}/..{mm}` deleting the same two
        // characters `{yyyy}/  ..  /{mm}` is refused for. So the check is on a
        // component's outer **edges**, wherever those edges are literal.
        // Interior literals are left alone: they are the separators the collapse
        // rule governs, and without them `{slug}` could not vanish from
        // `… {HH}{MM} {slug}` without leaving its space behind.
        let leaf_index = components.len().saturating_sub(1);
        for (index, component) in components.iter().enumerate() {
            if component.is_empty() {
                return Err(TemplateError::EmptyComponent);
            }
            // A folder written as text and nothing else has two shapes that
            // leave *nothing* to keep, and each gets the reason it can act on:
            // "write it without them" is no advice for a `..` or a `//`.
            if let Some(text) = typed_text(component) {
                let bare = text.trim_matches(char::is_whitespace);
                if bare == "." || bare == ".." {
                    return Err(TemplateError::ParentComponent);
                }
                if text.trim_matches(is_edge_noise).is_empty() {
                    return Err(TemplateError::EmptyComponent);
                }
            }
            let opens_padded = matches!(
                component.first(),
                Some(Segment::Literal(text)) if text.starts_with(is_edge_noise)
            );
            let closes_padded = matches!(
                component.last(),
                Some(Segment::Literal(text)) if text.ends_with(is_edge_noise)
            );
            if opens_padded || closes_padded {
                return Err(TemplateError::PaddedComponent(describe(component)));
            }
            // …and the two rewrites `render` would otherwise have made in
            // silence. They come after the padding check because padding is the
            // more specific fault: ` nul ` is a folder to write without its
            // spaces before it is a folder named after a device.
            //
            // Both are decided here for the same reason the ones above are: a
            // folder whose name owes nothing to the title renders text this
            // function can already read, so its length and its fold are known.
            // A folder holding `{title}` or `{slug}` keeps the render-time clamp
            // and `de_reserve`, because there they act on a *title* — data that
            // arrives with the recording, which this module filters rather than
            // refuses.
            if !holds_title_token(component) {
                let typed = title_free_render(component, &reference_ctx());
                // The leaf is the one folder `render` may still lengthen, so it
                // is measured **as it will actually render** at the widest
                // ordinal — not as it renders at seq 1, less a constant. The
                // two are not the same folder when the template wrote its own
                // `{seq}`: at seq 1 the ordinal is a gap that takes the
                // separator beside it as well, and at seq 2 that separator comes
                // back *with* the ordinal. Measured the wrong way,
                // `x` + `y`×242 + `-`×10 + `{seq}` read as 242 bytes, parsed,
                // and then lost the ten dashes the user typed on the first
                // collision — the very rewrite the reservation exists to
                // prevent. The widest ordinal rather than the one in hand,
                // because `parse` does not know which seq 40.3's retry loop will
                // reach.
                let measured = if index == leaf_index {
                    let widest = title_free_render(component, &widest_ordinal_ctx());
                    // …and where the template did *not* write `{seq}`, `render`
                    // appends the ordinal itself, so the room for it is added
                    // back here rather than rendered.
                    let appended = if holds_seq_token(component) {
                        0
                    } else {
                        seq_suffix(u32::MAX).len()
                    };
                    widest.len() + appended
                } else {
                    typed.len()
                };
                if measured > NAME_MAX_BYTES {
                    return Err(TemplateError::OverlongComponent(describe(component)));
                }
                // The device name is asked of the seq-1 form, which is the
                // folder the *first* recording of the minute gets: `nul{seq}`
                // reads `nul (2)` at a collision, and a template that names a
                // device before it collides is refused for the folder it would
                // have created first.
                if reserved_stem(&typed).is_some() {
                    return Err(TemplateError::ReservedComponent(describe(component)));
                }
            }
        }
        // Asked once the loop above has established that every component IS a
        // folder — an empty one is a doubled `/`, not a folder to count, and
        // "look for a doubled slash" is the advice that reader can act on. What
        // is counted here is folders, so the sentence that reports the count is
        // true.
        if components.len() > crate::recording::RECOVERY_MAX_DEPTH {
            return Err(TemplateError::TooDeep(components.len()));
        }
        // `{seq}` renames the folder it sits in. That is the point in the leaf —
        // the folder that collided — and a bug anywhere above it, where the
        // folder being renamed holds other recordings.
        if let Some(component) = seq_in
            .filter(|index| index + 1 != components.len())
            .and_then(|index| components.get(index))
        {
            return Err(TemplateError::SeqOutsideLeaf(describe(component)));
        }
        if !components.iter().any(|c| renders_something(c)) {
            return Err(TemplateError::MayRenderEmpty);
        }
        // …and when something *would* have rendered, the leaf is the one
        // component that must be part of it. The rendered path is the session
        // folder; a leaf that collapses hands that role to the year directory
        // above it, and the collision ordinal then renames the year.
        if let Some(leaf) = components.last() {
            if !renders_something(leaf) {
                return Err(TemplateError::OptionalLeaf(describe(leaf)));
            }
        }

        Ok(Self {
            raw: input.to_owned(),
            components,
            // Validated above: if there is a `{seq}` at all, it is in the leaf.
            has_seq: seq_in.is_some(),
        })
    }

    /// The template exactly as the user wrote it — nothing was rewritten.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Render this template against `ctx`. Infallible by construction.
    pub fn render(&self, ctx: &RenderCtx) -> RelativePath {
        debug_assert_ctx(ctx);

        let raw_title = ctx.title.as_deref().unwrap_or_default();
        let title = sanitize_title(raw_title);
        let slug = render_slug(raw_title);

        // The collision ordinal belongs to the leaf — the folder that is
        // actually colliding — and to nothing else. Either the template wrote
        // `{seq}` there itself, in which case it was rendered in place, or it is
        // appended below; `ordinal` is the same text either way, and the leaf
        // has to keep room for it in *both* orderings. Appended, that room is a
        // smaller budget; rendered in place, it is a reservation `fit_component`
        // makes when it clamps, because a clamp returns a prefix and the ordinal
        // sits at the end of the component — exactly where a prefix cut lands.
        let ordinal = if ctx.seq > 1 {
            seq_suffix(ctx.seq)
        } else {
            String::new()
        };
        let appended = if self.has_seq { "" } else { ordinal.as_str() };
        let leaf_index = self.components.len().saturating_sub(1);

        let mut rendered: Vec<String> = Vec::with_capacity(self.components.len());
        for (index, component) in self.components.iter().enumerate() {
            // Every component is a directory that has to be created, so every
            // component is measured — the leaf against a budget the appended
            // ordinal has already been taken out of, so appending it below
            // cannot overrun.
            let is_leaf = index == leaf_index;
            let budget = if is_leaf {
                NAME_MAX_BYTES.saturating_sub(appended.len())
            } else {
                NAME_MAX_BYTES
            };
            // The ordinal a clamp of *this* component would have to put back.
            let in_place = if is_leaf && self.has_seq {
                ordinal.as_str()
            } else {
                ""
            };

            let mut text =
                finish_component(&render_component(component, ctx, &title, &slug), in_place);
            if text.len() > budget {
                text = fit_component(component, ctx, &title, &slug, budget, in_place);
            }

            // A component that renders to nothing takes its separator with it,
            // which is why this drops the component rather than pushing an
            // empty one: `{yyyy}/{slug}/x` is `2026/x`, never `2026//x`. The
            // leaf never takes this path — `parse` guaranteed it renders, and
            // the guarantee is asserted rather than left to arithmetic three
            // functions away, because a leaf that vanished here would silently
            // promote the year directory into the session folder.
            debug_assert!(
                !(is_leaf && text.is_empty()),
                "the leaf of {:?} rendered nothing, which `parse` promised it could not",
                self.raw
            );
            if text.is_empty() {
                continue;
            }
            if is_leaf {
                text.push_str(appended);
            }
            rendered.push(text);
        }

        RelativePath(rendered.join("/"))
    }
}

/// A piece of one component: text the user typed, or a token to resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Token(Token),
}

/// The closed token vocabulary. See the module doc for the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    Year4,
    Year2,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    Title,
    Slug,
    Seq,
}

impl Token {
    /// The spelling this token was written with. The exact inverse of
    /// [`Token::from_name`], so a component can be quoted back to the user in
    /// an error in the words they typed rather than in the words of the IR.
    fn name(self) -> &'static str {
        match self {
            Self::Year4 => "yyyy",
            Self::Year2 => "yy",
            Self::Month => "mm",
            Self::Day => "dd",
            Self::Hour => "HH",
            Self::Minute => "MM",
            Self::Second => "SS",
            Self::Title => "title",
            Self::Slug => "slug",
            Self::Seq => "seq",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "yyyy" => Self::Year4,
            "yy" => Self::Year2,
            "mm" => Self::Month,
            "dd" => Self::Day,
            "HH" => Self::Hour,
            "MM" => Self::Minute,
            "SS" => Self::Second,
            "title" => Self::Title,
            "slug" => Self::Slug,
            "seq" => Self::Seq,
            _ => return None,
        })
    }

    /// Optional tokens may render to nothing — and when they do, they remove
    /// themselves and let the separators they stood between merge into one.
    /// The date and time tokens always render, which is what makes a component
    /// containing one a folder that is guaranteed to exist.
    fn is_optional(self) -> bool {
        matches!(self, Self::Title | Self::Slug | Self::Seq)
    }
}

/// One rendered piece, kept separate until the collapse pass has run.
///
/// A literal splits into up to three of these, because only its *edges* are
/// separators a collapse may eat: `x -` is text, then filler. Brackets are not
/// separators at all — they are text, and they leave in [pairs](pair_brackets).
enum Unit {
    /// Text nothing may remove: a token's own output, or the middle of a
    /// literal.
    Text {
        text: String,
        /// A rendered `{seq}`, which grows its own separating space at join
        /// time — after the collapse pass, because the collapse is what decides
        /// whether there is already a space in front of it.
        seq: bool,
        /// Whether this text is a literal the user typed, rather than a token's
        /// output. Only a literal's brackets may be claimed as half of a pair:
        /// a title is *data*, and the `(` in a title of `"("` is a character
        /// the recording arrived with, not punctuation this module wrote around
        /// something. Without the distinction, `{title}{slug})` with that title
        /// deleted the user's own bracket and the template's `)` together.
        from_literal: bool,
    },
    /// A run of separator characters the user typed. The only thing a collapse
    /// is allowed to remove, and only when a token that rendered empty is
    /// standing next to it.
    Filler { text: String },
    /// An optional token that rendered to nothing.
    Gap {
        /// Whether a matching pair of brackets left with this token. The pair
        /// was text standing between two neighbours, so when the collapse
        /// leaves nothing at all between them, a single space takes its place —
        /// a lone bracket could not, without becoming unbalanced.
        bracketed: bool,
    },
}

fn push_literal(current: &mut Vec<Segment>, literal: &mut String) {
    if !literal.is_empty() {
        current.push(Segment::Literal(std::mem::take(literal)));
    }
}

/// The text a component renders when it holds no token at all — which is
/// exactly the text the user typed, and therefore knowable at parse time. `None`
/// when a token shares the folder and the name depends on the recording.
///
/// This is the narrow question: is the folder *literally* what the user wrote?
/// Only the `.`/`..` and "nothing left" rejections need it that narrow, because
/// only they read the text as characters the user typed rather than as a name
/// that will exist. [`title_free_render`] asks the wider one.
fn typed_text(component: &[Segment]) -> Option<&str> {
    match component {
        [Segment::Literal(text)] => Some(text),
        _ => None,
    }
}

/// The name a component will render when nothing in it depends on the title —
/// text `parse` can already count and read. `None` when a `{title}` or a
/// `{slug}` shares the folder.
///
/// "Holds no token at all" was the wrong line and `{seq}` walked straight
/// through it: `{yyyy}/nul{seq}` rendered `2026/nul-rec` in silence while
/// `{yyyy}/nul` was refused, and `{yyyy}/x…{seq}` clamped fifty typed characters
/// away while the same folder without `{seq}` was refused. Everything but a
/// title is knowable here — a date or time token renders a fixed run of digits
/// at every instant, and `{seq}` renders nothing until a second recording of the
/// same minute exists — so the line is drawn at the title, which is the only
/// input this module treats as data.
///
/// The ordinal a *later* seq would add is not part of this text; the leaf's
/// budget keeps room for it instead, which is the only way to be right about a
/// seq the retry loop has not reached yet.
fn title_free_render(component: &[Segment], ctx: &RenderCtx) -> String {
    debug_assert!(
        !holds_title_token(component),
        "the title is not knowable here"
    );
    let text = render_component(component, ctx, "", "");
    // Trimmed the way `render` trims it, but *not* de-reserved: the whole point
    // of asking is to catch the device name before `de_reserve` hides it.
    text.trim_matches(is_edge_noise).to_owned()
}

/// Whether this component's rendered name depends on the recording's title, and
/// so cannot be decided until the recording exists. See [`title_free_render`].
fn holds_title_token(component: &[Segment]) -> bool {
    component
        .iter()
        .any(|segment| matches!(segment, Segment::Token(Token::Title | Token::Slug)))
}

/// Whether the template wrote the collision ordinal into this component itself,
/// in which case `render` does not append a second one.
fn holds_seq_token(component: &[Segment]) -> bool {
    component
        .iter()
        .any(|segment| matches!(segment, Segment::Token(Token::Seq)))
}

/// An untitled recording that has not collided, at an instant no parse-time
/// question may depend on. Both such questions — "can this folder render to
/// nothing" and "what does a folder that ignores the title render" — are asked
/// of it, through the renderer, so neither can drift from what `render` does.
fn reference_ctx() -> RenderCtx {
    RenderCtx {
        year: 2000,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        title: None,
        // 1 is "no collision", so `{seq}` renders nothing and collapses.
        seq: 1,
    }
}

/// [`reference_ctx`] at the *widest* collision ordinal, which is how the leaf's
/// length is measured at parse: `parse` cannot know which seq 40.3's retry loop
/// will reach, so the only figure that holds for all of them is the largest one.
fn widest_ordinal_ctx() -> RenderCtx {
    RenderCtx {
        seq: u32::MAX,
        ..reference_ctx()
    }
}

/// The ranges [`RenderCtx`] documents, checked where the only caller that can be
/// wrong is the shell.
///
/// Two of them are load-bearing rather than tidy. `{yyyy}` renders four digits
/// only inside `0..=9999`, and `parse` decides a title-free folder's length from
/// exactly that; and `seq` is 1-based, so `0` renders byte-identically to `1` and
/// a caller counting from zero would quietly write its second recording into the
/// first one's folder.
fn debug_assert_ctx(ctx: &RenderCtx) {
    debug_assert!(
        (0..=9999).contains(&ctx.year),
        "year {} is outside 0..=9999, where it stops being four digits wide",
        ctx.year
    );
    debug_assert!((1..=12).contains(&ctx.month), "month {}", ctx.month);
    debug_assert!((1..=31).contains(&ctx.day), "day {}", ctx.day);
    debug_assert!(ctx.hour <= 23, "hour {}", ctx.hour);
    debug_assert!(ctx.minute <= 59, "minute {}", ctx.minute);
    debug_assert!(ctx.second <= 59, "second {}", ctx.second);
    debug_assert!(ctx.seq >= 1, "seq is 1-based; {} names no folder", ctx.seq);
}

/// Whether this component is guaranteed to render to something, so the path can
/// never be empty — the single predicate behind both
/// [`TemplateError::MayRenderEmpty`] and [`TemplateError::OptionalLeaf`].
///
/// It *asks the renderer*, rather than restating the collapse rule in a second
/// walk that could drift from the first. The two used to be independent — one
/// over `Segment`s, one over `Unit`s — and the leaf guarantee, the whole reason
/// this module refuses `{yyyy}/{title}`, rests entirely on them agreeing. Now
/// they cannot: "can this folder render to nothing" is answered by rendering it,
/// so there is one walk and no second answer to drift from it.
///
/// What that leaves for a test to pin is the *wiring*, not the agreement:
/// whether `parse` asks this of the right component, whether the leaf rule and
/// `MayRenderEmpty` keep their precedence, and whether `render` then drops
/// exactly the components this returned `false` for. That is what
/// `renders_something_agrees_with_what_render_does` checks, and it is worth
/// checking — but it is not evidence that the collapse rule is right, because
/// both sides of it now come from the same code.
///
/// The untitled recording is the case to ask about, and it is enough: a title
/// only ever turns a [`Unit::Gap`] into text, and text is what keeps the
/// separators around it alive. Nothing a title can be makes a folder render
/// *less* — a title that contributes nothing collapses, which is exactly the
/// untitled case again.
fn renders_something(component: &[Segment]) -> bool {
    !finish_component(&render_component(component, &reference_ctx(), "", ""), "").is_empty()
}

/// A component written back out the way the user typed it, for an error that
/// has to point at one folder out of several — [elided](elide) if it is long
/// enough that quoting it whole would be the fault rather than the diagnosis.
fn describe(component: &[Segment]) -> String {
    let quoted: String = component
        .iter()
        .map(|segment| match segment {
            Segment::Literal(text) => text.clone(),
            Segment::Token(token) => format!("{{{}}}", token.name()),
        })
        .collect();
    elide(&quoted)
}

/// How much of a folder an error quotes back before it stops helping.
const QUOTED_MAX_CHARS: usize = 60;

/// `text`, cut to [`QUOTED_MAX_CHARS`] with an ellipsis.
///
/// `OverlongComponent` is raised *because* its payload is too long, so quoting
/// the payload whole made the message as unreadable as the fault: a 20 000-byte
/// folder produced a 20 000-byte sentence, which 40.2 prints inline beside the
/// settings field. The head of the folder is what identifies it out of several;
/// the rest is what the error is already about.
fn elide(text: &str) -> String {
    match text.char_indices().nth(QUOTED_MAX_CHARS) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

fn render_component(segments: &[Segment], ctx: &RenderCtx, title: &str, slug: &str) -> String {
    let mut units: Vec<Unit> = Vec::with_capacity(segments.len());
    for segment in segments {
        match segment {
            Segment::Literal(text) => push_literal_units(&mut units, text),
            Segment::Token(token) => {
                let text = render_token(*token, ctx, title, slug);
                // "Rendered empty" means empty *once edge noise is discounted*:
                // a title of `"..."` survives the title filter verbatim — dots
                // are neither whitespace nor illegal — and the folder's edge
                // trim then removes it anyway, so asking whether the token's
                // text is empty stranded the separator the collapse exists to
                // take (`{yyyy}-{mm}-{dd}_{title}` rendered `2026-08-05_`).
                // The token is optional and it contributed nothing; that is the
                // definition the collapse rule needs, and it puts `"..."` where
                // `"!!!"` already was.
                if token.is_optional() && text.trim_matches(is_edge_noise).is_empty() {
                    units.push(Unit::Gap { bracketed: false });
                } else {
                    units.push(Unit::Text {
                        text,
                        seq: *token == Token::Seq,
                        // A token's output is never the template's punctuation,
                        // whatever characters it happens to hold.
                        from_literal: false,
                    });
                }
            }
        }
    }
    pair_brackets(&mut units);
    join_units(&units)
}

/// A literal, split into the separators a collapse may reach and the text it
/// may not. Only the *edges* of a literal can ever stand next to a token, so
/// only they are filler; `a-b` is one piece of text, dash and all.
fn push_literal_units(units: &mut Vec<Unit>, text: &str) {
    push_split_units(units, text, false, true);
}

/// [`push_literal_units`] for text whose origin is already known — what
/// [`pair_brackets`] hands back when a claimed bracket leaves the rest of its
/// unit behind. `seq` and `from_literal` travel with it: a unit does not stop
/// being an ordinal, or stop being the user's own punctuation, because a
/// character was taken off its end.
fn push_split_units(units: &mut Vec<Unit>, text: &str, seq: bool, from_literal: bool) {
    let after_lead = text.trim_start_matches(is_filler);
    if after_lead.is_empty() {
        push_filler(units, text);
        return;
    }
    let lead = &text[..text.len() - after_lead.len()];
    push_filler(units, lead);
    let core = after_lead.trim_end_matches(is_filler);
    units.push(Unit::Text {
        text: core.to_owned(),
        seq,
        from_literal,
    });
    push_filler(units, &after_lead[core.len()..]);
}

fn push_filler(units: &mut Vec<Unit>, text: &str) {
    if !text.is_empty() {
        units.push(Unit::Filler {
            text: text.to_owned(),
        });
    }
}

/// **A collapsing token takes a matching pair of brackets with it**, and never
/// half of one.
///
/// Parenthesising the optional part is one of the first templates anyone writes,
/// and `({title})` untitled must not leave `()` behind — an empty pair of
/// brackets is a dangling separator and an "Untitled" placeholder at once, which
/// is what this epic refuses. Calling brackets *filler* was the wrong way to
/// reach that, because filler is directionless: each bracket then needed a
/// *facing* to know which neighbour it belonged to, and a facing rule cannot
/// keep a pair together. Measured under it, `{yyyy}-{mm}-{dd} {slug} ({HH}{MM})`
/// untitled lost the space and rendered `2026-08-05(1432)`, `{dd}({slug}){HH}`
/// welded `0514` — the welding defect this module had already fixed once — and
/// `({slug} {SS})` rendered `07)` while `{slug}(){SS}` rendered `(07`.
///
/// Pairing is symmetric, which is the whole point: a bracket is removed only
/// together with its partner, so a rendered name carries exactly the brackets
/// the template did, minus whole pairs. The rest of the collapse then runs
/// unchanged — the separators the removal exposed become [`Unit::Filler`] again
/// and [`join_units`] takes them by its ordinary exposure rule.
///
/// A bracket with no partner is then simply text, and is rendered as typed:
/// `{yyyy}/{slug}(` is `2026/(` untitled and `2026/standup(` titled, and
/// `{yyyy}/()` is `2026/()`. This module preserves the balance the user wrote
/// rather than inventing one — brackets around something that always renders are
/// brackets they asked for — and what it guarantees is that no pair is ever
/// removed by halves.
fn pair_brackets(units: &mut Vec<Unit>) {
    let mut from_start = vec![0usize; units.len()];
    let mut from_end = vec![0usize; units.len()];
    let mut paired = false;

    // **To a fixpoint**, because a pair can stand behind a pair. Claiming the
    // inner one exposes new edges, and those edges are frequently a pair as
    // well: `(({title}))` is two pairs around one token, not one pair and a
    // leftover `()`. One sweep left exactly that behind — and worse, made
    // `{yyyy}/(({title}))` a leaf that *parses*, naming every untitled session
    // `()`, then `() (2)`: the empty-bracket placeholder this epic refuses,
    // arrived at through the rule written to prevent it. Each pass claims at
    // least one character from some unit's ends or stops, and the units are
    // finite, so this ends.
    loop {
        let mut claimed = false;
        for at in 0..units.len() {
            if !matches!(units[at], Unit::Gap { .. }) {
                continue;
            }
            // The *nearest* neighbour on each side that still has text: filler,
            // the other tokens that left, and brackets an earlier pair already
            // claimed all stand aside, so `({slug}{title})` loses one pair
            // around both. A rendered ordinal stands aside too — it reads
            // `(2)`, which is this module's own punctuation rather than a
            // bracket written around anything, so it can neither be paired with
            // nor hide the text behind it. Abandoning the whole search when an
            // ordinal turned up was the earlier rule, and it made a folder's
            // brackets appear only on collision: `{mm}({slug}{seq})` rendered
            // `08` at seq 1 and `08( (2))` at seq 2.
            let neighbour = |i: usize| {
                !is_ordinal(&units[i]) && !kept_at(units, &from_start, &from_end, i).is_empty()
            };
            let Some(left) = (0..at).rev().find(|i| neighbour(*i)) else {
                continue;
            };
            let Some(right) = (at + 1..units.len()).find(|i| neighbour(*i)) else {
                continue;
            };
            // Only the *template's* punctuation pairs. A bracket that arrived
            // inside a title is data — one character of a name that came from a
            // bridge, an agent or a paste — and claiming it deleted the user's
            // own `(` together with a `)` they had written for something else.
            if !is_literal(&units[left]) || !is_literal(&units[right]) {
                continue;
            }
            let opens = kept_at(units, &from_start, &from_end, left)
                .chars()
                .next_back();
            let closes = kept_at(units, &from_start, &from_end, right).chars().next();
            if !matches!(
                (opens, closes),
                (Some('('), Some(')')) | (Some('['), Some(']'))
            ) {
                continue;
            }
            from_end[left] += 1;
            from_start[right] += 1;
            units[at] = Unit::Gap { bracketed: true };
            claimed = true;
            paired = true;
        }
        if !claimed {
            break;
        }
    }

    if !paired {
        return;
    }
    let mut rebuilt: Vec<Unit> = Vec::with_capacity(units.len());
    for (at, unit) in units.drain(..).enumerate() {
        match unit {
            Unit::Text {
                text,
                seq,
                from_literal,
            } if from_start[at] + from_end[at] > 0 => {
                // Whatever the bracket was hiding is put back through the same
                // splitter a literal goes through, so a separator the bracket
                // stood in front of becomes filler again rather than staying
                // welded to the text: `x ({title}) y` is `x y`, not `x   y`.
                let kept = drop_edges(&text, from_start[at], from_end[at]);
                push_split_units(&mut rebuilt, kept, seq, from_literal);
            }
            other => rebuilt.push(other),
        }
    }
    *units = rebuilt;
}

/// What a unit still contributes once the brackets already claimed from its ends
/// are discounted. Empty for everything that is not text — which is what makes
/// "the nearest neighbour that still has text" a single expression.
fn kept_at<'a>(units: &'a [Unit], from_start: &[usize], from_end: &[usize], at: usize) -> &'a str {
    match &units[at] {
        Unit::Text { text, .. } => drop_edges(text, from_start[at], from_end[at]),
        _ => "",
    }
}

/// `text` without its first `from_start` and last `from_end` characters.
fn drop_edges(text: &str, from_start: usize, from_end: usize) -> &str {
    let mut chars = text.chars();
    for _ in 0..from_start {
        if chars.next().is_none() {
            return "";
        }
    }
    for _ in 0..from_end {
        if chars.next_back().is_none() {
            return "";
        }
    }
    chars.as_str()
}

fn is_ordinal(unit: &Unit) -> bool {
    matches!(unit, Unit::Text { seq: true, .. })
}

/// Whether this unit is text the user typed into the template, as opposed to
/// what a token rendered. Only the former's brackets are this module's to pair.
fn is_literal(unit: &Unit) -> bool {
    matches!(
        unit,
        Unit::Text {
            from_literal: true,
            ..
        }
    )
}

/// The collapse rule: **a filler run is removed only when it is adjacent to a
/// token that rendered empty**, and runs the removal made adjacent to each other
/// merge into one.
///
/// The merge is what keeps two neighbours that both rendered from being welded
/// together: under an earlier rule where each collapsing token ate a separator
/// run of its own, `{dd} {slug}{title} {HH}` untitled rendered `0514` — two
/// removals, two runs eaten, and the day ran into the hour. Merging instead asks
/// how many separators are *left* between the two things still there, which is
/// one; and when the two runs differ the **left** one survives, because it is
/// the separator the user wrote against the text that stayed.
///
/// Adjacency is the whole of the gate. "Trim the component's edges after
/// anything collapsed" made a folder's own decoration depend on the recording
/// having a title — `_{HH}{MM}_{slug}` rendered `_1432_standup` titled but
/// `1432` untitled, so the author's underscores survived only sometimes. Filler
/// with no collapsed token beside it is text the user wrote, and it stays.
///
/// Brackets are not filler and are none of this pass's business: [`pair_brackets`]
/// has already removed the matching pairs a collapse took, and what that
/// exposed arrives here as ordinary filler.
fn join_units(units: &[Unit]) -> String {
    let mut out = String::new();
    let mut index = 0;
    while index < units.len() {
        if let Unit::Text { text, seq, .. } = &units[index] {
            // `(2)` carries its own separating space, so `{slug}{seq}` reads as
            // `standup (2)` — unless what survived the collapse already ends in
            // a separator, where a second one would only be noise. The gate is
            // *filler*, not whitespace: `{yyyy}-{mm}-{dd}-{seq}` is a user who
            // wrote their own separator, and inserting a space over it rendered
            // `2026-08-05- (2)`.
            if *seq && !text.is_empty() && !out.is_empty() && !out.ends_with(is_filler) {
                out.push(' ');
            }
            out.push_str(text);
            index += 1;
            continue;
        }

        // Everything between two pieces of surviving text: filler runs and the
        // tokens that left, taken together because what happens to one run
        // depends on what happened to its neighbours.
        let start = index;
        while index < units.len() && !matches!(units[index], Unit::Text { .. }) {
            index += 1;
        }
        let run = &units[start..index];
        let exposed = run.iter().any(|unit| matches!(unit, Unit::Gap { .. }));
        let between_text = start > 0 && index < units.len();

        if !exposed {
            for unit in run {
                if let Unit::Filler { text } = unit {
                    out.push_str(text);
                }
            }
            continue;
        }

        // What the collapse could actually reach: a separator leaves with the
        // token that was standing against it, and nothing else does.
        let mut kept = String::new();
        for (at, unit) in run.iter().enumerate() {
            let Unit::Filler { text } = unit else {
                continue;
            };
            let touching = run[..at]
                .iter()
                .any(|unit| matches!(unit, Unit::Gap { .. }))
                || run[at + 1..]
                    .iter()
                    .any(|unit| matches!(unit, Unit::Gap { .. }));
            if !touching {
                kept.push_str(text);
            }
        }
        // Two things that both rendered still have to be told apart, so when the
        // collapse ate every separator between them one comes back: the
        // leftmost, because it is the one the user wrote against the text that
        // stayed. Where a bracket pair left and there was no separator to begin
        // with, a single space stands in for it — `{dd}({slug}){HH}` is `05 14`,
        // never `0514` — because the pair is gone as a pair and half of it could
        // not come back without leaving the name unbalanced.
        if kept.is_empty() && between_text && !already_apart(&out, units.get(index)) {
            if let Some(Unit::Filler { text }) =
                run.iter().find(|unit| matches!(unit, Unit::Filler { .. }))
            {
                kept.push_str(text);
            } else if run
                .iter()
                .any(|unit| matches!(unit, Unit::Gap { bracketed: true }))
            {
                kept.push(' ');
            }
        }
        out.push_str(&kept);
        // …and when the collapse left nothing on one side, the separator has
        // nothing left to separate: it goes with the token it belonged to,
        // which is how `… {HH}{MM} {slug}` keeps no trailing space.
    }
    out
}

/// Whether a bracket already stands at this junction, so nothing has to be put
/// back between two neighbours that both rendered.
///
/// A bracket is text, but it is text that reads as an edge: `({slug} {SS})`
/// untitled wants `(07)`, not `( 07)`, and `({SS} {slug})` wants `(07)`, not
/// `(07 )`. The separator comes back only where two names would otherwise run
/// together.
fn already_apart(out: &str, next: Option<&Unit>) -> bool {
    let closes = match next {
        Some(Unit::Text { text, .. }) => text.chars().next(),
        _ => None,
    };
    matches!(out.chars().last(), Some('(' | '[')) || matches!(closes, Some(')' | ']'))
}

/// The rendered slug, filtered the way every other rendered character is.
///
/// `slug_stem` folds the *raw* title, and what it keeps is whatever
/// `char::is_alphanumeric` accepts — which excludes `Cc` and `Cf` today, and so
/// happened to make the slug path legal without ever consulting [`is_illegal`].
/// That is a coincidence to stop relying on: this module's promise is that no
/// rendered component holds a character from the illegal set, and a promise held
/// by another module's incidental behaviour is not held. The filter is a no-op
/// on every title `slug_stem` folds today, which is why `notes::naming::slug`
/// and `{slug}` still agree character for character.
fn render_slug(raw_title: &str) -> String {
    slug_stem(raw_title)
        .chars()
        .filter(|c| !is_illegal(*c))
        .collect()
}

fn render_token(token: Token, ctx: &RenderCtx, title: &str, slug: &str) -> String {
    match token {
        Token::Year4 => format!("{:04}", ctx.year),
        Token::Year2 => format!("{:02}", ctx.year.rem_euclid(100)),
        Token::Month => format!("{:02}", ctx.month),
        Token::Day => format!("{:02}", ctx.day),
        Token::Hour => format!("{:02}", ctx.hour),
        Token::Minute => format!("{:02}", ctx.minute),
        Token::Second => format!("{:02}", ctx.second),
        Token::Title => title.to_owned(),
        Token::Slug => slug.to_owned(),
        // The separating space is added at join time, once the collapse pass
        // has settled what comes before it.
        Token::Seq if ctx.seq > 1 => format!("({})", ctx.seq),
        Token::Seq => String::new(),
    }
}

fn seq_suffix(seq: u32) -> String {
    format!(" ({seq})")
}

/// The two rules a component obeys once its pieces are joined: nothing at its
/// edges that Windows would silently strip, and no MS-DOS device name.
///
/// `ordinal` is the collision suffix this component rendered *in place*, and is
/// held back while the device-name question is asked. The ordinal is this
/// module's punctuation rather than part of the name the user asked for, and
/// including it made one template answer the question two ways: `nu{slug}{seq}`
/// with a title of `"L"` folded onto `nul` at seq 1 and was escaped to
/// `nul-rec`, while at seq 2 it read `nul (2)`, which is not a device name and
/// was left alone — two siblings of one recording named on two different rules.
fn finish_component(text: &str, ordinal: &str) -> String {
    let trimmed = text.trim_matches(is_edge_noise);
    match trimmed
        .strip_suffix(ordinal)
        .filter(|_| !ordinal.is_empty())
    {
        Some(body) => format!("{}{ordinal}", de_reserve(body)),
        None => de_reserve(trimmed),
    }
}

/// Render one component again, shorter, until it fits `budget` bytes.
///
/// The title is what gives way. It is the only part of a component the user did
/// not write character by character — it is *data* — and a folder named after
/// all but the last few characters of a title is still the folder the user asked
/// for, while a folder the filesystem refuses to create is not. Dropping one
/// character frees at most four bytes, so the deficit is asked for in characters
/// conservatively: the loop never cuts more than it must, and never fewer than
/// one character, so it ends.
///
/// Every component gets this pass, not only the leaf: `{title}/{yyyy}-{mm}-{dd}`
/// puts a title in the *first* folder, where 80 emoji are 320 bytes and story
/// 40.3's retry loop — which only ever varies the last folder — could never
/// shorten it.
///
/// A component can be too long with no title in it at all — a literal the user
/// typed. `render` is infallible, so that is clamped rather than refused,
/// leaving room for the `-rec` a device name would gain back and re-running the
/// edge trim the clamp may have exposed.
///
/// `ordinal` is the collision suffix this component rendered *in place*, from a
/// `{seq}` the template wrote itself, and is empty everywhere else. A clamp
/// returns a **prefix**, and an in-place ordinal sits at the end of the
/// component — so without this the cut took the ordinal off, `has_seq`
/// suppressed the one `render` would otherwise have appended, and seq 1, 2 and 7
/// all named one folder. A collision must change the name; a cap that erases the
/// only thing telling two sessions apart is worse than a name the filesystem
/// refuses, because the filesystem at least says so.
fn fit_component(
    segments: &[Segment],
    ctx: &RenderCtx,
    title: &str,
    slug: &str,
    budget: usize,
    ordinal: &str,
) -> String {
    // How many bytes one character off the cap actually frees. A character is at
    // most four bytes, and it comes off *every* title-bearing token in the
    // component at once — so a component holding both `{title}` and `{slug}`
    // frees twice what a component holding one does, and asking for the deficit
    // in single-token characters cut it twice as far as it had to (measured: a
    // 198-byte leaf against a 255-byte budget). Still an over-estimate of what
    // one character frees, and deliberately: the loop must never ask for less
    // than it needs, or it would not end.
    let per_char = 4 * segments.iter().filter(|s| is_title_token(s)).count().max(1);
    let mut cap = title.chars().count().max(slug.chars().count());
    loop {
        let text = finish_component(
            &render_component(
                segments,
                ctx,
                &truncate_chars(title, cap),
                &truncate_chars(slug, cap),
            ),
            ordinal,
        );
        let over = text.len().saturating_sub(budget);
        if over == 0 {
            return text;
        }
        if cap == 0 {
            let room = budget.saturating_sub(RESERVED_SUFFIX.len());
            if ordinal.is_empty() {
                return finish_component(&clamp_to_fit(&text, room), "");
            }
            // Render once more with the ordinal held back — `seq: 1` is the
            // collapse rule's own way of saying "no ordinal", separators
            // included — cut that with the ordinal's room reserved, and put the
            // ordinal back on the end.
            let held_back = RenderCtx {
                seq: 1,
                ..ctx.clone()
            };
            let body = finish_component(&render_component(segments, &held_back, "", ""), "");
            let head =
                finish_component(&clamp_to_fit(&body, room.saturating_sub(ordinal.len())), "");
            return format!("{head}{ordinal}");
        }
        cap = cap.saturating_sub(over.div_ceil(per_char));
    }
}

/// Whether this segment is a token whose text comes from the title, and is
/// therefore what [`fit_component`] shortens.
fn is_title_token(segment: &Segment) -> bool {
    matches!(segment, Segment::Token(Token::Title | Token::Slug))
}

/// [`clamp_bytes`], minus any bracket the cut orphaned.
///
/// A clamp keeps a **prefix**, so a `(` whose `)` lay beyond the cut is left
/// standing alone — `(` + 400 `x`s + `{slug})` rendered a 251-byte leaf that
/// opened a bracket it never closed. The balance guarantee is stated over
/// *rendered names*, and it was enforced only in the collapse; this is the other
/// place a bracket can lose its partner.
///
/// Only the ones this cut widowed go. A bracket the user typed with no partner
/// of its own is their text and stays their text, exactly as it does when
/// nothing is clamped at all: preservation, not a rewrite.
fn clamp_to_fit(text: &str, max: usize) -> String {
    let kept = clamp_bytes(text, max).len();
    let mut orphaned: Vec<usize> = Vec::new();
    for (opens, closes) in [('(', ')'), ('[', ']')] {
        let mut open_at: Vec<usize> = Vec::new();
        for (at, c) in text.char_indices() {
            if c == opens {
                open_at.push(at);
            } else if c == closes {
                // Matched in full, and cut apart: the opener survives the clamp
                // and its partner does not.
                if let Some(from) = open_at.pop() {
                    if from < kept && at >= kept {
                        orphaned.push(from);
                    }
                }
            }
        }
    }
    text[..kept]
        .char_indices()
        .filter(|(at, _)| !orphaned.contains(at))
        .map(|(_, c)| c)
        .collect()
}

/// The first `chars` characters — never a fraction of one.
fn truncate_chars(text: &str, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((at, _)) => text[..at].to_owned(),
        None => text.to_owned(),
    }
}

/// The longest prefix of at most `max` bytes that is still whole characters.
fn clamp_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Filter a title into something a folder name can hold.
///
/// A title is *data* — it arrives from a bridge, an agent or a paste — so it is
/// filtered rather than refused: illegal characters and control codes out,
/// whitespace runs collapsed to a single space, trimmed, and capped on a
/// character boundary. Refusing to record because a title contains a slash
/// would be absurd; refusing a *template* that contains one is the point.
fn sanitize_title(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut pending_space = false;

    for c in raw.chars() {
        // Whitespace first: a tab and a newline are control characters too, and
        // a line break in a pasted title means a space, not nothing.
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if is_illegal(c) {
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }

    // Cap on a character boundary. A grapheme cluster can still be split (that
    // would need a dependency AD-55 refuses), but a codepoint never is.
    if out.chars().count() > TITLE_MAX_CHARS {
        let cut = out
            .char_indices()
            .nth(TITLE_MAX_CHARS)
            .map_or(out.len(), |(i, _)| i);
        out.truncate(cut);
    }

    out.trim().to_owned()
}

/// The device name a component folds onto, split from whatever follows it, or
/// `None` when it is an ordinary name. Reserved on Windows in *every* directory,
/// with or without an extension, so the question is asked of the stem.
///
/// One definition, two callers: `parse` refuses a folder written as text alone
/// that folds onto a device, and [`de_reserve`] escapes the one a *title* folded
/// onto. They could not usefully disagree about what a device name is.
fn reserved_stem(component: &str) -> Option<(&str, &str)> {
    let (stem, rest) = match component.find('.') {
        Some(at) => component.split_at(at),
        None => (component, ""),
    };
    RESERVED_DEVICE_NAMES
        .contains(&stem.trim().to_ascii_lowercase().as_str())
        .then_some((stem, rest))
}

/// Escape an MS-DOS device name a *title* folded onto — `nul.mp4` becomes
/// `nul-rec.mp4`, not `nul.mp4-rec`, which would still be a device.
///
/// Only a title reaches this: a folder the user wrote as text alone is refused
/// at parse with [`TemplateError::ReservedComponent`] instead, because there the
/// fold is knowable before the recording exists and suffixing it would be a
/// rewrite of what the user typed.
fn de_reserve(component: &str) -> String {
    match reserved_stem(component) {
        // The *trimmed* stem, which is the one the guard read. Windows ignores
        // trailing spaces before an extension, so `nul .txt` is the `nul` device
        // as surely as `nul.txt` is — but rebuilding from the untrimmed stem
        // wrote the escape on the far side of that space and produced
        // `nul -rec.txt`, a name nobody typed and nobody asked for.
        Some((stem, rest)) => format!("{}{RESERVED_SUFFIX}{rest}", stem.trim()),
        None => component.to_owned(),
    }
}

/// The union of what APFS, exFAT and NTFS refuse in a name, plus what they
/// accept and no human can read. `/` is included: inside a *component* it is as
/// illegal as the rest, and the template parser takes it out of the stream
/// before this is consulted.
///
/// The second half matters as much as the first. A folder name is something the
/// user has to see in Finder and type in a shell, so a name that renders to
/// nothing visible, or that Finder draws backwards, defeats the point of naming
/// it at all — see [`is_format`].
fn is_illegal(c: char) -> bool {
    matches!(
        c,
        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\0'
    ) || c.is_control()
        || is_format(c)
}

/// Unicode **format** characters, category `Cf`.
///
/// `char::is_control` covers category `Cc` and stops there, which left `U+FEFF`
/// legal as an entire template — a folder whose whole name is invisible — and
/// let the title `annual\u{202e}gpj.review` render verbatim, which Finder draws
/// as `annualweiver.jpg`: the ordinary right-to-left-override spoof, and titles
/// arrive from bridges, agents and pastes. Refusing `Cf` puts it on both sides
/// at once, since [`is_illegal`] is consulted by the template parser and by the
/// title filter alike.
///
/// `std` exposes no category test, and AD-55 refuses a unicode crate, so the
/// ranges are written out. They are **complete for Unicode 16.0**; a codepoint
/// that a later revision assigns to `Cf` would pass until this table is updated,
/// which is acceptable staleness for a folder-name filter and the reason the
/// table lives next to the rule it serves rather than in a generated file.
fn is_format(c: char) -> bool {
    matches!(c,
        '\u{00ad}'                        // soft hyphen
        | '\u{0600}'..='\u{0605}'         // Arabic number signs
        | '\u{061c}'                      // Arabic letter mark
        | '\u{06dd}'                      // Arabic end of ayah
        | '\u{070f}'                      // Syriac abbreviation mark
        | '\u{0890}'..='\u{0891}'         // Arabic pound/piastre marks
        | '\u{08e2}'                      // Arabic disputed end of ayah
        | '\u{180e}'                      // Mongolian vowel separator
        | '\u{200b}'..='\u{200f}'         // zero-width space … right-to-left mark
        | '\u{202a}'..='\u{202e}'         // bidirectional embedding and overrides
        | '\u{2060}'..='\u{2064}'         // word joiner … invisible plus
        | '\u{2066}'..='\u{206f}'         // bidirectional isolates and shaping
        | '\u{feff}'                      // zero-width no-break space (BOM)
        | '\u{fff9}'..='\u{fffb}'         // interlinear annotation
        | '\u{110bd}' | '\u{110cd}'       // Kaithi number signs
        | '\u{13430}'..='\u{1343f}'       // Egyptian hieroglyph format controls
        | '\u{1bca0}'..='\u{1bca3}'       // shorthand format controls
        | '\u{1d173}'..='\u{1d17a}'       // musical symbol beams and phrases
        | '\u{e0001}'                     // language tag
        | '\u{e0020}'..='\u{e007f}'       // tag characters
    )
}

/// Characters a collapsing token may eat on its way out: the separators a user
/// writes between tokens — and only the run standing next to the token that
/// left, never the same characters written somewhere else in the folder.
///
/// A bracket is **not** one of them. It stands *around* something rather than
/// between two things, so it leaves only with its partner and only when what
/// they enclose has gone — [`pair_brackets`], which runs before the filler rule
/// and hands it whatever the pair was hiding.
fn is_filler(c: char) -> bool {
    c.is_whitespace() || matches!(c, '.' | '-' | '_')
}

/// Characters that may not begin or end a component. Windows strips trailing
/// dots and spaces silently, which would make two distinct folders one.
fn is_edge_noise(c: char) -> bool {
    c.is_whitespace() || c == '.'
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-08-05T14:32:07 — the matrix's reference instant.
    fn ctx(title: Option<&str>, seq: u32) -> RenderCtx {
        RenderCtx {
            year: 2026,
            month: 8,
            day: 5,
            hour: 14,
            minute: 32,
            second: 7,
            title: title.map(str::to_owned),
            seq,
        }
    }

    fn template(source: &str) -> PathTemplate {
        PathTemplate::parse(source).expect("template parses")
    }

    fn render(source: &str, title: Option<&str>, seq: u32) -> String {
        template(source)
            .render(&ctx(title, seq))
            .as_str()
            .to_owned()
    }

    #[test]
    fn the_default_template_renders_a_titled_recording() {
        assert_eq!(
            render(DEFAULT_TEMPLATE, Some("Standup"), 1),
            "2026/2026-08-05 1432 standup"
        );
    }

    #[test]
    fn an_untitled_recording_loses_the_slug_and_its_separator() {
        assert_eq!(render(DEFAULT_TEMPLATE, None, 1), "2026/2026-08-05 1432");
        assert_eq!(
            render(DEFAULT_TEMPLATE, Some(""), 1),
            "2026/2026-08-05 1432"
        );
    }

    #[test]
    fn a_title_that_slugs_to_nothing_is_an_untitled_recording() {
        assert_eq!(
            render(DEFAULT_TEMPLATE, Some("!!!"), 1),
            "2026/2026-08-05 1432"
        );
        assert_eq!(
            render(DEFAULT_TEMPLATE, Some("🎉🎉"), 1),
            "2026/2026-08-05 1432"
        );
    }

    #[test]
    fn a_collapsing_token_does_not_leave_an_empty_folder() {
        // Mid-path, the collapse takes the whole component with its separator.
        assert_eq!(render("{yyyy}/{slug}/x", None, 1), "2026/x");
        assert_eq!(
            render("{yyyy}/{slug}/x", Some("Standup"), 1),
            "2026/standup/x"
        );
        assert_eq!(render("{slug}/{yyyy}", None, 1), "2026");
    }

    #[test]
    fn a_collision_suffixes_the_final_component_only() {
        assert_eq!(
            render(DEFAULT_TEMPLATE, Some("Standup"), 3),
            "2026/2026-08-05 1432 standup (3)"
        );
    }

    #[test]
    fn seq_one_and_seq_two_differ_only_in_the_final_component() {
        let parsed = template(DEFAULT_TEMPLATE);
        let first = parsed.render(&ctx(Some("Standup"), 1));
        let second = parsed.render(&ctx(Some("Standup"), 2));

        assert_ne!(first, second);
        let first: Vec<&str> = first.components().collect();
        let second: Vec<&str> = second.components().collect();
        assert_eq!(first.len(), second.len());
        assert_eq!(first[..first.len() - 1], second[..second.len() - 1]);
        assert_ne!(first.last(), second.last());
        assert_legal(
            &parsed.render(&ctx(Some("Standup"), 1)),
            &parsed,
            Some("Standup"),
        );
        assert_legal(
            &parsed.render(&ctx(Some("Standup"), 2)),
            &parsed,
            Some("Standup"),
        );
    }

    #[test]
    fn an_explicit_seq_token_places_the_ordinal_itself() {
        let source = "{yyyy}-{mm}-{dd} {slug}{seq}";
        assert_eq!(render(source, Some("Standup"), 1), "2026-08-05 standup");
        assert_eq!(render(source, Some("Standup"), 2), "2026-08-05 standup (2)");
        // With an explicit `{seq}`, nothing is appended a second time.
        assert_eq!(render(source, None, 2), "2026-08-05 (2)");
    }

    #[test]
    fn a_hostile_title_can_neither_deepen_nor_escape_the_path() {
        let source = "{yyyy}/{title} {HH}{MM}";
        let parsed = template(source);
        let rendered = parsed.render(&ctx(Some("a/b:c"), 1));

        assert_eq!(rendered.as_str(), "2026/abc 1432");
        assert_eq!(rendered.components().count(), 2);
        assert_legal(&rendered, &parsed, Some("a/b:c"));

        // `..`, a traversal attempt and a leading dot all sanitise away, and
        // the time keeps the folder from vanishing with them.
        assert_eq!(render(source, Some("../../etc"), 1), "2026/etc 1432");
        assert_eq!(render(source, Some(".."), 1), "2026/1432");
        assert_eq!(render(source, Some("C:\\Windows"), 1), "2026/CWindows 1432");
    }

    #[test]
    fn title_keeps_the_letters_and_slug_folds_them() {
        assert_eq!(
            render("{yyyy}-{title}", Some("Café Déjà Vu"), 1),
            "2026-Café Déjà Vu"
        );
        assert_eq!(
            render("{yyyy}-{slug}", Some("Café Déjà Vu"), 1),
            "2026-cafe-deja-vu"
        );
    }

    #[test]
    fn a_title_is_filtered_not_refused() {
        assert_eq!(
            render("{yyyy}-{title}", Some("  a\t\tb \n c  "), 1),
            "2026-a b c"
        );
        assert_eq!(
            render("{yyyy}-{title}", Some("no <ctrl>\u{1}\u{7f} here"), 1),
            "2026-no ctrl here"
        );
        // Capped on a character boundary, at characters and not bytes — 80 `ä`
        // are 160 bytes, well short of the leaf cap, so nothing else trims them.
        let long = render("{yyyy}/{title} {HH}{MM}", Some(&"ä".repeat(300)), 1);
        let component = long.split('/').next_back().unwrap_or_default();
        assert_eq!(component, format!("{} 1432", "ä".repeat(TITLE_MAX_CHARS)));
    }

    #[test]
    fn a_reserved_device_name_is_never_rendered_bare() {
        assert_eq!(render("{slug}/{yyyy}", Some("NUL"), 1), "nul-rec/2026");
        assert_eq!(render("{title}/{yyyy}", Some("con"), 1), "con-rec/2026");
        assert_eq!(
            render("{title}/{yyyy}", Some("aux.txt"), 1),
            "aux-rec.txt/2026"
        );
        // A name that merely starts with one is left alone.
        assert_eq!(render("{slug}/{yyyy}", Some("console"), 1), "console/2026");
    }

    #[test]
    fn the_slug_matches_what_the_notes_domain_would_produce() {
        // One fold in the crate: a recording folder and a note filename can
        // never disagree about what a title looks like.
        assert_eq!(
            render("{yyyy}-{slug}", Some("Weekly Review — Q3!"), 1),
            format!("2026-{}", crate::notes::naming::slug("Weekly Review — Q3!"))
        );
    }

    #[test]
    fn parse_refuses_a_traversal() {
        assert_eq!(
            PathTemplate::parse("../{yyyy}"),
            Err(TemplateError::ParentComponent)
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/./{mm}"),
            Err(TemplateError::ParentComponent)
        );
    }

    #[test]
    fn parse_refuses_an_absolute_path() {
        assert_eq!(
            PathTemplate::parse("/Users/x/{yyyy}"),
            Err(TemplateError::Absolute)
        );
    }

    #[test]
    fn parse_refuses_an_illegal_character_and_names_it() {
        assert_eq!(
            PathTemplate::parse("{HH}:{MM}"),
            Err(TemplateError::IllegalCharacter { ch: ':' })
        );
        assert_eq!(
            PathTemplate::parse(r"{yyyy}\{mm}"),
            Err(TemplateError::IllegalCharacter { ch: '\\' })
        );
        assert!(TemplateError::IllegalCharacter { ch: ':' }
            .to_string()
            .contains(':'));
    }

    #[test]
    fn parse_refuses_an_unknown_token_and_names_it() {
        assert_eq!(
            PathTemplate::parse("{yyyy}/{week}"),
            Err(TemplateError::UnknownToken("week".to_owned()))
        );
        assert!(PathTemplate::parse("{week}")
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default()
            .contains("{week}"));
        // Case matters: `{mm}` is the month, `{MM}` the minute, and neither is
        // spelled `{Mm}`.
        assert_eq!(
            PathTemplate::parse("{Mm}"),
            Err(TemplateError::UnknownToken("Mm".to_owned()))
        );
    }

    #[test]
    fn parse_refuses_an_unterminated_token() {
        assert_eq!(
            PathTemplate::parse("{yyyy"),
            Err(TemplateError::Unterminated)
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/{mm"),
            Err(TemplateError::Unterminated)
        );
    }

    #[test]
    fn parse_refuses_an_empty_template() {
        assert_eq!(PathTemplate::parse(""), Err(TemplateError::Empty));
        assert_eq!(PathTemplate::parse("   "), Err(TemplateError::Empty));
    }

    #[test]
    fn parse_refuses_a_template_that_guarantees_nothing() {
        assert_eq!(
            PathTemplate::parse("{slug}"),
            Err(TemplateError::MayRenderEmpty)
        );
        assert_eq!(
            PathTemplate::parse("{slug}/{title}"),
            Err(TemplateError::MayRenderEmpty)
        );
        assert_eq!(
            PathTemplate::parse("{title} {slug}{seq}"),
            Err(TemplateError::MayRenderEmpty)
        );
        // A literal that survives trimming is enough of a guarantee.
        assert_eq!(render("rec {slug}", None, 1), "rec");
    }

    #[test]
    fn parse_refuses_a_leaf_that_could_render_to_nothing() {
        // The rendered path *is* the session folder, so a leaf that vanishes
        // would hand that role to the year above it.
        assert_eq!(
            PathTemplate::parse("{yyyy}/{title}"),
            Err(TemplateError::OptionalLeaf("{title}".to_owned()))
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/{mm}/{slug}"),
            Err(TemplateError::OptionalLeaf("{slug}".to_owned()))
        );
        // …and with an explicit `{seq}` in the leaf, session 2 would land
        // *inside* session 1 rather than beside it.
        assert_eq!(
            PathTemplate::parse("{yyyy}/{slug}{seq}"),
            Err(TemplateError::OptionalLeaf("{slug}{seq}".to_owned()))
        );
        // Separators between optional tokens are filler and save nothing.
        assert_eq!(
            PathTemplate::parse("{yyyy}/{slug} - {title}"),
            Err(TemplateError::OptionalLeaf("{slug} - {title}".to_owned()))
        );
        // A trailing separator is a last folder built from nothing at all, and
        // dropping it quietly would reopen the same hole under `{yyyy}/{slug}/`.
        // It is refused as the empty folder it is, rather than as an optional
        // leaf — there is no component text to quote back, and "give it a date
        // token" is advice a stray "/" cannot follow.
        assert_eq!(
            PathTemplate::parse("{yyyy}/{slug}/"),
            Err(TemplateError::EmptyComponent)
        );

        // Precedence: when *no* component would have rendered, the reason is
        // the whole-template one, because naming the leaf would be a half-truth.
        assert_eq!(
            PathTemplate::parse("{slug}/{title}"),
            Err(TemplateError::MayRenderEmpty)
        );

        // The message quotes the component the way the user wrote it, so the
        // settings field can point at one folder out of several.
        assert!(TemplateError::OptionalLeaf("rec-{slug}".to_owned())
            .to_string()
            .contains("rec-{slug}"));

        // …and its advice — "give it some text of its own" — is advice that can
        // be followed literally, now that a bracket is text. A pair leaves only
        // when it encloses the token that collapsed, so one character anywhere
        // in the folder is enough, whether or not it is punctuation.
        assert_eq!(
            PathTemplate::parse("{yyyy}/({slug})"),
            Err(TemplateError::OptionalLeaf("({slug})".to_owned()))
        );
        assert!(PathTemplate::parse("{yyyy}/(x{slug})").is_ok());
        assert!(PathTemplate::parse("{yyyy}/({slug}").is_ok());
        assert!(PathTemplate::parse("{yyyy}/({slug}){SS}").is_ok());
        assert_eq!(render("{yyyy}/(x{slug})", None, 1), "2026/(x)");
        assert_eq!(render("{yyyy}/({slug}", None, 1), "2026/(");
    }

    #[test]
    fn parse_accepts_a_leaf_that_a_literal_or_a_time_saves() {
        assert_eq!(render("{yyyy}/rec-{slug}", None, 1), "2026/rec");
        assert_eq!(
            render("{yyyy}/rec-{slug}", Some("Standup"), 1),
            "2026/rec-standup"
        );
        assert_eq!(render("{yyyy}/{slug} {HH}{MM}", None, 1), "2026/1432");
        assert_eq!(
            render("{yyyy}/{slug} {HH}{MM}", Some("Standup"), 1),
            "2026/standup 1432"
        );
    }

    #[test]
    fn an_interior_component_may_still_collapse() {
        // Only the leaf is constrained: a middle folder is free to vanish with
        // its separator, which is how per-title nesting stays optional.
        assert_eq!(
            render("{yyyy}/{slug}/{yyyy}-{mm}-{dd}", None, 1),
            "2026/2026-08-05"
        );
        assert_eq!(
            render("{yyyy}/{slug}/{yyyy}-{mm}-{dd}", Some("Standup"), 1),
            "2026/standup/2026-08-05"
        );
    }

    #[test]
    fn parse_refuses_a_seq_above_the_last_folder() {
        // The ordinal would rename the year directory once per collision —
        // `2026`, `2026 (2)`, `2026 (3)` — which is the hazard the leaf rule
        // exists to close, arriving through a folder that *does* render.
        assert_eq!(
            PathTemplate::parse("{yyyy}{seq}/{yyyy}-{mm}-{dd} {HH}{MM}"),
            Err(TemplateError::SeqOutsideLeaf("{yyyy}{seq}".to_owned()))
        );
        // …and here the retry lands one level deeper, inside a folder named
        // `(2)`: untitled, the interior component collapses at seq 1 and
        // appears at seq 2.
        assert_eq!(
            PathTemplate::parse("{yyyy}/{slug}{seq}/{mm}-{dd}"),
            Err(TemplateError::SeqOutsideLeaf("{slug}{seq}".to_owned()))
        );
        // A second `{seq}` in the leaf does not excuse the first one above it.
        assert_eq!(
            PathTemplate::parse("{yyyy}{seq}/{mm}-{dd}{seq}"),
            Err(TemplateError::SeqOutsideLeaf("{yyyy}{seq}".to_owned()))
        );
        // The message quotes the folder the way the user wrote it.
        assert!(TemplateError::SeqOutsideLeaf("{yyyy}{seq}".to_owned())
            .to_string()
            .contains("{yyyy}{seq}"));

        // In the leaf it is exactly what it is for.
        assert_eq!(
            render("{yyyy}/{yyyy}-{mm}-{dd}{seq}", Some("Standup"), 2),
            "2026/2026-08-05 (2)"
        );
    }

    #[test]
    fn parse_caps_the_template_at_the_recovery_walks_depth() {
        use crate::recording::RECOVERY_MAX_DEPTH;

        // Exactly the cap records where both recovery walks can still reach, so
        // it parses.
        let at_cap = (0..RECOVERY_MAX_DEPTH)
            .map(|index| format!("f{index}"))
            .collect::<Vec<_>>()
            .join("/");
        assert!(
            PathTemplate::parse(&at_cap).is_ok(),
            "{RECOVERY_MAX_DEPTH} folders is still reachable, so it is legal: {at_cap}"
        );

        // One folder further and a crash there is unsalvageable: neither the
        // salvage pass nor the card scan descends that far, so the recording is
        // lost with no symptom at all. Refused where saying so is free.
        assert_eq!(
            PathTemplate::parse(&format!("{at_cap}/f{RECOVERY_MAX_DEPTH}")),
            Err(TemplateError::TooDeep(RECOVERY_MAX_DEPTH + 1))
        );
        // It is the FOLDER count that is capped — nothing about tokens or
        // length — so a template that looks entirely reasonable is refused the
        // same way.
        assert_eq!(
            PathTemplate::parse("{yyyy}/{mm}/{dd}/{HH}/a/b/c/d/{slug}"),
            Err(TemplateError::TooDeep(9))
        );
        // …and the count is a count of folders, so a doubled `/` is reported as
        // the empty folder it is rather than swelling the depth.
        assert_eq!(
            PathTemplate::parse(&format!("{at_cap}//x")),
            Err(TemplateError::EmptyComponent)
        );
    }

    #[test]
    fn the_depth_refusal_is_a_standalone_sentence_the_settings_card_can_print() {
        use crate::recording::RECOVERY_MAX_DEPTH;

        let message = TemplateError::TooDeep(RECOVERY_MAX_DEPTH + 1).to_string();
        assert_eq!(
            message,
            format!(
                "a template can be at most {RECOVERY_MAX_DEPTH} folders deep, and this one is {}: \
                 a recording nested any deeper is one keeper could not find again after a crash",
                RECOVERY_MAX_DEPTH + 1
            )
        );
        // Inline copy: one line, no heading, no capital opening a sentence that
        // is printed mid-card, nothing to trim, and both numbers present so the
        // reader knows how much to cut.
        assert!(!message.contains('\n'), "{message}");
        assert_eq!(message.trim(), message, "{message}");
        assert!(message.starts_with("a template"), "{message}");
        assert!(
            message.contains(&RECOVERY_MAX_DEPTH.to_string()),
            "{message}"
        );

        // And it survives the settings command's rejection unchanged: 40.2
        // wraps the reason in `TemplateInvalid`, whose own `Display` IS the
        // reason, so what the field prints is this sentence and nothing else.
        let reason = PathTemplate::parse("{yyyy}/{mm}/{dd}/{HH}/a/b/c/d/{slug}")
            .expect_err("nine folders is past the cap");
        let rejected = crate::error::RecordingError::TemplateInvalid {
            reason: reason.clone(),
        };
        assert_eq!(rejected.to_string(), reason.to_string());
        assert_eq!(rejected.to_string(), message);
    }

    #[test]
    fn parse_refuses_a_folder_render_would_have_rewritten() {
        // A doubled separator and a trailing one are empty folders, not
        // punctuation to be swallowed.
        assert_eq!(
            PathTemplate::parse("{yyyy}//{mm}"),
            Err(TemplateError::EmptyComponent)
        );
        // Padding does not make `..` a different folder.
        assert_eq!(
            PathTemplate::parse("{yyyy}/  ..  /{mm}"),
            Err(TemplateError::ParentComponent)
        );
        // A folder that is nothing but the characters a name may not end with
        // renders to nothing, so it is the empty folder it looks like.
        assert_eq!(
            PathTemplate::parse("{yyyy}/   /{mm}"),
            Err(TemplateError::EmptyComponent)
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/.../{mm}"),
            Err(TemplateError::EmptyComponent)
        );
        // …and one render would merely *trim* is refused rather than trimmed:
        // story 40.2 shows a live preview beside the raw field, and a preview
        // that disagrees with the field with no error explains nothing.
        assert_eq!(
            PathTemplate::parse("{yyyy}/..a"),
            Err(TemplateError::PaddedComponent("..a".to_owned()))
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/ x /{mm}"),
            Err(TemplateError::PaddedComponent(" x ".to_owned()))
        );
        assert!(TemplateError::PaddedComponent("..a".to_owned())
            .to_string()
            .contains("..a"));

        // A folder holding a token no longer buys its edges an exemption —
        // see `a_padded_edge_is_refused_even_when_the_folder_holds_a_token`.
        assert_eq!(
            PathTemplate::parse("  {yyyy}  /{mm}"),
            Err(TemplateError::PaddedComponent("  {yyyy}  ".to_owned()))
        );
        // What the collapse rule still governs is the folder's *interior*: the
        // space in front of `{slug}` leaves when `{slug}` does.
        assert_eq!(render(DEFAULT_TEMPLATE, None, 1), "2026/2026-08-05 1432");
    }

    #[test]
    fn a_format_character_is_illegal_in_a_template_and_filtered_from_a_title() {
        // A template whose whole name is invisible is not a folder anyone can
        // find, type or delete.
        assert_eq!(
            PathTemplate::parse("\u{feff}"),
            Err(TemplateError::IllegalCharacter { ch: '\u{feff}' })
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/\u{200b}{mm}"),
            Err(TemplateError::IllegalCharacter { ch: '\u{200b}' })
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/rec\u{202e}{mm}"),
            Err(TemplateError::IllegalCharacter { ch: '\u{202e}' })
        );

        // A title is data, so it is filtered instead — including the
        // right-to-left override Finder would draw as `annualweiver.jpg`.
        let spoof = render(
            "{yyyy}/{title} {HH}{MM}",
            Some("annual\u{202e}gpj.review"),
            1,
        );
        assert_eq!(spoof, "2026/annualgpj.review 1432");
        assert!(!spoof.contains('\u{202e}'));

        // Whitespace that is not whitespace, plus a zero-width space, is not a
        // title at all: it renders as the untitled recording it is, rather than
        // as a folder named after one invisible character.
        assert_eq!(
            render(DEFAULT_TEMPLATE, Some("\u{3000}\u{200b}\u{00a0}"), 1),
            "2026/2026-08-05 1432"
        );
        assert_eq!(
            render("{yyyy}/{title} {HH}{MM}", Some("a\u{00ad}b\u{2060}c"), 1),
            "2026/abc 1432"
        );
    }

    #[test]
    fn two_collapsing_tokens_do_not_weld_their_neighbours() {
        // Removal, then merge, then trim: two tokens leave, one separator
        // stays, and the day does not run into the hour.
        assert_eq!(render("{dd} {slug}{title} {HH}", None, 1), "05 14");
        // …and the same template with an extra space already answered this
        // correctly, so the two must now agree.
        assert_eq!(render("{dd} {slug} {title} {HH}", None, 1), "05 14");
        assert_eq!(
            render("{dd} {slug}{title} {HH}", Some("Standup"), 1),
            "05 standupStandup 14"
        );

        // Every answer the earlier rule got right is unchanged.
        assert_eq!(render(DEFAULT_TEMPLATE, None, 1), "2026/2026-08-05 1432");
        assert_eq!(render("{yyyy}/rec-{slug}", None, 1), "2026/rec");
        assert_eq!(render("{yyyy}/{slug} {HH}{MM}", None, 1), "2026/1432");
        assert_eq!(
            render("{yyyy}-{mm}-{dd} {slug}{seq}", None, 2),
            "2026-08-05 (2)"
        );
        assert_eq!(
            render("{yyyy}-{mm}-{dd} {slug}{seq}", Some("Standup"), 2),
            "2026-08-05 standup (2)"
        );
    }

    #[test]
    fn only_the_filler_a_collapse_exposed_is_trimmed() {
        // The author's underscores are not the collapse's to eat — only the one
        // the vanished `{slug}` was standing against. Before, the trim fired on
        // "something collapsed anywhere here", so this folder's own decoration
        // survived for titled recordings and not for untitled ones.
        assert_eq!(render("{yyyy}/_{HH}{MM}_{slug}", None, 1), "2026/_1432");
        assert_eq!(
            render("{yyyy}/_{HH}{MM}_{slug}", Some("Standup"), 1),
            "2026/_1432_standup"
        );

        // A folder that is filler with no collapsible token beside it renders
        // exactly what it says, so calling it optional was a false statement
        // about it — `{yyyy}/-/{mm}` had always rendered that `-` intact.
        assert_eq!(render("{yyyy}/-", None, 1), "2026/-");
        assert_eq!(render("{yyyy}/-/{mm}", None, 1), "2026/-/08");
        // …and where the `-` really can be eaten, the leaf is still refused.
        assert_eq!(
            PathTemplate::parse("{yyyy}/-{slug}"),
            Err(TemplateError::OptionalLeaf("-{slug}".to_owned()))
        );

        // Every earlier answer is unchanged, because in each of these the
        // filler *is* adjacent to the token that collapsed.
        assert_eq!(render(DEFAULT_TEMPLATE, None, 1), "2026/2026-08-05 1432");
        assert_eq!(render("{yyyy}/rec-{slug}", None, 1), "2026/rec");
        assert_eq!(render("{yyyy}/{slug} {HH}{MM}", None, 1), "2026/1432");
        assert_eq!(render("{dd} {slug}{title} {HH}", None, 1), "05 14");
        assert_eq!(render("{dd}-{slug} {HH}", None, 1), "05-14");
        assert_eq!(
            render("{yyyy}-{mm}-{dd} {slug}{seq}", None, 2),
            "2026-08-05 (2)"
        );
        assert_eq!(
            render("{yyyy}-{mm}-{dd} {slug}{seq}", Some("Standup"), 2),
            "2026-08-05 standup (2)"
        );
    }

    #[test]
    fn a_collapsing_token_takes_its_brackets_with_it() {
        // `2026/2026-08-05 ()` is a dangling separator and an "Untitled"
        // placeholder at once — the epic refuses both, and parenthesising the
        // optional part is one of the first templates anyone writes.
        let source = "{yyyy}/{yyyy}-{mm}-{dd} ({title})";
        assert_eq!(render(source, None, 1), "2026/2026-08-05");
        assert_eq!(
            render(source, Some("Standup"), 1),
            "2026/2026-08-05 (Standup)"
        );
        let source = "{yyyy}/{yyyy}-{mm}-{dd} [{slug}]";
        assert_eq!(render(source, None, 1), "2026/2026-08-05");
        assert_eq!(
            render(source, Some("Standup"), 1),
            "2026/2026-08-05 [standup]"
        );

        // Brackets are separators, so a folder built from nothing but brackets
        // and an optional token can render to nothing, exactly like any other
        // filler-only folder — and is refused in the leaf for that reason.
        assert_eq!(
            PathTemplate::parse("{yyyy}/({slug})"),
            Err(TemplateError::OptionalLeaf("({slug})".to_owned()))
        );
        assert_eq!(render("{yyyy}/({slug})/{mm}", None, 1), "2026/08");
        assert_eq!(
            render("{yyyy}/({slug})/{mm}", Some("Standup"), 1),
            "2026/(standup)/08"
        );

        // A bracket belongs to one side, so only the collapse it faces takes
        // it: here the `(` opens the second, which never collapses, and the
        // untitled recording keeps its parentheses instead of a stray `)`.
        assert_eq!(render("{yyyy}/{slug} ({SS})", None, 1), "2026/(07)");
        assert_eq!(
            render("{yyyy}/{slug} ({SS})", Some("Standup"), 1),
            "2026/standup (07)"
        );
        // …and between two things that both rendered, one separator survives,
        // which is never the bracket that lost its partner.
        assert_eq!(render("{dd} ({slug}) {HH}", None, 1), "05 14");
        assert_eq!(
            render("{dd} ({slug}) {HH}", Some("Standup"), 1),
            "05 (standup) 14"
        );

        // A bracket with no collapsible token beside it is text the user wrote,
        // by the same exposure rule that keeps `{yyyy}/-` a legal folder.
        assert_eq!(render("{yyyy}/({mm})", None, 1), "2026/(08)");
        assert_eq!(render("{yyyy}/[]", None, 1), "2026/[]");
    }

    #[test]
    fn brackets_pair_rather_than_face_a_direction() {
        // Brackets were filler for one loopback, and filler is directionless, so
        // each one had to be given a *facing* to know which neighbour it
        // belonged to. A facing rule cannot keep a pair together, and all four
        // measured consequences are below.

        // 1. The separator between two things that both rendered went with the
        //    bracket that faced the other way: `2026/2026-08-05(1432)`.
        let source = "{yyyy}/{yyyy}-{mm}-{dd} {slug} ({HH}{MM})";
        assert_eq!(render(source, None, 1), "2026/2026-08-05 (1432)");
        assert_eq!(
            render(source, Some("Standup"), 1),
            "2026/2026-08-05 standup (1432)"
        );

        // 2. …and with no separator to lose, the neighbours welded outright:
        //    `2026/0514`, verbatim the defect the collapse rule already fixed
        //    once. The pair leaves as a pair, and a space stands in for it,
        //    because half a bracket could not come back.
        assert_eq!(render("{yyyy}/{dd}({slug}){HH}", None, 1), "2026/05 14");
        assert_eq!(
            render("{yyyy}/{dd}({slug}){HH}", Some("Standup"), 1),
            "2026/05(standup)14"
        );
        assert_eq!(render("{dd} ({slug}) {HH}", None, 1), "05 14");

        // 3. Two names that no bracket rule may produce: `2026/07)` and
        //    `2026/(07`. Nothing pairs in either — in the first the `(` encloses
        //    the second as well, in the second the pair encloses nothing — so
        //    both are rendered as the text they are.
        assert_eq!(render("{yyyy}/({slug} {SS})", None, 1), "2026/(07)");
        assert_eq!(
            render("{yyyy}/({slug} {SS})", Some("Standup"), 1),
            "2026/(standup 07)"
        );
        assert_eq!(render("{yyyy}/{slug}(){SS}", None, 1), "2026/()07");

        // 4. A lone bracket is text the user typed, and stays text: it is not a
        //    separator, so no collapse can take it and leave the other half.
        assert_eq!(render("{yyyy}/{slug}(", None, 1), "2026/(");
        assert_eq!(
            render("{yyyy}/{slug}(", Some("Standup"), 1),
            "2026/standup("
        );

        // A pair the user wrote around something that always renders is text
        // they asked for, and an empty pair they typed themselves is too.
        assert_eq!(render("{yyyy}/(x)", None, 1), "2026/(x)");
        assert_eq!(render("{yyyy}/()", None, 1), "2026/()");
        assert_eq!(render("{yyyy}/({mm})", None, 1), "2026/(08)");

        // …while a pair around a token that can collapse leaves with it, so
        // there is nothing left and the leaf rule says so.
        assert_eq!(
            PathTemplate::parse("{yyyy}/({slug})"),
            Err(TemplateError::OptionalLeaf("({slug})".to_owned()))
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/[{title}]"),
            Err(TemplateError::OptionalLeaf("[{title}]".to_owned()))
        );

        // Only a *matching* pair goes: `[` does not close a `(`. The first of
        // these used to render `2026/08()05`, one pair short — the search ran
        // once per gap, so the `(`…`)` that became the nearest neighbours after
        // `[`…`]` left was never looked at again. What goes here is `[` with
        // `]` and then `(` with `)`, never `[` with `)`, which is what the
        // second line says and still says.
        assert_eq!(render("{yyyy}/{mm}([{slug}]){dd}", None, 1), "2026/08 05");
        assert_eq!(render("{yyyy}/{mm}[{slug}){dd}", None, 1), "2026/08[)05");

        // And whatever the pair was hiding comes back as the separator it is,
        // rather than staying welded to the text on either side of it.
        assert_eq!(render("{yyyy}/x ({slug}) y", None, 1), "2026/x y");
        assert_eq!(render("{yyyy}/x-({slug})-y", None, 1), "2026/x-y");
        assert_eq!(
            render("{yyyy}/x ({slug}) y", Some("Standup"), 1),
            "2026/x (standup) y"
        );
    }

    #[test]
    fn a_nested_pair_leaves_whole_or_not_at_all() {
        // The search ran once per collapsing token, so it could claim at most
        // one pair — and the pair that becomes the nearest neighbour *after* the
        // inner one has left was never looked at again. Every shape below kept
        // an empty pair the epic refuses.
        assert_eq!(
            render("{yyyy}/{mm}-{dd} (({title}))", None, 1),
            "2026/08-05"
        );
        assert_eq!(
            render("{yyyy}/{mm}-{dd} (({title}))", Some("Standup"), 1),
            "2026/08-05 ((Standup))"
        );
        assert_eq!(render("{yyyy}/{mm} ([{slug}])", None, 1), "2026/08");
        assert_eq!(render("{yyyy}/{mm} [({slug})]", None, 1), "2026/08");
        assert_eq!(render("{yyyy}/{mm}(((({slug}))))", None, 1), "2026/08");

        // …and the worst of them: a leaf of nothing but nested brackets around
        // an optional token *parsed*, and then named every untitled session
        // `()`, `() (2)`, `() (3)` — the "Untitled" placeholder spelled in
        // punctuation, arrived at through the rule written to forbid it. The
        // leaf rule can see it now, because the render it asks agrees.
        for leaf in ["(({slug}))", "((({title})))", "[({slug})]", "([{title}])"] {
            assert!(
                matches!(
                    PathTemplate::parse(&format!("{{yyyy}}/{leaf}")),
                    Err(TemplateError::OptionalLeaf(_))
                ),
                "{leaf:?} still parses as a leaf"
            );
            // An interior slot may still collapse, and takes every pair with it.
            assert_eq!(
                render(&format!("{{yyyy}}/{leaf}/{{mm}}"), None, 1),
                "2026/08"
            );
        }

        // A pair that does not enclose the token that left is text, however
        // deeply it nests: only the ones the collapse actually exposed go.
        assert_eq!(render("{yyyy}/(({mm}))", None, 1), "2026/((08))");
        assert_eq!(render("{yyyy}/((x{slug}))", None, 1), "2026/((x))");
        assert_eq!(render("{yyyy}/(({slug} {SS}))", None, 1), "2026/((07))");
        // …and an unmatched one still cannot pair, at any depth.
        assert_eq!(render("{yyyy}/((({slug}", None, 1), "2026/(((");
    }

    #[test]
    fn a_bracket_the_title_brought_is_never_half_of_a_template_pair() {
        // A `Unit::Text` built from a literal and one built from a token looked
        // alike, so a `(` that arrived *inside a title* could be claimed as the
        // opening half of a pair — deleting a character of the user's data and a
        // character of their template together. A title is data, and
        // `sanitize_title` is the only filter it is documented to pass.
        assert_eq!(
            render("{yyyy}/{title}{slug}){mm}", Some("("), 1),
            "2026/()08"
        );
        assert_eq!(
            render("{yyyy}/{mm} {title}{slug}) {HH}", Some("(("), 1),
            "2026/08 (() 14"
        );
        assert_eq!(
            render("{yyyy}/{mm} ({title}{slug}", Some(")"), 1),
            "2026/08 ()"
        );

        // The template's own pair still leaves whole when the title is what
        // collapsed — a title that renders *nothing* took no bracket with it.
        assert_eq!(render("{yyyy}/{mm} ({title})", None, 1), "2026/08");
        assert_eq!(render("{yyyy}/{mm} ({title})", Some("("), 1), "2026/08 (()");
    }

    #[test]
    fn the_byte_clamp_never_leaves_a_bracket_without_its_partner() {
        // The clamp keeps a **prefix**, and nothing on that path asked about
        // brackets: a leaf that opened with `(` and closed 400 characters later
        // rendered 251 bytes of opening bracket and no closing one. The collapse
        // was not the only place a pair could be broken in half.
        for (source, opens) in [
            (format!("{{yyyy}}/({}{{slug}})", "x".repeat(400)), '('),
            (format!("{{yyyy}}/[{}{{title}}]", "x".repeat(400)), '['),
        ] {
            let parsed = template(&source);
            for title in [None, Some("Standup")] {
                let rendered = parsed.render(&ctx(title, 3));
                let leaf = rendered.components().last().unwrap_or_default();
                assert!(leaf.len() <= NAME_MAX_BYTES, "{} bytes", leaf.len());
                assert!(
                    !leaf.starts_with(opens),
                    "{source:?} kept a {opens:?} the cut widowed: {leaf:?}"
                );
                assert_legal(&rendered, &parsed, title);
            }
        }

        // A bracket the *user* left unmatched is their text and stays their
        // text, clamped or not: preservation, never a repair.
        let source = format!("{{yyyy}}/({}{{slug}}", "x".repeat(400));
        let parsed = template(&source);
        let rendered = parsed.render(&ctx(Some("Standup"), 1));
        let leaf = rendered.components().last().unwrap_or_default();
        assert!(
            leaf.starts_with('('),
            "the user's own bracket went: {leaf:?}"
        );
        assert!(leaf.len() <= NAME_MAX_BYTES);
    }

    #[test]
    fn a_folders_brackets_do_not_appear_only_on_collision() {
        // A rendered ordinal is not a bracket the user wrote around anything, so
        // it may not pair — but *abandoning* the search when one turned up left
        // the pair standing, and a folder's punctuation then depended on whether
        // the recording had collided: `08` at seq 1, `08( (2))` at seq 2.
        let source = "{yyyy}/{mm}({slug}{seq})";
        assert_eq!(render(source, None, 1), "2026/08");
        assert_eq!(render(source, None, 2), "2026/08 (2)");
        assert_eq!(render(source, None, 7), "2026/08 (7)");
        assert_eq!(render(source, Some("Standup"), 2), "2026/08(standup (2))");

        let source = "{yyyy}/{mm}({title}{seq}){dd}";
        assert_eq!(render(source, None, 1), "2026/08 05");
        assert_eq!(render(source, None, 2), "2026/08 (2)05");

        // The ordinal's own `(` and `)` are still never usable as a pair: it
        // stands aside from the search rather than joining it, so the pair that
        // leaves here is the one the template wrote around `{slug}`, and the
        // ordinal keeps the `(2)` it rendered.
        let source = "{yyyy}/{mm}{seq}({slug}){dd}";
        assert_eq!(render(source, None, 1), "2026/08 05");
        assert_eq!(render(source, None, 2), "2026/08 (2) 05");
    }

    #[test]
    fn every_rendered_component_has_balanced_brackets() {
        // `assert_legal` checked legality and never legibility, which is how
        // `(1432` and `07)` passed 14 000 assertions. A bracket is removed only
        // with its partner, so a template whose brackets balance renders names
        // whose brackets balance — titled, untitled, and at a collision.
        let sources = [
            "{yyyy}/{yyyy}-{mm}-{dd} ({title})",
            "{yyyy}/{yyyy}-{mm}-{dd} [{slug}]",
            "{yyyy}/{yyyy}-{mm}-{dd} {slug} ({HH}{MM})",
            "{yyyy}/{slug} ({SS})",
            "{yyyy}/({slug} {SS})",
            "{yyyy}/({SS} {slug})",
            "{yyyy}/{dd}({slug}){HH}",
            "{yyyy}/({slug})/{mm}-{dd} [{title}] {HH}",
            "{yyyy}/[{slug}] ({title}) {HH}{MM}",
            "{yyyy}/({mm})-({dd}){seq}",
            "{yyyy}/x{slug}y({title})z",
            // Nested and mixed, which none of the eleven above were — and the
            // shapes where claiming one pair exposes another. `(({title}))`
            // untitled left `()` behind for one loopback, and `([{slug}])`
            // left `()` too.
            "{yyyy}/{mm}-{dd} (({title}))",
            "{yyyy}/{mm}-{dd} ([{slug}])",
            "{yyyy}/{mm}([{slug}]){dd}",
            "{yyyy}/({mm} [{slug}] {dd})",
            "{yyyy}/[({title})] {HH}{MM}",
        ];
        for source in sources {
            let parsed = template(source);
            for title in [None, Some("Standup"), Some("!!!"), Some("...")] {
                for seq in [1, 2, 9] {
                    // `assert_legal` carries the bracket property now, so this
                    // is the whole assertion.
                    assert_legal(&parsed.render(&ctx(title, seq)), &parsed, title);
                }
            }
        }
    }

    #[test]
    fn a_title_of_nothing_but_edge_noise_collapses_like_an_empty_one() {
        // `"..."` survives the title filter verbatim — dots are neither
        // whitespace nor illegal — so no collapse fired, and the folder's edge
        // trim then removed the dots anyway and stranded the separator they were
        // written against: `2026-08-05_`.
        let source = "{yyyy}/{yyyy}-{mm}-{dd}_{title}";
        assert_eq!(render(source, Some("..."), 1), "2026/2026-08-05");
        assert_eq!(render(source, None, 1), "2026/2026-08-05");
        assert_eq!(render(source, Some(" . . "), 1), "2026/2026-08-05");
        // A title that contributes something keeps its separator, and the
        // folder's own edge trim still runs on what it contributed.
        assert_eq!(
            render(source, Some("Standup"), 1),
            "2026/2026-08-05_Standup"
        );
        assert_eq!(render(source, Some("...a"), 1), "2026/2026-08-05_...a");

        // An interior folder that is nothing but such a title vanishes with its
        // separator, like the untitled recording it is.
        assert_eq!(render("{yyyy}/{title}/{mm}", Some("..."), 1), "2026/08");

        // This is where `"!!!"` already was for `{slug}`: a title that folds to
        // nothing has always been an untitled recording.
        assert_eq!(
            render(DEFAULT_TEMPLATE, Some("..."), 1),
            "2026/2026-08-05 1432"
        );
    }

    #[test]
    fn a_padded_edge_is_refused_even_when_the_folder_holds_a_token() {
        // The rule is about the literals the user typed, and a literal does not
        // stop being typed because a token shares its folder: scoped to
        // token-free folders, `{yyyy}/  ..  /{mm}` was a typed error while
        // `{yyyy}/..{mm}` deleted the same two characters without a word.
        assert_eq!(
            PathTemplate::parse("{yyyy}/..{mm}"),
            Err(TemplateError::PaddedComponent("..{mm}".to_owned()))
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/. {mm}"),
            Err(TemplateError::PaddedComponent(". {mm}".to_owned()))
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/{mm} "),
            Err(TemplateError::PaddedComponent("{mm} ".to_owned()))
        );
        assert_eq!(
            PathTemplate::parse("  {yyyy}  "),
            Err(TemplateError::PaddedComponent("  {yyyy}  ".to_owned()))
        );

        // Interior literals are untouched — they are the separators the
        // collapse rule governs, so nothing that renders a folder loses one.
        assert!(PathTemplate::parse(DEFAULT_TEMPLATE).is_ok());
        for source in sweep_templates() {
            assert!(
                PathTemplate::parse(&source).is_ok(),
                "{source:?} no longer parses"
            );
        }
        assert_eq!(
            render("{yyyy}/x-{slug}-y", Some("Standup"), 1),
            "2026/x-standup-y"
        );
        assert_eq!(render("{yyyy}/x-{slug}-y", None, 1), "2026/x-y");
    }

    #[test]
    fn the_byte_cap_never_erases_an_explicit_seq() {
        // A leaf whose literal alone overruns `NAME_MAX` has to be clamped, and
        // a clamp returns a *prefix* — which is exactly where an explicit
        // `{seq}` rendered its ordinal, while `has_seq` suppressed the one
        // `render` would otherwise have appended. Measured before this fix: seq
        // 1, 2 and 7 produced one byte-identical 251-byte leaf, so 40.3's
        // `for seq in 1..` loop would ask forever for a folder that exists.
        // The `{slug}` is what keeps this folder at the *render*-time clamp: a
        // folder whose name owes nothing to the title is measured at parse and
        // refused there instead, ordinal and all.
        let source = format!("{{yyyy}}/x{}{{slug}}{{seq}}", "y".repeat(400));
        let parsed = template(&source);

        for title in [None, Some("Standup")] {
            let mut leaves: Vec<String> = Vec::new();
            for seq in [1, 2, 7] {
                let rendered = parsed.render(&ctx(title, seq));
                assert_legal(&rendered, &parsed, title);

                let leaf = rendered.components().last().unwrap_or_default().to_owned();
                assert!(
                    leaf.len() <= NAME_MAX_BYTES,
                    "leaf is {} bytes: {leaf:?}",
                    leaf.len()
                );
                if seq > 1 {
                    assert!(
                        leaf.ends_with(&format!("({seq})")),
                        "the clamp cut the ordinal off at seq {seq}: {leaf:?}"
                    );
                }
                leaves.push(leaf);
            }
            assert_ne!(leaves[0], leaves[1], "seq 1 and seq 2 name one folder");
            assert_ne!(leaves[1], leaves[2], "seq 2 and seq 7 name one folder");
            assert_ne!(leaves[0], leaves[2], "seq 1 and seq 7 name one folder");
        }
    }

    #[test]
    fn every_component_fits_a_directory_entry_not_only_the_leaf() {
        // 80 emoji are 320 bytes, and here they land in the *first* folder,
        // which 40.3's retry loop never varies.
        let title = "🎉".repeat(120);
        let source = "{title}/{yyyy}-{mm}-{dd}";
        let parsed = template(source);
        let rendered = parsed.render(&ctx(Some(&title), 1));
        let first = rendered.components().next().unwrap_or_default();

        assert!(
            first.len() <= NAME_MAX_BYTES,
            "first component is {} bytes: {first:?}",
            first.len()
        );
        assert!(first.starts_with('🎉'), "the title was lost: {first:?}");
        assert_legal(&rendered, &parsed, Some(&title));

        // An interior folder whose *literals* alone overrun is clamped for the
        // same reason the leaf is: `render` is infallible and `mkdir` is not.
        // It has to hold a token to get that far — a folder of text alone is
        // refused at parse instead, see
        // `a_folder_of_text_alone_that_is_too_long_is_refused_not_clamped`.
        let source = format!("x{}{{slug}}/{{yyyy}}", "y".repeat(400));
        let parsed = template(&source);
        let rendered = parsed.render(&ctx(Some("Standup"), 2));
        assert_legal(&rendered, &parsed, Some("Standup"));
        assert_eq!(rendered.components().count(), 2);
    }

    #[test]
    fn a_collision_never_changes_the_shape_of_the_path() {
        // The property that would have caught a `{seq}` above the leaf: a
        // collision renames the recording's own folder and touches nothing
        // else — same depth, same parents, one different name.
        for source in sweep_templates() {
            let parsed = template(&source);
            for title in [None, Some("Standup"), Some("!!!")] {
                let first = parsed.render(&ctx(title, 1));
                let second = parsed.render(&ctx(title, 2));

                let first: Vec<&str> = first.components().collect();
                let second: Vec<&str> = second.components().collect();
                assert_eq!(
                    first.len(),
                    second.len(),
                    "{source:?} with {title:?} changes depth on collision: {first:?} vs {second:?}"
                );
                assert_eq!(
                    first[..first.len() - 1],
                    second[..second.len() - 1],
                    "{source:?} with {title:?} renames a folder above the recording"
                );
                assert_ne!(
                    first.last(),
                    second.last(),
                    "{source:?} with {title:?} reuses one folder for two recordings"
                );
            }
        }
    }

    #[test]
    fn a_long_title_is_cut_back_so_the_ordinal_still_fits() {
        // 80 Japanese characters are 240 bytes: with the date prefix the leaf
        // is 256 bytes before the ordinal is anywhere near it.
        let title = "議".repeat(TITLE_MAX_CHARS);
        let source = "{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {title}";
        let rendered = template(source).render(&ctx(Some(&title), 9));
        let leaf = rendered.components().last().unwrap_or_default();

        assert!(
            leaf.len() <= NAME_MAX_BYTES,
            "leaf is {} bytes: {leaf:?}",
            leaf.len()
        );
        assert!(leaf.ends_with(" (9)"), "ordinal was dropped: {leaf:?}");

        // Cut on a character boundary, never mid-codepoint: what is left of the
        // title is whole `議`s and nothing else.
        let kept = leaf
            .trim_start_matches("2026-08-05 1432 ")
            .trim_end_matches(" (9)");
        assert_eq!(kept, "議".repeat(kept.chars().count()));
        assert!(kept.chars().count() < TITLE_MAX_CHARS, "nothing was cut");
        // …and no more than it had to: one more character would not have fit.
        assert!(
            leaf.len() + "議".len() > NAME_MAX_BYTES,
            "cut too far: {leaf:?}"
        );
    }

    #[test]
    fn a_folder_of_text_alone_that_is_too_long_is_refused_not_clamped() {
        // The folder holds no token, so its rendered name is the text that was
        // typed and its length is knowable here. Clamped instead, it deleted
        // fifty typed characters in silence — a rewrite of the user's
        // specification, beside a 40.2 preview that would then disagree with the
        // field above it and explain nothing.
        let source = format!("{{yyyy}}/x{}", "y".repeat(300));
        // The folder is quoted back only as far as it identifies itself. This
        // error is raised *because* its subject is too long, so quoting the
        // subject whole made the sentence as unreadable as the fault — and 40.2
        // prints it inline, beside the field.
        let quoted = format!("x{}…", "y".repeat(QUOTED_MAX_CHARS - 1));
        assert_eq!(
            PathTemplate::parse(&source),
            Err(TemplateError::OverlongComponent(quoted.clone()))
        );
        assert!(quoted.chars().count() <= QUOTED_MAX_CHARS + 1);
        // …in any position, not only the leaf: every folder is one `mkdir`.
        assert_eq!(
            PathTemplate::parse(&format!("x{}/{{yyyy}}", "y".repeat(300))),
            Err(TemplateError::OverlongComponent(quoted.clone()))
        );
        // The error quotes the folder, so the settings field can point at one
        // out of several.
        assert!(TemplateError::OverlongComponent(quoted)
            .to_string()
            .contains("yyy"));
        // A folder short enough to read is still quoted whole.
        assert_eq!(
            PathTemplate::parse("{yyyy}/nul"),
            Err(TemplateError::ReservedComponent("nul".to_owned()))
        );

        // The boundary is `NAME_MAX` for an interior folder, which gains
        // nothing, and it is measured in bytes, not characters: 128 two-byte
        // characters are 256 bytes.
        assert!(PathTemplate::parse(&format!("{}/{{yyyy}}", "y".repeat(NAME_MAX_BYTES))).is_ok());
        assert_eq!(
            PathTemplate::parse(&format!("{}/{{yyyy}}", "y".repeat(NAME_MAX_BYTES + 1))),
            Err(TemplateError::OverlongComponent(format!(
                "{}…",
                "y".repeat(QUOTED_MAX_CHARS)
            )))
        );
        assert!(matches!(
            PathTemplate::parse(&format!("{{yyyy}}/{}", "ä".repeat(128))),
            Err(TemplateError::OverlongComponent(_))
        ));

        // A folder holding a *token* keeps the render-time clamp: there the
        // length depends on a title that arrives with the recording, and
        // `render` is infallible.
        let source = format!("{{yyyy}}/x{}{{slug}}", "y".repeat(400));
        let parsed = template(&source);
        let rendered = parsed.render(&ctx(Some("Standup"), 4));
        let leaf = rendered.components().last().unwrap_or_default();
        assert!(leaf.len() <= NAME_MAX_BYTES, "leaf is {} bytes", leaf.len());
        assert!(leaf.ends_with(" (4)"), "ordinal was dropped: {leaf:?}");
        assert_legal(&rendered, &parsed, Some("Standup"));
    }

    #[test]
    fn a_text_only_leaf_keeps_room_for_the_collision_ordinal() {
        // "Decided entirely at parse" broke on the first collision: `parse`
        // capped a text-only folder at 255 bytes while `render` gave the leaf
        // `255 - ordinal` and clamped to fit. Measured, a 255-byte typed leaf
        // rendered as typed at seq 1 and lost eight characters at seq 2 — and
        // 40.2's preview renders seq 1, so it could not have shown it.
        let budget = NAME_MAX_BYTES - seq_suffix(u32::MAX).len();
        let elided = format!("{}…", "y".repeat(QUOTED_MAX_CHARS));
        assert!(PathTemplate::parse(&format!("{{yyyy}}/{}", "y".repeat(budget))).is_ok());
        assert_eq!(
            PathTemplate::parse(&format!("{{yyyy}}/{}", "y".repeat(budget + 1))),
            Err(TemplateError::OverlongComponent(elided.clone()))
        );

        // The reservation is the *widest* ordinal, not the one in hand: `parse`
        // does not know which seq 40.3's retry loop will reach, so the only
        // figure that holds for all of them is the largest.
        let parsed = template(&format!("{{yyyy}}/{}", "y".repeat(budget)));
        for seq in [1, 2, 9, u32::MAX] {
            let rendered = parsed.render(&ctx(None, seq));
            let leaf = rendered.components().last().unwrap_or_default();
            assert!(
                leaf.len() <= NAME_MAX_BYTES,
                "leaf is {} bytes at seq {seq}",
                leaf.len()
            );
            assert!(
                leaf.starts_with(&"y".repeat(budget)),
                "the ordinal cost the user typed characters at seq {seq}"
            );
        }

        // An interior folder gains no ordinal — 40.3 only ever varies the last
        // one — so it keeps the whole 255.
        assert!(PathTemplate::parse(&format!("{}/{{yyyy}}", "y".repeat(NAME_MAX_BYTES))).is_ok());
        assert_eq!(
            PathTemplate::parse(&format!("{}/{{yyyy}}", "y".repeat(NAME_MAX_BYTES + 1))),
            Err(TemplateError::OverlongComponent(elided))
        );
    }

    #[test]
    fn a_leaf_that_writes_its_own_seq_is_measured_at_the_widest_ordinal() {
        // The budget was a *subtraction* on the seq-1 render, and the seq-1
        // render of a `{seq}`-bearing leaf is not that leaf minus the ordinal:
        // at seq 1 the token is a gap, so it takes the separator run beside it
        // as well, and at a collision that run comes back *with* the ordinal.
        // A fixed 13-byte reservation covers the ordinal and not the filler.
        //
        // Measured before this fix: this template read as 242 bytes, parsed, and
        // then needed 256 at seq 2 — so the clamp fired and deleted the ten
        // dashes and four `y`s the user had typed, which is exactly the rewrite
        // the reservation exists to refuse.
        let typed = format!("{}{}", "y".repeat(242), "-".repeat(10));
        assert!(matches!(
            PathTemplate::parse(&format!("{{yyyy}}/{typed}{{seq}}")),
            Err(TemplateError::OverlongComponent(_))
        ));

        // The boundary, exactly: an in-place ordinal renders `<text> (4294967295)`,
        // so 242 bytes of text is 255 and one more byte is one too many.
        let widest = format!(" ({})", u32::MAX);
        let fits = NAME_MAX_BYTES - widest.len();
        assert!(PathTemplate::parse(&format!("{{yyyy}}/{}{{seq}}", "y".repeat(fits))).is_ok());
        assert!(matches!(
            PathTemplate::parse(&format!("{{yyyy}}/{}{{seq}}", "y".repeat(fits + 1))),
            Err(TemplateError::OverlongComponent(_))
        ));

        // …and what parsed renders unshortened at every seq, ordinal and all.
        let parsed = template(&format!("{{yyyy}}/{}{{seq}}", "y".repeat(fits)));
        for seq in [1, 2, 7, u32::MAX] {
            let rendered = parsed.render(&ctx(None, seq));
            let leaf = rendered.components().last().unwrap_or_default();
            assert!(
                leaf.starts_with(&"y".repeat(fits)),
                "the ordinal cost the user typed characters at seq {seq}: {leaf:?}"
            );
            assert!(leaf.len() <= NAME_MAX_BYTES, "at seq {seq}: {}", leaf.len());
            if seq > 1 {
                assert!(leaf.ends_with(&format!(" ({seq})")), "at seq {seq}");
            }
        }

        // An *interior* folder is still measured at seq 1 against the whole 255:
        // no ordinal ever reaches it, so reserving room for one would refuse a
        // folder that renders perfectly well. (`{seq}` is refused above the leaf,
        // so the shape is a folder with a date token in it.)
        assert!(PathTemplate::parse(&format!("{}{{mm}}/{{yyyy}}", "y".repeat(253))).is_ok());
        assert!(matches!(
            PathTemplate::parse(&format!("{}{{mm}}/{{yyyy}}", "y".repeat(254))),
            Err(TemplateError::OverlongComponent(_))
        ));
    }

    #[test]
    fn a_text_only_leaf_renders_what_was_typed_plus_the_ordinal() {
        // The other half of the same rule, stated as the property it is: a leaf
        // the user spelled out is the folder they get, at every seq. Nothing
        // shortens it, because `parse` refused any that could not survive the
        // widest ordinal.
        let budget = NAME_MAX_BYTES - seq_suffix(u32::MAX).len();
        for typed in ["rec", "a b c", "x.y", &"y".repeat(budget), &"ä".repeat(120)] {
            let parsed = template(&format!("{{yyyy}}/{typed}"));
            for seq in [1, 2, 7, 1_000, u32::MAX] {
                let rendered = parsed.render(&ctx(None, seq));
                let leaf = rendered.components().last().unwrap_or_default();
                let expected = if seq > 1 {
                    format!("{typed}{}", seq_suffix(seq))
                } else {
                    typed.to_owned()
                };
                assert_eq!(leaf, expected, "at seq {seq}");
                assert!(leaf.len() <= NAME_MAX_BYTES, "at seq {seq}");
            }
            // …and a title changes nothing about it, since nothing in it asked.
            assert_eq!(
                render(&format!("{{yyyy}}/{typed}"), Some("Standup"), 1),
                format!("2026/{typed}")
            );
        }
    }

    #[test]
    fn a_seq_bearing_folder_is_decided_at_parse_like_a_text_only_one() {
        // `{seq}` is not a title. A folder whose only token is one renders text
        // that is still knowable here, so reading the rule as "holds no token"
        // left a hole straight through both parse checks: the same folder was
        // refused without `{seq}` and silently rewritten with it.
        let overlong = format!("x{}", "y".repeat(300));
        assert_eq!(
            PathTemplate::parse(&format!("{{yyyy}}/{overlong}{{seq}}")),
            // Quoted as far as it identifies the folder — see
            // `a_folder_of_text_alone_that_is_too_long_is_refused_not_clamped`.
            Err(TemplateError::OverlongComponent(format!(
                "x{}…",
                "y".repeat(QUOTED_MAX_CHARS - 1)
            )))
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/nul{seq}"),
            Err(TemplateError::ReservedComponent("nul{seq}".to_owned()))
        );
        // …which is exactly how each is refused without it.
        assert!(matches!(
            PathTemplate::parse(&format!("{{yyyy}}/{overlong}")),
            Err(TemplateError::OverlongComponent(_))
        ));
        assert_eq!(
            PathTemplate::parse("{yyyy}/nul"),
            Err(TemplateError::ReservedComponent("nul".to_owned()))
        );

        // A date or time token is not a title either: it renders a fixed run of
        // digits at every instant, so the same two questions are answerable.
        assert!(matches!(
            PathTemplate::parse(&format!("{{yyyy}}/{overlong}{{mm}}")),
            Err(TemplateError::OverlongComponent(_))
        ));
        assert_eq!(
            PathTemplate::parse("{yyyy}/nul.{mm}"),
            Err(TemplateError::ReservedComponent("nul.{mm}".to_owned()))
        );

        // A `{title}` or a `{slug}` is where the line is drawn, and there the
        // render-time clamp and `de_reserve` still act — on data that arrives
        // with the recording, which this module filters rather than refuses.
        assert!(PathTemplate::parse(&format!("{{yyyy}}/{overlong}{{slug}}")).is_ok());
        assert_eq!(
            render("{yyyy}/nu{slug}/{mm}", Some("L"), 1),
            "2026/nul-rec/08"
        );
        assert_eq!(render("{yyyy}/nu{slug}/{mm}", None, 1), "2026/nu/08");

        // Nothing about the seq-1 form is a claim about seq 2: `nul{seq}` reads
        // `nul (2)` there, and it is still refused, because the folder the first
        // recording of the minute gets is the one named after the device.
        assert!(PathTemplate::parse("{yyyy}/{mm}-{dd}{seq}").is_ok());
        assert_eq!(render("{yyyy}/{mm}-{dd}{seq}", None, 2), "2026/08-05 (2)");
    }

    #[test]
    fn a_folder_of_text_alone_that_names_a_device_is_refused_not_escaped() {
        // `2026/nul-rec` is not the folder the user asked for, and nothing said
        // so. The fold is as decidable here as `..a` is.
        assert_eq!(
            PathTemplate::parse("{yyyy}/nul"),
            Err(TemplateError::ReservedComponent("nul".to_owned()))
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/CON/{mm}"),
            Err(TemplateError::ReservedComponent("CON".to_owned()))
        );
        // Reserved with an extension too — Windows refuses `aux.notes` exactly
        // as it refuses `aux`.
        assert_eq!(
            PathTemplate::parse("{yyyy}/aux.notes/{mm}"),
            Err(TemplateError::ReservedComponent("aux.notes".to_owned()))
        );
        assert_eq!(
            PathTemplate::parse("lpt9/{yyyy}"),
            Err(TemplateError::ReservedComponent("lpt9".to_owned()))
        );
        assert!(TemplateError::ReservedComponent("nul".to_owned())
            .to_string()
            .contains("nul"));

        // A name that merely starts with one is a name.
        assert_eq!(render("{yyyy}/console", None, 1), "2026/console");
        assert_eq!(render("{yyyy}/nul2", None, 1), "2026/nul2");

        // …and a *title* that folds onto a device name is still escaped, not
        // refused: it is data, arriving from a bridge or a paste, and refusing
        // to record because of it would be absurd. This is the matrix row.
        assert_eq!(render("{slug}/{yyyy}", Some("NUL"), 1), "nul-rec/2026");
        assert_eq!(render("{title}/{yyyy}", Some("con"), 1), "con-rec/2026");
    }

    #[test]
    fn parse_keeps_the_template_verbatim() {
        let parsed = template(DEFAULT_TEMPLATE);
        assert_eq!(parsed.as_str(), DEFAULT_TEMPLATE);
        assert_eq!(template("rec-{slug}/{yyyy}").as_str(), "rec-{slug}/{yyyy}");
        // `  {yyyy}  ` used to be kept verbatim and rendered as `2026`, quietly
        // dropping the padding. It is refused now: padding cannot be rewritten
        // in one folder shape and rejected in another and still be called
        // "validated, never sanitised".
        assert_eq!(
            PathTemplate::parse("  {yyyy}  "),
            Err(TemplateError::PaddedComponent("  {yyyy}  ".to_owned()))
        );
    }

    #[test]
    fn a_relative_path_displays_and_splits_into_its_components() {
        let rendered = template(DEFAULT_TEMPLATE).render(&ctx(Some("Standup"), 1));
        assert_eq!(rendered.to_string(), rendered.as_str());
        assert_eq!(
            rendered.components().collect::<Vec<_>>(),
            vec!["2026", "2026-08-05 1432 standup"]
        );
    }

    #[test]
    fn every_token_renders_what_the_table_says() {
        assert_eq!(
            render(
                "{yyyy} {yy} {mm} {dd} {HH} {MM} {SS} {title} {slug}",
                Some("T"),
                1
            ),
            "2026 26 08 05 14 32 07 T t"
        );
    }

    // --- the property sweep -------------------------------------------------

    /// Every template the property sweep renders. Each bears tokens, and each
    /// parses — the sweep is about rendering, not validation.
    const SWEEP_TEMPLATES: &[&str] = &[
        DEFAULT_TEMPLATE,
        "{yyyy}/{title} {HH}{MM}",
        "{yyyy}/{slug}/{HH}{MM}",
        "{yy}{mm}{dd} {title} {slug}{seq}",
        "rec/{yyyy}/{mm}/{dd}/{HH}{MM}{SS} {title}",
        "{yyyy}-{mm}-{dd}_{slug}",
        "{yyyy}/{slug} ({SS})",
        // Brackets around the optional part: the collapse has to take them with
        // it, and the sweep is where a title that renders to *nearly* nothing
        // would leave one of them behind.
        "{yyyy}/{yyyy}-{mm}-{dd} ({title})",
        // …and the shape no sweep template had: rendered text, a collapsible
        // token, then rendered text *inside brackets*. Nothing here could see
        // the day run into the hour or the separator disappear, because nothing
        // asked a bracket to stand beside something that survives.
        "{yyyy}/{yyyy}-{mm}-{dd} {slug} ({HH}{MM})",
        // A `{title}` in an *interior* folder: the shape where a hostile or
        // enormous title lands somewhere the collision retry never touches.
        "{title}/{yyyy}-{mm}-{dd}",
        "{yyyy}/{title}/{mm}-{dd} {HH}{MM}",
        // …and `{seq}` where it is now the only place it may be written, next
        // to two tokens that can both collapse around it.
        "{yyyy}/{mm}-{dd} {slug}{title}{seq}",
    ];

    /// [`SWEEP_TEMPLATES`], plus the two shapes a `const` cannot hold: a leaf
    /// whose *literal* alone overruns [`NAME_MAX_BYTES`], with and without a
    /// `{seq}` of its own.
    ///
    /// Those are the only shapes that force the clamp, and the clamp is where an
    /// ordinal was measured to disappear. Without them the sweep rendered
    /// nothing long enough to notice, and every seq of a clamped leaf named one
    /// folder while all 14 000 assertions passed.
    ///
    /// Both carry a `{slug}`, and have to: a folder whose name owes nothing to
    /// the title is now measured at parse, so a leaf of literal-and-`{seq}`
    /// alone is refused rather than clamped and could not reach the clamp at
    /// all.
    fn sweep_templates() -> Vec<String> {
        let long = "y".repeat(400);
        let mut all: Vec<String> = SWEEP_TEMPLATES.iter().map(|s| (*s).to_owned()).collect();
        all.push(format!("{{yyyy}}/x{long} {{slug}}"));
        all.push(format!("{{yyyy}}/x{long}{{slug}}{{seq}}"));
        all
    }

    /// A seeded xorshift, so the sweep is the same 1 000 titles on every
    /// machine and a failure can be reproduced from the seed alone. Hand-rolled
    /// because AD-55 refuses a new dependency, here as much as in the crate.
    struct Rng(u64);

    impl Rng {
        fn next_u32(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 32) as u32
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next_u32() as usize) % n.max(1)
        }
    }

    /// The hostile alphabet: separators, `:`, control codes, dots, spaces,
    /// emoji, CJK, combining marks and MS-DOS device names.
    const FRAGMENTS: &[&str] = &[
        "/",
        "\\",
        ":",
        "<",
        ">",
        "\"",
        "|",
        "?",
        "*",
        "\u{0}",
        "\u{1}",
        "\u{7f}",
        "\t",
        "\n",
        "\r\n",
        ".",
        "..",
        "...",
        " ",
        "   ",
        "-",
        "_",
        "🎉",
        "👩‍👩‍👧",
        "日本語",
        "會議",
        "café",
        "e\u{301}",
        "a\u{300}\u{301}",
        "\u{200b}",
        "\u{feff}",
        "\u{202e}gpj.review",
        "a\u{00ad}b",
        // Brackets, which no fragment carried — so not one of the 1 000 swept
        // titles reached the pairing rule, though `assert_legal` has asserted a
        // bracket property since loopback 4. A title may legitimately hold a
        // lone one, which is why that property is preservation-shaped rather
        // than a balance the module would have to invent.
        "(",
        ")",
        "[",
        "]",
        "()",
        ")(",
        "((",
        "(Q3)",
        "con",
        "NUL",
        "lpt9",
        "aux.txt",
        "prn",
        "Standup",
        "Q3 review",
        "{yyyy}",
        "a",
        "9",
    ];

    fn sweep_titles(count: usize) -> Vec<String> {
        let mut rng = Rng(0x5EED_C0FF_EE12_3456);
        let mut titles = Vec::with_capacity(count);
        for i in 0..count {
            let mut title = String::new();
            for _ in 0..=rng.below(6) {
                title.push_str(FRAGMENTS[rng.below(FRAGMENTS.len())]);
            }
            // Every 25th title is a 300-character monster, so the cap and the
            // char-boundary truncation are swept too.
            if i % 25 == 0 {
                let filler = FRAGMENTS[rng.below(FRAGMENTS.len())];
                title.push_str(&filler.repeat(300 / filler.chars().count().max(1)));
            }
            titles.push(title);
        }
        titles
    }

    fn assert_legal(rendered: &RelativePath, parsed: &PathTemplate, title: Option<&str>) {
        let text = rendered.as_str();
        assert!(!text.is_empty(), "empty path from {:?}", parsed.as_str());
        assert!(!text.starts_with('/'), "absolute path {text:?}");
        assert!(!text.ends_with('/'), "trailing separator in {text:?}");

        let separators = parsed.as_str().matches('/').count();
        let components: Vec<&str> = rendered.components().collect();
        assert!(
            components.len() <= separators + 1,
            "{text:?} has more components than {:?} has separators",
            parsed.as_str()
        );

        for component in components {
            assert!(!component.is_empty(), "empty component in {text:?}");
            // Every component is a directory entry `mkdir` has to create, so
            // `NAME_MAX` binds all of them — not only the one the collision
            // retry would vary.
            assert!(
                component.len() <= NAME_MAX_BYTES,
                "component {component:?} of {text:?} is {} bytes",
                component.len()
            );
            assert!(
                component != "." && component != "..",
                "traversal component in {text:?}"
            );
            assert!(
                !component.chars().any(is_illegal),
                "illegal character in {text:?}"
            );
            assert!(
                !component.starts_with(is_edge_noise),
                "leading space or dot in {text:?}"
            );
            assert!(
                !component.ends_with(is_edge_noise),
                "trailing space or dot in {text:?}"
            );
            let stem = component.split('.').next().unwrap_or(component);
            assert!(
                !RESERVED_DEVICE_NAMES.contains(&stem.trim().to_ascii_lowercase().as_str()),
                "reserved device name in {text:?}"
            );
            // Legality is not legibility, and this file checked only legality:
            // 14 000 assertions passed while `({HH}{MM})` rendered `(1432` and
            // `07)`.
            //
            // The property is **preservation**, not balance, and the difference
            // is the title. A title is data — it arrives from a bridge, an agent
            // or a paste, and one that holds a lone `)` renders it exactly as it
            // renders any other legal character, so demanding a balanced name
            // would be demanding that this module rewrite a title to repair a
            // balance it never broke. What it does promise is that it never
            // removes half a pair and never invents a bracket: the template's
            // own brackets come through whole or leave in pairs, and a name
            // whose title contributed none therefore balances outright.
            let title_brackets = |c: char| title.map_or(0, |t| count_char(t, c));
            // A rendered ordinal reads `(2)` — a matched pair this module writes
            // itself, one per `{seq}` the template holds plus the one `render`
            // appends when it holds none.
            let ordinals = parsed.as_str().matches("{seq}").count() + 1;
            for (opens, closes) in [('(', ')'), ('[', ']')] {
                for c in [opens, closes] {
                    let ceiling = count_char(parsed.as_str(), c)
                        + title_brackets(c)
                        + if c == '(' || c == ')' { ordinals } else { 0 };
                    assert!(
                        count_char(component, c) <= ceiling,
                        "{text:?} holds more {c:?} than {:?} and {title:?} can account for",
                        parsed.as_str()
                    );
                }
                if title_brackets(opens) + title_brackets(closes) > 0 {
                    continue;
                }
                let mut depth = 0i32;
                for c in component.chars() {
                    if c == opens {
                        depth += 1;
                    } else if c == closes {
                        depth -= 1;
                    }
                    assert!(depth >= 0, "{closes:?} before its {opens:?} in {text:?}");
                }
                assert_eq!(depth, 0, "unclosed {opens:?} in {text:?}");
            }
        }
    }

    fn count_char(text: &str, c: char) -> usize {
        text.chars().filter(|x| *x == c).count()
    }

    #[test]
    fn a_thousand_hostile_titles_always_render_a_legal_path() {
        let titles = sweep_titles(1_000);
        assert_eq!(titles.len(), 1_000);

        for source in sweep_templates() {
            let parsed = template(&source);
            // The untitled recording is swept beside the hostile ones: it is
            // the case where the most tokens collapse at once, and so the case
            // the leaf rule exists for.
            let cases = titles
                .iter()
                .map(|title| Some(title.as_str()))
                .chain(std::iter::once(None));
            for title in cases {
                for seq in [1, 7] {
                    let rendered = parsed.render(&ctx(title, seq));
                    assert_legal(&rendered, &parsed, title);
                }
            }
        }
    }

    /// Whether `a` names `b` itself or a folder above it, compared component by
    /// component — `2026` is a prefix of `2026/x` but not of `2026 (2)`, which
    /// a `starts_with` on the raw string would get backwards.
    fn is_path_prefix(a: &RelativePath, b: &RelativePath) -> bool {
        let a: Vec<&str> = a.components().collect();
        let b: Vec<&str> = b.components().collect();
        a.len() <= b.len() && b[..a.len()] == a[..]
    }

    #[test]
    fn a_collision_ordinal_makes_a_sibling_never_a_child() {
        // Untitled is the hard case for the collapse: every optional token has
        // gone, so whatever the ordinal attaches to is all that separates the
        // two. Titled is the hard case for the byte cap, which cuts a title back
        // before it ever reaches the clamp — so both are swept, at three seqs
        // rather than two, because a cap can erase what tells them apart.
        for source in sweep_templates() {
            let parsed = template(&source);
            for title in [None, Some("Standup")] {
                let seqs = [1, 2, 7];
                let paths: Vec<RelativePath> = seqs
                    .iter()
                    .map(|seq| parsed.render(&ctx(title, *seq)))
                    .collect();

                for (seq, path) in seqs.iter().zip(&paths) {
                    assert_legal(path, &parsed, title);
                    // The ordinal is the whole of the difference, so it has to
                    // still be there — appended by `render` or rendered in
                    // place by the template's own `{seq}`, and surviving the
                    // clamp either way.
                    if *seq > 1 {
                        let leaf = path.components().last().unwrap_or_default();
                        assert!(
                            leaf.contains(&format!("({seq})")),
                            "{source:?} with {title:?} lost the ordinal at seq {seq}: {leaf:?}"
                        );
                    }
                }

                for (index, first) in paths.iter().enumerate() {
                    for second in paths.iter().skip(index + 1) {
                        assert_ne!(
                            first, second,
                            "{source:?} with {title:?} reuses one path for two sessions"
                        );
                        assert!(
                            !is_path_prefix(first, second),
                            "{source:?}: {second} is inside {first}"
                        );
                        assert!(
                            !is_path_prefix(second, first),
                            "{source:?}: {first} is inside {second}"
                        );
                    }
                }
            }
        }
    }

    /// Every folder shape the agreement property is asked of: one, two and three
    /// atoms drawn from a token that always renders, two that may collapse, some
    /// text, and each separator — brackets included, since they are the shapes
    /// where the two answers are easiest to get differently.
    fn component_shapes() -> Vec<String> {
        const ATOMS: &[&str] = &[
            "{yyyy}", "{slug}", "{title}", "x", "-", " ", "_", ".", "(", ")", "[", "]",
        ];
        let mut shapes = Vec::new();
        for a in ATOMS {
            shapes.push((*a).to_owned());
            for b in ATOMS {
                shapes.push(format!("{a}{b}"));
                for c in ATOMS {
                    shapes.push(format!("{a}{b}{c}"));
                }
            }
        }
        // Three atoms cannot reach a *nested* pair — `(({slug}))` is five — so
        // nothing built above could see the bracket search stop after one pair
        // and leave `()` standing where the token had been. These are written
        // out rather than enumerated, because the enumeration that reaches them
        // is 12⁵ shapes to find a handful.
        for nested in [
            "(({slug}))",
            "((({slug})))",
            "[({slug})]",
            "([{slug}])",
            "(({title}))",
            "(x({slug})y)",
            "(-{slug}-)",
            "((x))",
            "(({yyyy}))",
            "((({slug}",
            "({slug})){mm}",
        ] {
            shapes.push(nested.to_owned());
        }
        shapes
    }

    #[test]
    fn renders_something_agrees_with_what_render_does() {
        // What this pins, exactly, since the name overstates it. The predicate
        // *calls* the renderer, so the two cannot disagree about the collapse
        // rule and no number of shapes could show that they do — that agreement
        // is arranged in the code, not evidenced here. What is left is the
        // **wiring**, and it is worth a property: that `parse` asks the question
        // of the component it means to (the leaf, and every component for
        // `MayRenderEmpty`), that the two errors keep their precedence, and that
        // `render` then drops exactly the components the predicate answered
        // `false` for. Mutation testing bears the distinction out — a broken
        // separator repair still passes this test, because both sides of it move
        // together — so a defect in the collapse itself has to be caught by the
        // worked answers above, never here.
        let mut guaranteed = 0;
        let mut optional = 0;
        for shape in component_shapes() {
            // An interior slot accepts a folder that can render to nothing, so
            // both answers are observable for every shape. A shape refused for
            // some other reason — padded edges, `..`, nothing at all — never
            // reaches the question, in either slot.
            let Ok(parsed) = PathTemplate::parse(&format!("{{yyyy}}/{shape}/{{mm}}")) else {
                continue;
            };
            let predicate = renders_something(&parsed.components[1]);
            let rendered = parsed.render(&ctx(None, 1));
            let survived = rendered.components().count() == 3;
            assert_eq!(
                predicate, survived,
                "{shape:?}: the predicate says {predicate}, render says {rendered}"
            );

            // …and the leaf rule is that same question asked of the last folder:
            // a shape that survives an interior slot is a legal leaf, and one
            // that does not is refused as one.
            let leaf = PathTemplate::parse(&format!("{{yyyy}}/{shape}"));
            assert_eq!(
                leaf.is_ok(),
                survived,
                "{shape:?} renders {rendered}, but as a leaf it parses to {leaf:?}"
            );
            if survived {
                guaranteed += 1;
            } else {
                optional += 1;
            }
        }
        // Both answers are actually reached, so the property cannot pass by
        // never asking the question. There are fewer optional shapes than there
        // were: a bracket is text now, so `(x` and `{slug})` are folders that
        // render, and only a *matching* pair around a collapsing token still
        // leaves nothing behind.
        assert!(
            guaranteed > 1_000 && optional > 50,
            "{guaranteed} guaranteed and {optional} optional shapes"
        );

        // `{seq}` may only be written in the leaf, so its shapes are asked there.
        for (shape, guaranteed) in [
            ("{seq}", false),
            ("({seq})", false),
            ("-{seq}", false),
            ("x{seq}", true),
            ("{mm}{seq}", true),
        ] {
            let leaf = PathTemplate::parse(&format!("{{yyyy}}/{shape}"));
            assert_eq!(leaf.is_ok(), guaranteed, "{shape:?} parsed to {leaf:?}");
            if let Ok(parsed) = leaf {
                let rendered = parsed.render(&ctx(None, 1));
                assert_eq!(
                    rendered.components().count(),
                    2,
                    "{shape:?} rendered {rendered}"
                );
            }
        }
    }

    #[test]
    fn the_ordinal_uses_the_separator_the_user_wrote() {
        // `(2)` grows its own separating space so `{slug}{seq}` reads
        // `standup (2)`. Where the template already ends in a separator, that
        // space is a second one: `{yyyy}-{mm}-{dd}-{seq}` rendered
        // `2026-08-05- (2)`, which is neither what was typed nor what the
        // default renders.
        assert_eq!(render("{yyyy}-{mm}-{dd}-{seq}", None, 2), "2026-08-05-(2)");
        assert_eq!(render("{yyyy}-{mm}-{dd}_{seq}", None, 2), "2026-08-05_(2)");
        assert_eq!(render("{yyyy}-{mm}-{dd}.{seq}", None, 2), "2026-08-05.(2)");
        // …and at seq 1 the separator leaves with the token that did not render,
        // so the two are the same folder plus an ordinal.
        assert_eq!(render("{yyyy}-{mm}-{dd}-{seq}", None, 1), "2026-08-05");

        // Where there is no separator, the space is still what tells the name
        // from the ordinal.
        assert_eq!(render("{yyyy}-{mm}-{dd}{seq}", None, 2), "2026-08-05 (2)");
        assert_eq!(
            render("{yyyy}-{mm}-{dd} {slug}{seq}", Some("Standup"), 2),
            "2026-08-05 standup (2)"
        );
    }

    #[test]
    fn a_device_name_is_escaped_the_same_way_for_every_sibling() {
        // Two faults in one function, both visible in the folder it names.
        // First, the guard read the *trimmed* stem and the escape was written
        // against the untrimmed one, so a title that folded onto `nul ` gained
        // its `-rec` on the far side of the space.
        assert_eq!(
            render("{title}/{yyyy}", Some("nul .txt"), 1),
            "nul-rec.txt/2026"
        );
        assert_eq!(render("{title}/{yyyy}", Some("NUL  "), 1), "NUL-rec/2026");

        // Second, the question was asked of the component *with* its in-place
        // ordinal — and `nul (2)` is not a device name, so one template named
        // the first recording `nul-rec` and its sibling `nul (2)`, on two
        // different rules. The ordinal is this module's punctuation; the name is
        // what the user asked for, and it is the name that is asked about.
        let source = "{yyyy}/nu{slug}{seq}";
        assert_eq!(render(source, Some("L"), 1), "2026/nul-rec");
        assert_eq!(render(source, Some("L"), 2), "2026/nul-rec (2)");
        // …which is what the appended ordinal has always done.
        assert_eq!(render("{yyyy}/nu{slug}", Some("L"), 2), "2026/nul-rec (2)");

        // A folder the user wrote as text alone is still refused rather than
        // escaped — the fold is knowable at parse there.
        assert_eq!(
            PathTemplate::parse("{yyyy}/nul"),
            Err(TemplateError::ReservedComponent("nul".to_owned()))
        );
    }

    #[test]
    fn a_token_name_cannot_smuggle_an_illegal_character_into_the_message() {
        // The characters between `{` and `}` were collected without asking
        // `is_illegal`, and `UnknownToken` quotes them straight back — into
        // 40.2's inline settings copy, where a right-to-left override reverses
        // the rest of the sentence.
        assert_eq!(
            PathTemplate::parse("{\u{202e}gpj}"),
            Err(TemplateError::IllegalCharacter { ch: '\u{202e}' })
        );
        assert_eq!(
            PathTemplate::parse("{yyyy}/{\u{200b}}"),
            Err(TemplateError::IllegalCharacter { ch: '\u{200b}' })
        );
        assert_eq!(
            PathTemplate::parse("{we:ek}"),
            Err(TemplateError::IllegalCharacter { ch: ':' })
        );
        // A legal name that is simply not a token is still the token error, and
        // still quotes the name.
        assert_eq!(
            PathTemplate::parse("{week}"),
            Err(TemplateError::UnknownToken("week".to_owned()))
        );
    }

    #[test]
    fn a_padded_absolute_path_is_still_an_absolute_path() {
        // `starts_with('/')` ran before any trim, so a pasted path with a space
        // in front of it was refused as an `EmptyComponent` — true of the
        // padding, and advice that sends the reader looking for a doubled slash
        // that is not there.
        assert_eq!(
            PathTemplate::parse("  /Users/x/{yyyy}"),
            Err(TemplateError::Absolute)
        );
        assert_eq!(
            PathTemplate::parse(" /{yyyy}"),
            Err(TemplateError::Absolute)
        );
        // Padding that is not in front of a `/` is still the padding error.
        assert_eq!(
            PathTemplate::parse("  {yyyy}  "),
            Err(TemplateError::PaddedComponent("  {yyyy}  ".to_owned()))
        );
    }

    #[test]
    fn the_slug_is_filtered_like_every_other_rendered_character() {
        // `{slug}` folded the *raw* title and never consulted `is_illegal`. It
        // was legal only because `char::is_alphanumeric` happens to exclude
        // `Cc` and `Cf` — another module's incidental behaviour holding this
        // module's promise.
        for title in [
            "annual\u{202e}gpj.review",
            "a\u{200b}b",
            "\u{feff}",
            "a:b/c",
            "\u{1}\u{7f}x",
        ] {
            let rendered = render("{yyyy}/rec-{slug}", Some(title), 1);
            // Per component: `/` is illegal *inside* a name and is the
            // separator between two of them.
            for component in rendered.split('/') {
                assert!(
                    !component.chars().any(is_illegal),
                    "{title:?} rendered {rendered:?}"
                );
            }
        }

        // …and the fold itself is untouched, so a recording folder and a note
        // filename still spell one title one way.
        assert_eq!(
            render("{yyyy}-{slug}", Some("Weekly Review — Q3!"), 1),
            format!("2026-{}", crate::notes::naming::slug("Weekly Review — Q3!"))
        );
        assert_eq!(render("{yyyy}-{slug}", Some("a:b"), 1), "2026-a-b");
    }

    #[test]
    fn a_folder_with_two_title_tokens_is_not_cut_twice_as_far() {
        // One character off the cap comes off *every* title-bearing token at
        // once, so a folder holding both `{title}` and `{slug}` frees twice what
        // one holding a single token frees. Asking for the deficit in
        // single-token characters cut it twice as far as it had to: a 198-byte
        // leaf against a 255-byte budget, nearly a quarter of the name given up
        // for nothing.
        let title = "議".repeat(TITLE_MAX_CHARS);
        let source = "{yyyy}/{mm} {title} {slug}";
        let parsed = template(source);
        let rendered = parsed.render(&ctx(Some(&title), 1));
        let leaf = rendered.components().last().unwrap_or_default();

        assert!(leaf.len() <= NAME_MAX_BYTES, "leaf is {} bytes", leaf.len());
        // …and no more than it had to: one more character in each of the two
        // tokens would not have fit.
        assert!(
            leaf.len() + 2 * "議".len() > NAME_MAX_BYTES,
            "cut too far: {} bytes",
            leaf.len()
        );
        assert_legal(&rendered, &parsed, Some(&title));

        // A folder with one such token is unchanged — the scale is the count.
        let parsed = template("{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {title}");
        let rendered = parsed.render(&ctx(Some(&title), 9));
        let leaf = rendered.components().last().unwrap_or_default();
        assert!(leaf.len() + "議".len() > NAME_MAX_BYTES, "cut too far");
    }

    #[test]
    fn the_date_tokens_mean_here_what_they_mean_in_the_journal_template() {
        // The module doc says this is `journal_path`'s vocabulary and names it
        // as the file to change in step with this one — a claim nothing checked.
        // A user who has written one template has learned both, and two
        // vocabularies that drift apart are a bug nobody files.
        for token in ["{yyyy}", "{yy}", "{mm}", "{dd}"] {
            let source = format!("rec/{token}");
            let ours = render(&source, None, 1);
            let theirs = crate::notes::naming::journal_path(&source, 2026, 8, 5);
            assert_eq!(
                format!("{ours}.md"),
                theirs,
                "{token} renders differently in the two templates"
            );
        }
        // Together, in the shape both defaults actually use.
        let source = "journal/{yyyy}/{yyyy}-{mm}-{dd}";
        assert_eq!(
            format!("{}.md", render(source, None, 1)),
            crate::notes::naming::journal_path(source, 2026, 8, 5)
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "seq is 1-based")]
    fn a_zero_seq_is_a_caller_bug_rather_than_a_first_recording() {
        // `seq: 0` renders byte-identically to `seq: 1`, so a caller counting
        // from zero would write its second recording into the first one's folder
        // with nothing anywhere to say so. The ranges `RenderCtx` documents are
        // checked rather than described.
        template(DEFAULT_TEMPLATE).render(&RenderCtx {
            seq: 0,
            ..ctx(None, 1)
        });
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "outside 0..=9999")]
    fn a_year_outside_four_digits_is_a_caller_bug() {
        // `{yyyy}` is fixed-width only inside `0..=9999`, and `parse` decides a
        // title-free folder's length from exactly that.
        template(DEFAULT_TEMPLATE).render(&RenderCtx {
            year: 10_000,
            ..ctx(None, 1)
        });
    }

    #[test]
    fn rendering_is_deterministic() {
        let parsed = template(DEFAULT_TEMPLATE);
        let once = parsed.render(&ctx(Some("Standup"), 1));
        let twice = parsed.render(&ctx(Some("Standup"), 1));
        assert_eq!(once, twice);
    }
}
