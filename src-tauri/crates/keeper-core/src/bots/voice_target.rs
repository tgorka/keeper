//! Where a spoken turn goes (Epic 67, Story 67.1, AD-206).
//!
//! The hands-free turn finishes in Rust (AD-205): once the turn has heard a
//! question, the shell sends it without a screen to ask. So the shell needs
//! an answer to "to whom?" that does not come from what is open on the
//! screen — there is no screen — and does not guess. This module is that
//! answer, decided from three facts the shell reads and hands over:
//!
//! 1. `bots.voice_target`, the bot the person picked in the Bots voice
//!    section. When it names a pinned bot, the turn goes there — into that
//!    bot's most recent conversation, or a new one when it has none.
//! 2. Unset (or naming a bot that is no longer pinned), the turn goes to the
//!    pinned bot most recently talked to: the newest conversation whose bot
//!    is still pinned, in the order the conversation list itself uses
//!    (`session::list_sessions`, newest activity first).
//! 3. Neither: the turn is refused with [`NO_TARGET_SENTENCE`], shown where
//!    the switch's refusal is shown and recorded in the ring. Nothing is
//!    sent to a bot nobody chose.
//!
//! `bots.voice_target` may instead name one of the person's own proxy
//! conversations as `agent:<room id>` (AD-384, ruling R31): the question
//! goes into that room as the person's own message. A room id is the same
//! on every device, so the value travels as it is (`settings_sync`). The
//! room must be one the account reads as its proxy's `main` or
//! `conversation` room *now* — a choice that is no longer one is refused
//! with [`AGENT_ROOM_GONE_SENTENCE`], never sent elsewhere, and never read
//! as "the most recent bot": the person chose to talk to their agent.
//!
//! The model a spoken turn sends is decided here too ([`model_for`], AD-217):
//! the one that last answered in the target conversation, else the
//! provider's own default, else the first offered model that can chat —
//! never one whose name says `embed` — else a refusal that says what to open.
//!
//! And how long a bot takes to start answering ([`median_first_token`],
//! AD-216): `bot_messages` already holds `ttft_ms` per answer, so the picker
//! can show each pinned bot's median first token over its last ten answers.
//! Nothing is measured that is not already stored.

use crate::agents::proxy::ProxyRoomVm;
use crate::bots::session::{BotMessage, BotSession};
use crate::bots::{Bot, ProviderKind};
use crate::vm::{BotModelVm, VoiceAgentTargetVm};

/// The sentence a spoken turn is refused with when there is no bot to send
/// it to. Shown beside the switch (AD-190), spoken by the lock-screen banner
/// (AD-207) and recorded in the ring (AD-192) — one wording, here.
pub const NO_TARGET_SENTENCE: &str = "Nothing to talk to yet: choose a bot to talk to under Bots.";

/// The sentence a spoken turn is refused with when `bots.voice_target`
/// names an agent room that is not one of the person's proxy conversations
/// on any signed-in account.
pub const AGENT_ROOM_GONE_SENTENCE: &str =
    "The conversation chosen under Speak to is not one of your assistant's here: choose again under Speak to.";

/// How `bots.voice_target` names an agent room: this prefix and the room id.
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

/// Where a spoken turn goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceTarget {
    /// A pinned bot, and which of its conversations.
    Bot {
        /// The pinned bot.
        bot_id: String,
        /// The bot's most recent conversation, or `None` when a new one is
        /// to be opened on it.
        session_id: Option<String>,
    },
    /// One of the person's proxy conversations (R31).
    Agent {
        /// The room's id.
        room_id: String,
    },
}

/// Why a spoken turn could not be sent — a sentence with its remedy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpokenRefusal {
    /// No target is chosen and no pinned bot has ever been talked to.
    NoTarget,
    /// The target bot offers no model this turn could send with.
    NoModel {
        /// The bot's display name.
        bot: String,
    },
    /// The chosen agent room is not one of the person's proxy
    /// conversations on any signed-in account.
    AgentRoomGone,
}

