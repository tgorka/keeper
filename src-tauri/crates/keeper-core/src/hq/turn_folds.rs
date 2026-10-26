//! Rust's ordered reading column for an HQ turn (109.13).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Map, Value};

use super::{
    AgentMessageVm, AgentTimelineItemVm as Item, AgentTurnFailureVm, AgentTurnFoldVm,
    AgentTurnStatusVm as Status,
};
use crate::hq::live::{self, AgentToolStepStatusVm as Step, AgentToolStepVm, Extras, LiveTurn};
use crate::hq::sessions::{int, is_ask, json, text};

/// How the runtime closed a turn, by its Closed row's `metadata.finish`.
/// Every word keeper-hq writes is named; any other word is a provider's
/// own finish reason, an ordinary close.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Close {
    /// `stop`, `length`, `tool_calls`, an unreported (null) reason, or a
    /// provider's own word.
    Normal,
    /// `cancelled`: a person's Stop (109.19), `served.rs` `Ending::Stopped`.
    Stopped,
    /// `cut`: the instance restarted mid-turn, `worker.rs` `end_cut`.
    Interrupted,
    /// `failed`: a broken stream or a provider/runtime error.
    Failed,
    /// `content_filter`: the provider refused to answer.
    Refused,
}

impl Close {
    fn of(finish: Option<&str>) -> Self {
        match finish {
            Some("cancelled") => Self::Stopped,
            Some("cut") => Self::Interrupted,
            Some("failed") => Self::Failed,
            Some("content_filter") => Self::Refused,
            _ => Self::Normal,
        }
    }
}

