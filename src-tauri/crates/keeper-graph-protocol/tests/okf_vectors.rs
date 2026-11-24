//! `syncular/okf-profile.json` and `syncular/okf-profile.vectors.json` hold
//! for this implementation (story 117.1): the profile's names pass its
//! grammar and shadow nothing reserved, the order key's between and
//! re-keying rules, the number rule and the title rule each reproduce their
//! vectors. keeper-hq's server tests read the same two files.

use keeper_graph_protocol::okf::{
    self, admits_importance, admits_name, between, insert_at, is_name, is_order_key, later_key,
    seed_keys, shadows_reserved, title, Member, Of, On, Profile,
};
use keeper_graph_protocol::value::check_node_properties;
use keeper_graph_protocol::vocabulary::{check_definition, RESERVED_KINDS, RESERVED_RELATIONS};
use serde_json::Value;

const VECTORS: &str = include_str!("../../../../syncular/okf-profile.vectors.json");

fn vectors() -> Value {
    serde_json::from_str(VECTORS).expect("vector JSON")
}

fn profile() -> Profile {
    Profile::pinned().expect("okf-profile.json reads")
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_else(|| panic!("text in {value}"))
}

fn texts(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("list in {value}"))
        .iter()
        .map(text)
        .collect()
}

fn key(value: &Value) -> Option<&str> {
    value.as_str()
}

fn rekeyed(value: &Value) -> Vec<(usize, String)> {
    value
        .as_array()
        .expect("rekeyed")
        .iter()
        .map(|pair| {
            (
                pair[0].as_u64().expect("index") as usize,
                text(&pair[1]).to_owned(),
            )
        })
        .collect()
}

