//! Where a spoken turn goes (Epic 67, AD-206; AD-384; 108.2; Story 108.3a).
//!
//! The hands-free turn finishes in Rust (AD-205): once the turn has heard a
//! question, the shell sends it without a screen to ask. So the shell needs an
//! answer to "to whom?" that does not come from what is open on the screen and
//! does not guess. This module is that answer, decided from `voice.target` and
//! the agents the shell reads:
//!
//! - `voice.target` names one of the person's agents as `agent:<agent>`
//!   ([`crate::hq::spoken::agent_target`]): the question goes into that
//!   agent's main conversation in the agents' databases
//!   ([`crate::hq::spoken`]). An agent's id is the same on every device, so
//!   the value travels as it is (`settings_sync`). The agent must be one a
//!   database this device holds names *now* — a choice that is no longer one
//!   is refused with [`crate::hq::spoken::AGENT_GONE_SENTENCE`], never sent
//!   elsewhere.
//! - Unset, or a value that names no agent — one of the agents' old Matrix
//!   rooms (`agent:!room…`, before 108.2; `crate::agents_cleanup` clears it
//!   with a notice), or a pinned bot's id from before the bots left — is no
//!   choice: the turn is refused with [`NO_TARGET_SENTENCE`]. Nothing is sent
//!   to an agent nobody chose.
//!
//! Voice is not a bot feature (AD-179): until Story 108.3a a spoken turn could
//! also go to a pinned ⌘9 bot, and that target left with the bots.

use crate::hq::spoken::{target_agent, AGENT_GONE_SENTENCE, AGENT_TARGET_PREFIX};

/// The sentence a spoken turn is refused with when nothing is chosen under
/// *Speak to*. Shown beside the switch (AD-190), spoken by the lock-screen
/// banner (AD-207) and recorded in the ring (AD-192) — one wording, here.
pub const NO_TARGET_SENTENCE: &str = "Nothing to talk to yet: choose an agent under Speak to.";

/// Why a spoken turn could not be sent — a sentence with its remedy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpokenRefusal {
    /// Nothing is chosen under *Speak to*.
    NoTarget,
    /// The chosen agent is in none of the databases this device holds.
    AgentGone,
}

impl SpokenRefusal {
    /// The sentence the person is shown.
    pub fn message(&self) -> &'static str {
        match self {
            Self::NoTarget => NO_TARGET_SENTENCE,
            Self::AgentGone => AGENT_GONE_SENTENCE,
        }
    }
}

/// Decide where a spoken turn goes (AD-206, AD-384): the actor id of the
/// chosen agent.
///
/// `chosen` is `voice.target` as stored; `agents` the actor id of every agent
/// a database this device holds names (`Hq::spoken_agents`).
pub fn resolve(chosen: Option<&str>, agents: &[String]) -> Result<String, SpokenRefusal> {
    let Some(chosen) = chosen.filter(|chosen| names_an_agent(chosen)) else {
        return Err(SpokenRefusal::NoTarget);
    };
    target_agent(chosen)
        .and_then(|agent| agents.iter().find(|listed| listed.as_str() == agent))
        .cloned()
        .ok_or(SpokenRefusal::AgentGone)
}

/// Whether a stored value is a choice of one of the person's agents — an
/// `agent:` value that is not one of the agents' old Matrix rooms — so the
/// agents need reading at all.
pub fn names_an_agent(chosen: &str) -> bool {
    target_agent(chosen).is_some()
        || chosen
            .trim()
            .strip_prefix(AGENT_TARGET_PREFIX)
            .is_some_and(|rest| rest.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AD-384, 108.2: `agent:<agent>` names one of the person's agents; a
    /// choice no held database names any more is refused rather than sent
    /// elsewhere.
    #[test]
    fn a_voice_target_is_the_agent_it_names() {
        let mine = vec!["nixie".to_owned(), "tola".to_owned()];
        assert_eq!(resolve(Some("agent:tola"), &mine), Ok("tola".to_owned()));
        for stale in ["agent:someone-else", "agent:", "agent:  "] {
            assert_eq!(
                resolve(Some(stale), &mine),
                Err(SpokenRefusal::AgentGone),
                "{stale}"
            );
        }
        assert_eq!(SpokenRefusal::AgentGone.message(), AGENT_GONE_SENTENCE);
    }

    /// Unset, an old Matrix room, and a pinned bot's id from before the bots
    /// left are no choice: refused with the sentence, never guessed.
    #[test]
    fn nothing_chosen_is_refused_with_the_sentence() {
        let mine = vec!["nixie".to_owned()];
        for chosen in [
            None,
            Some("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            Some("agent:!dm:example.org"),
        ] {
            let refusal = resolve(chosen, &mine).expect_err("refused");
            assert_eq!(refusal, SpokenRefusal::NoTarget, "{chosen:?}");
            assert_eq!(refusal.message(), NO_TARGET_SENTENCE);
        }
    }
}
