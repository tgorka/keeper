//! The databases this device holds (R-HQ-17): one replica per server,
//! database and actor under `Platform::data_dir()`, at
//! `hq/servers/<server>/<database>/<actor>.sqlite`, plus
//! `hq/local/local.sqlite` for what is written while no server is paired. The
//! two namespaces never meet, whatever a server is called. The whole `hq/`
//! tree is kept out of device backup.
//!
//! Which databases sync is the server's answer to `GET /databases`. What it
//! last said is kept beside the replicas ([`Authorization`], no secret in
//! it), so a restart opens every held replica local-only and links only what
//! that answer — or the next — authorised; a device the server removed stays
//! removed until it is paired again.
//!
//! Which server this device last synced with is kept too
//! ([`remember_server`], its base URL and nothing else): with the address
//! cleared, that server's replicas stay listed and written to, and what is
//! written waits in each one's outbox — no request is made.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hq::routes::is_database_name;
use crate::hq::server::ServerUrl;

/// The directory under the data dir every replica lives in.
pub fn root(data_dir: &Path) -> PathBuf {
    data_dir.join("hq")
}

/// The replica of what is written while no server is paired.
pub fn local_replica(data_dir: &Path) -> PathBuf {
    root(data_dir).join("local").join("local.sqlite")
}

/// The server this device last held replicas of with a pairing.
fn last_server_file(data_dir: &Path) -> PathBuf {
    root(data_dir).join("last-server.json")
}

#[derive(Serialize, Deserialize)]
struct LastServer {
    server: String,
}

/// Remember `server` as the one this device last synced with; a no-op when
/// it already is.
pub fn remember_server(data_dir: &Path, server: &ServerUrl) -> std::io::Result<()> {
    if remembered_server(data_dir).as_ref() == Some(server) {
        return Ok(());
    }
    let path = last_server_file(data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(&LastServer {
        server: server.as_str().to_owned(),
    })
    .map_err(std::io::Error::other)?;
    let staged = path.with_extension("json.new");
    std::fs::write(&staged, bytes)?;
    std::fs::rename(staged, path)
}

/// The server this device last synced with; nothing kept, unreadable, or no
/// longer an address the grammar takes is none.
pub fn remembered_server(data_dir: &Path) -> Option<ServerUrl> {
    let bytes = std::fs::read(last_server_file(data_dir)).ok()?;
    let last: LastServer = serde_json::from_slice(&bytes).ok()?;
    ServerUrl::parse(&last.server).ok().flatten()
}

/// Forget that this device synced with `server`, when it is the one kept.
pub fn forget_server(data_dir: &Path, server: &ServerUrl) -> std::io::Result<()> {
    if remembered_server(data_dir).as_ref() != Some(server) {
        return Ok(());
    }
    match std::fs::remove_file(last_server_file(data_dir)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// One server's directory.
pub fn server_dir(data_dir: &Path, server: &ServerUrl) -> PathBuf {
    root(data_dir).join("servers").join(server.dir_name())
}

/// One database's replica of `actor`.
pub fn replica(data_dir: &Path, server: &ServerUrl, database: &str, actor: &str) -> PathBuf {
    server_dir(data_dir, server)
        .join(database)
        .join(format!("{actor}.sqlite"))
}

/// The databases of `server` this device already holds a replica of for
/// `actor`, by name.
pub fn held(data_dir: &Path, server: &ServerUrl, actor: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(server_dir(data_dir, server)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_database_name(name))
        .filter(|name| replica(data_dir, server, name, actor).is_file())
        .collect();
    names.sort();
    names
}

/// Every database this device shows: the ones the server opens to it, in the
/// server's order, then the ones it holds that the server no longer opens.
/// Each with whether it is open.
pub fn plan(held: &[String], open: &[String]) -> Vec<(String, bool)> {
    let mut shown: Vec<(String, bool)> = open.iter().map(|name| (name.clone(), true)).collect();
    for name in held {
        if !open.contains(name) {
            shown.push((name.clone(), false));
        }
    }
    shown
}

/// What the server last said about this device: whether it was removed, and
/// which databases its token opens. Nothing secret.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Authorization {
    pub removed: bool,
    pub open: Vec<String>,
}

fn authorization_file(data_dir: &Path, server: &ServerUrl) -> PathBuf {
    server_dir(data_dir, server).join("authorization.json")
}

impl Authorization {
    /// What was kept for `server`; nothing kept (or unreadable) is no
    /// authorisation at all — every held replica stays local until the
    /// server answers.
    pub fn load(data_dir: &Path, server: &ServerUrl) -> Authorization {
        std::fs::read(authorization_file(data_dir, server))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path, server: &ServerUrl) -> std::io::Result<()> {
        let path = authorization_file(data_dir, server);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        let staged = path.with_extension("json.new");
        std::fs::write(&staged, bytes)?;
        std::fs::rename(staged, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(raw: &str) -> ServerUrl {
        ServerUrl::parse(raw).expect("parses").expect("an address")
    }

    #[test]
    fn one_replica_per_server_database_and_actor_and_none_in_the_local_namespace() {
        let data = Path::new("/data");
        let a = server("http://100.64.0.7:8790");
        let path = replica(data, &a, "alpha", "ana");
        assert!(path.starts_with("/data/hq/servers/"), "{}", path.display());
        assert!(path.ends_with("alpha/ana.sqlite"));
        assert_ne!(path, replica(data, &a, "beta", "ana"));
        assert_ne!(
            path,
            replica(data, &server("https://other.example.org"), "alpha", "ana")
        );
        assert_eq!(
            local_replica(data),
            Path::new("/data/hq/local/local.sqlite")
        );
        // A server named `local` is not the local store.
        assert!(!server_dir(data, &server("http://local")).starts_with(root(data).join("local")));
    }

    #[test]
    fn the_held_replicas_are_found_on_disk_by_actor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = server("https://hq.example.org");
        for (db, actor) in [("alpha", "ana"), ("beta", "ana"), ("gamma", "ben")] {
            let path = replica(dir.path(), &a, db, actor);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"").expect("write");
        }
        std::fs::create_dir_all(server_dir(dir.path(), &a).join("Not A Name")).expect("mkdir");
        assert_eq!(held(dir.path(), &a, "ana"), ["alpha", "beta"]);
        assert!(held(dir.path(), &server("https://x.example.org"), "ana").is_empty());
    }

    #[test]
    fn a_database_the_server_dropped_is_still_shown_not_open() {
        let held = ["beta".to_owned(), "alpha".to_owned()];
        assert_eq!(
            plan(&held, &["alpha".to_owned()]),
            [("alpha".to_owned(), true), ("beta".to_owned(), false)]
        );
    }

    #[test]
    fn what_the_server_said_survives_a_restart_and_nothing_kept_authorises_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = server("https://hq.example.org");
        assert_eq!(
            Authorization::load(dir.path(), &a),
            Authorization::default()
        );
        let said = Authorization {
            removed: true,
            open: vec!["alpha".to_owned()],
        };
        said.save(dir.path(), &a).expect("saves");
        assert_eq!(Authorization::load(dir.path(), &a), said);
        std::fs::write(authorization_file(dir.path(), &a), b"{not json").expect("write");
        assert_eq!(
            Authorization::load(dir.path(), &a),
            Authorization::default()
        );
    }
}