#[test]
fn the_profile_file_and_the_crate_agree() {
    let p = profile();
    let v = vectors();
    assert_eq!(p.profile, "okf");
    assert_eq!(p.version, okf::VERSION);
    assert_eq!(v["version"], okf::VERSION);
    assert_eq!(p.namespace, okf::NAMESPACE);
    assert_eq!(p.reserved_namespaces, ["okf", "keeper"]);
    assert_eq!(p.grammar.max_bytes, okf::MAX_NAME_BYTES);
    assert_eq!(p.grammar.exempt, ["order_key"]);
    assert_eq!(p.kg1.kinds, RESERVED_KINDS);
    assert_eq!(p.kg1.relations, RESERVED_RELATIONS);
    assert_eq!(p.kg1.edge_properties, ["order_key"]);
    let order = &p.order_key;
    assert_eq!(order.property, "order_key");
    assert_eq!(order.alphabet.as_bytes(), okf::ALPHABET);
    assert_eq!((order.min_length, order.max_length), (1, okf::MAX_KEY_LEN));
    assert_eq!(order.never_ends_with, "-");
    assert_eq!(order.number_width, okf::NUMBER_WIDTH);
    assert_eq!(order.rank_digits, okf::RANK_DIGITS);
    assert_eq!(order.unnumbered_prefix, okf::UNNUMBERED_PREFIX.to_string());
    assert_eq!(order.unnumbered_rank_digits, okf::UNNUMBERED_RANK_DIGITS);
    assert_eq!(order.title_prefix_max_digits, okf::TITLE_PREFIX_MAX_DIGITS);
    assert_eq!(p.importance.property, "okf:importance");
    assert_eq!(
        (p.importance.min, p.importance.max),
        (okf::IMPORTANCE_MIN, okf::IMPORTANCE_MAX)
    );
    // The alphabet is in ASCII order, so a key's order is its byte order.
    assert!(okf::ALPHABET.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn every_profile_name_passes_the_grammar_and_shadows_nothing_reserved() {
    let p = profile();
    let mut seen = std::collections::BTreeSet::new();
    for entry in &p.names {
        assert!(
            admits_name(&entry.name),
            "{} outside the grammar",
            entry.name
        );
        assert!(!shadows_reserved(&entry.name), "{} shadows", entry.name);
        assert!(entry.name.starts_with("okf:"), "{} not in okf:", entry.name);
        assert!(seen.insert(entry.name.as_str()), "{} twice", entry.name);
        assert!(
            !entry.meaning.trim().is_empty(),
            "{} has no meaning",
            entry.name
        );
        match entry.of {
            Of::Relation => {
                assert!(entry.on.is_none() && entry.from.is_some() && entry.to.is_some())
            }
            Of::Property => assert!(entry.on.is_some(), "{} has no place", entry.name),
            Of::Kind => panic!("v1 defines no kind: {}", entry.name),
        }
        for kind in entry.from.iter().chain(&entry.to).flatten() {
            assert!(RESERVED_KINDS.contains(&kind.as_str()), "{kind}");
        }
        // Its `Vocabulary` node is one KG1 admits.
        check_definition(&entry.definition()).expect("definition admitted");
        check_node_properties(Some("Vocabulary"), &entry.vocabulary_properties())
            .expect("Vocabulary properties admitted");
    }
    // v1's ~30 names fit one change set below 127 operations, with the root.
    assert!(p.names.len() < 127, "{}", p.names.len());
    // `okf:part_of` is not in v1 (AC13).
    assert!(p.name("okf:part_of").is_none());
    for name in [
        "okf:about",
        "okf:related_to",
        "okf:links_to",
        "okf:directory",
        "okf:extra",
        "okf:predicate",
        "okf:drive_okf_version",
        "okf:profile_version",
        "okf:profile_nodes",
        "okf:importance",
        "okf:type",
    ] {
        assert!(p.name(name).is_some(), "{name} missing");
    }
    assert_eq!(p.name("okf:predicate").and_then(|n| n.on), Some(On::Edge));
    assert_eq!(p.name("okf:directory").and_then(|n| n.on), Some(On::Node));
}

#[test]
fn the_names_for_model_text_are_reserved_and_admitted() {
    let p = profile();
    let v = vectors();
    let reserved = &v["names"]["modelText"];
    assert_eq!(p.model_text.summary, text(&reserved["summary"]));
    assert_eq!(p.model_text.provenance, texts(&reserved["provenance"]));
    assert_eq!(p.model_text.note_property, None, "no property for a note");
    assert!(!p.model_text.rules.is_empty());
    let mut names = vec![p.model_text.summary.as_str()];
    names.extend(p.model_text.provenance.iter().map(String::as_str));
    for name in names {
        assert!(admits_name(name), "{name}");
        let entry = p.name(name).unwrap_or_else(|| panic!("{name} not defined"));
        assert_eq!(entry.of, Of::Property);
        assert_eq!(
            entry.on,
            Some(On::Node),
            "{name} is a top-level node property"
        );
        assert_eq!(entry.reserved_for.as_deref(), Some("model_text"), "{name}");
    }
    // Nothing else is reserved for model text, and no name is a note's.
    let others: Vec<_> = p
        .names
        .iter()
        .filter(|n| n.reserved_for.is_some())
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(others.len(), 1 + p.model_text.provenance.len());
    assert!(p.names.iter().all(|n| !n.name.contains("note")));
}

#[test]
fn names_are_accepted_and_refused_as_the_vectors_say() {
    let v = vectors();
    for name in texts(&v["names"]["accepted"]) {
        assert!(is_name(name) && admits_name(name), "{name} refused");
    }
    for entry in v["names"]["refused"].as_array().expect("refused") {
        let name = text(&entry["name"]);
        assert!(!admits_name(name), "{name} accepted ({})", entry["why"]);
    }
    for name in texts(&v["names"]["shadowsReserved"]) {
        assert!(shadows_reserved(name), "{name} does not shadow");
        assert!(!admits_name(name), "{name} admitted");
    }
}

#[test]
fn importance_is_an_integer_from_0_to_100() {
    let v = vectors();
    for value in v["importance"]["accepted"].as_array().expect("accepted") {
        assert!(admits_importance(value), "{value}");
    }
    for value in v["importance"]["refused"].as_array().expect("refused") {
        assert!(!admits_importance(value), "{value}");
    }
}

#[test]
fn order_keys_and_the_key_between_two_neighbours() {
    let v = vectors();
    for k in texts(&v["orderKeys"]["accepted"]) {
        assert!(is_order_key(k), "{k}");
    }
    for entry in v["orderKeys"]["refused"].as_array().expect("refused") {
        assert!(!is_order_key(text(&entry["key"])), "{entry}");
    }
    for entry in v["between"].as_array().expect("between") {
        let (lo, hi) = (key(&entry["lo"]), key(&entry["hi"]));
        let got = between(lo, hi);
        assert_eq!(got.as_deref(), key(&entry["key"]), "{entry}");
        if let Some(got) = got {
            assert!(is_order_key(&got));
            assert!(lo.is_none_or(|lo| lo < got.as_str()), "{entry}");
            assert!(hi.is_none_or(|hi| got.as_str() < hi), "{entry}");
        }
    }
}

#[test]
fn the_between_key_is_the_shortest_and_its_middle() {
    // No shorter key fits, and the key is the middle of those that do.
    let alphabet: Vec<char> = okf::ALPHABET.iter().map(|&b| b as char).collect();
    let keys_of = |len: usize| {
        let mut all = vec![String::new()];
        for _ in 0..len {
            all = all
                .iter()
                .flat_map(|prefix| alphabet.iter().map(move |c| format!("{prefix}{c}")))
                .collect();
        }
        all.retain(|k| is_order_key(k));
        all
    };
    let short: Vec<Vec<String>> = (1..=3).map(keys_of).collect();
    for (lo, hi) in [
        (None, None),
        (Some("a"), Some("b")),
        (Some("a"), Some("a0")),
        (Some("0"), Some("1")),
        (None, Some("0")),
        (Some("zz"), None),
        (Some("a-1"), Some("a0")),
    ] {
        let fits =
            |k: &String| lo.is_none_or(|lo| lo < k.as_str()) && hi.is_none_or(|hi| k.as_str() < hi);
        let got = between(lo, hi).expect("a key fits");
        let len = got.len();
        for shorter in &short[..len - 1] {
            assert!(
                !shorter.iter().any(fits),
                "{lo:?} {hi:?}: a shorter key fits"
            );
        }
        let candidates: Vec<&String> = short[len - 1].iter().filter(|k| fits(k)).collect();
        assert!(candidates.contains(&&got), "{lo:?} {hi:?}: {got}");
        let position = candidates.iter().position(|k| **k == got).expect("found");
        let middle = (candidates.len() - 1) / 2;
        assert!(
            position.abs_diff(middle) <= 1,
            "{lo:?} {hi:?}: {got} at {position} of {}",
            candidates.len()
        );
    }
}

#[test]
fn equal_keys_and_insertions_re_key_only_neighbours() {
    let v = vectors();
    for entry in v["insertAt"].as_array().expect("insertAt") {
        let siblings = texts(&entry["siblings"]);
        let at = entry["at"].as_u64().expect("at") as usize;
        let got = insert_at(&siblings, at).expect("an insertion");
        assert_eq!(got.key, text(&entry["key"]), "{entry}");
        assert_eq!(got.rekeyed, rekeyed(&entry["rekeyed"]), "{entry}");
    }
}

/// Insert again and again at one place; every step keeps the order and the
/// bound, and the first insertion that finds no key re-keys only the
/// neighbours, in the same insertion.
#[test]
fn repeated_insertion_at_one_place_re_keys_only_the_neighbours() {
    let v = vectors();
    for entry in v["repeatedInsertion"]
        .as_array()
        .expect("repeatedInsertion")
    {
        let mut keys: Vec<String> = texts(&entry["siblings"])
            .into_iter()
            .map(str::to_owned)
            .collect();
        let after_first = text(&entry["place"]) == "afterFirst";
        let mut plain = 0;
        let last = loop {
            let at = if after_first { 1 } else { keys.len() - 1 };
            let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
            let insertion = insert_at(&refs, at).expect("an insertion");
            let mut next = keys.clone();
            for (index, key) in &insertion.rekeyed {
                next[*index] = key.clone();
            }
            next.insert(at, insertion.key.clone());
            assert!(next.iter().all(|k| is_order_key(k)), "bound broken");
            assert!(next.windows(2).all(|w| w[0] < w[1]), "order broken");
            if !insertion.rekeyed.is_empty() {
                // Only members next to the insertion point move.
                for (index, _) in &insertion.rekeyed {
                    assert!(
                        index.abs_diff(at) <= insertion.rekeyed.len(),
                        "{index} far from {at}"
                    );
                }
                break insertion;
            }
            plain += 1;
            keys = next;
        };
        assert_eq!(
            plain,
            entry["insertsWithoutRekey"].as_u64().expect("count") as usize
        );
        assert_eq!(last.key, text(&entry["then"]["key"]), "{entry}");
        assert_eq!(last.rekeyed, rekeyed(&entry["then"]["rekeyed"]), "{entry}");
    }
}

#[test]
fn the_number_rule_gives_seed_keys_and_titles() {
    let v = vectors();
    for entry in v["seed"].as_array().expect("seed") {
        let names = texts(&entry["names"]);
        let keys: Vec<Option<String>> = seed_keys(&names);
        let expected: Vec<Option<String>> = texts(&entry["keys"])
            .into_iter()
            .map(|k| Some(k.to_owned()))
            .collect();
        assert_eq!(keys, expected, "{}", entry["case"]);
        if let Some(titles) = entry.get("titles") {
            let got: Vec<&str> = names
                .iter()
                .map(|n| title(n, Member::Folder, None))
                .collect();
            assert_eq!(got, texts(titles), "{}", entry["case"]);
        }
    }
}

#[test]
fn a_later_sibling_never_changes_an_existing_key() {
    let v = vectors();
    for entry in v["later"].as_array().expect("later") {
        let siblings = texts(&entry["siblings"]);
        let before = siblings.clone();
        let got = later_key(text(&entry["name"]), &siblings);
        assert_eq!(
            got.as_deref(),
            Some(text(&entry["key"])),
            "{}",
            entry["case"]
        );
        // The rule returns one key and takes the siblings read-only; the
        // new key is unused among them.
        assert_eq!(siblings, before);
        assert!(
            !siblings.contains(&text(&entry["key"])),
            "{}",
            entry["case"]
        );
    }
}

#[test]
fn only_a_short_folder_number_leaves_the_title() {
    let v = vectors();
    for entry in v["titles"].as_array().expect("titles") {
        let member = match text(&entry["member"]) {
            "folder" => Member::Folder,
            _ => Member::Document,
        };
        let got = title(text(&entry["name"]), member, key(&entry["config"]));
        assert_eq!(got, text(&entry["title"]), "{entry}");
    }
}
