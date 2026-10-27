use super::*;
use serde_json::json;

const EXPECTED: &str = "28571bedbb9cdd80fe3b7a4b691ebed6eb6531fd";

fn parsed(args: Value) -> Request {
    Request::parse(&args).expect("parses")
}

fn refused(args: Value) -> Refusal {
    Request::parse(&args).expect_err("refused")
}

fn why(args: Value) -> String {
    match refused(args) {
        Refusal::Args { why } => why,
        other => panic!("not dw.args: {other:?}"),
    }
}

fn edit(old: &str, new: &str) -> Value {
    json!({"drive": "tgdrive", "op": "edit", "path": "10-notes/inbox.md",
        "expected": EXPECTED, "old_text": old, "new_text": new})
}

// ── The digest (contract § 4.3) ────────────────────────────────────────────

#[test]
fn the_contract_test_vector_is_the_digest_binding_digest_computes() {
    let args = json!({"drive":"tgdrive","op":"edit","path":"10-notes/inbox.md",
        "expected":"28571bedbb9cdd80fe3b7a4b691ebed6eb6531fd",
        "old_text":"- [ ] milk\n","new_text":"- [x] milk\n"});
    let checkpoint = row_checkpoint("01JTURN0000000000000000000", "01JCALL0000000000000000000");
    assert_eq!(
        checkpoint.sha256,
        "c2e959f832367a3f7edfd11950d60aa3cd6568cf0db0473051e9e5e9b5853693"
    );
    assert_eq!(
        digest(
            "01JAPPR0000000000000000000",
            "tgorka.hesperia.7",
            "nixi",
            &args,
            "01JTURN0000000000000000000",
            "01JCALL0000000000000000000",
        )
        .expect("digests"),
        "sha256:8f824e507e0e7d867ba161e7ef337573dd67f3d649abe24447a4d82eb403e486"
    );
}

