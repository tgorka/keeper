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

/// The pattern id the zone's own `_template/` answers to.
///
/// The directory name doubles as the id on purpose: a template pattern's id
/// **is** the zone-relative directory it copies out of, which is what lets one
/// named template be addressed without a second registry to keep in step with
/// the filesystem (AD-110).
pub const TEMPLATE_ID: &str = super::model::TEMPLATE_DIR;

/// What the zone's own skeleton calls itself in the picker, and why it is
/// there. Here rather than in the shell because the picker renders the
/// domain's sentences and composes none of its own (AD-7, AD-108).
pub const TEMPLATE_LABEL: &str = "Zone template";
pub const TEMPLATE_DETAIL: &str = "the zone's own skeleton — copied whole";

/// What a named template says about itself. A named template's *label* is its
/// folder name — the operator named it, and keeper does not improve on that.
pub const NAMED_TEMPLATE_DETAIL: &str = "a named template — copied whole";

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

/// The four directories a **folder-shaped** session has, whether or not the
/// source did.
const STANDARD_DIRS: [&str; 4] = ["workspace", "artifacts", "refs", "prompts"];

/// The two a **flat** one has (FR-268).
///
/// `refs/` and `prompts/` are gone because in the flat contract they are tag
/// queries, not places — creating them empty would put two directories in a new
/// session that `AGENTS.md` says not to create, which is a contradiction the
/// operator would meet on their first session and reasonably read as a bug.
/// `artifacts/` and `workspace/` stay: their difference is about versioning,
/// not about kind, so no tag can replace them.
const FLAT_DIRS: [&str; 2] = ["workspace", "artifacts"];

/// The directories a new session of this shape starts with.
pub fn standard_dirs(shape: super::shape::Shape) -> &'static [&'static str] {
    match shape {
        super::shape::Shape::Flat => &FLAT_DIRS,
        super::shape::Shape::Folder => &STANDARD_DIRS,
    }
}

/// Whether a path is a placeholder rather than content — the `.gitkeep` rule
/// [`super::model::freshness`] already applies to activity, applied here to
/// counting. A placeholder is copied (the empty directory needs it) but is
/// never *shown*: "copies 1 file" for an empty `refs/` would be a lie told
/// with a true sentence.
pub fn is_placeholder(rel: &str) -> bool {
    rel.rsplit('/').next() == Some(".gitkeep")
}

/// What a pattern id names, resolved once (FR-266).
///
/// The shell used to decide this with `pattern_id.filter(|v| v != "_template")`
/// — everything that is not the zone template is a session. That held exactly
/// as long as `_template` was the only template there could be: a named
/// template's id slips past an equality test and is then looked up in the
/// session index, where it is not, so `_template/house` failed with *no such
/// session: \_template/house*. The test is a prefix test now, and it lives here
/// so there is one answer to "what is this id" rather than one per call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternSource {
    /// A template — `root` is the zone-relative directory to copy out of,
    /// either `_template` or `_template/<name>`.
    Template { root: String },
    /// A session that already exists, by id.
    Session { id: String },
}

impl PatternSource {
    /// Which rule decides what travels out of this source.
    pub fn kind(&self) -> PatternKind {
        match self {
            Self::Template { .. } => PatternKind::Template,
            Self::Session { .. } => PatternKind::Session,
        }
    }
}

/// The id a named template answers to. One spelling, composed here, so the
/// picker's row and the create that follows it cannot disagree (AD-65).
pub fn named_template_id(name: &str) -> String {
    format!("{TEMPLATE_ID}/{name}")
}

/// Resolve a pattern id to what it names — `None` when it is a `_template/…`
/// spelling keeper will not accept.
///
/// `None` and `"_template"` are both the zone's own skeleton: the argument is
/// optional on the wire and absent means default, which is the same thing said
/// twice rather than two things.
///
/// A refusal is a refusal and not a fallback: an id keeper cannot resolve must
/// not quietly create a session from the zone template, because "it made
/// something, just not what you asked for" is the failure nobody notices until
/// the wrong skeleton is three sessions deep.
pub fn resolve(pattern_id: Option<&str>) -> Option<PatternSource> {
    let template = || PatternSource::Template {
        root: TEMPLATE_ID.to_owned(),
    };
    let Some(id) = pattern_id else {
        return Some(template());
    };
    if id == TEMPLATE_ID {
        return Some(template());
    }
    match id
        .strip_prefix(TEMPLATE_ID)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        // Under `_template/`, so it can only be a template — a bad name is
        // refused rather than reinterpreted as a session id.
        Some(name) => safe_segment(name).then(|| PatternSource::Template {
            root: id.to_owned(),
        }),
        None => Some(PatternSource::Session { id: id.to_owned() }),
    }
}

