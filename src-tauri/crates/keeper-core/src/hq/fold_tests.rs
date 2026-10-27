//! Recorded HQ rows replayed through the real SQLite replica and session VM.
use super::*;
use timeline::{AgentTimelineItemVm as Item, AgentTurnStatusVm as Status};

fn call(w: &World, id: &str, name: &str, status: &str, at: i64, result: Option<&str>) -> RowOp {
    upsert(
        "tool_calls",
        json!({
            "id": id, "session_id": "dev1.1", "page_key": w.page(), "turn_id": TURN,
            "name": name, "args": "{}", "status": status, "result": result,
            "result_blob": null, "started_at_ms": w.at + at,
            "ended_at_ms": result.map(|_| w.at + at + 1), "created_at_ms": w.at + at,
            "author_actor": "alpha", "author_client": INSTANCE
        }),
    )
}

fn approval(w: &World, expires: i64) -> RowOp {
    upsert(
        "approvals",
        json!({
            "id": "ap1", "session_id": "dev1.1", "page_key": w.page(), "tool_call_id": "c1",
            "tier": "T2", "binding_digest": "digest", "summary": "search", "approvers": "[\"ana\"]",
            "state": "pending", "decided_by_actor": null, "decided_by_client": null,
            "consumed_by_client": null, "expires_at_ms": expires, "created_at_ms": w.at,
            "author_actor": "alpha", "author_client": INSTANCE
        }),
    )
}

#[test]
fn screenshot_turn_folds_once_with_late_searches_and_a_separate_answer_last() {
    let w = world(true);
    w.put(vec![
        upsert(
            "messages",
            json!({"id": "m-lead", "session_id": "dev1.1", "page_key": w.page(),
            "turn_id": TURN, "role": "agent", "body": "Sprawdzam — minutę 🧜‍♀️",
            "body_blob": null, "metadata": "{}", "created_at_ms": w.at,
            "author_actor": "alpha", "author_client": INSTANCE}),
        ),
        call(&w, "run", "run", "done", 1, Some("ok")),
        call(&w, "search1", "web_search", "done", 2, Some("weather")),
        call(
            &w,
            "search2",
            "web_search",
            "refused",
            3,
            Some("Not allowed\n  outside the grant"),
        ),
        w.reasoning("r1", 1, true, "Compare weather sources"),
        w.answer(
            "Tomorrow: sunny, 18 °C.",
            json!({"duration_ms": 14000, "finish": "stop"}),
        ),
    ]);
    let before = w.view(&opened(VERBOSE, &[], &[]));
    w.put(vec![
        call(
            &w,
            "search3",
            "web_search",
            "done",
            200,
            Some("late result"),
        ),
        call(
            &w,
            "search4",
            "web_search",
            "done",
            300,
            Some("later result"),
        ),
    ]);
    let view = w.view(&opened(VERBOSE, &[], &[]));
    assert_eq!(
        view.items,
        vec![
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::Message {
                message_id: "m-final".into()
            }
        ]
    );
    assert_eq!(view.items, before.items, "late work stays above the answer");
    assert_eq!(view.folds.len(), 1, "one trace, not one per agent message");
    let fold = &view.folds[0];
    assert_eq!(fold.lead_in_ids, ["m-lead"]);
    assert_eq!(
        view.messages
            .iter()
            .find(|m| m.id == fold.lead_in_ids[0])
            .expect("lead")
            .body,
        "Sprawdzam — minutę 🧜‍♀️"
    );
    assert_eq!(
        fold.steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["run", "search1", "search2", "search3", "search4"]
    );
    assert_eq!(fold.reasoning.as_deref(), Some("Compare weather sources"));
    assert_eq!(fold.duration_ms, Some(14000));
    assert_eq!(
        fold.summary,
        "Worked for 14 s · ran 1 command, searched the web 4 times"
    );
    assert_eq!(
        fold.status,
        Status::Done,
        "a refused search does not refuse the successful answer"
    );
    assert_eq!(
        fold.failures
            .iter()
            .map(|f| f.sentence.as_str())
            .collect::<Vec<_>>(),
        ["web_search · Refused: Not allowed outside the grant"]
    );
    assert_eq!(
        view.messages.last().expect("answer").body,
        "Tomorrow: sunny, 18 °C."
    );
    // No grouping/wording protocol hidden in TypeScript: the serialized VM
    // contains the same ordered, Rust-authored projection as the typed view.
    let wire = serde_json::to_value(&view).expect("serialize");
    assert_eq!(
        wire["items"][1],
        json!({"kind": "message", "messageId": "m-final"})
    );
}

