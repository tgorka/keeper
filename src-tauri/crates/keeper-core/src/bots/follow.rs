//! Following the conversation from the other device (Epic 63, Story 63.7,
//! FR-425, FR-426, AD-177, AD-178).
//!
//! # What "following" is, and what it is not
//!
//! Two devices write one Hermes session (AD-176). While the Mac is holding a
//! turn, the phone wants to watch it grow. Hermes cannot serve that as a
//! stream: `GET /v1/runs/{id}/events` is one `asyncio.Queue` per run with a
//! destructive read, so a second subscriber steals events from the first and
//! either side disconnecting pops the queue out from under the survivor.
//! **keeper never opens an SSE against a run it did not start** (AD-177).
//!
//! What Hermes does persist is step-granular: the user turn is flushed at
//! turn start, the assistant's tool-call block before the tools execute, and
//! the final text before the loop exits. So the other device follows by
//! re-reading `GET /api/sessions/{id}/messages` — the route Story 63.6 already
//! reads on open — and sees the user message at once, each tool round as it
//! starts, and each answer when it completes. Never a token. The surface says
//! so in one sentence rather than pretending to stream.
//!
//! No new destination (AD-178): the only route this module causes to be
//! called is the history read keeper already makes.
//!
//! # The two pure decisions here
//!
//! [`decide`] says whether to read again, and when. It is driven by the
//! session's own signals — whether the newest step is one a turn is still
//! working on, when the session last moved, whether *this* device holds the
//! turn — and it backs off as the session quietens and stops when it goes
//! cold. Polling forever is a battery defect on a phone, so the default
//! outcome of nothing happening is [`Follow::Stop`].
//!
//! [`merge`] folds the gateway's transcript into this device's own rows under
//! one precedence rule: **a turn this device sent is shown as this device
//! wrote it, and every other turn as the gateway holds it.** That keeps this
//! device's live `partial` row and the gateway's copy of the same turn from
//! doubling up, and keeps a row the local stream already closed — with its
//! measured usage, its finish reason, its Stop — from being overwritten by
//! the gateway's rendering of it.

use crate::bots::remote::{self, RemoteMessage};
use crate::bots::session::BotMessage;

/// How often to read while a turn is open on the other device — a step lands
/// every few seconds at most, and a read sooner than this would find nothing.
pub const LIVE_INTERVAL_MS: i64 = 2_000;

/// How often to read once the turn has closed but the session moved recently:
/// the person on the other device may be typing a follow-up.
pub const SETTLED_INTERVAL_MS: i64 = 5_000;

/// How often to read once the session has been quiet a while.
pub const QUIET_INTERVAL_MS: i64 = 15_000;

/// How long after the last movement a settled session counts as quiet.
pub const QUIET_AFTER_MS: i64 = 60_000;

/// How long after the last movement a session counts as cold and following
/// stops. Five minutes covers a slow tool round on the other device; a turn
/// that has shown nothing for longer is one the other device has abandoned or
/// one Hermes died inside, and neither is worth a phone's radio every few
/// seconds. Reopening the conversation reads it fresh.
pub const COLD_AFTER_MS: i64 = 5 * 60_000;

/// What the session itself says about whether to keep reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FollowSignals {
    /// This device's clock, ms since the Unix epoch.
    pub now_ms: i64,
    /// Whether this device is streaming a turn of this session right now.
    /// It already has the stream; reading the transcript underneath it would
    /// only race its own rows.
    pub owns_turn: bool,
    /// Whether the newest step is one a turn is still working on
    /// ([`turn_open`]).
    pub turn_open: bool,
    /// The newest remote row's timestamp, where the gateway stated one.
    pub newest_ms: Option<i64>,
    /// The gateway's `last_active` for the session, as of keeper's last list
    /// read, where it stated one. Touched at most every ~30 s on the far side,
    /// so a coarse liveness hint and not a clock.
    pub last_active_ms: Option<i64>,
    /// When keeper's own row last changed — the local send that started a
    /// turn here, or the open that adopted the session. The signal that is
    /// always present.
    pub updated_ms: i64,
}

/// What to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    /// Read the transcript again after this many ms.
    Poll { after_ms: i64 },
    /// Stop reading: this device holds the turn, or the session has gone cold.
    Stop,
}

impl Follow {
    /// The interval, or `None` for a stop — the shape the view model carries.
    pub fn after_ms(self) -> Option<i64> {
        match self {
            Follow::Poll { after_ms } => Some(after_ms),
            Follow::Stop => None,
        }
    }
}

