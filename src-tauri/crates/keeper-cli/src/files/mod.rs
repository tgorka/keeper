//! `keeper-cli files`: a domain's drives from a shell.
//! `drives`, `ls`, `get`, `put`, `move`, `delete`, `propose`, `proposals`,
//! `proposal`, `decide`, `unreviewed`, `acknowledge`, `preview-reversal`,
//! `reverse` and `history` act on one `(server, domain)` through
//! `keeper-files`: a listing reads the replica alone, offline as online;
//! `get` fetches into the device's cache only what is not cached already;
//! `put` uploads, then commits, and waits in the queue when the server
//! cannot be reached; `--propose` and the review verbs drive proposals,
//! decisions, acknowledgements, reversals and history. Every verb first
//! commits what waits in the queue; a queued put whose source is gone or
//! changed is dropped and named in `flushed`, and a flush that fails is
//! reported as `flush_error` without failing the verb.
//!
//! The replica, the cache (`cache/`), the upload plans (`plans/`) and the
//! queue (`files-queue.json`) live under `--state`; the bearer token is read
//! from the environment variable `--token-env` names and never printed.
//!
//! Exit codes: `0` done · `1` failed, may succeed later (server, store,
//! storage, offline, unanswered) · `2` the invocation is wrong or the server
//! refused the change for good · `4` the put waits in the queue.

mod review;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_files::{Files, FilesError, Layout, PutOutcome, PutReport, Target};
use keeper_ports::transfer::FileTransfer;
use keeper_ports::{Link, LinkState, ReplicaError, ReplicaSpec, Source, StoreSlot};
use keeper_replica::transfer::{announced_store, HttpGrants, StoreTransfer, TransferConfig};
use keeper_replica::SyncularReplica;
use serde_json::{json, Value};

