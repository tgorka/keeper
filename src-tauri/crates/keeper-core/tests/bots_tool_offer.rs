//! The two decisions the live turn hands to `keeper-core` before a request
//! goes out (Epic 61's transport: Stories 61.10 and 61.11).
//!
//! Both are pure and both are the part of the wiring that must not be wrong:
//! [`offer_tools`] answers "what does this turn put in `tools`, and why not",
//! and [`context_targets`] answers "which context files may this turn read".
//! The shell that calls them does not compile on Linux, so every arm is pinned
//! here — one test per arm — and the shell is left with nothing to decide.

use keeper_core::bots::context_files::{context_targets, OKF_DIGEST};
use keeper_core::bots::grant::{
    Grant, GrantMode, GrantScope, ToolTarget, GRANTS_ALL_NONE_NO_TOOLS, HERMES_RUNS_ITS_OWN_TOOLS,
    MODEL_HAS_NO_TOOLS, NO_GRANT_NO_TOOLS, TOOLS_CAPABILITY_UNKNOWN,
};
use keeper_core::bots::tools::{default_profile_id, offer_tools, ToolOffer};
use keeper_core::bots::ProviderKind;

fn grant(id: &str, scope: GrantScope, mode: GrantMode) -> Grant {
    Grant {
        id: id.to_owned(),
        provider_id: "prov".to_owned(),
        bot_id: Some("bot".to_owned()),
        scope,
        mode,
        created_ms: 1,
    }
}

fn subtree(profile: &str, subpath: &str, mode: GrantMode) -> Grant {
    grant(
        &format!("g-{profile}-{subpath}"),
        GrantScope::Subtree {
            profile_id: profile.to_owned(),
            subpath: subpath.to_owned(),
        },
        mode,
    )
}

fn profile(profile: &str, mode: GrantMode) -> Grant {
    grant(
        &format!("g-{profile}"),
        GrantScope::Profile {
            profile_id: profile.to_owned(),
        },
        mode,
    )
}

fn names(offer: &ToolOffer) -> Vec<String> {
    offer.specs().iter().map(|spec| spec.name.clone()).collect()
}

fn withheld_reason(offer: &ToolOffer) -> &'static str {
    match offer {
        ToolOffer::Withheld { reason } => reason,
        ToolOffer::Offered { .. } => panic!("expected nothing on offer, got {offer:?}"),
    }
}

// ---------------------------------------------------------------------------
// offer_tools — one test per arm
// ---------------------------------------------------------------------------

#[test]
fn hermes_is_offered_nothing_whatever_the_grants_say() {
    let grants = vec![profile("drive-1", GrantMode::Write)];
    let offer = offer_tools(ProviderKind::Hermes, Some(true), &grants);
    assert_eq!(withheld_reason(&offer), HERMES_RUNS_ITS_OWN_TOOLS);
    assert!(offer.specs().is_empty());
    assert!(!offer.is_offered());
}

#[test]
fn an_ollama_model_that_states_tools_is_offered_the_grant_vocabulary() {
    let grants = vec![profile("drive-1", GrantMode::Read)];
    let offer = offer_tools(ProviderKind::Ollama, Some(true), &grants);
    match &offer {
        ToolOffer::Offered {
            mode,
            specs,
            warning,
        } => {
            assert_eq!(*mode, GrantMode::Read);
            assert_eq!(specs.len(), 5, "a read grant offers the five readers");
            assert!(warning.is_none(), "a stated capability carries no warning");
        }
        ToolOffer::Withheld { .. } => panic!("{offer:?}"),
    }
    let offered = names(&offer);
    assert!(offered.contains(&"drive_read".to_owned()));
    assert!(!offered.contains(&"drive_write".to_owned()));
}

#[test]
fn an_ollama_model_that_states_no_tools_is_offered_nothing() {
    let grants = vec![profile("drive-1", GrantMode::Write)];
    let offer = offer_tools(ProviderKind::Ollama, Some(false), &grants);
    assert_eq!(withheld_reason(&offer), MODEL_HAS_NO_TOOLS);
}

#[test]
fn an_unknown_capability_is_offered_with_the_warning_not_refused() {
    let grants = vec![profile("drive-1", GrantMode::Write)];
    let offer = offer_tools(ProviderKind::Ollama, None, &grants);
    match &offer {
        ToolOffer::Offered { warning, specs, .. } => {
            assert_eq!(*warning, Some(TOOLS_CAPABILITY_UNKNOWN));
            assert_eq!(specs.len(), 7, "a write grant offers every verb");
        }
        ToolOffer::Withheld { .. } => panic!("unknown is not false (AD-27): {offer:?}"),
    }
}

#[test]
fn grants_all_at_mode_none_offer_nothing() {
    let grants = vec![
        profile("drive-1", GrantMode::None),
        subtree("drive-1", "notes", GrantMode::None),
    ];
    let offer = offer_tools(ProviderKind::Ollama, Some(true), &grants);
    assert_eq!(withheld_reason(&offer), GRANTS_ALL_NONE_NO_TOOLS);
}

#[test]
fn no_grant_offers_nothing() {
    let offer = offer_tools(ProviderKind::Ollama, Some(true), &[]);
    assert_eq!(withheld_reason(&offer), NO_GRANT_NO_TOOLS);
}

