//! The databases this device holds (R-HQ-17): one replica per server,
//! database and actor under `Platform::data_dir()`, at
//! `hq/<server>/<database>/<actor>.sqlite`, plus `hq/local/local.sqlite` for
//! what is written while no server is paired. The whole `hq/` tree is kept out
//! of device backup.
//!
//! Which databases sync is the server's answer to `GET /databases` after
//! pairing and at each start; until it answers, the replicas already on disk
//! open, so every read answers offline. A database that drops out of the
//! answer stops syncing and its replica stays readable until the person
//! removes it.

use std::path::{Path, PathBuf};

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

/// One server's directory.
pub fn server_dir(data_dir: &Path, server: &ServerUrl) -> PathBuf {
    root(data_dir).join(server.dir_name())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn server(raw: &str) -> ServerUrl {
        ServerUrl::parse(raw).expect("parses").expect("an address")
    }

    #[test]
    fn one_replica_per_server_database_and_actor() {
        let data = Path::new("/data");
        let a = server("http://100.64.0.7:8790");
        assert_eq!(
            replica(data, &a, "tgdrive", "tgorka"),
            Path::new("/data/hq/100.64.0.7_8790/tgdrive/tgorka.sqlite")
        );
        assert_ne!(
            replica(data, &a, "tgdrive", "tgorka"),
            replica(data, &a, "neuradrive", "tgorka")
        );
        assert_ne!(
            replica(data, &a, "tgdrive", "tgorka"),
            replica(
                data,
                &server("https://other.example.org"),
                "tgdrive",
                "tgorka"
            )
        );
        assert_eq!(
            local_replica(data),
            Path::new("/data/hq/local/local.sqlite")
        );
    }

    #[test]
    fn the_held_replicas_are_found_on_disk_by_actor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = server("https://hq.example.org");
        for (db, actor) in [
            ("tgdrive", "tgorka"),
            ("neuradrive", "tgorka"),
            ("other", "marta"),
        ] {
            let path = replica(dir.path(), &a, db, actor);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"").expect("write");
        }
        std::fs::create_dir_all(server_dir(dir.path(), &a).join("Not A Name")).expect("mkdir");
        assert_eq!(held(dir.path(), &a, "tgorka"), ["neuradrive", "tgdrive"]);
        assert!(held(dir.path(), &server("https://x.example.org"), "tgorka").is_empty());
    }

    #[test]
    fn a_database_the_server_dropped_is_still_shown_not_open() {
        let held = ["neuradrive".to_owned(), "tgdrive".to_owned()];
        assert_eq!(
            plan(&held, &["tgdrive".to_owned()]),
            [
                ("tgdrive".to_owned(), true),
                ("neuradrive".to_owned(), false)
            ]
        );
        assert_eq!(
            plan(&[], &["tgdrive".to_owned(), "neuradrive".to_owned()]),
            [
                ("tgdrive".to_owned(), true),
                ("neuradrive".to_owned(), true)
            ]
        );
    }
}