use crate::output::Printer;
use crate::{EXIT_CONFIG, EXIT_DEFERRED, EXIT_FAILURE, EXIT_OK};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// `keeper-cli files`'s arguments, before the verb.
#[derive(clap::Args)]
pub struct Args {
    /// Where the replica, the cache, the upload plans and the queue persist.
    #[arg(long)]
    pub(crate) state: PathBuf,
    /// keeper-hq's base URL (`<url>/<domain>` is the mount).
    #[arg(long)]
    pub(crate) url: String,
    /// The domain (database).
    #[arg(long)]
    pub(crate) database: String,
    /// The person's actor id.
    #[arg(long)]
    pub(crate) actor: String,
    /// This device's client id.
    #[arg(long)]
    pub(crate) client: String,
    /// The environment variable holding the bearer token.
    #[arg(long, default_value = "KEEPER_FILES_TOKEN")]
    pub(crate) token_env: String,
    /// How long to wait for the server's first answers (milliseconds).
    #[arg(long, default_value_t = 5000)]
    pub(crate) wait_ms: u64,
    /// Override the single-PUT threshold in bytes (tests).
    #[arg(long, hide = true)]
    pub(crate) threshold: Option<u64>,
    /// Override the part size in bytes (tests).
    #[arg(long, hide = true)]
    pub(crate) part_bytes: Option<u64>,
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// A drive and a path in it, or an entry by id.
#[derive(clap::Args, Clone, Debug)]
pub(crate) struct Where {
    /// The drive's name.
    #[arg(long, conflicts_with = "entry")]
    pub(crate) drive: Option<String>,
    /// An entry's id, in place of a drive and path.
    #[arg(long)]
    pub(crate) entry: Option<String>,
    /// The drive-relative path (`""` is the drive's root).
    pub(crate) path: Option<String>,
}

/// Flags shared by verbs that can submit a proposal.
#[derive(clap::Args, Default, Clone, Debug)]
pub(crate) struct ProposeFlags {
    /// Submit the change as a proposal instead of applying it directly.
    #[arg(long)]
    pub(crate) propose: bool,
    /// Actor id allowed to decide the proposal (repeatable).
    #[arg(long, value_name = "ACTOR", conflicts_with = "reviewers_role")]
    pub(crate) reviewer: Vec<String>,
    /// Address the proposal to the domain's reviewer role.
    #[arg(long, conflicts_with = "reviewer")]
    pub(crate) reviewers_role: bool,
    /// Proposal expiry timestamp in milliseconds since Unix epoch.
    #[arg(long, value_name = "MS")]
    pub(crate) expires_at_ms: Option<i64>,
    /// Optional change-set ULID grouping related changes or proposals.
    #[arg(long, value_name = "ULID")]
    pub(crate) change_set: Option<String>,
}

#[derive(clap::Subcommand)]
pub(crate) enum Command {
    /// The domain's drives and their root entry ids.
    Drives,
    /// A folder's entries, from the replica.
    Ls {
        #[command(flatten)]
        at: Where,
    },
    /// A file into the cache (fetched only when not cached), and to `--out`.
    Get {
        #[command(flatten)]
        at: Where,
        /// Where to write a copy of the file.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Put a local file: uploaded, then committed (queued while offline), or proposed.
    Put {
        /// The local file.
        #[arg(long)]
        file: PathBuf,
        #[command(flatten)]
        at: Where,
        #[command(flatten)]
        propose: ProposeFlags,
    },
    /// Move or rename an entry directly, or propose the move.
    Move {
        #[command(flatten)]
        at: Where,
        /// Destination drive name.
        #[arg(long)]
        to_drive: String,
        /// Destination drive-relative path.
        #[arg(long)]
        to: String,
        #[command(flatten)]
        propose: ProposeFlags,
    },
    /// Delete an entry directly, or propose the deletion.
    Delete {
        #[command(flatten)]
        at: Where,
        #[command(flatten)]
        propose: ProposeFlags,
    },
    /// Propose putting a local file (equivalent to `put --propose`).
    Propose {
        /// The local file.
        #[arg(long)]
        file: PathBuf,
        #[command(flatten)]
        at: Where,
        #[command(flatten)]
        propose: ProposeFlags,
    },
    /// List open proposals addressed to this caller (or own revisions with `--mine`).
    Proposals {
        /// Filter to proposals in this change set.
        #[arg(long, value_name = "ID")]
        change_set: Option<String>,
        /// List this actor's own proposal revisions from the replica.
        #[arg(long)]
        mine: bool,
    },
    /// Inspect one proposal's revisions, optionally opening the latest revision's files.
    #[command(name = "proposal")]
    ProposalCmd {
        /// The proposal id.
        id: String,
        /// Fetch/open the latest revision's file contents into the cache.
        #[arg(long)]
        open: bool,
    },
    /// Approve or reject one or more proposals, or every open proposal in a change set.
    #[command(group(
        clap::ArgGroup::new("verdict")
            .required(true)
            .multiple(false)
            .args(["approve", "reject"])
    ))]
    Decide {
        /// Proposal ids to decide.
        ids: Vec<String>,
        /// Decide every open proposal in this change set.
        #[arg(long, value_name = "ID")]
        change_set: Option<String>,
        /// Approve the named proposals.
        #[arg(long)]
        approve: bool,
        /// Reject the named proposals.
        #[arg(long)]
        reject: bool,
    },
    /// List unreviewed agent-, tool- and job-origin events.
    Unreviewed {
        /// Return events with sequence above this cursor.
        #[arg(long, default_value = "0")]
        after: String,
        /// Maximum number of events to return.
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Filter to events in this change set.
        #[arg(long, value_name = "ID")]
        change_set: Option<String>,
    },
    /// Acknowledge one or more events, or every unreviewed event in a change set.
    Acknowledge {
        /// Event ids to acknowledge.
        ids: Vec<String>,
        /// Acknowledge every unreviewed event in this change set.
        #[arg(long, value_name = "ID")]
        change_set: Option<String>,
    },
    /// Preview the server-planned inverse of an event without applying it.
    PreviewReversal {
        /// The event id to preview reversing.
        event_id: String,
    },
    /// Apply the server-planned inverse of an event.
    Reverse {
        /// The event id to reverse.
        event_id: String,
    },
    /// Read paged event history, optionally filtered by entity, actor, origin or change set.
    History {
        /// Return events with sequence above this cursor.
        #[arg(long, default_value = "0")]
        after: String,
        /// Maximum number of events to return.
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Filter to events touching this entity id.
        #[arg(long, value_name = "ID")]
        entity: Option<String>,
        /// Filter to events authored by this actor id.
        #[arg(long, value_name = "ID")]
        actor: Option<String>,
        /// Filter to events with this origin class (`human`, `agent`, `tool`, `import`, `job`).
        #[arg(long, value_name = "CLASS")]
        origin: Option<String>,
        /// Filter to every matching event in this change set.
        #[arg(long, value_name = "ID")]
        change_set: Option<String>,
    },
}

