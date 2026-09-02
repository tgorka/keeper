//! Story 61.12 — an image you can paste, a path you can open.
//!
//! Two halves, tested the way each one can fail.
//!
//! The **image gate** is pure and its whole risk is a tri-state collapsed into
//! a boolean somewhere, so every one of the three vision answers gets its own
//! named test, and so does each cap — with the refusal's own wording asserted,
//! because a cap whose sentence omits the number is a cap the person cannot
//! act on.
//!
//! The **path scanner** is where the real defect lives: a scanner that reads a
//! code example as a deliverable, or a grant check that is skipped because the
//! path "looks local", is how this feature would turn into keeper opening a
//! file nobody granted. Those two get the most tests, and the grant half is
//! exercised through [`keeper_core::bots::grant::decide`]'s real algebra with
//! real [`Grant`] rows rather than a stand-in.

use std::path::{Path, PathBuf};

use keeper_core::bots::chat::{build_body, ChatMessage, ChatRequest, ContentPart, Role};
use keeper_core::bots::deliverable::{
    accept_image, discard_staged, image_content_part, image_data_uri, image_offer, read_staged,
    refuse_mime, refuse_oversize, refuse_too_many, refuse_vision, resolve_deliverables, scan_paths,
    stage_image, warn_vision_unknown, Deliverable, DeliverableControl, DeliverableRoot, ImageOffer,
    IMAGE_MIMES, MAX_IMAGES_PER_MESSAGE, MAX_IMAGE_BYTES, NO_CONTROL_MISSING, NO_CONTROL_NO_GRANT,
    NO_CONTROL_OUTSIDE_DRIVE,
};
use keeper_core::bots::grant::{Grant, GrantMode, GrantScope};
use keeper_core::bots::ProviderKind;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A scratch directory no other test can land in — `bots/store.rs`'s helper and
/// its reason, restated: the pid, a nanosecond stamp and a process-wide
/// counter, because two threads asking inside one clock tick otherwise collide.
fn temp_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-deliverable-test-{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

/// The one profile every path test resolves against.
fn roots() -> Vec<DeliverableRoot> {
    vec![DeliverableRoot {
        profile_id: "drive".to_owned(),
        local_path: PathBuf::from("/home/ada/Drive"),
    }]
}

/// A live grant on `drive`, at `scope`, in `mode`.
fn grant(scope: GrantScope, mode: GrantMode) -> Grant {
    Grant {
        id: "g1".to_owned(),
        provider_id: "p1".to_owned(),
        bot_id: None,
        scope,
        mode,
        created_ms: 0,
    }
}

/// Everything exists.
fn present(_: &Path) -> bool {
    true
}

/// Nothing exists.
fn absent(_: &Path) -> bool {
    false
}

/// The reason on a mention that got no control, or a panic naming what it got.
fn reason(item: &Deliverable) -> &str {
    match &item.control {
        DeliverableControl::None { reason } => reason,
        DeliverableControl::Reveal { .. } => panic!("expected no control, got a reveal"),
    }
}

// ---------------------------------------------------------------------------
// The vision tri-state (FR-392)
// ---------------------------------------------------------------------------

#[test]
fn deliverable_vision_true_offers_with_nothing_to_disclose() {
    assert_eq!(image_offer(Some(true), "llava:13b"), ImageOffer::Offer);
    assert_eq!(
        accept_image(Some(true), "llava:13b", "image/png", 1024, 0),
        Ok(ImageOffer::Offer)
    );
}

#[test]
fn deliverable_vision_unknown_offers_with_a_warning_naming_the_model() {
    let offer = image_offer(None, "mystery:7b");
    let ImageOffer::OfferWithWarning(sentence) = offer else {
        panic!("unknown must offer, not refuse or offer silently");
    };
    assert_eq!(sentence, warn_vision_unknown("mystery:7b"));
    assert!(sentence.contains("mystery:7b"));
    assert!(sentence.contains("could not read"));
    // And the paste itself goes through — unknown is not false (AD-27).
    assert!(accept_image(None, "mystery:7b", "image/png", 1024, 0).is_ok());
}

#[test]
fn deliverable_vision_false_refuses_and_names_the_model() {
    let offer = image_offer(Some(false), "llama4:8b");
    assert_eq!(offer, ImageOffer::Refuse(refuse_vision("llama4:8b")));
    let refusal = accept_image(Some(false), "llama4:8b", "image/png", 1024, 0)
        .expect_err("a model that cannot see must refuse the paste");
    assert!(
        refusal.contains("llama4:8b"),
        "the refusal must name the model: {refusal}"
    );
    assert!(refusal.contains("does not accept images"));
}

#[test]
fn deliverable_vision_is_checked_before_the_size_cap() {
    // A person pasting into a bot that cannot see must hear that, not that
    // their screenshot is large — a true sentence about the wrong problem.
    let refusal = accept_image(
        Some(false),
        "llama4:8b",
        "image/png",
        MAX_IMAGE_BYTES + 1,
        0,
    )
    .expect_err("must refuse");
    assert_eq!(refusal, refuse_vision("llama4:8b"));
}

// ---------------------------------------------------------------------------
// The caps (FR-392)
// ---------------------------------------------------------------------------

#[test]
fn deliverable_oversize_refusal_carries_both_numbers() {
    let refusal = accept_image(Some(true), "llava:13b", "image/png", MAX_IMAGE_BYTES + 1, 0)
        .expect_err("one byte over the cap must refuse");
    assert_eq!(refusal, refuse_oversize(MAX_IMAGE_BYTES + 1));
    assert!(
        refusal.contains("8.0 MB"),
        "the cap itself must be in the sentence: {refusal}"
    );
    assert!(
        refusal.contains("8.1 MB"),
        "the image's own size must be in the sentence: {refusal}"
    );
}

#[test]
fn deliverable_image_exactly_at_the_cap_is_accepted() {
    // The boundary is inclusive, so the refusal never fires on a file whose
    // size equals the number the sentence quotes.
    assert!(accept_image(Some(true), "llava:13b", "image/png", MAX_IMAGE_BYTES, 0).is_ok());
}

#[test]
fn deliverable_count_cap_refuses_the_next_one_with_the_count() {
    let refusal = accept_image(
        Some(true),
        "llava:13b",
        "image/png",
        1024,
        MAX_IMAGES_PER_MESSAGE,
    )
    .expect_err("one image past the count cap must refuse");
    assert_eq!(refusal, refuse_too_many());
    assert!(refusal.contains(&MAX_IMAGES_PER_MESSAGE.to_string()));
    // And the last permitted one is still permitted.
    assert!(accept_image(
        Some(true),
        "llava:13b",
        "image/png",
        1024,
        MAX_IMAGES_PER_MESSAGE - 1
    )
    .is_ok());
}

#[test]
fn deliverable_svg_is_not_an_attachable_image() {
    // Ollama has errored on SVG since v0.4.6 (research §7), and an SVG is a
    // document that can carry script.
    let refusal = accept_image(Some(true), "llava:13b", "image/svg+xml", 1024, 0)
        .expect_err("SVG must refuse");
    assert_eq!(refusal, refuse_mime("image/svg+xml"));
    assert!(!IMAGE_MIMES.contains(&"image/svg+xml"));
}

// ---------------------------------------------------------------------------
// Staging: the bytes never travel as base64 (AD-58)
// ---------------------------------------------------------------------------

#[test]
fn deliverable_staged_image_round_trips_through_the_filesystem() {
    let dir = temp_dir();
    let bytes = b"\x89PNG\r\n\x1a\nnot really a png".to_vec();
    let staged = stage_image(&dir, "image/png", &bytes).expect("stage");
    assert_eq!(staged.byte_len, bytes.len());
    assert_eq!(staged.mime, "image/png");
    assert_eq!(read_staged(&dir, &staged.id).expect("read back"), bytes);
    discard_staged(&dir, &staged.id);
    assert!(
        read_staged(&dir, &staged.id).is_err(),
        "a discarded image must not still be readable"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deliverable_staging_refuses_an_oversize_body_before_it_writes() {
    let dir = temp_dir();
    let bytes = vec![0u8; MAX_IMAGE_BYTES + 1];
    let error = stage_image(&dir, "image/png", &bytes).expect_err("must refuse");
    assert!(error.to_string().contains("8.0 MB"), "{error}");
    assert!(
        !dir.join("bots/attachments").exists(),
        "a refused paste must not create the staging folder"
    );
}

#[test]
fn deliverable_data_uri_exists_only_in_the_outbound_request_body() {
    let bytes = [0u8, 1, 2, 3];
    let uri = image_data_uri("image/png", &bytes);
    assert_eq!(uri, "data:image/png;base64,AAECAw==");
    let part = image_content_part("image/png", &bytes);
    assert_eq!(
        part,
        ContentPart::Image {
            url: uri.clone(),
            detail: None,
        }
    );

    // The shape that actually reaches each back end, from the quirk table —
    // an object on Hermes, a bare string on Ollama (research §2.5, §7).
    let request = ChatRequest {
        model: "m".to_owned(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: vec![ContentPart::Text("what is this".to_owned()), part],
            ..ChatMessage::default()
        }],
        ..ChatRequest::default()
    };
    let hermes = build_body(ProviderKind::Hermes, &request).expect("hermes body");
    assert_eq!(hermes["messages"][0]["content"][1]["image_url"]["url"], uri);
    let ollama = build_body(ProviderKind::Ollama, &request).expect("ollama body");
    assert_eq!(ollama["messages"][0]["content"][1]["image_url"], uri);
}

// ---------------------------------------------------------------------------
// The path scanner (FR-393)
// ---------------------------------------------------------------------------

#[test]
fn deliverable_finds_a_path_in_prose() {
    let reply = "I wrote the summary to /home/ada/Drive/notes/summary.md for you.";
    let found = scan_paths(reply);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].raw, "/home/ada/Drive/notes/summary.md");
    // The offsets point into the reply as received — keeper never rewrites it.
    assert_eq!(&reply[found[0].start..found[0].end], found[0].raw);
}

