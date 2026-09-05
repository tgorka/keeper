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
//! The model a spoken turn sends is decided here too ([`model_for`]): the
//! one that last answered in the target conversation, else the endpoint's
//! first — the rule the pane's picker applies when a bot is chosen without a
//! model — else a refusal that says what to open.

use crate::bots::session::{BotMessage, BotSession};
use crate::bots::Bot;
use crate::vm::BotModelVm;

/// The sentence a spoken turn is refused with when there is no bot to send
/// it to. Shown beside the switch (AD-190), spoken by the lock-screen banner
/// (AD-207) and recorded in the ring (AD-192) — one wording, here.
pub const NO_TARGET_SENTENCE: &str = "Nothing to talk to yet: choose a bot to talk to under Bots.";

/// Which bot, and which of its conversations, a spoken turn goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceTarget {
    /// The pinned bot.
    pub bot_id: String,
    /// The bot's most recent conversation, or `None` when a new one is to be
    /// opened on it.
    pub session_id: Option<String>,
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
}

impl SpokenRefusal {
    /// The sentence the person is shown.
    pub fn message(&self) -> String {
        match self {
            Self::NoTarget => NO_TARGET_SENTENCE.to_owned(),
            Self::NoModel { bot } => format!(
                "{bot} offers no model to answer with: open a conversation with it under Bots and choose one."
            ),
        }
    }
}

/// Decide where a spoken turn goes (AD-206).
///
/// `chosen` is `bots.voice_target` as stored; `bots` every pinned bot;
/// `sessions` every live conversation, newest activity first, as
/// `session::list_sessions(dir, false)` lists them. Never `chosen` when it
/// names a bot that is no longer pinned: an unpinned bot is not a bot to
/// talk to, so the rule falls through to the most recent — a choice that
/// went stale is treated as no choice, never as a send to a bot the list
/// does not show.
pub fn resolve(
    chosen: Option<&str>,
    bots: &[Bot],
    sessions: &[BotSession],
) -> Result<VoiceTarget, SpokenRefusal> {
    let pinned = |id: &str| bots.iter().any(|bot| bot.id == id);
    if let Some(bot_id) = chosen.filter(|id| pinned(id)) {
        let session_id = sessions
            .iter()
            .find(|session| session.bot_id == bot_id)
            .map(|session| session.id.clone());
        return Ok(VoiceTarget {
            bot_id: bot_id.to_owned(),
            session_id,
        });
    }
    sessions
        .iter()
        .find(|session| pinned(&session.bot_id))
        .map(|session| VoiceTarget {
            bot_id: session.bot_id.clone(),
            session_id: Some(session.id.clone()),
        })
        .ok_or(SpokenRefusal::NoTarget)
}

/// The model a spoken turn sends with.
///
/// `history` is the target conversation's messages in order (empty for a new
/// conversation); `offered` is what the endpoint lists for the bot, in its
/// order. The last assistant row that names its model wins — the person
/// chose it, or the picker did, and a follow-up question goes to the model
/// that was answering — else the first offered, the picker's own default.
pub fn model_for(
    bot: &Bot,
    history: &[BotMessage],
    offered: &[BotModelVm],
) -> Result<String, SpokenRefusal> {
    history
        .iter()
        .rev()
        .filter(|message| message.role == "assistant")
        .find_map(|message| message.model.clone())
        .or_else(|| offered.first().map(|model| model.id.clone()))
        .ok_or_else(|| SpokenRefusal::NoModel {
            bot: bot.name.clone(),
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
            resolve(Some("a"), &bots, &sessions),
            Ok(VoiceTarget {
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
            resolve(Some("a"), &bots, &sessions),
            Ok(VoiceTarget {
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
            resolve(None, &bots, &sessions),
            Ok(VoiceTarget {
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
        let expected = Ok(VoiceTarget {
            bot_id: "a".to_owned(),
            session_id: Some("s2".to_owned()),
        });
        assert_eq!(resolve(None, &bots, &sessions), expected);
        // A stale choice is no choice, not a send to a bot the list hides.
        assert_eq!(resolve(Some("gone"), &bots, &sessions), expected);
    }

    #[test]
    fn nothing_to_talk_to_is_refused_with_the_sentence() {
        let refused = resolve(None, &[bot("a")], &[]).expect_err("no conversation, no choice");
        assert_eq!(refused, SpokenRefusal::NoTarget);
        assert!(refused
            .message()
            .contains("choose a bot to talk to under Bots"));
        // No bots at all, and a stale choice, are the same refusal.
        assert_eq!(
            resolve(Some("a"), &[], &[session("s1", "a", 10)]),
            Err(SpokenRefusal::NoTarget)
        );
    }

    #[test]
    fn the_model_is_the_one_that_last_answered_else_the_first_offered() {
        let b = bot("a");
        let history = [
            message("user", None),
            message("assistant", Some("llama4:8b")),
            message("user", None),
            message("assistant", None),
        ];
        let offered = [model("qwen3"), model("llama4:8b")];
        assert_eq!(
            model_for(&b, &history, &offered),
            Ok("llama4:8b".to_owned())
        );
        assert_eq!(model_for(&b, &[], &offered), Ok("qwen3".to_owned()));
        let refused = model_for(&b, &[], &[]).expect_err("nothing to send with");
        assert_eq!(
            refused,
            SpokenRefusal::NoModel {
                bot: "Bot a".to_owned()
            }
        );
        assert!(refused.message().contains("Bot a"));
    }
}
