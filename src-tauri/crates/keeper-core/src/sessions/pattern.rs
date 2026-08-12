//! What a new session is shaped from (FR-253, AD-116).
//!
//! A session is born from a **pattern**: either the zone's own `_template/`
//! or a session that already exists. Those were two separate verbs — *New
//! session* copied the template, *New like this* copied a session — which is
//! two doors into one room. They are one choice here, and this module is the
//! rule that choice runs on.
//!
//! The whole point of the module is a single decision function,
//! [`apply`], that answers "what happens to each file of the source" —
//! copied, or left behind and **why**. Both the plan and the picker's preview
//! read that one answer, so what the user is promised and what lands on disk
//! cannot drift. A preview computed separately from the plan is a lie waiting
//! to happen; this is the same value rendered twice.
//!
//! Pure over `&[(path, is_dir)]`, like everything in the sessions domain — it
//! opens nothing and decides nothing about the filesystem's contents.

/// Which kind of thing a new session is shaped from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    /// The zone's `_template/` — the skeleton, copied verbatim.
    Template,
    /// A session that already exists — structure only, plus lineage.
    Session,
}

impl PatternKind {
    /// The wire spelling, for the VM and the picker.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Template => "template",
            Self::Session => "session",
        }
    }
}

/// Why one file of the source does not travel — the sentence a person would
/// give, kept here rather than in the UI so the preview quotes the rule
/// instead of paraphrasing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Promoted output belongs to the session that produced it.
    Output,
    /// Scratch dies with its session — that is what makes it scratch.
    Scratch,
    /// The README is rebuilt from headings; prose never travels.
    Record,
    /// A loose file beside the README is part of that session's record.
    Loose,
}

impl SkipReason {
    /// The reason, spelled for a human. Stable strings: the preview groups by
    /// them, and the UI renders them verbatim.
    pub fn sentence(self) -> &'static str {
        match self {
            Self::Output => "artifacts stay with the session that produced them",
            Self::Scratch => "workspace scratch dies with its session",
            Self::Record => "the README is rebuilt from its headings — prose never travels",
            Self::Loose => "loose files stay with the session they were written in",
        }
    }
}

/// What applying a pattern does, whole: what lands, and what stays behind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PatternOutcome {
    /// `(source-relative path, is_dir)` the plan creates or copies, sorted so
    /// a directory always precedes what is inside it — which is exactly what
    /// makes the compiled step order safe to execute top to bottom.
    pub copies: Vec<(String, bool)>,
    /// What the pattern deliberately leaves behind, with its reason, in path
    /// order. Directories are never listed here: a directory that survives
    /// empty is in `copies`; only its *contents* are skipped, and each is
    /// named individually so the preview can count them.
    pub skips: Vec<(String, SkipReason)>,
}

/// The four directories every session has, whether or not the source did.
const STANDARD_DIRS: [&str; 4] = ["workspace", "artifacts", "refs", "prompts"];

/// Whether a path is a placeholder rather than content — the `.gitkeep` rule
/// [`super::model::freshness`] already applies to activity, applied here to
/// counting. A placeholder is copied (the empty directory needs it) but is
/// never *shown*: "copies 1 file" for an empty `refs/` would be a lie told
/// with a true sentence.
pub fn is_placeholder(rel: &str) -> bool {
    rel.rsplit('/').next() == Some(".gitkeep")
}

/// Apply a pattern to a source file list.
///
/// **Template**: everything travels except the README, which is stamped from
/// the template's own headings rather than copied — the plan's last step
/// writes it, so copying it first would be dead work the preview would have
/// to explain.
///
/// **Session** (FR-239): `prompts/**` (reusable by design) and `refs/**`
/// (pointers worth keeping) travel; the four standard directories exist
/// empty; artifacts, workspace, the README and any loose file stay behind,
/// each named with its reason.
pub fn apply(kind: PatternKind, source: &[(String, bool)]) -> PatternOutcome {
    let mut out = PatternOutcome::default();
    if kind == PatternKind::Session {
        for dir in STANDARD_DIRS {
            out.copies.push((dir.to_owned(), true));
        }
    }
    for (rel, is_dir) in source {
        if out.copies.iter().any(|(existing, _)| existing == rel) {
            continue;
        }
        match fate(kind, rel) {
            None => out.copies.push((rel.clone(), *is_dir)),
            Some(reason) => {
                // A skipped directory is silent — what the reader cares about
                // is the files inside it, which arrive on their own lines.
                if !is_dir {
                    out.skips.push((rel.clone(), reason));
                }
            }
        }
    }
    out.copies.sort_by(|(a, _), (b, _)| a.cmp(b));
    out.skips.sort_by(|(a, _), (b, _)| a.cmp(b));
    out
}

/// The per-path decision: `None` travels, `Some(reason)` stays.
fn fate(kind: PatternKind, rel: &str) -> Option<SkipReason> {
    if rel == super::model::README {
        return Some(SkipReason::Record);
    }
    if kind == PatternKind::Template {
        return None;
    }
    if in_dir(rel, "prompts") || in_dir(rel, "refs") {
        return None;
    }
    if in_dir(rel, "artifacts") {
        return Some(SkipReason::Output);
    }
    if in_dir(rel, "workspace") {
        return Some(SkipReason::Scratch);
    }
    Some(SkipReason::Loose)
}

