//! Which of a person's devices an agent's surface call goes to (AD-383).
//!
//! Each of the person's keeper clients publishes `dev.keeper.agent.presence`
//! in their principal's control room: when whether it is in front changes
//! (after [`PRESENCE_STILLNESS`]), and every [`RENEW_EVERY`] while it does
//! not, each one live for [`PRESENCE_TTL_MS`]. The content is metadata only —
//! state events are not encrypted — so nothing a person reads travels in it
//! ([`PresenceContent`]). A host sends a surface call to the live, focused
//! device renewed last ([`surface_target`]).

use std::time::{Duration, Instant};

use chrono::DateTime;
use matrix_sdk::ruma::{OwnedUserId, UserId};
use serde_json::Value;

use crate::agents::claim::rfc3339;
use crate::agents::events::{PresenceContent, PresencePlatform, CONTENT_VERSION};

/// How long whether the app is in front must hold before it is published.
pub const PRESENCE_STILLNESS: Duration = Duration::from_secs(1);
/// How often an unchanged presence is published again.
pub const RENEW_EVERY: Duration = Duration::from_secs(60);
/// How long a published presence counts: three renewals.
pub const PRESENCE_TTL_MS: u64 = 180_000;
/// The view a presence names before the app has said which one it shows.
pub const UNKNOWN_VIEW: &str = "app";

/// What one device publishes about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePresence {
    pub platform: PresencePlatform,
    pub focused: bool,
    pub view: String,
}

/// Whether `view` names a primary view (`notes`, `chats`): a short
/// lower-case id, never a path, a title or text.
pub fn is_view_id(view: &str) -> bool {
    (1..=32).contains(&view.len())
        && view
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// When a device's presence is published: once a change has held for
/// [`PRESENCE_STILLNESS`] and differs from the last one published, and
/// every [`RENEW_EVERY`] after the last one.
#[derive(Debug, Default)]
pub struct PresenceClock {
    pending: Option<(DevicePresence, Instant)>,
    sent: Option<(DevicePresence, Instant)>,
}

impl PresenceClock {
    /// The device's state changed at `now`.
    pub fn change(&mut self, state: DevicePresence, now: Instant) {
        self.pending = Some((state, now));
    }

    /// The presence to publish at `now`, if one is due; it is then the last
    /// one published.
    pub fn due(&mut self, now: Instant) -> Option<DevicePresence> {
        if let Some((_, at)) = &self.pending {
            if now.saturating_duration_since(*at) < PRESENCE_STILLNESS {
                return None;
            }
            if let Some((state, _)) = self.pending.take() {
                if self.sent.as_ref().map(|(sent, _)| sent) != Some(&state) {
                    self.sent = Some((state.clone(), now));
                    return Some(state);
                }
            }
        }
        let (state, at) = self.sent.as_mut()?;
        if now.saturating_duration_since(*at) < RENEW_EVERY {
            return None;
        }
        *at = now;
        Some(state.clone())
    }

    /// When the next presence could be due.
    pub fn next_due(&self) -> Option<Instant> {
        match (&self.pending, &self.sent) {
            (Some((_, at)), _) => Some(*at + PRESENCE_STILLNESS),
            (None, Some((_, at))) => Some(*at + RENEW_EVERY),
            (None, None) => None,
        }
    }
}

/// The presence `user`'s `device` publishes at `now_ms`: renewed now, live
/// for [`PRESENCE_TTL_MS`].
pub fn presence_content(
    user: &UserId,
    device: &str,
    state: &DevicePresence,
    now_ms: u64,
) -> PresenceContent {
    PresenceContent {
        v: CONTENT_VERSION,
        user: user.to_owned(),
        device: device.to_owned(),
        platform: state.platform,
        focused: state.focused,
        view: if is_view_id(&state.view) {
            state.view.clone()
        } else {
            UNKNOWN_VIEW.to_owned()
        },
        renewed_at: rfc3339(now_ms),
        expires_at: rfc3339(now_ms.saturating_add(PRESENCE_TTL_MS)),
    }
}

/// One presence state event as a control room holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Published {
    pub state_key: String,
    pub sender: OwnedUserId,
    pub content: Value,
}

