use std::collections::HashSet;

use super::*;

#[test]
fn the_compiled_vocabulary_parses() {
    assert!(PARSED.is_some(), "syncular/vocabulary.json does not parse");
}

/// The contract's examples were computed outside keeper; a derivation that
/// reads the digest another way, or another byte, gives other words.
#[test]
fn every_example_session_has_the_handle_the_contract_names() {
    let examples = handle_examples();
    assert!(examples.len() >= 3);
    for example in examples {
        assert_eq!(
            handle(&example.session_id).as_deref(),
            Some(example.handle.as_str()),
            "{}",
            example.session_id
        );
    }
}

/// A handle is a pure function of the id: the same id always reads the
/// same, and two ids of one device read differently here.
#[test]
fn a_handle_depends_on_the_id_alone() {
    assert_eq!(handle("tgorka.1a2b3c4d.1"), handle("tgorka.1a2b3c4d.1"));
    assert_ne!(handle("tgorka.1a2b3c4d.1"), handle("tgorka.1a2b3c4d.2"));
}

/// Each list is a power of two long (no index is likelier than another) and
/// holds distinct lowercase ASCII words, so a handle splits back on `-`
/// into exactly three words.
#[test]
fn the_word_lists_are_even_and_hyphen_free() {
    let Some(vocabulary) = PARSED.as_ref() else {
        panic!("syncular/vocabulary.json does not parse");
    };
    let words = &vocabulary.handle;
    for list in [&words.adjectives, &words.nouns, &words.verbs] {
        assert!(list.len().is_power_of_two() && list.len() <= 1 << 16);
        assert_eq!(list.iter().collect::<HashSet<_>>().len(), list.len());
        assert!(list
            .iter()
            .all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase())));
    }
}

/// D-33: a T4 action is decided on another device every time, so no grant
/// may cover it, and a grant covers only a tier that asks a person at all.
#[test]
fn no_grant_tier_reaches_t4() {
    let tiers = grant_tiers();
    assert!(!tiers.is_empty());
    for tier in tiers {
        assert!(["T2", "T3"].contains(&tier.as_str()), "{tier}");
    }
}

#[test]
fn efforts_are_distinct_and_known_by_name() {
    let all = efforts();
    assert_eq!(all.iter().collect::<HashSet<_>>().len(), all.len());
    assert!(is_effort("low") && is_effort("xhigh"));
    assert!(!is_effort("") && !is_effort("max") && !is_effort("LOW"));
}
