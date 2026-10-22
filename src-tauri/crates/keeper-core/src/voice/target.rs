//! Where a spoken turn goes (Epic 67, AD-206; AD-384; Story 108.3a).
//!
//! The hands-free turn finishes in Rust (AD-205): once the turn has heard a
//! question, the shell sends it without a screen to ask. So the shell needs an
//! answer to "to whom?" that does not come from what is open on the screen and
//! does not guess. This module is that answer, decided from `voice.target` and
//! the rooms the shell reads:
//!
//! - `voice.target` names one of the person's own proxy conversations as
//!   `agent:<room id>` (AD-384, ruling R31): the question goes into that room
//!   as the person's own message. A room id is the same on every device, so the
//!   value travels as it is (`settings_sync`). The room must be one an account
//!   reads as its proxy's `main` or `conversation` room *now* — a choice that is
//!   no longer one is refused with [`AGENT_ROOM_GONE_SENTENCE`], never sent
//!   elsewhere.
//! - Unset, there is nothing to talk to and the turn is refused with
//!   [`NO_TARGET_SENTENCE`]. Nothing is sent to an agent nobody chose.
//!
//! Voice is not a bot feature (AD-179): until Story 108.3a a spoken turn could
//! also go to a pinned ⌘9 bot, and that target left with the bots.

use crate::agents::proxy::ProxyRoomVm;
use crate::vm::VoiceAgentTargetVm;

/// The sentence a spoken turn is refused with when nothing is chosen under
/// *Speak to*. Shown beside the switch (AD-190), spoken by the lock-screen
/// banner (AD-207) and recorded in the ring (AD-192) — one wording, here.
pub const NO_TARGET_SENTENCE: &str =
    "Nothing to talk to yet: choose who answers under Speak to.";

/// The sentence a spoken turn is refused with when `voice.target` names an
/// agent room that is not one of the person's proxy conversations on any
/// signed-in account.
pub const AGENT_ROOM_GONE_SENTENCE: &str =
    "The conversation chosen under Speak to is not one of your assistant's here: choose again under Speak to.";

/// How `voice.target` names an agent room: this prefix and the room id.
pub const AGENT_TARGET_PREFIX: &str = "agent:";

/// The stored value that names agent room `room_id`.
pub fn agent_target(room_id: &str) -> String {
    format!("{AGENT_TARGET_PREFIX}{room_id}")
}

/// The room a stored value names, when it names an agent room.
pub fn agent_room_of(chosen: &str) -> Option<&str> {
    chosen
        .strip_prefix(AGENT_TARGET_PREFIX)
        .map(str::trim)
        .filter(|room| !room.is_empty())
}

/// The picker's agent entries from every live account's proxy conversations
/// in their order (`AccountManager::agent_rooms_everywhere`), each with the
/// value that chooses it; a room listed twice is offered once.
pub fn agent_targets(rooms: Vec<(String, ProxyRoomVm)>) -> Vec<VoiceAgentTargetVm> {
    let mut targets: Vec<VoiceAgentTargetVm> = Vec::with_capacity(rooms.len());
    for (account_id, room) in rooms {
        if targets.iter().any(|target| target.room_id == room.room_id) {
            continue;
        }
        targets.push(VoiceAgentTargetVm {
            target: agent_target(&room.room_id),
            account_id,
            room_id: room.room_id,
            name: room.name,
            kind: room.kind,
        });
    }
    targets
}

/// Why a spoken turn could not be sent — a sentence with its remedy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpokenRefusal {
    /// Nothing is chosen under *Speak to*.
    NoTarget,
    /// The chosen agent room is not one of the person's proxy conversations
    /// on any signed-in account.
    AgentRoomGone,
}

impl SpokenRefusal {
    /// The sentence the person is shown.
    pub fn message(&self) -> &'static str {
        match self {
            Self::NoTarget => NO_TARGET_SENTENCE,
            Self::AgentRoomGone => AGENT_ROOM_GONE_SENTENCE,
        }
    }
}

/// Decide where a spoken turn goes (AD-206, AD-384): the room id of the
/// chosen agent room.
///
/// `chosen` is `voice.target` as stored; `agent_rooms` the ids of every room a
/// signed-in account reads as its person's proxy conversation
/// (`proxy::proxy_rooms`). A value that names no agent room — blank, or a
/// pinned bot's id left over from before Story 108.3a — is no choice.
pub fn resolve(chosen: Option<&str>, agent_rooms: &[String]) -> Result<String, SpokenRefusal> {
    let Some(chosen) = chosen.filter(|chosen| chosen.starts_with(AGENT_TARGET_PREFIX)) else {
        return Err(SpokenRefusal::NoTarget);
    };
    agent_room_of(chosen)
        .and_then(|room| agent_rooms.iter().find(|listed| listed.as_str() == room))
        .cloned()
        .ok_or(SpokenRefusal::AgentRoomGone)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AD-384: `agent:<room>` names one of the person's proxy conversations;
    /// a choice that is no longer one is refused rather than sent elsewhere.
    #[test]
    fn a_voice_target_is_the_proxy_room_it_names() {
        let mine = vec!["!dm:server".to_owned(), "!talk:server".to_owned()];
        assert_eq!(agent_target("!dm:server"), "agent:!dm:server");
        assert_eq!(
            resolve(Some("agent:!talk:server"), &mine),
            Ok("!talk:server".to_owned())
        );
        for stale in ["agent:!elsewhere:server", "agent:", "agent:  "] {
            assert_eq!(
                resolve(Some(stale), &mine),
                Err(SpokenRefusal::AgentRoomGone),
                "{stale}"
            );
        }
    }

    /// Unset, and a value naming no agent room — a pinned bot's id from before
    /// the bots left — is no choice: refused with the sentence, never guessed.
    #[test]
    fn nothing_chosen_is_refused_with_the_sentence() {
        let mine = vec!["!dm:server".to_owned()];
        for chosen in [None, Some("01ARZ3NDEKTSV4RRFFQ69G5FAV")] {
            let refusal = resolve(chosen, &mine).expect_err("refused");
            assert_eq!(refusal, SpokenRefusal::NoTarget);
            assert_eq!(refusal.message(), NO_TARGET_SENTENCE);
        }
    }
}
