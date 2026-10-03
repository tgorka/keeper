//! The mount rule and the pin (AD-377 layer 1, ruling R28 S-15).
//!
//! A process mounts a drive only if the drive's readers include every reader
//! of every drive it homes agents in: otherwise a session's own log write, or
//! a read it reports, would put a home's content where a reader of the
//! mounted drive who is not a reader of the home could read it. The rule runs
//! on the readers the host **pinned** (`agentd.toml` `[[drives]]`, or the
//! desktop's device-local pin), never on `_drive.toml`, which every reader of
//! the drive can edit — so it runs before any checkout, and a drive that fails
//! it is never fetched (C8).

use std::collections::BTreeSet;

use matrix_sdk::ruma::OwnedUserId;

use crate::agents::agentd::DrivePin;
use crate::agents::drive::DriveDecl;
use crate::agents::label::Readers;

/// Why a host may not mount a drive.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MountRefusal {
    /// The mounted drive's pinned readers miss readers of a home.
    #[error(
        "{drive} is never mounted here: this host homes agents in {home}, whose readers include {}, and {drive}'s pinned readers do not.",
        names(missing)
    )]
    Narrower {
        drive: String,
        home: String,
        missing: Vec<OwnedUserId>,
    },
    /// A home that is not among the drives this host mounts.
    #[error("this host homes agents in {home}, which it does not mount.")]
    HomeNotMounted { home: String },
}

impl MountRefusal {
    /// The refusal as the sentence an operator reads.
    pub fn sentence(&self) -> String {
        self.to_string()
    }
}

/// AD-377's rule over pinned readers: for every mounted drive `m` and every
/// home `h`, `readers(h) ⊆ readers(m)`. Drives are checked in the order given,
/// homes in theirs, and the first violation is the answer.
pub fn check(mounted: &[(String, Readers)], homes: &[String]) -> Result<(), MountRefusal> {
    let readers_of = |id: &str| {
        mounted
            .iter()
            .find(|(drive, _)| drive == id)
            .map(|(_, readers)| readers)
    };
    for home in homes {
        let home_readers =
            readers_of(home).ok_or_else(|| MountRefusal::HomeNotMounted { home: home.clone() })?;
        for (drive, readers) in mounted {
            if home_readers.is_within(readers) {
                continue;
            }
            let missing = match (home_readers, readers) {
                (Readers::Only(home), Readers::Only(drive)) => {
                    home.difference(drive).cloned().collect()
                }
                // `Anyone` is within everything and contains everything, so
                // the only failing shape left is a home open to anyone.
                _ => Vec::new(),
            };
            return Err(MountRefusal::Narrower {
                drive: drive.clone(),
                home: home.clone(),
                missing,
            });
        }
    }
    Ok(())
}

/// How a drive's `_drive.toml` differs from the host's pin. A zone whose
/// declaration differs hosts nothing until a person re-pins or fixes the file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{drive}'s agents zone hosts nothing here: {}.", differences.join("; "))]
pub struct PinDifference {
    pub drive: String,
    /// One clause per difference, each naming both values.
    pub differences: Vec<String>,
}

impl PinDifference {
    /// The difference as the sentence `status` and `agents list` print.
    pub fn sentence(&self) -> String {
        self.to_string()
    }
}

/// Compare a drive's declaration with its pin: the id, the owner, the readers
/// and `local_only`. The pin is what the host believes; the file never widens
/// it. A file may be stricter than the pin about `local_only` (it then binds,
/// as `_drive.toml` always does), never weaker. `principal`, `title` and
/// `[integrity]` are not pinned: they say nothing about who may read.
pub fn pin_matches(decl: &DriveDecl, pin: &DrivePin) -> Result<(), PinDifference> {
    let mut differences = Vec::new();
    if decl.id != pin.id {
        differences.push(format!(
            "_drive.toml names the id {}; this host pinned {}",
            decl.id, pin.id
        ));
    }
    if decl.owner != pin.owner {
        differences.push(format!(
            "_drive.toml names the owner {}; this host pinned {}",
            decl.owner, pin.owner
        ));
    }
    if decl.readers != pin.readers {
        differences.push(format!(
            "_drive.toml names the readers {}; this host pinned {}",
            names(&decl.readers),
            names(&pin.readers)
        ));
    }
    if pin.local_only && !decl.local_only {
        differences.push(
            "_drive.toml says local_only = false; this host pinned local_only = true".to_owned(),
        );
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(PinDifference {
            drive: pin.id.clone(),
            differences,
        })
    }
}

