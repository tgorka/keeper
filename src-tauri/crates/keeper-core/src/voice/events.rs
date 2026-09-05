//! What the voice port did, kept in memory (Epic 65, Story 65.3, FR-448,
//! FR-449, AD-192).
//!
//! A phone has no console beside it. When the phrase is armed and nothing
//! answers, the only thing that can say why is the port itself — so the
//! shell feeds every arm, refusal, interruption, resume, roll and turn
//! transition into this ring, and Settings → Bots on the phone reads it back,
//! newest first. It is a view of memory: nothing is written to disk and
//! nothing leaves the device (AD-196).
//!
//! The ring is pure. Time comes in as an argument
//! ([`VoiceEvents::push`]'s `at_ms`, milliseconds since the Unix epoch as the
//! shell's clock gives them), so the same sequence of pushes reads the same
//! on the dev host as on the phone. It is bounded at [`CAPACITY`]: an armed
//! phone rolls its request every 45 s, and 200 entries is over two hours of
//! nothing but rolls — enough to read what went wrong, small enough that a
//! phone left listening for a week costs the same as one that listened for
//! an hour.

use std::collections::VecDeque;

use super::TurnState;
use crate::vm::VoiceEventVm;

/// How many events the ring keeps. The oldest is dropped for the newest.
pub const CAPACITY: usize = 200;

/// What kind of thing happened. A closed set with a stable string form
/// ([`VoiceEventKind::as_str`]) — the form the surface and a log reader see,
/// so it never changes without this enum changing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEventKind {
    /// The phrase was armed and the port opened for it.
    Armed,
    /// The switch went off and the device was released.
    Disarmed,
    /// The port refused — at arming, or a request that would not start.
    /// The detail is the sentence the person is shown.
    Refused,
    /// The system took the audio session (a call, Siri, a non-mixing app).
    InterruptionBegun,
    /// The system gave the audio session back.
    InterruptionEnded,
    /// The media server restarted under the port and every audio object it
    /// held is invalid (Epic 65, Story 65.4); the capture is rebuilt.
    MediaReset,
    /// The audio route changed — headphones, a car kit, a speaker — with
    /// the reason as the detail (Story 65.4). The capture continues, or is
    /// rebuilt when the change stopped the engine.
    RouteChanged,
    /// The capture was rebuilt after the system took it, or after the
    /// engine stopped on its own.
    Resumed,
    /// A fresh recognition request replaced the running one.
    Rolled,
    /// The turn moved to this state — the machine's own name for it.
    Turn(&'static str),
    /// The phrase was heard, or the control that stands in for it pressed.
    WakeMatched,
    /// An answer was handed to the synthesiser.
    Spoken,
    /// The Live Activity on the phone (Epic 65, Story 65.5, AD-194) did
    /// this — `started`, `updated`, `ended`, `refused` — with the state
    /// word or the system's refusal as the detail. The island is the only
    /// surface that cannot be read back from the app, so the ring is where
    /// its refusal on a free team is measured.
    Island(&'static str),
}

impl VoiceEventKind {
    /// The kind for a transition into `state`.
    pub fn turn(state: &TurnState) -> Self {
        Self::Turn(match state {
            TurnState::Idle => "idle",
            TurnState::Listening { .. } => "listening",
            TurnState::Heard { .. } => "heard",
            TurnState::Sending { .. } => "sending",
            TurnState::Speaking => "speaking",
            TurnState::Failed { .. } => "failed",
        })
    }

    /// The stable string form: `snake_case`, and `turn:<state>` for a
    /// transition.
    pub fn as_str(&self) -> String {
        match self {
            Self::Armed => "armed".to_owned(),
            Self::Disarmed => "disarmed".to_owned(),
            Self::Refused => "refused".to_owned(),
            Self::InterruptionBegun => "interruption_begun".to_owned(),
            Self::InterruptionEnded => "interruption_ended".to_owned(),
            Self::MediaReset => "media_reset".to_owned(),
            Self::RouteChanged => "route_changed".to_owned(),
            Self::Resumed => "resumed".to_owned(),
            Self::Rolled => "rolled".to_owned(),
            Self::Turn(state) => format!("turn:{state}"),
            Self::WakeMatched => "wake_matched".to_owned(),
            Self::Spoken => "spoken".to_owned(),
            Self::Island(what) => format!("island:{what}"),
        }
    }
}

/// One thing the port did, when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceEvent {
    /// Position in the ring's history, monotonic across the ring's life
    /// and never reused when the oldest is dropped: the one identity a
    /// surface can key a row on, since a kind at a millisecond is not unique.
    pub seq: u64,
    /// When, in milliseconds since the Unix epoch, as the caller's clock
    /// said.
    pub at_ms: i64,
    pub kind: VoiceEventKind,
    /// The words that go with it — a refusal's sentence, a transcript, a
    /// state's reason — or nothing.
    pub detail: Option<String>,
}

impl VoiceEvent {
    /// The surface's projection.
    pub fn vm(&self) -> VoiceEventVm {
        VoiceEventVm {
            seq: self.seq,
            at_ms: self.at_ms,
            kind: self.kind.as_str(),
            detail: self.detail.clone(),
        }
    }
}