#[test]
fn interrupted_is_not_refused_and_parked_requires_a_real_unexpired_approval_to_wait() {
    let w = world(true);
    w.put(vec![w.tool_call(
        "interrupted",
        Some("Process stopped\nby the person"),
    )]);
    let interrupted = w.view(&Opened::default());
    let fold = &interrupted.folds[0];
    assert_eq!(
        (fold.status, fold.status_label.as_str()),
        (Status::Interrupted, "Interrupted")
    );
    assert_eq!(fold.steps[0].status_label, "Interrupted");
    assert_eq!(
        fold.failures[0].sentence,
        "web_search · Interrupted: Process stopped by the person"
    );
    w.put(vec![w.tool_call("parked", None)]);
    let paused = w.view(&Opened::default());
    assert_eq!(
        (
            paused.folds[0].status,
            paused.folds[0].status_label.as_str()
        ),
        (Status::Parked, "Paused")
    );
    assert!(paused.folds[0].failures.is_empty());
    w.put(vec![approval(&w, sessions::now_ms() + 60000)]);
    let waiting = w.view(&Opened::default());
    assert_eq!(
        (
            waiting.folds[0].status,
            waiting.folds[0].status_label.as_str()
        ),
        (Status::Waiting, "Waiting for you")
    );
    assert!(waiting.folds[0].summary.starts_with("Waiting for you"));
    w.put(vec![approval(&w, w.at)]);
    assert_eq!(
        w.view(&Opened::default()).folds[0].status,
        Status::Parked,
        "an expired approval is no longer asking"
    );
}

#[test]
fn live_fold_becomes_ended_without_repeating_trace_and_details_do_not_gate_duration() {
    let w = world(true);
    w.put(vec![
        w.chunk(1, "The answer so far"),
        w.tool_call("started", None),
    ]);
    let docs = [session_doc(2, "…")];
    let running = w.view(&opened(AgentViewPrefsVm::default(), &docs, &[]));
    assert_eq!(
        running.items,
        vec![
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::LiveAnswer {
                turn_id: TURN.into()
            }
        ]
    );
    assert_eq!(running.folds[0].summary, "Searching the web… · 1 step");
    assert_eq!(
        running.folds[0].duration_ms, None,
        "never fabricate a duration"
    );
    w.put(vec![
        w.answer("The whole answer", json!({"duration_ms": 2700})),
        w.tool_call("done", Some("source")),
    ]);
    let ended = w.view(&opened(AgentViewPrefsVm::default(), &docs, &[]));
    assert!(
        ended.live.is_empty(),
        "stale live tails do not duplicate an ended turn"
    );
    assert_eq!(
        ended.items,
        vec![
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::Message {
                message_id: "m-final".into()
            }
        ]
    );
    assert_eq!(ended.folds[0].status, Status::Done);
    assert_eq!(ended.folds[0].duration_ms, Some(2700));
    assert!(ended.messages[0].details.is_none());
}

#[test]
fn fold_trace_and_reasoning_respect_membership_and_reasoning_preference() {
    for reads in [true, false] {
        let w = world(reads);
        w.put(vec![
            w.tool_call("refused", Some("private refusal")),
            w.reasoning("r1", 1, true, "private thought"),
            w.answer("public answer", json!({})),
        ]);
        let on = w.view(&opened(VERBOSE, &[], &[]));
        let fold = &on.folds[0];
        assert_eq!(
            fold.reasoning.as_deref(),
            reads.then_some("private thought")
        );
        if reads {
            assert_eq!(fold.steps[0].result.as_deref(), Some("private refusal"));
            assert_eq!(
                fold.failures[0].sentence,
                "web_search · Refused: private refusal"
            );
        } else {
            assert!(fold.steps.is_empty());
            assert!(fold.failures.is_empty());
            assert_eq!(
                fold.summary, "Worked",
                "no trace facts in the summary either"
            );
        }
        let off = w.view(&Opened::default());
        assert!(off.folds[0].reasoning.is_none());
    }
}

