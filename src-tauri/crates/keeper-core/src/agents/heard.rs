//! Where the person stopped listening to a spoken answer (AD-411, story 97.3):
//! the replay's cut.
//!
//! A log's `heard` line marks one answer: how many of its sentences were
//! finished aloud and where the last of them ends (`heard_until`). It never
//! cancels the turn (R44, DW-419). Since 108.2 a person's device says where
//! it stopped with a heard row in the agents' database
//! (`keeper_core::hq::spoken`, R-HQ-68); the Matrix event the device sent
//! before and its room drawing are gone. What stays is the rule both
//! replays and the claim holder's next context apply:
//!
//! - **The answer is the stream's.** An answer is every round of its turn
//!   joined the way the stream joined them ([`answer_text`]): the rounds'
//!   `assistant` lines up to the closing one, which carries the anchor.
//!   [`Answers`] follows a session's lines and keeps, per anchor, those
//!   rounds.
//! - **Both replays cut alike.** The cold replay and a served session's
//!   context feed every conversation line to [`Answers::observe`], which
//!   answers a `heard` line with the [`Cut`] to make: the rounds keep their
//!   text up to `heard_until`, the round holding the cut gets
//!   [`STOPPED_LISTENING`] after it, later rounds lose their text and keep
//!   their tool calls ([`cut_rounds`]). The log line keeps the whole answer;
//!   only the replayed message is cut.

use std::collections::HashMap;

use ulid::Ulid;

use crate::agents::log::{ApprovalState, LineBody, LogLine, WITHHELD_CODE};
use crate::bots::chat::{ChatMessage, ContentPart};

/// What a cut answer says where the person stopped listening.
pub const STOPPED_LISTENING: &str = "[The person stopped listening here.]";

/// One anchored answer of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// Its rounds' `assistant` lines in the room's order; the last is the
    /// line naming the anchor, which a `heard` line names as `assistant`.
    pub rounds: Vec<Ulid>,
    /// Its room showed a fixed sentence instead (R168).
    pub withheld: bool,
    /// A `heard` line marks it.
    pub marked: bool,
}

/// The cut a `heard` line makes in a replayed answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cut {
    /// The answer's rounds, as [`Answer::rounds`].
    pub rounds: Vec<Ulid>,
    pub heard_until: u64,
}

impl Cut {
    /// Cut the answer in `messages` — every replayed message with the line
    /// that made it. Nothing is cut when one of the rounds has no message
    /// any more (a compact replaced it).
    pub fn apply<'a>(&self, messages: impl IntoIterator<Item = (Ulid, &'a mut ChatMessage)>) {
        let mut slots: Vec<Option<&'a mut ChatMessage>> =
            self.rounds.iter().map(|_| None).collect();
        for (line, message) in messages {
            if let Some(at) = self.rounds.iter().position(|round| *round == line) {
                slots[at] = Some(message);
            }
        }
        let Some(mut slots) = slots.into_iter().collect::<Option<Vec<_>>>() else {
            return;
        };
        let texts: Vec<String> = slots.iter().map(|message| message_text(message)).collect();
        let Some(kept) = cut_rounds(&texts, self.heard_until) else {
            return;
        };
        for (message, text) in slots.iter_mut().zip(kept) {
            message.content = if text.is_empty() {
                Vec::new()
            } else {
                vec![ContentPart::Text(text)]
            };
        }
    }
}