/// The device of `person` a surface call goes to at `now_ms`: of the live,
/// focused presences, the one renewed last; `None` when there is none.
///
/// A presence counts only when its sender is the user it names and its
/// state key is the device it names: anyone may write a state event a
/// person could, so one naming somebody else's device is ignored.
pub fn surface_target(presences: &[Published], person: &UserId, now_ms: u64) -> Option<String> {
    let ms = |at: &str| {
        DateTime::parse_from_rfc3339(at)
            .ok()
            .and_then(|at| u64::try_from(at.timestamp_millis()).ok())
    };
    presences
        .iter()
        .filter(|published| published.sender == person)
        .filter_map(|published| {
            let content: PresenceContent =
                serde_json::from_value(published.content.clone()).ok()?;
            let live = content.v == CONTENT_VERSION
                && content.user == person
                && content.device == published.state_key
                && content.focused
                && ms(&content.expires_at)? > now_ms;
            live.then_some((ms(&content.renewed_at)?, content.device))
        })
        .max_by_key(|(renewed, _)| *renewed)
        .map(|(_, device)| device)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::*;
    use crate::agents::events::Focus;

    const T0: u64 = 1_790_000_000_000;

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn state(focused: bool) -> DevicePresence {
        DevicePresence {
            platform: PresencePlatform::Macos,
            focused,
            view: "notes".to_owned(),
        }
    }

    #[test]
    fn presence_is_metadata_only() {
        // The person is reading a note in a private drive; what is
        // published says only that the Mac shows the notes view.
        let focus = Focus {
            drive: "marta-diary".to_owned(),
            path: "secret/salary-talk.md".to_owned(),
            heading: Some("What I will ask for".to_owned()),
        };
        let tgorka = user("@tgorka:example.org");
        let content = presence_content(&tgorka, "HESPERIA", &state(true), T0);
        let value = serde_json::to_value(&content).expect("serialise");
        let keys: BTreeSet<String> = value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            keys,
            [
                "device",
                "expires_at",
                "focused",
                "platform",
                "renewed_at",
                "user",
                "v",
                "view"
            ]
            .map(str::to_owned)
            .into()
        );
        let text = value.to_string();
        for secret in [
            focus.drive.as_str(),
            focus.path.as_str(),
            "salary",
            "What I will ask for",
        ] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        // A view that is not a view id is never published as one.
        for leak in [
            focus.path.as_str(),
            "notes/plan.md",
            "drafts/q3",
            "Plans",
            "",
        ] {
            assert!(!is_view_id(leak), "{leak}");
            let named = DevicePresence {
                view: leak.to_owned(),
                ..state(true)
            };
            assert_eq!(
                presence_content(&tgorka, "HESPERIA", &named, T0).view,
                UNKNOWN_VIEW
            );
        }
        // Reading one back refuses a key the schema does not have.
        let mut forged = value;
        forged["path"] = json!(focus.path);
        assert!(serde_json::from_value::<PresenceContent>(forged).is_err());
        assert_eq!(content.expires_at, rfc3339(T0 + PRESENCE_TTL_MS));
    }

    fn published(sender: &str, device: &str, focused: bool, renewed: u64) -> Published {
        let mut content = serde_json::to_value(presence_content(
            &user(sender),
            device,
            &state(focused),
            renewed,
        ))
        .expect("serialise");
        content["user"] = json!(sender);
        Published {
            state_key: device.to_owned(),
            sender: user(sender),
            content,
        }
    }

    #[test]
    fn the_target_is_the_newest_live_focused_device() {
        let tgorka = user("@tgorka:example.org");
        let now = T0 + 60_000;
        let mac = published("@tgorka:example.org", "HESPERIA", true, now - 10_000);
        let iphone = published("@tgorka:example.org", "KALYPSO", true, now - 2_000);
        let unfocused = published("@tgorka:example.org", "HESPERIA2", false, now);
        assert_eq!(
            surface_target(
                &[mac.clone(), iphone.clone(), unfocused.clone()],
                &tgorka,
                now
            )
            .as_deref(),
            Some("KALYPSO")
        );
        // An expired presence is never chosen, however new its renewal.
        let expired = published("@tgorka:example.org", "OLD", true, now - PRESENCE_TTL_MS);
        assert_eq!(
            surface_target(&[expired.clone(), mac.clone()], &tgorka, now).as_deref(),
            Some("HESPERIA")
        );
        assert_eq!(surface_target(&[expired], &tgorka, now), None);
        // A forged presence: sent by someone other than the user it names,
        // or naming a device other than its state key.
        let mut forged = published("@tgorka:example.org", "FORGED", true, now);
        forged.sender = user("@marta:example.org");
        let mut moved = published("@tgorka:example.org", "KALYPSO", true, now);
        moved.state_key = "ELSEWHERE".to_owned();
        let marta = published("@marta:example.org", "MARTA", true, now);
        assert_eq!(
            surface_target(&[mac.clone(), forged, moved, marta], &tgorka, now).as_deref(),
            Some("HESPERIA")
        );
        // None live: unavailable.
        assert_eq!(surface_target(&[unfocused], &tgorka, now), None);
        assert_eq!(surface_target(&[], &tgorka, now), None);
    }

    #[test]
    fn presence_goes_out_after_a_second_of_stillness_and_every_minute() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut clock = PresenceClock::default();
        assert_eq!(clock.due(at(0)), None, "nothing to say yet");
        // Three changes inside 600 ms: one publication, the last state.
        clock.change(state(true), at(0));
        clock.change(state(false), at(300));
        clock.change(state(true), at(600));
        assert_eq!(clock.next_due(), Some(at(1_600)));
        assert_eq!(clock.due(at(1_500)), None);
        assert_eq!(clock.due(at(1_600)), Some(state(true)));
        // The same state again is no new publication.
        clock.change(state(true), at(2_000));
        assert_eq!(clock.due(at(3_100)), None);
        // Unchanged, it is renewed a minute after it was last published.
        assert_eq!(clock.next_due(), Some(at(61_600)));
        assert_eq!(clock.due(at(61_500)), None);
        assert_eq!(clock.due(at(61_600)), Some(state(true)));
        assert_eq!(clock.due(at(62_000)), None);
        // A change publishes after its second, and moves the renewal.
        clock.change(state(false), at(70_000));
        assert_eq!(clock.due(at(71_000)), Some(state(false)));
        assert_eq!(clock.next_due(), Some(at(131_000)));
    }
}
