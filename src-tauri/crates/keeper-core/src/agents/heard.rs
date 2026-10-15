//! Where the person stopped listening to a spoken answer (AD-411, story 97.3).
//!
//! When the person stops an answer keeper is speaking, their device sends
//! `dev.keeper.agent.heard` ([`HeardContent`]) into the proxy's room: the
//! answer's anchor, how many of its sentences the synthesiser finished and
//! where the last of them ends. The owning host reads it here and writes one
//! `heard` line; it never cancels the turn (R44, DW-419).
//!
//! - **The answer is the room's.** An answer's room message is every round
//!   of its turn joined the way the stream joined them ([`answer_text`]):
//!   the rounds' `assistant` lines up to the closing one, which carries the
//!   anchor. [`Answers`] follows a session's lines and keeps, per anchor,
//!   those rounds.
//! - **The host counts.** [`Answers::mark`] checks the event against the
//!   answer and recomputes `heard_until` from `sentence` with the speech
//!   segmenter's own cut ([`sentence_ends`]) over that text, so the mark does
//!   not depend on which edit the device saw last; a device value that
//!   disagrees is named and not used (Q9). An answer is marked once; an
//!   answer its room never showed (withheld, R168) is counted over the
//!   fixed sentence the room showed instead ([`NARROWER_THAN_ROOM`]) and
//!   cut nothing.
//! - **Both replays cut alike.** The cold replay and a served session's
//!   context feed every conversation line to [`Answers::observe`], which
//!   answers a `heard` line with the [`Cut`] to make: the rounds keep their
//!   text up to `heard_until`, the round holding the cut gets
//!   [`STOPPED_LISTENING`] after it, later rounds lose their text and keep
//!   their tool calls. The log line keeps the whole answer; only the
//!   replayed message is cut.
//! - **The room draws it.** An agent room's header reader folds the person's
//!   own sealed marks ([`RoomMarks`]); the timeline sends, beside the
//!   stream, where each marked answer's heard text ends in the body it shows
//!   ([`HeardMarkVm`], [`split_utf16`]) — recounted from `sentence` like the
//!   host's, in UTF-16 so the front slices and counts nothing (UX-DR143).

use std::borrow::Cow;
use std::collections::HashMap;

use matrix_sdk::ruma::{EventId, OwnedEventId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use ulid::Ulid;

use crate::agents::events::{CONTENT_VERSION, HEARD};
use crate::agents::log::{
    ApprovalState, HeardBody, HeardReason, LineBody, LogLine, NARROWER_THAN_ROOM, WITHHELD_CODE,
};
use crate::bots::chat::{ChatMessage, ContentPart};
use crate::voice::speech::sentence_ends;

/// What a cut answer says where the person stopped listening.
pub const STOPPED_LISTENING: &str = "[The person stopped listening here.]";

/// `dev.keeper.agent.heard` (timeline, encrypted), from the person's
/// speaking device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeardContent {
    pub v: u32,
    /// The answer's anchor: the agent's message the device spoke.
    pub anchor: OwnedEventId,
    /// Where the last sentence the synthesiser finished ends in the
    /// answer's text, in Unicode scalar values; `0` when none finished.
    pub heard_until: u64,
    /// How many of the answer's sentences the synthesiser finished.
    pub sentence: u32,
    pub reason: HeardReason,
}

impl HeardContent {
    /// The event's content, if it reads as this version's.
    pub fn read(content: &Value) -> Option<HeardContent> {
        serde_json::from_value::<HeardContent>(content.clone())
            .ok()
            .filter(|heard| heard.v == CONTENT_VERSION)
    }
}

/// Why a `heard` event is not logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeardRefusal {
    /// It does not read as a `heard` content of this version.
    Unreadable,
    /// Its anchor is no answer of this session.
    NotAnAnswer,
    /// The answer is marked already.
    AlreadyMarked,
    /// A compact replaced the answer in the session's context.
    Gone,
    /// It counts more sentences, or reaches further, than the answer holds.
    PastTheAnswer,
}