/// A message's text: its text parts, in order.
pub fn message_text(message: &ChatMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// A session's anchored answers, followed line by line in log order.
#[derive(Debug, Clone, Default)]
pub struct Answers {
    /// The `assistant` lines of the stream under way, in order.
    open: Vec<Ulid>,
    /// Each answer's last line, by its anchor.
    by_anchor: HashMap<String, Ulid>,
    answers: HashMap<Ulid, Answer>,
    /// A turn's closing lines — its anchored line, the `error` under it —
    /// with the answer each closes.
    closings: HashMap<Ulid, Ulid>,
}

impl Answers {
    /// Take one conversation line (a helper's step and a review pass's
    /// lines are none): the cut a `heard` line makes, if it makes one.
    ///
    /// A stream starts at a `user` or `peer` line and again where a call
    /// parks the turn (its resumption speaks under a new anchor); every
    /// `assistant` line after that is a round of it, and the one naming an
    /// anchor closes it.
    pub fn observe(&mut self, line: &LogLine) -> Option<Cut> {
        match &line.body {
            LineBody::User(_) | LineBody::Peer(_) => self.open.clear(),
            LineBody::Approval(approval) if approval.state == ApprovalState::Requested => {
                self.open.clear()
            }
            LineBody::Assistant(body) => {
                self.open.push(line.id);
                if let Some(anchor) = &body.anchor_event {
                    let rounds = std::mem::take(&mut self.open);
                    self.closings.insert(line.id, line.id);
                    self.by_anchor.insert(anchor.clone(), line.id);
                    self.answers.insert(
                        line.id,
                        Answer {
                            rounds,
                            withheld: false,
                            marked: false,
                        },
                    );
                }
            }
            LineBody::Error(error) => {
                let key = line
                    .parent
                    .and_then(|parent| self.closings.get(&parent).copied())?;
                if error.code == WITHHELD_CODE {
                    if let Some(answer) = self.answers.get_mut(&key) {
                        answer.withheld = true;
                    }
                } else {
                    self.closings.insert(line.id, key);
                }
            }
            LineBody::Heard(heard) => {
                let answer = self.answers.get_mut(&heard.assistant)?;
                if std::mem::replace(&mut answer.marked, true) || answer.withheld {
                    return None;
                }
                return Some(Cut {
                    rounds: answer.rounds.clone(),
                    heard_until: heard.heard_until,
                });
            }
            _ => {}
        }
        None
    }

    /// The answer whose room message is `anchor`.
    pub fn anchored(&self, anchor: &str) -> Option<&Answer> {
        self.by_anchor
            .get(anchor)
            .and_then(|key| self.answers.get(key))
    }
}

/// The answer as its room showed it: each round's text, a blank line before
/// a round with text unless the text so far is empty or ends with a line
/// break — the join the stream makes as each round starts. A closing line
/// with no text (an `ask`, a reply at the gate) is no round of the room's
/// and adds no blank line.
pub fn answer_text<S: AsRef<str>>(rounds: &[S]) -> String {
    let mut text = String::new();
    for (round, said) in rounds.iter().enumerate() {
        let said = said.as_ref();
        if round > 0 && !said.is_empty() && !text.is_empty() && !text.ends_with('\n') {
            text.push_str("\n\n");
        }
        text.push_str(said);
    }
    text
}

/// Each round's text once the person stopped at `heard_until` of
/// [`answer_text`]: what came before it kept, [`STOPPED_LISTENING`] after the
/// last round that keeps any words (the first round when none does); a round
/// that starts after the cut keeps nothing. `None` when the whole answer was
/// heard.
pub fn cut_rounds<S: AsRef<str>>(rounds: &[S], heard_until: u64) -> Option<Vec<String>> {
    let until = usize::try_from(heard_until).unwrap_or(usize::MAX);
    let mut kept = Vec::with_capacity(rounds.len());
    let mut count = 0usize;
    let mut last: Option<char> = None;
    for (round, said) in rounds.iter().enumerate() {
        let said = said.as_ref();
        if round > 0 && !said.is_empty() && last.is_some_and(|c| c != '\n') {
            count += 2;
            last = Some('\n');
        }
        let take = until.saturating_sub(count);
        let end = said
            .char_indices()
            .nth(take)
            .map_or(said.len(), |(at, _)| at);
        kept.push(said[..end].to_owned());
        count += said.chars().count();
        if let Some(c) = said.chars().next_back() {
            last = Some(c);
        }
    }
    if until >= count {
        return None;
    }
    let holder = kept
        .iter()
        .rposition(|text| !text.trim().is_empty())
        .unwrap_or(0);
    if let Some(text) = kept.get_mut(holder) {
        *text = if text.trim().is_empty() {
            STOPPED_LISTENING.to_owned()
        } else {
            format!("{}\n\n{STOPPED_LISTENING}", text.trim_end())
        };
    }
    Some(kept)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::{TimeZone, Utc};
    use serde_json::Value;

    use super::*;
    use crate::agents::label::{Integrity, Label, Readers};
    use crate::agents::log::reader::SessionLog;
    use crate::agents::log::replay::replay;
    use crate::agents::log::{
        ApprovalBody, AssistantBody, ErrorBody, HeardBody, HeardReason, HostSlug, ToolCallBody,
        ToolOutcomeWord, ToolResultBody, Usage, UserBody, LINE_VERSION,
    };
    use crate::agents::nudge::TOOL_ROUND_FINISH;
    use crate::bots::chat::Role;

    /// A log built line by line, each line's id and time after the last.
    #[derive(Default)]
    struct Log {
        lines: Vec<LogLine>,
    }

    impl Log {
        fn push(&mut self, parent: Option<Ulid>, body: LineBody) -> Ulid {
            let seq = self.lines.len() as i64;
            let id = Ulid::from_parts(1_000 + seq as u64, 0);
            self.lines.push(LogLine {
                v: LINE_VERSION,
                id,
                parent,
                ts: Utc.timestamp_millis_opt(1_000 + seq).single().expect("ts"),
                host: HostSlug::new("electra").expect("slug"),
                epoch: 0,
                claim: None,
                matrix_event: None,
                body,
            });
            id
        }

        fn user(&mut self, text: &str) -> Ulid {
            self.push(
                None,
                LineBody::User(UserBody {
                    sender: "@tgorka:example.org".try_into().expect("user"),
                    text: text.to_owned(),
                    attachments: Vec::new(),
                }),
            )
        }

        fn said(&mut self, parent: Option<Ulid>, text: &str, anchor: Option<&str>) -> Ulid {
            self.push(
                parent,
                LineBody::Assistant(AssistantBody {
                    bot: None,
                    effort: None,
                    text: text.to_owned(),
                    model: "model".to_owned(),
                    finish: if anchor.is_some() {
                        "stop"
                    } else {
                        TOOL_ROUND_FINISH
                    }
                    .to_owned(),
                    usage: Usage::default(),
                    ttft_ms: None,
                    duration_ms: 0,
                    anchor_event: anchor.map(str::to_owned),
                }),
            )
        }

        /// A round that says `text` and calls a tool, with its result.
        fn round(&mut self, text: &str, call: &str) -> Ulid {
            let round = self.said(None, text, None);
            let called = self.push(
                Some(round),
                LineBody::ToolCall(ToolCallBody {
                    call_id: call.to_owned(),
                    tool: "drive_read".to_owned(),
                    args: "{}".to_owned(),
                    tier: 0,
                    grant_id: None,
                }),
            );
            self.push(
                Some(called),
                LineBody::ToolResult(ToolResultBody {
                    call_id: call.to_owned(),
                    outcome: ToolOutcomeWord::Ok,
                    content: "read".to_owned(),
                    truncated: None,
                    label: Label {
                        readers: Readers::Only(BTreeSet::new()),
                        integrity: Integrity::Owner,
                        local_only: false,
                    },
                    paseo: None,
                    effect: None,
                }),
            );
            round
        }

        fn error(&mut self, parent: Ulid, code: &str) -> Ulid {
            self.push(
                Some(parent),
                LineBody::Error(ErrorBody {
                    sentence: "said".to_owned(),
                    code: code.to_owned(),
                }),
            )
        }

        fn heard(&mut self, assistant: Ulid, heard_until: u64, sentence: u32) -> Ulid {
            self.push(
                None,
                LineBody::Heard(HeardBody {
                    assistant,
                    heard_until,
                    sentence,
                    reason: HeardReason::Stop,
                }),
            )
        }

        fn answers(&self) -> Answers {
            let mut answers = Answers::default();
            for line in &self.lines {
                answers.observe(line);
            }
            answers
        }

        fn replayed(&self) -> Vec<ChatMessage> {
            let log = SessionLog {
                lines: self.lines.clone(),
                ..SessionLog::default()
            };
            replay(&log, &|_| Ok(Value::Null)).expect("replay").messages
        }
    }

    /// 97.3 #7 (the anchor → lines mapping): an anchor names its turn's
    /// rounds, from the question or from where a parked call resumed under a
    /// new anchor, and never a round a cut-off turn left behind; an answer
    /// the room never showed is withheld, whether its turn completed or
    /// failed; an unknown event is no answer.
    #[test]
    fn an_anchor_names_its_turns_rounds() {
        let mut log = Log::default();
        log.user("first");
        let first = log.round("One. Two.", "c1");
        let closing = log.said(None, "Three.", Some("$a1:h"));

        log.user("second");
        log.round("Let me check.", "c2");
        log.push(
            None,
            LineBody::Approval(ApprovalBody {
                id: "ap".to_owned(),
                state: ApprovalState::Requested,
                decision: None,
                by: None,
                result: None,
                reason: None,
                scope: None,
            }),
        );
        let resumed = log.said(None, "Done.", Some("$a2:h"));

        // A turn a restart cut off after a round: its round answers nothing.
        log.user("cut off");
        log.round("Lost.", "c0");

        log.user("third");
        let withheld = log.said(None, "Secret.", Some("$a3:h"));
        log.error(withheld, WITHHELD_CODE);

        log.user("fourth");
        let half = log.round("Half.", "c4");
        let partial = log.said(None, "More", Some("$a4:h"));
        let failed = log.error(partial, "turn_failed");
        log.error(failed, WITHHELD_CODE);

        let answers = log.answers();
        let answer = |anchor| answers.anchored(anchor).cloned();
        assert_eq!(
            answer("$a1:h"),
            Some(Answer {
                rounds: vec![first, closing],
                withheld: false,
                marked: false
            })
        );
        assert_eq!(answer("$a2:h").map(|a| a.rounds), Some(vec![resumed]));
        assert_eq!(
            answer("$a3:h").map(|a| (a.rounds, a.withheld)),
            Some((vec![withheld], true))
        );
        assert_eq!(
            answer("$a4:h").map(|a| (a.rounds, a.withheld)),
            Some((vec![half, partial], true))
        );
        assert_eq!(answer("$nope:h"), None);
    }

    fn assistant(text: &str, calls: &[&str]) -> (Role, String, Vec<String>) {
        (
            Role::Assistant,
            text.to_owned(),
            calls.iter().map(|c| (*c).to_owned()).collect(),
        )
    }

    fn shape(messages: &[ChatMessage]) -> Vec<(Role, String, Vec<String>)> {
        messages
            .iter()
            .map(|m| {
                (
                    m.role,
                    message_text(m),
                    m.tool_calls.iter().map(|c| c.id.clone()).collect(),
                )
            })
            .collect()
    }

    /// 97.3 #8: replay cuts a marked answer at `heard_until` across its
    /// rounds — the round holding the cut keeps its calls and ends with the
    /// note, later rounds keep their calls and lose their text — counting
    /// Unicode scalar values; an answer with no `heard` line, one heard to
    /// its end (also when its closing line has no text), a withheld one and
    /// a second mark replay as they were.
    #[test]
    fn replay_truncates_at_heard_until() {
        let mut log = Log::default();
        log.user("q1");
        log.round("One. Two.", "c1");
        let closing = log.said(None, "Three. Four.", Some("$a1:h"));
        log.user("q2");
        log.said(None, "Fine. Whole.", Some("$a2:h"));
        let unmarked = log.replayed();

        let mut cut = Log {
            lines: log.lines.clone(),
        };
        cut.heard(closing, 17, 3);
        // A later mark of the same answer is not the first.
        cut.heard(closing, 4, 1);
        let tool = |m: &ChatMessage| (m.role, message_text(m), Vec::new());
        let replayed = cut.replayed();
        assert_eq!(
            shape(&replayed),
            vec![
                (Role::User, "q1".to_owned(), vec![]),
                assistant("One. Two.", &["c1"]),
                tool(&replayed[2]),
                assistant(&format!("Three.\n\n{STOPPED_LISTENING}"), &[]),
                (Role::User, "q2".to_owned(), vec![]),
                assistant("Fine. Whole.", &[]),
            ]
        );
        assert_eq!(shape(&unmarked)[5], assistant("Fine. Whole.", &[]));
        assert_eq!(shape(&unmarked)[3], assistant("Three. Four.", &[]));

        // Stopped after the first sentence: the cut lies in the first
        // round, and the last round keeps nothing.
        let mut early = Log {
            lines: log.lines.clone(),
        };
        early.heard(closing, 4, 1);
        let replayed = shape(&early.replayed());
        assert_eq!(
            replayed[1],
            assistant(&format!("One.\n\n{STOPPED_LISTENING}"), &["c1"])
        );
        assert_eq!(replayed[3], assistant("", &[]));

        // Nothing heard: the note alone.
        let mut none = Log::default();
        none.user("q");
        let only = none.said(None, "Zażółć. Gęślą jaźń.", Some("$z:h"));
        let mut multi = Log {
            lines: none.lines.clone(),
        };
        multi.heard(only, 7, 1);
        none.heard(only, 0, 0);
        assert_eq!(
            shape(&none.replayed())[1],
            assistant(STOPPED_LISTENING, &[])
        );
        assert_eq!(
            shape(&multi.replayed())[1],
            assistant(&format!("Zażółć.\n\n{STOPPED_LISTENING}"), &[])
        );

        // Three rounds: every blank line between them counts, so the cut
        // in the third falls just after its first sentence.
        let mut three = Log::default();
        three.user("q");
        three.round("One.", "c1");
        three.round("Two.", "c2");
        let last = three.said(None, "Three. Four.", Some("$t:h"));
        // "One.\n\nTwo.\n\nThree. Four.": the third sentence ends at 18.
        three.heard(last, 18, 3);
        let replayed = shape(&three.replayed());
        assert_eq!(replayed[3], assistant("Two.", &["c2"]));
        assert_eq!(
            replayed[5],
            assistant(&format!("Three.\n\n{STOPPED_LISTENING}"), &[])
        );

        // Heard to the end, and withheld: whole.
        let mut whole = Log {
            lines: log.lines.clone(),
        };
        whole.heard(closing, 23, 4);
        assert_eq!(shape(&whole.replayed()), shape(&unmarked));
        // Heard to the end of an answer an `ask` closed with no text: no
        // blank line follows the last sentence, so nothing is cut.
        let mut asked = Log::default();
        asked.user("q");
        asked.round("One. Two.", "c1");
        let closed = asked.said(None, "", Some("$q:h"));
        let before = shape(&asked.replayed());
        asked.heard(closed, 9, 2);
        assert_eq!(shape(&asked.replayed()), before);
        let mut withheld = Log {
            lines: log.lines.clone(),
        };
        withheld.error(closing, WITHHELD_CODE);
        let before = shape(&withheld.replayed());
        withheld.heard(closing, 4, 1);
        assert_eq!(shape(&withheld.replayed()), before);
    }
}