impl Where {
    pub(crate) fn target(&self) -> Result<Target, Value> {
        match (&self.entry, &self.drive) {
            (Some(entry), None) if self.path.is_none() => Ok(Target::Entry(entry.clone())),
            (None, Some(drive)) => Ok(Target::Path {
                drive: drive.clone(),
                path: self.path.clone().unwrap_or_default(),
            }),
            _ => Err(failure(
                "usage",
                "name --drive <drive> [path], or --entry <id> alone",
            )),
        }
    }
}

pub(crate) fn failure(code: &str, message: &str) -> Value {
    json!({"ok": false, "code": code, "error": message})
}

pub(crate) fn files_failure(error: &FilesError) -> Value {
    let code = match error {
        FilesError::PathRefused(_) => "path_refused".to_owned(),
        FilesError::NameRefused(_) => "name_refused".to_owned(),
        FilesError::NotFound(_) => "not_found".to_owned(),
        FilesError::NotAFolder(_) => "not_a_folder".to_owned(),
        FilesError::NotAFile(_) => "not_a_file".to_owned(),
        FilesError::Offline(_) => "offline".to_owned(),
        FilesError::Unanswered(_) => "unanswered".to_owned(),
        FilesError::Refused { code, .. } => serde_json::to_value(code)
            .ok()
            .and_then(|code| code.as_str().map(str::to_owned))
            .unwrap_or_else(|| "refused".to_owned()),
        FilesError::Row(_) => "row".to_owned(),
        FilesError::Storage(_) => "storage".to_owned(),
        FilesError::Replica(error) => replica_code(error),
    };
    let mut answer = failure(&code, &error.to_string());
    if let FilesError::Refused {
        detail: Some(detail),
        ..
    } = error
    {
        answer["detail"] = detail.clone();
    }
    answer
}

fn replica_code(error: &ReplicaError) -> String {
    match error {
        ReplicaError::Remote { code, .. }
        | ReplicaError::Rejected { code, .. }
        | ReplicaError::Unheard { code, .. } => code.clone(),
        ReplicaError::Client(_) => "client.error".to_owned(),
        ReplicaError::NotSyncing => "client.not_syncing".to_owned(),
        ReplicaError::Closed => "client.closed".to_owned(),
    }
}

/// The exit code an answer earns.
pub(crate) fn exit_code(answer: &Value) -> u8 {
    if answer["ok"] == true {
        return match answer["outcome"].as_str() {
            Some("queued" | "sent") => EXIT_DEFERRED,
            Some("dropped") => EXIT_FAILURE,
            _ => EXIT_OK,
        };
    }
    match answer["code"].as_str().unwrap_or_default() {
        "unanswered" => EXIT_FAILURE,
        "usage"
        | "config"
        | "path_refused"
        | "name_refused"
        | "not_found"
        | "not_a_folder"
        | "not_a_file"
        | "sync.credential_refused" => EXIT_CONFIG,
        code if keeper_files_api::refusal::Code::ALL.iter().any(|known| {
            serde_json::to_value(known)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                == Some(code.to_owned())
        }) =>
        {
            EXIT_CONFIG
        }
        _ => EXIT_FAILURE,
    }
}