impl SpokenRefusal {
    /// The sentence the person is shown.
    pub fn message(&self) -> String {
        match self {
            Self::NoTarget => NO_TARGET_SENTENCE.to_owned(),
            Self::NoModel { bot } => format!(
                "{bot} offers no model to answer with: open a conversation with it under Bots and choose one."
            ),
            Self::AgentRoomGone => AGENT_ROOM_GONE_SENTENCE.to_owned(),
        }
    }
}

/// Decide where a spoken turn goes (AD-206, AD-384).
///
/// `chosen` is `bots.voice_target` as stored; `bots` every pinned bot;
/// `sessions` every live conversation, newest activity first, as
/// `session::list_sessions(dir, false)` lists them; `agent_rooms` the ids
/// of every room a signed-in account reads as its person's proxy
/// conversation (`proxy::proxy_rooms`). Never `chosen` when it names a bot
/// that is no longer pinned: an unpinned bot is not a bot to talk to, so
/// the rule falls through to the most recent — a choice that went stale is
/// treated as no choice, never as a send to a bot the list does not show.
/// An agent room is different: it is the person's own agent, and a choice
/// of it that went stale is refused rather than sent to a bot instead.
pub fn resolve(
    chosen: Option<&str>,
    bots: &[Bot],
    sessions: &[BotSession],
    agent_rooms: &[String],
) -> Result<VoiceTarget, SpokenRefusal> {
    if let Some(chosen) = chosen.filter(|chosen| chosen.starts_with(AGENT_TARGET_PREFIX)) {
        return agent_room_of(chosen)
            .and_then(|room| agent_rooms.iter().find(|listed| listed.as_str() == room))
            .map(|listed| VoiceTarget::Agent {
                room_id: listed.clone(),
            })
            .ok_or(SpokenRefusal::AgentRoomGone);
    }
    let pinned = |id: &str| bots.iter().any(|bot| bot.id == id);
    if let Some(bot_id) = chosen.filter(|id| pinned(id)) {
        let session_id = sessions
            .iter()
            .find(|session| session.bot_id == bot_id)
            .map(|session| session.id.clone());
        return Ok(VoiceTarget::Bot {
            bot_id: bot_id.to_owned(),
            session_id,
        });
    }
    sessions
        .iter()
        .find(|session| pinned(&session.bot_id))
        .map(|session| VoiceTarget::Bot {
            bot_id: session.bot_id.clone(),
            session_id: Some(session.id.clone()),
        })
        .ok_or(SpokenRefusal::NoTarget)
}

/// The alias a Hermes gateway always answers to (`/v1/models` lists it first
/// on a stock install, research §2.9): the provider's own default when the
/// conversation names no model. Ollama has no default of its own.
pub const HERMES_DEFAULT_MODEL: &str = "hermes-agent";

/// How many of a bot's latest answers the first-token median is taken over.
pub const FIRST_TOKEN_WINDOW: usize = 10;

/// Fewer answers than this and no median is shown: one or two numbers are
/// an anecdote, not a speed.
pub const FIRST_TOKEN_MIN_SAMPLES: usize = 3;

/// The model a spoken turn sends with (AD-217).
///
/// `history` is the target conversation's messages in order (empty for a new
/// conversation); `offered` is what the endpoint lists for the bot, in its
/// order. The rule, rung by rung:
///
/// 1. the last assistant row that names its model — the person chose it, or
///    the picker did, and a follow-up question goes to the model that was
///    answering;
/// 2. else the provider's own default: [`HERMES_DEFAULT_MODEL`] on Hermes,
///    which the gateway answers to whatever it lists; Ollama has none;
/// 3. else the first offered model that can chat — one whose name does not
///    say `embed`. The old rule took "first offered" and on a Hermes that
///    listed `embeddinggemma:latest` first, eight answers went out under a
///    name that cannot chat (epic 68's measurement 4);
/// 4. else the refusal, naming the bot and what to open.
pub fn model_for(
    bot: &Bot,
    kind: ProviderKind,
    history: &[BotMessage],
    offered: &[BotModelVm],
) -> Result<String, SpokenRefusal> {
    history
        .iter()
        .rev()
        .filter(|message| message.role == "assistant")
        .find_map(|message| message.model.clone())
        .or_else(|| provider_default(kind).map(str::to_owned))
        .or_else(|| {
            offered
                .iter()
                .find(|model| can_chat(&model.id))
                .map(|model| model.id.clone())
        })
        .ok_or_else(|| SpokenRefusal::NoModel {
            bot: bot.name.clone(),
        })
}