/// A refused `heard` event, in the host's own log.
pub const UNREADABLE_HEARD: &str = "a heard event that does not read as one is ignored";
/// Its anchor is no answer here.
pub const NOT_AN_ANSWER: &str = "a heard event names an answer of this session or it is ignored";
/// A second mark of one answer.
pub const ALREADY_MARKED: &str =
    "an answer is marked once where the person stopped listening; a second mark is ignored";
/// The answer was compacted away.
pub const ANSWER_GONE: &str =
    "the answer a heard event names was compacted, so there is nothing to mark";
/// Past the answer's end.
pub const PAST_THE_ANSWER: &str = "a heard event past the end of its answer is ignored";

impl HeardRefusal {
    /// The host's note for it.
    pub fn note(self) -> &'static str {
        match self {
            Self::Unreadable => UNREADABLE_HEARD,
            Self::NotAnAnswer => NOT_AN_ANSWER,
            Self::AlreadyMarked => ALREADY_MARKED,
            Self::Gone => ANSWER_GONE,
            Self::PastTheAnswer => PAST_THE_ANSWER,
        }
    }
}

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

/// A mark the host logs, and the device's own `heard_until` when it
/// disagreed with the host's count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marked {
    pub body: HeardBody,
    pub disagreed: Option<u64>,
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

    /// The `heard` line `content` makes, checked against its answer, whose
    /// rounds' texts `texts` gives (`None` once a compact replaced one).
    pub fn mark(
        &self,
        content: &HeardContent,
        texts: impl FnOnce(&[Ulid]) -> Option<Vec<String>>,
    ) -> Result<Marked, HeardRefusal> {
        let answer = self
            .anchored(content.anchor.as_str())
            .ok_or(HeardRefusal::NotAnAnswer)?;
        if answer.marked {
            return Err(HeardRefusal::AlreadyMarked);
        }
        let assistant = *answer.rounds.last().ok_or(HeardRefusal::NotAnAnswer)?;
        // What the room showed was the fixed sentence: the mark is counted
        // over it like any answer, and cuts nothing.
        let text: Cow<'static, str> = if answer.withheld {
            Cow::Borrowed(NARROWER_THAN_ROOM)
        } else {
            Cow::Owned(answer_text(
                &texts(&answer.rounds).ok_or(HeardRefusal::Gone)?,
            ))
        };
        let ends = sentence_ends(&text);
        let length = text.chars().count() as u64;
        let sentence = usize::try_from(content.sentence).unwrap_or(usize::MAX);
        if content.heard_until > length || sentence > ends.len() {
            return Err(HeardRefusal::PastTheAnswer);
        }
        let heard_until = match sentence {
            0 => 0,
            n => ends[n - 1] as u64,
        };
        Ok(Marked {
            body: HeardBody {
                assistant,
                heard_until,
                sentence: content.sentence,
                reason: content.reason,
            },
            disagreed: (heard_until != content.heard_until).then_some(content.heard_until),
        })
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

/// Where the person stopped listening to an answer, as its agent room draws
/// it (UX-DR143): the answer's render key and where what was heard ends in
/// the body the room shows, in UTF-16 code units — what a JS string slices
/// by, so the front counts nothing. Sent beside the stream
/// (`TimelineBatch::heard`), never as an item (R33).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HeardMarkVm {
    /// The answer's stable opaque render key (its item's `unique_id`).
    pub key: String,
    /// The body's UTF-16 length of what was heard: the rule goes here and
    /// the text after it is dimmed.
    pub split_utf16: u32,
}

/// The marks the person's own devices left in a session room, by the
/// answer's anchor: how many of its sentences were finished aloud. Only an
/// event from the room's own user, sealed by a device of theirs
/// ([`crate::agents::room::sealed_by_sender`], R185), that reads as
/// [`HeardContent`] counts; of two marks of one answer the one with the
/// smaller `origin_server_ts` stands — the room's order, as the host logs
/// only the first ([`ALREADY_MARKED`]) — whichever the room folded first:
/// back-pagination can bring the older mark after the newer (R327).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoomMarks {
    /// Each answer's mark: its `origin_server_ts` and its `sentence`.
    by_anchor: HashMap<OwnedEventId, (u64, u32)>,
}

