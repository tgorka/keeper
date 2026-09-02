//! The wire shape the tool-call row and the context disclosure render (Story
//! 61.11, FR-388, FR-391, NFR-49).
//!
//! The rendering itself is asserted in `src/components/bots/bot-tool-call.test.tsx`
//! and `bot-context-note.test.tsx`; this file asserts the half those tests
//! cannot see — that the projection carries the bound, the provenance and the
//! refusal **out of** `keeper_core::bots` without inventing or losing any of
//! them. A row that renders `truncatedAtBytes` perfectly is still a row that
//! says nothing if the projection dropped it.

use keeper_core::bots::context_files::{merge, LoadedContext, MAX_CONTEXT_TOTAL_BYTES};
use keeper_core::bots::tools::{okf_facts, ToolCallRecord, ToolName, ToolOutcome};
use keeper_core::vm::{BotContextBundleVm, BotToolCallVm, BotToolOutcomeKind};

/// One record as the loop writes it for a successful call.
fn record() -> ToolCallRecord {
    ToolCallRecord {
        id: "call_1".to_owned(),
        requested_name: "drive_read".to_owned(),
        name: Some(ToolName::Read),
        display_path: Some("work/notes/a.md".to_owned()),
        refusal: None,
        grant_denied: false,
    }
}