/// One path segment keeper will join onto a zone root.
///
/// The traversal cases (`.`, `..`, an embedded separator) are the reason this
/// exists at all: a pattern id arrives from the frontend, and `_template/../..`
/// joined onto the zone would read a directory outside it. Underscored and
/// dotted names are refused too — [`super::model::skipped`] hides them from
/// every walk, so a template named `_house` would be a row in the picker that
/// nothing else in the zone can see.
fn safe_segment(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && name != "."
        && name != ".."
        && !super::model::skipped(name)
}

/// Whether a `_template/` entry is worth reading as a named template at all —
/// the cheap half of the test, applied before anything is opened.
pub fn could_be_named_template(name: &str, is_dir: bool) -> bool {
    is_dir && safe_segment(name)
}

/// Whether one subdirectory of `_template/` is a **named template** rather than
/// a part of the zone skeleton, from its own top-level entry names.
///
/// The distinction has to be made, because `_template/refs/` in a folder-shaped
/// zone is part of the skeleton and `_template/house/` is a template of its
/// own, and both are just directories. Nothing is stored to tell them apart:
/// the test is that the directory holds a file that *is a session's record* —
/// `AGENTS.md`, `about.md` or `README.md` — which is [`super::shape::shape`]'s
/// own signal widened by one. A `refs/` full of pointers is not a template; an
/// empty `workspace/` is not a template; a folder somebody built to start
/// sessions from says what it is on its first line.
pub fn is_named_template(top_level: &[String]) -> bool {
    top_level.iter().any(|name| {
        name == super::shape::AGENTS || name == super::shape::ABOUT || name == super::model::README
    })
}

/// A source list with everything under `dirs` removed.
///
/// This is what stops the zone's own skeleton from carrying the named templates
/// that live inside it. Without it, a zone with `_template/house/` would copy
/// `house/AGENTS.md` into every session created from *Zone template* — a
/// template that grows a sibling would silently start polluting its own output.
pub fn without_dirs(source: &[(String, bool)], dirs: &[String]) -> Vec<(String, bool)> {
    source
        .iter()
        .filter(|(rel, _)| !dirs.iter().any(|dir| in_dir(rel, dir)))
        .cloned()
        .collect()
}

/// Apply a pattern to a source file list.
///
/// **Both kinds**: the standard directories for the source's shape exist in the
/// new session, whether or not the source had them (FR-288).
///
/// **Template**: everything else travels except the README, which is stamped
/// from the template's own headings rather than copied — the plan's last step
/// writes it, so copying it first would be dead work the preview would have
/// to explain. A template create is therefore *nearly* verbatim rather than
/// exactly so: it adds those directories, and never a file.
///
/// **Session** (FR-239): `prompts/**` (reusable by design) and `refs/**`
/// (pointers worth keeping) travel; artifacts, workspace, the record and any
/// loose file stay behind, each named with its reason.
///
/// The shape is derived from the source's own top-level names, so a
/// continuation inherits the contract of what it continues — a flat session
/// begets a flat one and never sprouts the two directories its `AGENTS.md`
/// tells the reader are not how kinds are filed.
pub fn apply(kind: PatternKind, source: &[(String, bool)]) -> PatternOutcome {
    apply_with_kinds(kind, source, |_| None)
}

