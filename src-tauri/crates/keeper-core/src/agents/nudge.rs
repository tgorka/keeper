//! When an agent reviews what it learned (story 95.1, P12): Hermes' two
//! nudges, counted from a session's log. The memory nudge fires after
//! `[memory].nudge_user_turns` person turns (`user` lines), the skill nudge
//! after `[memory].nudge_tool_iterations` tool rounds (`assistant` lines that
//! called tools), which a `skill_propose` call resets. A review pass resets
//! the counters of the nudges it ran for; its own lines count for nothing.
//!
//! Pure: the host folds every line it writes or reads into [`Nudges`], so a
//! session's counters are the same on every host and after a restart.

use super::home::MemorySettings;
use super::log::{LineBody, LogLine, MemoryBody, MemoryOp, ReviewLines};

/// The `finish` of an `assistant` line whose round called tools: the turn
/// goes on after it.
pub const TOOL_ROUND_FINISH: &str = "tool_calls";

/// The tool whose call resets the skill nudge.
const SKILL_PROPOSE: &str = "skill_propose";

/// Which nudges fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    pub memory: bool,
    pub skills: bool,
}

impl Due {
    /// The `ref` of the `memory` line that begins the review.
    pub fn as_ref_word(self) -> &'static str {
        match (self.memory, self.skills) {
            (true, true) => "memory,skill",
            (true, false) => "memory",
            _ => "skill",
        }
    }
}

/// A session's nudge counters.
#[derive(Debug, Clone, Default)]
pub struct Nudges {
    /// `user` lines since the last memory review.
    user_turns: u32,
    /// Tool rounds since the last skill review or `skill_propose`.
    tool_rounds: u32,
    review: ReviewLines,
}

impl Nudges {
    /// Take one line, in log order; whether it is a review pass's.
    pub fn observe(&mut self, line: &LogLine) -> bool {
        if self.review.take(line) {
            if let LineBody::Memory(MemoryBody {
                op: MemoryOp::Review,
                reference,
            }) = &line.body
            {
                for nudge in reference.split(',') {
                    match nudge {
                        "memory" => self.user_turns = 0,
                        "skill" => self.tool_rounds = 0,
                        _ => {}
                    }
                }
            }
            return true;
        }
        match &line.body {
            LineBody::User(_) => self.user_turns += 1,
            LineBody::Assistant(body) if body.finish == TOOL_ROUND_FINISH => {
                self.tool_rounds += 1;
            }
            LineBody::ToolCall(call) if call.tool == SKILL_PROPOSE => self.tool_rounds = 0,
            _ => {}
        }
        false
    }

    /// The nudges `settings` let fire now, if any (`0` is off).
    pub fn due(&self, settings: &MemorySettings) -> Option<Due> {
        let fires = |count: u32, every: u32| every > 0 && count >= every;
        let due = Due {
            memory: fires(self.user_turns, settings.nudge_user_turns),
            skills: fires(self.tool_rounds, settings.nudge_tool_iterations),
        };
        (due.memory || due.skills).then_some(due)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use ulid::Ulid;

    use super::*;
    use crate::agents::log::{
        AssistantBody, HostSlug, LogLine, ToolCallBody, Usage, UserBody, LINE_VERSION,
    };

    fn line(parent: Option<Ulid>, body: LineBody) -> LogLine {
        LogLine {
            v: LINE_VERSION,
            id: Ulid::new(),
            parent,
            ts: Utc
                .with_ymd_and_hms(2026, 10, 6, 9, 0, 0)
                .single()
                .expect("ts"),
            host: HostSlug::new("electra").expect("slug"),
            epoch: 1,
            claim: None,
            matrix_event: None,
            body,
        }
    }

    fn user() -> LineBody {
        LineBody::User(UserBody {
            sender: "@tgorka:example.org".try_into().expect("user"),
            text: "hi".to_owned(),
            attachments: Vec::new(),
        })
    }

    fn round(finish: &str) -> LineBody {
        LineBody::Assistant(AssistantBody {
            text: String::new(),
            model: "m".to_owned(),
            finish: finish.to_owned(),
            usage: Usage::default(),
            ttft_ms: None,
            duration_ms: 0,
            anchor_event: None,
        })
    }

    fn call(tool: &str) -> LineBody {
        LineBody::ToolCall(ToolCallBody {
            call_id: "c".to_owned(),
            tool: tool.to_owned(),
            args: "{}".to_owned(),
            tier: 1,
            grant_id: None,
        })
    }

    fn review(nudges: &str) -> LineBody {
        LineBody::Memory(MemoryBody {
            op: MemoryOp::Review,
            reference: nudges.to_owned(),
        })
    }

    fn settings(user_turns: u32, tool_iterations: u32) -> MemorySettings {
        MemorySettings {
            nudge_user_turns: user_turns,
            nudge_tool_iterations: tool_iterations,
            promote: true,
        }
    }

    /// 95.1 acceptance 10, the counts: ten person turns fire the memory
    /// nudge, fifteen tool rounds the skill nudge; a `skill_propose` call
    /// resets the second; a review resets what it ran for and its own
    /// rounds count for nothing; `0` turns a nudge off.
    #[test]
    fn nudge_counters() {
        let on = settings(10, 15);
        let mut nudges = Nudges::default();
        for _ in 0..9 {
            nudges.observe(&line(None, user()));
        }
        assert_eq!(nudges.due(&on), None);
        nudges.observe(&line(None, user()));
        assert_eq!(
            nudges.due(&on),
            Some(Due {
                memory: true,
                skills: false
            })
        );
        assert_eq!(nudges.due(&settings(0, 15)), None, "0 is off");

        // A review for memory resets the turns; its own lines are not the
        // conversation's.
        let marker = line(None, review("memory"));
        nudges.observe(&marker);
        let inner = line(Some(marker.id), round(TOOL_ROUND_FINISH));
        nudges.observe(&inner);
        nudges.observe(&line(Some(inner.id), call(SKILL_PROPOSE)));
        assert_eq!(nudges.due(&on), None);
        assert_eq!((nudges.user_turns, nudges.tool_rounds), (0, 0));

        // Tool rounds: a final answer is not one, a skill_propose resets.
        for _ in 0..14 {
            nudges.observe(&line(None, round(TOOL_ROUND_FINISH)));
        }
        nudges.observe(&line(None, round("stop")));
        assert_eq!(nudges.due(&on), None);
        nudges.observe(&line(None, call(SKILL_PROPOSE)));
        for _ in 0..14 {
            nudges.observe(&line(None, round(TOOL_ROUND_FINISH)));
        }
        assert_eq!(nudges.due(&on), None);
        nudges.observe(&line(None, round(TOOL_ROUND_FINISH)));
        assert_eq!(
            nudges.due(&on),
            Some(Due {
                memory: false,
                skills: true
            })
        );
        assert_eq!(nudges.due(&settings(10, 0)), None);

        // Both, then a combined review resets both.
        for _ in 0..10 {
            nudges.observe(&line(None, user()));
        }
        let both = nudges.due(&on).expect("both due");
        assert_eq!(both.as_ref_word(), "memory,skill");
        nudges.observe(&line(None, review(both.as_ref_word())));
        assert_eq!(nudges.due(&on), None);
    }
}