#[test]
fn the_checkpoint_hashes_the_call_and_turn_object_with_its_keys_sorted() {
    let checkpoint = row_checkpoint("T", "C");
    assert_eq!(
        checkpoint.sha256,
        crate::agents::approval::sha256_hex(br#"{"call":"C","turn":"T"}"#)
    );
    assert_eq!(
        (checkpoint.chunk.as_str(), checkpoint.through.as_str()),
        ("T", "C")
    );
}

#[test]
fn the_digest_binds_every_input_it_is_given() {
    let args = edit("a", "b");
    let base = digest("A", "S", "nixi", &args, "T", "C").expect("digests");
    for other in [
        digest("A2", "S", "nixi", &args, "T", "C"),
        digest("A", "S2", "nixi", &args, "T", "C"),
        digest("A", "S", "dixi", &args, "T", "C"),
        digest("A", "S", "nixi", &edit("a", "c"), "T", "C"),
        digest("A", "S", "nixi", &args, "T2", "C"),
        digest("A", "S", "nixi", &args, "T", "C2"),
    ] {
        assert_ne!(other.expect("digests"), base);
    }
}

#[test]
fn a_float_in_the_args_refuses_the_digest() {
    let args = json!({"drive": "tgdrive", "op": "delete", "path": "a.md",
        "expected": EXPECTED, "x": 1.5});
    assert_eq!(
        digest("A", "S", "nixi", &args, "T", "C")
            .expect_err("a float")
            .path,
        "args.x"
    );
}

// ── Parsing, every op ─────────────────────────────────────────────────────

#[test]
fn every_op_reads_with_exactly_its_own_arguments() {
    let create = parsed(json!({"drive": "tgdrive", "op": "create", "path": "a.md", "content": ""}));
    assert_eq!(create.op(), Op::Create);
    assert_eq!(create.expected, None);
    assert_eq!(
        create.change,
        Change::Create {
            content: String::new()
        }
    );

    let replace = parsed(json!({"drive": "tgdrive", "op": "replace", "path": "a.md",
        "expected": EXPECTED, "content": "new"}));
    assert_eq!(replace.op(), Op::Replace);
    assert_eq!(replace.expected.as_deref(), Some(EXPECTED));

    let edited = parsed(edit("old", ""));
    assert_eq!(
        edited.change,
        Change::Edit {
            old_text: "old".into(),
            new_text: String::new()
        }
    );

    let append = parsed(json!({"drive": "tgdrive", "op": "append", "path": "a.md",
        "expected": EXPECTED, "text": "\nmore"}));
    assert_eq!(append.op(), Op::Append);

    let moved = parsed(json!({"drive": "tgdrive", "op": "move", "path": "a.md",
        "expected": EXPECTED, "to": "b/a.md"}));
    assert_eq!(moved.op(), Op::Move);
    assert_eq!(moved.to(), Some("b/a.md"));

    let delete = parsed(json!({"drive": "tgdrive", "op": "delete", "path": "a.md",
        "expected": EXPECTED}));
    assert_eq!(delete.op(), Op::Delete);
    assert_eq!(delete.to(), None);
    assert_eq!(delete.drive, "tgdrive");
    assert_eq!(delete.path, "a.md");
}

#[test]
fn a_null_argument_is_an_absent_one() {
    let create = parsed(json!({"drive": "tgdrive", "op": "create", "path": "a.md",
        "content": "x", "expected": null, "to": null, "old_text": null}));
    assert_eq!(create.expected, None);
    assert_eq!(
        why(json!({"drive": "tgdrive", "op": "delete", "path": "a.md", "expected": null})),
        "delete needs expected"
    );
}

#[test]
fn arguments_that_are_not_an_object_or_name_no_op_do_not_read() {
    assert_eq!(why(json!(["create"])), "they are not one JSON object");
    for op in [json!(null), json!("mkdir"), json!("Create"), json!(3)] {
        assert_eq!(
            why(json!({"drive": "tgdrive", "op": op, "path": "a"})),
            "op is one of create, replace, edit, append, move, delete"
        );
    }
}

#[test]
fn an_op_without_an_argument_it_needs_names_it() {
    assert_eq!(
        why(json!({"op": "create", "path": "a.md", "content": ""})),
        "create needs drive"
    );
    assert_eq!(
        why(json!({"drive": "", "op": "create", "path": "a.md", "content": ""})),
        "create needs drive"
    );
    assert_eq!(
        why(json!({"drive": "tgdrive", "op": "create", "content": ""})),
        "create needs path"
    );
    assert_eq!(
        why(json!({"drive": "tgdrive", "op": "create", "path": "", "content": ""})),
        "create needs path"
    );
    assert_eq!(
        why(json!({"drive": "tgdrive", "op": "create", "path": "a.md"})),
        "create needs content"
    );
    assert_eq!(
        why(json!({"drive": "tgdrive", "op": "replace", "path": "a.md", "expected": EXPECTED})),
        "replace needs content"
    );
    assert_eq!(
        why(json!({"drive": "tgdrive", "op": "replace", "path": "a.md", "content": "x"})),
        "replace needs expected"
    );
    assert_eq!(why(edit("", "x")), "edit needs old_text");
    assert_eq!(
        why(
            json!({"drive": "tgdrive", "op": "edit", "path": "a.md", "expected": EXPECTED,
            "old_text": "x"})
        ),
        "edit needs new_text"
    );
    assert_eq!(
        why(
            json!({"drive": "tgdrive", "op": "append", "path": "a.md", "expected": EXPECTED,
            "text": ""})
        ),
        "append needs text"
    );
    assert_eq!(
        why(json!({"drive": "tgdrive", "op": "move", "path": "a.md", "expected": EXPECTED})),
        "move needs to"
    );
    assert_eq!(
        why(
            json!({"drive": "tgdrive", "op": "move", "path": "a.md", "expected": EXPECTED,
            "to": 7})
        ),
        "move needs to"
    );
}

#[test]
fn an_argument_the_op_does_not_take_is_refused_by_name() {
    assert_eq!(
        why(
            json!({"drive": "tgdrive", "op": "edit", "path": "a.md", "expected": EXPECTED,
            "old_text": "a", "new_text": "b", "content": "c"})
        ),
        "edit takes no content"
    );
    assert_eq!(
        why(
            json!({"drive": "tgdrive", "op": "create", "path": "a.md", "content": "",
            "expected": EXPECTED})
        ),
        "create takes no expected"
    );
    assert_eq!(
        why(
            json!({"drive": "tgdrive", "op": "delete", "path": "a.md", "expected": EXPECTED,
            "to": "b.md"})
        ),
        "delete takes no to"
    );
    assert_eq!(
        why(
            json!({"drive": "tgdrive", "op": "delete", "path": "a.md", "expected": EXPECTED,
            "recursive": true})
        ),
        "delete takes no recursive"
    );
    let mut long = json!({"drive": "tgdrive", "op": "delete", "path": "a.md",
        "expected": EXPECTED});
    long["k".repeat(60).as_str()] = json!(1);
    assert_eq!(why(long), format!("delete takes no {}…", "k".repeat(40)));
}

#[test]
fn expected_is_forty_lowercase_hex_characters() {
    for bad in [
        "28571BEDBB9CDD80FE3B7A4B691EBED6EB6531FD",
        "28571bedbb9cdd80fe3b7a4b691ebed6eb6531f",
        "28571bedbb9cdd80fe3b7a4b691ebed6eb6531fdd",
        "sha256:28571bedbb9cdd80fe3b7a4b691ebed6eb65",
        "g8571bedbb9cdd80fe3b7a4b691ebed6eb6531fd",
    ] {
        assert_eq!(
            why(json!({"drive": "tgdrive", "op": "delete", "path": "a.md", "expected": bad})),
            "expected is the 40-character version drive_read printed",
            "{bad}"
        );
    }
}

#[test]
fn a_path_is_at_most_1024_bytes() {
    let path = format!("{}/{}", "a".repeat(255), "b".repeat(768));
    assert_eq!(path.len(), 1_024);
    parsed(json!({"drive": "tgdrive", "op": "create", "path": path, "content": ""}));
    let longer = format!("{path}c");
    assert_eq!(
        why(json!({"drive": "tgdrive", "op": "create", "path": longer, "content": ""})),
        "path is 1025 bytes; a path is at most 1024 bytes"
    );
    assert_eq!(
        why(
            json!({"drive": "tgdrive", "op": "move", "path": "a.md", "expected": EXPECTED,
            "to": longer})
        ),
        "to is 1025 bytes; a path is at most 1024 bytes"
    );
}

// ── The caps and the NUL rule ─────────────────────────────────────────────

#[test]
fn a_text_argument_holds_at_most_262144_bytes() {
    let at_cap = "é".repeat(MAX_TEXT_BYTES / 2);
    assert_eq!(at_cap.len(), 262_144);
    parsed(json!({"drive": "tgdrive", "op": "create", "path": "a.md", "content": at_cap}));
    let over = format!("{at_cap}x");
    assert_eq!(
        refused(json!({"drive": "tgdrive", "op": "create", "path": "a.md", "content": over})),
        Refusal::ArgSize {
            arg: "content",
            n: 262_145
        }
    );
    assert_eq!(refused(edit("a", &over)).code(), "dw.arg_size");
    assert_eq!(
        refused(edit(&over, "a")),
        Refusal::ArgSize {
            arg: "old_text",
            n: 262_145
        }
    );
    assert_eq!(
        refused(json!({"drive": "tgdrive", "op": "append", "path": "a.md",
            "expected": EXPECTED, "text": over})),
        Refusal::ArgSize {
            arg: "text",
            n: 262_145
        }
    );
}

#[test]
fn a_file_holds_at_most_1000000_bytes_after_the_change() {
    let current = "x".repeat(MAX_FILE_BYTES - 3);
    let append = |text: &str| {
        parsed(json!({"drive": "tgdrive", "op": "append", "path": "a.md",
            "expected": EXPECTED, "text": text}))
    };
    let at_cap = append("abc")
        .post_image(Some(current.as_bytes()))
        .expect("at the cap")
        .expect("text");
    assert_eq!(at_cap.len(), 1_000_000);
    assert_eq!(
        append("abcd").post_image(Some(current.as_bytes())),
        Err(Refusal::FileSize {
            path: "a.md".into(),
            n: 1_000_001
        })
    );
    // An edit that grows a file at the cap past it, and one that shrinks a
    // file already over it back under.
    let full = format!("{}!e", "x".repeat(MAX_FILE_BYTES - 2));
    assert_eq!(
        parsed(edit("!e", "!ee")).post_image(Some(full.as_bytes())),
        Err(Refusal::FileSize {
            path: "10-notes/inbox.md".into(),
            n: 1_000_001
        })
    );
    let over = format!("{}!end", "x".repeat(MAX_FILE_BYTES));
    let shrunk = parsed(edit("x!end", ""))
        .post_image(Some(over.as_bytes()))
        .expect("back under")
        .expect("text");
    assert_eq!(shrunk.len(), MAX_FILE_BYTES - 1);
    let replaced = parsed(json!({"drive": "tgdrive", "op": "replace", "path": "a.md",
        "expected": EXPECTED, "content": "y"}))
    .post_image(Some(over.as_bytes()))
    .expect("replaced");
    assert_eq!(replaced.as_deref(), Some("y"));
}

#[test]
fn a_nul_in_any_text_or_path_is_refused_by_name() {
    assert_eq!(
        refused(json!({"drive": "tgdrive", "op": "create", "path": "a.md", "content": "a\0b"})),
        Refusal::Nul { arg: "content" }
    );
    assert_eq!(refused(edit("a", "\0")), Refusal::Nul { arg: "new_text" });
    assert_eq!(
        refused(json!({"drive": "tgdrive", "op": "create", "path": "a\0.md", "content": ""})),
        Refusal::Nul { arg: "path" }
    );
    assert_eq!(
        refused(
            json!({"drive": "tgdrive", "op": "move", "path": "a.md", "expected": EXPECTED,
            "to": "b\0.md"})
        ),
        Refusal::Nul { arg: "to" }
    );
}

#[test]
fn a_request_that_changes_nothing_is_refused() {
    assert_eq!(
        refused(edit("same", "same")),
        Refusal::Noop {
            what: "new_text is old_text"
        }
    );
    assert_eq!(
        refused(
            json!({"drive": "tgdrive", "op": "move", "path": "a.md", "expected": EXPECTED,
            "to": "a.md"})
        ),
        Refusal::Noop { what: "to is path" }
    );
    let replace = parsed(json!({"drive": "tgdrive", "op": "replace", "path": "a.md",
        "expected": EXPECTED, "content": "same\n"}));
    assert_eq!(
        replace.post_image(Some(b"same\n")),
        Err(Refusal::Noop {
            what: "the file already holds this text"
        })
    );
}

// ── The post-image, every op ──────────────────────────────────────────────

#[test]
fn create_writes_its_content_where_nothing_is() {
    let create = parsed(json!({"drive": "tgdrive", "op": "create", "path": "a.md",
        "content": "no newline"}));
    assert_eq!(create.post_image(None), Ok(Some("no newline".to_owned())));
    assert_eq!(
        create.post_image(Some(b"")),
        Err(Refusal::Exists {
            path: "a.md".into()
        })
    );
}

#[test]
fn replace_writes_the_whole_text_exactly() {
    let replace = parsed(json!({"drive": "tgdrive", "op": "replace", "path": "a.md",
        "expected": EXPECTED, "content": "one\r\ntwo"}));
    assert_eq!(
        replace.post_image(Some(b"old\n")),
        Ok(Some("one\r\ntwo".to_owned()))
    );
    assert_eq!(
        replace.post_image(None),
        Err(Refusal::Absent {
            path: "a.md".into()
        })
    );
}

#[test]
fn edit_replaces_the_one_place_its_passage_starts() {
    let file = "- [ ] milk\n- [ ] eggs\n";
    assert_eq!(
        parsed(edit("- [ ] milk\n", "- [x] milk\n")).post_image(Some(file.as_bytes())),
        Ok(Some("- [x] milk\n- [ ] eggs\n".to_owned()))
    );
    assert_eq!(
        parsed(edit("eggs", "")).post_image(Some(file.as_bytes())),
        Ok(Some("- [ ] milk\n- [ ] \n".to_owned()))
    );
    assert_eq!(
        parsed(edit("bread", "x")).post_image(Some(file.as_bytes())),
        Err(Refusal::EditNone {
            path: "10-notes/inbox.md".into()
        })
    );
    assert_eq!(
        parsed(edit("- [ ] ", "* ")).post_image(Some(file.as_bytes())),
        Err(Refusal::EditMany {
            path: "10-notes/inbox.md".into(),
            n: 2
        })
    );
    assert_eq!(
        parsed(edit("x", "y")).post_image(None),
        Err(Refusal::Absent {
            path: "10-notes/inbox.md".into()
        })
    );
}

#[test]
fn overlapping_occurrences_of_an_edit_count_as_two_places() {
    assert_eq!(
        parsed(edit("aa", "b")).post_image(Some(b"aaa")),
        Err(Refusal::EditMany {
            path: "10-notes/inbox.md".into(),
            n: 2
        })
    );
    assert_eq!(
        parsed(edit("abab", "x")).post_image(Some(b"zababab")),
        Err(Refusal::EditMany {
            path: "10-notes/inbox.md".into(),
            n: 2
        })
    );
    assert_eq!(
        parsed(edit("aab", "x")).post_image(Some(b"aaab")),
        Ok(Some("ax".to_owned()))
    );
}

#[test]
fn append_adds_its_text_as_given() {
    let append = parsed(json!({"drive": "tgdrive", "op": "append", "path": "log.md",
        "expected": EXPECTED, "text": "- b"}));
    assert_eq!(
        append.post_image(Some(b"- a")),
        Ok(Some("- a- b".to_owned()))
    );
    assert_eq!(
        append.post_image(None),
        Err(Refusal::Absent {
            path: "log.md".into()
        })
    );
}

#[test]
fn move_and_delete_write_no_text_and_take_any_file_that_is_there() {
    for args in [
        json!({"drive": "tgdrive", "op": "move", "path": "p.png", "expected": EXPECTED,
            "to": "img/p.png"}),
        json!({"drive": "tgdrive", "op": "delete", "path": "p.png", "expected": EXPECTED}),
    ] {
        let request = parsed(args);
        assert_eq!(request.post_image(Some(b"\x89PNG\0\xff")), Ok(None));
        assert_eq!(
            request.post_image(None),
            Err(Refusal::Absent {
                path: "p.png".into()
            })
        );
    }
}

#[test]
fn a_file_that_is_not_text_can_only_be_moved_or_deleted() {
    let binary = Refusal::Binary {
        path: "10-notes/inbox.md".into(),
    };
    let ops = [
        json!({"drive": "tgdrive", "op": "replace", "path": "10-notes/inbox.md",
            "expected": EXPECTED, "content": "x"}),
        edit("a", "b"),
        json!({"drive": "tgdrive", "op": "append", "path": "10-notes/inbox.md",
            "expected": EXPECTED, "text": "x"}),
    ];
    for args in ops {
        let request = parsed(args);
        assert_eq!(request.post_image(Some(b"a\xff")), Err(binary.clone()));
        assert_eq!(request.post_image(Some(b"a\0")), Err(binary.clone()));
    }
}

#[test]
fn the_guard_compares_the_version_read_with_the_one_here() {
    let delete = parsed(json!({"drive": "tgdrive", "op": "delete", "path": "a.md",
        "expected": EXPECTED}));
    assert_eq!(delete.guard(Some(EXPECTED)), Ok(()));
    let now = "c216fb8de0e24c06bf3fff9aaf6deb2036e93f1b";
    assert_eq!(
        delete.guard(Some(now)),
        Err(Refusal::Stale {
            path: "a.md".into(),
            expected: EXPECTED.into(),
            now: now.into()
        })
    );
    assert_eq!(
        delete.guard(None),
        Err(Refusal::Absent {
            path: "a.md".into()
        })
    );
    let create = parsed(json!({"drive": "tgdrive", "op": "create", "path": "a.md",
        "content": ""}));
    assert_eq!(create.guard(None), Ok(()));
    assert_eq!(
        create.guard(Some(now)),
        Err(Refusal::Exists {
            path: "a.md".into()
        })
    );
}

// ── The summary and the spec ──────────────────────────────────────────────

#[test]
fn every_op_has_its_own_summary_line() {
    let cases = [
        (
            json!({"drive": "tgdrive", "op": "create", "path": "a.md", "content": "héllo"}),
            "Create `a.md` in tgdrive (6 bytes)",
        ),
        (
            json!({"drive": "tgdrive", "op": "replace", "path": "a.md", "expected": EXPECTED,
                "content": "abc"}),
            "Replace the text of `a.md` in tgdrive (3 bytes)",
        ),
        (
            edit("- [ ] milk\n", "- [x] milk and bread\n"),
            "Edit `10-notes/inbox.md` in tgdrive: 11 bytes become 21 bytes",
        ),
        (
            json!({"drive": "tgdrive", "op": "append", "path": "a.md", "expected": EXPECTED,
                "text": "\n- c"}),
            "Append 4 bytes to `a.md` in tgdrive",
        ),
        (
            json!({"drive": "tgdrive", "op": "move", "path": "a.md", "expected": EXPECTED,
                "to": "b/a.md", "message": null}),
            "Move `a.md` to `b/a.md` in tgdrive",
        ),
        (
            json!({"drive": "neuradrive", "op": "delete", "path": "a.md", "expected": EXPECTED}),
            "Delete `a.md` in neuradrive",
        ),
    ];
    for (args, line) in cases {
        assert_eq!(parsed(args).summary(), line);
    }
}

#[test]
fn the_spec_offers_the_ticked_drives_sorted_once_and_takes_nothing_else() {
    let spec = spec(&["tgdrive".into(), "neuradrive".into(), "tgdrive".into()]);
    assert_eq!(spec.name, "drive_write");
    assert_eq!(
        spec.parameters["properties"]["drive"]["enum"],
        json!(["neuradrive", "tgdrive"])
    );
    assert_eq!(
        spec.parameters["properties"]["op"]["enum"],
        json!(["create", "replace", "edit", "append", "move", "delete"])
    );
    assert_eq!(spec.parameters["required"], json!(["drive", "op", "path"]));
    assert_eq!(spec.parameters["additionalProperties"], json!(false));
    let properties: Vec<&String> = spec.parameters["properties"]
        .as_object()
        .expect("properties")
        .keys()
        .collect();
    assert_eq!(properties.len(), 9);
    assert!(spec
        .description
        .contains("at most 262144 bytes of new text per call, and at most 1000000 bytes"));
    assert!(spec.description.starts_with(
        "Change one file of a drive a person ticked for this conversation: create a text file,"
    ));
    assert!(spec
        .description
        .ends_with("A folder is made by creating or moving a file into it. The person sees exactly what you send."));
}

// ── Every refusal code and its sentence (contract § 7.3) ──────────────────

#[test]
fn every_refusal_has_its_code_and_its_sentence() {
    let cases = [
        (
            Refusal::Args {
                why: "edit needs old_text".into(),
            },
            "dw.args",
            "drive_write's arguments do not read: edit needs old_text. Nothing was asked.",
        ),
        (
            Refusal::Unticked {
                drive: "tgdrive".into(),
            },
            "dw.unticked",
            "tgdrive is not a drive ticked for this session. A person ticks the drives an agent \
             may use. Nothing was asked.",
        ),
        (
            Refusal::Unreadable {
                drive: "tgdrive".into(),
                why: "no index".into(),
            },
            "dw.unreadable",
            "tgdrive's tracked files could not be read here (no index). Nothing was asked.",
        ),
        (
            Refusal::Path {
                sentence: "\".git/config\" is git's or keeper's own file, which an agent never \
                           writes."
                    .into(),
            },
            "dw.path",
            "\".git/config\" is git's or keeper's own file, which an agent never writes. \
             Nothing was asked.",
        ),
        (
            Refusal::Absent {
                path: "a.md".into(),
            },
            "dw.absent",
            "\"a.md\" is not in this drive; create makes a new file. Nothing was asked.",
        ),
        (
            Refusal::Exists {
                path: "a.md".into(),
            },
            "dw.exists",
            "\"a.md\" already exists; read it, then replace, edit or append with its expected. \
             Nothing was asked.",
        ),
        (
            Refusal::ToExists { to: "b.md".into() },
            "dw.to_exists",
            "\"b.md\" already exists; nothing is moved over a file. Nothing was asked.",
        ),
        (
            Refusal::Stale {
                path: "a.md".into(),
                expected: "e".into(),
                now: "n".into(),
            },
            "dw.stale",
            "\"a.md\" changed since you read it (you sent e; it is n here). Read it again, then \
             ask again. Nothing was asked.",
        ),
        (
            Refusal::Binary {
                path: "p.png".into(),
            },
            "dw.binary",
            "\"p.png\" is not text, so it can only be moved or deleted. Nothing was asked.",
        ),
        (
            Refusal::Folder { path: "10".into() },
            "dw.folder",
            "\"10\" is a folder; drive_write changes one file. Create a file inside it to make \
             a folder. Nothing was asked.",
        ),
        (
            Refusal::EditNone {
                path: "a.md".into(),
            },
            "dw.edit_none",
            "old_text does not appear in \"a.md\"; read it again and quote the passage exactly. \
             Nothing was asked.",
        ),
        (
            Refusal::EditMany {
                path: "a.md".into(),
                n: 3,
            },
            "dw.edit_many",
            "old_text appears 3 times in \"a.md\"; quote enough around it to name one place. \
             Nothing was asked.",
        ),
        (
            Refusal::ArgSize {
                arg: "content",
                n: 262_145,
            },
            "dw.arg_size",
            "content is 262145 bytes; one call sends at most 262144 bytes of new text. Nothing \
             was asked.",
        ),
        (
            Refusal::FileSize {
                path: "a.md".into(),
                n: 1_000_001,
            },
            "dw.file_size",
            "\"a.md\" would be 1000001 bytes; an agent writes files of at most 1000000 bytes. \
             Nothing was asked.",
        ),
        (
            Refusal::Nul { arg: "text" },
            "dw.nul",
            "text holds a NUL character; drive_write writes text only. Nothing was asked.",
        ),
        (
            Refusal::Noop { what: "to is path" },
            "dw.noop",
            "This changes nothing (to is path). Nothing was asked.",
        ),
        (
            Refusal::Trust {
                why: "no row".into(),
            },
            "dw.trust",
            "drive_write was not asked: this session's trust could not be read (no row).",
        ),
    ];
    for (refusal, code, sentence) in cases {
        assert_eq!(refusal.code(), code);
        assert_eq!(refusal.to_string(), sentence, "{code}");
    }
}

// ── The preview ───────────────────────────────────────────────────────────

fn line(kind: LineKind, text: &str) -> PreviewLine {
    PreviewLine {
        kind,
        text: text.to_owned(),
    }
}

fn numbered(lines: std::ops::Range<usize>) -> String {
    lines.map(|n| format!("line {n}\n")).collect()
}

#[test]
fn an_edit_shows_one_hunk_with_three_lines_of_context() {
    let before = numbered(1..21);
    let after = before.replace("line 10\n", "line ten\r\n");
    let preview = preview(Some(&before), Some(&after));
    assert_eq!(preview.whole, None);
    assert_eq!(preview.deleted, None);
    assert_eq!(
        preview.hunks,
        vec![Hunk {
            old_start: 7,
            new_start: 7,
            lines: vec![
                line(LineKind::Context, "line 7"),
                line(LineKind::Context, "line 8"),
                line(LineKind::Context, "line 9"),
                line(LineKind::Remove, "line 10"),
                line(LineKind::Add, "line ten"),
                line(LineKind::Context, "line 11"),
                line(LineKind::Context, "line 12"),
                line(LineKind::Context, "line 13"),
            ],
        }]
    );
}

#[test]
fn changes_far_apart_are_two_hunks_and_near_ones_share_one() {
    let before = numbered(1..41);
    let far = before
        .replace("line 5\n", "five\n")
        .replace("line 30\n", "thirty\n");
    let hunks = preview(Some(&before), Some(&far)).hunks;
    assert_eq!(hunks.len(), 2);
    assert_eq!((hunks[1].old_start, hunks[1].new_start), (27, 27));
    // Six unchanged lines between two changes: their contexts touch.
    let near = before
        .replace("line 5\n", "five\n")
        .replace("line 12\n", "twelve\n");
    let hunks = preview(Some(&before), Some(&near)).hunks;
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].lines.len(), 3 + 2 + 6 + 2 + 3);
    // Seven between them: two hunks.
    let apart = before
        .replace("line 5\n", "five\n")
        .replace("line 13\n", "thirteen\n");
    assert_eq!(preview(Some(&before), Some(&apart)).hunks.len(), 2);
}