impl RoomMarks {
    /// Fold one event (decrypted, as JSON) `own` reads, `sealed` whether a
    /// device of its sender sealed it. Returns whether the marks changed.
    pub fn apply(&mut self, event: &Value, own: &UserId, sealed: bool) -> bool {
        if event["type"] != HEARD || event["sender"].as_str() != Some(own.as_str()) || !sealed {
            return false;
        }
        let Some(heard) = HeardContent::read(&event["content"]) else {
            return false;
        };
        let at = event["origin_server_ts"].as_u64().unwrap_or(u64::MAX);
        match self.by_anchor.entry(heard.anchor) {
            std::collections::hash_map::Entry::Occupied(mut entry) if at < entry.get().0 => {
                entry.insert((at, heard.sentence));
                true
            }
            std::collections::hash_map::Entry::Occupied(_) => false,
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((at, heard.sentence));
                true
            }
        }
    }

    /// How many sentences of the answer anchored at `anchor` were heard,
    /// if the person marked it.
    pub fn sentence(&self, anchor: &EventId) -> Option<u32> {
        self.by_anchor.get(anchor).map(|(_, sentence)| *sentence)
    }

    /// Whether no answer of the room is marked.
    pub fn is_empty(&self) -> bool {
        self.by_anchor.is_empty()
    }
}