/// The replica, linked to the server: reads answer from its file whether
/// or not the server answers.
fn open(args: &Args, token: &str) -> Result<Source, Value> {
    std::fs::create_dir_all(&args.state).map_err(|e| failure("storage", &e.to_string()))?;
    let url = args.url.trim_end_matches('/');
    let replica = SyncularReplica::open(ReplicaSpec {
        database: args.database.clone(),
        path: args.state.join(format!("{}.sqlite", args.database)),
        client_id: Some(args.client.clone()),
        link: Some(Link {
            base_url: format!("{url}/{}", args.database),
            token: token.to_owned(),
            store: StoreSlot::default(),
        }),
        units: keeper_files::units(&args.database, &args.actor, &args.client),
        watch_presence: None,
    })
    .map_err(|e| failure("storage", &e.to_string()))?;
    Ok(Source {
        database: args.database.clone(),
        replica: Arc::new(replica),
        actor: args.actor.clone(),
        client: args.client.clone(),
    })
}

/// The transfer, once the server has announced its store; `None` when it
/// cannot be reached (offline: reads and the cache still answer).
fn transfer(args: &Args, token: &str) -> Result<Option<Arc<dyn FileTransfer>>, Value> {
    let url = args.url.trim_end_matches('/');
    let store = match announced_store(url, &args.database, token, REQUEST_TIMEOUT) {
        Ok(store) => store,
        Err(ReplicaError::Unheard { .. } | ReplicaError::NotSyncing) => return Ok(None),
        Err(error) => return Err(failure(&replica_code(&error), &error.to_string())),
    };
    let slot = StoreSlot::default();
    slot.set(store);
    let base = format!("{url}/{}", args.database);
    let grants = Arc::new(
        HttpGrants::new(&base, token, REQUEST_TIMEOUT)
            .map_err(|e| failure(&replica_code(&e), &e.to_string()))?,
    );
    let mut config = TransferConfig::contract(args.state.join("plans"));
    if let Some(threshold) = args.threshold {
        config.threshold = threshold;
    }
    if let Some(part_bytes) = args.part_bytes.filter(|bytes| *bytes > 0) {
        config.part_bytes = part_bytes;
    }
    let transfer = StoreTransfer::new(grants, &base, slot, config)
        .map_err(|e| failure(&replica_code(&e), &e.to_string()))?;
    Ok(Some(Arc::new(transfer)))
}

