//! Rust's ordered reading column for an HQ turn (109.13).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Map, Value};

use super::{
    AgentMessageVm, AgentTimelineItemVm as Item, AgentTurnFailureVm, AgentTurnFoldVm,
    AgentTurnStatusVm as Status,
};
use crate::hq::live::{
    self, AgentLiveTurnVm, AgentToolStepStatusVm as Step, AgentToolStepVm, Extras,
};
use crate::hq::sessions::{json, text};

/// All steps stay with their turn, even when committed after its answer.
/// The legacy message projection is untouched until the renderer cutover.
pub(super) fn project(
    rows: &[Map<String, Value>],
    messages: &[AgentMessageVm],
    live: &[AgentLiveTurnVm],
    extras: &Extras,
    pending: &HashSet<String>,
) -> (Vec<Item>, Vec<AgentTurnFoldVm>) {
    let mut by_message = HashMap::new();
    let mut answers = HashMap::new();
    let mut lead_ins: HashMap<&str, Vec<String>> = HashMap::new();
    let mut metadata = HashMap::new();
    for row in rows.iter().filter(|row| text(row, "role") == "agent") {
        let turn = text(row, "turn_id");
        if turn.is_empty() {
            continue;
        }
        let id = text(row, "id");
        by_message.insert(id, turn);
        if let Some(previous) = answers.insert(turn, id) {
            lead_ins.entry(turn).or_default().push(previous.to_owned());
        }
        metadata.insert(turn, json(row, "metadata"));
    }
    // A tool result can arrive without any answer chunks. It is work, not
    // evidence of a live answer; keep the legacy live projection unchanged.
    let mut tool_turns: Vec<_> = extras
        .steps
        .iter()
        .filter(|(id, _)| {
            !answers.contains_key(id.as_str()) && !live.iter().any(|turn| turn.turn_id == **id)
        })
        .map(|(id, steps)| {
            (
                steps
                    .iter()
                    .filter_map(|step| step.started_at_ms)
                    .min()
                    .unwrap_or(i64::MAX),
                id.as_str(),
            )
        })
        .collect();
    tool_turns.sort_unstable();
    let mut items = Vec::new();
    let mut folds = Vec::new();
    let mut shown = HashSet::new();
    let turns = messages
        .iter()
        .filter_map(|message| by_message.get(message.id.as_str()).copied())
        .chain(live.iter().map(|turn| turn.turn_id.as_str()))
        .chain(tool_turns.iter().map(|(_, id)| *id));
    for id in turns {
        if !shown.insert(id) {
            continue;
        }
        let active = live.iter().find(|turn| turn.turn_id == id);
        let steps = extras.steps.get(id).cloned().unwrap_or_default();
        let details = metadata.get(id).and_then(live::details);
        let interrupted = active.is_some_and(|turn| turn.stopped)
            || steps.iter().any(|step| step.status == Step::Interrupted)
            || details.as_ref().and_then(|d| d.finish.as_deref()) == Some("interrupted");
        let waiting = pending.contains(id);
        let running = active.is_some() || steps.iter().any(|step| step.status == Step::Started);
        let refused = (!answers.contains_key(id)
            && steps.iter().any(|step| step.status == Step::Refused))
            || details
                .as_ref()
                .is_some_and(|d| d.error.is_some() || d.finish.as_deref() == Some("refused"));
        let (status, label) = if waiting {
            (Status::Waiting, "Waiting for you")
        } else if steps.iter().any(|step| step.status == Step::Parked) {
            (Status::Parked, "Paused")
        } else if interrupted {
            (Status::Interrupted, "Interrupted")
        } else if running {
            (Status::Running, "Working")
        } else if refused {
            (Status::Refused, "Refused")
        } else {
            (Status::Done, "Finished")
        };
        let mut failures: Vec<AgentTurnFailureVm> = steps
            .iter()
            .filter(|step| matches!(step.status, Step::Refused | Step::Interrupted))
            .map(|step| AgentTurnFailureVm {
                step_id: step.id.clone(),
                sentence: format!(
                    "{} · {}{}",
                    step.name,
                    step.status.label(),
                    step.result
                        .as_ref()
                        .map(|result| format!(": {}", one_line(result)))
                        .unwrap_or_default()
                ),
            })
            .collect();
        if let Some(error) = details.as_ref().and_then(|d| d.error.as_deref()) {
            failures.push(AgentTurnFailureVm {
                step_id: String::new(),
                sentence: one_line(error),
            });
        }
        let duration_ms = details
            .as_ref()
            .and_then(|d| d.duration_ms)
            .filter(|ms| *ms >= 0);
        let summary = summary(
            &steps,
            active.and_then(|turn| turn.tool.as_deref()),
            status,
            duration_ms,
        );
        folds.push(AgentTurnFoldVm {
            turn_id: id.to_owned(),
            lead_in_ids: lead_ins.remove(id).unwrap_or_default(),
            reasoning: active
                .and_then(|turn| turn.reasoning.clone())
                .or_else(|| live::ended_thinking(extras, id)),
            steps,
            failures,
            status,
            status_label: label.to_owned(),
            summary,
            duration_ms,
        });
    }
    // Fold then terminal answer, never a work item below the answer.
    let mut emitted = HashSet::new();
    for message in messages {
        if let Some(id) = by_message.get(message.id.as_str()) {
            if emitted.insert(*id) {
                items.push(Item::Fold {
                    turn_id: (*id).to_owned(),
                });
                if let Some(answer) = answers.get(id) {
                    items.push(Item::Message {
                        message_id: (*answer).to_owned(),
                    });
                }
            }
        } else {
            items.push(Item::Message {
                message_id: message.id.clone(),
            });
        }
    }
    for turn in live {
        if emitted.insert(turn.turn_id.as_str()) {
            items.push(Item::Fold {
                turn_id: turn.turn_id.clone(),
            });
            items.push(Item::LiveAnswer {
                turn_id: turn.turn_id.clone(),
            });
        }
    }
    for (_, id) in tool_turns {
        items.push(Item::Fold {
            turn_id: id.to_owned(),
        });
    }
    (items, folds)
}

