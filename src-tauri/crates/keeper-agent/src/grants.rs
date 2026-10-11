//! Where a turn's grants come from (C3): the fifth port.
//!
//! A bot in the app is granted folders by a person in Settings › Bots, and
//! those rows are keyed by (provider, bot) in `keeper.db`. An agent is
//! granted its drives by its own `agent.toml` (`[tools].drives` and
//! `[tools].allow`), so two agents on one model never share a grant. Both
//! answer the same two questions — what is granted, for arming a turn, and
//! may this one call proceed, asked again at every call (FR-386) — and both
//! answer the second through `keeper_core::bots::grant`, so the precedence
//! rules are written once.

use std::path::PathBuf;

use keeper_core::bots::grant::{
    self, Effect, Grant, GrantMode, GrantScope, GrantVerdict, ToolTarget,
};
use keeper_core::bots::store;
use keeper_core::bots::tools::ToolName;

use crate::turn::AgentError;

/// The grants one turn runs under.
pub trait GrantSource: Send + Sync {
    /// The live grants, for what goes on offer and which context files the
    /// model is told about. An `Err` arms the turn with none.
    fn grants(&self) -> Result<Vec<Grant>, AgentError>;

    /// The verdict for one call, read afresh every call: a grant revoked
    /// mid-turn stops the next call in that turn.
    fn verdict(&self, target: &ToolTarget, effect: Effect) -> Result<GrantVerdict, AgentError>;
}

/// The app's grants: the `keeper.db` rows of one (provider, bot), with
/// `grant::check`'s whole answer, `DENY_REVOKED` included (D10).
pub struct StoreGrants {
    data_dir: PathBuf,
    provider_id: String,
    bot_id: Option<String>,
}

impl StoreGrants {
    pub fn new(data_dir: PathBuf, provider_id: String, bot_id: Option<String>) -> StoreGrants {
        StoreGrants {
            data_dir,
            provider_id,
            bot_id,
        }
    }
}

impl GrantSource for StoreGrants {
    fn grants(&self) -> Result<Vec<Grant>, AgentError> {
        Ok(
            store::list_grants_for_bot(&self.data_dir, &self.provider_id, self.bot_id.as_deref())?
                .live,
        )
    }

    fn verdict(&self, target: &ToolTarget, effect: Effect) -> Result<GrantVerdict, AgentError> {
        Ok(grant::check(
            &self.data_dir,
            &self.provider_id,
            self.bot_id.as_deref(),
            target,
            effect,
        )?)
    }
}

/// An agent's grants: one profile-wide grant per drive that is both in the
/// agent's `[tools].drives` and in the session's scope, `write` when the
/// agent may `drive_write` or `drive_edit`, else `read`.
///
/// Profile-wide on purpose: AD-158 makes a write under a profile-wide grant
/// an ask, and before Epic 93 every ask an agent makes is refused with
/// [`crate::host::UNATTENDED_REFUSAL`] (C4). A drive outside the scope has no
/// grant at all, so a call naming it is `DENY_NO_GRANT`.
pub struct AgentGrants {
    grants: Vec<Grant>,
}

impl AgentGrants {
    /// `agent_drives` is `[tools].drives`, `scope` the session's drives in
    /// scope and `allow` `[tools].allow`. The tool host's profiles are named
    /// by drive id, so a grant's profile is the drive id.
    pub fn new(
        provider_id: &str,
        bot_id: &str,
        agent_drives: &[String],
        scope: &[String],
        allow: &[String],
    ) -> AgentGrants {
        let writes = [ToolName::Write, ToolName::Edit]
            .iter()
            .any(|tool| allow.iter().any(|name| name == tool.as_wire()));
        let mode = if writes {
            GrantMode::Write
        } else {
            GrantMode::Read
        };
        let grants = agent_drives
            .iter()
            .filter(|drive| scope.contains(drive))
            .map(|drive| Grant {
                id: format!("agent:{drive}"),
                provider_id: provider_id.to_owned(),
                bot_id: Some(bot_id.to_owned()),
                scope: GrantScope::Profile {
                    profile_id: drive.clone(),
                },
                mode,
                created_ms: 0,
            })
            .collect();
        AgentGrants { grants }
    }
}

impl GrantSource for AgentGrants {
    fn grants(&self) -> Result<Vec<Grant>, AgentError> {
        Ok(self.grants.clone())
    }

    fn verdict(&self, target: &ToolTarget, effect: Effect) -> Result<GrantVerdict, AgentError> {
        Ok(grant::decide(&self.grants, target, effect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(allow: &[&str]) -> AgentGrants {
        let owned = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        AgentGrants::new(
            "provider",
            "bot",
            &owned(&["tgdrive", "neuradrive"]),
            &owned(&["tgdrive", "elsewhere"]),
            &owned(allow),
        )
    }

    fn target(drive: &str, path: &str) -> ToolTarget {
        ToolTarget::parse(drive, path).expect("a target")
    }

    /// Only a drive both the agent names and the session has in scope is
    /// reachable; a write there is an ask, never a silent allow.
    #[test]
    fn an_agent_reaches_only_its_drives_in_scope() {
        let grants = agent(&["drive_read", "drive_write"]);
        assert!(matches!(
            grants.verdict(&target("tgdrive", "notes/a.md"), Effect::Read),
            Ok(GrantVerdict::Allow { .. })
        ));
        for drive in ["neuradrive", "elsewhere"] {
            assert_eq!(
                grants
                    .verdict(&target(drive, "notes/a.md"), Effect::Read)
                    .expect("a verdict"),
                GrantVerdict::Deny {
                    reason: grant::DENY_NO_GRANT.to_owned()
                },
                "{drive}"
            );
        }
        assert!(matches!(
            grants.verdict(&target("tgdrive", "notes/a.md"), Effect::Write),
            Ok(GrantVerdict::Ask { .. })
        ));
        let reader = agent(&["drive_read"]);
        assert_eq!(
            reader
                .verdict(&target("tgdrive", "notes/a.md"), Effect::Write)
                .expect("a verdict"),
            GrantVerdict::Deny {
                reason: grant::DENY_READ_ONLY.to_owned()
            }
        );
    }
}
