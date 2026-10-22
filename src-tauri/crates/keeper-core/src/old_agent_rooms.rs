//! The agents' old Matrix rooms, after the agents left Matrix (story 108.2,
//! FR-863, UX-DR-E108-2).
//!
//! Before 108.2 an agent's conversation was a Matrix room keeper drew as an
//! agent's: a session room (`m.room.create` type `dev.keeper.agent.session`)
//! or a principal's control room (`dev.keeper.agent.control`). The agents now
//! live in their database ([`crate::hq`]), and keeper no longer draws those
//! rooms — but the person's account is still in them. Left alone they would
//! fall into the Inbox as strange rooms, so every list leaves them out by
//! their create type ([`is_old_agent_room`], the room lists' filter
//! [`list_filter`]), and one action leaves them all
//! (`AccountManager::leave_old_agent_rooms`). Nothing here reads or sends an
//! agent event: the type of a room is all that is looked at.

use matrix_sdk::ruma::room::RoomType;
use matrix_sdk_ui::room_list_service::filters::BoxedFilterFn;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `m.room.create` `type` of an agent's session room before 108.2.
pub const SESSION_ROOM_TYPE: &str = "dev.keeper.agent.session";

/// `m.room.create` `type` of a principal's control room before 108.2.
pub const CONTROL_ROOM_TYPE: &str = "dev.keeper.agent.control";

/// Whether a room created with `room_type` is one of the agents' old rooms.
pub fn is_old_agent_room(room_type: Option<&RoomType>) -> bool {
    room_type.is_some_and(|kind| {
        let kind = kind.to_string();
        kind == SESSION_ROOM_TYPE || kind == CONTROL_ROOM_TYPE
    })
}

/// Whether a room created with `room_type` is an old agent *session* room —
/// the one kind of the two a person once wrote in.
pub fn is_old_session_room(room_type: Option<&RoomType>) -> bool {
    room_type.is_some_and(|kind| kind.to_string() == SESSION_ROOM_TYPE)
}

/// The room lists' filter: every room that is not one of the agents' old
/// rooms. Applied at the room list's source, so the Inbox, the Archive, the
/// Pins, the Favorites, the Spaces and every per-account list never hold one.
pub fn list_filter() -> BoxedFilterFn {
    Box::new(|item| !is_old_agent_room(item.room_type().as_ref()))
}

/// The agents' old rooms of the signed-in accounts, as *Leave old agent
/// rooms* shows them, and what the last leave did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OldAgentRoomsVm {
    /// How many old agent rooms the live accounts are still in (joined or
    /// invited).
    pub remaining: u32,
    /// How many the last leave left; `0` before any.
    pub left: u32,
    /// What the last leave could not leave, as a sentence; `None` when it
    /// left every one or none was asked.
    pub failure: Option<String>,
}

/// The sentence for `failed` rooms the server would not let the person
/// leave (R381: said, never swallowed).
pub fn failure_sentence(failed: u32) -> Option<String> {
    match failed {
        0 => None,
        1 => Some("keeper could not leave one old agent room. Try again later.".to_owned()),
        n => Some(format!(
            "keeper could not leave {n} old agent rooms. Try again later."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UX-DR-E108-2: a session room and a control room are the agents' old
    /// rooms; a chat, a Space, any other custom type and a room with no type
    /// are not — a messenger room is never hidden.
    #[test]
    fn only_the_agents_create_types_are_old_agent_rooms() {
        let session = RoomType::from(SESSION_ROOM_TYPE);
        let control = RoomType::from(CONTROL_ROOM_TYPE);
        assert!(is_old_agent_room(Some(&session)));
        assert!(is_old_agent_room(Some(&control)));
        assert!(is_old_session_room(Some(&session)));
        assert!(!is_old_session_room(Some(&control)));
        for other in [
            RoomType::Space,
            RoomType::from("dev.keeper.agent"),
            RoomType::from("dev.keeper.agent.session.v2"),
            RoomType::from("org.example.board"),
        ] {
            assert!(!is_old_agent_room(Some(&other)), "{other}");
            assert!(!is_old_session_room(Some(&other)), "{other}");
        }
        assert!(!is_old_agent_room(None));
        assert!(!is_old_session_room(None));
    }

    /// What the action says after a leave: nothing when every room was left,
    /// one sentence naming how many were not.
    #[test]
    fn a_leave_that_failed_says_how_many_rooms_stayed() {
        assert_eq!(failure_sentence(0), None);
        assert!(failure_sentence(1).is_some_and(|s| s.contains("one old agent room")));
        assert!(failure_sentence(3).is_some_and(|s| s.contains("3 old agent rooms")));
    }
}