/// Whether `rel` is the directory `dir` or something inside it.
fn in_dir(rel: &str, dir: &str) -> bool {
    rel == dir
        || rel
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[(&str, bool)]) -> Vec<(String, bool)> {
        paths
            .iter()
            .map(|(rel, dir)| ((*rel).to_owned(), *dir))
            .collect()
    }

    const SESSION: &[(&str, bool)] = &[
        ("README.md", false),
        ("notes-to-self.md", false),
        ("prompts", true),
        ("prompts/01-scope.md", false),
        ("refs", true),
        ("refs/.gitkeep", false),
        ("refs/pointer.md", false),
        ("artifacts", true),
        ("artifacts/final-report.md", false),
        ("workspace", true),
        ("workspace/scratch.csv", false),
    ];

    /// The session pattern is structure-only (FR-239) — and every file it
    /// refuses is named with the reason, because a preview that shows only
    /// what travels cannot answer "where did my report go".
    #[test]
    fn a_session_pattern_takes_prompts_and_refs_and_says_why_the_rest_stays() {
        let out = apply(PatternKind::Session, &files(SESSION));
        let copied: Vec<&str> = out.copies.iter().map(|(rel, _)| rel.as_str()).collect();
        assert!(copied.contains(&"prompts/01-scope.md"));
        assert!(copied.contains(&"refs/pointer.md"));
        for dir in STANDARD_DIRS {
            assert!(copied.contains(&dir), "{dir} exists in every new session");
        }
        assert!(!copied.contains(&"artifacts/final-report.md"));
        assert!(!copied.contains(&"workspace/scratch.csv"));
        assert!(!copied.contains(&"README.md"));

        let reason = |path: &str| {
            out.skips
                .iter()
                .find(|(rel, _)| rel == path)
                .map(|(_, reason)| *reason)
        };
        assert_eq!(
            reason("artifacts/final-report.md"),
            Some(SkipReason::Output)
        );
        assert_eq!(reason("workspace/scratch.csv"), Some(SkipReason::Scratch));
        assert_eq!(reason("README.md"), Some(SkipReason::Record));
        assert_eq!(reason("notes-to-self.md"), Some(SkipReason::Loose));
        // A directory is never its own skip line — its contents are.
        assert!(!out.skips.iter().any(|(rel, _)| rel == "artifacts"));
    }

    /// The template travels whole; only the README stays, and it stays for a
    /// reason the preview can state rather than for silence.
    #[test]
    fn the_template_pattern_copies_everything_but_the_rebuilt_readme() {
        let out = apply(
            PatternKind::Template,
            &files(&[
                ("README.md", false),
                ("refs", true),
                ("refs/.gitkeep", false),
                ("house-style.md", false),
            ]),
        );
        let copied: Vec<&str> = out.copies.iter().map(|(rel, _)| rel.as_str()).collect();
        assert_eq!(copied, vec!["house-style.md", "refs", "refs/.gitkeep"]);
        assert_eq!(
            out.skips,
            vec![("README.md".to_owned(), SkipReason::Record)]
        );
    }

    /// Directory-before-contents is not cosmetic: the compiled plan runs the
    /// list top to bottom, so a copy into `prompts/` that sorted before
    /// `prompts` itself would land in a directory that does not exist yet.
    #[test]
    fn copies_sort_a_directory_before_everything_inside_it() {
        let out = apply(
            PatternKind::Session,
            &files(&[
                ("prompts/02-later.md", false),
                ("prompts", true),
                ("prompts/01-first.md", false),
                ("refs/deep", true),
                ("refs/deep/x.md", false),
            ]),
        );
        let copied: Vec<&str> = out.copies.iter().map(|(rel, _)| rel.as_str()).collect();
        for (index, (rel, is_dir)) in out.copies.iter().enumerate() {
            if *is_dir {
                continue;
            }
            if let Some((parent, _)) = rel.rsplit_once('/') {
                let parent_at = copied.iter().position(|seen| *seen == parent);
                assert!(
                    parent_at.is_some_and(|at| at < index),
                    "{parent} must precede {rel}"
                );
            }
        }
    }

    /// A placeholder is copied but never counted — an empty `refs/` that
    /// advertised "1 file" would be true about bytes and false about meaning.
    #[test]
    fn placeholders_travel_but_do_not_count_as_content() {
        assert!(is_placeholder("refs/.gitkeep"));
        assert!(!is_placeholder("refs/pointer.md"));
        let out = apply(PatternKind::Session, &files(SESSION));
        assert!(
            out.copies.iter().any(|(rel, _)| rel == "refs/.gitkeep"),
            "the placeholder still lands, or the empty directory would not"
        );
        let shown = out
            .copies
            .iter()
            .filter(|(rel, is_dir)| !*is_dir && !is_placeholder(rel))
            .count();
        assert_eq!(shown, 2, "01-scope.md and pointer.md, and nothing else");
    }
}
