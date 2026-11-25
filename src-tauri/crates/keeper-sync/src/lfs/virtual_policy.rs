//! The virtualization grammar moved to `keeper_folder::virtual_policy` (story 119.1)
//! and is re-exported here; what stays is reading it from a sync profile.

pub use keeper_folder::virtual_policy::*;

use crate::error::Result;
use crate::profile::SyncProfile;

/// Compile the policy a sync profile implies: its worktree's committed file,
/// its `virtualPatterns` and its size floor.
pub trait CompileForProfile: Sized {
    fn compile(profile: &SyncProfile) -> Result<Self>;
}

/// Refuse a `virtualPatterns` list the grammar would refuse, as this crate's
/// error (`SyncProfile::validate` calls it).
pub fn check_patterns(entries: &[String]) -> Result<()> {
    Ok(keeper_folder::virtual_policy::check_patterns(entries)?)
}

impl CompileForProfile for VirtualPolicy {
    fn compile(profile: &SyncProfile) -> Result<Self> {
        Ok(Self::compile_in(
            &profile.local_path,
            &profile.virtual_patterns,
            profile.virtual_over_bytes,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SyncError;
    use crate::profile::{FolderTier, FOLDER_CONFIG_DIR};
    use std::path::Path;

    fn profile(root: &Path) -> SyncProfile {
        SyncProfile::new("01JVIRT", "media", root, "https://git.invalid/r.git")
    }

    /// A temp worktree, optionally holding a committed pattern file. Real files
    /// throughout: the whole claim of this module is about what is on disk, and
    /// a hand-mocked source would assert nothing.
    fn worktree(pattern_file: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        if let Some(text) = pattern_file {
            std::fs::write(dir.path().join(VIRTUAL_PATTERN_FILE), text)
                .expect("write .keepervirtual");
        }
        dir
    }

    /// Write one folder config file, as `profile::folder`'s own tests do.
    fn folder_file(root: &Path, name: &str, text: &str) {
        let keeper = root.join(FOLDER_CONFIG_DIR);
        std::fs::create_dir_all(&keeper).expect("create .keeper");
        std::fs::write(keeper.join(name), text).expect("write folder config");
    }

    /// Nothing configured must be nothing virtual — not "everything", which is
    /// what an empty pattern set read as a wildcard would mean.
    #[test]
    fn nothing_configured_leaves_every_path_materialized() {
        let dir = worktree(None);
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Unset);
        for path in ["a.mp4", "40-media/a.mp4", "deep/er/still/a.iso"] {
            assert_eq!(
                policy.resolve(Path::new(path), 10_000_000),
                Virtualization::Materialize,
                "{path}: with no policy configured nothing may stay away"
            );
        }
    }

    /// The anchoring rule is gitignore's, and both halves of it matter: a
    /// pattern with a `/` covers its own zone, and only that zone. A
    /// `40-media/**` that also swallowed `30-work/40-media/` would dehydrate a
    /// folder the user never named.
    #[test]
    fn a_root_anchored_pattern_covers_its_own_zone_and_nothing_named_like_it_elsewhere() {
        let dir = worktree(Some("40-media/**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::PatternFile);
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Virtual
        );
        assert_eq!(
            policy.resolve(Path::new("10-notes/a.md"), 10_000_000),
            Virtualization::Materialize,
            "a zone the policy never named keeps its content"
        );
        assert_eq!(
            policy.resolve(Path::new("30-work/40-media/a.mp4"), 10_000_000),
            Virtualization::Materialize,
            "a pattern with a slash is anchored at the root, not matched at any depth"
        );
    }

    /// The other half of the same rule: a bare basename covers every depth, so a
    /// user who wrote `*.mp4` does not have to enumerate their folders.
    #[test]
    fn a_pattern_without_a_slash_matches_that_basename_at_any_depth() {
        let dir = worktree(Some("*.mp4\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("a/b/c.mp4"), 10_000_000),
            Virtualization::Virtual
        );
        assert_eq!(
            policy.resolve(Path::new("a/b/c.md"), 10_000_000),
            Virtualization::Materialize
        );
    }

    /// Negation is how `never` gets populated, and it wins unconditionally —
    /// not last-match-wins. Ordering the lines the other way round must not
    /// change the answer, because the file that survives here is somebody's.
    #[test]
    fn a_negated_line_keeps_one_files_bytes_while_its_siblings_may_leave() {
        for text in [
            "40-media/**\n!40-media/keep.mp4\n",
            "!40-media/keep.mp4\n40-media/**\n",
        ] {
            let dir = worktree(Some(text));
            let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
            assert_eq!(
                policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
                Virtualization::Materialize,
                "the negated path must keep its content whatever the line order"
            );
            assert_eq!(
                policy.resolve(Path::new("40-media/other.mp4"), 10_000_000),
                Virtualization::Virtual,
                "and its siblings are still covered"
            );
        }
    }

    /// A stray newline or a comment must not be read as a pattern. Read as
    /// "match everything" — the way an empty glob would be — one blank line
    /// would authorize dehydrating an entire repository.
    #[test]
    fn comments_and_blank_lines_say_nothing_and_never_mean_match_everything() {
        let dir = worktree(Some("# note about the policy\n\n   \n*.iso\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("50-iso/debian.iso"), 10_000_000),
            Virtualization::Virtual,
            "the one real line is still in force"
        );
        for path in ["10-notes/a.md", "note about the policy", "#"] {
            assert_eq!(
                policy.resolve(Path::new(path), 10_000_000),
                Virtualization::Materialize,
                "{path}: a comment or a blank line matches nothing at all"
            );
        }

        // And a file of nothing but noise is a file that says nothing, which the
        // tier has to report honestly.
        let quiet = worktree(Some("# only a comment\n\n"));
        let policy = VirtualPolicy::compile(&profile(quiet.path())).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Unset);
    }

    /// Without the escape, a file whose name starts with `!` or `#` would be
    /// unnameable — every line describing it would be read as a negation or a
    /// comment.
    #[test]
    fn a_backslash_escapes_a_filename_that_really_starts_with_a_bang() {
        let dir = worktree(Some("\\!literal.bin\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("!literal.bin"), 10_000_000),
            Virtualization::Virtual,
            "the escaped line names a real file, it is not a negation"
        );
        assert_eq!(
            policy.resolve(Path::new("literal.bin"), 10_000_000),
            Virtualization::Materialize,
            "and the `!` is part of the name, not stripped from it"
        );
    }

    /// Inclusive at the floor, exactly as `LfsPolicy`'s threshold is. Two
    /// size rules in one engine that disagreed about their own boundary would
    /// be a trap: a file could be routed through LFS and then refused
    /// virtualization for being one byte too small.
    #[test]
    fn the_size_floor_is_inclusive_so_a_file_exactly_at_it_may_stay_away() {
        let dir = worktree(Some("*.bin\n"));
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("a.bin"), 1023),
            Virtualization::Materialize,
            "below the floor the pattern does not matter"
        );
        assert_eq!(
            policy.resolve(Path::new("a.bin"), 1024),
            Virtualization::Virtual,
            "at the floor it does — the boundary is inclusive"
        );
    }

    /// The profile tiers override the repository's file wholesale rather than
    /// merging with it: a machine that says "only these" must not silently
    /// inherit the file's list as well, or one host would dehydrate paths its
    /// own configuration never named.
    #[test]
    fn a_non_empty_profile_list_replaces_the_committed_file_wholesale() {
        let dir = worktree(Some("40-media/**\n"));
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["50-iso/**".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Profile);
        assert_eq!(
            policy.resolve(Path::new("50-iso/debian.iso"), 10_000_000),
            Virtualization::Virtual
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Materialize,
            "the file's list is replaced, not merged into"
        );
    }

    /// An empty list is silence, not "override with nothing" — the same reading
    /// `lfs_never.is_empty()` already takes. Otherwise every profile that had
    /// never been edited would mute the repository's own committed policy.
    #[test]
    fn an_empty_profile_list_is_silence_so_the_committed_file_stays_in_force() {
        let dir = worktree(Some("40-media/**\n"));
        let p = profile(dir.path());
        assert!(
            p.virtual_patterns.is_empty(),
            "the default is an empty list"
        );
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::PatternFile);
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Virtual
        );
    }

    /// The folder TOML tier is the canonical home for anything both hosts must
    /// honour (AD-132), and it is a *real* file resolved by the real tier here:
    /// a hand-built profile would assert nothing about whether `virtualPatterns`
    /// is even an accepted folder key.
    #[test]
    fn a_real_folder_toml_layer_overrides_the_committed_file() {
        let dir = worktree(Some("40-media/**\n"));
        folder_file(
            dir.path(),
            "keeper.toml",
            "[folder]\nvirtualPatterns = [\"70-vm/**\"]\n",
        );
        let outcome = FolderTier::new("hesperia", None).apply(&profile(dir.path()));
        assert!(
            outcome.faults.is_empty(),
            "the folder file must be accepted, got {:?}",
            outcome.faults
        );
        let policy = VirtualPolicy::compile(&outcome.profile).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Profile);
        assert_eq!(
            policy.resolve(Path::new("70-vm/disk.img"), 10_000_000),
            Virtualization::Virtual,
            "the folder file's list is in force"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Materialize,
            "and it replaced the committed file's list"
        );
    }

