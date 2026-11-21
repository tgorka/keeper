//! What a device does with each domain it holds, given what the server last
//! said (story 114.5; replaces keeper-core's `databases::plan`).
//!
//! - a domain a successful discovery lists is **open**: opened, and linked
//!   once its refusal backoff allows;
//! - a held domain a successful discovery no longer lists was **left**: the
//!   device stops syncing it and deletes its copy (DF/AD-13). Only a
//!   discovery that answered can leave a domain: an unreachable server, a
//!   refusal or a map the server has not reconciled (503 `hq.map_closed`,
//!   an unknown barrier state) is no answer, and changes nothing (114.5-AC10);
//! - a device the server removed (401 on discovery) **keeps** every held
//!   domain, readable and unlinked, and deletes nothing (114.5-AC4).

/// What the server said to this device.
#[derive(Debug, Clone, Copy)]
pub enum Answer<'a> {
    /// A successful discovery, for a device the server has not removed:
    /// the domains it lists, in its order.
    Listed(&'a [String]),
    /// The server no longer knows this device: what it last listed, which
    /// orders the held domains and nothing else.
    Removed { last_listed: &'a [String] },
}

/// One domain's fate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    /// The server opens it to this device.
    Open,
    /// Held, readable and unlinked; nothing is deleted.
    Keep,
    /// The person left it: stop serving it and delete its copy.
    Leave,
}

/// Every domain this device opens or holds, each with its fate: the ones the
/// answer lists in its order, then the held ones it does not.
pub fn plan(held: &[String], answer: Answer<'_>) -> Vec<(String, Fate)> {
    let mut fates: Vec<(String, Fate)> = Vec::new();
    match answer {
        Answer::Listed(listed) => {
            for name in listed {
                if !fates.iter().any(|(known, _)| known == name) {
                    fates.push((name.clone(), Fate::Open));
                }
            }
            for name in held {
                if !listed.contains(name) {
                    fates.push((name.clone(), Fate::Leave));
                }
            }
        }
        Answer::Removed { last_listed } => {
            let ordered = last_listed
                .iter()
                .filter(|name| held.contains(name))
                .chain(held.iter().filter(|name| !last_listed.contains(name)));
            for name in ordered {
                if !fates.iter().any(|(known, _)| known == name) {
                    fates.push((name.clone(), Fate::Keep));
                }
            }
        }
    }
    fates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_listed_domain_is_opened_whether_or_not_it_is_held() {
        let held = names(&["makidrive"]);
        let listed = names(&["tgdrive", "makidrive", "crew"]);
        assert_eq!(
            plan(&held, Answer::Listed(&listed)),
            vec![
                ("tgdrive".to_owned(), Fate::Open),
                ("makidrive".to_owned(), Fate::Open),
                ("crew".to_owned(), Fate::Open),
            ]
        );
    }

    #[test]
    fn a_held_domain_a_successful_discovery_omits_is_left() {
        let held = names(&["crew", "makidrive", "tgdrive"]);
        let listed = names(&["tgdrive", "makidrive"]);
        assert_eq!(
            plan(&held, Answer::Listed(&listed)),
            vec![
                ("tgdrive".to_owned(), Fate::Open),
                ("makidrive".to_owned(), Fate::Open),
                ("crew".to_owned(), Fate::Leave),
            ]
        );
    }

    #[test]
    fn a_removed_device_keeps_every_held_domain_and_opens_nothing_new() {
        let held = names(&["crew", "tgdrive"]);
        let last = names(&["tgdrive", "makidrive"]);
        assert_eq!(
            plan(&held, Answer::Removed { last_listed: &last }),
            vec![
                ("tgdrive".to_owned(), Fate::Keep),
                ("crew".to_owned(), Fate::Keep),
            ]
        );
    }

    #[test]
    fn an_empty_listing_leaves_everything_and_a_repeated_name_opens_once() {
        let held = names(&["crew"]);
        assert_eq!(
            plan(&held, Answer::Listed(&[])),
            vec![("crew".to_owned(), Fate::Leave)]
        );
        let twice = names(&["crew", "crew"]);
        assert_eq!(
            plan(&held, Answer::Listed(&twice)),
            vec![("crew".to_owned(), Fate::Open)]
        );
    }
}
