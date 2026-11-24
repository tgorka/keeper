//! Validation and authorization rules for `config_values` writes.

use serde::{Deserialize, Serialize};

use crate::refusal::Refusal;
use crate::write::DomainKind;
use crate::Limits;

/// Maximum UTF-8 byte length of a `config_values` key.
const MAX_CONFIG_KEY_BYTES: usize = 256;

/// Role held by a person in a domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Read-only access.
    Reader,
    /// Content editing access.
    Editor,
    /// Review access.
    Reviewer,
    /// Full ownership of the domain.
    Owner,
}

/// Principal attempting a `config_values` write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigWriter {
    /// Administrator acting through the admin path.
    Admin,
    /// Human account holder with an optional role in the target domain.
    Person {
        /// Domain role, if any.
        #[serde(default)]
        role: Option<Role>,
    },
    /// An agent instance.
    Agent,
    /// A tool client.
    Tool,
    /// The internal service client.
    Service,
}

/// Validate a `config_values` key against length and segment grammar rules.
pub fn check_key(key: &str) -> Result<(), Refusal> {
    if key.is_empty() || key.len() > MAX_CONFIG_KEY_BYTES {
        return Err(Refusal::ConfigMalformed);
    }
    for segment in key.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(Refusal::ConfigMalformed);
        }
        if !segment
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
        {
            return Err(Refusal::ConfigMalformed);
        }
    }
    Ok(())
}

/// Map a `_template`-relative path to its `template/<path>` config key and validate it.
pub fn template_key(relative_path: &str) -> Result<String, Refusal> {
    let key = format!("template/{relative_path}");
    check_key(&key)?;
    Ok(key)
}

/// Return whether any `[A-Za-z0-9_-]` token in `text` matches a domain name ASCII-case-insensitively.
pub fn names_domain(text: &str, domains: &[&str]) -> bool {
    if domains.is_empty() {
        return false;
    }
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .filter(|token| !token.is_empty())
        .any(|token| {
            domains
                .iter()
                .any(|domain| !domain.is_empty() && token.eq_ignore_ascii_case(domain))
        })
}

/// Validate a `config_values` write in strict check order.
pub fn check_config_write(
    writer: &ConfigWriter,
    domain: DomainKind,
    key: &str,
    value: &str,
    domains: &[&str],
    limits: &Limits,
) -> Result<(), Refusal> {
    let allowed = match domain {
        DomainKind::General => matches!(writer, ConfigWriter::Admin),
        DomainKind::Personal | DomainKind::Group => matches!(
            writer,
            ConfigWriter::Admin
                | ConfigWriter::Person {
                    role: Some(Role::Owner),
                }
        ),
    };
    if !allowed {
        return Err(Refusal::ConfigWriterRefused);
    }

    check_key(key)?;

    if names_domain(key, domains) || names_domain(value, domains) {
        return Err(Refusal::ConfigNamesDomain);
    }

    if value.len() as u64 > limits.setting_value_max_bytes {
        return Err(Refusal::ConfigMalformed);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_limits() -> Limits {
        Limits {
            setting_value_max_bytes: 16,
            attachment_max_bytes: 1024,
        }
    }

    #[test]
    fn names_domain_is_case_insensitive_and_token_bounded() {
        assert!(!names_domain("tgdrive-light", &["tgdrive"]));
        assert!(names_domain("path/TGDrive.toml", &["tgdrive"]));
        assert!(names_domain("use=anadrive;", &["anadrive"]));
    }

    #[test]
    fn writer_refused_before_key_and_domain_checks() {
        let err = check_config_write(
            &ConfigWriter::Service,
            DomainKind::General,
            "../BAD",
            "makidrive",
            &["makidrive"],
            &sample_limits(),
        );
        assert_eq!(err, Err(Refusal::ConfigWriterRefused));
    }

    #[test]
    fn check_key_runs_before_names_domain_and_size() {
        let domains = ["makidrive"];
        assert_eq!(
            check_config_write(
                &ConfigWriter::Admin,
                DomainKind::General,
                "bad/../makidrive",
                "too long value exceeding sixteen bytes",
                &domains,
                &sample_limits(),
            ),
            Err(Refusal::ConfigMalformed)
        );
        assert_eq!(
            check_config_write(
                &ConfigWriter::Admin,
                DomainKind::General,
                "template/base.toml",
                "makidrive and too long value exceeding sixteen bytes",
                &domains,
                &sample_limits(),
            ),
            Err(Refusal::ConfigNamesDomain)
        );
    }

    #[test]
    fn template_key_prefixes_and_validates() {
        assert_eq!(
            template_key("class/desktop.toml"),
            Ok("template/class/desktop.toml".to_owned())
        );
        assert_eq!(
            template_key("../escape.toml"),
            Err(Refusal::ConfigMalformed)
        );
    }

    #[test]
    fn config_writer_deserializes_from_tagged_json() {
        assert_eq!(
            serde_json::from_str::<ConfigWriter>(r#"{"kind":"admin"}"#).ok(),
            Some(ConfigWriter::Admin)
        );
        assert_eq!(
            serde_json::from_str::<ConfigWriter>(r#"{"kind":"person","role":"owner"}"#).ok(),
            Some(ConfigWriter::Person {
                role: Some(Role::Owner),
            })
        );
        assert_eq!(
            serde_json::from_str::<ConfigWriter>(r#"{"kind":"person"}"#).ok(),
            Some(ConfigWriter::Person { role: None })
        );
    }
}