/// Where the room draws the mark in an answer whose body is `body` once
/// `sentence` of its sentences were finished aloud: the end of that
/// sentence by the speech segmenter's own cut ([`sentence_ends`]), as the
/// host counts it — so the mark does not depend on which edit the device
/// saw last — converted from Unicode scalar values to UTF-16 code units;
/// `0` when none was finished. `None` when nothing but whitespace follows
/// it (the answer was heard to its end) or the body does not hold that
/// many sentences (yet).
pub fn split_utf16(body: &str, sentence: u32) -> Option<u32> {
    let ends = sentence_ends(body);
    let end = match usize::try_from(sentence).ok()? {
        0 => 0,
        n => *ends.get(n - 1)?,
    };
    let mut heard = body.chars();
    let units: usize = heard.by_ref().take(end).map(char::len_utf16).sum();
    if heard.as_str().trim().is_empty() {
        return None;
    }
    u32::try_from(units).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::*;
    use crate::agents::label::{Integrity, Label, Readers};
    use crate::agents::log::reader::SessionLog;
    use crate::agents::log::replay::replay;
    use crate::agents::log::{
        ApprovalBody, AssistantBody, ErrorBody, HostSlug, ToolCallBody, ToolOutcomeWord,
        ToolResultBody, Usage, UserBody, LINE_VERSION,
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

    fn heard(anchor: &str, heard_until: u64, sentence: u32) -> HeardContent {
        HeardContent::read(&json!({
            "v": 1,
            "anchor": anchor,
            "heard_until": heard_until,
            "sentence": sentence,
            "reason": "stop",
        }))
        .expect("a heard content")
    }

    fn texts_of(log: &Log) -> impl Fn(&[Ulid]) -> Option<Vec<String>> + '_ {
        |rounds| {
            rounds
                .iter()
                .map(|id| {
                    log.lines
                        .iter()
                        .find(|line| line.id == *id)
                        .and_then(|line| match &line.body {
                            LineBody::Assistant(body) => Some(body.text.clone()),
                            _ => None,
                        })
                })
                .collect()
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
        assert_eq!(
            answers.mark(&heard("$nope:h", 0, 0), texts_of(&log)),
            Err(HeardRefusal::NotAnAnswer)
        );
    }

    /// The `heard` line a mark of `assistant`'s answer makes.
    fn logged(
        assistant: Ulid,
        heard_until: u64,
        sentence: u32,
        disagreed: Option<u64>,
    ) -> Result<Marked, HeardRefusal> {
        Ok(Marked {
            body: HeardBody {
                assistant,
                heard_until,
                sentence,
                reason: HeardReason::Stop,
            },
            disagreed,
        })
    }

    /// Q9: the host counts the sentences the device says it finished over
    /// the answer as its room showed it (two rounds, a blank line between;
    /// none before a closing line with no text) and logs that offset,
    /// naming a device value that disagrees; a count or an offset past the
    /// answer, a second mark and a compacted answer are refused; a withheld
    /// answer is counted the same way over the sentence its room showed.
    #[test]
    fn the_host_counts_where_the_person_stopped() {
        let mut log = Log::default();
        log.user("q");
        log.round("One. Two.", "c1");
        let closing = log.said(None, "Three. Four.", Some("$a:h"));
        let answers = log.answers();
        // "One. Two.\n\nThree. Four." ends at 4, 9, 17 and 23.
        let mark = |content| answers.mark(&content, texts_of(&log));
        assert_eq!(mark(heard("$a:h", 17, 3)), logged(closing, 17, 3, None));
        assert_eq!(mark(heard("$a:h", 12, 3)), logged(closing, 17, 3, Some(12)));
        assert_eq!(mark(heard("$a:h", 0, 0)), logged(closing, 0, 0, None));
        assert_eq!(mark(heard("$a:h", 23, 4)), logged(closing, 23, 4, None));
        assert_eq!(mark(heard("$a:h", 23, 5)), Err(HeardRefusal::PastTheAnswer));
        assert_eq!(mark(heard("$a:h", 24, 4)), Err(HeardRefusal::PastTheAnswer));
        assert_eq!(
            answers.mark(&heard("$a:h", 4, 1), |_| None),
            Err(HeardRefusal::Gone)
        );

        let mut marked = log.answers();
        marked.observe(&LogLine {
            body: LineBody::Heard(HeardBody {
                assistant: closing,
                heard_until: 4,
                sentence: 1,
                reason: HeardReason::BargeIn,
            }),
            ..log.lines[0].clone()
        });
        assert_eq!(
            marked.mark(&heard("$a:h", 9, 2), texts_of(&log)),
            Err(HeardRefusal::AlreadyMarked)
        );

        // Closed by a line with no text (an `ask`): the room showed
        // "One. Two." and nothing after it, so 11 is past its end.
        let mut asked = Log::default();
        asked.user("q");
        asked.round("One. Two.", "c1");
        asked.said(None, "", Some("$q:h"));
        assert_eq!(
            asked
                .answers()
                .mark(&heard("$q:h", 11, 2), texts_of(&asked)),
            Err(HeardRefusal::PastTheAnswer)
        );

        // Withheld: counted over the sentence the room showed, whose
        // sentences end at 90 and 119, never over the answer's own text.
        let mut log = Log::default();
        log.user("q");
        let shown = log.said(None, "Secret words.", Some("$w:h"));
        log.error(shown, WITHHELD_CODE);
        let answers = log.answers();
        let mark = |content| answers.mark(&content, |_| None);
        assert_eq!(mark(heard("$w:h", 13, 1)), logged(shown, 90, 1, Some(13)));
        assert_eq!(mark(heard("$w:h", 119, 2)), logged(shown, 119, 2, None));
        assert_eq!(
            mark(heard("$w:h", 120, 1)),
            Err(HeardRefusal::PastTheAnswer)
        );
        assert_eq!(mark(heard("$w:h", 90, 3)), Err(HeardRefusal::PastTheAnswer));
    }

    /// The content reads only as this version's, with no field it does not
    /// know.
    #[test]
    fn a_heard_content_reads_only_as_version_one() {
        let content = json!({"v": 1, "anchor": "$a:h", "heard_until": 4, "sentence": 1, "reason": "barge_in"});
        assert!(HeardContent::read(&content).is_some());
        let mut later = content.clone();
        later["v"] = json!(2);
        assert_eq!(HeardContent::read(&later), None);
        let mut more = content;
        more["text"] = json!("what was said");
        assert_eq!(HeardContent::read(&more), None);
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

    const PERSON: &str = "@tgorka:example.org";

    fn mark_event(sender: &str, anchor: &str, sentence: u32) -> Value {
        mark_at(sender, anchor, sentence, 1)
    }

    fn mark_at(sender: &str, anchor: &str, sentence: u32, origin_server_ts: u64) -> Value {
        json!({
            "type": HEARD, "event_id": format!("$mark-{anchor}-{sentence}"), "sender": sender,
            "origin_server_ts": origin_server_ts,
            "content": {
                "v": 1, "anchor": anchor, "heard_until": 0, "sentence": sentence,
                "reason": "stop",
            },
        })
    }

    /// The room draws only the person's own marks, sealed by a device of
    /// theirs, readable as this version's, and of one answer's the first in
    /// the room's order, whichever was folded first (R327).
    #[test]
    fn the_room_folds_the_persons_own_sealed_marks() {
        let own = UserId::parse(PERSON).expect("user");
        let anchor = |id: &str| EventId::parse(id).expect("event");
        let mut marks = RoomMarks::default();
        assert!(marks.is_empty());

        // Anyone else's, an unsealed one and another type are not marks.
        assert!(!marks.apply(&mark_event("@nixi:example.org", "$a", 1), &own, true));
        assert!(!marks.apply(&mark_event(PERSON, "$a", 1), &own, false));
        let mut other = mark_event(PERSON, "$a", 1);
        other["type"] = json!("dev.keeper.agent.surface.result");
        assert!(!marks.apply(&other, &own, true));
        // Unreadable: a newer version, an unknown field.
        let mut newer = mark_event(PERSON, "$a", 1);
        newer["content"]["v"] = json!(2);
        assert!(!marks.apply(&newer, &own, true));
        let mut extra = mark_event(PERSON, "$a", 1);
        extra["content"]["cancel"] = json!(true);
        assert!(!marks.apply(&extra, &own, true));
        assert!(marks.is_empty());
        assert_eq!(marks.sentence(&anchor("$a")), None);

        assert!(marks.apply(&mark_event(PERSON, "$a", 2), &own, true));
        assert!(marks.apply(&mark_event(PERSON, "$b", 0), &own, true));
        // A second mark of one answer: the first stands.
        assert!(!marks.apply(&mark_event(PERSON, "$a", 5), &own, true));
        assert_eq!(marks.sentence(&anchor("$a")), Some(2));
        assert_eq!(marks.sentence(&anchor("$b")), Some(0));
        assert_eq!(marks.sentence(&anchor("$c")), None);

        // R327 (01): the first in the room's order stands, though the newer
        // mark was folded first (back-pagination brings the older later);
        // a later one, or one at the same time, does not replace it.
        let mut marks = RoomMarks::default();
        assert!(marks.apply(&mark_at(PERSON, "$d", 5, 20), &own, true));
        assert!(marks.apply(&mark_at(PERSON, "$d", 2, 10), &own, true));
        assert_eq!(marks.sentence(&anchor("$d")), Some(2));
        assert!(!marks.apply(&mark_at(PERSON, "$d", 7, 30), &own, true));
        assert!(!marks.apply(&mark_at(PERSON, "$d", 9, 10), &own, true));
        assert_eq!(marks.sentence(&anchor("$d")), Some(2));
    }

    /// The mark sits where the heard sentences end by the segmenter's cut,
    /// in UTF-16 code units of the body (an astral character is two, a
    /// combining mark one), and nowhere when the answer was heard to its end
    /// or does not hold that many sentences.
    #[test]
    fn a_mark_splits_the_body_in_utf16_where_the_heard_sentences_end() {
        let body = "Hi e\u{301} 😀. Zażółć gęślą!\nFour.\n";
        let heard = |split: u32| {
            let units: Vec<u16> = body.encode_utf16().collect();
            String::from_utf16(&units[..split as usize]).expect("a split between characters")
        };
        assert_eq!(sentence_ends(body).len(), 3);

        let first = split_utf16(body, 1).expect("one heard");
        assert_eq!(
            first, 9,
            "eight scalar values: the combining acute one unit, the emoji two"
        );
        assert_eq!(heard(first), "Hi e\u{301} 😀.");
        let second = split_utf16(body, 2).expect("two heard");
        assert_eq!(heard(second), "Hi e\u{301} 😀. Zażółć gęślą!");
        // None finished: the rule opens the answer.
        assert_eq!(split_utf16(body, 0), Some(0));
        // Heard to its end (trailing whitespace is nothing left), and past it.
        assert_eq!(split_utf16(body, 3), None);
        assert_eq!(split_utf16(body, 4), None);
        assert_eq!(split_utf16(body, u32::MAX), None);
        // A body still growing shows the mark once more of it follows.
        assert_eq!(split_utf16("Hi 😀.", 1), None);
        assert_eq!(split_utf16("Hi 😀. Zaż", 1), Some(6));
    }
}