#[test]
fn a_last_line_that_loses_its_newline_is_a_change() {
    let hunks = preview(Some("a\nb\n"), Some("a\nb")).hunks;
    assert_eq!(
        hunks,
        vec![Hunk {
            old_start: 1,
            new_start: 1,
            lines: vec![
                line(LineKind::Context, "a"),
                line(LineKind::Remove, "b"),
                line(LineKind::Add, "b"),
            ],
        }]
    );
}

#[test]
fn a_create_shows_every_line_added_and_an_append_only_its_tail() {
    let created = preview(None, Some("one\ntwo\n"));
    assert_eq!(
        created.hunks,
        vec![Hunk {
            old_start: 0,
            new_start: 1,
            lines: vec![line(LineKind::Add, "one"), line(LineKind::Add, "two")],
        }]
    );
    let before = numbered(1..11);
    let appended = preview(Some(&before), Some(&format!("{before}tail\n")));
    assert_eq!(
        appended.hunks,
        vec![Hunk {
            old_start: 8,
            new_start: 8,
            lines: vec![
                line(LineKind::Context, "line 8"),
                line(LineKind::Context, "line 9"),
                line(LineKind::Context, "line 10"),
                line(LineKind::Add, "tail"),
            ],
        }]
    );
}

#[test]
fn a_side_a_hunk_shows_no_line_of_starts_at_the_line_it_comes_after() {
    let hunks = preview(Some("gone\nkept\n"), Some("kept\n")).hunks;
    assert_eq!(
        hunks,
        vec![Hunk {
            old_start: 1,
            new_start: 1,
            lines: vec![
                line(LineKind::Remove, "gone"),
                line(LineKind::Context, "kept")
            ],
        }]
    );
    let emptied = preview(Some("gone\n"), Some("")).hunks;
    assert_eq!(
        emptied,
        vec![Hunk {
            old_start: 1,
            new_start: 0,
            lines: vec![line(LineKind::Remove, "gone")],
        }]
    );
}