#[test]
fn distinct_turns_keep_their_work_and_person_messages_without_a_global_fold() {
    let w = world(true);
    w.put(vec![w.answer("First", json!({})), w.tool_call("done", Some("first source")),
        upsert("messages", json!({"id": "p2", "session_id": "dev1.1", "page_key": w.page(),
            "turn_id": null, "role": "person", "body": "Next?", "body_blob": null,
            "metadata": "{}", "created_at_ms": w.at + 200, "author_actor": "ana", "author_client": "dev1"})),
        upsert("messages", json!({"id": "m2", "session_id": "dev1.1", "page_key": w.page(),
            "turn_id": "t2", "role": "agent", "body": "Second", "body_blob": null,
            "metadata": json!({"finish": null}).to_string(), "created_at_ms": w.at + 300, "author_actor": "alpha", "author_client": INSTANCE})),
    ]);
    let view = w.view(&Opened::default());
    assert_eq!(
        view.items,
        vec![
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::Message {
                message_id: "m-final".into()
            },
            Item::Message {
                message_id: "p2".into()
            },
            Item::Fold {
                turn_id: "t2".into()
            },
            Item::Message {
                message_id: "m2".into()
            }
        ]
    );
    assert_eq!(
        view.folds
            .iter()
            .map(|f| f.turn_id.as_str())
            .collect::<Vec<_>>(),
        [TURN, "t2"]
    );
    assert!(view.folds[1].steps.is_empty());
}

#[test]
fn provider_failure_remains_a_single_line_without_inventing_duration() {
    let w = world(true);
    // keeper-hq `served.rs` `Ending::Error`: `finish: failed` and the error.
    w.put(vec![w.answer(
        "I cannot do that.",
        json!({"error": "Provider answered\n  HTTP 500", "finish": "failed", "duration_ms": -1}),
    )]);
    let view = w.view(&Opened::default());
    assert_eq!(
        (view.folds[0].status, view.folds[0].status_label.as_str()),
        (Status::Failed, "Failed")
    );
    assert_eq!(
        view.folds[0].failures[0].sentence,
        "Provider answered HTTP 500"
    );
    assert_eq!(view.folds[0].duration_ms, None);
    assert_eq!(
        view.items.last(),
        Some(&Item::Message {
            message_id: "m-final".into()
        })
    );
    w.put(vec![
        w.answer("Finished immediately", json!({"duration_ms": 0}))
    ]);
    assert_eq!(w.view(&Opened::default()).folds[0].duration_ms, Some(0));
}

#[test]
fn tool_only_outcomes_do_not_fabricate_a_live_answer() {
    let w = world(true);
    for (raw, expected) in [
        ("refused", Status::Refused),
        ("done", Status::Done),
        ("started", Status::Running),
    ] {
        w.put(vec![w.tool_call(raw, None)]);
        let view = w.view(&Opened::default());
        assert_eq!(view.folds[0].status, expected, "tool-only {raw}");
        assert!(view.live.is_empty(), "tool rows are not an answer stream");
        assert_eq!(
            view.items,
            vec![Item::Fold {
                turn_id: TURN.into()
            }]
        );
    }
}

fn recorded_message(
    w: &World,
    id: &str,
    turn: Option<&str>,
    body: &str,
    at: i64,
    metadata: Value,
) -> RowOp {
    let (role, actor, client) = if turn.is_some() {
        ("agent", "alpha", INSTANCE)
    } else {
        ("person", "ana", "dev1")
    };
    upsert(
        "messages",
        json!({"id": id, "session_id": "dev1.1", "page_key": w.page(),
            "turn_id": turn, "role": role, "body": body, "body_blob": null,
            "metadata": metadata.to_string(), "created_at_ms": w.at + at,
            "author_actor": actor, "author_client": client}),
    )
}

#[test]
fn intervening_person_stays_before_its_turns_terminal_answer() {
    let w = world(true);
    w.put(vec![
        recorded_message(&w, "lead", Some(TURN), "Checking", 0, json!({})),
        recorded_message(&w, "steer", None, "Use Celsius", 50, json!({})),
        w.answer("18 C", json!({"finish": "stop"})),
    ]);
    let view = w.view(&Opened::default());
    assert_eq!(
        view.items,
        [
            Item::Message {
                message_id: "steer".into()
            },
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::Message {
                message_id: "m-final".into()
            },
        ],
        "the person's instruction precedes the answer, not its hidden lead-in"
    );
    assert_eq!(view.folds[0].lead_in_ids, ["lead"]);
    assert_eq!(
        view.messages
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["lead", "steer", "m-final"],
        "legacy messages retain their chronological rows"
    );
}