/// Wait at most `limit` for the replica's first round, then pull what the
/// head names: each drive subscribed, then one more round.
fn settle(source: &Source, files: &Files, limit: Duration) {
    let started = Instant::now();
    let round = |since: u64| {
        source.replica.sync_now();
        while started.elapsed() < limit {
            let state = source.replica.state();
            if matches!(
                state.link,
                LinkState::Refused | LinkState::Closed | LinkState::Offline
            ) || (matches!(state.link, LinkState::Connected) && state.rows != since)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    round(source.replica.state().rows);
    if matches!(source.replica.state().link, LinkState::Connected) {
        let before = source.replica.state().rows;
        if files.subscribe_drives().is_ok() {
            round(before);
        }
    }
}

fn put_answer(report: &PutReport) -> Value {
    let mut answer = json!({
        "request_id": report.request_id,
        "entry_id": report.entry_id,
        "content_id": report.content_id,
        "size": report.size,
        "bytes_sent": report.bytes_sent,
        "parts_sent": report.parts_sent,
        "parts_skipped": report.parts_skipped,
    });
    answer["outcome"] = json!(match &report.outcome {
        PutOutcome::Accepted => "accepted",
        PutOutcome::ConflictCopy { .. } => "conflict_copy",
        PutOutcome::Queued => "queued",
        PutOutcome::Sent => "sent",
        PutOutcome::Dropped(_) => "dropped",
    });
    match &report.outcome {
        PutOutcome::ConflictCopy { of, entry_id, name } => {
            answer["conflict"] = json!({"of": of, "entry_id": entry_id, "name": name});
        }
        PutOutcome::Dropped(reason) => {
            answer["dropped"] = json!(reason.word());
        }
        PutOutcome::Accepted | PutOutcome::Queued | PutOutcome::Sent => {}
    }
    answer
}

/// Run one verb against an already-opened [`Files`] handle.
pub(crate) fn answer(files: &Files, command: &Command) -> Result<Value, Value> {
    match command {
        Command::Drives => Ok(json!({
            "drives": files.drives().map_err(|e| files_failure(&e))?,
        })),
        Command::Ls { at } => {
            let listing = files.list(&at.target()?).map_err(|e| files_failure(&e))?;
            Ok(json!({"folder": listing.folder, "entries": listing.entries}))
        }
        Command::Get { at, out } => {
            let fetched = files
                .get(&at.target()?, &mut |_| {})
                .map_err(|e| files_failure(&e))?;
            if let Some(out) = out {
                std::fs::copy(&fetched.file, out)
                    .map_err(|e| failure("storage", &e.to_string()))?;
            }
            Ok(json!({
                "entry": fetched.entry,
                "file": fetched.file,
                "out": out,
                "fetched": fetched.fetched,
                "bytes_fetched": fetched.bytes_fetched,
                "grants": fetched.grants,
            }))
        }
        Command::Put { file, at, propose } => {
            if propose.propose {
                review::propose_put(files, file, &at.target()?, propose)
            } else {
                let report = files
                    .put(file, &at.target()?, &mut |_| {})
                    .map_err(|e| files_failure(&e))?;
                Ok(put_answer(&report))
            }
        }
        Command::Move {
            at,
            to_drive,
            to,
            propose,
        } => review::move_entry(
            files,
            &at.target()?,
            &Target::Path {
                drive: to_drive.clone(),
                path: to.clone(),
            },
            propose,
        ),
        Command::Delete { at, propose } => review::delete_entry(files, &at.target()?, propose),
        Command::Propose { file, at, propose } => {
            review::propose_put(files, file, &at.target()?, propose)
        }
        Command::Proposals { change_set, mine } => {
            review::proposals(files, change_set.as_deref(), *mine)
        }
        Command::ProposalCmd { id, open } => review::proposal(files, id, *open),
        Command::Decide {
            ids,
            change_set,
            approve,
            reject,
        } => review::decide(files, ids, change_set.as_deref(), *approve, *reject),
        Command::Unreviewed {
            after,
            limit,
            change_set,
        } => review::unreviewed(files, after, *limit, change_set.as_deref()),
        Command::Acknowledge { ids, change_set } => {
            review::acknowledge(files, ids, change_set.as_deref())
        }
        Command::PreviewReversal { event_id } => review::preview_reversal(files, event_id),
        Command::Reverse { event_id } => review::reverse(files, event_id),
        Command::History {
            after,
            limit,
            entity,
            actor,
            origin,
            change_set,
        } => review::history(
            files,
            after,
            *limit,
            entity.as_deref(),
            actor.as_deref(),
            origin.as_deref(),
            change_set.as_deref(),
        ),
    }
}

fn run(args: &Args) -> Value {
    let token = match std::env::var(&args.token_env) {
        Ok(token) if !token.is_empty() => token,
        _ => {
            return failure(
                "config",
                &format!(
                    "environment variable {} is not set or empty",
                    args.token_env
                ),
            )
        }
    };
    let source = match open(args, &token) {
        Ok(source) => source,
        Err(answer) => return answer,
    };
    let answer = (|| {
        let transfer = transfer(args, &token)?;
        let files = Files::new(
            args.url.trim_end_matches('/'),
            source.clone(),
            transfer,
            &Layout::under(&args.state),
        )
        .map_err(|e| files_failure(&e))?;
        settle(&source, &files, Duration::from_millis(args.wait_ms));
        let (flushed, flush_error) = match files.flush() {
            Ok(reports) => (reports.iter().map(put_answer).collect::<Vec<_>>(), None),
            Err(e) => (Vec::new(), Some(files_failure(&e))),
        };
        let at = json!(files.at());
        let mut answer = answer(&files, &args.command)?;
        answer["ok"] = json!(true);
        answer["at"] = at;
        answer["flushed"] = json!(flushed);
        if let Some(flush_error) = flush_error {
            answer["flush_error"] = flush_error;
        }
        Ok::<Value, Value>(answer)
    })()
    .unwrap_or_else(|answer| answer);
    source.replica.close();
    source.replica.join();
    answer
}

pub(crate) fn sentence(answer: &Value) -> String {
    if answer["ok"] != true {
        return format!(
            "{}: {}",
            answer["code"].as_str().unwrap_or("error"),
            answer["error"].as_str().unwrap_or_default()
        );
    }
    if let Some(drives) = answer["drives"].as_array() {
        return drives
            .iter()
            .map(|d| {
                format!(
                    "{}\t{}",
                    d["name"].as_str().unwrap_or_default(),
                    d["root_id"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(entries) = answer["entries"].as_array() {
        return entries
            .iter()
            .map(|e| {
                format!(
                    "{}\t{}\t{}\t{}\t{}",
                    e["id"].as_str().unwrap_or_default(),
                    e["kind"].as_str().unwrap_or_default(),
                    e["size"]
                        .as_u64()
                        .map_or_else(|| "-".to_owned(), |s| s.to_string()),
                    e["revision"].as_str().unwrap_or_default(),
                    e["name"].as_str().unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(proposals) = answer["proposals"].as_array() {
        return proposals
            .iter()
            .map(proposal_line)
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(revisions) = answer["revisions"].as_array() {
        if revisions.is_empty() {
            return format!(
                "{}: no revisions",
                answer["proposal_id"].as_str().unwrap_or_default()
            );
        }
        return revisions
            .iter()
            .map(proposal_line)
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(events) = answer["events"].as_array() {
        return events.iter().map(event_line).collect::<Vec<_>>().join("\n");
    }
    if let (Some(target), Some(status)) = (
        answer["target_event_id"].as_str(),
        answer["status"].as_str(),
    ) {
        let ops = answer["plan"].as_array().map_or(0, Vec::len);
        return format!("{target} ({status}, {ops} ops)");
    }
    if let Some(outcome) = answer["outcome"].as_str() {
        if let Some(proposal_id) = answer["proposal_id"].as_str() {
            return format!(
                "{outcome} {proposal_id}/{}",
                answer["proposal_revision"].as_str().unwrap_or_default()
            );
        }
        if let Some(items) = answer["items"].as_array() {
            return format!("{outcome}: {} items", items.len());
        }
        if let Some(targets) = answer["targets"].as_array() {
            return format!(
                "{outcome}: {} targets ({})",
                targets.len(),
                answer["event_id"].as_str().unwrap_or_default()
            );
        }
        if let Some(entry_id) = answer["entry_id"].as_str() {
            return format!("{outcome}: {entry_id} ({} bytes)", answer["size"]);
        }
        return format!(
            "{outcome}: {} (seq {})",
            answer["event_id"].as_str().unwrap_or_default(),
            answer["seq"].as_str().unwrap_or("-")
        );
    }
    format!(
        "{} ({})",
        answer["file"].as_str().unwrap_or_default(),
        if answer["fetched"] == true {
            "fetched"
        } else {
            "cached"
        }
    )
}

fn proposal_line(proposal: &Value) -> String {
    format!(
        "{}/{}\t{}\t{}",
        proposal["proposal_id"].as_str().unwrap_or_default(),
        proposal["revision"].as_str().unwrap_or_default(),
        proposal["status"].as_str().unwrap_or_default(),
        proposal["proposer_actor"].as_str().unwrap_or_default(),
    )
}

fn event_line(item: &Value) -> String {
    let event = &item["event"];
    format!(
        "{}\t{}\t{}\t{}\t{}",
        event["seq"].as_str().unwrap_or_default(),
        event["id"].as_str().unwrap_or_default(),
        event["descriptor"].as_str().unwrap_or_default(),
        event["actor_id"].as_str().unwrap_or_default(),
        event["origin_class"].as_str().unwrap_or_default(),
    )
}

/// Run one verb, print its answer through `printer` and return its exit
/// code.
pub fn main(args: Args, printer: &Printer) -> u8 {
    let answer = run(&args);
    printer.answer(&answer, sentence);
    exit_code(&answer)
}
