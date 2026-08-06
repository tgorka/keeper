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
//! | Token     | Renders                                    | Example        |
//! |-----------|--------------------------------------------|----------------|
//! | `{yyyy}`  | four-digit year                            | `2026`         |
//! | `{yy}`    | two-digit year                             | `26`           |
//! | `{mm}`    | **month**, zero-padded                     | `08`           |
//! | `{dd}`    | day of the month, zero-padded              | `05`           |
//! | `{HH}`    | hour on a 24-hour clock, zero-padded       | `14`           |
//! | `{MM}`    | **minute**, zero-padded                    | `32`           |
//! | `{SS}`    | second, zero-padded                        | `07`           |
//! | `{title}` | the title, illegal characters removed      | `Café Standup` |
//! | `{slug}`  | the title, folded to a slug                | `cafe-standup` |
//! | `{seq}`   | collision ordinal — nothing, or ` (2)`     | ` (2)`         |
//!
//! `{mm}` is the **month** and `{MM}` is the **minute** — case-sensitive, and
//! the one pair worth reading twice.
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
//! quietly rewritten into a path the user did not ask for.
//! [`PathTemplate::render`] is then **infallible** — every decision was made at
//! parse time — and its output holds unconditionally, for every template that
//! parsed and every title that exists:
//!
//! - no `:` anywhere, and nothing else from the illegal set
//!   (`< > : " / \ | ? *`, `NUL`, control characters — the union of what APFS,
//!   exFAT and NTFS refuse, so a FAT pendrive stays a legal destination);
//! - never absolute, no leading or trailing separator;
//! - no component that is empty, `.`, `..` or an MS-DOS device name;
//! - no component with a leading or trailing space or `.`;
//! - and a title can never introduce more components than the template's own
//!   `/` count — a hostile title cannot deepen or escape the path.
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
/// short enough that a title in any ordinary script — together with the date
/// prefix the default template puts in front of it — stays well inside the
/// 255-byte name limit. It is a *character* cap, not a byte cap: 80 four-byte
/// codepoints still exceed 255 bytes, and that pathological title is left for
/// the filesystem to refuse by the name it was actually given, rather than
/// silently reshaped here into a folder the user did not ask for.
const TITLE_MAX_CHARS: usize = 80;

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
    /// destination root.
    #[error("a template cannot contain a \".\" or \"..\" folder")]
    ParentComponent,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderCtx {
    /// Full year, e.g. `2026`.
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
    /// Whether the user placed `{seq}` themselves. If not, the collision
    /// ordinal is appended to the final component instead.
    has_seq: bool,
}

impl PathTemplate {
    /// Validate `input`, or say precisely what is wrong with it.
    pub fn parse(input: &str) -> Result<Self, TemplateError> {
        if input.trim().is_empty() {
            return Err(TemplateError::Empty);
        }
        if input.starts_with('/') {
            return Err(TemplateError::Absolute);
        }

        let mut components: Vec<Vec<Segment>> = Vec::new();
        let mut current: Vec<Segment> = Vec::new();
        let mut literal = String::new();
        let mut has_seq = false;
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
                        name.push(c);
                    }
                    if !closed {
                        return Err(TemplateError::Unterminated);
                    }
                    let Some(token) = Token::from_name(&name) else {
                        return Err(TemplateError::UnknownToken(name));
                    };
                    has_seq |= token == Token::Seq;
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

        for component in &components {
            if let [Segment::Literal(text)] = component.as_slice() {
                if text == "." || text == ".." {
                    return Err(TemplateError::ParentComponent);
                }
            }
        }
        if !components.iter().any(|c| renders_something(c)) {
            return Err(TemplateError::MayRenderEmpty);
        }

