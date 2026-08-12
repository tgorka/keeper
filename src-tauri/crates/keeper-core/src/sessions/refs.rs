//! What a session points at (Phase 7, FR-255, AD-118).
//!
//! The zone's own rule is that a session does not hold what it refers to:
//! *"Big binaries never live in a session — file them in their zone
//! (`40-media`, …) and reference by repo-root-relative path from `refs/` or the
//! README."* So a session is a hub of pointers, and the thing that breaks is
//! the pointer: move the file, and the session still says where it used to be.
//! Nothing reported that. This module is what reports it.
//!
//! **The tree is not this.** [`super::vm::SessionTreeVm`] lists the bytes a
//! session *holds*; this lists what it *names*. `refs/inputs.md` appears in the
//! tree as a file and here as a source — and the `40-media/standup.m4a` it
//! points at appears only here, because it is not in the session at all.
//!
//! ## Six kinds, each with a real predicate
//!
//! Every one is somebody else's rule, asked rather than restated:
//!
//! - **note** — the vault index resolves it ([`crate::notes::index::IndexSnapshot::resolve_link`],
//!   which is what backlinks are built from). One resolver, or a link opens one
//!   note and appears in another note's backlinks.
//! - **recording** — a note whose frontmatter carries `session:`
//!   ([`crate::notes::recording_note::is_recording_note`]). **Not an extension.**
//!   A loose `.m4a` is a file; a recording is a session keeper minted an id for,
//!   and classifying by suffix would be the second predicate that module spends
//!   ten lines warning against.
//! - **file** — a path that exists on disk, session-relative first, then
//!   profile-relative ([`crate::notes::embed::candidates`]' shape, session-sized).
//! - **session** — a file that turned out to be another session's folder. The
//!   lineage chips already carry `continues`/`continued-by`; this is the other
//!   way sessions reference each other, by path, in prose.
//! - **external** — `http`/`https`. Opened by the system browser, never probed:
//!   keeper does not do network IO to decide whether a row is red.
//! - **missing** — everything a resolver answered `None` to. The bucket the
//!   feature exists for.
//!
//! ## Two asymmetries, both deliberate
//!
//! **A link can be missing; a quoted path can only be found.** Prose is full of
//! backticked paths that are not references — `src/main.rs` in a paragraph about
//! some other repository, `node_modules/.bin/vite` in a command. A link is an
//! author saying "this is a thing"; a backticked path is an author typing. So a
//! quoted path that resolves is reported and one that does not is silently
//! dropped, because the alternative is a widget that cries missing about every
//! shell command in the log.
//!
//! **External is never missing.** `https://example.com/gone` is a 404 keeper
//! would have to go on the network to learn, and a references widget that made
//! HTTP requests would be a tracker. It says external and opens the browser.
//!
//! Pure, like everything else in [`super`]: bytes and a probe in, values out.
//! The probe is a trait rather than three closures because the shell answers all
//! three from the same registries, and [`crate::notes::export::plan`] set the
//! precedent of handing the rules an `exists` rather than a filesystem.

use crate::notes::export::names_a_note;
use crate::notes::links;
use crate::notes::tags::{code_spans, in_code};

/// One pointer, exactly as the author wrote it, before anything resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRef {
    /// The target text, anchor stripped, percent-decoding already applied for
    /// markdown links — whatever a resolver should be asked about.
    pub target: String,
    /// The link's own words, when it had any: `[[target|alias]]`, `[text](…)`.
    pub label: Option<String>,
    /// An `http`/`https` URL rather than something inside the drive.
    pub external: bool,
    /// Found inside backticks rather than written as a link, which is what
    /// makes it findable-only (see the module doc).
    pub quoted: bool,
}

/// A note the vault index answered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteHit {
    pub note_id: String,
    pub title: String,
    /// The note's frontmatter carries `session:` — keeper wrote it about a
    /// recording.
    pub recording: bool,
}

/// Another session the path landed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHit {
    pub title: String,
}

