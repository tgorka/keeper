//! Adding a reference to a session (FR-265, AD-118).
//!
//! [`super::refs`] reads the pointers a session already wrote and says which of
//! them broke. This is the other direction: the operator picks a note, a file
//! or a recording, and keeper writes the pointer. The two share a vocabulary on
//! purpose — what this module writes is exactly what that one reads back, and a
//! write path whose output its own reader cannot classify would be a feature
//! that looks like it works until somebody reopens the session.
//!
//! ## A reference is a line, not a file
//!
//! The flat contract makes a session a pool of markdown, so the tempting shape
//! is one file per reference. It is the wrong one: a working session points at
//! twenty things, and twenty one-line files would bury the four files somebody
//! actually wrote. A reference is a **bullet in a file tagged `ref`** — which
//! is what the zone already did with `refs/inputs.md`, minus the folder.
//!
//! So this module appends. The target file is one the operator chose from the
//! pool, or one keeper creates tagged `ref` when the session has none yet, and
//! the append is a [`PlanStep::GuardedWrite`] because an agent may be writing
//! that same file this second (`migrate`'s reason, verbatim).
//!
//! ## Three syntaxes, each because a reader exists
//!
//! - a note → `[[Title]]`, resolved by the vault index, which is what makes the
//!   reference survive the note being renamed;
//! - a path → `[label](target)`, session-relative when the target is inside the
//!   session and profile-relative when it is not — the two orders
//!   [`super::refs::candidates`] probes, in that order;
//! - a URL → written bare, because [`super::refs::scan`] finds a bare URL and
//!   because wrapping it in a link would invent a label the operator did not
//!   type.
//!
//! Nothing is written that the scanner drops. A backticked path is *findable
//! only* by that module's second asymmetry, so keeper never writes one: a
//! reference keeper added and then declines to call missing would be the worst
//! of both rules.
//!
//! ## `workspace/` is offered a promotion, never given one silently
//!
//! `workspace/` is scratch. The archive verb empties it
//! ([`PlanStep::EmptyDirKeep`] exists for precisely that), so a pointer into it
//! is a dangling link with a date on it. When the pick is inside this session's
//! workspace, [`promotion`] proposes a copy into `artifacts/` and the link then
//! names the copy.
//!
//! It is an offer and not a rule because the operator may mean the scratch file
//! — a reference to `workspace/node_modules/.bin/vite` in a log about a build
//! is a true statement about a thing that was there — and because copying bytes
//! somebody did not ask to copy is how `artifacts/` fills up with a hundred
//! megabytes of `target/`.
//!
//! Pure, like everything else in [`super`]: a pick and some listings in, a plan
//! out. Nothing here touches disk and nothing here has a clock (AD-108).

use std::collections::BTreeSet;

use crate::sessions::files::{self, NewFileKind};
use crate::sessions::plan::{Plan, PlanStep};
use crate::sessions::refs::RefKind;
use crate::sessions::shape::KindTag;

/// The scratch folder a reference should not point into. `files.rs`'s constant
/// spelled again rather than shared, because that one is a *write* fence and
/// this is a *pointer* rule: they agree today and are allowed to stop agreeing.
const WORKSPACE: &str = "workspace";

/// Where a promoted copy lands — the zone's own word for "kept output".
const ARTIFACTS: &str = "artifacts";

/// The file keeper creates when a session has nowhere to put a reference yet.
///
/// Plural, unlike a [`KindTag`], for the reason the spaces are plural: it
/// collects them. And named rather than stamped, because a references file is
/// not a sitting — a session grows one of these and appends to it for weeks.
pub const DEFAULT_REF_FILE: &str = "references.md";

/// What the operator picked out of the picker.
///
/// Three variants rather than the six [`RefKind`]s has: `session` and `missing`
/// are things a *reader* discovers, not things a person picks. You cannot pick
/// a missing file, and a session folder is picked as the path it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// A note in the profile's vault, addressed the way a wikilink addresses
    /// one. `recording` is the vault's own flag, not a second predicate here.
    Note {
        title: String,
        recording: bool,
    },
    /// Something on disk, profile-relative and `/`-joined — the scope
    /// [`super::refs::RefProbe::exists`] answers in.
    Path {
        subpath: String,
    },
    Url {
        url: String,
    },
}