#[test]
fn deliverable_finds_two_paths_on_one_line() {
    let reply = "Compare /home/ada/Drive/a.md and /home/ada/Drive/b.md.";
    let found = scan_paths(reply);
    assert_eq!(
        found.iter().map(|m| m.raw.as_str()).collect::<Vec<_>>(),
        vec!["/home/ada/Drive/a.md", "/home/ada/Drive/b.md"]
    );
    // The trailing full stop belongs to the sentence, not to the file name.
    assert_eq!(&reply[found[1].start..found[1].end], "/home/ada/Drive/b.md");
}

#[test]
fn deliverable_ignores_a_path_inside_a_fenced_block() {
    let reply = "Run this:\n\n```sh\ncat /home/ada/Drive/secret.md\n```\n\nThat is all.";
    assert!(
        scan_paths(reply).is_empty(),
        "a path inside a fence is an example, not a deliverable: {:?}",
        scan_paths(reply)
    );
}

#[test]
fn deliverable_ignores_a_path_inside_an_inline_code_span() {
    let reply = "The config lives at `/home/ada/Drive/conf.toml` on this machine.";
    assert!(scan_paths(reply).is_empty(), "{:?}", scan_paths(reply));
}

#[test]
fn deliverable_reads_prose_around_a_fence_it_skipped() {
    let reply = "```\n/home/ada/Drive/inside.md\n```\nand /home/ada/Drive/outside.md is real.";
    let found = scan_paths(reply);
    assert_eq!(
        found.iter().map(|m| m.raw.as_str()).collect::<Vec<_>>(),
        vec!["/home/ada/Drive/outside.md"]
    );
}

