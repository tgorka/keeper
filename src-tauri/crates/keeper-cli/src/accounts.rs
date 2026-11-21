//! `keeper-cli accounts`: the accounts module without a window (story
//! 114.5). One verb:
//!
//! - `domains --data-dir <dir> --url <server>` — the person's domains as a
//!   keeper device with that data directory last showed them: each domain's
//!   kind, name and people, the git repository it replaces, its file-write
//!   record and graph root when discovery gave them, the domains left, and
//!   how current it is (`state`, `readAtMs`). Under `--json` it is the view
//!   model `keeper-accounts` builds, as the app's Settings renders it, one
//!   line; otherwise one sentence per domain.
//!
//! It reads the view the device keeps beside its replicas and contacts no
//! server, so it reads a running app's state without disturbing it.
//!
//! Exit codes: `0` the view was read; `2` the address is not one keeper
//! contacts; `3` the device keeps no view for that server (never paired with
//! it, or it never answered); `1` the kept view cannot be read.

use std::path::{Path, PathBuf};

use crate::output::Printer;
use crate::{EXIT_CONFIG, EXIT_FAILURE, EXIT_OK, EXIT_PREREQUISITE};

use keeper_accounts::{DomainsState, DomainsVm};
use keeper_ports::server::ServerUrl;

/// `keeper-cli accounts`'s verbs.
#[derive(clap::Subcommand)]
pub enum Args {
    /// The person's domains, as the device last showed them.
    Domains {
        /// The keeper data directory of the device.
        #[arg(long)]
        data_dir: PathBuf,
        /// The agents server's base URL, as set in the app.
        #[arg(long)]
        url: String,
    },
}

pub fn main(args: Args, printer: &Printer) -> u8 {
    match args {
        Args::Domains { data_dir, url } => domains(&data_dir, &url, printer),
    }
}

/// Where keeper-core's `hq::databases::domains_file` keeps the view: its
/// replica layout, `hq/servers/<server>/`, and the file beside them.
fn kept_view(data_dir: &Path, server: &ServerUrl) -> PathBuf {
    data_dir
        .join("hq")
        .join("servers")
        .join(server.dir_name())
        .join("domains.json")
}

fn domains(data_dir: &Path, url: &str, printer: &Printer) -> u8 {
    let server = match ServerUrl::parse(url) {
        Ok(Some(server)) => server,
        Ok(None) | Err(_) => {
            eprintln!("keeper-cli: {url:?} is not an agents server address keeper contacts");
            return EXIT_CONFIG;
        }
    };
    let path = kept_view(data_dir, &server);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "keeper-cli: this device keeps no domains for {} ({})",
                server.as_str(),
                path.display()
            );
            return EXIT_PREREQUISITE;
        }
        Err(error) => {
            eprintln!("keeper-cli: {}: {error}", path.display());
            return EXIT_FAILURE;
        }
    };
    let view: DomainsVm = match serde_json::from_slice(&bytes) {
        Ok(view) => view,
        Err(error) => {
            eprintln!("keeper-cli: {}: {error}", path.display());
            return EXIT_FAILURE;
        }
    };
    match serde_json::to_value(&view) {
        Ok(answer) => {
            printer.answer(&answer, |_| human(&view));
            EXIT_OK
        }
        Err(error) => {
            eprintln!("keeper-cli: {error}");
            EXIT_FAILURE
        }
    }
}

