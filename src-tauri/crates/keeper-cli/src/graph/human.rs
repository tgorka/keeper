//! The human rendering of a graph verb's answer: what `--json` prints, said
//! as sentences. Rendered from the very document `--json` prints, so the two
//! can never tell different stories.

use serde_json::Value;

/// A scalar as a person reads it: strings bare, `null` as `none`.
fn text(value: &Value) -> String {
    match value {
        Value::Null => "none".to_owned(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn count(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The sentence for `verb`'s `answer`.
pub fn sentence(verb: &str, answer: &Value) -> String {
    if answer["ok"] != true {
        return failure(answer);
    }
    match verb {
        "apply" | "propose" | "decide" | "acknowledge" | "reverse" => applied(answer),
        "enqueue" => format!(
            "Queued request {} ({}); nothing was sent.",
            text(&answer["request_id"]),
            text(&answer["state"])
        ),
        "discard" => format!(
            "Withdrew request {} ({}); it will never be sent.",
            text(&answer["request_id"]),
            text(&answer["state"])
        ),
        "pending" | "resume" => entries(verb, answer),
        "sync" => format!(
            "The projection is at graph seq {}: {} ({}).",
            text(&answer["graph_seq"]),
            if answer["built"] == true {
                "a new generation was built"
            } else {
                "nothing new to build"
            },
            text(&answer["completeness"])
        ),
        "query" => rows(answer),
        "inspect" => inspected(answer),
        "history" | "unreviewed" => page(verb, answer),
        "proposal" => revisions(answer),
        "preview-reversal" => preview(&answer["preview"]),
        "status" => status(answer),
        _ => answer.to_string(),
    }
}

/// A refused or failed run: its stable code and what it said.
fn failure(answer: &Value) -> String {
    let mut out = format!(
        "Failed: {} ({}).",
        text(&answer["error"]),
        text(&answer["detail"])
    );
    if answer["preview"].is_object() {
        out.push('\n');
        out.push_str(&preview(&answer["preview"]));
    }
    out
}

/// A command's request and the server's outcome.
fn applied(answer: &Value) -> String {
    let o = &answer["outcome"];
    let what = match o["outcome"].as_str().unwrap_or_default() {
        "accepted" => format!(
            "accepted as event {} at graph seq {}",
            text(&o["event_id"]),
            text(&o["graph_seq"])
        ),
        "proposed" => "proposed: it waits for its reviewers".to_owned(),
        "conflict" => "a conflict: nothing was written".to_owned(),
        "rejected" => format!("rejected: {}", text(&o["reason"])),
        "pending_visibility" => format!(
            "accepted at commit {}, not yet visible here",
            text(&o["commit_seq"])
        ),
        "pending" if o["cause"].is_string() => format!("pending: {}", text(&o["cause"])),
        _ => format!(
            "pending: {}",
            text(
                &o.get("error")
                    .or_else(|| o.get("state"))
                    .cloned()
                    .unwrap_or_default()
            )
        ),
    };
    let mut out = format!(
        "Request {} ({}): {}.",
        text(&answer["request_id"]),
        text(&answer["descriptor"]),
        what
    );
    match &answer["fenced"] {
        Value::Bool(true) => out.push_str(" The domain is now fenced."),
        Value::Bool(false) => out.push_str(" The domain could not be fenced."),
        _ => {}
    }
    out
}

/// The queue's entries (`pending`) or what `resume` did with each.
fn entries(verb: &str, answer: &Value) -> String {
    let list = answer["entries"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut out = if verb == "pending" {
        format!("{} in the queue.", plural(list.len(), "entry", "entries"))
    } else {
        format!("Resumed {}.", plural(list.len(), "entry", "entries"))
    };
    for e in list {
        out.push_str(&format!(
            "\n  {} {}",
            text(&e["request_id"]),
            text(&e["state"])
        ));
        for key in ["attempts", "cause", "age_ms", "error"] {
            if let Some(value) = e.get(key).filter(|v| !v.is_null()) {
                out.push_str(&format!(" {}={}", key, text(value)));
            }
        }
    }
    out
}

fn footer(answer: &Value) -> String {
    format!(
        "at graph seq {} ({}, projection {}, engine {})",
        text(&answer["graph_seq"]),
        text(&answer["completeness"]),
        text(&answer["projection_version"]),
        text(&answer["engine"])
    )
}

fn rows(answer: &Value) -> String {
    let n = count(&answer["rows"]);
    let mut out = format!("{} {}.", plural(n, "row", "rows"), footer(answer));
    for row in answer["rows"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        out.push_str(&format!("\n  {row}"));
    }
    out
}

fn inspected(answer: &Value) -> String {
    match &answer["rows"] {
        Value::Null => format!("Not found {}.", footer(answer)),
        found => format!("Found {}:\n  {found}", footer(answer)),
    }
}

/// One page of history or of the unreviewed feed.
fn page(verb: &str, answer: &Value) -> String {
    let page = &answer["page"];
    let events = page["events"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut out = format!(
        "{} {}, scanned through graph seq {}.",
        plural(events.len(), "event", "events"),
        if verb == "history" {
            "of retained history"
        } else {
            "unreviewed"
        },
        text(&page["scanned_through"])
    );
    for e in events {
        let h = &e["event"];
        out.push_str(&format!(
            "\n  {} {} {} by {} ({} ops, delta {})",
            text(&h["graph_seq"]),
            text(&h["id"]),
            text(&h["descriptor"]),
            text(&h["actor_id"]),
            text(&h["op_count"]),
            text(&h["delta_digest"])
        ));
    }
    out
}

fn revisions(answer: &Value) -> String {
    let list = answer["revisions"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut out = format!("{}.", plural(list.len(), "revision", "revisions"));
    for r in list {
        out.push_str(&format!(
            "\n  {} revision {} {} by {}, digest {}",
            text(&r["proposal_id"]),
            text(&r["revision"]),
            text(&r["status"]),
            text(&r["proposer_actor"]),
            text(&r["digest"])
        ));
    }
    out
}

fn preview(p: &Value) -> String {
    let mut out = format!(
        "Reversing event {}: {}",
        text(&p["target_event_id"]),
        text(&p["outcome"])
    );
    if !p["reason"].is_null() {
        out.push_str(&format!(" ({})", text(&p["reason"])));
    }
    out.push_str(&format!(
        "; {} expected.",
        plural(count(&p["expected"]), "revision", "revisions")
    ));
    out
}

fn status(answer: &Value) -> String {
    let checkpoint = &answer["checkpoint"];
    let queue = answer["queue"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(state, n)| format!("{state} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "empty".to_owned());
    format!(
        "Replica head {}; checkpoint {} at {}.\nRuntime: {}.\nFence: {}.\nQueue: {}.",
        text(&answer["head"]),
        text(&checkpoint["scope"]),
        text(&checkpoint["cursor"]),
        text(&answer["runtime"]),
        text(&answer["fence"]),
        queue
    )
}
