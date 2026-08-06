//! Titles in, filenames out (FR-96, FR-98, FR-99).
//!
//! Pure by construction: the caller hands in the set of sibling names it already
//! has, and gets back a name that is free. Nothing here stats a directory, and
//! nothing here writes a file — that is story 36.5's job in the shell.
//!
//! The rules are shaped by one fact that is easy to forget on a Mac: **this
//! vault syncs to Windows.** So the slug alphabet excludes every character
//! Windows forbids in a path, a name may not end in a dot, and the MS-DOS device
//! names (`con`, `nul`, `com1`, …) are refused — on Windows a file called `con`
//! cannot be created at all, and the failure surfaces as a sync error on a
//! machine the author never touched.

use crate::notes::search::fold_str;

/// Where a journal entry lands when the vault has not configured otherwise
/// (FR-99).
pub const DEFAULT_JOURNAL_TEMPLATE: &str = "journal/{yyyy}/{yyyy}-{mm}-{dd}.md";

/// Character cap on a slug. Long enough to stay readable, short enough that
/// `<date>-<slug>-<n>.md` clears the 255-byte name limit even when every
/// character is a 4-byte codepoint.
const SLUG_MAX_CHARS: usize = 60;

/// What an emoji-only or punctuation-only title slugs to. A note must always
/// get a usable filename; refusing to name it would lose the note.
const FALLBACK_SLUG: &str = "untitled";

/// MS-DOS device names. Reserved on Windows in *every* directory, with or
/// without an extension.
///
/// Crate-visible because a recording folder is subject to exactly the same
/// Windows rule as a note filename — see
/// [`crate::recording::path_template`], which refuses to render one bare.
pub(crate) const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Fold a title into a slug, with **no** fallback and **no** device-name
/// escape: an emoji-only title folds to the empty string, and `"NUL"` folds to
/// `"nul"`.
///
/// This is the whole slug algorithm minus the two decisions that only a
/// *filename* has to make. [`slug`] adds them back; the recording path
/// renderer ([`crate::recording::path_template`]) makes the opposite choice —
/// an empty slug there collapses out of the path together with its separator,
/// so a fallback word would be exactly the "Untitled" placeholder epic 40
/// refuses. Sharing the fold is what stops one title from becoming two
/// different names in one app.
///
/// Lowercased and diacritic-folded through the same table [`crate::notes::search`]
/// uses, so a title's slug matches what a searching user types. Letters and
/// digits survive — including CJK, which has no case and no transliteration we
/// could honestly apply — and every other character collapses into a single
/// `-`, which is then trimmed from both ends.
///
/// keeper has no unicode-normalisation dependency (AD-55 rejects acquiring one),
/// so canonical equivalence is approximated rather than computed: combining
/// marks are dropped and precomposed Latin letters are folded to their base, and
/// the two spellings of `café` therefore land on the same slug. That is the
/// property NFC was wanted for.
pub(crate) fn slug_stem(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut gap = false;

    for c in fold_str(title).chars() {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }

    // Cap on a character boundary. A grapheme cluster can still be split here
    // (that would need a dependency), but a codepoint never is, so the result is
    // always valid UTF-8 and always a legal filename.
    if out.chars().count() > SLUG_MAX_CHARS {
        let cut = out
            .char_indices()
            .nth(SLUG_MAX_CHARS)
            .map_or(out.len(), |(i, _)| i);
        out.truncate(cut);
    }
    while out.ends_with('-') {
        out.pop();
    }

    out
}

/// Fold a title into a filename-safe slug: [`slug_stem`], plus the two things a
/// note *filename* cannot do without — a fallback word when the fold leaves
/// nothing, and a suffix when the fold lands on an MS-DOS device name.
///
/// A note must always get a usable filename; refusing to name it would lose the
/// note.
pub fn slug(title: &str) -> String {
    let mut out = slug_stem(title);

    if out.is_empty() {
        return FALLBACK_SLUG.to_owned();
    }
    if RESERVED_DEVICE_NAMES.contains(&out.as_str()) {
        out.push_str("-note");
    }
    out
}