/// One sentence per domain, then one per domain left.
fn human(view: &DomainsVm) -> String {
    let state = match view.state {
        DomainsState::NotConnected => "not connected",
        DomainsState::Reading => "being read",
        DomainsState::Current => "current",
        DomainsState::Offline => "offline, last known",
        DomainsState::Removed => "this device was removed; copies kept",
    };
    let mut lines = vec![format!(
        "{} domain(s), {state}{}",
        view.domains.len(),
        view.read_at_ms
            .map(|at| format!(", read at {at} ms"))
            .unwrap_or_default()
    )];
    for domain in &view.domains {
        let kind = serde_json::to_value(domain.kind)
            .ok()
            .and_then(|kind| kind.as_str().map(str::to_owned))
            .unwrap_or_default();
        let people = match &domain.members {
            None => "people not read yet".to_owned(),
            Some(members) => members
                .iter()
                .map(|member| member.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        };
        let mut line = format!(
            "{} ({kind}, {}): {people}",
            domain.display_name, domain.name
        );
        if let Some(repository) = &domain.replaced_repository {
            line.push_str(&format!("; replaces the git repository {repository}"));
        }
        lines.push(line);
    }
    for left in &view.left {
        lines.push(if left.kept {
            format!(
                "left {} ({}): its copy is kept here with {} unsent write(s) and {} graph \
                 change(s), sent if it is listed again",
                left.display_name, left.name, left.unsent, left.graph_intents
            )
        } else {
            format!(
                "left {} ({}): its copy is deleted here",
                left.display_name, left.name
            )
        });
    }
    lines.join("\n")
}

/// What `--json` prints: the kept view model, as it is.
#[cfg(test)]
fn answer(view: &DomainsVm) -> serde_json::Value {
    serde_json::to_value(view).unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_accounts_api::discovery::Discovery;
    use keeper_accounts_api::DomainKind;
    use serde_json::Value;

    const CORPUS: &str = include_str!("../../../../syncular/accounts.vectors.json");

    /// 114.5-AC5 and AC9: the kept view reads back as the view model
    /// `keeper-accounts` built, `replaced_repository`, `files_writes` and
    /// `graph_root` carried exactly as discovery gave them.
    #[test]
    fn the_kept_view_prints_as_the_view_model_it_is() {
        let corpus: Value = serde_json::from_str(CORPUS).expect("corpus");
        let dir = std::env::temp_dir().join(format!("keeper-cli-accounts-{}", std::process::id()));
        let server = ServerUrl::parse("https://hq.example.org")
            .expect("parses")
            .expect("an address");
        for vector in corpus["discovery"].as_array().expect("vectors") {
            let found: Discovery = serde_json::from_value(vector["json"].clone()).expect("parses");
            let mut view = DomainsVm {
                state: DomainsState::Current,
                ..DomainsVm::default()
            };
            view.discovered(&found.databases, 42);
            let path = kept_view(&dir, &server);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, serde_json::to_vec(&view).expect("json")).expect("keeps");
            let read: DomainsVm =
                serde_json::from_slice(&std::fs::read(&path).expect("reads")).expect("parses");
            let printed = answer(&read);
            assert_eq!(printed, serde_json::to_value(&view).expect("json"));
            let want = vector["want"]["databases"].as_array().expect("want");
            for (domain, wanted) in printed["domains"]
                .as_array()
                .expect("domains")
                .iter()
                .zip(want)
            {
                assert_eq!(
                    domain.get("replacedRepository"),
                    wanted.get("replaced_repository")
                );
                assert_eq!(domain.get("filesWrites"), wanted.get("files_writes"));
                assert_eq!(domain.get("graphRoot"), wanted.get("graph_root"));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_human_answer_says_the_repository_is_replaced_not_cut_over() {
        let mut view = DomainsVm::default();
        view.discovered(
            &[keeper_accounts_api::discovery::DomainEntry {
                name: "tgdrive".to_owned(),
                mount: "/tgdrive".to_owned(),
                kind: DomainKind::Personal,
                display_name: "tgdrive".to_owned(),
                replaced_repository: Some("https://forge.example/tgdrive.git".to_owned()),
                graph_root: None,
                files_writes: None,
            }],
            1,
        );
        view.leave(
            "crew",
            keeper_accounts::Unsent {
                outbox: 2,
                graph_intents: 1,
                unreadable: false,
            },
            3,
        );
        view.leave("scratch", keeper_accounts::Unsent::default(), 4);
        let text = human(&view);
        assert!(text.contains("replaces the git repository https://forge.example/tgdrive.git"));
        assert!(text.contains(
            "left crew (crew): its copy is kept here with 2 unsent write(s) and 1 graph change(s)"
        ));
        assert!(text.contains("left scratch (scratch): its copy is deleted here"));
    }
}