    /// AD-99's precedence, asserted through the tier rather than assumed: this
    /// machine's file wins where it speaks, and the shared file still wins on
    /// every key it alone sets — which is what stops a per-host override from
    /// muting the folder's shared floor.
    #[test]
    fn the_host_folder_file_wins_and_the_shared_one_still_speaks_where_it_alone_does() {
        let dir = worktree(None);
        folder_file(
            dir.path(),
            "keeper.toml",
            "[folder]\nvirtualPatterns = [\"70-vm/**\"]\nvirtualOverBytes = 4096\n",
        );
        folder_file(
            dir.path(),
            "keeper.hesperia.toml",
            "[folder]\nvirtualPatterns = [\"80-hosts/**\"]\n",
        );
        let outcome = FolderTier::new("hesperia", None).apply(&profile(dir.path()));
        assert!(
            outcome.faults.is_empty(),
            "both folder files must be accepted, got {:?}",
            outcome.faults
        );
        let policy = VirtualPolicy::compile(&outcome.profile).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("80-hosts/disk.img"), 10_000_000),
            Virtualization::Virtual,
            "this machine's file wins on the key it sets"
        );
        assert_eq!(
            policy.resolve(Path::new("70-vm/disk.img"), 10_000_000),
            Virtualization::Materialize,
            "the shared list was replaced, not merged"
        );
        assert_eq!(
            policy.resolve(Path::new("80-hosts/disk.img"), 4095),
            Virtualization::Materialize,
            "the shared file's floor still applies: the host file never set one"
        );
    }

    /// AD-98: the folder file outranks the stored row on every read, which is
    /// what lets the file keep winning after a user has once edited the form.
    #[test]
    fn a_folder_file_outranks_the_stored_profile_row() {
        let dir = worktree(None);
        folder_file(
            dir.path(),
            "keeper.toml",
            "[folder]\nvirtualPatterns = [\"70-vm/**\"]\n",
        );
        let mut stored = profile(dir.path());
        stored.virtual_patterns = vec!["90-stored/**".to_owned()];
        let outcome = FolderTier::new("hesperia", None).apply(&stored);
        assert!(
            outcome.faults.is_empty(),
            "the folder file must be accepted, got {:?}",
            outcome.faults
        );
        let policy = VirtualPolicy::compile(&outcome.profile).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("70-vm/disk.img"), 10_000_000),
            Virtualization::Virtual,
            "the file wins"
        );
        assert_eq!(
            policy.resolve(Path::new("90-stored/disk.img"), 10_000_000),
            Virtualization::Materialize,
            "the stored row is the base the file overrides, not a merge partner"
        );
    }

    /// A policy made of nothing but protections is **in force** and authorizes
    /// **nothing** — and the two questions have to be asked separately (Story
    /// 56.10).
    ///
    /// A repository owner pre-protecting a zone before authorizing any is an
    /// ordinary thing to commit, and `tier` says `PatternFile` for it, quite
    /// correctly: a list did decide what may stay away, and the answer was
    /// "nothing". A caller that read the tier as "there is something here worth
    /// asking about" would open the repository and parse the index once per
    /// candidate row on every sync of that folder, forever, to be told no.
    #[test]
    fn a_policy_of_only_protections_is_in_force_and_authorizes_nothing() {
        let dir = worktree(Some("!30-masters/**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::PatternFile,
            "a `!` line is a source saying something"
        );
        assert!(
            !policy.authorizes_anything(),
            "...and it authorizes nothing, which is the question a caller has to ask"
        );
        for path in ["30-masters/a.mp4", "40-media/a.mp4", "a.mp4"] {
            assert_eq!(
                policy.resolve(Path::new(path), 10_000_000),
                Virtualization::Materialize,
                "{path}: nothing may stay away under a policy with no permissive line"
            );
        }

        let named = worktree(Some("40-media/**\n!40-media/keep.mp4\n"));
        let policy = VirtualPolicy::compile(&profile(named.path())).expect("compiles");
        assert!(
            policy.authorizes_anything(),
            "one permissive line is what makes the policy worth consulting"
        );
    }

    /// A profile list of nothing but `!` protections does **not** replace the
    /// committed file's permissive list (Story 56.14).
    ///
    /// A protection is not a claim about the zone — it is an exception inside
    /// whatever zone is in force — so only the PERMISSIVE half of a profile
    /// list can override the committed one. Without the fix `overrides` asked
    /// `says_something()`, which either half satisfies: the empty profile
    /// permissive list replaced the committed `40-media/**`,
    /// `authorizes_anything()` went false, `40-media/other.mp4` read
    /// `Materialize` and `tier()` reported `Profile` — so a machine restating
    /// ONE exception silently switched the whole folder's virtualization off
    /// from a line written to PROTECT one path, which narrows what is
    /// authorized to nothing and is the inverse of AD-123's rule.
    #[test]
    fn a_profile_list_of_only_protections_does_not_replace_the_committed_zone() {
        let dir = worktree(Some("40-media/**\n"));
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["!40-media/keep.mp4".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("40-media/other.mp4"), 10_000_000),
            Virtualization::Virtual,
            "the repository's zone survives a machine that only named an exception"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
            Virtualization::Materialize,
            "and the machine's own protection applies inside it"
        );
        assert!(
            policy.authorizes_anything(),
            "a protection cannot empty the permissive list it was written against"
        );
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::PatternFile,
            "the file supplied the list in force, so that is the file to edit"
        );
    }

    /// A profile list of only protections, over no committed file at all, is
    /// still a policy and still authorizes nothing (Story 56.14).
    ///
    /// This is the tier fallback the fix adds: the profile is the only source
    /// that spoke, so reporting `Unset` would say nothing configured anything.
    /// Without the fix the answer was `Profile` for the wrong reason — an
    /// override that had discarded a list — and after the naive repair, which
    /// only tightens `overrides`, it would read `Unset` for a folder that does
    /// carry a policy.
    #[test]
    fn a_protections_only_profile_list_over_no_file_is_still_the_profiles_policy() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["!30-masters/**".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert!(
            !policy.authorizes_anything(),
            "no source stated a permissive line, so nothing may stay away"
        );
        for path in ["30-masters/a.mp4", "40-media/a.mp4", "a.mp4"] {
            assert_eq!(
                policy.resolve(Path::new(path), 10_000_000),
                Virtualization::Materialize,
                "{path}: a policy with no permissive line authorizes nothing"
            );
        }
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::Profile,
            "the profile is the only source that spoke, and it did speak"
        );
    }

    /// `check_patterns` refuses exactly what `compile` would refuse, so the
    /// Settings form can say no at the box (Story 56.14, FR-329).
    ///
    /// FR-329 says a malformed pattern is a hard `SyncError::Config` naming its
    /// source and never a silently dropped pattern. Without the fix there was
    /// no such entry point at all: nothing compiled the list before it was
    /// stored, so `scans/[` was accepted and only the next sync said so.
    #[test]
    fn check_patterns_accepts_every_legitimate_list_and_refuses_a_malformed_glob() {
        check_patterns(&[]).expect("an empty list is the documented spelling of silence");
        check_patterns(&["!30-masters/**".to_owned()])
            .expect("a list of only protections is a policy about exceptions");
        check_patterns(&["40-media/**".to_owned(), "!40-media/keep.mp4".to_owned()])
            .expect("and a well-formed list of both halves is the ordinary case");

        let err = check_patterns(&["scans/[".to_owned()])
            .expect_err("an unclosed character class must not be saveable");
        assert!(
            matches!(err, SyncError::Config(_)),
            "a bad glob is a configuration refusal, got {err:?}"
        );
        let text = format!("{err}");
        assert!(
            text.contains("scans/["),
            "the message must quote the entry as typed, got: {text}"
        );
        assert!(
            text.contains(PROFILE_SOURCE),
            "and name the list it is in, got: {text}"
        );
    }

    /// A typo must name the file it is in. Silently dropping the line would let
    /// a repository believe it had a policy it does not have (FR-329).
    #[test]
    fn a_malformed_glob_in_the_committed_file_names_the_file_and_quotes_the_line() {
        let dir = worktree(Some("[unclosed\n"));
        let err = VirtualPolicy::compile(&profile(dir.path())).expect_err("must refuse");
        let text = format!("{err}");
        assert!(
            text.contains(VIRTUAL_PATTERN_FILE),
            "the message must say which source the typo is in, got: {text}"
        );
        assert!(
            text.contains("[unclosed"),
            "the message must quote the line as typed, got: {text}"
        );
    }

    /// And a typo in one machine's TOML must name *that* key, or the user goes
    /// looking in the committed file for a line that is not there.
    #[test]
    fn a_malformed_glob_in_the_profile_list_names_the_config_key_and_quotes_the_entry() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["[unclosed".to_owned()];
        let err = VirtualPolicy::compile(&p).expect_err("must refuse");
        let text = format!("{err}");
        assert!(
            text.contains("virtualPatterns"),
            "the message must name the config key, got: {text}"
        );
        assert!(
            text.contains("[unclosed"),
            "the message must quote the entry as typed, got: {text}"
        );
        assert!(
            !text.contains(VIRTUAL_PATTERN_FILE),
            "and must not send the user to a file they did not edit, got: {text}"
        );
    }

    /// A negated line is compiled from its remainder, so without care the
    /// refusal would quote a string the user never typed and they would search
    /// the file for it in vain.
    #[test]
    fn a_malformed_negated_glob_is_quoted_with_the_bang_the_user_typed() {
        let dir = worktree(Some("40-media/**\n![unclosed\n"));
        let err = VirtualPolicy::compile(&profile(dir.path())).expect_err("must refuse");
        let text = format!("{err}");
        assert!(
            text.contains("\"![unclosed\""),
            "the message must quote the negation as typed, got: {text}"
        );
    }

    /// Absence is silence: most repositories will never carry this file, and
    /// demanding it would make the ordinary case an error.
    #[test]
    fn an_absent_pattern_file_is_silence_not_a_failure() {
        let dir = worktree(None);
        assert!(!dir.path().join(VIRTUAL_PATTERN_FILE).exists());
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("absence is not an error");
        assert_eq!(policy.tier(), VirtualPolicyTier::Unset);
    }

    /// A policy we cannot read is not a policy. Reading an unreadable file as
    /// "nothing is virtual" is a plausible answer to the wrong question, and it
    /// would hide a broken checkout for as long as nobody looked.
    #[test]
    fn a_directory_named_like_the_pattern_file_is_refused_with_the_os_reason() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join(VIRTUAL_PATTERN_FILE)).expect("create a directory");
        let err = VirtualPolicy::compile(&profile(dir.path())).expect_err("must refuse");
        assert!(
            matches!(err, SyncError::Config(_)),
            "an unreadable policy is a config fault, got {err:?}"
        );
        let text = format!("{err}");
        assert!(
            text.contains(VIRTUAL_PATTERN_FILE) && text.contains("could not read"),
            "the message must name the file and the OS reason, got: {text}"
        );
    }

    /// FR-328, proved the only way it can be: the worktree is deleted out from
    /// under the compiled policy, and the answers do not change. If `resolve`
    /// touched the filesystem at all this test could not pass.
    #[test]
    fn resolve_answers_identically_after_the_entire_worktree_is_deleted() {
        let dir = worktree(Some("40-media/**\n!40-media/keep.mp4\n"));
        let root = dir.path().to_path_buf();
        let mut p = profile(&root);
        p.virtual_over_bytes = 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        let cases = [
            ("40-media/a.mp4", 10_000u64, Virtualization::Virtual),
            ("40-media/keep.mp4", 10_000, Virtualization::Materialize),
            ("40-media/tiny.mp4", 10, Virtualization::Materialize),
            ("10-notes/a.md", 10_000, Virtualization::Materialize),
        ];
        for (path, size, want) in cases {
            assert_eq!(policy.resolve(Path::new(path), size), want, "{path} before");
        }

        std::fs::remove_dir_all(&root).expect("delete the whole worktree");
        assert!(!root.exists(), "the worktree must really be gone");
        for (path, size, want) in cases {
            assert_eq!(
                policy.resolve(Path::new(path), size),
                want,
                "{path}: resolve must answer from the compiled policy alone"
            );
        }
    }

    /// FR-330 and AD-123. This pins the git-lfs#3092 failure mode: there, a
    /// pattern change dropped content that existed nowhere else. Editing a
    /// policy here may change only the answer — deleting a byte needs
    /// per-object proof, which this module never has and never asks for.
    #[test]
    fn editing_the_policy_changes_only_the_answer_and_never_a_byte() {
        let dir = worktree(Some("50-iso/**\n"));
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("40-media")).expect("create the zone");
        let payload = vec![7u8; 4096];
        let target = root.join("40-media/a.mp4");
        std::fs::write(&target, &payload).expect("write real bytes");

        let before = VirtualPolicy::compile(&profile(&root)).expect("compiles");
        assert_eq!(
            before.resolve(Path::new("40-media/a.mp4"), payload.len() as u64),
            Virtualization::Materialize,
            "the first policy does not cover the file"
        );

        std::fs::write(root.join(VIRTUAL_PATTERN_FILE), "40-media/**\n").expect("rewrite policy");
        let after = VirtualPolicy::compile(&profile(&root)).expect("recompiles");
        assert_eq!(
            after.resolve(Path::new("40-media/a.mp4"), payload.len() as u64),
            Virtualization::Virtual,
            "the edited policy now covers it"
        );

        let on_disk = std::fs::read(&target).expect("the file must still be there");
        assert_eq!(
            on_disk.len(),
            payload.len(),
            "the file's length must be untouched by a policy edit"
        );
        assert_eq!(
            on_disk, payload,
            "and its bytes byte-for-byte: authorizing virtualization is not deleting"
        );
    }

    /// The two fields are very nearly opposites, and the names are close enough
    /// that conflating them is a live risk: `lfs_never` says "never route this
    /// through LFS", so a file matched there has no pointer to stay away behind
    /// at all. Reading it as a virtualization request would dehydrate exactly
    /// the files the user asked to keep as ordinary blobs.
    #[test]
    fn lfs_never_is_not_a_virtualization_request_and_neither_field_moves_the_other() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.lfs_never = vec!["*.mp4".to_owned()];
        assert!(p.virtual_patterns.is_empty());

        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Unset);
        assert_eq!(
            policy.resolve(Path::new("a.mp4"), 10_000),
            Virtualization::Materialize,
            "an LFS opt-out is not a request to virtualize"
        );

        let lfs = crate::lfs::stage::LfsPolicy::from_profile(&p).expect("compiles");
        assert!(
            !lfs.applies(Path::new("a.mp4"), 10_000),
            "and the opt-out still means what it always meant"
        );

        // The other direction: a virtual pattern must not start routing a path
        // through LFS that the user excluded, nor exclude one they did not.
        let mut both = p.clone();
        both.virtual_patterns = vec!["*.iso".to_owned()];
        let lfs_both = crate::lfs::stage::LfsPolicy::from_profile(&both).expect("compiles");
        assert!(
            !lfs_both.applies(Path::new("a.mp4"), 10_000),
            "the opt-out is unchanged by the virtual list"
        );
        assert!(
            lfs_both.applies(Path::new("a.iso"), 10_000_000),
            "and a virtualized path is still an ordinary LFS candidate"
        );
        let virtual_both = VirtualPolicy::compile(&both).expect("compiles");
        assert_eq!(
            virtual_both.resolve(Path::new("a.mp4"), 10_000_000),
            Virtualization::Materialize,
            "and lfs_never never widens the virtual list"
        );
    }

    /// `resolve` is written against repository-relative paths, and the earlier
    /// version of this test proved that only for a root-anchored pattern, where
    /// it held by accident because the leading segments did not line up. A
    /// basename pattern is the dialect's default shape, and for one of those an
    /// absolute path used to answer `Virtual`: `match_string` drops the root
    /// component, so `/home/u/tgdrive/a.mp4` was matched as a relative path this
    /// repository does not contain.
    #[test]
    fn a_path_outside_the_repository_is_never_authorized_to_stay_away() {
        let dir = worktree(Some("*.mp4\n40-media/**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        for outside in [
            "/home/u/tgdrive/40-media/a.mp4",
            "/etc/a.mp4",
            "../../etc/a.mp4",
            "40-media/../../etc/a.mp4",
        ] {
            assert_eq!(
                policy.resolve(Path::new(outside), 10_000_000),
                Virtualization::Materialize,
                "{outside} is outside the frame the patterns are written against"
            );
        }
        // The relative form of the same name is still answered normally, so the
        // guard rejects the frame violation and not the pattern.
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Virtual
        );
    }

    /// Gitignore's leading `/` is the first spelling anyone coming from
    /// `.gitignore` reaches for, and it used to compile to a glob that matched
    /// nothing — silently. On the protective side that is the whole failure:
    /// the one file the user explicitly named was authorized to leave.
    #[test]
    fn a_leading_slash_anchors_at_the_root_instead_of_matching_nothing() {
        let dir = worktree(Some("40-media/**\n!/40-media/keep.mp4\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
            Virtualization::Materialize,
            "the protection the user wrote must actually protect"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/other.mp4"), 10_000_000),
            Virtualization::Virtual,
            "and it must protect only what it names"
        );
    }

    /// The point of the leading slash is that it does NOT then go back through
    /// the basename rule: `/keep.mp4` is the root's own file, not every
    /// `keep.mp4` in the repository.
    #[test]
    fn a_root_anchored_single_segment_does_not_become_a_basename_rule() {
        let dir = worktree(Some("/keep.mp4\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("keep.mp4"), 10_000_000),
            Virtualization::Virtual
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
            Virtualization::Materialize,
            "a rooted pattern must not reach a same-named file deeper in the tree"
        );
    }

    /// A trailing slash names a directory, so the rule covers what is under it.
    /// It does not anchor on its own, which is gitignore's own reading.
    #[test]
    fn a_trailing_slash_covers_the_subtree_at_any_depth() {
        let dir = worktree(Some("scratch/\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        for inside in ["scratch/a.mp4", "40-media/scratch/a.mp4"] {
            assert_eq!(
                policy.resolve(Path::new(inside), 10_000_000),
                Virtualization::Virtual,
                "{inside} is under a directory the policy named"
            );
        }
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Materialize
        );
    }

    /// The natural pairing — virtualize a zone, but not one folder inside it —
    /// protected nothing in any of its three spellings, because a directory
    /// pattern only ever matched the directory entry and `resolve` is never
    /// asked about one. Every file the user meant to keep was authorized to go.
    #[test]
    fn negating_a_directory_protects_the_files_inside_it_in_every_spelling() {
        for line in ["!40-media/secret", "!40-media/secret/", "!/40-media/secret"] {
            let dir = worktree(Some(&format!("40-media/**\n{line}\n")));
            let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
            assert_eq!(
                policy.resolve(Path::new("40-media/secret/tax.pdf"), 10_000_000),
                Virtualization::Materialize,
                "{line} must protect what is inside it"
            );
            assert_eq!(
                policy.resolve(Path::new("40-media/secret/deep/passport.jpg"), 10_000_000),
                Virtualization::Materialize,
                "{line} must protect the whole subtree, not one level"
            );
            assert_eq!(
                policy.resolve(Path::new("40-media/clip.mp4"), 10_000_000),
                Virtualization::Virtual,
                "{line} must not protect the rest of the zone"
            );
        }
    }

    /// The permissive half gets no subtree expansion, and the asymmetry is
    /// deliberate: widening a protection can only keep more bytes, while
    /// widening an authorization would let go of bytes nobody named.
    #[test]
    fn naming_a_folder_without_a_slash_does_not_authorize_its_contents() {
        let dir = worktree(Some("40-media\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Materialize,
            "a bare folder name authorizes nothing inside it; write `40-media/` or `40-media/**`"
        );
    }

    /// AD-123, and the git-lfs#3092 shape one layer up from the byte: a policy
    /// edit may widen what may leave and may never narrow what is kept. A host
    /// restating the repository's zone — believing it changed nothing — used to
    /// discard the repository's own exceptions along with the list.
    #[test]
    fn a_profile_list_cannot_drop_a_protection_the_committed_file_states() {
        let dir = worktree(Some("40-media/**\n!40-media/keep.mp4\n"));
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["40-media/**".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Profile);
        assert_eq!(
            policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
            Virtualization::Materialize,
            "the repository's protection survives a profile that replaced the zone"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/other.mp4"), 10_000_000),
            Virtualization::Virtual,
            "while the profile's own list still decides what may leave"
        );
    }

    /// Protections union in both directions: a machine may add one the
    /// repository never stated, and it applies alongside the file's.
    #[test]
    fn a_machine_may_add_a_protection_of_its_own() {
        let dir = worktree(Some("40-media/**\n!40-media/keep.mp4\n"));
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["40-media/**".to_owned(), "!40-media/mine.mp4".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        for kept in ["40-media/keep.mp4", "40-media/mine.mp4"] {
            assert_eq!(
                policy.resolve(Path::new(kept), 10_000_000),
                Virtualization::Materialize,
                "{kept} is protected by one source or the other"
            );
        }
    }

    /// A BOM is not whitespace, so `trim` leaves it attached and the first line
    /// stops being the pattern the user wrote. The dangerous half is a `!` line:
    /// prefixed by a BOM it no longer starts with `!`, so a line written to
    /// protect a file was filed as one that authorizes it.
    #[test]
    fn a_byte_order_mark_does_not_swallow_the_first_line_or_invert_it() {
        let dir = worktree(Some("\u{feff}40-media/**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Virtual,
            "a BOM must not silently kill the first pattern"
        );

        let dir = worktree(Some("\u{feff}!40-media/keep.mp4\n40-media/**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        assert_eq!(
            policy.resolve(Path::new("40-media/keep.mp4"), 10_000_000),
            Virtualization::Materialize,
            "and it must not turn a protection into an authorization"
        );
    }

    /// A mechanism must not be able to lose the file it is configured by.
    /// `crate::exclude` refuses to let a user pattern take `.keeper/keeper.toml`
    /// out of sync for the same reason. Here it is sharper: a virtualized
    /// `.keepervirtual` is a policy that erases itself, and the next `compile`
    /// would read `version https://git-lfs.github.com/spec/v1` as a pattern.
    #[test]
    fn keepers_own_control_files_are_never_authorized_to_stay_away() {
        let dir = worktree(Some("**\n"));
        let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
        for control in [
            VIRTUAL_PATTERN_FILE,
            ".gitattributes",
            "40-media/.gitattributes",
            ".gitignore",
            ".gitmodules",
            ".lfsconfig",
            ".keeper/keeper.toml",
            ".keeper/keeper.hesperia.toml",
            ".git/config",
        ] {
            assert_eq!(
                policy.resolve(Path::new(control), 10_000_000),
                Virtualization::Materialize,
                "{control} carries the rules; its own bytes can never be optional"
            );
        }
        // An ordinary file under the same broad pattern still resolves normally,
        // so the carve-out is a carve-out and not a blanket refusal.
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
            Virtualization::Virtual
        );
    }

    /// A profile list that parses to nothing is silence, not an override. One
    /// stray whitespace entry in a folder TOML array used to disable a
    /// repository's entire committed policy, with `tier()` still reporting that
    /// the profile was in force.
    #[test]
    fn a_profile_list_of_nothing_but_noise_does_not_mute_the_committed_file() {
        for noise in [vec!["   ".to_owned()], vec!["!".to_owned()]] {
            let dir = worktree(Some("40-media/**\n"));
            let mut p = profile(dir.path());
            p.virtual_patterns = noise.clone();
            let policy = VirtualPolicy::compile(&p).expect("compiles");
            assert_eq!(
                policy.tier(),
                VirtualPolicyTier::PatternFile,
                "{noise:?} says nothing, so the committed file is still what is in force"
            );
            assert_eq!(
                policy.resolve(Path::new("40-media/a.mp4"), 10_000_000),
                Virtualization::Virtual
            );
        }
    }

    /// Punctuation with no pattern behind it says nothing, and must not be
    /// counted as the source having spoken — `tier()` claiming a policy that
    /// compiled to nothing is the one thing it exists not to do.
    #[test]
    fn punctuation_with_no_pattern_behind_it_is_not_a_policy() {
        for text in ["!\n", "!   \n", "/\n", "!/\n"] {
            let dir = worktree(Some(text));
            let policy = VirtualPolicy::compile(&profile(dir.path())).expect("compiles");
            assert_eq!(
                policy.tier(),
                VirtualPolicyTier::Unset,
                "{text:?} configured nothing, so no tier is in force"
            );
            assert_eq!(
                policy.resolve(Path::new("a.mp4"), 10_000_000),
                Virtualization::Materialize
            );
        }
    }

    /// A TOML array carries no comments — the format has its own — so a `#` in
    /// an entry is the first character of a path. Treating it as a remark made a
    /// path named `#drafts` unexpressible from the tier AD-132 calls canonical.
    #[test]
    fn a_hash_in_a_toml_entry_is_a_path_and_not_a_comment() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["#drafts/**".to_owned()];
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(policy.tier(), VirtualPolicyTier::Profile);
        assert_eq!(
            policy.resolve(Path::new("#drafts/a.mp4"), 10_000_000),
            Virtualization::Virtual,
            "the entry names a folder that really starts with a hash"
        );
    }

    /// The refusal has to name the list and quote the line as typed, `!` and
    /// all, for a protection as much as for an authorization — a user told
    /// `[unclosed` is bad while their line reads `![unclosed` looks in the wrong
    /// place, and an anchored glob keeper derived is not a line anyone wrote.
    #[test]
    fn a_malformed_protection_in_a_profile_entry_names_that_list() {
        let dir = worktree(Some("40-media/**\n"));
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["40-media/**".to_owned(), "![unclosed".to_owned()];
        let err = VirtualPolicy::compile(&p).expect_err("must refuse");
        let text = format!("{err}");
        assert!(
            text.contains("virtualPatterns") && text.contains("![unclosed"),
            "the message must name the list and quote the line as typed, got: {text}"
        );
        assert!(
            !text.contains("**/!"),
            "and must not quote the glob keeper derived, got: {text}"
        );
    }

    /// The owner's own stored row, verbatim, and the defect this story exists
    /// for (Story 56.16):
    ///
    /// ```json
    /// {"name":"tgdrive-light","lfsMode":"materialize","lfsThresholdBytes":262144,"virtualPatterns":[],"virtualOverBytes":1048576}
    /// ```
    ///
    /// He named the folder for what he wanted from it and set a 1 MiB floor,
    /// which can only mean "don't fetch the big files". Without the fix
    /// `resolve` demanded `self.patterns.matches(rela)` before it would answer
    /// `Virtual`, and with no permissive line in any source the compiled set is
    /// empty and matches nothing — so the 4 MiB assertion below read
    /// `Materialize`, `tier()` read `Unset`, `authorizes_anything()` was false,
    /// and all 16 GB of the folder downloaded. A control that silently did
    /// nothing, with a form note describing a match that never happens.
    #[test]
    fn the_owners_stored_configuration_keeps_his_large_files_away() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.lfs_mode = crate::profile::LfsMode::Materialize;
        p.lfs_threshold_bytes = 262_144;
        p.virtual_patterns = vec![];
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        assert_eq!(
            policy.resolve(Path::new("40-media/holiday.mov"), 4 * 1024 * 1024),
            Virtualization::Virtual,
            "a file above the floor is what the floor was set to keep away"
        );
        assert_eq!(
            policy.resolve(Path::new("10-notes/scan.png"), 64 * 1024),
            Virtualization::Materialize,
            "and one below it is still fetched: the floor is the whole selector"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/exact.mov"), 1024 * 1024),
            Virtualization::Virtual,
            "exactly at the floor stays away — the boundary is inclusive, as \
             `LfsPolicy`'s threshold is"
        );
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::SizeFloor,
            "the floor is the source that decided, and the surface must be able \
             to say so rather than report nothing configured"
        );
        assert!(
            policy.authorizes_anything(),
            "the engine's release gate skips a policy that authorizes nothing, \
             so a floor that selects has to answer yes here or it never runs"
        );
    }

    /// A floor that selects on its own is still only an authorization, so a
    /// protection committed to the repository beats it exactly as it beats a
    /// pattern (AD-123, Story 56.16).
    ///
    /// This is the assertion that keeps the new selector inside the existing
    /// safety rule rather than beside it. Written before the fix it passed
    /// vacuously — nothing was virtual at all — so it is only load-bearing
    /// once the floor selects: a fix that answered `Virtual` from the floor
    /// *before* consulting `never` would have dehydrated the one zone the
    /// repository explicitly named.
    #[test]
    fn a_committed_protection_still_wins_over_a_floor_that_selects_on_its_own() {
        let dir = worktree(Some("!30-masters\n"));
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        assert_eq!(
            policy.resolve(Path::new("30-masters/a.mov"), 4 * 1024 * 1024),
            Virtualization::Materialize,
            "the committed protection wins over the floor, unconditionally"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mov"), 4 * 1024 * 1024),
            Virtualization::Virtual,
            "and it protects only what it names: the floor still selects the rest"
        );
    }

    /// The same, from the machine's own list — and the tier is the interesting
    /// half (Story 56.16 over 56.14).
    ///
    /// A `!`-only list does not override anything (Story 56.14: the override is
    /// decided on the permissive half alone), so the effective permissive set
    /// is empty and the floor is what selects. `tier()` must therefore say
    /// `SizeFloor` and not `Profile`: `Profile` would claim `virtualPatterns`
    /// decided what may stay away, which is the empty list, and a regression
    /// that stopped the floor selecting could then still pass a tier assertion.
    /// Without the fix this test failed on the `40-media` line — `Materialize`,
    /// nothing selected anything — and on the tier, which read `Profile`.
    #[test]
    fn a_profile_protection_still_wins_over_a_floor_that_selects_on_its_own() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_patterns = vec!["!30-masters".into()];
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        assert_eq!(
            policy.resolve(Path::new("30-masters/a.mov"), 4 * 1024 * 1024),
            Virtualization::Materialize,
            "the machine's own protection wins over the floor too"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mov"), 4 * 1024 * 1024),
            Virtualization::Virtual,
            "and the floor selects everything it did not name"
        );
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::SizeFloor,
            "a `!`-only list overrides nothing, so the permissive set is empty \
             and the floor is the source that decided"
        );
    }

    /// The floor never widens a zone some source already named (Story 56.16).
    ///
    /// `floor_selects` is computed from the *effective* permissive set, after
    /// precedence has run, so a folder that names files keeps naming exactly
    /// those and the floor keeps its old job of holding the small ones back.
    /// The `50-iso` line is what a floor computed before precedence — or one
    /// that ignored the pattern set entirely — would have failed: it would
    /// have read `Virtual` for a zone the committed file never mentions, which
    /// is a folder silently dehydrating on upgrade.
    #[test]
    fn a_floor_never_widens_a_zone_a_pattern_file_already_named() {
        let dir = worktree(Some("40-media/**\n"));
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        assert_eq!(
            policy.resolve(Path::new("50-iso/big.iso"), 4 * 1024 * 1024),
            Virtualization::Materialize,
            "a zone no source named must not gain a selector from the floor"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mov"), 4 * 1024 * 1024),
            Virtualization::Virtual,
            "the named zone is unchanged"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/small.mov"), 64 * 1024),
            Virtualization::Materialize,
            "and inside it the floor still does its old job of holding the \
             small ones back"
        );
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::PatternFile,
            "the file supplied the list in force, so that is still the file to edit"
        );
    }

    /// A floor of `0` with no patterns says nothing, and has to keep saying
    /// nothing (Story 56.16).
    ///
    /// This is the other silent reading, and the one the fix must not create.
    /// `0` is the default every profile ever written carries, and it is the
    /// documented spelling of "no floor" — so if the floor is now what selects,
    /// zero must select nothing at all. A `floor_selects` computed as
    /// `patterns.is_empty()` alone would have made every unconfigured folder in
    /// existence answer `Virtual` for every LFS path on the next sync, which is
    /// the whole 16 GB failure inverted and running on everybody's machine.
    #[test]
    fn a_floor_of_zero_with_no_patterns_still_says_nothing() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 0;
        assert!(
            p.virtual_patterns.is_empty(),
            "the default is an empty list"
        );
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::Unset,
            "no source said anything, and a zero floor is not a source saying \
             something"
        );
        assert!(
            !policy.authorizes_anything(),
            "so there is no point consulting this policy at all"
        );
        assert_eq!(
            policy.resolve(Path::new("40-media/a.mov"), 10_000_000),
            Virtualization::Materialize,
            "and nothing of any size may stay away"
        );
    }

    /// The sharpest hazard the new selector opens (Story 56.16).
    ///
    /// A floor-only policy authorizes by size and by nothing else, so it names
    /// no path and excludes none either — and a repository whose own
    /// `.gitattributes` had grown past the floor would have had its LFS routing
    /// rules turned into pointer text, `.keepervirtual` would have become a
    /// policy that erased itself, and the next `compile` would read
    /// `version https://git-lfs.github.com/spec/v1` as a pattern. The control
    /// carve-out already sits ahead of every other term in `resolve` and this
    /// pins it there against a fix that answered from the floor first.
    #[test]
    fn a_control_file_is_never_virtual_under_a_floor_that_selects_on_its_own() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::SizeFloor,
            "the fixture really is a floor-only policy"
        );

        for control in [VIRTUAL_PATTERN_FILE, ".gitattributes", ".lfsconfig"] {
            assert_eq!(
                policy.resolve(Path::new(control), 4 * 1024 * 1024),
                Virtualization::Materialize,
                "{control} carries the rules; a floor must not be able to make \
                 its own bytes optional"
            );
        }
    }

    /// The frame guard survives the new selector too (Story 56.16).
    ///
    /// `exclude::match_string` silently drops a root component, which is why an
    /// absolute path used to be matched as a relative one; under a floor-only
    /// policy there is no pattern to mismatch at all, so *nothing but* this
    /// guard stands between a path outside the repository and a `Virtual`
    /// answer. Anything the caller hands over that is not repository-relative
    /// has to be refused on the frame violation, not on the size.
    #[test]
    fn a_path_outside_the_repository_is_still_never_virtual_under_a_floor() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");

        for outside in ["/home/u/tgdrive/40-media/a.mov", "40-media/../../etc/a.mov"] {
            assert_eq!(
                policy.resolve(Path::new(outside), 4 * 1024 * 1024),
                Virtualization::Materialize,
                "{outside} is outside the frame the policy is written against, \
                 and a floor-only policy has no pattern to reject it for"
            );
        }
    }

    /// A path that names nothing is not a path a floor may select (Story
    /// 56.16).
    ///
    /// `""` and `"."` are relative and carry no `..`, so the frame guard used
    /// to admit them, and they resolved `Materialize` only by accident:
    /// `exclude::match_string` renders both to the empty string and
    /// `PatternSet::matches` refuses an empty candidate. `resolve`'s
    /// `floor_selects ||` short-circuits ahead of the pattern set, so under a
    /// floor-only policy both answered `Virtual` — the repository root itself
    /// classified as releasable. No byte is deleted today, because
    /// `release_target` refuses an empty subpath further down, but the
    /// classification is already wrong at the point it is made, and leaning on
    /// a downstream guard is exactly what this module refuses to do: the
    /// consumers that reach `resolve` do it with a ledger-supplied
    /// `Path::new(&row.path)`, whose contents this module does not get to
    /// choose.
    #[test]
    fn a_path_with_no_components_is_not_selected_by_a_floor() {
        let dir = worktree(None);
        let mut p = profile(dir.path());
        p.virtual_over_bytes = 1024 * 1024;
        let policy = VirtualPolicy::compile(&p).expect("compiles");
        assert_eq!(
            policy.tier(),
            VirtualPolicyTier::SizeFloor,
            "the fixture really is a floor-only policy, or the assertions below \
             would pass with nothing selecting anything at all"
        );

        for nothing in ["", "."] {
            assert_eq!(
                policy.resolve(Path::new(nothing), 4 * 1024 * 1024),
                Virtualization::Materialize,
                "{nothing:?} names no file in the repository, so there is \
                 nothing here for a floor to authorize"
            );
        }
    }
}