#[test]
fn deliverable_scanner_is_not_fooled_by_a_url_or_a_bare_slash() {
    let reply = "See https://example.com/notes/a.md or the ratio 3/4 or just / on its own.";
    assert!(scan_paths(reply).is_empty(), "{:?}", scan_paths(reply));
}

#[test]
fn deliverable_never_rewrites_the_reply() {
    // The record is the reply. Everything this module returns is an offset
    // into it, so there is no shape in which a caller receives edited text.
    let reply = "Saved to /home/ada/Drive/x.md — and `/home/ada/Drive/y.md` is untouched.";
    let items = resolve_deliverables(
        reply,
        None,
        &roots(),
        &[grant(GrantScope::Drive, GrantMode::Read)],
        &present,
    );
    assert_eq!(items.len(), 1);
    assert_eq!(
        &reply[items[0].mention.start..items[0].mention.end],
        "/home/ada/Drive/x.md"
    );
}

// ---------------------------------------------------------------------------
// The grant check on a mentioned path (FR-393, AD-160)
// ---------------------------------------------------------------------------

#[test]
fn deliverable_inside_a_read_grant_offers_reveal() {
    let items = resolve_deliverables(
        "Written to /home/ada/Drive/notes/summary.md",
        None,
        &roots(),
        &[grant(
            GrantScope::Subtree {
                profile_id: "drive".to_owned(),
                subpath: "notes".to_owned(),
            },
            GrantMode::Read,
        )],
        &present,
    );
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].control,
        DeliverableControl::Reveal {
            profile_id: "drive".to_owned(),
            subpath: "notes/summary.md".to_owned(),
        }
    );
}