/// Whether the transcript's newest step belongs to a turn still in progress.
///
/// Hermes flushes in this order: the user message, then an assistant row
/// carrying the tool calls (empty of text) before the tools run, then the
/// tool results, then the assistant's final text. So the turn is open while
/// the last row is the person's question, a tool result, or an assistant row
/// that is either carrying tool calls or has no text yet. It is closed on an
/// assistant row with text and no tool calls, and on an empty transcript —
/// there is nothing in progress in a session nobody has spoken to.
pub fn turn_open(remote: &[RemoteMessage]) -> bool {
    match remote.last() {
        None => false,
        Some(last) => match last.role.as_str() {
            "assistant" => last.tool_call_count > 0 || last.content.trim().is_empty(),
            "user" | "tool" => true,
            _ => false,
        },
    }
}

/// The newest timestamp the gateway stated on any row.
pub fn newest_ms(remote: &[RemoteMessage]) -> Option<i64> {
    remote.iter().filter_map(|row| row.timestamp_ms).max()
}

/// Whether to read the transcript again, and when.
///
/// The rules, in order:
///
/// 1. A device holding the turn does not read — it has the stream.
/// 2. Idle is measured from the freshest thing known about the session: the
///    newest remote row, the gateway's `last_active`, or keeper's own row.
///    A gateway clock ahead of this device's counts as idle zero rather than
///    as a negative number.
/// 3. Past [`COLD_AFTER_MS`] of idle, stop — whatever the newest row looks
///    like. A turn that has shown no step in five minutes is not one worth
///    a phone's radio.
/// 4. With a turn open, read at [`LIVE_INTERVAL_MS`] while the session moved
///    inside [`QUIET_AFTER_MS`], and at [`SETTLED_INTERVAL_MS`] after.
/// 5. With the turn closed, read at [`SETTLED_INTERVAL_MS`] inside
///    [`QUIET_AFTER_MS`], and at [`QUIET_INTERVAL_MS`] after.
pub fn decide(signals: &FollowSignals) -> Follow {
    if signals.owns_turn {
        return Follow::Stop;
    }
    let freshest = [
        signals.newest_ms,
        signals.last_active_ms,
        Some(signals.updated_ms),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(signals.updated_ms);
    let idle = signals.now_ms.saturating_sub(freshest).max(0);
    if idle >= COLD_AFTER_MS {
        return Follow::Stop;
    }
    let recent = idle < QUIET_AFTER_MS;
    let after_ms = match (signals.turn_open, recent) {
        (true, true) => LIVE_INTERVAL_MS,
        (true, false) | (false, true) => SETTLED_INTERVAL_MS,
        (false, false) => QUIET_INTERVAL_MS,
    };
    Follow::Poll { after_ms }
}

/// The gateway's transcript folded into this device's own rows.
///
/// **A turn this device sent is shown as this device wrote it, and every
/// other turn as the gateway holds it.** A turn is a user row and everything
/// up to the next user row. This device's turns are `local` — keeper writes
/// only its own turns into `keeper.db` for a remote session — and each is
/// matched to the gateway's turn with the same question, nearest in time
/// where the same words were sent twice. The match replaces the gateway's
/// rendering (an assistant row of tool calls, the tool results, the final
/// text) with keeper's one answer row: partial while it streams, closed with
/// its measured usage after, stopped if the person pressed Stop. A local
/// turn the gateway has not flushed yet — one that just started, or one that
/// never reached it — is kept where its own clock puts it.
///
/// Turns are ordered by when they started, on the gateway's clock for every
/// turn it holds and on this device's only for the turns it does not — so a
/// skew between the two clocks can only ever move a turn the gateway has not
/// seen, which is the in-flight one at the end.
pub fn merge(
    local: &[BotMessage],
    remote: &[RemoteMessage],
    local_session_id: &str,
    provider_id: &str,
    fallback_ms: i64,
) -> Vec<BotMessage> {
    let remote_turns = turns(remote, |row| &row.role);
    let local_turns = turns(local, |row| &row.role);
    let composed = remote::compose_messages(remote, local_session_id, provider_id, fallback_ms);

    // Which remote turn each local turn stands in for, if any.
    let mut claimed = vec![false; remote_turns.len()];
    let mut stand_in: Vec<Option<usize>> = Vec::with_capacity(local_turns.len());
    for turn in &local_turns {
        let head = &local[turn.start];
        let matched = if head.role == "user" {
            remote_turns
                .iter()
                .enumerate()
                .filter(|(index, candidate)| {
                    !claimed[*index]
                        && remote[candidate.start].role == "user"
                        && remote[candidate.start].content.trim() == head.content.trim()
                })
                .min_by_key(|(_, candidate)| {
                    remote[candidate.start]
                        .timestamp_ms
                        .map_or(i64::MAX, |ms| (ms - head.created_ms).abs())
                })
                .map(|(index, _)| index)
        } else {
            None
        };
        if let Some(index) = matched {
            claimed[index] = true;
        }
        stand_in.push(matched);
    }

    // Every turn with the clock it sorts by.
    let mut groups: Vec<(i64, Vec<BotMessage>)> = Vec::with_capacity(remote_turns.len() + 1);
    for (index, turn) in remote_turns.iter().enumerate() {
        let started = composed[turn.start].created_ms;
        let rows = match stand_in.iter().position(|claim| *claim == Some(index)) {
            Some(local_index) => local[local_turns[local_index].range()].to_vec(),
            None => composed[turn.range()].to_vec(),
        };
        groups.push((started, rows));
    }
    for (local_index, turn) in local_turns.iter().enumerate() {
        if stand_in[local_index].is_none() {
            groups.push((local[turn.start].created_ms, local[turn.range()].to_vec()));
        }
    }
    groups.sort_by_key(|(started, _)| *started);

    let mut out: Vec<BotMessage> = groups.into_iter().flat_map(|(_, rows)| rows).collect();
    for (seq, row) in out.iter_mut().enumerate() {
        row.seq = i64::try_from(seq).unwrap_or(i64::MAX);
    }
    out
}

/// One turn's bounds in a transcript: `start..end`.
#[derive(Debug, Clone, Copy)]
struct TurnSpan {
    start: usize,
    end: usize,
}

impl TurnSpan {
    fn range(self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

/// Split a transcript into turns: each user row starts one, and whatever
/// precedes the first user row is a turn of its own.
fn turns<T>(rows: &[T], role: impl Fn(&T) -> &str) -> Vec<TurnSpan> {
    let mut spans = Vec::new();
    let mut start = 0usize;
    for (index, row) in rows.iter().enumerate() {
        if index > 0 && role(row) == "user" {
            spans.push(TurnSpan { start, end: index });
            start = index;
        }
    }
    if start < rows.len() {
        spans.push(TurnSpan {
            start,
            end: rows.len(),
        });
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_756_000_000_000;

    fn remote(id: &str, role: &str, content: &str, tools: i64, at: i64) -> RemoteMessage {
        RemoteMessage {
            id: id.to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            timestamp_ms: Some(at),
            token_count: None,
            finish_reason: (role == "assistant" && tools == 0).then(|| "stop".to_owned()),
            tool_call_count: tools,
        }
    }

    fn local(id: &str, role: &str, content: &str, partial: bool, at: i64) -> BotMessage {
        BotMessage {
            id: id.to_owned(),
            session_id: "local-1".to_owned(),
            seq: 0,
            role: role.to_owned(),
            content: content.to_owned(),
            model: Some("hermes-agent".to_owned()),
            provider_id: Some("prov".to_owned()),
            prompt_tokens: Some(10),
            completion_tokens: Some(20),
            total_tokens: Some(30),
            ttft_ms: Some(120),
            duration_ms: Some(900),
            finish_reason: (!partial).then(|| "stop".to_owned()),
            request_id: None,
            tool_call_count: 0,
            partial,
            created_ms: at,
        }
    }

    fn signals(turn_open: bool, idle_ms: i64) -> FollowSignals {
        FollowSignals {
            now_ms: NOW,
            owns_turn: false,
            turn_open,
            newest_ms: Some(NOW - idle_ms),
            last_active_ms: None,
            updated_ms: NOW - COLD_AFTER_MS * 10,
        }
    }

    // -- turn_open ---------------------------------------------------------

    #[test]
    fn the_turn_is_open_on_every_step_but_the_final_answer() {
        assert!(!turn_open(&[]));
        assert!(turn_open(&[remote("1", "user", "hi", 0, NOW)]));
        assert!(turn_open(&[remote("2", "assistant", "", 1, NOW)]));
        assert!(turn_open(&[remote("3", "tool", "{}", 0, NOW)]));
        assert!(turn_open(&[remote("4", "assistant", "", 0, NOW)]));
        assert!(!turn_open(&[remote("5", "assistant", "done", 0, NOW)]));
        assert!(!turn_open(&[remote("6", "system", "prompt", 0, NOW)]));
    }

    // -- decide ------------------------------------------------------------

    #[test]
    fn a_device_holding_the_turn_does_not_poll() {
        let mut live = signals(true, 0);
        live.owns_turn = true;
        assert_eq!(decide(&live), Follow::Stop);
    }

    #[test]
    fn an_open_turn_is_read_every_two_seconds() {
        assert_eq!(
            decide(&signals(true, 0)),
            Follow::Poll {
                after_ms: LIVE_INTERVAL_MS
            }
        );
    }

    #[test]
    fn a_closed_turn_backs_off_as_the_session_quietens() {
        assert_eq!(
            decide(&signals(false, 1_000)),
            Follow::Poll {
                after_ms: SETTLED_INTERVAL_MS
            }
        );
        assert_eq!(
            decide(&signals(false, QUIET_AFTER_MS)),
            Follow::Poll {
                after_ms: QUIET_INTERVAL_MS
            }
        );
        // An open turn that has shown nothing for a minute slows too.
        assert_eq!(
            decide(&signals(true, QUIET_AFTER_MS)),
            Follow::Poll {
                after_ms: SETTLED_INTERVAL_MS
            }
        );
    }

    #[test]
    fn polling_stops_when_the_session_goes_cold() {
        assert_eq!(decide(&signals(false, COLD_AFTER_MS)), Follow::Stop);
        // Even with a turn that looks open: five silent minutes is a turn the
        // other device abandoned, not one worth a phone's radio.
        assert_eq!(decide(&signals(true, COLD_AFTER_MS)), Follow::Stop);
        assert_eq!(
            decide(&signals(false, COLD_AFTER_MS - 1)).after_ms(),
            Some(QUIET_INTERVAL_MS)
        );
    }

    #[test]
    fn idle_is_measured_from_the_freshest_signal() {
        // The gateway's last_active is newer than any row it served.
        let mut s = signals(false, COLD_AFTER_MS);
        s.last_active_ms = Some(NOW - 1_000);
        assert_eq!(decide(&s).after_ms(), Some(SETTLED_INTERVAL_MS));
        // No remote signal at all: keeper's own row is the clock.
        let s = FollowSignals {
            newest_ms: None,
            last_active_ms: None,
            updated_ms: NOW - 2_000,
            ..signals(false, 0)
        };
        assert_eq!(decide(&s).after_ms(), Some(SETTLED_INTERVAL_MS));
        // A gateway clock ahead of ours is idle zero, not a negative number.
        assert_eq!(
            decide(&signals(true, -60_000)).after_ms(),
            Some(LIVE_INTERVAL_MS)
        );
    }

    // -- merge -------------------------------------------------------------

    fn other_devices_turn(at: i64) -> [RemoteMessage; 4] {
        [
            remote("10", "user", "what changed", 0, at),
            remote("11", "assistant", "", 1, at + 1_000),
            remote("12", "tool", "{\"ok\":true}", 0, at + 2_000),
            remote("13", "assistant", "Three notes.", 0, at + 3_000),
        ]
    }

    #[test]
    fn a_turn_the_other_device_sent_is_shown_as_the_gateway_holds_it() {
        let out = merge(&[], &other_devices_turn(NOW), "local-1", "prov", NOW);
        let ids: Vec<&str> = out.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["10", "11", "12", "13"]);
        assert_eq!(out.iter().map(|m| m.seq).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert!(out.iter().all(|m| m.session_id == "local-1"));
    }

    /// The precedence rule, one direction: the local stream closed the row
    /// with its measured usage; the gateway's copy of the same turn must not
    /// overwrite it — and must not sit beside it either.
    #[test]
    fn a_turn_this_device_closed_wins_over_the_gateway_copy_of_it() {
        let local = [
            local("L1", "user", "what changed", false, NOW),
            local("L2", "assistant", "Three notes.", false, NOW),
        ];
        let out = merge(
            &local,
            &other_devices_turn(NOW - 500),
            "local-1",
            "prov",
            NOW,
        );
        let ids: Vec<&str> = out.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            ["L1", "L2"],
            "the local turn stands in for all four gateway rows"
        );
        assert_eq!(out[1].total_tokens, Some(30), "the measured usage survived");
        assert_eq!(out[1].duration_ms, Some(900));
    }

    /// The other direction: this device's partial row is still open, and the
    /// gateway has already flushed the turn's first steps. One turn on
    /// screen, not two.
    #[test]
    fn a_turn_this_device_is_still_streaming_does_not_double_up() {
        let local = [
            local("L1", "user", "what changed", false, NOW),
            local("L2", "assistant", "Three", true, NOW),
        ];
        let flushed = [
            remote("10", "user", "what changed", 0, NOW + 200),
            remote("11", "assistant", "", 1, NOW + 1_200),
        ];
        let out = merge(&local, &flushed, "local-1", "prov", NOW);
        let ids: Vec<&str> = out.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["L1", "L2"]);
        assert!(out[1].partial);
    }

    /// A stopped answer is a record of what this device saw; the gateway may
    /// hold the finished text, and it still does not replace the record.
    #[test]
    fn a_stopped_answer_is_kept_as_stopped() {
        let local = [
            local("L1", "user", "what changed", false, NOW),
            local("L2", "assistant", "Thr", true, NOW),
        ];
        let out = merge(&local, &other_devices_turn(NOW), "local-1", "prov", NOW);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].content, "Thr");
        assert!(out[1].partial);
    }

    #[test]
    fn turns_from_both_devices_interleave_by_when_they_started() {
        let local = [
            local("L1", "user", "and the notes?", false, NOW + 10_000),
            local("L2", "assistant", "Two.", false, NOW + 10_000),
        ];
        let mut remote_rows = other_devices_turn(NOW).to_vec();
        remote_rows.push(remote("20", "user", "and the notes?", 0, NOW + 10_100));
        remote_rows.push(remote("21", "assistant", "Two.", 0, NOW + 11_000));
        remote_rows.push(remote("30", "user", "thanks", 0, NOW + 20_000));
        remote_rows.push(remote("31", "assistant", "Any time.", 0, NOW + 21_000));
        let out = merge(&local, &remote_rows, "local-1", "prov", NOW);
        let ids: Vec<&str> = out.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["10", "11", "12", "13", "L1", "L2", "30", "31"]);
        assert_eq!(
            out.iter().map(|m| m.seq).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5, 6, 7]
        );
    }

    /// A turn that started here and has not reached the gateway yet — or
    /// never will — is kept, at the end where its clock puts it.
    #[test]
    fn a_local_turn_the_gateway_has_not_flushed_is_kept() {
        let local = [
            local("L1", "user", "one more thing", false, NOW + 5_000),
            local("L2", "assistant", "", true, NOW + 5_000),
        ];
        let out = merge(&local, &other_devices_turn(NOW), "local-1", "prov", NOW);
        let ids: Vec<&str> = out.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["10", "11", "12", "13", "L1", "L2"]);
    }

    /// The same words sent from both devices: each local turn claims the
    /// gateway turn nearest to it in time, so the other device's identical
    /// question is still shown as the gateway holds it.
    #[test]
    fn the_same_question_twice_matches_by_nearest_time() {
        let local = [
            local("L1", "user", "ok", false, NOW + 10_000),
            local("L2", "assistant", "Sure.", false, NOW + 10_000),
        ];
        let remote_rows = [
            remote("10", "user", "ok", 0, NOW),
            remote("11", "assistant", "Yes.", 0, NOW + 1_000),
            remote("20", "user", "ok", 0, NOW + 10_050),
            remote("21", "assistant", "Sure.", 0, NOW + 11_000),
        ];
        let out = merge(&local, &remote_rows, "local-1", "prov", NOW);
        let ids: Vec<&str> = out.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["10", "11", "L1", "L2"]);
    }

    #[test]
    fn rows_before_the_first_question_are_a_turn_of_their_own() {
        let remote_rows = [
            remote("1", "system", "You are keeper's bot.", 0, NOW - 1),
            remote("10", "user", "hi", 0, NOW),
            remote("11", "assistant", "Hello.", 0, NOW + 1),
        ];
        let out = merge(&[], &remote_rows, "local-1", "prov", NOW);
        let ids: Vec<&str> = out.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["1", "10", "11"]);
        let spans = turns(&remote_rows, |row| &row.role);
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].start, spans[0].end), (0, 1));
        assert_eq!((spans[1].start, spans[1].end), (1, 3));
    }
}