/// What the shell can answer about the world. Three questions, because the
/// three kinds that can resolve resolve against three different registries.
pub trait RefProbe {
    /// The vault index's own resolution, or `None` for a target nothing answers
    /// to (an ordinary thing to have written).
    fn note(&self, target: &str) -> Option<NoteHit>;
    /// Whether a **profile-relative** path is on disk.
    fn exists(&self, subpath: &str) -> bool;
    /// Whether an existing profile-relative path belongs to a session *other
    /// than the one being read* — the caller's own session is not a reference
    /// to itself.
    fn session(&self, subpath: &str) -> Option<SessionHit>;
}

/// What a reference turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Note,
    Recording,
    File,
    Session,
    External,
    Missing,
}

impl RefKind {
    /// The wire spelling — one word, the vocabulary the rest of the product
    /// already uses (`missing` is [`crate::notes::export::NoteExportPlan`]'s).
    pub fn as_str(self) -> &'static str {
        match self {
            RefKind::Note => "note",
            RefKind::Recording => "recording",
            RefKind::File => "file",
            RefKind::Session => "session",
            RefKind::External => "external",
            RefKind::Missing => "missing",
        }
    }
}

/// What clicking a reference should open. The shell turns these into
/// [`crate::panels::PanelTargetVm`]s — it holds the ids, and a vault id, a
/// profile id and a sessions root id are all the same string (AD-90, AD-107),
/// which is exactly the coincidence a pure module should not be asserting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefTarget {
    /// A note in this profile's vault.
    Note { note_id: String },
    /// A profile-relative path.
    File { subpath: String },
    /// An `http(s)` URL, for the system browser.
    External { url: String },
    /// Nothing to open, and the paths keeper looked in — `embed`'s lesson:
    /// "keeper could not find it" sends somebody to search four hundred
    /// folders, and naming the paths tells them the file is one `mv` away.
    Missing { looked: Vec<String> },
}

/// One row of the references widget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRef {
    pub kind: RefKind,
    /// The target **as the author spelled it** — never a normalised key. The
    /// export receipt's rule: a missing row a person cannot find in their own
    /// file is a missing row they cannot fix.
    pub target: String,
    /// What to call it: the resolved title, else the link's own words, else the
    /// target.
    pub label: String,
    /// Session-relative path of the file it was written in (`README.md`,
    /// `refs/inputs.md`).
    pub source: String,
    pub open: RefTarget,
}

/// Every pointer in one body, in document order.
///
/// Two passes, because they see different things: [`links::extract`] finds the
/// four link shapes and **drops every external URL at the door** (a `[docs](…)`
/// is not an edge in the vault graph, which is right for backlinks and wrong
/// here), so the URLs are scanned separately. A one-pass merge would mean
/// reimplementing wikilink parsing, and two parsers for `[[…]]` is the kind of
/// drift this codebase keeps a single `link_key` to avoid.
pub fn scan(body: &str) -> Vec<RawRef> {
    let code = code_spans(body);
    let mut out: Vec<(usize, RawRef)> = Vec::new();

    for link in links::extract(body) {
        out.push((
            link.span.0,
            RawRef {
                target: link.target,
                label: link.alias,
                external: false,
                quoted: false,
            },
        ));
    }
    out.extend(external_urls(body, &code));
    out.extend(quoted_paths(body, &code));

    out.sort_by_key(|(at, _)| *at);
    out.into_iter().map(|(_, raw)| raw).collect()
}

/// `http(s)` URLs, however they were written: bare, `<autolinked>`, or as a
/// markdown link's destination. One scan over the scheme catches all three,
/// where matching each syntax separately would miss the shape nobody thought of.
fn external_urls(body: &str, code: &[(usize, usize)]) -> Vec<(usize, RawRef)> {
    const SCHEMES: [&str; 2] = ["https://", "http://"];
    let mut out = Vec::new();
    let mut at = 0usize;

    while at < body.len() {
        let Some(found) = SCHEMES
            .iter()
            .filter_map(|scheme| body[at..].find(scheme).map(|i| (at + i, scheme.len())))
            .min_by_key(|(i, _)| *i)
        else {
            break;
        };
        let (start, scheme_len) = found;
        if in_code(code, start) {
            at = start + scheme_len;
            continue;
        }
        let end = url_end(body, start);
        let url = &body[start..end];
        // A scheme with nothing after it is prose about URLs, not one.
        if url.len() > scheme_len {
            out.push((
                start,
                RawRef {
                    target: url.to_owned(),
                    label: markdown_label(body, start),
                    external: true,
                    quoted: false,
                },
            ));
        }
        at = end.max(start + scheme_len);
    }
    out
}