/// NFR-49: a bounded read reaches the row with its bound, and the row's
/// `result` is the same text the model was handed — the disclosure included, so
/// the two cannot disagree.
#[test]
fn a_bounded_read_projects_the_bound_and_the_body_the_model_read() {
    let outcome = ToolOutcome::Text {
        body: "the first part".to_owned(),
        truncated_at: Some(65_536),
        of_bytes: Some(1_258_291),
        okf: None,
    };

    let vm = BotToolCallVm::compose(&record(), r#"{"path":"work/notes/a.md"}"#, Some(&outcome));

    assert_eq!(vm.outcome, Some(BotToolOutcomeKind::Text));
    assert_eq!(vm.truncated_at_bytes, Some(65_536));
    assert_eq!(vm.of_bytes, Some(1_258_291));
    assert_eq!(vm.bytes, Some(14));
    assert_eq!(vm.arguments, r#"{"path":"work/notes/a.md"}"#);
    assert!(
        vm.result.contains("Truncated at 65536 bytes of 1258291"),
        "the row shows the very text the model read: {}",
        vm.result
    );
}

/// A whole read carries **no** bound. A row that always has one is a row whose
/// disclosure means nothing.
#[test]
fn a_whole_read_projects_no_bound_and_no_provenance_it_did_not_have() {
    let outcome = ToolOutcome::Text {
        body: "# Just a heading\n".to_owned(),
        truncated_at: None,
        of_bytes: None,
        okf: okf_facts("# Just a heading\n"),
    };

    let vm = BotToolCallVm::compose(&record(), "{}", Some(&outcome));

    assert_eq!(vm.truncated_at_bytes, None);
    assert_eq!(vm.of_bytes, None);
    // No frontmatter, so no provenance — the row must not sprout one.
    assert!(vm.okf.is_none());
}

/// FR-391: the one thing keeper can say that a generic filesystem tool cannot,
/// and the one it must not get wrong — a note only a model checked is not a
/// note a person verified.
#[test]
fn a_model_checked_note_never_projects_as_person_verified() {
    let body = "---\ntype: Note\ngenerated:\n  by: claude/opus-5\n  at: 2026-01-01T00:00:00Z\n\
                verified:\n  - by: agent:reviewer\n    at: 2026-01-02T00:00:00Z\n---\nbody\n";
    let outcome = ToolOutcome::Text {
        body: body.to_owned(),
        truncated_at: None,
        of_bytes: None,
        okf: okf_facts(body),
    };

    let vm = BotToolCallVm::compose(&record(), "{}", Some(&outcome));
    let okf = vm
        .okf
        .expect("a note with frontmatter projects its provenance");

    assert_eq!(okf.doc_type.as_deref(), Some("Note"));
    assert_eq!(okf.generated_by.as_deref(), Some("claude/opus-5"));
    assert_eq!(okf.generated_actor.as_deref(), Some("a tool"));
    assert_eq!(okf.verified_actor.as_deref(), Some("an agent"));
    assert!(
        !okf.human_reviewed,
        "an agent's review is not a person's review"
    );
    assert!(okf.sentence.contains("checked but not by a person"));
}

/// A person's review reaches the row as one, with the actor word the model was
/// given.
#[test]
fn a_person_reviewed_note_projects_the_person() {
    let body = "---\ntype: Decision\nverified:\n  - by: human:alice\n    at: 2026-01-02T00:00:00Z\n---\nbody\n";
    let outcome = ToolOutcome::Text {
        body: body.to_owned(),
        truncated_at: None,
        of_bytes: None,
        okf: okf_facts(body),
    };

    let okf = BotToolCallVm::compose(&record(), "{}", Some(&outcome))
        .okf
        .expect("provenance");

    assert!(okf.human_reviewed);
    assert_eq!(okf.verified_by.as_deref(), Some("human:alice"));
    assert_eq!(okf.verified_actor.as_deref(), Some("a person"));
}

/// A refusal reaches the row in the refusing layer's own words, and the row
/// keeps the one distinction a person can act on.
#[test]
fn a_refusal_projects_verbatim_and_says_whose_refusal_it_was() {
    let reason = "No grant covers work/private.".to_owned();
    let refused = ToolCallRecord {
        refusal: Some(reason.clone()),
        grant_denied: true,
        ..record()
    };

    let vm = BotToolCallVm::compose(
        &refused,
        "{}",
        Some(&ToolOutcome::Refused {
            reason: reason.clone(),
        }),
    );

    assert_eq!(vm.outcome, Some(BotToolOutcomeKind::Refused));
    assert_eq!(vm.refusal.as_deref(), Some(reason.as_str()));
    assert!(vm.grant_denied);
    assert_eq!(vm.bytes, None, "a refusal read nothing");
}

/// A listing's bound is a count, not a byte figure, and both numbers reach the
/// row.
#[test]
fn a_bounded_listing_projects_its_counts() {
    let outcome = ToolOutcome::Entries {
        subpath: "work".to_owned(),
        entries: Vec::new(),
        truncated_at: Some(200),
        of_entries: 1240,
    };

    let vm = BotToolCallVm::compose(&record(), "{}", Some(&outcome));

    assert_eq!(vm.outcome, Some(BotToolOutcomeKind::Entries));
    assert_eq!(vm.truncated_at_entries, Some(200));
    assert_eq!(vm.of_entries, Some(1240));
    assert_eq!(vm.truncated_at_bytes, None);
}

/// FR-391: the disclosure keeps the prompt's render order, marks a prefix as a
/// prefix, and — the part that changes an answer — names the instruction file
/// the budget dropped, in `keeper-core`'s own sentence.
#[test]
fn the_context_bundle_projects_render_order_and_the_dropped_file() {
    let big = "n".repeat(40_000);
    let bundle = merge(vec![
        LoadedContext {
            subpath: "work/AGENTS.md".to_owned(),
            text: big.clone(),
            of_bytes: 40_000,
        },
        LoadedContext {
            subpath: "AGENTS.md".to_owned(),
            text: big.clone(),
            of_bytes: 40_000,
        },
        LoadedContext {
            subpath: "CLAUDE.md".to_owned(),
            text: big,
            of_bytes: 40_000,
        },
    ]);

    let vm = BotContextBundleVm::compose(&bundle);

    // Root first, nearest last: the order the prompt put them in.
    let order: Vec<&str> = vm.files.iter().map(|file| file.subpath.as_str()).collect();
    assert_eq!(order, vec!["AGENTS.md", "work/AGENTS.md"]);
    assert!(
        vm.files.iter().all(|file| file.truncated),
        "each of these was cut by a cap, and each says so"
    );
    assert_eq!(
        vm.total_bytes,
        u64::try_from(MAX_CONTEXT_TOTAL_BYTES).expect("the budget fits a u64")
    );
    // The third file was found and left out, and the row says which and why.
    assert_eq!(vm.skipped.len(), 1);
    let skip = &vm.skipped[0];
    assert_eq!(skip.subpath, "CLAUDE.md");
    assert!(skip.over_budget);
    assert_eq!(skip.of_bytes, Some(40_000));
    assert!(skip.sentence.contains("context budget was already spent"));
    // And the preamble is the one the prompt carries, not a second wording.
    assert_eq!(
        vm.preamble,
        keeper_core::bots::context_files::UNTRUSTED_PREAMBLE
    );
}

/// An empty context file is a skip, and it is **not** the same news as a
/// dropped instruction file.
#[test]
fn an_empty_context_file_is_not_reported_as_a_dropped_one() {
    let bundle = merge(vec![LoadedContext {
        subpath: "work/GEMINI.md".to_owned(),
        text: String::new(),
        of_bytes: 0,
    }]);

    let vm = BotContextBundleVm::compose(&bundle);

    assert!(vm.files.is_empty());
    assert_eq!(vm.skipped.len(), 1);
    assert!(!vm.skipped[0].over_budget);
    assert_eq!(vm.skipped[0].of_bytes, None);
    assert_eq!(vm.skipped[0].sentence, "work/GEMINI.md is empty.");
}