#[test]
fn deliverable_outside_every_grant_renders_as_text_with_a_reason() {
    let items = resolve_deliverables(
        "Written to /home/ada/Drive/private/ledger.md",
        None,
        &roots(),
        &[grant(
            GrantScope::Subtree {
                profile_id: "drive".to_owned(),
                subpath: "notes".to_owned(),
            },
            GrantMode::Read,
        )],
        &present,
    );
    assert_eq!(items.len(), 1);
    assert_eq!(reason(&items[0]), NO_CONTROL_NO_GRANT);
}

#[test]
fn deliverable_a_grant_set_to_no_access_offers_nothing() {
    let items = resolve_deliverables(
        "Written to /home/ada/Drive/notes/summary.md",
        None,
        &roots(),
        &[grant(GrantScope::Drive, GrantMode::None)],
        &present,
    );
    assert_eq!(reason(&items[0]), NO_CONTROL_NO_GRANT);
}

#[test]
fn deliverable_outside_every_synced_folder_offers_nothing() {
    let items = resolve_deliverables(
        "Look at /etc/hosts for the answer",
        None,
        &roots(),
        // A drive-wide grant, and it still buys nothing: the path is not in
        // the drive at all, so there is no scope that could cover it.
        &[grant(GrantScope::Drive, GrantMode::Write)],
        &present,
    );
    assert_eq!(items.len(), 1);
    assert_eq!(reason(&items[0]), NO_CONTROL_OUTSIDE_DRIVE);
}

#[test]
fn deliverable_a_sibling_folder_is_not_inside_the_grant() {
    // The segment rule, on the root match this time: `/home/ada/Drive-old` is
    // not inside `/home/ada/Drive`, and a `starts_with` would say it is.
    let items = resolve_deliverables(
        "Written to /home/ada/Drive-old/notes/a.md",
        None,
        &roots(),
        &[grant(GrantScope::Drive, GrantMode::Read)],
        &present,
    );
    assert_eq!(reason(&items[0]), NO_CONTROL_OUTSIDE_DRIVE);
}

#[test]
fn deliverable_expands_a_tilde_path_against_the_home_directory() {
    let items = resolve_deliverables(
        "It is at ~/Drive/notes/summary.md now",
        Some(Path::new("/home/ada")),
        &roots(),
        &[grant(GrantScope::Drive, GrantMode::Read)],
        &present,
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].mention.raw, "~/Drive/notes/summary.md");
    assert_eq!(items[0].absolute, "/home/ada/Drive/notes/summary.md");
    assert_eq!(
        items[0].control,
        DeliverableControl::Reveal {
            profile_id: "drive".to_owned(),
            subpath: "notes/summary.md".to_owned(),
        }
    );
}

#[test]
fn deliverable_a_tilde_path_with_no_home_is_outside_the_drive() {
    let items = resolve_deliverables(
        "It is at ~/Drive/notes/summary.md now",
        None,
        &roots(),
        &[grant(GrantScope::Drive, GrantMode::Read)],
        &present,
    );
    assert_eq!(reason(&items[0]), NO_CONTROL_OUTSIDE_DRIVE);
}

#[test]
fn deliverable_a_granted_path_that_is_not_there_offers_nothing() {
    let items = resolve_deliverables(
        "Written to /home/ada/Drive/notes/summary.md",
        None,
        &roots(),
        &[grant(GrantScope::Drive, GrantMode::Read)],
        &absent,
    );
    assert_eq!(items.len(), 1);
    assert_eq!(reason(&items[0]), NO_CONTROL_MISSING);
}

#[test]
fn deliverable_never_stats_a_path_no_grant_covers() {
    // keeper does not probe a location it has no permission to look at: a
    // reply author must not be able to use reveal-or-not as an existence
    // oracle over the drive.
    let probed = std::cell::RefCell::new(Vec::<PathBuf>::new());
    let items = resolve_deliverables(
        "Written to /home/ada/Drive/private/ledger.md",
        None,
        &roots(),
        &[],
        &|path| {
            probed.borrow_mut().push(path.to_path_buf());
            true
        },
    );
    assert_eq!(reason(&items[0]), NO_CONTROL_NO_GRANT);
    assert!(
        probed.borrow().is_empty(),
        "the filesystem was touched for an ungranted path: {:?}",
        probed.borrow()
    );
}