#[test]
fn a_deletion_shows_the_deleted_texts_beginning_and_a_move_shows_nothing() {
    let before = numbered(1..251);
    let deleted = preview(Some(&before), None);
    let excerpt = deleted.deleted.expect("deleted");
    assert_eq!(excerpt.lines.len(), 200);
    assert_eq!(excerpt.lines[0], "line 1");
    assert_eq!(excerpt.more, 50);
    assert!(deleted.hunks.is_empty());
    assert_eq!(preview(None, None), Preview::default());
}

#[test]
fn a_side_over_5000_lines_shows_the_new_texts_beginning() {
    let at_cap = numbered(1..5_001);
    let changed = at_cap.replace("line 2500\n", "middle\n");
    assert_eq!(preview(Some(&at_cap), Some(&changed)).hunks.len(), 1);
    let over = numbered(1..5_002);
    let shown = preview(Some(&over), Some(&changed));
    assert!(shown.hunks.is_empty());
    let whole = shown.whole.expect("whole");
    assert_eq!(whole.lines.first().map(String::as_str), Some("line 1"));
    assert_eq!(whole.more, 5_000 - 200);
    assert!(preview(Some(&changed), Some(&over)).whole.is_some());
}

#[test]
fn sides_that_differ_in_more_than_1000_lines_show_the_new_texts_beginning() {
    let before = numbered(1..501);
    let replaced: String = (1..502).map(|n| format!("other {n}\n")).collect();
    // 500 removed and 501 added: one past the cap.
    let shown = preview(Some(&before), Some(&replaced));
    assert!(shown.hunks.is_empty());
    assert_eq!(shown.whole.expect("whole").lines[0], "other 1");
    let at_cap: String = (1..501).map(|n| format!("other {n}\n")).collect();
    assert_eq!(preview(Some(&before), Some(&at_cap)).hunks.len(), 1);
}

