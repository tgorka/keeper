//! One process at a time holds a principal's copies.
//!
//! Each agent's copy keeps its Olm account and Megolm sessions in a sqlite
//! store under the data directory, and matrix-sdk's store is not safe for two
//! processes signed in as the same device: two `OlmMachine`s advancing one
//! account leave the one that wrote second out of step with the disk. So
//! `run` holds an exclusive lock on [`FILE`] for as long as it serves, and
//! `agents init`, which signs a copy in to make the proxy's DM, takes the
//! same lock or refuses, naming the process that holds it.

use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;

/// The lock file, in the data directory.
pub const FILE: &str = "agentd.lock";

/// The lock, held until it is dropped.
#[derive(Debug)]
pub struct HostLock {
    _file: File,
}

/// Who holds the lock, as its holder wrote it: `"<pid> <host>"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder(pub String);

/// The lock file under `data`.
pub fn path(data: &Path) -> PathBuf {
    data.join(FILE)
}

/// Take the lock under `data` for `host`, or say who holds it.
pub fn take(data: &Path, host: &str) -> std::io::Result<Result<HostLock, Holder>> {
    std::fs::create_dir_all(data)?;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path(data))?;
    if !file.try_lock_exclusive()? {
        let mut said = String::new();
        // The holder's line is a courtesy: an unreadable one still refuses.
        let _ = file.read_to_string(&mut said);
        return Ok(Err(Holder(said.trim().to_owned())));
    }
    file.set_len(0)?;
    file.rewind()?;
    writeln!(file, "{} {host}", std::process::id())?;
    file.sync_all()?;
    Ok(Ok(HostLock { _file: file }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lock is exclusive while held, names its holder, and is free
    /// again once dropped.
    #[test]
    fn one_holder_at_a_time() {
        let dir = tempfile::tempdir().expect("tempdir");
        let held = take(dir.path(), "electra").expect("io").expect("free");
        let refused = take(dir.path(), "electra").expect("io").expect_err("held");
        assert_eq!(refused, Holder(format!("{} electra", std::process::id())));
        drop(held);
        take(dir.path(), "electra")
            .expect("io")
            .expect("free again");
    }
}