fn older_tool_only_history(live_answer: bool) {
    let w = world(true);
    w.put(vec![
        w.tool_call("interrupted", Some("Process stopped")),
        recorded_message(&w, "p2", None, "Try again", 50, json!({})),
    ]);
    let mut opened = Opened::default();
    let answer = if live_answer {
        w.put(vec![upsert(
            "turn_chunks",
            json!({
                "id": "t2-k0", "session_id": "dev1.1", "turn_id": "t2",
                "seq": 0, "text": "New answer", "created_at_ms": w.at + 100,
                "author_actor": "alpha", "author_client": INSTANCE
            }),
        )]);
        let peer = Peer {
            doc: json!({"turn": "t2", "from_seq": 1, "text": " so far", "tool": null}),
            ..session_doc(1, "")
        };
        opened = super::opened(AgentViewPrefsVm::default(), &[peer], &[]);
        Item::LiveAnswer {
            turn_id: "t2".into(),
        }
    } else {
        w.put(vec![recorded_message(
            &w,
            "answer2",
            Some("t2"),
            "New answer",
            100,
            json!({"finish": "stop"}),
        )]);
        Item::Message {
            message_id: "answer2".into(),
        }
    };
    let view = w.view(&opened);
    assert_eq!(
        view.items,
        [
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::Message {
                message_id: "p2".into()
            },
            Item::Fold {
                turn_id: "t2".into()
            },
            answer,
        ],
        "old tool-only work stays before the retry and its newer answer"
    );
    assert_eq!(
        view.folds
            .iter()
            .map(|f| f.turn_id.as_str())
            .collect::<Vec<_>>(),
        [TURN, "t2"],
        "each turn has one fold in reading order"
    );
    assert_eq!(view.folds[0].status, Status::Interrupted);
    assert!(view.live.iter().all(|turn| turn.turn_id != TURN));
    if live_answer {
        assert_eq!(view.live[0].text, "New answer so far");
    } else {
        assert!(view.live.is_empty());
    }
}

#[test]
fn old_tool_only_fold_precedes_a_newer_completed_answer() {
    older_tool_only_history(false);
}

#[test]
fn old_tool_only_fold_precedes_a_newer_live_answer() {
    older_tool_only_history(true);
}

#[test]
fn committed_lead_in_keeps_the_live_answer_until_a_terminal_row_replaces_it() {
    let w = world(true);
    w.put(vec![
        recorded_message(&w, "lead", Some(TURN), "Checking", 0, json!({})),
        w.chunk(0, ""),
        w.chunk(1, "The temperature is"),
        w.tool_call("done", Some("Weather source")),
    ]);
    let docs = [session_doc(2, " 18 C")];
    let opened = opened(AgentViewPrefsVm::default(), &docs, &[]);
    let running = w.view(&opened);
    assert_eq!(
        running.items,
        [
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::LiveAnswer {
                turn_id: TURN.into()
            }
        ],
        "a committed lead-in is not the terminal answer"
    );
    assert_eq!(running.live[0].text, "The temperature is 18 C");
    assert_eq!(running.folds[0].lead_in_ids, ["lead"]);
    assert_eq!(
        (
            running.folds[0].status,
            running.folds[0].status_label.as_str()
        ),
        (Status::Running, "Working")
    );
    assert_eq!(
        running.messages[0].id, "lead",
        "legacy lead-in remains available"
    );
    // Closed always writes the finish key, including null if the provider
    // did not report a reason. Both forms must beat stale chunks/presence.
    for finish in [json!("stop"), Value::Null] {
        w.put(vec![w.answer(
            "The temperature is 18 C.",
            json!({"finish": finish}),
        )]);
        let ended = w.view(&opened);
        assert_eq!(
            ended.items,
            [
                Item::Fold {
                    turn_id: TURN.into()
                },
                Item::Message {
                    message_id: "m-final".into()
                }
            ],
            "terminal persistence replaces the streamed answer exactly once"
        );
        assert!(
            ended.live.is_empty(),
            "terminal row wins over stale presence/chunks"
        );
        assert_eq!(ended.folds[0].lead_in_ids, ["lead"]);
        assert_eq!(ended.folds[0].status, Status::Done);
        assert_eq!(
            ended.messages.last().expect("answer").body,
            "The temperature is 18 C."
        );
    }
}