fn one_line(text: &str) -> String {
    let mut line = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    line
}

fn summary(
    steps: &[AgentToolStepVm],
    tool: Option<&str>,
    status: Status,
    duration_ms: Option<i64>,
) -> String {
    use std::fmt::Write as _;

    let mut sentence = match status {
        Status::Waiting => "Waiting for you".to_owned(),
        Status::Parked => "Paused".to_owned(),
        Status::Interrupted => "Interrupted".to_owned(),
        Status::Running => match tool.or_else(|| {
            steps
                .iter()
                .rev()
                .find(|step| step.status == Step::Started)
                .map(|step| step.name.as_str())
        }) {
            Some("web_search") => "Searching the web…".to_owned(),
            Some("run") => "Running a command…".to_owned(),
            Some(name) => format!("Calling {name}…"),
            None => "Working…".to_owned(),
        },
        _ => duration_ms
            .map(|ms| format!("Worked for {} s", ms / 1000))
            .unwrap_or_else(|| "Worked".to_owned()),
    };
    if status == Status::Running {
        if !steps.is_empty() {
            let _ = write!(
                sentence,
                " · {} {}",
                steps.len(),
                if steps.len() == 1 { "step" } else { "steps" }
            );
        }
        return sentence;
    }
    let mut counts = BTreeMap::new();
    for step in steps {
        *counts.entry(step.name.as_str()).or_insert(0usize) += 1;
    }
    for (index, (name, count)) in counts.into_iter().enumerate() {
        sentence.push_str(if index == 0 { " · " } else { ", " });
        match name {
            "web_search" => {
                let _ = write!(
                    sentence,
                    "searched the web {count} {}",
                    if count == 1 { "time" } else { "times" }
                );
            }
            "run" => {
                let _ = write!(
                    sentence,
                    "ran {count} {}",
                    if count == 1 { "command" } else { "commands" }
                );
            }
            _ => {
                let _ = write!(
                    sentence,
                    "called {name} {count} {}",
                    if count == 1 { "time" } else { "times" }
                );
            }
        }
    }
    sentence
}