/// Where a URL stops. Whitespace and the delimiters that wrap one end it, and
/// so does trailing prose punctuation — `see https://example.com.` is a
/// sentence, and the full stop is not part of the address.
fn url_end(body: &str, start: usize) -> usize {
    let mut end = body.len();
    for (offset, c) in body[start..].char_indices() {
        if c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`' | '|' | '\\') {
            end = start + offset;
            break;
        }
    }
    let url = &body[start..end];
    // A `)` closes the markdown link that wrapped it unless the URL opened one
    // itself, which Wikipedia addresses do constantly.
    let balanced = url.contains('(');
    let trimmed = url.trim_end_matches(|c| match c {
        '.' | ',' | ';' | ':' | '!' | '?' | '\'' | ']' => true,
        ')' => !balanced,
        _ => false,
    });
    start + trimmed.len()
}

/// The link text of a `[text](url)` whose destination starts at `at`, when that
/// is what this URL is. Read backwards, because the scan found the destination.
fn markdown_label(body: &str, at: usize) -> Option<String> {
    let before = body[..at].trim_end();
    let open = before.strip_suffix('(')?;
    let close = open.strip_suffix(']')?;
    let start = close.rfind('[')?;
    let text = close[start + 1..].trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Backticked spans that look like paths. Strict on purpose: a `/`, no
/// whitespace, and no leading URL scheme. Everything else a person types
/// between backticks is a command, and a widget that called `cargo test`
/// a missing reference would be one nobody reads twice.
fn quoted_paths(body: &str, code: &[(usize, usize)]) -> Vec<(usize, RawRef)> {
    let mut out = Vec::new();
    for (start, end) in code {
        let raw = &body[*start..*end];
        let inner = raw.trim_matches('`').trim();
        if inner.contains('/')
            && !inner.contains(char::is_whitespace)
            && !inner.contains("://")
            // A fenced block is code, not a path, however few lines it has.
            && !raw.starts_with("```")
            && !inner.starts_with('/')
        {
            out.push((
                *start,
                RawRef {
                    target: inner.to_owned(),
                    label: None,
                    external: false,
                    quoted: true,
                },
            ));
        }
    }
    out
}

/// Resolve and order everything found across a session's files.
///
/// `found` is `(source, raw)` in the order the shell read them — the README
/// first, then `refs/`. `prefix` is the session's own profile-relative folder,
/// which is what makes `artifacts/report.md` in a session README mean the file
/// beside it rather than one at the drive root.
///
/// **Missing rows sort first**, and the rest keep document order. The widget's
/// whole reason to exist is the pointer that broke while nobody was looking, so
/// burying it under thirty working links would be a report that technically
/// contains the answer. Sorted here rather than in React for the reason the
/// tree gives: the order is a rule, and a rule that lives in the renderer is a
/// rule two renderers can disagree about.
///
/// One row per target: a file linked five times is one thing that either
/// resolves or does not ([`crate::notes::export::plan`]'s rule, for the same
/// reason). The first source wins, because that is where a person will start
/// looking.
pub fn plan(found: &[(String, RawRef)], prefix: &str, probe: &dyn RefProbe) -> Vec<ResolvedRef> {
    let mut out: Vec<ResolvedRef> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for (source, raw) in found {
        let key = dedupe_key(raw);
        if seen.contains(&key) {
            continue;
        }
        if let Some(resolved) = resolve(source, raw, prefix, probe) {
            seen.push(key);
            out.push(resolved);
        }
    }

    out.sort_by_key(|row| u8::from(row.kind != RefKind::Missing));
    out
}

/// What makes two pointers the same pointer. Case-folded, because a link is
/// typed by hand and `40-Media/x.m4a` is not a second file.
fn dedupe_key(raw: &RawRef) -> String {
    raw.target.trim().to_lowercase()
}