/// Whether one candidate survives what the operator typed into the picker.
///
/// Not [`crate::notes::query`]. That language selects *notes in a vault* and
/// answers with an index; this filters a list of three different things — notes,
/// recordings and files — that is already in hand, and running a query engine
/// over a `Vec` in order to filter it would be the second evaluator AD-73
/// exists to prevent going the other way round.
///
/// What it does support is the one operator-facing term the request named:
/// `tag:x` matches a tag (and any tag *under* it, so `tag:project` finds
/// `project/keeper` — the hierarchy the tag index already gives for free).
/// Everything else is a plain word matched case-insensitively against the label,
/// the detail line and the tags. Words are ANDed, because narrowing is what a
/// second word is for.
#[must_use]
pub fn matches(query: &str, label: &str, detail: &str, tags: &[String]) -> bool {
    query.split_whitespace().all(|word| {
        let word = word.to_lowercase();
        match word.strip_prefix("tag:") {
            Some(wanted) if !wanted.is_empty() => tags.iter().any(|tag| {
                let tag = tag.to_lowercase();
                tag == wanted || tag.starts_with(&format!("{wanted}/"))
            }),
            // A bare `tag:` is somebody mid-type. Matching everything is the
            // right answer to a term that has not been finished yet — the list
            // should not empty out between the colon and the first letter.
            Some(_) => true,
            None => {
                label.to_lowercase().contains(&word)
                    || detail.to_lowercase().contains(&word)
                    || tags.iter().any(|tag| tag.to_lowercase().contains(&word))
            }
        }
    })
}

/// A copy keeper offers to make before writing the pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Promotion {
    /// Zone-relative source, inside the session's `workspace/`.
    pub from: String,
    /// Zone-relative destination, inside the session's `artifacts/`.
    pub to: String,
    /// Session-relative destination — what the written link says.
    pub rel: String,
}

/// Why a pick was refused. Each is a sentence, for [`Self::message`]'s reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddRefError {
    /// A pick with nothing in it — an empty path, an empty title, a bare
    /// scheme.
    Empty,
    /// A path that climbs out of the profile, or an absolute one. Refused
    /// rather than normalised: a reference keeper cannot resolve is a reference
    /// keeper should not write.
    Outside { subpath: String },
    /// The reference would be written into `workspace/`, which keeper does not
    /// write to at all (AD-113).
    IntoWorkspace { rel: String },
    /// The target file is not markdown. A pointer lives in prose.
    NotMarkdown { rel: String },
    /// A pick naming a kind this module does not have. Only reachable from a
    /// caller that composed the word itself, which is why it names the word
    /// back rather than silently choosing a default.
    UnknownKind { kind: String },
}

impl AddRefError {
    /// The sentence the operator reads. Written here rather than in React for
    /// UX-DR43's reason — keeper knows which term it refused, and a renderer
    /// composing its own version would be the second voice saying the same
    /// failure differently.
    pub fn message(&self) -> String {
        match self {
            AddRefError::Empty => {
                "There is nothing to reference — pick a note, a file or a link first.".to_owned()
            }
            AddRefError::Outside { subpath } => format!(
                "{subpath} is outside the synced folder, so keeper cannot write a reference that \
                 would still resolve on another machine."
            ),
            AddRefError::IntoWorkspace { rel } => format!(
                "{rel} is in workspace/, which keeper never writes to — it is scratch and is \
                 emptied when the session is archived."
            ),
            AddRefError::NotMarkdown { rel } => {
                format!("{rel} is not markdown, and a reference is a line of prose.")
            }
            AddRefError::UnknownKind { kind } => {
                format!("keeper does not know how to reference a {kind}.")
            }
        }
    }
}