/// [`apply`], told what each flat file *is*.
///
/// In the folder contract a file's kind is its directory, so `prompts/**` and
/// `refs/**` can be recognised from the path alone. In the flat contract the
/// kind is a tag inside the file, and a path cannot answer it: two markdown
/// files sitting side by side at the session root can be a reusable prompt and
/// last Tuesday's log. `kind_of` is how the caller — which has already parsed
/// the pool — answers that without this module doing IO.
///
/// A caller that cannot answer passes a closure returning `None`, which is
/// exactly [`apply`]: every loose file then stays behind with `Loose`. That is
/// the conservative direction. Leaving a prompt out of a continuation is a
/// missing file the operator can copy in; carrying last week's log into a fresh
/// session is a false record, and false records are the failure this whole
/// shape exists to prevent.
pub fn apply_with_kinds(
    kind: PatternKind,
    source: &[(String, bool)],
    kind_of: impl Fn(&str) -> Option<super::shape::KindTag>,
) -> PatternOutcome {
    let top_level: Vec<String> = source
        .iter()
        .filter(|(rel, _)| !rel.contains('/'))
        .map(|(rel, _)| rel.clone())
        .collect();
    let shape = super::shape::shape(&top_level);
    let mut out = PatternOutcome::default();
    // The shape's own directories exist in every new session, whichever pattern
    // it was made from (FR-288). This used to be `kind == PatternKind::Session`,
    // and a session created from a template therefore had `artifacts/` and
    // `workspace/` only if the template happened to carry them — the owner's own
    // `_template/` did, by luck, which is why nobody saw it. A hand-made
    // template lacking them produced a session whose `AGENTS.md` describes two
    // directories it does not have, and whose first promoted artifact had
    // nowhere to go.
    //
    // **So a template create is no longer purely verbatim, and that is the
    // trade.** It adds directories and never files: what it forces is exactly
    // the pair (or, for a folder-shaped template, the four) the same create
    // forces out of a session source, so the two paths agree instead of one
    // being the honest one. The loop below de-duplicates against the source, so
    // a template that carries them is unchanged.
    for dir in standard_dirs(shape) {
        out.copies.push(((*dir).to_owned(), true));
    }
    for (rel, is_dir) in source {
        if out.copies.iter().any(|(existing, _)| existing == rel) {
            continue;
        }
        match fate(kind, shape, rel, &kind_of) {
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
fn fate(
    kind: PatternKind,
    shape: super::shape::Shape,
    rel: &str,
    kind_of: &impl Fn(&str) -> Option<super::shape::KindTag>,
) -> Option<SkipReason> {
    // The record is stamped, never copied — in either contract, and from
    // either kind of pattern. `about.md` joins `README.md` here because it is
    // the same file under a different name: a flat template whose `about.md`
    // travelled verbatim would hand every new session the template's title and
    // the template's date, which is exactly the thing stamping exists to
    // prevent.
    if rel == super::model::README || rel == super::shape::ABOUT {
        return Some(SkipReason::Record);
    }
    // `AGENTS.md` is deliberately NOT a record. It is the navigation contract,
    // and a zone that edited its own copy meant it — keeper stamps its default
    // only when the pattern did not supply one, the same way it treats every
    // other file the operator owns.
    if kind == PatternKind::Template {
        return None;
    }
    // A continuation inherits the contract it continues, edits included.
    if rel == super::shape::AGENTS {
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
    // A flat pool file travels on what it declares, not on where it sits: the
    // same two kinds the folder contract carries by directory.
    if shape == super::shape::Shape::Flat {
        return match kind_of(rel) {
            Some(super::shape::KindTag::Prompt) | Some(super::shape::KindTag::Ref) => None,
            _ => Some(SkipReason::Loose),
        };
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
        assert_eq!(
            standard_dirs(super::super::shape::Shape::Folder),
            STANDARD_DIRS,
            "a folder-shaped source keeps all four"
        );
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
    ///
    /// **Row 10 (Story 51.2).** A hand-made template lacking `artifacts/` and
    /// `workspace/` still begets a session that has them: this source is
    /// folder-shaped by its own top-level names, so it gets that shape's four.
    /// The `PatternKind::Session` guard that used to sit in front of this is
    /// gone, which makes a template create *nearly* verbatim — directories added,
    /// never a file — and this list is where that trade is visible.
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
        assert_eq!(
            copied,
            vec![
                "artifacts",
                "house-style.md",
                "prompts",
                "refs",
                "refs/.gitkeep",
                "workspace"
            ]
        );
        // The template's own `refs/` is not doubled: the loop de-duplicates
        // against the source, so a template that already carries one of these is
        // copied rather than re-created.
        assert_eq!(
            copied.iter().filter(|rel| **rel == "refs").count(),
            1,
            "a directory the source carries must appear once"
        );
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

    /// The bug this module's prefix test exists to prevent: before it, a named
    /// template's id was not equal to `_template`, so it fell through to the
    /// session index and failed with "no such session: _template/house".
    #[test]
    fn a_named_template_id_resolves_to_a_template_and_not_to_a_session() {
        assert_eq!(
            resolve(Some("_template/house")),
            Some(PatternSource::Template {
                root: "_template/house".to_owned()
            })
        );
        assert_eq!(
            resolve(Some("_template/house")).map(|source| source.kind()),
            Some(PatternKind::Template)
        );
        // …and its root is the directory it copies out of, which is the id.
        assert_eq!(named_template_id("house"), "_template/house");
    }

    /// Absent and `_template` are the same request said two ways; a ULID is
    /// still a session, which is the case the prefix test must not break.
    #[test]
    fn absent_and_the_bare_id_are_the_zone_template_and_a_ulid_is_a_session() {
        let zone = Some(PatternSource::Template {
            root: "_template".to_owned(),
        });
        assert_eq!(resolve(None), zone);
        assert_eq!(resolve(Some("_template")), zone);
        assert_eq!(
            resolve(Some("01J5AAAAAAAAAAAAAAAAAAAAAA")),
            Some(PatternSource::Session {
                id: "01J5AAAAAAAAAAAAAAAAAAAAAA".to_owned()
            })
        );
    }

    /// A `_template/…` id keeper will not accept is **refused**, never
    /// downgraded to the zone template: creating the wrong skeleton silently is
    /// the failure nobody notices until three sessions later. Traversal is the
    /// sharp case — the id crosses the IPC boundary and is joined onto a zone.
    #[test]
    fn a_template_id_keeper_cannot_join_is_refused_rather_than_reinterpreted() {
        for bad in [
            "_template/..",
            "_template/.",
            "_template/../../etc",
            "_template/a/b",
            "_template/",
            "_template/.hidden",
            "_template/_inner",
            "_template/side\\ways",
        ] {
            assert_eq!(resolve(Some(bad)), None, "{bad} must be refused");
        }
        // A session id that merely starts with the same letters is untouched:
        // the prefix test is on the separator, not on the characters.
        assert_eq!(
            resolve(Some("_templates-old")),
            Some(PatternSource::Session {
                id: "_templates-old".to_owned()
            })
        );
    }

    /// What tells a named template apart from the skeleton's own `refs/`: a
    /// file that is a session's record. Nothing is stored to say so (AD-110).
    #[test]
    fn a_named_template_is_a_directory_holding_a_record_file() {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(is_named_template(&names(&["AGENTS.md", "about.md"])));
        assert!(is_named_template(&names(&["about.md"])));
        // A folder-shaped named template still says what it is.
        assert!(is_named_template(&names(&["README.md", "prompts"])));
        // The skeleton's own directories are not templates.
        assert!(!is_named_template(&names(&[".gitkeep"])));
        assert!(!is_named_template(&names(&["design.md", "pointer.md"])));
        assert!(!is_named_template(&[]));
        // And the cheap pre-test refuses what `resolve` would refuse anyway.
        assert!(could_be_named_template("house", true));
        assert!(!could_be_named_template("house", false));
        assert!(!could_be_named_template("_inner", true));
    }

    /// The zone template must not carry the named templates that live inside
    /// it: a template that grows a sibling would otherwise start copying that
    /// sibling into every session made from it.
    #[test]
    fn the_zone_template_leaves_its_named_templates_behind() {
        let source = files(&[
            ("AGENTS.md", false),
            ("about.md", false),
            ("house", true),
            ("house/AGENTS.md", false),
            ("house/about.md", false),
            ("refs", true),
            ("refs/.gitkeep", false),
        ]);
        let trimmed = without_dirs(&source, &["house".to_owned()]);
        let out = apply(PatternKind::Template, &trimmed);
        let copied: Vec<&str> = out.copies.iter().map(|(rel, _)| rel.as_str()).collect();
        // `about.md` is absent because it is the record under its flat name and
        // gets restamped with this session's own title and date — copying the
        // template's would name every new session after the template.
        //
        // `artifacts/` and `workspace/` are present although this skeleton has
        // neither on disk: every new session has the two its `AGENTS.md`
        // describes (FR-288), and this source is flat by its own names, so it is
        // that pair and not the folder shape's four.
        assert_eq!(
            copied,
            vec![
                "AGENTS.md",
                "artifacts",
                "refs",
                "refs/.gitkeep",
                "workspace"
            ]
        );
        // Not "left behind with a reason" either — a named template is not a
        // file this session refused, it is a different pattern entirely, and
        // listing it as a skip would put it in the preview's "Leaves behind".
        assert!(!out.skips.iter().any(|(rel, _)| rel.starts_with("house")));
    }

    /// A flat source begets a flat session (FR-268): two directories, not
    /// four. Creating `refs/` and `prompts/` here would be a brand-new session
    /// holding the two directories its own `AGENTS.md` says are not how kinds
    /// are filed — a contradiction the operator meets on day one and would
    /// reasonably read as a bug.
    ///
    /// **Row 11 (Story 51.2): unchanged.** A session source always got its
    /// shape's directories; dropping the `PatternKind::Session` guard extended
    /// that to templates and must not have moved this list by one entry.
    #[test]
    fn a_flat_source_begets_a_flat_session_with_two_directories() {
        let out = apply(
            PatternKind::Session,
            &files(&[
                ("AGENTS.md", false),
                ("about.md", false),
                ("2026-08-12-0930-opened.md", false),
                ("artifacts", true),
                ("artifacts/report.pdf", false),
                ("workspace", true),
            ]),
        );
        let copied: Vec<&str> = out.copies.iter().map(|(rel, _)| rel.as_str()).collect();
        assert_eq!(copied, vec!["AGENTS.md", "artifacts", "workspace"]);
        assert!(
            !copied.contains(&"refs") && !copied.contains(&"prompts"),
            "the flat contract has no kind directories — kinds are tags"
        );
    }

    /// In the flat pool a file's kind is a tag, so the path cannot answer what
    /// travels. Told the kinds, `apply` carries the same two it carries by
    /// directory in the folder shape — and nothing else.
    #[test]
    fn a_flat_file_travels_on_what_it_declares_not_where_it_sits() {
        use super::super::shape::KindTag;

        let source = files(&[
            ("AGENTS.md", false),
            ("about.md", false),
            ("house-style.md", false),
            ("reading-list.md", false),
            ("2026-08-12-0930-opened.md", false),
            ("ship-it.md", false),
            ("loose.md", false),
            ("workspace", true),
        ]);
        let out = apply_with_kinds(PatternKind::Session, &source, |rel| match rel {
            "house-style.md" => Some(KindTag::Prompt),
            "reading-list.md" => Some(KindTag::Ref),
            "2026-08-12-0930-opened.md" => Some(KindTag::Log),
            "ship-it.md" => Some(KindTag::Task),
            _ => None,
        });
        let copied: Vec<&str> = out.copies.iter().map(|(rel, _)| rel.as_str()).collect();
        assert_eq!(
            copied,
            vec![
                "AGENTS.md",
                "artifacts",
                "house-style.md",
                "reading-list.md",
                "workspace"
            ]
        );

        let reason = |path: &str| {
            out.skips
                .iter()
                .find(|(rel, _)| rel == path)
                .map(|(_, reason)| *reason)
        };
        // A log is the previous session's record of its own sittings, and a
        // task is that session's state — carrying either forward would make the
        // new session's board a lie the moment it opened.
        assert_eq!(reason("2026-08-12-0930-opened.md"), Some(SkipReason::Loose));
        assert_eq!(reason("ship-it.md"), Some(SkipReason::Loose));
        assert_eq!(reason("loose.md"), Some(SkipReason::Loose));
        assert_eq!(reason("about.md"), Some(SkipReason::Record));
    }

    /// The conservative default: a caller that cannot classify the pool leaves
    /// every loose file behind rather than guessing. A missing prompt is a file
    /// the operator copies in; a carried-over log is a false record, and false
    /// records are the failure the flat shape exists to prevent.
    #[test]
    fn an_unclassified_flat_pool_carries_nothing_loose() {
        let out = apply(
            PatternKind::Session,
            &files(&[
                ("AGENTS.md", false),
                ("about.md", false),
                ("house-style.md", false),
            ]),
        );
        let copied: Vec<&str> = out.copies.iter().map(|(rel, _)| rel.as_str()).collect();
        assert_eq!(copied, vec!["AGENTS.md", "artifacts", "workspace"]);
        assert!(out
            .skips
            .iter()
            .any(|(rel, reason)| rel == "house-style.md" && *reason == SkipReason::Loose));
    }

    /// The two files keeper stamps are the two it must not copy: `about.md`
    /// carries the source's title and date, and copying it would hand every new
    /// session someone else's record. `AGENTS.md` travels, because an edited
    /// navigation contract is the operator's and keeper does not improve on it.
    #[test]
    fn the_record_never_travels_under_either_name_but_the_contract_does() {
        for kind in [PatternKind::Session, PatternKind::Template] {
            let out = apply(
                kind,
                &files(&[
                    ("AGENTS.md", false),
                    ("about.md", false),
                    ("README.md", false),
                ]),
            );
            let copied: Vec<&str> = out.copies.iter().map(|(rel, _)| rel.as_str()).collect();
            assert!(copied.contains(&"AGENTS.md"), "{kind:?} keeps the contract");
            assert!(
                !copied.contains(&"about.md"),
                "{kind:?} restamps the record"
            );
            assert!(
                !copied.contains(&"README.md"),
                "{kind:?} restamps the record"
            );
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
