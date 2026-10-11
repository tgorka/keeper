//! `harden_process`: what agentd does to itself before any thread starts (S-07).

use keeper_sync::xdg::{secret_file_name, SecretStore};

/// Why a host could not harden itself.
#[derive(Debug, thiserror::Error)]
pub enum HardenError {
    /// A secret the configuration names could not be read: a loose file, a
    /// symlinked one, a secrets directory that breaks the strict rule.
    #[error("secret:{name} cannot be read: {source}")]
    Secret {
        name: String,
        #[source]
        source: keeper_sync::SyncError,
    },
    /// A secret the configuration names is in none of the three places.
    #[error(
        "secret:{name} is not set: give the unit LoadCredential={credential}:<file>, set {variable}, \
         or write it to {} with mode 0600",
        file.display()
    )]
    Missing {
        name: String,
        /// The credential's name in `$CREDENTIALS_DIRECTORY`.
        credential: String,
        variable: String,
        file: std::path::PathBuf,
    },
    /// A secret variable could not be held (its value is not text).
    #[error("keeper-agentd could not take its secrets out of the environment: {0}")]
    Environment(keeper_sync::SyncError),
    /// The kernel refused `PR_SET_DUMPABLE`.
    #[error("keeper-agentd could not make itself non-dumpable: {0}")]
    Dumpable(std::io::Error),
}

/// Read every secret the configuration names, move every
/// `KEEPER_AGENTD_SECRET_*` variable out of the environment into the store,
/// and, on Linux, make the process non-dumpable — so no child it starts
/// inherits a secret variable, and no other process of the same user reads
/// its memory or its `/proc` files. A needed secret that is absent stops it
/// here, naming where it may be put, rather than at its first use.
///
/// Call it from `main` while the process has one thread, before the async
/// runtime is built: changing the environment while another thread reads it
/// is undefined behaviour in libc.
pub fn harden_process(store: &mut SecretStore, needed: &[&str]) -> Result<(), HardenError> {
    for name in needed {
        let found = store.get(name).map_err(|source| HardenError::Secret {
            name: (*name).to_owned(),
            source,
        })?;
        if found.is_none() {
            return Err(HardenError::Missing {
                name: (*name).to_owned(),
                credential: secret_file_name(name),
                variable: store.env_var_of(name),
                file: store.path_of(name),
            });
        }
    }
    store.scrub_env().map_err(HardenError::Environment)?;
    #[cfg(target_os = "linux")]
    rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
        .map_err(|errno| HardenError::Dumpable(errno.into()))?;
    Ok(())
}