/// Matrix ids, comma-separated, in the order given (sorted for a set).
fn names<'a>(users: impl IntoIterator<Item = &'a OwnedUserId>) -> String {
    users
        .into_iter()
        .map(|user| user.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A set of Matrix ids from text, for tests and callers that hold strings.
pub fn readers_from(ids: &[&str]) -> Option<BTreeSet<OwnedUserId>> {
    ids.iter()
        .map(|id| OwnedUserId::try_from(*id).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(ids: &[&str]) -> Readers {
        Readers::Only(readers_from(ids).expect("ids"))
    }

    const TGORKA: &str = "@tgorka:example.org";
    const MARTA: &str = "@marta:example.org";

    #[test]
    fn neuraffica_never_mounts_tgdrive() {
        let mounted = vec![
            ("neuradrive".to_owned(), only(&[TGORKA, MARTA])),
            ("tgdrive".to_owned(), only(&[TGORKA])),
        ];
        let refusal = check(&mounted, &["neuradrive".to_owned()]).expect_err("refused");
        assert_eq!(
            refusal,
            MountRefusal::Narrower {
                drive: "tgdrive".to_owned(),
                home: "neuradrive".to_owned(),
                missing: vec![OwnedUserId::try_from(MARTA).expect("id")],
            }
        );
        assert!(refusal.sentence().contains("@marta:example.org"));
    }

    #[test]
    fn tgorka_may_mount_neuradrive() {
        let mounted = vec![
            ("tgdrive".to_owned(), only(&[TGORKA])),
            ("neuradrive".to_owned(), only(&[TGORKA, MARTA])),
        ];
        assert_eq!(check(&mounted, &["tgdrive".to_owned()]), Ok(()));
    }

    #[test]
    fn a_home_that_is_not_mounted_is_refused() {
        let mounted = vec![("tgdrive".to_owned(), only(&[TGORKA]))];
        assert_eq!(
            check(&mounted, &["neuradrive".to_owned()]),
            Err(MountRefusal::HomeNotMounted {
                home: "neuradrive".to_owned()
            })
        );
    }

    fn decl(owner: &str, readers: &[&str]) -> DriveDecl {
        DriveDecl {
            id: "tgdrive".to_owned(),
            title: "tgdrive".to_owned(),
            principal: "tgorka".to_owned(),
            owner: OwnedUserId::try_from(owner).expect("owner"),
            readers: readers_from(readers).expect("readers"),
            local_only: false,
            untrusted: Vec::new(),
        }
    }

    fn pin(owner: &str, readers: &[&str]) -> DrivePin {
        DrivePin {
            id: "tgdrive".to_owned(),
            remote: "/srv/tgdrive.git".to_owned(),
            credential: None,
            owner: OwnedUserId::try_from(owner).expect("owner"),
            readers: readers_from(readers).expect("readers"),
            local_only: false,
        }
    }

    #[test]
    fn a_declaration_that_differs_from_its_pin_hosts_nothing_naming_the_difference() {
        let pinned = pin(TGORKA, &[MARTA, TGORKA]);

        let wider = decl(TGORKA, &[MARTA, TGORKA, "@x:example.org"]);
        let sentence = pin_matches(&wider, &pinned).expect_err("wider").sentence();
        assert!(
            sentence.contains(
                "_drive.toml names the readers @marta:example.org, @tgorka:example.org, @x:example.org; this host pinned @marta:example.org, @tgorka:example.org"
            ),
            "{sentence}"
        );

        let other_owner = decl(MARTA, &[MARTA, TGORKA]);
        let sentence = pin_matches(&other_owner, &pinned)
            .expect_err("owner")
            .sentence();
        assert!(
            sentence.contains("_drive.toml names the owner @marta:example.org; this host pinned @tgorka:example.org"),
            "{sentence}"
        );

        assert_eq!(
            pin_matches(&decl(TGORKA, &[TGORKA, MARTA]), &pinned),
            Ok(())
        );
    }

    #[test]
    fn a_declaration_weaker_than_a_local_only_pin_hosts_nothing() {
        let pinned = DrivePin {
            local_only: true,
            ..pin(TGORKA, &[TGORKA])
        };
        let sentence = pin_matches(&decl(TGORKA, &[TGORKA]), &pinned)
            .expect_err("a reader turned local_only off")
            .sentence();
        assert_eq!(
            sentence,
            "tgdrive's agents zone hosts nothing here: _drive.toml says local_only = false; this host pinned local_only = true."
        );

        let strict = DriveDecl {
            local_only: true,
            ..decl(TGORKA, &[TGORKA])
        };
        assert_eq!(pin_matches(&strict, &pinned), Ok(()));
        assert_eq!(
            pin_matches(&strict, &pin(TGORKA, &[TGORKA])),
            Ok(()),
            "a file stricter than its pin binds; it is not a difference"
        );
    }
}