/// `YYYY-MM-DD-<slug>.md`, with a `-2`, `-3`, … counter appended until the name
/// is free.
///
/// `taken` is the set of sibling *file names* the caller already has. The
/// comparison is case-insensitive because APFS and NTFS are: two notes that
/// differ only in case are one file on the machine the user is looking at, and
/// discovering that during a sync push is far worse than discovering it here.
///
/// The loop always terminates — every iteration produces a name not yet tried,
/// and `taken` is finite.
pub fn note_filename(title: &str, date: &str, taken: &[String]) -> String {
    let slug = slug(title);
    let stem = if date.is_empty() {
        slug
    } else {
        format!("{date}-{slug}")
    };

    let mut candidate = format!("{stem}.md");
    let mut n: u32 = 1;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&candidate)) {
        n += 1;
        candidate = format!("{stem}-{n}.md");
    }
    candidate
}

/// The note's title as a human would read it: the first non-empty body line,
/// with ATX heading markers removed.
///
/// Returns an empty string for an empty body so the caller can fall back to the
/// filename stem — inventing "Untitled" here would put that word in the index,
/// the tray and the switcher for a journal note that already has a perfectly
/// good name on disk.
pub fn title_from_body(body: &str) -> String {
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Only strip `#` when it actually opens a heading. `#project/keeper` on
        // its own line is a tag, and its text is not the note's title.
        let hashes = line.len() - line.trim_start_matches('#').len();
        let rest = &line[hashes..];
        let title = if hashes > 0 && (rest.is_empty() || rest.starts_with(' ')) {
            rest.trim().trim_end_matches('#').trim()
        } else {
            line
        };

        if !title.is_empty() {
            return title.to_owned();
        }
    }
    String::new()
}