/// The longest common subsequence's length, the slow way.
fn lcs(a: &[&str], b: &[&str]) -> usize {
    let mut row = vec![0usize; b.len() + 1];
    for x in a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

#[test]
fn the_edit_script_is_a_shortest_one_and_rebuilds_both_sides() {
    // A small deterministic generator: no new dependency for a test.
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = |bound: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % bound
    };
    let words = ["a\n", "b\n", "c\n", "d\n", "e"];
    for _ in 0..500 {
        let n = next(12) as usize;
        let m = next(12) as usize;
        let a: Vec<&str> = (0..n).map(|_| words[next(5) as usize]).collect();
        let b: Vec<&str> = (0..m).map(|_| words[next(5) as usize]).collect();
        let script = edits(&a, &b, 1_000).expect("within the cap");
        let keeps = script.iter().filter(|step| **step == Step::Keep).count();
        assert_eq!(keeps, lcs(&a, &b), "{a:?} → {b:?}");
        let (mut i, mut j) = (0, 0);
        let (mut old, mut new) = (Vec::new(), Vec::new());
        for step in script {
            match step {
                Step::Keep => {
                    assert_eq!(a[i], b[j]);
                    old.push(a[i]);
                    new.push(b[j]);
                    i += 1;
                    j += 1;
                }
                Step::Remove => {
                    old.push(a[i]);
                    i += 1;
                }
                Step::Add => {
                    new.push(b[j]);
                    j += 1;
                }
            }
        }
        assert_eq!((old, new), (a.clone(), b.clone()));
    }
}
