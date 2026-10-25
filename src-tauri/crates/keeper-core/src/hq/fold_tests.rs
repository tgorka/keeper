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
    let before = w.view(&opened(SHOW_ALL, &[], &[]));
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
    let view = w.view(&opened(SHOW_ALL, &[], &[]));
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
        let on = w.view(&opened(SHOW_ALL, &[], &[]));
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
fn provider_refusal_remains_a_single_line_without_inventing_duration() {
    let w = world(true);
    w.put(vec![w.answer(
        "I cannot do that.",
        json!({"error": "Provider refused\n  the request", "finish": "refused", "duration_ms": -1}),
    )]);
    let view = w.view(&Opened::default());
    assert_eq!(
        (view.folds[0].status, view.folds[0].status_label.as_str()),
        (Status::Refused, "Refused")
    );
    assert_eq!(
        view.folds[0].failures[0].sentence,
        "Provider refused the request"
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
fn live_answers_and_tool_only_work_use_their_recorded_chronological_positions() {
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
                turn_id: TURN.into()
            },
            Item::LiveAnswer {
                turn_id: TURN.into()
            },
            Item::Fold {
                turn_id: "t2".into()
            },
            Item::Message {
                message_id: "later-person".into()
            },
        ],
        "a live answer's first chunk is not moved below later work or person rows"
    );
    assert_eq!(view.live[0].text, "Earlier streamed answer");
}