/// The model a provider answers with when none is named, where it has one.
fn provider_default(kind: ProviderKind) -> Option<&'static str> {
    match kind {
        ProviderKind::Hermes => Some(HERMES_DEFAULT_MODEL),
        ProviderKind::Ollama | ProviderKind::OpenAi => None,
    }
}

/// Whether a model's name says it can chat: an embedding model answers a
/// prompt with a vector, and every provider keeper knows names one with
/// `embed` somewhere in the tag (`embeddinggemma`, `nomic-embed-text`,
/// `mxbai-embed-large`, `text-embedding-3-small`).
fn can_chat(model_id: &str) -> bool {
    !model_id.to_ascii_lowercase().contains("embed")
}

/// A bot's first-token median over its latest answers (AD-216), in
/// milliseconds: `samples` is `ttft_ms` of the bot's assistant rows, newest
/// first, as `session::first_token_samples` reads them. Only the first
/// [`FIRST_TOKEN_WINDOW`] are counted, and fewer than
/// [`FIRST_TOKEN_MIN_SAMPLES`] answer `None` — the picker shows nothing
/// rather than a number one slow answer made. An even count takes the mean
/// of the two middle values.
pub fn median_first_token(samples: &[i64]) -> Option<u64> {
    let mut window: Vec<u64> = samples
        .iter()
        .take(FIRST_TOKEN_WINDOW)
        .map(|ms| ms.unsigned_abs())
        .collect();
    if window.len() < FIRST_TOKEN_MIN_SAMPLES {
        return None;
    }
    window.sort_unstable();
    let mid = window.len() / 2;
    Some(if window.len().is_multiple_of(2) {
        (window[mid - 1] + window[mid]) / 2
    } else {
        window[mid]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::BotIdentity;

    fn bot(id: &str) -> Bot {
        Bot {
            id: id.to_owned(),
            provider_id: "p1".to_owned(),
            target: id.to_owned(),
            name: format!("Bot {id}"),
            pin_order: 0,
            identity: BotIdentity::default(),
            created_ms: 0,
        }
    }

    fn session(id: &str, bot_id: &str, updated_ms: i64) -> BotSession {
        BotSession {
            id: id.to_owned(),
            bot_id: bot_id.to_owned(),
            provider_id: "p1".to_owned(),
            title: id.to_owned(),
            created_ms: updated_ms,
            updated_ms,
            archived: false,
            remote_session_id: None,
            remote_last_active_ms: None,
            remote_source: None,
        }
    }

    fn message(role: &str, model: Option<&str>) -> BotMessage {
        BotMessage {
            id: "m".to_owned(),
            session_id: "s".to_owned(),
            seq: 0,
            role: role.to_owned(),
            content: "x".to_owned(),
            model: model.map(str::to_owned),
            provider_id: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            ttft_ms: None,
            duration_ms: None,
            finish_reason: None,
            request_id: None,
            tool_call_count: 0,
            partial: false,
            created_ms: 0,
        }
    }

    fn model(id: &str) -> BotModelVm {
        BotModelVm {
            id: id.to_owned(),
            family: None,
            parameter_size: None,
            quantization: None,
            size_bytes: None,
            context_window: None,
            max_output_tokens: None,
            vision: None,
            tools: None,
            reasoning: None,
            embedding: None,
            capabilities: Vec::new(),
        }
    }

    #[test]
    fn a_chosen_bot_gets_its_newest_conversation() {
        let bots = [bot("a"), bot("b")];
        // Newest first, as the list reads: b was talked to last.
        let sessions = [
            session("s3", "b", 30),
            session("s2", "a", 20),
            session("s1", "a", 10),
        ];
        assert_eq!(
            resolve(Some("a"), &bots, &sessions, &[]),
            Ok(VoiceTarget::Bot {
                bot_id: "a".to_owned(),
                session_id: Some("s2".to_owned()),
            })
        );
    }

    #[test]
    fn a_chosen_bot_never_talked_to_opens_a_new_conversation() {
        let bots = [bot("a"), bot("b")];
        let sessions = [session("s1", "b", 10)];
        assert_eq!(
            resolve(Some("a"), &bots, &sessions, &[]),
            Ok(VoiceTarget::Bot {
                bot_id: "a".to_owned(),
                session_id: None,
            })
        );
    }

    #[test]
    fn unset_is_the_pinned_bot_most_recently_talked_to() {
        let bots = [bot("a"), bot("b")];
        let sessions = [session("s3", "b", 30), session("s2", "a", 20)];
        assert_eq!(
            resolve(None, &bots, &sessions, &[]),
            Ok(VoiceTarget::Bot {
                bot_id: "b".to_owned(),
                session_id: Some("s3".to_owned()),
            })
        );
    }

    #[test]
    fn an_unpinned_bot_is_skipped_whether_chosen_or_recent() {
        let bots = [bot("a")];
        // The newest conversation is with a bot that was unpinned since.
        let sessions = [session("s3", "gone", 30), session("s2", "a", 20)];
        let expected = Ok(VoiceTarget::Bot {
            bot_id: "a".to_owned(),
            session_id: Some("s2".to_owned()),
        });
        assert_eq!(resolve(None, &bots, &sessions, &[]), expected);
        // A stale choice is no choice, not a send to a bot the list hides.
        assert_eq!(resolve(Some("gone"), &bots, &sessions, &[]), expected);
    }

    #[test]
    fn nothing_to_talk_to_is_refused_with_the_sentence() {
        let refused = resolve(None, &[bot("a")], &[], &[]).expect_err("no conversation, no choice");
        assert_eq!(refused, SpokenRefusal::NoTarget);
        assert!(refused
            .message()
            .contains("choose a bot to talk to under Bots"));
        // No bots at all, and a stale choice, are the same refusal.
        assert_eq!(
            resolve(Some("a"), &[], &[session("s1", "a", 10)], &[]),
            Err(SpokenRefusal::NoTarget)
        );
    }

    /// AD-384: `agent:<room>` names one of the person's proxy
    /// conversations and goes there — never to a bot, and never to a room
    /// the account does not read as its proxy's, which is refused with the
    /// sentence naming *Speak to* even when bots could answer.
    #[test]
    fn a_voice_target_may_be_the_proxy_room() {
        let bots = [bot("a")];
        let sessions = [session("s1", "a", 10)];
        let mine = ["!dm:server".to_owned(), "!talk:server".to_owned()];
        assert_eq!(agent_target("!dm:server"), "agent:!dm:server");
        assert_eq!(
            resolve(Some("agent:!dm:server"), &bots, &sessions, &mine),
            Ok(VoiceTarget::Agent {
                room_id: "!dm:server".to_owned()
            })
        );
        assert_eq!(
            resolve(Some("agent:!talk:server"), &[], &[], &mine),
            Ok(VoiceTarget::Agent {
                room_id: "!talk:server".to_owned()
            })
        );
        for stale in ["agent:!elsewhere:server", "agent:", "agent:  "] {
            let refused =
                resolve(Some(stale), &bots, &sessions, &mine).expect_err("not my proxy's room");
            assert_eq!(refused, SpokenRefusal::AgentRoomGone, "{stale}");
            assert!(refused.message().contains("Speak to"));
        }
        // A bare id is a bot id, as before: an agent room is never guessed.
        assert_eq!(
            resolve(Some("a"), &bots, &sessions, &mine),
            Ok(VoiceTarget::Bot {
                bot_id: "a".to_owned(),
                session_id: Some("s1".to_owned())
            })
        );
        assert_eq!(resolve(None, &[], &[], &mine), Err(SpokenRefusal::NoTarget));
    }

    #[test]
    fn the_model_is_the_one_that_last_answered() {
        let b = bot("a");
        let history = [
            message("user", None),
            message("assistant", Some("llama4:8b")),
            message("user", None),
            message("assistant", None),
        ];
        let offered = [model("qwen3"), model("llama4:8b")];
        // Whatever the provider: a conversation's own model wins.
        assert_eq!(
            model_for(&b, ProviderKind::Ollama, &history, &offered),
            Ok("llama4:8b".to_owned())
        );
        assert_eq!(
            model_for(&b, ProviderKind::Hermes, &history, &[]),
            Ok("llama4:8b".to_owned())
        );
    }

    #[test]
    fn else_the_providers_own_default_where_it_has_one() {
        let b = bot("a");
        // Hermes answers to its alias whatever it lists — even an embedding
        // model first, the measured case — and even with nothing listed.
        let offered = [model("embeddinggemma:latest"), model("qwen3")];
        assert_eq!(
            model_for(&b, ProviderKind::Hermes, &[], &offered),
            Ok(HERMES_DEFAULT_MODEL.to_owned())
        );
        assert_eq!(
            model_for(&b, ProviderKind::Hermes, &[], &[]),
            Ok(HERMES_DEFAULT_MODEL.to_owned())
        );
        // Ollama and an OpenAI-compatible endpoint have none: the first
        // offered that can chat.
        for kind in [ProviderKind::Ollama, ProviderKind::OpenAi] {
            assert_eq!(provider_default(kind), None);
            assert_eq!(
                model_for(
                    &b,
                    kind,
                    &[],
                    &[
                        model("text-embedding-3-small"),
                        model("gpt-5"),
                        model("qwen3")
                    ]
                ),
                Ok("gpt-5".to_owned())
            );
        }
    }

    #[test]
    fn else_the_first_offered_that_is_not_an_embedding_model() {
        let b = bot("a");
        let offered = [
            model("nomic-embed-text"),
            model("mxbai-EMBED-large"),
            model("qwen3"),
            model("llama4:8b"),
        ];
        assert_eq!(
            model_for(&b, ProviderKind::Ollama, &[], &offered),
            Ok("qwen3".to_owned())
        );
    }

    #[test]
    fn else_the_refusal_names_the_bot() {
        let b = bot("a");
        // Nothing offered, and only embedding models offered, refuse alike.
        for offered in [Vec::new(), vec![model("embeddinggemma:latest")]] {
            let refused = model_for(&b, ProviderKind::Ollama, &[], &offered)
                .expect_err("nothing to send with");
            assert_eq!(
                refused,
                SpokenRefusal::NoModel {
                    bot: "Bot a".to_owned()
                }
            );
            assert!(refused.message().contains("Bot a"));
        }
    }

    #[test]
    fn the_median_is_over_the_last_ten_and_needs_three() {
        assert_eq!(median_first_token(&[]), None);
        assert_eq!(median_first_token(&[28_550]), None);
        assert_eq!(median_first_token(&[28_550, 1_000]), None);
        // Three: the middle one, whatever the order they arrived in.
        assert_eq!(median_first_token(&[28_550, 1_000, 2_000]), Some(2_000));
        // Even: the mean of the two middle values.
        assert_eq!(median_first_token(&[4, 1, 3, 2]), Some(2));
        // Newest first: the eleventh and later are not counted. Ten small
        // recent answers hide one huge old one entirely.
        let mut samples = vec![100; 10];
        samples.push(1_000_000);
        assert_eq!(median_first_token(&samples), Some(100));
        // And a newest slow answer among ten counts as one of ten.
        let mut samples = vec![90_000];
        samples.extend(std::iter::repeat_n(2_000, 9));
        samples.push(1);
        assert_eq!(median_first_token(&samples), Some(2_000));
    }
}