/// All steps stay with their turn, even when committed after its answer.
/// `live` is oldest first; `stopping` holds the turns a person's stop row
/// names.
pub(super) fn project(
    rows: &[Map<String, Value>],
    messages: &[AgentMessageVm],
    live: &[LiveTurn],
    extras: &Extras,
    pending: &HashSet<String>,
    stopping: &HashSet<String>,
) -> (Vec<Item>, Vec<AgentTurnFoldVm>) {
    // A terminal row's turn, by message id; the turn's last close, which
    // decides its status; every close's error, so none disappears.
    let mut answers: HashMap<&str, &str> = HashMap::new();
    let mut last_close: HashMap<&str, (i64, &str, Value)> = HashMap::new();
    let mut errors: HashMap<&str, Vec<String>> = HashMap::new();
    let mut lead_ins: HashMap<&str, Vec<String>> = HashMap::new();
    let mut hidden = HashSet::new();
    let mut work_at = HashMap::new();
    let active_by_turn: HashMap<_, _> = live
        .iter()
        .map(|turn| (turn.vm.turn_id.as_str(), turn))
        .collect();
    for row in rows.iter().filter(|row| text(row, "role") == "agent") {
        let turn = text(row, "turn_id");
        let details = json(row, "metadata");
        // An ask is a question to the person (R-HQ-28): always read where
        // it was asked, never a lead-in, never an answer, never work.
        if turn.is_empty() || is_ask(&details) {
            continue;
        }
        let id = text(row, "id");
        let at = int(row, "created_at_ms").unwrap_or(i64::MAX);
        if live::is_terminal(&details) {
            answers.insert(id, turn);
            if let Some(error) = live::details(&details).and_then(|d| d.error) {
                errors.entry(turn).or_default().push(one_line(&error));
            }
            let later = last_close
                .get(turn)
                .is_none_or(|(when, which, _)| (at, id) > (*when, *which));
            if later {
                last_close.insert(turn, (at, id, details));
            }
        } else {
            hidden.insert(id);
            lead_ins.entry(turn).or_default().push(id.to_owned());
        }
        let first = work_at.entry(turn).or_insert(at);
        *first = (*first).min(at);
    }
    // Work without an answer has its own historical position, never a
    // fabricated live answer or an append-only slot below newer answers.
    for (id, steps) in &extras.steps {
        let at = steps
            .iter()
            .filter_map(|step| step.started_at_ms)
            .min()
            .unwrap_or(i64::MAX);
        let first = work_at.entry(id.as_str()).or_insert(at);
        *first = (*first).min(at);
    }
    let answered: HashSet<&str> = answers.values().copied().collect();
    enum Reading<'a> {
        Message(&'a AgentMessageVm),
        Work(&'a str),
        Live(&'a LiveTurn),
    }
    let mut reading: Vec<_> = messages
        .iter()
        .filter(|message| !hidden.contains(message.id.as_str()))
        .map(|message| {
            (
                message.created_at_ms,
                message.id.as_str(),
                Reading::Message(message),
            )
        })
        .chain(work_at.into_iter().filter_map(|(id, at)| {
            (!answered.contains(id) && !active_by_turn.contains_key(id)).then_some((
                at,
                id,
                Reading::Work(id),
            ))
        }))
        .collect();
    reading.sort_unstable_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    // A streaming answer sits where its Closed row will: that row is
    // written when the turn ends, after everything written so far.
    reading.extend(
        live.iter()
            .map(|turn| (i64::MAX, turn.vm.turn_id.as_str(), Reading::Live(turn))),
    );
    let mut items = Vec::new();
    let mut folds = Vec::new();
    let mut folded = HashSet::new();
    for (_, _, entry) in reading {
        let (id, answer) = match entry {
            Reading::Message(message) => {
                let answer = Item::Message {
                    message_id: message.id.clone(),
                };
                // The turn's first finished answer opens its one fold; a
                // later one (a kill-cut row, then a late close) is read
                // where it was written.
                match answers.get(message.id.as_str()) {
                    Some(turn) if !folded.contains(turn) => (*turn, Some(answer)),
                    _ => {
                        items.push(answer);
                        continue;
                    }
                }
            }
            Reading::Work(id) => (id, None),
            Reading::Live(turn) => (
                turn.vm.turn_id.as_str(),
                Some(Item::LiveAnswer {
                    turn_id: turn.vm.turn_id.clone(),
                }),
            ),
        };
        folded.insert(id);
        items.push(Item::Fold {
            turn_id: id.to_owned(),
        });
        if let Some(answer) = answer {
            items.push(answer);
        }
        let active = active_by_turn.get(id).copied();
        let steps = extras.steps.get(id).cloned().unwrap_or_default();
        let details = last_close
            .get(id)
            .and_then(|(_, _, metadata)| live::details(metadata));
        let close = last_close
            .contains_key(id)
            .then(|| Close::of(details.as_ref().and_then(|d| d.finish.as_deref())));
        // A person's stop decides while it lands; once the turn has closed,
        // its close does (a stop that lost the race to a normal close).
        let stopped = close.map_or_else(|| stopping.contains(id), |c| c == Close::Stopped);
        let interrupted = close == Some(Close::Interrupted)
            || active.is_some_and(|turn| turn.vm.stopped)
            || steps.iter().any(|step| step.status == Step::Interrupted);
        let waiting = pending.contains(id);
        // Only a step the runtime says started is running work, and only
        // until the turn closes: the close decides a closed turn, even over
        // a step its row still calls started. An unknown status word is
        // never running work.
        let running = active.is_some()
            || (close.is_none() && steps.iter().any(|step| step.status == Step::Started));
        let failed = close == Some(Close::Failed)
            || (close.is_none() && steps.iter().any(|step| step.status == Step::Failed));
        let refused = close == Some(Close::Refused)
            || (close.is_none() && steps.iter().any(|step| step.status == Step::Refused));
        let (status, label) = if stopped {
            (Status::Stopped, "Stopped")
        } else if waiting {
            (Status::Waiting, "Waiting for you")
        } else if steps.iter().any(|step| step.status == Step::Parked) {
            (Status::Parked, "Paused")
        } else if interrupted {
            (Status::Interrupted, "Interrupted")
        } else if running {
            (Status::Running, "Working")
        } else if failed {
            (Status::Failed, "Failed")
        } else if refused {
            (Status::Refused, "Refused")
        } else {
            (Status::Done, "Finished")
        };
        // A step that was refused, interrupted or failed, or whose status
        // keeper does not know, keeps one always-visible line in Rust's words.
        let mut failures: Vec<AgentTurnFailureVm> = steps
            .iter()
            .filter(|step| {
                matches!(
                    step.status,
                    Step::Refused | Step::Interrupted | Step::Failed | Step::Unknown
                )
            })
            .map(|step| AgentTurnFailureVm {
                step_id: step.id.clone(),
                sentence: format!(
                    "{} · {}{}",
                    step.name,
                    step.status_label,
                    step.result
                        .as_ref()
                        .map(|result| format!(": {}", one_line(result)))
                        .unwrap_or_default()
                ),
            })
            .collect();
        failures.extend(
            errors
                .remove(id)
                .unwrap_or_default()
                .into_iter()
                .map(|sentence| AgentTurnFailureVm {
                    step_id: String::new(),
                    sentence,
                }),
        );
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
        Status::Stopped => "Stopped".to_owned(),
        Status::Failed => "Failed".to_owned(),
        // The fold line is the turn's one visible status: a refusal says so.
        Status::Refused => "Refused".to_owned(),
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