impl Pick {
    /// The pick one wire word and one target describe.
    ///
    /// The words are [`RefKind::as_str`]'s, so a picker row's `kind` comes back
    /// unchanged and neither side translates. `session` and `missing` are
    /// refused rather than mapped: a session folder arrives as the path it is,
    /// and a reference to something that is already missing is not a thing to
    /// add.
    ///
    /// # Errors
    /// [`AddRefError::UnknownKind`] for a word outside the set,
    /// [`AddRefError::Empty`] for a target with nothing in it.
    pub fn parse(kind: &str, target: &str) -> Result<Pick, AddRefError> {
        let target = target.trim();
        if target.is_empty() {
            return Err(AddRefError::Empty);
        }
        match kind {
            "note" => Ok(Pick::Note {
                title: target.to_owned(),
                recording: false,
            }),
            "recording" => Ok(Pick::Note {
                title: target.to_owned(),
                recording: true,
            }),
            "file" => Ok(Pick::Path {
                subpath: target.to_owned(),
            }),
            "external" | "url" => Ok(Pick::Url {
                url: target.to_owned(),
            }),
            other => Err(AddRefError::UnknownKind {
                kind: other.to_owned(),
            }),
        }
    }
}

/// What a pick will be once written, in [`super::refs`]' own vocabulary.
///
/// Asked here so the picker can show the row the way the references list will
/// show it — the same icon and the same word, decided once (AD-73).
#[must_use]
pub fn kind_of(pick: &Pick) -> RefKind {
    match pick {
        Pick::Note {
            recording: true, ..
        } => RefKind::Recording,
        Pick::Note { .. } => RefKind::Note,
        Pick::Path { .. } => RefKind::File,
        Pick::Url { .. } => RefKind::External,
    }
}

/// The session's own folder, profile-relative — `refs::plan`'s `prefix`.
#[must_use]
fn prefix(zone: &str, session: &str) -> String {
    if zone.is_empty() {
        session.to_owned()
    } else {
        format!("{zone}/{session}")
    }
}

/// The promotion this pick deserves, or `None` when it needs none.
///
/// `taken` is the artifact folder's current basenames, so a second promotion of
/// a file called `notes.md` becomes `notes-2.md` rather than overwriting the
/// first — [`files::new_named`]'s rule, and case-insensitive for its reason.
///
/// Only the *caller's own* session's workspace counts. Another session's
/// workspace is somebody else's scratch and copying out of it would be keeper
/// reaching into a folder the operator was not looking at.
#[must_use]
pub fn promotion(
    zone: &str,
    session: &str,
    subpath: &str,
    taken: &BTreeSet<String>,
) -> Option<Promotion> {
    let inside = subpath.strip_prefix(&format!("{}/{WORKSPACE}/", prefix(zone, session)))?;
    let base = inside.rsplit('/').next().unwrap_or(inside);
    if base.is_empty() {
        return None;
    }
    let (stem, ext) = match base.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (base, String::new()),
    };
    let mut name = format!("{stem}{ext}");
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
        name = format!("{stem}-{n}{ext}");
        n += 1;
    }
    let rel = format!("{ARTIFACTS}/{name}");
    Some(Promotion {
        from: format!("{session}/{WORKSPACE}/{inside}"),
        to: format!("{session}/{rel}"),
        rel,
    })
}

/// What the written link points at: session-relative inside the session,
/// profile-relative outside it.
///
/// The order is [`super::refs::candidates`]' order, which is what makes
/// `artifacts/report.md` in a session mean the file beside it. Writing the long
/// form for a file that is *in* the session would still resolve, and would make
/// every reference in a session unreadable to a person moving the folder.
#[must_use]
fn link_path(zone: &str, session: &str, subpath: &str) -> String {
    subpath
        .strip_prefix(&format!("{}/", prefix(zone, session)))
        .unwrap_or(subpath)
        .to_owned()
}

/// Whether a profile-relative path is one keeper is willing to point at.
fn check_subpath(subpath: &str) -> Result<(), AddRefError> {
    let trimmed = subpath.trim();
    if trimmed.is_empty() {
        return Err(AddRefError::Empty);
    }
    if trimmed.starts_with('/') || trimmed.split('/').any(|part| part == "..") {
        return Err(AddRefError::Outside {
            subpath: trimmed.to_owned(),
        });
    }
    Ok(())
}

