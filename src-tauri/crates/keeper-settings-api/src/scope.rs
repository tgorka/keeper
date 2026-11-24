//! Resolution of the `settings_owner` sync scope.

use serde::{Deserialize, Serialize};

use crate::refusal::Refusal;
use crate::write::{Domain, DomainKind};

/// Kind of caller requesting a settings scope or blob read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerKind {
    /// A human account holder.
    Person,
    /// An agent instance.
    Agent,
    /// A tool client.
    Tool,
    /// The internal service client.
    Service,
}

/// Resolve the allowed `settings_owner` scope values for a caller and domain.
pub fn settings_owner_scope(
    caller: CallerKind,
    actor: &str,
    domain: &Domain,
) -> Result<Vec<String>, Refusal> {
    match caller {
        CallerKind::Agent | CallerKind::Tool => Err(Refusal::ScopeRefused),
        CallerKind::Service => Ok(Vec::new()),
        CallerKind::Person => {
            if domain.kind == DomainKind::Personal && domain.owner_actor.as_deref() == Some(actor) {
                Ok(vec![actor.to_owned()])
            } else {
                Ok(Vec::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn personal_domain() -> Domain {
        Domain {
            kind: DomainKind::Personal,
            owner_actor: Some("@ana:example.org".to_owned()),
        }
    }

    #[test]
    fn service_gets_empty_scope() {
        assert_eq!(
            settings_owner_scope(CallerKind::Service, "@ana:example.org", &personal_domain()),
            Ok(Vec::new())
        );
    }

    #[test]
    fn agent_and_tool_are_refused() {
        let domain = personal_domain();
        assert_eq!(
            settings_owner_scope(CallerKind::Agent, "@agent-nixie:example.org", &domain),
            Err(Refusal::ScopeRefused)
        );
        assert_eq!(
            settings_owner_scope(CallerKind::Tool, "@ana:example.org", &domain),
            Err(Refusal::ScopeRefused)
        );
    }

    #[test]
    fn person_gets_own_personal_domain_only() {
        let personal = personal_domain();
        assert_eq!(
            settings_owner_scope(CallerKind::Person, "@ana:example.org", &personal),
            Ok(vec!["@ana:example.org".to_owned()])
        );
        assert_eq!(
            settings_owner_scope(CallerKind::Person, "@bo:example.org", &personal),
            Ok(Vec::new())
        );

        let general = Domain {
            kind: DomainKind::General,
            owner_actor: None,
        };
        assert_eq!(
            settings_owner_scope(CallerKind::Person, "@ana:example.org", &general),
            Ok(Vec::new())
        );
    }
}