#[test]
fn the_widest_live_mode_picks_the_vocabulary() {
    // A `none` beside a `write`: the offer is the widest; `decide` per call is
    // what keeps the `none` subtree closed.
    let grants = vec![
        subtree("drive-1", "notes/private", GrantMode::None),
        profile("drive-1", GrantMode::Write),
    ];
    let offer = offer_tools(ProviderKind::Ollama, Some(true), &grants);
    assert!(names(&offer).contains(&"drive_write".to_owned()));

    let grants = vec![
        subtree("drive-1", "notes", GrantMode::Read),
        profile("drive-1", GrantMode::None),
    ];
    let offer = offer_tools(ProviderKind::Ollama, Some(true), &grants);
    assert_eq!(offer.specs().len(), 5);
}

// ---------------------------------------------------------------------------
// context_targets
// ---------------------------------------------------------------------------

fn paths(targets: &[ToolTarget]) -> Vec<String> {
    targets.iter().map(ToolTarget::display_path).collect()
}

#[test]
fn a_subtree_grant_reads_context_inside_the_subtree_only() {
    let grants = vec![subtree("drive-1", "notes/2026", GrantMode::Read)];
    let targets = context_targets(&grants, &["drive-1", "drive-2"]);
    let got = paths(&targets);
    assert_eq!(
        got.first().map(String::as_str),
        Some("drive-1/notes/2026/AGENTS.md"),
        "nearest first"
    );
    assert!(
        got.iter()
            .all(|path| path.starts_with("drive-1/notes/2026/")),
        "the walk climbs no higher than the grant: {got:?}"
    );
    assert!(
        !got.contains(&"drive-1/AGENTS.md".to_owned()),
        "the profile root's rules are outside the grant"
    );
    assert!(
        !got.contains(&format!("drive-1/{OKF_DIGEST}")),
        "the digest sits at the profile root, which this grant does not cover"
    );
}

#[test]
fn a_profile_grant_reads_the_profile_root_and_the_digest() {
    let grants = vec![profile("drive-1", GrantMode::Write)];
    let got = paths(&context_targets(&grants, &["drive-1", "drive-2"]));
    assert_eq!(
        got,
        vec![
            "drive-1/AGENTS.md",
            "drive-1/CLAUDE.md",
            "drive-1/GEMINI.md",
            "drive-1/.cursorrules",
            &format!("drive-1/{OKF_DIGEST}"),
        ]
    );
}

#[test]
fn a_drive_grant_reads_every_profile_root() {
    let grants = vec![grant("g", GrantScope::Drive, GrantMode::Read)];
    let got = paths(&context_targets(&grants, &["drive-1", "drive-2"]));
    assert!(got.contains(&"drive-1/AGENTS.md".to_owned()));
    assert!(got.contains(&"drive-2/AGENTS.md".to_owned()));
    assert_eq!(got.len(), 10);
}

#[test]
fn no_grant_and_a_none_grant_read_nothing() {
    assert!(context_targets(&[], &["drive-1"]).is_empty());
    let none = vec![profile("drive-1", GrantMode::None)];
    assert!(context_targets(&none, &["drive-1"]).is_empty());
}

#[test]
fn a_none_subtree_hides_its_rules_under_a_wider_read() {
    let grants = vec![
        profile("drive-1", GrantMode::Read),
        subtree("drive-1", "notes/private", GrantMode::None),
    ];
    // The private subtree contributes nothing, even though its own root is a
    // walk root: the per-target `decide` refuses each candidate under it.
    let got = paths(&context_targets(&grants, &["drive-1"]));
    assert!(
        got.iter().all(|path| !path.contains("notes/private")),
        "{got:?}"
    );
    assert!(got.contains(&"drive-1/AGENTS.md".to_owned()));
}

#[test]
fn overlapping_grants_read_each_file_once() {
    let grants = vec![
        subtree("drive-1", "notes", GrantMode::Read),
        profile("drive-1", GrantMode::Read),
    ];
    let got = paths(&context_targets(&grants, &["drive-1"]));
    let mut deduped = got.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(got.len(), deduped.len(), "no path twice: {got:?}");
    assert_eq!(
        got.first().map(String::as_str),
        Some("drive-1/notes/AGENTS.md"),
        "the narrower grant's nearest file is spent first"
    );
}

// ---------------------------------------------------------------------------
// default_profile_id
// ---------------------------------------------------------------------------

#[test]
fn an_unqualified_path_resolves_against_the_granted_profile() {
    let grants = vec![
        grant("d", GrantScope::Drive, GrantMode::Read),
        subtree("drive-2", "notes", GrantMode::Read),
    ];
    assert_eq!(
        default_profile_id(&grants, &["drive-1", "drive-2"]).as_deref(),
        Some("drive-2"),
        "the grant that names a folder names the default"
    );
    let drive_only = vec![grant("d", GrantScope::Drive, GrantMode::Write)];
    assert_eq!(
        default_profile_id(&drive_only, &["drive-1", "drive-2"]).as_deref(),
        Some("drive-1")
    );
    let none_only = vec![profile("drive-1", GrantMode::None)];
    assert_eq!(default_profile_id(&none_only, &["drive-1"]), None);
    assert_eq!(default_profile_id(&[], &["drive-1"]), None);
}