/// The bullet one pick becomes.
///
/// `label` is the operator's own words when they typed any, and nothing when
/// they did not — an alias keeper invented would be keeper naming somebody
/// else's reference. A note with no label is a bare `[[Title]]`, which is how
/// every other wikilink in the vault is written.
///
/// # Errors
/// [`AddRefError::Empty`] for a pick with no content, [`AddRefError::Outside`]
/// for a path keeper will not point at.
pub fn line(
    zone: &str,
    session: &str,
    pick: &Pick,
    label: Option<&str>,
    promoted: Option<&Promotion>,
) -> Result<String, AddRefError> {
    let label = label.map(str::trim).filter(|text| !text.is_empty());
    match pick {
        Pick::Note { title, .. } => {
            let title = title.trim();
            if title.is_empty() {
                return Err(AddRefError::Empty);
            }
            Ok(match label {
                // `[[Target|alias]]`, which `refs::pick_label` reads as the
                // author naming this reference in this place.
                Some(alias) => format!("- [[{title}|{alias}]]"),
                None => format!("- [[{title}]]"),
            })
        }
        Pick::Path { subpath } => {
            check_subpath(subpath)?;
            let target = match promoted {
                Some(promotion) => promotion.rel.clone(),
                None => link_path(zone, session, subpath.trim()),
            };
            let text = label.unwrap_or_else(|| {
                target
                    .rsplit('/')
                    .next()
                    .filter(|base| !base.is_empty())
                    .unwrap_or(&target)
            });
            // A destination with a space needs the angle-bracket form, which
            // `links::markdown_link` strips back off — without it the link ends
            // at the space and the reference points at half a filename.
            let dest = if target.contains(' ') {
                format!("<{target}>")
            } else {
                target.clone()
            };
            Ok(format!("- [{text}]({dest})"))
        }
        Pick::Url { url } => {
            let url = url.trim();
            if url.is_empty() || url == "http://" || url == "https://" {
                return Err(AddRefError::Empty);
            }
            Ok(match label {
                Some(text) => format!("- [{text}]({url})"),
                // Bare, because `refs::external_urls` finds one and a label
                // keeper made up is a label keeper made up.
                None => format!("- {url}"),
            })
        }
    }
}

/// The bytes a reference file has after one line is appended.
///
/// Appended at the end rather than spliced under a heading: a references file
/// is a list, and finding "the right section" would mean a second parser of
/// somebody else's structure — the thing AD-20 exists to prevent. A file that
/// wants sections gets them by the operator moving the line, which is one
/// keystroke and does not require keeper to guess.
///
/// The separating newline is added only when the file does not end in one, so a
/// well-formed file is byte-identical except for the line that was added
/// (FR-121's spirit, one directory down).
#[must_use]
pub fn appended(existing: &str, line: &str) -> String {
    if existing.is_empty() {
        return format!("{line}\n");
    }
    if existing.ends_with('\n') {
        format!("{existing}{line}\n")
    } else {
        format!("{existing}\n{line}\n")
    }
}

/// The bytes a brand-new references file starts with — [`files::render_new`]'s
/// output, tagged `ref` so the zone's References space lists it immediately,
/// plus the first line.
#[must_use]
pub fn seeded(title: &str, id: &str, now: &str, line: &str) -> String {
    let head = files::render_new(NewFileKind::Markdown, Some(KindTag::Ref), title, id, now);
    appended(&head, line)
}