/// Expand a journal path template for one date (FR-99).
///
/// Closed placeholder set: `{yyyy}`, `{yy}`, `{mm}`, `{dd}`. Anything else is
/// left literal rather than guessed at.
///
/// The result is sanitised into a vault-relative path: separators normalised to
/// `/`, empty and `.` and `..` segments dropped, `.md` appended when absent.
/// The template is configuration an agent can edit, so `..` in it must
/// not be able to walk out of the vault root; the shell canonicalises again
/// before writing, and this is the first of the two gates.
pub fn journal_path(template: &str, y: i32, m: u32, d: u32) -> String {
    let expanded = template
        .replace("{yyyy}", &format!("{y:04}"))
        .replace("{yy}", &format!("{:02}", y.rem_euclid(100)))
        .replace("{mm}", &format!("{m:02}"))
        .replace("{dd}", &format!("{d:02}"));

    let mut parts: Vec<&str> = Vec::new();
    for segment in expanded.split(['/', '\\']) {
        match segment {
            "" | "." | ".." => continue,
            s => parts.push(s),
        }
    }

    let mut out = parts.join("/");
    if out.is_empty() {
        out = format!("{y:04}-{m:02}-{d:02}");
    }
    if !out.to_ascii_lowercase().ends_with(".md") {
        out.push_str(".md");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_an_ordinary_title() {
        assert_eq!(slug("Weekly Review — Q3!"), "weekly-review-q3");
        assert_eq!(slug("  leading and trailing  "), "leading-and-trailing");
        assert_eq!(slug("a///b"), "a-b");
    }

    #[test]
    fn slugs_accents_to_their_ascii_base() {
        assert_eq!(slug("Café Déjà Vu"), "cafe-deja-vu");
        assert_eq!(slug("Łódź trip"), "lodz-trip");
        // Decomposed input reaches the same slug, which is what NFC was for.
        assert_eq!(slug("cafe\u{301}"), slug("caf\u{e9}"));
    }

    #[test]
    fn keeps_cjk_because_there_is_no_honest_transliteration() {
        assert_eq!(slug("日本語のノート"), "日本語のノート");
        assert_eq!(slug("会議 2026"), "会議-2026");
    }

    #[test]
    fn an_emoji_only_title_still_yields_a_usable_name() {
        assert_eq!(slug("🎉🎉🎉"), FALLBACK_SLUG);
        assert_eq!(slug("···"), FALLBACK_SLUG);
        assert_eq!(slug(""), FALLBACK_SLUG);
        // …and it is still a legal filename.
        assert!(!slug("🎉").is_empty());
    }

    #[test]
    fn caps_a_long_title_on_a_character_boundary() {
        let title = "ä".repeat(300);
        let s = slug(&title);
        assert_eq!(s.chars().count(), SLUG_MAX_CHARS);
        assert_eq!(s, "a".repeat(SLUG_MAX_CHARS));

        // A 300-char CJK title caps at 60 *characters*, not 60 bytes.
        let cjk = slug(&"語".repeat(300));
        assert_eq!(cjk.chars().count(), SLUG_MAX_CHARS);
        assert_eq!(cjk.len(), SLUG_MAX_CHARS * 3);
    }

    #[test]
    fn never_ends_in_a_separator_after_capping() {
        // The 61st character is the one that would have become the `-`.
        let title = format!("{} tail", "a".repeat(SLUG_MAX_CHARS));
        assert_eq!(slug(&title), "a".repeat(SLUG_MAX_CHARS));
    }

    #[test]
    fn refuses_windows_device_names() {
        assert_eq!(slug("CON"), "con-note");
        assert_eq!(slug("nul"), "nul-note");
        assert_eq!(slug("lpt9"), "lpt9-note");
        assert_eq!(slug("console"), "console");
    }

    #[test]
    fn never_ends_in_a_dot() {
        // A trailing dot is silently stripped by Windows, so two notes would
        // collide on one file. The slug alphabet cannot produce one.
        assert_eq!(slug("Ready."), "ready");
        assert_eq!(slug("Ready..."), "ready");
    }

    #[test]
    fn two_meetings_on_one_day_get_distinct_filenames() {
        let mut taken: Vec<String> = Vec::new();

        let first = note_filename("Meeting", "2026-08-02", &taken);
        assert_eq!(first, "2026-08-02-meeting.md");
        taken.push(first);

        let second = note_filename("Meeting", "2026-08-02", &taken);
        assert_eq!(second, "2026-08-02-meeting-2.md");
        taken.push(second);

        let third = note_filename("Meeting", "2026-08-02", &taken);
        assert_eq!(third, "2026-08-02-meeting-3.md");
    }

    #[test]
    fn collision_check_ignores_case_because_the_filesystem_does() {
        let taken = vec!["2026-08-02-MEETING.MD".to_owned()];
        assert_eq!(
            note_filename("Meeting", "2026-08-02", &taken),
            "2026-08-02-meeting-2.md"
        );
    }

    #[test]
    fn title_comes_from_the_first_non_empty_line() {
        assert_eq!(
            title_from_body("\n\n# Weekly review\n\nbody"),
            "Weekly review"
        );
        assert_eq!(
            title_from_body("plain first line\n# later heading"),
            "plain first line"
        );
        assert_eq!(title_from_body("### Closed heading ###"), "Closed heading");
        assert_eq!(title_from_body("   "), "");
        assert_eq!(title_from_body(""), "");
    }

    #[test]
    fn a_lone_tag_line_is_not_a_heading() {
        assert_eq!(
            title_from_body("#project/keeper\n\nreal text"),
            "#project/keeper"
        );
    }

    #[test]
    fn journal_path_expands_the_closed_placeholder_set() {
        assert_eq!(
            journal_path(DEFAULT_JOURNAL_TEMPLATE, 2026, 8, 2),
            "journal/2026/2026-08-02.md"
        );
        assert_eq!(
            journal_path("log/{yy}/{mm}{dd}", 2026, 8, 2),
            "log/26/0802.md"
        );
        assert_eq!(
            journal_path("daily/{yyyy}-{unknown}.md", 2026, 8, 2),
            "daily/2026-{unknown}.md"
        );
    }

    #[test]
    fn journal_path_cannot_walk_out_of_the_vault() {
        assert_eq!(
            journal_path("../../etc/{yyyy}.md", 2026, 8, 2),
            "etc/2026.md"
        );
        assert_eq!(journal_path("/abs/{yyyy}.md", 2026, 8, 2), "abs/2026.md");
        assert_eq!(journal_path(r"win\path\{dd}", 2026, 8, 2), "win/path/02.md");
        assert_eq!(journal_path("..", 2026, 8, 2), "2026-08-02.md");
    }
}
