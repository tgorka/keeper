//! Dedicated empty scratch domain; run serially. Provision the base editor, a second
//! editor device, a reviewer and two tools through the admin path. For each extra role
//! (PEER, REVIEWER, PROPOSER, APPLIER) set `KEEPER_FILES_LIVE_<ROLE>_{ACTOR,CLIENT,TOKEN}`.
//! PROPOSER and APPLIER have propose-only and apply grants over A/ respectively.
//! Generate KEEPER_FILES_LIVE_RUN (32 lowercase random hex digits); provision domain
//! fileslive<RUN>, directories A/, B/, Move-<RUN>/ (empty), and a regular file
//! marker-<RUN> in the drive root containing exactly the run ID (no newline).
//! needs Linux `unshare -Urn` (each offline CLI gets its own empty network namespace).
use super::{check_ok_at, hash_file, live, required, Live, BIN};
use serde_json::Value;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

#[path = "manifest.rs"]
mod manifest;
#[path = "reviews.rs"]
mod reviews;

pub(super) struct Device<'a> {
    cfg: &'a Live,
    actor: String,
    client: String,
    token_env: String,
    state: TempDir,
}

impl<'a> Device<'a> {
    pub(super) fn new(cfg: &'a Live, role: &str) -> Self {
        let (actor, client, token_env) = if role == "EDITOR" {
            (
                cfg.actor.clone(),
                cfg.client.clone(),
                "KEEPER_HQ_LIVE_TOKEN_1".to_owned(),
            )
        } else {
            let prefix = format!("KEEPER_FILES_LIVE_{role}");
            (
                required(&format!("{prefix}_ACTOR")),
                required(&format!("{prefix}_CLIENT")),
                format!("{prefix}_TOKEN"),
            )
        };
        let _ = required(&token_env);
        Self {
            cfg,
            actor,
            client,
            token_env,
            state: tempfile::tempdir().expect("isolated CLI state"),
        }
    }

    fn call(&self, args: &[&str], status: i32, offline: bool) -> Value {
        let mut cmd = if offline {
            let mut cmd = Command::new("unshare");
            cmd.args(["-Urn", "--", BIN]);
            cmd
        } else {
            Command::new(BIN)
        };
        let out = cmd
            .args(["files", "--json", "--state"])
            .arg(self.state.path())
            .args([
                "--url",
                &self.cfg.url,
                "--database",
                &self.cfg.database,
                "--actor",
                &self.actor,
                "--client",
                &self.client,
                "--token-env",
                &self.token_env,
            ])
            .args(args)
            .output()
            .expect("run files CLI");
        let value: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|_| panic!("files CLI did not return JSON"));
        assert!(
            out.status.code() == Some(status),
            "unexpected files CLI exit status"
        );
        if status == 2 {
            assert!(
                value["ok"].as_bool() == Some(false),
                "expected files CLI refusal"
            );
        } else {
            check_ok_at(&value, &self.cfg.url, &self.cfg.database)
                .expect("ok in the intended domain");
            assert!(value.get("flush_error").is_none(), "files CLI flush failed");
        }
        value
    }

    pub(super) fn ok(&self, args: &[&str]) -> Value {
        self.call(args, 0, false)
    }

    fn body(&self, bytes: &[u8]) -> String {
        let path = self.state.path().join("fixture.bin");
        fs::write(&path, bytes).expect("write synthetic bytes");
        path.to_str().expect("utf8 fixture").to_owned()
    }

    fn put(&self, path: &str, bytes: &[u8]) -> Value {
        let file = self.body(bytes);
        let answer = self.ok(&["put", "--file", &file, "--drive", &self.cfg.drive, path]);
        assert!(answer["outcome"] == "accepted");
        assert!(answer["content_id"] == hash_file(Path::new(&file)).1);
        answer
    }

    fn get(&self, path: &str, bytes: &[u8]) -> Value {
        let answer = self.ok(&["get", "--drive", &self.cfg.drive, path]);
        assert!(
            fs::read(text(&answer, "file")).expect("read cached bytes") == bytes,
            "get: content mismatch"
        );
        answer
    }
}

pub(super) fn verify_fixture(cfg: &Live) {
    let editor = Device::new(cfg, "EDITOR");
    let mut actors = std::collections::HashSet::new();
    let mut clients = std::collections::HashSet::new();
    let mut tokens = std::collections::HashSet::new();
    for d in [
        Device::new(cfg, "PEER"),
        Device::new(cfg, "REVIEWER"),
        Device::new(cfg, "PROPOSER"),
        Device::new(cfg, "APPLIER"),
    ]
    .iter()
    .chain(std::iter::once(&editor))
    {
        assert!(
            actors.insert(d.actor.as_str())
                && clients.insert(d.client.as_str())
                && tokens.insert(required(&d.token_env)),
            "reused fixture principal, client or credential"
        );
    }
    // Fresh state: this read cannot flush an old queue or attest from an old cache.
    let marker = editor.get(&format!("marker-{}", cfg.run), cfg.run.as_bytes());
    assert!(
        marker["fetched"].as_bool() == Some(true),
        "run marker was not fetched"
    );
}

fn rows<'a>(answer: &'a Value, key: &str) -> &'a [Value] {
    answer[key].as_array().expect("array field")
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn entry<'a>(listing: &'a Value, name: &str) -> &'a Value {
    rows(listing, "entries")
        .iter()
        .find(|v| v["name"] == name)
        .expect("named entry")
}

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}