/// One pointer, resolved. `None` drops it from the widget entirely — which
/// happens only for a quoted path that resolved to nothing (see the module
/// doc's second asymmetry).
fn resolve(source: &str, raw: &RawRef, prefix: &str, probe: &dyn RefProbe) -> Option<ResolvedRef> {
    let row = |kind: RefKind, label: String, open: RefTarget| ResolvedRef {
        kind,
        target: raw.target.clone(),
        label,
        source: source.to_owned(),
        open,
    };
    let written = || raw.label.clone().unwrap_or_else(|| raw.target.clone());

    if raw.external {
        return Some(row(
            RefKind::External,
            written(),
            RefTarget::External {
                url: raw.target.clone(),
            },
        ));
    }

    // A note first, and only when the target names one: `names_a_note` is what
    // keeps `40-media/rec.m4a` from being asked about in the note index, where
    // a stem match could answer for an unrelated `rec.md`.
    if names_a_note(&raw.target) {
        if let Some(hit) = probe.note(&raw.target) {
            let kind = if hit.recording {
                RefKind::Recording
            } else {
                RefKind::Note
            };
            return Some(row(
                kind,
                pick_label(&hit.title, raw),
                RefTarget::Note {
                    note_id: hit.note_id,
                },
            ));
        }
    }

    let looked = candidates(&raw.target, prefix);
    if let Some(hit) = looked.iter().find(|subpath| probe.exists(subpath)) {
        if let Some(session) = probe.session(hit) {
            return Some(row(
                RefKind::Session,
                pick_label(&session.title, raw),
                RefTarget::File {
                    subpath: hit.clone(),
                },
            ));
        }
        return Some(row(
            RefKind::File,
            written(),
            RefTarget::File {
                subpath: hit.clone(),
            },
        ));
    }

    // Nothing answered. A link says so; a backticked path was probably never a
    // reference in the first place.
    if raw.quoted {
        return None;
    }
    Some(row(
        RefKind::Missing,
        written(),
        RefTarget::Missing { looked },
    ))
}

/// The resolved title beats the link's own words, which beat the raw target —
/// but an explicit alias beats everything, because `[[Standup|yesterday's
/// call]]` is the author naming this reference in this place.
fn pick_label(title: &str, raw: &RawRef) -> String {
    match &raw.label {
        Some(alias) => alias.clone(),
        None if !title.trim().is_empty() => title.to_owned(),
        None => raw.target.clone(),
    }
}

/// Where a target could be, profile-relative, in the order keeper looks.
///
/// Session-relative first: a link written inside a session almost always means
/// the file beside it, and resolving `README.md` to the *zone's* README would
/// be both wrong and confusing. Then the target as written, which is the
/// repo-root-relative spelling the drives' own AGENTS.md asks for.
///
/// [`crate::notes::embed::candidates`]' shape rather than its function: that one folds in the
/// vault's `attachments/` folder, which a session does not have.
fn candidates(target: &str, prefix: &str) -> Vec<String> {
    let clean = target.trim_start_matches("./");
    let mut out = Vec::new();
    if !prefix.is_empty() {
        out.push(format!("{prefix}/{clean}"));
    }
    if !out.iter().any(|existing| existing == clean) {
        out.push(clean.to_owned());
    }
    out
}