/// The plan that adds one reference.
///
/// `session` is zone-relative (`active/2026-08-14-keeper`), `rel` is
/// session-relative, and `existing` is the target file's current bytes — `None`
/// when keeper is creating it. The promotion's copy comes **first**: a plan
/// whose link is written before the bytes it names exist would, if the copy
/// then failed, leave a session pointing at a file keeper had promised to make.
/// The write is last, and it is the only step that is hard to take back
/// (AD-111).
///
/// A [`PlanStep::GuardedWrite`] rather than a plain write, guarded on the
/// length the caller read: an agent appending to the same references file turns
/// into a refusal the operator can retry, rather than a lost line.
///
/// # Errors
/// [`AddRefError::IntoWorkspace`] for a target inside scratch,
/// [`AddRefError::NotMarkdown`] for a target that is not `.md`, and whatever
/// [`line`] refused.
pub fn compile_add(
    session: &str,
    rel: &str,
    existing: Option<&str>,
    line: &str,
    promotion: Option<&Promotion>,
) -> Result<Plan, AddRefError> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err(AddRefError::Empty);
    }
    if rel == WORKSPACE || rel.starts_with(&format!("{WORKSPACE}/")) {
        return Err(AddRefError::IntoWorkspace {
            rel: rel.to_owned(),
        });
    }
    if !rel.to_ascii_lowercase().ends_with(".md") {
        return Err(AddRefError::NotMarkdown {
            rel: rel.to_owned(),
        });
    }

    let mut steps = Vec::new();
    if let Some(promotion) = promotion {
        steps.push(PlanStep::MkDir {
            path: format!("{session}/{ARTIFACTS}"),
        });
        steps.push(PlanStep::CopyFile {
            from: promotion.from.clone(),
            to: promotion.to.clone(),
        });
    }

    let path = format!("{session}/{rel}");
    match existing {
        Some(text) => steps.push(PlanStep::GuardedWrite {
            path,
            expect_len: text.len(),
            content: appended(text, line),
        }),
        // Nothing to guard against on a file that does not exist yet — the same
        // argument `files::compile_new` makes for its plain write.
        None => steps.push(PlanStep::WriteFile {
            path,
            content: line.to_owned(),
        }),
    }

    Ok(Plan {
        verb: "ref-add".to_owned(),
        session: session.to_owned(),
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_recording_is_a_recording_because_the_vault_said_so() {
        // Not an extension: `refs.rs` spends ten lines on why, and a second
        // predicate here would be the drift it warns about.
        assert_eq!(
            kind_of(&Pick::Note {
                title: "Standup".to_owned(),
                recording: true,
            }),
            RefKind::Recording
        );
        assert_eq!(
            kind_of(&Pick::Note {
                title: "Standup".to_owned(),
                recording: false,
            }),
            RefKind::Note
        );
    }

    #[test]
    fn a_tag_term_reaches_the_tags_below_it() {
        let tags = vec!["project/keeper".to_owned(), "meeting".to_owned()];
        assert!(matches("tag:project", "Standup", "", &tags));
        assert!(matches("tag:project/keeper", "Standup", "", &tags));
        assert!(!matches("tag:projector", "Standup", "", &tags));
    }

    #[test]
    fn a_second_word_narrows_rather_than_widens() {
        let tags = vec!["meeting".to_owned()];
        assert!(matches("stand meeting", "Standup", "notes/2026", &tags));
        assert!(!matches("stand retro", "Standup", "notes/2026", &tags));
    }

    #[test]
    fn a_word_matches_the_path_as_well_as_the_name() {
        // The detail line is where a file's folder lives, and "the one in
        // artifacts" is how a person describes a file they can see.
        assert!(matches(
            "artifacts",
            "report.md",
            "artifacts/report.md",
            &[]
        ));
    }

    #[test]
    fn a_half_typed_tag_term_does_not_empty_the_list() {
        assert!(matches("tag:", "Standup", "", &["meeting".to_owned()]));
    }

    #[test]
    fn the_wire_words_are_the_reader_s_own_words() {
        // Round-trip: what `refs::RefKind::as_str` prints, `Pick::parse` reads.
        assert_eq!(
            Pick::parse("recording", "Standup"),
            Ok(Pick::Note {
                title: "Standup".to_owned(),
                recording: true,
            })
        );
        assert_eq!(
            kind_of(&Pick::parse("file", "a/b.md").expect("file parses")),
            RefKind::File
        );
        assert_eq!(
            kind_of(&Pick::parse("external", "https://x.dev").expect("url parses")),
            RefKind::External
        );
    }

    #[test]
    fn a_kind_that_is_not_pickable_is_refused_by_name() {
        // `session` and `missing` are answers a reader gives, not things a
        // person picks — and the refusal says which word it was handed.
        let refused = Pick::parse("session", "active/other").expect_err("a session is not a pick");
        assert!(refused.message().contains("session"));
        assert!(matches!(
            Pick::parse("missing", "gone.md"),
            Err(AddRefError::UnknownKind { .. })
        ));
    }

    #[test]
    fn a_note_is_written_as_a_wikilink_the_index_can_follow() {
        let pick = Pick::Note {
            title: "Standup".to_owned(),
            recording: false,
        };
        let written = line("60-sessions", "active/s", &pick, None, None)
            .expect("a titled note is referenceable");
        assert_eq!(written, "- [[Standup]]");
    }

    #[test]
    fn an_operators_own_words_become_the_alias_and_not_the_target() {
        let pick = Pick::Note {
            title: "Standup".to_owned(),
            recording: false,
        };
        let written = line(
            "60-sessions",
            "active/s",
            &pick,
            Some("yesterday's call"),
            None,
        )
        .expect("a titled note is referenceable");
        assert_eq!(written, "- [[Standup|yesterday's call]]");
    }

    #[test]
    fn a_file_inside_the_session_is_written_session_relative() {
        let pick = Pick::Path {
            subpath: "60-sessions/active/s/artifacts/report.md".to_owned(),
        };
        let written =
            line("60-sessions", "active/s", &pick, None, None).expect("an inside path resolves");
        assert_eq!(written, "- [report.md](artifacts/report.md)");
    }

    #[test]
    fn a_file_elsewhere_in_the_drive_keeps_its_profile_relative_path() {
        let pick = Pick::Path {
            subpath: "40-media/standup.m4a".to_owned(),
        };
        let written =
            line("60-sessions", "active/s", &pick, None, None).expect("an outside path resolves");
        assert_eq!(written, "- [standup.m4a](40-media/standup.m4a)");
    }

    #[test]
    fn a_destination_with_a_space_is_wrapped_so_the_link_does_not_end_early() {
        let pick = Pick::Path {
            subpath: "40-media/team standup.m4a".to_owned(),
        };
        let written = line("60-sessions", "active/s", &pick, None, None)
            .expect("a spaced path is still a path");
        assert_eq!(written, "- [team standup.m4a](<40-media/team standup.m4a>)");
    }

    #[test]
    fn a_url_is_written_bare_and_a_labelled_one_as_a_link() {
        let bare = line(
            "60-sessions",
            "active/s",
            &Pick::Url {
                url: "https://example.com/spec".to_owned(),
            },
            None,
            None,
        )
        .expect("a url is referenceable");
        assert_eq!(bare, "- https://example.com/spec");

        let labelled = line(
            "60-sessions",
            "active/s",
            &Pick::Url {
                url: "https://example.com/spec".to_owned(),
            },
            Some("the spec"),
            None,
        )
        .expect("a url is referenceable");
        assert_eq!(labelled, "- [the spec](https://example.com/spec)");
    }

    #[test]
    fn a_scheme_with_nothing_after_it_is_refused() {
        assert_eq!(
            line(
                "60-sessions",
                "active/s",
                &Pick::Url {
                    url: "https://".to_owned(),
                },
                None,
                None,
            ),
            Err(AddRefError::Empty)
        );
    }

    #[test]
    fn a_path_that_climbs_out_of_the_profile_is_refused_not_normalised() {
        let pick = Pick::Path {
            subpath: "../secrets/keys.txt".to_owned(),
        };
        let refused = line("60-sessions", "active/s", &pick, None, None)
            .expect_err("a climbing path is not referenceable");
        assert!(refused.message().contains("outside the synced folder"));
    }

    #[test]
    fn an_absolute_path_is_refused_for_the_same_reason() {
        let pick = Pick::Path {
            subpath: "/Volumes/merope/tgdrive/40-media/x.m4a".to_owned(),
        };
        assert!(matches!(
            line("60-sessions", "active/s", &pick, None, None),
            Err(AddRefError::Outside { .. })
        ));
    }

    #[test]
    fn a_workspace_file_is_offered_a_copy_into_artifacts() {
        let found = promotion(
            "60-sessions",
            "active/s",
            "60-sessions/active/s/workspace/out/report.md",
            &taken(&[]),
        )
        .expect("a workspace file is promotable");
        assert_eq!(
            found,
            Promotion {
                from: "active/s/workspace/out/report.md".to_owned(),
                to: "active/s/artifacts/report.md".to_owned(),
                rel: "artifacts/report.md".to_owned(),
            }
        );
    }

    #[test]
    fn a_second_promotion_of_the_same_name_does_not_overwrite_the_first() {
        let found = promotion(
            "60-sessions",
            "active/s",
            "60-sessions/active/s/workspace/report.md",
            &taken(&["Report.md"]),
        )
        .expect("a workspace file is promotable");
        // Case-folded, because APFS and NTFS fold case and the operator is
        // looking at one file, not two.
        assert_eq!(found.rel, "artifacts/report-2.md");
    }

    #[test]
    fn a_file_outside_the_workspace_is_offered_nothing() {
        assert_eq!(
            promotion(
                "60-sessions",
                "active/s",
                "60-sessions/active/s/artifacts/report.md",
                &taken(&[]),
            ),
            None
        );
        // Another session's scratch is somebody else's scratch.
        assert_eq!(
            promotion(
                "60-sessions",
                "active/s",
                "60-sessions/active/other/workspace/report.md",
                &taken(&[]),
            ),
            None
        );
    }

    #[test]
    fn a_promoted_pick_links_the_copy_and_not_the_scratch_file() {
        let promoted = promotion(
            "60-sessions",
            "active/s",
            "60-sessions/active/s/workspace/report.md",
            &taken(&[]),
        )
        .expect("a workspace file is promotable");
        let pick = Pick::Path {
            subpath: "60-sessions/active/s/workspace/report.md".to_owned(),
        };
        let written = line("60-sessions", "active/s", &pick, None, Some(&promoted))
            .expect("a promoted path is referenceable");
        assert_eq!(written, "- [report.md](artifacts/report.md)");
    }

    #[test]
    fn appending_leaves_every_other_byte_alone() {
        assert_eq!(
            appended("# Refs\n\n- one\n", "- two"),
            "# Refs\n\n- one\n- two\n"
        );
        // A file somebody left without a trailing newline gets one rather than
        // a line glued onto its last word.
        assert_eq!(appended("- one", "- two"), "- one\n- two\n");
        assert_eq!(appended("", "- two"), "- two\n");
    }

    #[test]
    fn the_copy_runs_before_the_link_that_names_it() {
        let promoted = Promotion {
            from: "active/s/workspace/report.md".to_owned(),
            to: "active/s/artifacts/report.md".to_owned(),
            rel: "artifacts/report.md".to_owned(),
        };
        let plan = compile_add(
            "active/s",
            "references.md",
            Some("- one\n"),
            "- [report.md](artifacts/report.md)",
            Some(&promoted),
        )
        .expect("a markdown target accepts a reference");

        assert_eq!(plan.verb, "ref-add");
        assert_eq!(
            plan.steps,
            vec![
                PlanStep::MkDir {
                    path: "active/s/artifacts".to_owned(),
                },
                PlanStep::CopyFile {
                    from: "active/s/workspace/report.md".to_owned(),
                    to: "active/s/artifacts/report.md".to_owned(),
                },
                PlanStep::GuardedWrite {
                    path: "active/s/references.md".to_owned(),
                    expect_len: "- one\n".len(),
                    content: "- one\n- [report.md](artifacts/report.md)\n".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_file_that_does_not_exist_yet_is_written_plainly() {
        let plan = compile_add("active/s", "references.md", None, "- [[Standup]]", None)
            .expect("a markdown target accepts a reference");
        assert_eq!(
            plan.steps,
            vec![PlanStep::WriteFile {
                path: "active/s/references.md".to_owned(),
                content: "- [[Standup]]".to_owned(),
            }]
        );
    }

    #[test]
    fn a_reference_is_never_written_into_scratch() {
        assert_eq!(
            compile_add("active/s", "workspace/notes.md", None, "- x", None),
            Err(AddRefError::IntoWorkspace {
                rel: "workspace/notes.md".to_owned(),
            })
        );
    }

    #[test]
    fn a_reference_is_never_written_into_something_that_is_not_prose() {
        let refused = compile_add("active/s", "artifacts/data.csv", None, "- x", None)
            .expect_err("a csv is not a references file");
        assert!(refused.message().contains("not markdown"));
    }

    #[test]
    fn a_seeded_file_is_tagged_so_the_references_space_finds_it_at_once() {
        let text = seeded(
            "References",
            "01J5AAAAAAAAAAAAAAAAAAAAAA",
            "2026-08-14T10:00:00Z",
            "- [[Standup]]",
        );
        assert!(text.contains("tags:"));
        assert!(text.contains("- ref"));
        assert!(text.ends_with("- [[Standup]]\n"));
    }
}