        Ok(Self {
            raw: input.to_owned(),
            components,
            has_seq,
        })
    }

    /// The template exactly as the user wrote it — nothing was rewritten.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Render this template against `ctx`. Infallible by construction.
    pub fn render(&self, ctx: &RenderCtx) -> RelativePath {
        let raw_title = ctx.title.as_deref().unwrap_or_default();
        let title = sanitize_title(raw_title);
        let slug = slug_stem(raw_title);

        let mut rendered: Vec<String> = Vec::with_capacity(self.components.len());
        for component in &self.components {
            // A component that renders to nothing takes its separator with it,
            // which is why this drops the component rather than pushing an
            // empty one: `{yyyy}/{slug}/x` is `2026/x`, never `2026//x`.
            let text = render_component(component, ctx, &title, &slug);
            let text = text.trim_matches(is_edge_noise);
            if !text.is_empty() {
                rendered.push(de_reserve(text));
            }
        }

        // Without an explicit `{seq}`, the collision ordinal lands on the final
        // component — the folder that is actually colliding — and nowhere else.
        if !self.has_seq && ctx.seq > 1 {
            if let Some(last) = rendered.last_mut() {
                last.push_str(&seq_suffix(ctx.seq));
            }
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

    /// Optional tokens may render to nothing — and when they do, they collapse
    /// together with one adjacent separator run. The date and time tokens
    /// always render, which is what makes a component containing one a folder
    /// that is guaranteed to exist.
    fn is_optional(self) -> bool {
        matches!(self, Self::Title | Self::Slug | Self::Seq)
    }
}

/// One rendered piece, kept separate until the collapse pass has run.
struct Piece {
    text: String,
    /// Literal text the user typed — the only thing a collapse may eat into.
    literal: bool,
    /// An optional token that rendered to nothing.
    collapsed: bool,
    /// A rendered `{seq}`, which grows its own separating space at join time —
    /// after the collapse pass, because the collapse is what decides whether
    /// there is already a space in front of it.
    seq: bool,
}

fn push_literal(current: &mut Vec<Segment>, literal: &mut String) {
    if !literal.is_empty() {
        current.push(Segment::Literal(std::mem::take(literal)));
    }
}

/// Whether this component is guaranteed to render to something, so the path can
/// never be empty. True when it holds a date/time token, or literal text that
/// survives both trimming and collapsing.
fn renders_something(component: &[Segment]) -> bool {
    component.iter().any(|segment| match segment {
        Segment::Token(token) => !token.is_optional(),
        Segment::Literal(text) => text.chars().any(|c| !is_filler(c)),
    })
}

fn render_component(segments: &[Segment], ctx: &RenderCtx, title: &str, slug: &str) -> String {
    let mut pieces: Vec<Piece> = Vec::with_capacity(segments.len());
    for segment in segments {
        match segment {
            Segment::Literal(text) => pieces.push(Piece {
                text: text.clone(),
                literal: true,
                collapsed: false,
                seq: false,
            }),
            Segment::Token(token) => {
                let text = render_token(*token, ctx, title, slug);
                let collapsed = token.is_optional() && text.is_empty();
                pieces.push(Piece {
                    text,
                    literal: false,
                    collapsed,
                    seq: *token == Token::Seq,
                });
            }
        }
    }

    // The collapse rule: an optional token that rendered to nothing removes
    // itself *and one adjacent literal separator run* — the preceding one if
    // there is one, otherwise the following one. Done here, during assembly,
    // rather than by trimming the finished string, because the difference shows
    // up in the middle of a component as much as at its edges.
    let mut i = 0;
    while i < pieces.len() {
        if pieces[i].collapsed {
            let mut absorbed = false;
            if i > 0 && pieces[i - 1].literal {
                let kept = pieces[i - 1].text.trim_end_matches(is_filler).to_owned();
                if kept.len() != pieces[i - 1].text.len() {
                    pieces[i - 1].text = kept;
                    absorbed = true;
                }
            }
            if !absorbed && i + 1 < pieces.len() && pieces[i + 1].literal {
                pieces[i + 1].text = pieces[i + 1].text.trim_start_matches(is_filler).to_owned();
            }
        }
        i += 1;
    }

    // `(2)` carries its own separating space, so `{slug}{seq}` reads as
    // `standup (2)` — unless whatever survived the collapse already ends in
    // whitespace, where a second space would only be noise.
    let mut out = String::new();
    for piece in pieces {
        if piece.seq
            && !piece.text.is_empty()
            && !out.is_empty()
            && !out.ends_with(char::is_whitespace)
        {
            out.push(' ');
        }
        out.push_str(&piece.text);
    }
    out
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

/// Escape an MS-DOS device name. Reserved on Windows in *every* directory, with
/// or without an extension — so the check is on the stem, and so is the fix:
/// `nul.mp4` must become `nul-rec.mp4`, not `nul.mp4-rec`, which would still be
/// a device.
fn de_reserve(component: &str) -> String {
    let (stem, rest) = match component.find('.') {
        Some(at) => component.split_at(at),
        None => (component, ""),
    };
    if RESERVED_DEVICE_NAMES.contains(&stem.trim().to_ascii_lowercase().as_str()) {
        format!("{stem}{RESERVED_SUFFIX}{rest}")
    } else {
        component.to_owned()
    }
}

/// The union of what APFS, exFAT and NTFS refuse in a name. `/` is included:
/// inside a *component* it is as illegal as the rest, and the template parser
/// takes it out of the stream before this is consulted.
fn is_illegal(c: char) -> bool {
    matches!(
        c,
        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\0'
    ) || c.is_control()
}

/// Characters a collapsing token may eat on its way out: the separators a user
/// writes between tokens.
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
        assert_legal(&parsed.render(&ctx(Some("Standup"), 1)), &parsed);
        assert_legal(&parsed.render(&ctx(Some("Standup"), 2)), &parsed);
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
        let source = "{yyyy}/{title}";
        let parsed = template(source);
        let rendered = parsed.render(&ctx(Some("a/b:c"), 1));

        assert_eq!(rendered.as_str(), "2026/abc");
        assert_eq!(rendered.components().count(), 2);
        assert_legal(&rendered, &parsed);

        // `..`, a traversal attempt and a leading dot all sanitise away.
        assert_eq!(render(source, Some("../../etc"), 1), "2026/etc");
        assert_eq!(render(source, Some(".."), 1), "2026");
        assert_eq!(render(source, Some("C:\\Windows"), 1), "2026/CWindows");
    }

    #[test]
    fn title_keeps_the_letters_and_slug_folds_them() {
        // The matrix states these as bare `{title}` / `{slug}` templates; both
        // are refused by `MayRenderEmpty` (see the parse test below), so the
        // token semantics are shown under a folder that always renders.
        assert_eq!(
            render("{yyyy}/{title}", Some("Café Déjà Vu"), 1),
            "2026/Café Déjà Vu"
        );
        assert_eq!(
            render("{yyyy}/{slug}", Some("Café Déjà Vu"), 1),
            "2026/cafe-deja-vu"
        );
    }

    #[test]
    fn a_title_is_filtered_not_refused() {
        assert_eq!(
            render("{yyyy}/{title}", Some("  a\t\tb \n c  "), 1),
            "2026/a b c"
        );
        assert_eq!(
            render("{yyyy}/{title}", Some("no <ctrl>\u{1}\u{7f} here"), 1),
            "2026/no ctrl here"
        );
        // Capped on a character boundary, at characters and not bytes.
        let long = render("{yyyy}/{title}", Some(&"ä".repeat(300)), 1);
        let component = long.split('/').next_back().unwrap_or_default();
        assert_eq!(component.chars().count(), TITLE_MAX_CHARS);
    }

    #[test]
    fn a_reserved_device_name_is_never_rendered_bare() {
        assert_eq!(render("{yyyy}/{slug}", Some("NUL"), 1), "2026/nul-rec");
        assert_eq!(render("{yyyy}/{title}", Some("con"), 1), "2026/con-rec");
        assert_eq!(
            render("{yyyy}/{title}", Some("aux.txt"), 1),
            "2026/aux-rec.txt"
        );
        // A name that merely starts with one is left alone.
        assert_eq!(render("{yyyy}/{slug}", Some("console"), 1), "2026/console");
    }

    #[test]
    fn the_slug_matches_what_the_notes_domain_would_produce() {
        // One fold in the crate: a recording folder and a note filename can
        // never disagree about what a title looks like.
        assert_eq!(
            render("{yyyy}/{slug}", Some("Weekly Review — Q3!"), 1),
            format!("2026/{}", crate::notes::naming::slug("Weekly Review — Q3!"))
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
    fn parse_keeps_the_template_verbatim() {
        let parsed = template(DEFAULT_TEMPLATE);
        assert_eq!(parsed.as_str(), DEFAULT_TEMPLATE);
        assert_eq!(template("  {yyyy}  ").as_str(), "  {yyyy}  ");
        // …and renders it without the padding the user left around it.
        assert_eq!(render("  {yyyy}  ", None, 1), "2026");
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
            render("{yyyy} {yy} {mm} {dd} {HH} {MM} {SS}/{title}", Some("T"), 1),
            "2026 26 08 05 14 32 07/T"
        );
    }

    // --- the property sweep -------------------------------------------------

    /// Every template the property sweep renders. Each bears tokens, and each
    /// parses — the sweep is about rendering, not validation.
    const SWEEP_TEMPLATES: &[&str] = &[
        DEFAULT_TEMPLATE,
        "{yyyy}/{title}",
        "{yyyy}/{slug}/{HH}{MM}",
        "{yy}{mm}{dd} {title} {slug}{seq}",
        "rec/{yyyy}/{mm}/{dd}/{HH}{MM}{SS} {title}",
        "{yyyy}-{mm}-{dd}_{slug}",
        "{yyyy}/{slug} ({SS})",
    ];

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

    fn assert_legal(rendered: &RelativePath, parsed: &PathTemplate) {
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
        }
    }

    #[test]
    fn a_thousand_hostile_titles_always_render_a_legal_path() {
        let titles = sweep_titles(1_000);
        assert_eq!(titles.len(), 1_000);

        for source in SWEEP_TEMPLATES {
            let parsed = template(source);
            for title in &titles {
                for seq in [1, 7] {
                    let rendered = parsed.render(&ctx(Some(title), seq));
                    assert_legal(&rendered, &parsed);
                }
            }
        }
    }

    #[test]
    fn rendering_is_deterministic() {
        let parsed = template(DEFAULT_TEMPLATE);
        let once = parsed.render(&ctx(Some("Standup"), 1));
        let twice = parsed.render(&ctx(Some("Standup"), 1));
        assert_eq!(once, twice);
    }
}