#[test]
fn live_answers_sit_below_later_tool_only_work_and_person_rows_like_their_close_will() {
    let w = world(true);
    w.put(vec![
        w.chunk(0, "Earlier streamed answer"),
        upsert(
            "tool_calls",
            json!({"id": "c2", "session_id": "dev1.1", "page_key": w.page(),
            "turn_id": "t2", "name": "web_search", "args": "{}", "status": "interrupted",
            "result": "Stopped", "result_blob": null, "started_at_ms": w.at + 50,
            "ended_at_ms": w.at + 60, "created_at_ms": w.at + 50,
            "author_actor": "alpha", "author_client": INSTANCE}),
        ),
        recorded_message(
            &w,
            "later-person",
            None,
            "Another instruction",
            100,
            json!({}),
        ),
    ]);
    let view = w.view(&Opened::default());
    assert_eq!(
        view.items,
        [
            Item::Fold {
                turn_id: "t2".into()
            },
            Item::Message {
                message_id: "later-person".into()
            },
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::LiveAnswer {
                turn_id: TURN.into()
            },
        ],
        "a live answer is read where its Closed row will be: after what is already written"
    );
    assert_eq!(view.live[0].text, "Earlier streamed answer");
}

/// keeper-hq `served.rs`: what a stopped answer's row and pane carry.
const STOPPED_REASON: &str = "Stopped. What had arrived is kept.";
/// keeper-hq `served.rs`: what an answer whose stream broke carries.
const BROKE_REASON: &str = "The answer stopped before it finished.";
/// keeper-hq `worker.rs` `cut_sentence()`: the kill-cut row's error.
const CUT_SENTENCE: &str =
    "My answer was cut off when the agent restarted. Ask again if you still need it.";

/// A fold's status as the wire spells it, and its label.
fn state(fold: &timeline::AgentTurnFoldVm) -> (Value, &str) {
    (
        serde_json::to_value(fold.status).expect("status"),
        fold.status_label.as_str(),
    )
}