/// The bounded ring. Oldest at the front, newest at the back; a push past
/// [`CAPACITY`] drops the oldest.
#[derive(Debug, Default)]
pub struct VoiceEvents {
    ring: VecDeque<VoiceEvent>,
    /// The next `seq` to hand out.
    next: u64,
}

impl VoiceEvents {
    /// An empty ring. `const`, so the shell can hold one in a `static`.
    pub const fn new() -> Self {
        Self {
            ring: VecDeque::new(),
            next: 0,
        }
    }

    /// Record one event. Timestamps are the caller's: the ring keeps the
    /// order it was given, not the order of the clocks.
    pub fn push(&mut self, at_ms: i64, kind: VoiceEventKind, detail: Option<String>) {
        if self.ring.len() == CAPACITY {
            self.ring.pop_front();
        }
        let seq = self.next;
        self.next = self.next.wrapping_add(1);
        self.ring.push_back(VoiceEvent {
            seq,
            at_ms,
            kind,
            detail,
        });
    }

    /// The most recent `limit` events, newest first, as the surface reads
    /// them.
    pub fn newest(&self, limit: usize) -> Vec<VoiceEventVm> {
        self.ring
            .iter()
            .rev()
            .take(limit)
            .map(VoiceEvent::vm)
            .collect()
    }

    /// How many events the ring holds right now.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// Whether nothing has happened yet.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(n: usize) -> VoiceEvents {
        let mut events = VoiceEvents::new();
        for i in 0..n {
            events.push(i as i64, VoiceEventKind::Rolled, None);
        }
        events
    }

    /// The ring never holds more than `CAPACITY`, and what it drops is the
    /// oldest — the newest push is always readable.
    #[test]
    fn the_ring_is_bounded_and_drops_the_oldest() {
        let mut events = filled(CAPACITY);
        assert_eq!(events.len(), CAPACITY);
        events.push(1_000, VoiceEventKind::WakeMatched, Some("nixie".to_owned()));
        assert_eq!(events.len(), CAPACITY);
        let all = events.newest(usize::MAX);
        assert_eq!(all.len(), CAPACITY);
        assert_eq!(all[0].at_ms, 1_000);
        assert_eq!(all[0].kind, "wake_matched");
        // The oldest (at 0) is gone; the one after it is now the last.
        assert_eq!(all[CAPACITY - 1].at_ms, 1);
    }

    /// Newest first, and `limit` is honoured from the newest end.
    #[test]
    fn newest_reads_backwards_and_honours_the_limit() {
        let mut events = VoiceEvents::new();
        events.push(10, VoiceEventKind::Armed, None);
        events.push(20, VoiceEventKind::Rolled, None);
        events.push(
            30,
            VoiceEventKind::Refused,
            Some("no microphone".to_owned()),
        );
        let two = events.newest(2);
        assert_eq!(
            two.iter().map(|e| e.at_ms).collect::<Vec<_>>(),
            vec![30, 20]
        );
        assert_eq!(two[0].detail.as_deref(), Some("no microphone"));
        assert_eq!(events.newest(0), Vec::new());
        assert_eq!(events.newest(10).len(), 3);
    }

    /// The ring keeps the order it was given, not the order of the clocks:
    /// a clock that went backwards does not reorder the evidence.
    #[test]
    fn order_is_the_order_of_pushes() {
        let mut events = VoiceEvents::new();
        events.push(500, VoiceEventKind::Armed, None);
        events.push(400, VoiceEventKind::Rolled, None);
        assert_eq!(events.newest(2)[0].kind, "rolled");
    }

    /// Every kind has one stable string, and a transition names the
    /// machine's state.
    #[test]
    fn kinds_have_a_stable_string_form() {
        assert_eq!(VoiceEventKind::Armed.as_str(), "armed");
        assert_eq!(VoiceEventKind::Disarmed.as_str(), "disarmed");
        assert_eq!(VoiceEventKind::Refused.as_str(), "refused");
        assert_eq!(
            VoiceEventKind::InterruptionBegun.as_str(),
            "interruption_begun"
        );
        assert_eq!(
            VoiceEventKind::InterruptionEnded.as_str(),
            "interruption_ended"
        );
        assert_eq!(VoiceEventKind::MediaReset.as_str(), "media_reset");
        assert_eq!(VoiceEventKind::RouteChanged.as_str(), "route_changed");
        assert_eq!(VoiceEventKind::Resumed.as_str(), "resumed");
        assert_eq!(VoiceEventKind::Rolled.as_str(), "rolled");
        assert_eq!(VoiceEventKind::WakeMatched.as_str(), "wake_matched");
        assert_eq!(VoiceEventKind::Spoken.as_str(), "spoken");
        assert_eq!(VoiceEventKind::Island("refused").as_str(), "island:refused");
        assert_eq!(
            VoiceEventKind::turn(&TurnState::Listening {
                heard: String::new()
            })
            .as_str(),
            "turn:listening"
        );
        assert_eq!(
            VoiceEventKind::turn(&TurnState::Failed {
                reason: "x".to_owned()
            })
            .as_str(),
            "turn:failed"
        );
        assert_eq!(VoiceEventKind::turn(&TurnState::Idle).as_str(), "turn:idle");
    }

    /// An empty ring reads as empty, not as an error.
    #[test]
    fn an_empty_ring_answers_nothing() {
        let events = VoiceEvents::new();
        assert!(events.is_empty());
        assert!(events.newest(5).is_empty());
    }
}