/// The sentence a missing row carries, naming what keeper looked for — the
/// acceptance criterion [`crate::notes::embed::not_found_notice`] was written to, applied to
/// a session's own frame.
pub fn missing_notice(target: &str, looked: &[String]) -> String {
    let paths = looked.join(" and ");
    format!(
        "{target}: this session points at something the drive does not have — \
         keeper looked for {paths}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A probe with everything spelled out, so each test says which world it
    /// is asking about.
    #[derive(Default)]
    struct Fake {
        notes: Vec<(&'static str, &'static str, &'static str, bool)>,
        files: Vec<&'static str>,
        sessions: Vec<(&'static str, &'static str)>,
    }

    impl RefProbe for Fake {
        fn note(&self, target: &str) -> Option<NoteHit> {
            self.notes
                .iter()
                .find(|(key, _, _, _)| key.eq_ignore_ascii_case(target))
                .map(|(_, id, title, recording)| NoteHit {
                    note_id: (*id).to_owned(),
                    title: (*title).to_owned(),
                    recording: *recording,
                })
        }
        fn exists(&self, subpath: &str) -> bool {
            self.files.contains(&subpath)
        }
        fn session(&self, subpath: &str) -> Option<SessionHit> {
            self.sessions
                .iter()
                .find(|(path, _)| *path == subpath)
                .map(|(_, title)| SessionHit {
                    title: (*title).to_owned(),
                })
        }
    }

    const PREFIX: &str = "60-sessions/active/2026-08-10-keeper";

    fn found(body: &str) -> Vec<(String, RawRef)> {
        scan(body)
            .into_iter()
            .map(|raw| ("README.md".to_owned(), raw))
            .collect()
    }

    /// The external half of the scan is the half `links::extract` cannot do —
    /// bare, autolinked and markdown-wrapped URLs all arrive, with the
    /// markdown one keeping its own words.
    #[test]
    fn finds_external_urls_in_all_three_spellings() {
        let raws = scan(
            "bare https://example.com/a, auto <https://example.com/b>, [the RFC](https://example.com/c)",
        );
        let external: Vec<_> = raws.iter().filter(|raw| raw.external).collect();
        assert_eq!(external.len(), 3);
        assert_eq!(external[0].target, "https://example.com/a");
        assert_eq!(external[1].target, "https://example.com/b");
        assert_eq!(external[2].target, "https://example.com/c");
        assert_eq!(external[2].label.as_deref(), Some("the RFC"));
    }

    /// Trailing prose punctuation is prose. A parenthesis the URL opened
    /// itself is not.
    #[test]
    fn a_url_stops_before_the_full_stop_that_ends_the_sentence() {
        let raws = scan(
            "See https://example.com/page. Also https://en.wikipedia.org/wiki/Foo_(bar) here.",
        );
        let external: Vec<_> = raws.iter().filter(|raw| raw.external).collect();
        assert_eq!(external[0].target, "https://example.com/page");
        assert_eq!(
            external[1].target,
            "https://en.wikipedia.org/wiki/Foo_(bar)"
        );
    }

    /// A URL inside a fenced block is documentation about a URL.
    #[test]
    fn skips_urls_and_links_inside_code() {
        let raws =
            scan("```sh\ncurl https://example.com/nope\n```\n\nreal https://example.com/yes\n");
        let external: Vec<_> = raws.iter().filter(|raw| raw.external).collect();
        assert_eq!(external.len(), 1);
        assert_eq!(external[0].target, "https://example.com/yes");
    }

    /// A note link resolves through the vault index, and a note the index says
    /// keeper wrote about a recording is a recording — by its frontmatter key,
    /// never by a file suffix.
    #[test]
    fn a_recording_is_a_note_with_a_session_key_not_a_media_extension() {
        let probe = Fake {
            notes: vec![
                ("Standup", "01JNOTE", "Standup 2026-08-10", true),
                ("Vault as a lens", "01JLENS", "Vault as a lens", false),
            ],
            files: vec!["40-media/standup.m4a"],
            ..Fake::default()
        };
        let rows = plan(
            &found("[[Standup]] and [[Vault as a lens]] and [clip](40-media/standup.m4a)"),
            PREFIX,
            &probe,
        );
        assert_eq!(rows[0].kind, RefKind::Recording);
        assert_eq!(rows[0].label, "Standup 2026-08-10");
        assert_eq!(rows[1].kind, RefKind::Note);
        // The audio file is a FILE. It is media, it is not a recording.
        assert_eq!(rows[2].kind, RefKind::File);
        assert_eq!(
            rows[2].open,
            RefTarget::File {
                subpath: "40-media/standup.m4a".to_owned()
            }
        );
    }

    /// A link beside the session beats one at the drive root, and the drive
    /// root is what a repo-root-relative pointer means.
    #[test]
    fn looks_beside_the_session_first_then_at_the_drive_root() {
        let probe = Fake {
            files: vec![
                "60-sessions/active/2026-08-10-keeper/artifacts/report.md",
                "40-media/clip.mov",
            ],
            ..Fake::default()
        };
        let rows = plan(
            &found("[r](artifacts/report.md) and [c](40-media/clip.mov)"),
            PREFIX,
            &probe,
        );
        assert_eq!(
            rows[0].open,
            RefTarget::File {
                subpath: "60-sessions/active/2026-08-10-keeper/artifacts/report.md".to_owned()
            }
        );
        assert_eq!(
            rows[1].open,
            RefTarget::File {
                subpath: "40-media/clip.mov".to_owned()
            }
        );
    }

    /// The feature's whole reason to exist: the pointer that broke. It says
    /// missing, keeps the author's own spelling, names both places keeper
    /// looked, and sorts to the top past a working link written before it.
    #[test]
    fn a_moved_file_is_missing_first_in_the_authors_own_words() {
        let probe = Fake {
            files: vec!["40-media/kept.mov"],
            ..Fake::default()
        };
        let rows = plan(
            &found("[fine](40-media/kept.mov) then [gone](40-Media/Moved.m4a)"),
            PREFIX,
            &probe,
        );
        assert_eq!(rows[0].kind, RefKind::Missing);
        assert_eq!(rows[0].target, "40-Media/Moved.m4a", "spelled as written");
        assert_eq!(rows[1].kind, RefKind::File, "the working row sorts after");
        let RefTarget::Missing { looked } = &rows[0].open else {
            panic!("missing carries where keeper looked");
        };
        assert_eq!(
            looked,
            &[
                "60-sessions/active/2026-08-10-keeper/40-Media/Moved.m4a".to_owned(),
                "40-Media/Moved.m4a".to_owned()
            ]
        );
        assert!(missing_notice(&rows[0].target, looked).contains("keeper looked for"));
    }

    /// A path that landed on another session is that session, not a loose file.
    #[test]
    fn a_path_into_another_session_is_a_session() {
        let probe = Fake {
            files: vec!["60-sessions/archive/2025/2025-03-01-taxes/README.md"],
            sessions: vec![(
                "60-sessions/archive/2025/2025-03-01-taxes/README.md",
                "Taxes 2024",
            )],
            ..Fake::default()
        };
        let rows = plan(
            &found("see [the old one](60-sessions/archive/2025/2025-03-01-taxes/README.md)"),
            PREFIX,
            &probe,
        );
        assert_eq!(rows[0].kind, RefKind::Session);
        // The link's own words win over the session's title: the author named
        // this reference in this place.
        assert_eq!(rows[0].label, "the old one");
    }

    /// A backticked path that resolves is a reference; one that does not is
    /// somebody typing. Otherwise every shell command in a log reads as broken.
    #[test]
    fn a_quoted_path_is_reported_only_when_it_resolves() {
        let probe = Fake {
            files: vec!["30-code/keeper/AGENTS.md"],
            ..Fake::default()
        };
        let rows = plan(
            &found("ran `cargo test`, read `30-code/keeper/AGENTS.md`, edited `src/nope/gone.rs`"),
            PREFIX,
            &probe,
        );
        assert_eq!(rows.len(), 1, "the command and the unresolved path are out");
        assert_eq!(rows[0].kind, RefKind::File);
        assert_eq!(rows[0].target, "30-code/keeper/AGENTS.md");
    }

    /// An external target is never probed and never missing — keeper does not
    /// go on the network to colour a row.
    #[test]
    fn an_external_url_is_external_even_though_nothing_was_checked() {
        let rows = plan(
            &found("[dead](https://example.com/gone)"),
            PREFIX,
            &Fake::default(),
        );
        assert_eq!(rows[0].kind, RefKind::External);
        assert_eq!(
            rows[0].open,
            RefTarget::External {
                url: "https://example.com/gone".to_owned()
            }
        );
    }

    /// One row per target however many times it is written, and the first
    /// source is the one a person is sent to.
    #[test]
    fn one_row_per_target_keeping_the_first_source() {
        let probe = Fake {
            files: vec!["40-media/clip.mov"],
            ..Fake::default()
        };
        let mut found = found("[a](40-media/clip.mov) and again [b](40-media/clip.mov)");
        found.push((
            "refs/inputs.md".to_owned(),
            scan("[c](40-media/clip.mov)").remove(0),
        ));
        let rows = plan(&found, PREFIX, &probe);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "README.md");
    }

    /// A session with nothing but prose has no references, and says so by
    /// being empty rather than by inventing a row.
    #[test]
    fn prose_alone_produces_nothing() {
        assert!(plan(
            &found("Just a paragraph about the work.\n"),
            PREFIX,
            &Fake::default()
        )
        .is_empty());
    }
}