/// Every legacy message is read exactly once, as an item or as a fold's
/// lead-in; every live turn has one live answer; `folds` is in item order.
fn assert_items_account_for_legacy(view: &timeline::AgentSessionViewVm) {
    let mut read: Vec<&str> = view
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Message { message_id } => Some(message_id.as_str()),
            _ => None,
        })
        .chain(
            view.folds
                .iter()
                .flat_map(|fold| fold.lead_in_ids.iter().map(String::as_str)),
        )
        .collect();
    read.sort_unstable();
    let mut legacy: Vec<&str> = view.messages.iter().map(|m| m.id.as_str()).collect();
    legacy.sort_unstable();
    assert_eq!(read, legacy, "each legacy message is read exactly once");
    let live: Vec<&str> = view
        .items
        .iter()
        .filter_map(|item| match item {
            Item::LiveAnswer { turn_id } => Some(turn_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        live,
        view.live
            .iter()
            .map(|t| t.turn_id.as_str())
            .collect::<Vec<_>>()
    );
    let folded: Vec<&str> = view
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fold { turn_id } => Some(turn_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        folded,
        view.folds
            .iter()
            .map(|f| f.turn_id.as_str())
            .collect::<Vec<_>>()
    );
}

fn person_actor(id: &str) -> RowOp {
    upsert(
        "actors",
        json!({"id": id, "kind": "person", "display_name": id, "family": null,
               "dir": "alpha", "created_at_ms": 1, "author_actor": "server",
               "author_client": "server"}),
    )
}

#[test]
fn an_ask_is_a_reading_item_at_its_place_and_never_folded() {
    let w = world(true);
    // keeper-hq `ask.rs` `row()`: a turn of its own (its id), no `finish`.
    let ask = json!({"kind": "ask", "options": ["Celsius", "Fahrenheit"]});
    w.put(vec![
        recorded_message(&w, "p1", None, "Weather in Kraków?", 0, json!({})),
        recorded_message(&w, "lead", Some(TURN), "Checking", 5, json!({})),
        recorded_message(&w, "ask1", Some("ask1"), "Which unit?", 10, ask),
        w.answer("18 C", json!({"finish": "stop"})),
    ]);
    let view = w.view(&Opened::default());
    assert_eq!(
        view.items,
        [
            Item::Message {
                message_id: "p1".into()
            },
            Item::Message {
                message_id: "ask1".into()
            },
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::Message {
                message_id: "m-final".into()
            },
        ],
        "the question to the person is read where it was asked"
    );
    assert_eq!(
        view.folds
            .iter()
            .map(|f| (f.turn_id.as_str(), f.lead_in_ids.clone()))
            .collect::<Vec<_>>(),
        [(TURN, vec!["lead".to_owned()])],
        "an ask is neither a fold of its own nor anyone's lead-in"
    );
    let legacy = view.messages.iter().find(|m| m.id == "ask1").expect("ask");
    assert!(
        legacy.ask.is_some(),
        "legacy messages keep the ask's options"
    );
    assert_items_account_for_legacy(&view);
}

#[test]
fn a_persons_stop_reads_stopped_while_it_lands_and_after_the_turn_closes() {
    let w = world(true);
    w.put(vec![
        person_actor("ana"),
        w.chunk(0, ""),
        w.chunk(1, "The temperature is"),
    ]);
    let docs = [session_doc(2, " 18")];
    let opened = opened(AgentViewPrefsVm::default(), &docs, &[]);
    assert_eq!(
        state(&w.view(&opened).folds[0]),
        (json!("running"), "Working")
    );
    // A row in a person's role written by an agent is no person's stop:
    // the runtime's `stop::targets` ignores it, so keeper does too.
    w.put(vec![upsert(
        "messages",
        json!({"id": "agent-stop", "session_id": "dev1.1", "page_key": w.page(),
               "turn_id": null, "role": "person", "body": "Stopped this turn.",
               "body_blob": null, "metadata": json!({"kind": "stop", "turn": TURN}).to_string(),
               "created_at_ms": w.at + 40, "author_actor": "alpha",
               "author_client": INSTANCE}),
    )]);
    assert_eq!(
        state(&w.view(&opened).folds[0]),
        (json!("running"), "Working")
    );
    // keeper `hq/commands.rs` `/stop`: the person's stop row names the turn.
    w.put(vec![recorded_message(
        &w,
        "stop",
        None,
        "Stopped this turn.",
        50,
        json!({"kind": "stop", "turn": TURN}),
    )]);
    let landing = w.view(&opened);
    assert_eq!(
        state(&landing.folds[0]),
        (json!("stopped"), "Stopped"),
        "the stop is landing: not Working, not Refused"
    );
    assert_eq!(
        landing.items.last(),
        Some(&Item::LiveAnswer {
            turn_id: TURN.into()
        }),
        "what had arrived stays shown"
    );
    // keeper-hq `served.rs` `close()` for `Ending::Stopped`, through
    // `sink.rs` `closed()`.
    w.put(vec![w.answer(
        "The temperature is 18",
        json!({"model": "m", "effort": null, "model_fallback": null,
               "usage": {"prompt_tokens": null, "completion_tokens": null, "total_tokens": null},
               "ttft_ms": 400, "duration_ms": 3000, "finish": "cancelled", "tool_calls": 0,
               "error": STOPPED_REASON}),
    )]);
    let ended = w.view(&opened);
    assert_eq!(state(&ended.folds[0]), (json!("stopped"), "Stopped"));
    assert!(
        ended.folds[0].summary.starts_with("Stopped"),
        "{}",
        ended.folds[0].summary
    );
    assert!(ended.live.is_empty());
    // A stop that lost the race to an ordinary close: the close decides.
    w.put(vec![w.answer(
        "The temperature is 18 C.",
        json!({"finish": "stop"}),
    )]);
    assert_eq!(
        state(&w.view(&opened).folds[0]),
        (json!("done"), "Finished")
    );
}

#[test]
fn each_runtime_close_reads_its_own_status_and_only_a_refusal_reads_refused() {
    let w = world(true);
    for (metadata, expected) in [
        (json!({"finish": "stop"}), (json!("done"), "Finished")),
        (json!({"finish": "length"}), (json!("done"), "Finished")),
        (json!({"finish": "tool_calls"}), (json!("done"), "Finished")),
        (json!({"finish": null}), (json!("done"), "Finished")),
        (
            json!({"finish": "cancelled", "error": STOPPED_REASON}),
            (json!("stopped"), "Stopped"),
        ),
        (
            json!({"finish": "cut", "error": CUT_SENTENCE}),
            (json!("interrupted"), "Interrupted"),
        ),
        (
            json!({"finish": "failed", "error": BROKE_REASON}),
            (json!("failed"), "Failed"),
        ),
        (
            json!({"finish": "failed", "error": "provider answered HTTP 500"}),
            (json!("failed"), "Failed"),
        ),
        (
            json!({"finish": "content_filter"}),
            (json!("refused"), "Refused"),
        ),
    ] {
        w.put(vec![w.answer("partial", metadata.clone())]);
        let view = w.view(&Opened::default());
        assert_eq!(state(&view.folds[0]), expected, "{metadata}");
    }
}

#[test]
fn a_person_message_sent_mid_stream_never_moves_the_answer() {
    let w = world(true);
    // Production shape: the lead-in streams as the first chunks.
    w.put(vec![
        w.chunk(0, ""),
        w.chunk(1, "Checking — one minute. "),
        recorded_message(&w, "steer", None, "Use Celsius", 50, json!({})),
    ]);
    let docs = [session_doc(2, "The temperature is")];
    let opened = opened(AgentViewPrefsVm::default(), &docs, &[]);
    let streaming = w.view(&opened);
    assert_eq!(
        streaming.items,
        [
            Item::Message {
                message_id: "steer".into()
            },
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::LiveAnswer {
                turn_id: TURN.into()
            },
        ],
        "the streaming answer sits where it will sit once finished"
    );
    w.put(vec![w.answer(
        "Checking — one minute. The temperature is 18 C.",
        json!({"finish": "stop"}),
    )]);
    let finished = w.view(&opened);
    assert_eq!(
        finished.items,
        [
            Item::Message {
                message_id: "steer".into()
            },
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::Message {
                message_id: "m-final".into()
            },
        ]
    );
    assert_items_account_for_legacy(&streaming);
    assert_items_account_for_legacy(&finished);
}

#[test]
fn a_cut_row_and_a_late_close_of_one_turn_both_stay_in_items() {
    let w = world(true);
    w.put(vec![
        recorded_message(&w, "p1", None, "Weather?", 0, json!({})),
        // keeper-hq `worker.rs` `end_cut`.
        recorded_message(
            &w,
            "cut",
            Some(TURN),
            "Checking",
            10,
            json!({"finish": "cut", "error": CUT_SENTENCE}),
        ),
        w.answer("18 C", json!({"finish": "stop"})),
    ]);
    let view = w.view(&Opened::default());
    assert_eq!(
        view.items,
        [
            Item::Message {
                message_id: "p1".into()
            },
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::Message {
                message_id: "cut".into()
            },
            Item::Message {
                message_id: "m-final".into()
            },
        ],
        "neither finished answer disappears"
    );
    assert_eq!(view.folds.len(), 1, "still one fold for the turn");
    assert!(view.folds[0].lead_in_ids.is_empty());
    assert!(
        view.folds[0]
            .failures
            .iter()
            .any(|f| f.sentence == CUT_SENTENCE),
        "the cut row's sentence stays visible"
    );
    assert_items_account_for_legacy(&view);
}

/// A `tool_calls` row as keeper-hq's importer writes it (`hq-import`
/// `plan.rs`): first `interrupted`, then its result's outcome word.
fn imported_call(w: &World, id: &str, turn: &str, status: &str, result: &str) -> RowOp {
    upsert(
        "tool_calls",
        json!({"id": id, "session_id": "dev1.1", "page_key": w.page(), "turn_id": turn,
               "name": "run", "args": "{\"argv\":[\"make\"]}", "status": status,
               "result": result, "result_blob": null, "started_at_ms": w.at + 10,
               "ended_at_ms": w.at + 20, "created_at_ms": w.at + 10,
               "author_actor": "alpha", "author_client": "import"}),
    )
}

#[test]
fn an_imported_failed_tool_call_reads_failed_and_never_working() {
    let w = world(true);
    w.put(vec![
        imported_call(&w, "c1", TURN, "failed", "exit status 1"),
        // The importer's terminal answer (`plan.rs`): its own metadata shape.
        w.answer(
            "The build failed.",
            json!({"model": "m", "usage": {"prompt_tokens": 10, "completion_tokens": 5,
                   "total_tokens": 15}, "ttft_ms": 300, "duration_ms": 4000,
                   "finish": "stop", "tool_calls": 1, "rounds": 2}),
        ),
        // An imported turn whose answer had no text: work only.
        imported_call(&w, "c2", "t-tools", "failed", "no such file"),
    ]);
    let view = w.view(&Opened::default());
    let answered = view.folds.iter().find(|f| f.turn_id == TURN).expect("t1");
    assert_eq!(state(answered), (json!("done"), "Finished"));
    assert_eq!(
        (
            serde_json::to_value(answered.steps[0].status).expect("status"),
            answered.steps[0].status_label.as_str()
        ),
        (json!("failed"), "Failed")
    );
    assert_eq!(answered.summary, "Worked for 4 s · ran 1 command");
    assert_eq!(
        answered
            .failures
            .iter()
            .map(|f| f.sentence.as_str())
            .collect::<Vec<_>>(),
        ["run · Failed: exit status 1"]
    );
    let tools = view
        .folds
        .iter()
        .find(|f| f.turn_id == "t-tools")
        .expect("work-only turn");
    assert_eq!(state(tools), (json!("failed"), "Failed"));
    assert!(view.live.is_empty());
}

#[test]
fn an_unknown_step_status_never_outranks_the_close() {
    let w = world(true);
    w.put(vec![
        imported_call(&w, "c1", TURN, "frobbed\nlater", "whatever happened"),
        w.answer("Done.", json!({"finish": "stop", "duration_ms": 2000})),
    ]);
    let view = w.view(&Opened::default());
    let fold = &view.folds[0];
    assert_eq!(
        state(fold),
        (json!("done"), "Finished"),
        "the close decides, never Working"
    );
    assert_eq!(
        fold.steps[0].status_label, "Unknown status \"frobbed later\"",
        "Rust names what it did not understand, on one line"
    );
    assert_eq!(
        fold.failures
            .iter()
            .map(|f| f.sentence.as_str())
            .collect::<Vec<_>>(),
        ["run · Unknown status \"frobbed later\": whatever happened"]
    );
    assert!(
        fold.summary.starts_with("Worked for 2 s"),
        "{}",
        fold.summary
    );
    // Every close, not just a normal one, still decides.
    w.put(vec![w.answer(
        "Stopped.",
        json!({"finish": "cancelled", "error": STOPPED_REASON}),
    )]);
    assert_eq!(
        state(&w.view(&Opened::default()).folds[0]),
        (json!("stopped"), "Stopped")
    );
}

/// 109.13's review note, fixed for 109.14: a turn that closed decides its
/// own status even when a call's row still says `started` — never
/// "Working" under a finished answer. Without a close, the same row is
/// running work.
#[test]
fn a_closed_turn_with_a_started_step_reads_by_its_close() {
    let w = world(true);
    w.put(vec![imported_call(&w, "c1", TURN, "started", "")]);
    let open = w.view(&Opened::default());
    assert_eq!(state(&open.folds[0]), (json!("running"), "Working"));
    w.put(vec![w.answer(
        "Done.",
        json!({"finish": "stop", "duration_ms": 2000}),
    )]);
    let closed = w.view(&Opened::default());
    let fold = &closed.folds[0];
    assert_eq!(
        state(fold),
        (json!("done"), "Finished"),
        "the close decides, never Working"
    );
    assert_eq!(fold.summary, "Worked for 2 s · ran 1 command");
}

/// The fold line is the one status a collapsed turn shows: a refused turn
/// says so in its summary rather than "Worked".
#[test]
fn a_refused_turn_says_refused_on_its_fold_line() {
    let w = world(true);
    w.put(vec![
        imported_call(&w, "c1", TURN, "done", "ok"),
        w.answer("", json!({"finish": "content_filter", "duration_ms": 1000})),
    ]);
    let fold = &w.view(&Opened::default()).folds[0];
    assert_eq!(state(fold), (json!("refused"), "Refused"));
    assert_eq!(fold.summary, "Refused · ran 1 command");
}

/// Owner report 2026-10-27 (`.9`, 07:32:59Z): a turn that parks on a `run` first closes with a
/// 0-byte row, `finish = "tool_calls"`. That row is never its own bubble: the turn is read as its
/// fold, at its work, waiting for the person; and a blank lead-in is not drawn inside the fold.
#[test]
fn a_blank_close_before_a_parked_call_is_never_a_bubble_of_its_own() {
    let w = world(true);
    w.put(vec![
        recorded_message(&w, "ask", None, "List the repo", 0, json!({})),
        recorded_message(&w, "blank-lead", Some(TURN), "", 1, json!({})),
        call(&w, "c1", "run", "parked", 2, None),
        recorded_message(
            &w,
            "blank-close",
            Some(TURN),
            " \n",
            3,
            json!({"finish": "tool_calls", "model": "m", "duration_ms": 900}),
        ),
        approval(&w, sessions::now_ms() + 60_000),
    ]);
    let view = w.view(&Opened::default());
    assert_eq!(
        view.items,
        [
            Item::Message {
                message_id: "ask".into()
            },
            Item::Fold {
                turn_id: TURN.into()
            },
        ],
        "the blank close is not an item"
    );
    let fold = &view.folds[0];
    assert!(fold.lead_in_ids.is_empty(), "nothing blank inside the fold");
    assert_eq!(
        (fold.status, fold.status_label.as_str()),
        (Status::Waiting, "Waiting for you")
    );
    // The turn's real answer, once the call ran, is read under the same fold.
    w.put(vec![w.answer("Done: 3 files.", json!({"finish": "stop"}))]);
    let answered = w.view(&Opened::default());
    assert_eq!(
        answered.items,
        [
            Item::Message {
                message_id: "ask".into()
            },
            Item::Fold {
                turn_id: TURN.into()
            },
            Item::Message {
                message_id: "m-final".into()
            },
        ]
    );
}
