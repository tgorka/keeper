//! `drive_search` (story 95.4, AD-403): one call searches the drives in the
//! session's scope the way each drive's own OKF tools see it, and writes
//! nothing in them (D-21, R211: SQLite's own `-wal`/`-shm` beside the vault's
//! index aside).
//!
//! Per drive: its `.okf/config.yaml`; without one, its notes vault only; with
//! one keeper cannot read or interpret, nothing (R209). Inside the vault, the
//! vault's own index ranks first — FTS5, and the query's embedding when the
//! session's label lets the configured model see it and it answers within a
//! second. Nothing tells a reader the index holds every note as it is now,
//! so the vault is scanned as well, and an index whose vault the scan could
//! not finish is said to cover it only in part (R236). Elsewhere, each
//! bundle's `index.md` listing first, then a scan bounded by
//! `keeper_core::agents::search`'s caps, in bundle order. Nothing read once
//! is read again.
//!
//! Every read — the config, a listing, a bundle's folder, a ranked or listed
//! document, a scanned file — is admitted where it is asked for AND where it
//! lands ([`browse::resolve_known`]) before it is made, and made by
//! keeper-sync's no-follow reader on that landing alone
//! ([`ScanBudget::read`], [`bots_fs::search_walk`]): what a bundle excludes
//! is never opened, and keeper's own rules hold whatever the config says — a
//! session's `workspace/`, `.keeper/` and `.git/`, folded as keeper's fences
//! fold names, are never searched; an LFS pointer is never read as text,
//! never fetched, and is a result by its name alone. Every hit carries the
//! label of the bytes it was made from, taken at the read; the caller joins
//! them into the session's label as a drive read's.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keeper_core::agents::drive::DriveDecl;
use keeper_core::agents::label::{check_sink, Label, Sink, SinkVerdict};
use keeper_core::agents::search::{
    self as shape, Found, Hit, Query, Said, DRIVE_SEARCH, EMBED_MILLIS, LINES_PER_HIT,
    MAX_FILE_BYTES, SCAN_BYTES, SCAN_FILES, SCAN_MILLIS,
};
use keeper_core::bots::chat::{ToolCall as WireToolCall, ToolSpec};
use keeper_core::bots::store::ProviderRow;
use keeper_core::bots::tools::ToolOutcome;
use keeper_core::notes::search_index::{
    self, Meter, SearchIndex, SearchIndexError, SEARCH_DB_FILE,
};
use keeper_ported::okf::{self, Config, Doc, Link};
use keeper_sync::bots_fs::{self, Clock, ScanBudget, Scanned, SearchEntry, Step, Unread};
use keeper_sync::browse;
use serde_json::{json, Value};

use crate::turn::TurnEnv;

/// The most entries one call's walks look at, opened or not.
const WALK_ENTRIES: usize = 200_000;

/// Whether `name` is this module's tool.
pub fn serves(name: &str) -> bool {
    name == DRIVE_SEARCH
}

/// The tool's spec.
pub fn spec() -> ToolSpec {
    ToolSpec {
        name: DRIVE_SEARCH.to_owned(),
        description: format!(
            "Search the drives this session may read for LITERAL words — not a regular expression: through each drive's OKF bundles and its notes index, never what a bundle excludes. Returns at most {} results (k, default {}), each with its path, title, OKF type, up to {LINES_PER_HIT} matching lines and its label, and says what it did not search.",
            shape::MAX_K,
            shape::DEFAULT_K
        ),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Words to find; each must occur, case and accents aside."},
                "drives": {"type": "array", "items": {"type": "string"}, "description": "Which drives; all of this session's when left out."},
                "k": {"type": "integer", "minimum": 1, "maximum": shape::MAX_K}
            },
            "required": ["query"],
            "additionalProperties": false
        }),
    }
}

/// One drive a session may search.
#[derive(Debug, Clone)]
pub struct SearchDrive {
    pub id: String,
    pub decl: DriveDecl,
    /// The checkout's root on this host.
    pub root: PathBuf,
    /// The notes vault's folder, drive-relative.
    pub vault: Option<String>,
    /// The sessions zone's folder, drive-relative.
    pub sessions: Option<String>,
}

impl SearchDrive {
    /// The drive as the profile checked out on this host says it is.
    pub fn of(profile: &keeper_sync::SyncProfile, decl: DriveDecl) -> SearchDrive {
        let folder = |subfolder: &str| subfolder.trim().trim_matches('/').to_owned();
        SearchDrive {
            id: profile.id.clone(),
            decl,
            root: profile.local_path.clone(),
            vault: profile.notes.as_ref().map(|notes| folder(&notes.subfolder)),
            sessions: profile
                .sessions
                .as_ref()
                .map(|sessions| folder(&sessions.subfolder)),
        }
    }
}

/// Why a query's embedding could not be had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedFailure {
    Late,
    Failed,
}

/// Asks the configured embeddings model for one query's vector.
pub type AskEmbedding = dyn Fn(&str) -> Result<Vec<f32>, EmbedFailure> + Send + Sync;

/// The notes index's embeddings model on this host: its name, whether its
/// provider runs on the readers' own machines, and how to ask it.
pub struct Embeddings {
    pub model: String,
    pub local: bool,
    pub ask: Box<AskEmbedding>,
}

/// The configured embeddings model, asked through `env` as a turn asks
/// its own provider, within [`EMBED_MILLIS`]; `None` when none is
/// configured or its provider is gone.
pub fn configured_embeddings(env: &TurnEnv, data_dir: &Path) -> Option<Embeddings> {
    let model = keeper_core::registry::get_embedding_model(data_dir).ok()??;
    let row = keeper_core::bots::store::get_provider(data_dir, &model.provider).ok()??;
    Some(http_embeddings(env.clone(), row, model.model))
}

/// Ask `row`'s `/v1/embeddings` for `model`'s vector of a query.
pub fn http_embeddings(env: TurnEnv, row: ProviderRow, model: String) -> Embeddings {
    let local = keeper_core::agents::home::serves_local_models(row.provider.kind);
    let name = model.clone();
    let ask = move |query: &str| {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return Err(EmbedFailure::Failed);
        };
        let input = format!("{}{query}", keeper_core::bots::embed::query_prefix(&model));
        let asked = async {
            let endpoint = crate::turn::endpoint_of(&env, &row, None)
                .await
                .map_err(|_| EmbedFailure::Failed)?;
            let client = keeper_core::bots::http::client(keeper_core::bots::http::READ_TIMEOUT)
                .map_err(|_| EmbedFailure::Failed)?;
            keeper_core::bots::embed::embed(
                &client,
                &endpoint,
                keeper_core::bots::embed::EmbedRequest {
                    model: &model,
                    inputs: vec![input],
                },
            )
            .await
            .map_err(|_| EmbedFailure::Failed)?
            .into_iter()
            .next()
            .ok_or(EmbedFailure::Failed)
        };
        tokio::task::block_in_place(|| {
            handle.block_on(async {
                tokio::time::timeout(Duration::from_millis(EMBED_MILLIS), asked)
                    .await
                    .unwrap_or(Err(EmbedFailure::Late))
            })
        })
    };
    Embeddings {
        model: name,
        local,
        ask: Box::new(ask),
    }
}

/// One turn's `drive_search`.
pub struct SearchTools {
    /// The drives this host mounts with a declaration, by id.
    pub drives: Vec<SearchDrive>,
    /// The session's scope: the drives it may read.
    pub scope: Vec<String>,
    pub embeddings: Option<Embeddings>,
    /// What a call's [`SCAN_MILLIS`] are measured by.
    clock: Clock,
    /// What the last call read: each hit's label and `drive/path`.
    reads: Mutex<Vec<(Label, String)>>,
}

/// The arguments of one call.
struct Args {
    query: Query,
    drives: Option<Vec<String>>,
    k: usize,
}

/// One call's results and what it says beside them, as facts: what
/// [`shape::render`] renders.
#[derive(Debug)]
struct Answer {
    query: Query,
    hits: Vec<Hit>,
    said: Vec<Said>,
}

fn args_of(args: &Value) -> Result<Args, String> {
    let text = args["query"]
        .as_str()
        .ok_or("drive_search needs a \"query\" argument.")?;
    let query = Query::parse(text)
        .ok_or("drive_search needs a word to search for: a letter or a digit.")?;
    let drives = match &args["drives"] {
        Value::Null => None,
        Value::Array(items) => Some(
            items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or("\"drives\" is a list of drive names.")?,
        ),
        _ => return Err("\"drives\" is a list of drive names.".to_owned()),
    };
    Ok(Args {
        query,
        drives,
        k: shape::k_of(args["k"].as_u64()),
    })
}

/// Which of keeper's own rules keeps `rel` out, whatever the drive's config
/// says: `.git/` and `.keeper/` anywhere, a session's `workspace/` — every
/// name folded as keeper's workspace fence folds it (`files_write`), since
/// on a Mac's volume `.Keeper` is `.keeper`.
fn kept_out(drive: &SearchDrive, rel: &str) -> bool {
    let is = |part: &str, name: &str| part.eq_ignore_ascii_case(name);
    if rel
        .split('/')
        .any(|part| is(part, ".git") || is(part, ".keeper"))
    {
        return true;
    }
    match &drive.sessions {
        Some(sessions) if !sessions.is_empty() => {
            let mut parts = rel.split('/');
            sessions
                .split('/')
                .all(|zone| parts.next().is_some_and(|part| is(part, zone)))
                && parts.any(|part| is(part, "workspace"))
        }
        _ => false,
    }
}

fn under(folder: &str, rel: &str) -> bool {
    folder.is_empty() || rel == folder || rel.starts_with(&format!("{folder}/"))
}

fn is_markdown(rel: &str) -> bool {
    rel.rsplit('/')
        .next()
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".md"))
}

/// `parent` and `name`, `/`-joined.
fn join(parent: &str, name: &str) -> String {
    match (parent.is_empty(), name.is_empty()) {
        (true, _) => name.to_owned(),
        (_, true) => parent.to_owned(),
        _ => format!("{parent}/{name}"),
    }
}

/// `sub`, met in a walk of the folder `start` landed at (`landed`), as it
/// is named under `start`.
fn as_asked(start: &str, landed: &str, sub: &str) -> String {
    let rest = if landed.is_empty() {
        sub
    } else {
        sub.strip_prefix(landed)
            .map_or(sub, |rest| rest.trim_start_matches('/'))
    };
    join(start, rest)
}

/// How one drive is read for this call.
struct Plan<'d> {
    drive: &'d SearchDrive,
    /// `None`: the drive's notes only.
    config: Option<Config>,
}

impl Plan<'_> {
    /// Whether `rel`, a file, may be a result: Markdown, kept in by
    /// keeper's rules and by the drive's config (or in its vault without
    /// one), and not an `index.md` listing.
    fn admits(&self, rel: &str) -> bool {
        if !is_markdown(rel) || kept_out(self.drive, rel) {
            return false;
        }
        if rel.rsplit('/').next() == Some("index.md") {
            return false;
        }
        match &self.config {
            Some(config) => {
                !okf::is_excluded(config, rel) && okf::bundle_for(config, rel).is_some()
            }
            None => self
                .drive
                .vault
                .as_deref()
                .is_some_and(|vault| under(vault, rel)),
        }
    }

    /// Whether the folder `dir` may be walked: not one keeper's rules keep
    /// out, not one the config excludes whole.
    fn walks(&self, dir: &str) -> bool {
        !kept_out(self.drive, dir)
            && self
                .config
                .as_ref()
                .is_none_or(|config| dir.is_empty() || !okf::matcher::excludes_folder(config, dir))
    }

    /// Whether the listing `rel` may be read: not where keeper's rules keep
    /// out, and not excluded by the config (a guide never is).
    fn reads_listing(&self, rel: &str) -> bool {
        !kept_out(self.drive, rel)
            && self
                .config
                .as_ref()
                .is_some_and(|config| !okf::is_excluded(config, rel))
    }
}

/// One read of a search: the path it was asked for, where that landed,
/// and what the read of the landing came to.
struct Read {
    requested: String,
    landed: String,
    got: Result<Scanned, Unread>,
}

/// What a landing's one read came to, kept for every other name that
/// reaches it: a pointer's size, which such a name alone can make a
/// result; of anything else nothing more — a text was a result by its
/// content already, and a read that handed nothing over hands nothing
/// over again.
#[derive(Debug, Clone, Copy)]
enum Done {
    Pointer(u64),
    Read,
}

impl Done {
    fn of(got: &Result<Scanned, Unread>) -> Done {
        match got {
            Ok(Scanned::Pointer { size }) => Done::Pointer(*size),
            _ => Done::Read,
        }
    }
}

/// Where `rel` of `drive` lands, root-relative and `/`-joined
/// ([`browse::resolve_known`]): `None` only where the disk says nothing is
/// there under a checkout that is; a checkout that is gone, and anything
/// the disk cannot vouch for, is refused.
fn landing(drive: &SearchDrive, rel: &str) -> Result<Option<String>, browse::BrowseRefusal> {
    Ok(browse::resolve_known(&drive.root, rel)?
        .under_root()?
        .map(|landing| landing.relative()))
}

/// Read `rel` of `drive` by `read`, where `admit` admits it both as asked
/// for and where it lands — nothing is read otherwise, nor where nothing
/// is. A landing this drive's call read already (`done`) is not read
/// again: a pointer is evaluated under this name from that read, anything
/// else is no new read. A landing that cannot be established is no
/// absence: it is a read that handed nothing over ([`Unread::Skipped`]),
/// since what is there is unknown.
fn read_admitted(
    drive: &SearchDrive,
    rel: &str,
    admit: &dyn Fn(&str) -> bool,
    done: &mut HashMap<String, Done>,
    read: &mut dyn FnMut(&str) -> Result<Scanned, Unread>,
) -> Option<Read> {
    if !admit(rel) {
        return None;
    }
    let landed = match landing(drive, rel) {
        Ok(Some(landed)) => landed,
        Ok(None) => return None,
        Err(_) => {
            return Some(Read {
                requested: rel.to_owned(),
                landed: rel.to_owned(),
                got: Err(Unread::Skipped),
            })
        }
    };
    if landed != rel && !admit(&landed) {
        return None;
    }
    let got = match done.get(&landed) {
        Some(Done::Pointer(size)) => Ok(Scanned::Pointer { size: *size }),
        Some(Done::Read) => return None,
        None => {
            let got = read(&landed);
            done.insert(landed.clone(), Done::of(&got));
            got
        }
    };
    Some(Read {
        requested: rel.to_owned(),
        landed,
        got,
    })
}

/// The label of `text`, read at `requested` and landed at `landed`: the
/// stricter of the two places' (R195's rule for a drive read), taken from
/// the bytes read, never from a later look at the file.
fn label_of(decl: &DriveDecl, requested: &str, landed: &str, text: &str) -> Label {
    let asked = crate::agent::file_label(decl, requested, text);
    if requested == landed {
        asked
    } else {
        asked.join(&crate::agent::file_label(decl, landed, text))
    }
}

/// What a read text comes to as a result: its title and type as the
/// document states them, its lines holding a term, labelled by its bytes.
fn text_hit(drive: &SearchDrive, read: &Read, text: &str, query: &Query, found: Found) -> Hit {
    let doc = Doc::read(&read.landed, text);
    Hit {
        drive: drive.id.clone(),
        path: read.landed.clone(),
        title: doc.title(),
        kind: doc.kind().map(str::to_owned),
        lines: query.lines(text),
        absent: None,
        label: label_of(&drive.decl, &read.requested, &read.landed, text),
        found,
    }
}

/// What a pointer comes to as a result: one only by its name, with its
/// real size and none of its text.
fn pointer_hit(
    drive: &SearchDrive,
    read: &Read,
    size: u64,
    query: &Query,
    found: Found,
) -> Option<Hit> {
    query
        .found_in(&[&read.requested, &read.landed])
        .then(|| Hit {
            drive: drive.id.clone(),
            path: read.landed.clone(),
            title: Doc::read(&read.landed, "").title(),
            kind: None,
            lines: Vec::new(),
            absent: Some(size),
            label: label_of(&drive.decl, &read.requested, &read.landed, ""),
            found,
        })
}

/// What a scanned file comes to: a text whose path, title and body hold
/// every term; a pointer by its name.
fn scan_hit(drive: &SearchDrive, read: &Read, query: &Query) -> Option<Hit> {
    match &read.got {
        Ok(Scanned::Text(text)) => {
            let hit = text_hit(drive, read, text, query, Found::Scan);
            query
                .found_in(&[&read.requested, &read.landed, &hit.title, text])
                .then_some(hit)
        }
        Ok(Scanned::Pointer { size }) => pointer_hit(drive, read, *size, query, Found::Scan),
        Err(_) => None,
    }
}

/// The drive's OKF config: `None` where the disk says it has none, which
/// alone lets its notes stand for it; the reason where it has one keeper
/// cannot read or interpret, one that lands where keeper never reads, one
/// the call's bounds left cannot hold, or where whether it has one cannot
/// be established — a folder on the way that may not be searched, a link
/// to nothing ([`browse::resolve_known`]).
fn read_config(drive: &SearchDrive, budget: &mut ScanBudget) -> Result<Option<Config>, String> {
    const AT: &str = ".okf/config.yaml";
    let landed = match landing(drive, AT) {
        Ok(Some(landed)) => landed,
        Ok(None) => return Ok(None),
        Err(refusal) => return Err(refusal.to_string()),
    };
    if kept_out(drive, &landed) {
        return Err(format!(
            "it is a link into {landed}, which keeper never reads"
        ));
    }
    match budget.read(&drive.root, &landed) {
        Ok(Scanned::Text(text)) => okf::load_config(&text)
            .map(Some)
            .map_err(|error| error.to_string()),
        Ok(Scanned::Pointer { .. }) => Err("it is not on this device".to_owned()),
        Err(Unread::Capped) => Err(format!(
            "the search's {} ran out before it",
            shape::bounds_words()
        )),
        Err(Unread::Skipped) => Err(format!(
            "it is not a text file of at most {} keeper can open",
            shape::size_words(MAX_FILE_BYTES)
        )),
    }
}

/// The vault's own index, read-only, where it is exactly
/// `<vault>/.keeper/search.db` — a regular file whose content is on this
/// device, with no link between it and the vault's landing
/// ([`bots_fs::search_file`]): the one file of a `.keeper/` a search opens
/// (R209). It reads within the call's time: a lock is waited on no longer,
/// and a statement still running when the time is up is interrupted. Its
/// open is admitted as every read is ([`ScanBudget::admit_open`]): once the
/// call's files, bytes or time are spent, SQLite never opens it.
/// `Ok(None)` where the drive has no vault or the vault no index; what is
/// said where the file is there and is not that, or was not opened.
fn open_index(drive: &SearchDrive, budget: &mut ScanBudget) -> Result<Option<SearchIndex>, Said> {
    let unusable = |why: String| Said::IndexUnusable {
        drive: drive.id.clone(),
        why,
    };
    let Some(vault) = drive.vault.as_deref() else {
        return Ok(None);
    };
    let vault_landed = match landing(drive, vault) {
        Ok(Some(landed)) => landed,
        Ok(None) => return Ok(None),
        Err(refusal) => return Err(unusable(refusal.to_string())),
    };
    if kept_out(drive, vault) || kept_out(drive, &vault_landed) {
        return Err(unusable(format!(
            "the vault lands in {vault_landed}, which keeper never reads"
        )));
    }
    let db = join(&join(&vault_landed, ".keeper"), SEARCH_DB_FILE);
    match landing(drive, &db) {
        Ok(None) => Ok(None),
        Ok(Some(landed)) if landed == db => {
            let path = bots_fs::search_file(&drive.root, &landed).map_err(|_| {
                unusable(format!(
                    "{db} is not a file whose content is on this device"
                ))
            })?;
            if !budget.admit_open() {
                return Err(Said::IndexCapped {
                    drive: drive.id.clone(),
                });
            }
            // Read-only, and never where none exists: an index is the
            // desktop's to build, never this search's (D-21).
            SearchIndex::open_bounded(&path, budget.time_left(), budget.expiry())
                .map(Some)
                .map_err(|error| unusable(error.to_string()))
        }
        Ok(Some(landed)) => Err(unusable(format!("{db} is a link to {landed}"))),
        Err(refusal) => Err(unusable(refusal.to_string())),
    }
}

/// One drive's search under way: its plan, what it found and skipped.
struct Searching<'s, 'd> {
    plan: Plan<'d>,
    query: &'s Query,
    budget: &'s mut ScanBudget,
    hits: Vec<Hit>,
    /// The landings already a hit.
    seen: HashSet<String>,
    /// The landings already read, by the index's ranking, a listing or the
    /// scan, and what each read came to: none is read twice.
    done: HashMap<String, Done>,
    skipped: usize,
}

impl Searching<'_, '_> {
    fn keep(&mut self, hit: Option<Hit>, got: &Result<Scanned, Unread>) {
        if got == &Err(Unread::Skipped) {
            self.skipped += 1;
        }
        if let Some(hit) = hit {
            if self.seen.insert(hit.path.clone()) {
                self.hits.push(hit);
            }
        }
    }

    /// The vault index's ranked notes, each read as it is shown: a text
    /// with its own title, type and lines, a pointer by its name alone.
    /// Whether the call's bounds stopped the reading before every note —
    /// asked before each note, before its path is resolved, so notes that
    /// are gone or kept out stop at the bounds as read ones do.
    fn index_hits(&mut self, paths: &[String], k: usize) -> bool {
        let Some(vault) = self.plan.drive.vault.as_deref() else {
            return false;
        };
        let drive = self.plan.drive;
        for path in paths {
            if self.hits.len() >= k {
                return false;
            }
            if self.budget.exhausted() {
                return true;
            }
            let Searching {
                plan, budget, done, ..
            } = self;
            let Some(read) = read_admitted(
                drive,
                &join(vault, path),
                &|rel| plan.admits(rel),
                done,
                &mut |landed| budget.read(&drive.root, landed),
            ) else {
                continue;
            };
            let hit = match &read.got {
                Ok(Scanned::Text(text)) => {
                    Some(text_hit(drive, &read, text, self.query, Found::Index))
                }
                Ok(Scanned::Pointer { size }) => {
                    pointer_hit(drive, &read, *size, self.query, Found::Index)
                }
                Err(Unread::Capped) => return true,
                Err(Unread::Skipped) => None,
            };
            self.keep(hit, &read.got);
        }
        false
    }

    /// Each bundle's `index.md` lines whose title or description holds every
    /// term, as hits on the documents they link: each read as a ranked note
    /// is, under the listing's title, labelled by the listing and by the
    /// document. Whether the call's bounds stopped the reading before every
    /// listing and listed document — asked before each listing and after
    /// each line of it is read, whatever the line is: a heading, another
    /// section's, one that lists nothing.
    fn listing_hits(&mut self) -> bool {
        let drive = self.plan.drive;
        let Some(config) = self.plan.config.as_ref() else {
            return false;
        };
        let dirs: Vec<String> = config
            .bundles
            .iter()
            .filter(|bundle| bundle.index)
            .map(|bundle| {
                if bundle.is_root() {
                    String::new()
                } else {
                    bundle.path.clone()
                }
            })
            .collect();
        for dir in dirs {
            if self.budget.exhausted() {
                return true;
            }
            let Searching {
                plan, budget, done, ..
            } = self;
            let Some(listing) = read_admitted(
                drive,
                &join(&dir, "index.md"),
                &|rel| plan.reads_listing(rel),
                done,
                &mut |landed| budget.read(&drive.root, landed),
            ) else {
                continue;
            };
            let text = match &listing.got {
                Ok(Scanned::Text(text)) => text,
                Err(Unread::Capped) => return true,
                _ => {
                    self.keep(None, &listing.got);
                    continue;
                }
            };
            let listed = label_of(&drive.decl, &listing.requested, &listing.landed, text);
            for entry in okf::index::lines(text) {
                if self.budget.exhausted() {
                    return true;
                }
                let Some(entry) = entry.filter(|entry| entry.section == "Documents") else {
                    continue;
                };
                if !self.query.found_in(&[&entry.title, &entry.description]) {
                    continue;
                }
                let Link::Path(rel) = okf::resolve(&dir, &entry.link) else {
                    continue;
                };
                if rel.starts_with("..") || self.seen.contains(&rel) {
                    continue;
                }
                let Searching {
                    plan, budget, done, ..
                } = self;
                let Some(read) =
                    read_admitted(drive, &rel, &|rel| plan.admits(rel), done, &mut |landed| {
                        budget.read(&drive.root, landed)
                    })
                else {
                    continue;
                };
                let hit = match &read.got {
                    Ok(Scanned::Text(text)) => Some(Hit {
                        title: entry.title.clone(),
                        ..text_hit(drive, &read, text, self.query, Found::Listing)
                    }),
                    Ok(Scanned::Pointer { size }) => {
                        pointer_hit(drive, &read, *size, self.query, Found::Listing)
                    }
                    Err(Unread::Capped) => return true,
                    Err(Unread::Skipped) => None,
                };
                let hit = hit.map(|hit| Hit {
                    label: hit.label.join(&listed),
                    ..hit
                });
                self.keep(hit, &read.got);
            }
        }
        false
    }

    /// The bounded scan from `starts`, in order, leaving what was read
    /// already (a pointer met again under another name is evaluated from
    /// that read, where the walk meets it — in walk order, and only while
    /// the walk's time lasts, as every entry is offered); what it says of
    /// its bounds and of folders it could not read goes to `said`. No start
    /// is resolved once the time or the walk's entries are spent. Whether it
    /// stopped before it had met and read everything it would have.
    fn scan(&mut self, starts: &[String], said: &mut Vec<Said>) -> bool {
        let drive = self.plan.drive;
        let (mut candidates, mut opened, mut walk_capped, mut unlisted) = (0, 0, false, 0);
        for start in starts {
            // What the starts not walked hold is not counted.
            if self.budget.walk_spent() {
                walk_capped = true;
                break;
            }
            let landed = match landing(drive, start) {
                Ok(Some(landed)) => landed,
                Ok(None) => continue,
                // A bundle whose landing cannot be asked about is not an
                // empty one: what it holds is unknown.
                Err(_) => {
                    unlisted += usize::from(self.plan.walks(start));
                    continue;
                }
            };
            let Searching {
                plan,
                query,
                budget,
                hits,
                seen,
                done,
                skipped,
            } = self;
            // The walk offers each entry and hands over each read file
            // through two callbacks; both keep their hits here, in the order
            // the walk meets them.
            let kept = RefCell::new((hits, seen));
            let keep = |read: &Read| {
                if let Some(hit) = scan_hit(drive, read, query) {
                    let (hits, seen) = &mut *kept.borrow_mut();
                    if seen.insert(hit.path.clone()) {
                        hits.push(hit);
                    }
                }
            };
            let mut pointers = Vec::new();
            let mut visit = |entry: &SearchEntry| {
                let asked = as_asked(start, &landed, &entry.subpath);
                let both = [asked.as_str(), entry.subpath.as_str()];
                let enter = if entry.is_dir {
                    // A nested bundle is walked as its own.
                    let nested = asked != *start
                        && plan
                            .config
                            .as_ref()
                            .is_some_and(|config| config.bundles.iter().any(|b| b.path == asked));
                    !nested && both.iter().all(|dir| plan.walks(dir))
                } else if both.iter().all(|rel| plan.admits(rel)) {
                    match done.get(&entry.subpath) {
                        None => {
                            done.insert(entry.subpath.clone(), Done::Read);
                            true
                        }
                        // The walk asked the time before offering it: a
                        // pointer read already is evaluated here, never
                        // after the walk stops.
                        Some(Done::Pointer(size)) => {
                            keep(&Read {
                                requested: asked,
                                landed: entry.subpath.clone(),
                                got: Ok(Scanned::Pointer { size: *size }),
                            });
                            false
                        }
                        Some(Done::Read) => false,
                    }
                } else {
                    false
                };
                if enter {
                    Step::Enter
                } else {
                    Step::Skip
                }
            };
            let mut found = |sub: &str, file: Scanned| {
                if let Scanned::Pointer { size } = file {
                    pointers.push((sub.to_owned(), size));
                }
                keep(&Read {
                    requested: as_asked(start, &landed, sub),
                    landed: sub.to_owned(),
                    got: Ok(file),
                });
            };
            match bots_fs::search_walk(&drive.root, &landed, budget, &mut visit, &mut found) {
                Ok(walked) => {
                    candidates += walked.candidates;
                    opened += walked.searched + walked.skipped;
                    *skipped += walked.skipped;
                    walk_capped |= walked.walk_capped;
                    unlisted += walked.unreadable;
                }
                Err(_) => unlisted += 1,
            }
            for (sub, size) in pointers {
                done.insert(sub, Done::Pointer(size));
            }
        }
        let capped = self.budget.capped || walk_capped;
        if capped {
            // A folder not read whole may hold any number of files more.
            said.push(Said::ScanCapped {
                drive: drive.id.clone(),
                searched: opened,
                of: (!walk_capped && unlisted == 0).then_some(candidates),
            });
        }
        if unlisted > 0 {
            said.push(Said::Unlisted {
                drive: drive.id.clone(),
                folders: unlisted,
            });
        }
        capped || unlisted > 0
    }
}

impl SearchTools {
    pub fn new(
        drives: Vec<SearchDrive>,
        scope: Vec<String>,
        embeddings: Option<Embeddings>,
    ) -> Self {
        SearchTools {
            drives,
            scope,
            embeddings,
            clock: Arc::new(Instant::now),
            reads: Mutex::new(Vec::new()),
        }
    }

    /// The same tools, a call's time measured by `clock`.
    pub fn with_clock(self, clock: Clock) -> Self {
        SearchTools { clock, ..self }
    }

    /// What the last call read, once: each hit's label and `drive/path`.
    pub fn take_reads(&self) -> Vec<(Label, String)> {
        std::mem::take(&mut *self.reads.lock().unwrap_or_else(|p| p.into_inner()))
    }

    /// Where an audit row says a call went: the drives it names.
    pub fn at(&self, wire: &WireToolCall) -> String {
        args_of(wire.arguments.as_ref().unwrap_or(&Value::Null))
            .ok()
            .and_then(|args| args.drives)
            .unwrap_or_else(|| self.scope.clone())
            .join(",")
    }

    /// Run one call in a session labelled `label`; `may_read` is the
    /// grant's answer for a read of a drive's root.
    pub fn run(
        &self,
        wire: &WireToolCall,
        label: &Label,
        may_read: &dyn Fn(&str) -> Result<(), String>,
    ) -> ToolOutcome {
        match self.answer(wire, label, may_read) {
            Ok(answer) => ToolOutcome::Text {
                body: shape::render(&answer.query, &answer.hits, &answer.said),
                truncated_at: None,
                of_bytes: None,
                okf: None,
            },
            Err(reason) => ToolOutcome::Refused { reason },
        }
    }

    /// One call's results and what it says beside them, before they are
    /// rendered; the reason where the call is refused.
    fn answer(
        &self,
        wire: &WireToolCall,
        label: &Label,
        may_read: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<Answer, String> {
        let args = args_of(wire.arguments.as_ref().unwrap_or(&Value::Null))?;
        let wanted = args.drives.clone().unwrap_or_else(|| self.scope.clone());
        let mut drives = Vec::new();
        for id in &wanted {
            let drive = self
                .drives
                .iter()
                .find(|drive| drive.id == *id)
                .filter(|_| self.scope.contains(id));
            match drive {
                Some(drive) if may_read(id).is_ok() => drives.push(drive),
                _ => return Err(Said::OutOfScope { drive: id.clone() }.sentence()),
            }
        }
        let mut said = Vec::new();
        let mut budget = ScanBudget {
            max_files: SCAN_FILES,
            max_bytes: SCAN_BYTES,
            max_file_bytes: MAX_FILE_BYTES,
            deadline: (self.clock)() + Duration::from_millis(SCAN_MILLIS),
            clock: Arc::clone(&self.clock),
            max_entries: WALK_ENTRIES,
            opened: 0,
            bytes: 0,
            entries: 0,
            capped: false,
        };
        let mut vector: Option<Option<Vec<f32>>> = None;
        let per_drive: Vec<Vec<Hit>> = drives
            .into_iter()
            .map(|drive| {
                self.search_drive(drive, &args, label, &mut budget, &mut vector, &mut said)
            })
            .collect();
        let hits = shape::merge(per_drive, args.k);
        *self.reads.lock().unwrap_or_else(|p| p.into_inner()) = hits
            .iter()
            .map(|hit| (hit.label.clone(), format!("{}/{}", hit.drive, hit.path)))
            .collect();
        Ok(Answer {
            query: args.query,
            hits,
            said,
        })
    }

    /// One drive's hits, in its order: the vault index's, the listings',
    /// the scan's.
    fn search_drive(
        &self,
        drive: &SearchDrive,
        args: &Args,
        label: &Label,
        budget: &mut ScanBudget,
        vector: &mut Option<Option<Vec<f32>>>,
        said: &mut Vec<Said>,
    ) -> Vec<Hit> {
        let config = match read_config(drive, budget) {
            Ok(config) => config,
            // What the config excludes is unknown: nothing is searched.
            Err(why) => {
                said.push(Said::UnreadableOkf {
                    drive: drive.id.clone(),
                    why,
                });
                return Vec::new();
            }
        };
        if config.is_none() {
            said.push(Said::NoOkf {
                drive: drive.id.clone(),
            });
        }
        let ranked = match open_index(drive, budget) {
            Ok(Some(index)) => self
                .rank(&index, args, label, vector, said, budget)
                .map(Some)
                .map_err(|why| Said::IndexUnusable {
                    drive: drive.id.clone(),
                    why,
                }),
            Ok(None) => {
                if drive.vault.is_some() {
                    said.push(Said::NoIndex {
                        drive: drive.id.clone(),
                    });
                }
                Ok(None)
            }
            Err(not_read) => Err(not_read),
        };
        let ranked = ranked.unwrap_or_else(|not_read| {
            said.push(not_read);
            None
        });
        let starts: Vec<String> = match &config {
            Some(config) => config
                .bundles
                .iter()
                .map(|bundle| {
                    if bundle.is_root() {
                        String::new()
                    } else {
                        bundle.path.clone()
                    }
                })
                .collect(),
            None => drive.vault.iter().cloned().collect(),
        };
        let mut searching = Searching {
            plan: Plan { drive, config },
            query: &args.query,
            budget,
            hits: Vec::new(),
            seen: HashSet::new(),
            done: HashMap::new(),
            skipped: 0,
        };
        let mut incomplete = false;
        if let Some(paths) = &ranked {
            incomplete = searching.index_hits(paths, args.k);
        }
        incomplete |= searching.listing_hits();
        if incomplete {
            said.push(Said::Incomplete {
                drive: drive.id.clone(),
            });
        }
        // The index may predate a note or miss one: the scan reads the
        // vault as well, and where it could not finish, the index's answer
        // covers the vault only in part.
        let stopped = searching.scan(&starts, said);
        if ranked.is_some() && (incomplete || stopped) {
            said.push(Said::IndexPartial {
                drive: drive.id.clone(),
            });
        }
        if searching.skipped > 0 {
            said.push(Said::Skipped {
                drive: drive.id.clone(),
                files: searching.skipped,
            });
        }
        searching.hits
    }

    /// The vault index's ranking — lexical, fused with the query's vector
    /// when one may be had (asked once per call) — as vault-relative paths;
    /// the reason where the index cannot answer, which is not an empty
    /// answer. Every row it reads counts against the call's bytes, and a
    /// ranking by meaning the call's time or bytes stop is said and left
    /// out.
    fn rank(
        &self,
        index: &SearchIndex,
        args: &Args,
        label: &Label,
        vector: &mut Option<Option<Vec<f32>>>,
        said: &mut Vec<Said>,
        budget: &mut ScanBudget,
    ) -> Result<Vec<String>, String> {
        let mut meter = Meter::new(budget.bytes_left());
        let ranked = self.rank_within(index, args, label, vector, said, &mut meter);
        budget.charge(meter.taken);
        ranked.map_err(|error| error.to_string())
    }

    fn rank_within(
        &self,
        index: &SearchIndex,
        args: &Args,
        label: &Label,
        vector: &mut Option<Option<Vec<f32>>>,
        said: &mut Vec<Said>,
        meter: &mut Meter,
    ) -> Result<Vec<String>, SearchIndexError> {
        let text = args.query.index_text();
        let lexical = index.query_within(&text, search_index::LEXICAL_POOL, meter)?;
        let wanted = vector.get_or_insert_with(|| self.embed(args, label, said));
        let scores = match (wanted, &self.embeddings) {
            (Some(query), Some(embeddings)) => match index.cosine_top_k_within(
                &embeddings.model,
                query,
                search_index::VECTOR_POOL,
                meter,
            ) {
                Ok(meaning) => search_index::fuse(&lexical, &meaning),
                Err(SearchIndexError::Bounds) => {
                    said.push(Said::MeaningCapped);
                    search_index::lexical_only(&lexical)
                }
                Err(_) => search_index::lexical_only(&lexical),
            },
            _ => search_index::lexical_only(&lexical),
        };
        let ids: Vec<&str> = scores.iter().map(|score| score.note_id.as_str()).collect();
        let paths = index.paths_of(&ids, meter)?;
        Ok(scores
            .iter()
            .filter_map(|score| paths.get(&score.note_id).cloned())
            .collect())
    }

    /// The query's vector, when an embeddings model is configured, the
    /// session's label may reach it (a `local_only` label, a remote model:
    /// never asked) and it answers in time.
    fn embed(&self, args: &Args, label: &Label, said: &mut Vec<Said>) -> Option<Vec<f32>> {
        let embeddings = self.embeddings.as_ref()?;
        if let SinkVerdict::Block { .. } = check_sink(
            label,
            &Sink::Model {
                local: embeddings.local,
            },
        ) {
            said.push(Said::StaysLocal);
            return None;
        }
        match (embeddings.ask)(&args.query.index_text()) {
            Ok(vector) => Some(vector),
            Err(EmbedFailure::Late) => {
                said.push(Said::EmbedLate);
                None
            }
            Err(EmbedFailure::Failed) => {
                said.push(Said::EmbedFailed);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::io::{BufRead, BufReader, Read as _, Write as _};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use keeper_core::agents::label::{Integrity, Readers};
    use keeper_core::error::CoreError;
    use keeper_core::notes::search_index::NoteDoc;
    use keeper_core::platform::Platform;

    use super::*;

    const TGORKA: &str = "@tgorka:example.org";
    const MARTA: &str = "@marta:example.org";

    /// The drive's own shape, as its config writes it: a root bundle,
    /// nested zone bundles, the drop zones excluded with their guides
    /// kept, and the inert session-workspace line (Q9).
    const CONFIG: &str = "okf_version: \"0.2\"\nbundles:\n  - path: \".\"\n    name: tgdrive\n  - path: 10-notes\n    name: tgdrive-notes\n  - path: 30-work\n    name: tgdrive-work\n  - path: 60-sessions\n    name: tgdrive-sessions\nexclude:\n  - 00-inbox/**\n  - 99-temp/**\n  - 60-sessions/**/workspace/**\n  - recordings/**\n  - \"*.sync-conflict-*.md\"\n  - 10-notes/.keeper/**\n  - 30-work/clients/**\nguides:\n  - 00-inbox/README.md\n";

    fn decl(id: &str, readers: &[&str]) -> DriveDecl {
        let readers: Vec<String> = readers.iter().map(|r| format!("\"{r}\"")).collect();
        keeper_core::agents::drive::parse(&format!(
            "version = 1\nid = \"{id}\"\ntitle = \"{id}\"\nprincipal = \"tgorka\"\nowner = \"{TGORKA}\"\nreaders = [{}]\n",
            readers.join(", ")
        ))
        .expect("a declaration")
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }

    struct Fixture {
        dir: tempfile::TempDir,
        id: &'static str,
        readers: Vec<&'static str>,
    }

    impl Fixture {
        /// A drive `id` read by `readers`, with the drive's OKF config when
        /// `okf`, a notes vault at `10-notes` and sessions at `60-sessions`.
        fn new(id: &'static str, readers: &[&'static str], okf: bool) -> Fixture {
            let dir = tempfile::tempdir().expect("drive");
            if okf {
                write(dir.path(), ".okf/config.yaml", CONFIG);
            }
            Fixture {
                dir,
                id,
                readers: readers.to_vec(),
            }
        }

        fn root(&self) -> &Path {
            self.dir.path()
        }

        fn write(&self, rel: &str, text: &str) {
            write(self.root(), rel, text);
        }

        fn drive(&self) -> SearchDrive {
            SearchDrive {
                id: self.id.to_owned(),
                decl: decl(self.id, &self.readers),
                root: self.root().to_owned(),
                vault: Some("10-notes".to_owned()),
                sessions: Some("60-sessions".to_owned()),
            }
        }
    }

    fn tools(drives: &[&Fixture], embeddings: Option<Embeddings>) -> SearchTools {
        SearchTools::new(
            drives.iter().map(|fixture| fixture.drive()).collect(),
            drives.iter().map(|fixture| fixture.id.to_owned()).collect(),
            embeddings,
        )
    }

    fn label(readers: &[&str], local_only: bool) -> Label {
        Label {
            readers: Readers::Only(
                readers
                    .iter()
                    .map(|r| matrix_sdk::ruma::OwnedUserId::try_from(*r).expect("user"))
                    .collect::<BTreeSet<_>>(),
            ),
            integrity: Integrity::Owner,
            local_only,
        }
    }

    fn wire(args: Value) -> WireToolCall {
        WireToolCall {
            id: "s1".to_owned(),
            name: DRIVE_SEARCH.to_owned(),
            arguments_raw: args.to_string(),
            arguments: Some(args),
        }
    }

    fn call(tools: &SearchTools, args: Value, label: &Label) -> ToolOutcome {
        tools.run(&wire(args), label, &|_| Ok(()))
    }

    /// One call's facts, in a session labelled `label`.
    fn answer(tools: &SearchTools, args: Value, label: &Label) -> Answer {
        tools
            .answer(&wire(args), label, &|_| Ok(()))
            .expect("an answer")
    }

    fn search(tools: &SearchTools, args: Value) -> Answer {
        answer(tools, args, &label(&[TGORKA, MARTA], false))
    }

    /// The `drive/path` of every result, in order.
    fn found(answer: &Answer) -> Vec<String> {
        answer
            .hits
            .iter()
            .map(|hit| format!("{}/{}", hit.drive, hit.path))
            .collect()
    }

    /// The result at `drive/path`.
    fn hit<'a>(answer: &'a Answer, at: &str) -> &'a Hit {
        answer
            .hits
            .iter()
            .find(|hit| format!("{}/{}", hit.drive, hit.path) == at)
            .unwrap_or_else(|| panic!("{at}: {answer:?}"))
    }

    /// Whether the call said a fact `is` names.
    fn says(answer: &Answer, is: impl Fn(&Said) -> bool) -> bool {
        answer.said.iter().any(is)
    }

    /// Whether the call said `drive`'s fact `is` names.
    fn says_of(answer: &Answer, drive: &str, is: impl Fn(&Said) -> Option<&String>) -> bool {
        says(answer, |said| is(said).is_some_and(|of| of == drive))
    }

    fn unreadable_okf(said: &Said) -> Option<&String> {
        match said {
            Said::UnreadableOkf { drive, .. } => Some(drive),
            _ => None,
        }
    }

    fn incomplete(said: &Said) -> Option<&String> {
        match said {
            Said::Incomplete { drive } => Some(drive),
            _ => None,
        }
    }

    fn scan_capped(said: &Said) -> Option<&String> {
        match said {
            Said::ScanCapped { drive, .. } => Some(drive),
            _ => None,
        }
    }

    fn index_unusable(said: &Said) -> Option<&String> {
        match said {
            Said::IndexUnusable { drive, .. } => Some(drive),
            _ => None,
        }
    }

    fn skipped(said: &Said) -> Option<&String> {
        match said {
            Said::Skipped { drive, .. } => Some(drive),
            _ => None,
        }
    }

    /// Acceptance 2: files the drive's config excludes are never opened —
    /// each is unreadable, so an open would show as a file not searched —
    /// and the search answers with none of them and no error.
    #[test]
    fn excluded_files_are_never_opened() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        for rel in [
            "00-inbox/secret.md",
            "99-temp/x.md",
            "recordings/x.md",
            "30-work/clients/acme/x.md",
            "30-work/projects/z.sync-conflict-1.md",
        ] {
            drive.write(rel, "zebrafish\n");
            std::fs::set_permissions(
                drive.root().join(rel),
                std::fs::Permissions::from_mode(0o000),
            )
            .expect("mode 000");
        }
        let got = search(&tools(&[&drive], None), json!({"query": "zebrafish"}));
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(
            !says(&got, |said| skipped(said).is_some()),
            "an excluded file was opened: {got:?}"
        );
        drive.write("30-work/projects/z.md", "zebrafish\n");
        let got = search(&tools(&[&drive], None), json!({"query": "zebrafish"}));
        assert_eq!(found(&got), ["tgdrive/30-work/projects/z.md"], "{got:?}");
    }

    /// Acceptance 3: a session's `workspace/` is never searched though the
    /// drive's own line for it is inert, nor `.keeper/` or `.git/`
    /// anywhere; the session's own README is.
    #[test]
    fn session_workspaces_are_never_searched() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        for rel in [
            "60-sessions/active/2026-10-06-s/workspace/a.md",
            "60-sessions/active/2026-10-06-s/workspace/deep/b.md",
            "60-sessions/active/2026-10-06-s/.keeper/c.md",
            "30-work/.keeper/d.md",
            ".git/e.md",
            "30-work/.git/f.md",
        ] {
            drive.write(rel, "walrus\n");
        }
        drive.write("60-sessions/active/2026-10-06-s/README.md", "walrus\n");
        let got = search(&tools(&[&drive], None), json!({"query": "walrus"}));
        assert_eq!(
            found(&got),
            ["tgdrive/60-sessions/active/2026-10-06-s/README.md"],
            "{got:?}"
        );
    }

    /// A `commit_paths` under way (a night's or a curator's) holds its own
    /// names beside the files it writes: the new file staged as
    /// `.keeper.<request>-<n>.tmp`, the old one moved aside as
    /// `.keeper-displaced-<request>-<n>`, one taken back as
    /// `.keeper-taken-<request>-<n>`. None of them is ever read as a
    /// document candidate, whether the scan meets it or a listing links
    /// it — a search's document candidates are the folder's `*.md` files
    /// (R281, R282).
    #[test]
    fn a_commits_transient_files_are_never_searched() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        for name in [
            ".keeper.n1-0.tmp",
            ".keeper-displaced-n1-0",
            ".keeper-taken-n1-1",
        ] {
            drive.write(&format!("30-work/{name}"), "walrus\n");
        }
        drive.write("30-work/a.md", "walrus\n");
        drive.write(
            "30-work/index.md",
            "---\nokf_bundle_name: tgdrive-work\n---\n# Work\n\n## Documents\n\n* [Walrus](.keeper-displaced-n1-0) - walrus\n",
        );
        let got = search(&tools(&[&drive], None), json!({"query": "walrus"}));
        assert_eq!(found(&got), ["tgdrive/30-work/a.md"], "{got:?}");
    }

    fn pointer(size: u64) -> String {
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {size}\n",
            "ab".repeat(32)
        )
    }

    /// Acceptance 4: an LFS pointer is never read as text — its pointer
    /// text holding the query is no hit — and one whose name matches is
    /// said to be not on this device, with its real size and none of its
    /// text. Nothing is fetched: the search has no endpoint to fetch from.
    #[test]
    fn pointers_are_never_read_as_text() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        drive.write("30-work/big.md", &pointer(12_345));
        drive.write("30-work/zebra-video.md", &pointer(13_000));
        let got = search(&tools(&[&drive], None), json!({"query": "git-lfs"}));
        assert!(found(&got).is_empty(), "{got:?}");
        let got = search(&tools(&[&drive], None), json!({"query": "zebra"}));
        assert_eq!(found(&got), ["tgdrive/30-work/zebra-video.md"], "{got:?}");
        let video = hit(&got, "tgdrive/30-work/zebra-video.md");
        assert_eq!(video.absent, Some(13_000));
        assert!(video.lines.is_empty(), "{video:?}");
        let ToolOutcome::Text { body, .. } = call(
            &tools(&[&drive], None),
            json!({"query": "zebra"}),
            &label(&[TGORKA], false),
        ) else {
            panic!("a result");
        };
        assert!(
            !body.contains("git-lfs") && !body.contains("sha256"),
            "{body}"
        );
    }

    /// Acceptance 7: the scan matches a query's characters, never a
    /// pattern.
    #[test]
    fn a_regex_query_is_matched_literally() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        drive.write("30-work/literal.md", "see a.*b here\n");
        drive.write("30-work/pattern.md", "axxb and a b\n");
        let got = search(&tools(&[&drive], None), json!({"query": "a.*b"}));
        assert_eq!(found(&got), ["tgdrive/30-work/literal.md"], "{got:?}");
        assert_eq!(got.hits[0].lines, [(1, "see a.*b here".to_owned())]);
    }

    /// A clock that stands still: no bound but the file count and the
    /// bytes can stop a scan, whatever the host's speed.
    fn frozen() -> Clock {
        let now = Instant::now();
        Arc::new(move || now)
    }

    /// Acceptance 6: `k` is 10 unless asked and 25 at most; a long line is
    /// cut at 240 characters; a scan that reaches its file cap says how many
    /// of how many files it searched — 1 999, the config being the call's
    /// first of 2 000 (R95S-10: under a clock that stands still, so the
    /// count is the cap's and not the host's speed); one whose time runs
    /// out stops walking and says it did not count them all.
    #[test]
    fn drive_search_is_bounded_and_says_so() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        let long = format!("needle {}", "x".repeat(400));
        for n in 0..5_312 {
            drive.write(&format!("30-work/projects/{n:05}.md"), &format!("{long}\n"));
        }
        let tools = tools(&[&drive], None).with_clock(frozen());
        let got = search(&tools, json!({"query": "needle"}));
        assert_eq!(found(&got).len(), 10);
        let capped = Said::ScanCapped {
            drive: "tgdrive".to_owned(),
            searched: 1_999,
            of: Some(5_312),
        };
        assert!(got.said.contains(&capped), "{:?}", got.said);
        let (line, cut) = &got.hits[0].lines[0];
        assert_eq!(*line, 1);
        assert_eq!(cut.chars().count(), 241);
        assert!(cut.ends_with('…'));
        // The rendered result carries every fact said beside its hits.
        let ToolOutcome::Text { body, .. } = call(
            &tools,
            json!({"query": "needle"}),
            &label(&[TGORKA, MARTA], false),
        ) else {
            panic!("a result");
        };
        for said in &got.said {
            assert!(body.contains(&said.sentence()), "{said:?}");
        }
        let got = search(&tools, json!({"query": "needle", "k": 100}));
        assert_eq!(found(&got).len(), 25);

        // Every look at the clock is a millisecond later: the 1.5 s run out
        // while the 5 312-file folder is still being read.
        let slow = tools_with_clock(&drive, ticking());
        let got = search(&slow, json!({"query": "needle"}));
        assert!(
            says(&got, |said| matches!(
                said,
                Said::ScanCapped { drive, of: None, .. } if drive == "tgdrive"
            )),
            "{:?}",
            got.said
        );
    }

    fn tools_with_clock(drive: &Fixture, clock: Clock) -> SearchTools {
        tools(&[drive], None).with_clock(clock)
    }

    /// A clock a millisecond later at every look: a walk's every entry is
    /// a look, so 1 500 entries run a call's time out.
    fn ticking() -> Clock {
        let start = Instant::now();
        let ticks = Arc::new(AtomicUsize::new(0));
        Arc::new(move || start + Duration::from_millis(ticks.fetch_add(1, Ordering::SeqCst) as u64))
    }

    /// Acceptance 8: a drive outside the session's scope, or one its grant
    /// does not let it read, is refused by name and nothing is searched;
    /// two drives' hits each carry their own drive's readers, and joined
    /// they narrow the session to `{tgorka}`; an agent-written note is
    /// `agent` integrity and the inbox's guide `untrusted`.
    #[test]
    fn results_are_labelled_and_scope_is_enforced() {
        let tgdrive = Fixture::new("tgdrive", &[TGORKA, MARTA], true);
        let neuradrive = Fixture::new("neuradrive", &[TGORKA], true);
        tgdrive.write("30-work/projects/a.md", "otter plan\n");
        tgdrive.write("00-inbox/README.md", "otter arrivals land here\n");
        neuradrive.write(
            "10-notes/knowledge/b.md",
            "---\ntype: Note\nhuman_reviewed: false\ngenerated:\n  by: agent:nixi@electra\n---\notter facts\n",
        );
        let tools = tools(&[&tgdrive, &neuradrive], None);
        let session = label(&[TGORKA, MARTA], false);
        let refused = call(
            &tools,
            json!({"query": "otter", "drives": ["tgdrive", "otherdrive"]}),
            &session,
        );
        assert_eq!(
            refused,
            ToolOutcome::Refused {
                reason: Said::OutOfScope {
                    drive: "otherdrive".to_owned()
                }
                .sentence()
            }
        );
        assert!(tools.take_reads().is_empty());
        let wire = WireToolCall {
            id: "s2".to_owned(),
            name: DRIVE_SEARCH.to_owned(),
            arguments_raw: String::new(),
            arguments: Some(json!({"query": "otter"})),
        };
        let denied = tools.run(&wire, &session, &|drive| {
            if drive == "neuradrive" {
                Err("no grant".to_owned())
            } else {
                Ok(())
            }
        });
        assert_eq!(
            denied,
            ToolOutcome::Refused {
                reason: Said::OutOfScope {
                    drive: "neuradrive".to_owned()
                }
                .sentence()
            }
        );

        let got = answer(&tools, json!({"query": "otter"}), &session);
        let mut hits = found(&got);
        hits.sort();
        assert_eq!(
            hits,
            [
                "neuradrive/10-notes/knowledge/b.md",
                "tgdrive/00-inbox/README.md",
                "tgdrive/30-work/projects/a.md"
            ],
            "{got:?}"
        );
        let reads = tools.take_reads();
        let of = |path: &str| {
            reads
                .iter()
                .find(|(_, at)| at == path)
                .map(|(label, _)| label.clone())
                .expect(path)
        };
        assert_eq!(
            of("tgdrive/30-work/projects/a.md").readers,
            label(&[TGORKA, MARTA], false).readers
        );
        assert_eq!(
            of("tgdrive/30-work/projects/a.md").integrity,
            Integrity::Agent
        );
        assert_eq!(
            of("tgdrive/00-inbox/README.md").integrity,
            Integrity::Untrusted
        );
        let harvested = of("neuradrive/10-notes/knowledge/b.md");
        assert_eq!(harvested.readers, label(&[TGORKA], false).readers);
        assert_eq!(harvested.integrity, Integrity::Agent);
        let joined = reads
            .iter()
            .fold(session.clone(), |joined, (label, _)| joined.join(label));
        assert_eq!(joined.readers, label(&[TGORKA], false).readers);
        assert_eq!(
            hit(&got, "neuradrive/10-notes/knowledge/b.md").label,
            harvested
        );
    }

    /// Acceptance 9: a drive with no OKF configuration is searched in its
    /// notes vault only, and says so.
    #[test]
    fn a_drive_without_okf_searches_its_notes_only() {
        let drive = Fixture::new("tgdrive", &[TGORKA], false);
        drive.write("10-notes/a.md", "heron\n");
        drive.write("30-work/b.md", "heron\n");
        drive.write("README.md", "heron\n");
        let got = search(&tools(&[&drive], None), json!({"query": "heron"}));
        assert_eq!(found(&got), ["tgdrive/10-notes/a.md"], "{got:?}");
        assert!(
            got.said.contains(&Said::NoOkf {
                drive: "tgdrive".to_owned()
            }),
            "{:?}",
            got.said
        );
    }

    /// Acceptance 10: with no notes index on this host, the vault is
    /// scanned, the result says so, and no index is created.
    #[test]
    fn agentd_searches_lexically() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        drive.write("10-notes/a.md", "lynx\n");
        let got = search(&tools(&[&drive], None), json!({"query": "lynx"}));
        assert_eq!(found(&got), ["tgdrive/10-notes/a.md"], "{got:?}");
        assert!(
            got.said.contains(&Said::NoIndex {
                drive: "tgdrive".to_owned()
            }),
            "{:?}",
            got.said
        );
        assert!(!drive.root().join("10-notes/.keeper").exists());
    }

    /// Build the vault's index as the desktop does, at `10-notes/.keeper`,
    /// over the notes `notes` names, each also written to the disk; the
    /// vectors are 2-d, `meaning.md` pointing where the query embeds.
    fn index_vault(drive: &Fixture, notes: &[(&str, &str)]) {
        let db = drive.root().join("10-notes/.keeper").join(SEARCH_DB_FILE);
        let mut index = SearchIndex::open(&db, "vault").expect("index");
        let fields = std::collections::BTreeMap::new();
        for (path, body) in notes {
            drive.write(&format!("10-notes/{path}"), body);
            let (id, title) = (format!("id-{path}"), format!("Title of {path}"));
            index
                .replace_note(&NoteDoc {
                    id: &id,
                    path,
                    title: &title,
                    tags: &[],
                    fields: &fields,
                    body,
                    stat: None,
                })
                .expect("indexed");
        }
        let pending = index.chunks_without_vectors("m", 100).expect("pending");
        let rows: Vec<(i64, String, Vec<f32>)> = pending
            .into_iter()
            .map(|chunk| {
                let vector = if chunk.embedding_text.contains("meaning") {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                (chunk.rowid, chunk.text_hash, vector)
            })
            .collect();
        index.put_vectors("m", &rows).expect("vectors");
    }

    struct DataDir(PathBuf);

    impl Platform for DataDir {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(self.0.clone())
        }
        fn keychain_set(&self, _: &str, _: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn keychain_get(&self, _: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
        fn keychain_delete(&self, _: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn open_url(&self, _: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn notify(
            &self,
            _: &str,
            _: &str,
            _: &keeper_core::vm::NotifyTarget,
        ) -> Result<(), CoreError> {
            Ok(())
        }
        fn sidecar_path(&self, _: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("no sidecar".to_owned()))
        }
        fn exclude_from_backup(&self, _: &Path) -> Result<(), CoreError> {
            Ok(())
        }
        fn set_badge_count(&self, _: Option<u32>) -> Result<(), CoreError> {
            Ok(())
        }
    }

    /// An OpenAI-shaped embeddings provider answering `[1, 0]` after
    /// `pause_ms`; `hits` counts every request it was sent.
    struct EmbedStub {
        url: String,
        hits: Arc<AtomicUsize>,
    }

    impl EmbedStub {
        fn start(pause_ms: u64) -> EmbedStub {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let url = format!("http://{}", listener.local_addr().expect("addr"));
            let hits = Arc::new(AtomicUsize::new(0));
            let counted = Arc::clone(&hits);
            std::thread::spawn(move || {
                for socket in listener.incoming() {
                    let Ok(mut socket) = socket else { continue };
                    counted.fetch_add(1, Ordering::SeqCst);
                    std::thread::spawn(move || {
                        let mut reader = BufReader::new(socket.try_clone().expect("clone"));
                        let mut length = 0;
                        loop {
                            let mut line = String::new();
                            if reader.read_line(&mut line).is_err()
                                || line == "\r\n"
                                || line.is_empty()
                            {
                                break;
                            }
                            if let Some(value) =
                                line.to_ascii_lowercase().strip_prefix("content-length:")
                            {
                                length = value.trim().parse().unwrap_or(0);
                            }
                        }
                        let mut body = vec![0; length];
                        let _ = reader.read_exact(&mut body);
                        std::thread::sleep(Duration::from_millis(pause_ms));
                        let answer = r#"{"data":[{"index":0,"embedding":[1.0,0.0]}]}"#;
                        let _ = write!(
                            socket,
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                            answer.len()
                        );
                    });
                }
            });
            EmbedStub { url, hits }
        }

        /// This host's configured embeddings model `m`, on this stub, of
        /// `kind`.
        fn configured(&self, data: &Path, kind: keeper_core::bots::ProviderKind) -> Embeddings {
            keeper_core::bots::store::insert_provider(
                data,
                &keeper_core::bots::Provider {
                    id: "embedder".to_owned(),
                    kind,
                    name: "stub".to_owned(),
                    base_url: self.url.clone(),
                    created_ms: 1,
                },
            )
            .expect("provider");
            keeper_core::registry::set_embedding_model(
                data,
                Some(keeper_core::registry::EmbeddingModel {
                    provider: "embedder".to_owned(),
                    model: "m".to_owned(),
                }),
            )
            .expect("model");
            let env = TurnEnv::new(Arc::new(DataDir(data.to_owned())));
            configured_embeddings(&env, data).expect("configured")
        }
    }

    fn vault_drive() -> Fixture {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        index_vault(
            &drive,
            &[
                ("port.md", "the harbour plan\n"),
                ("meaning.md", "about the meaning of docks\n"),
            ],
        );
        drive.write("30-work/projects/harbour.md", "harbour works\n");
        drive.write(
            "30-work/index.md",
            "---\nokf_bundle_name: tgdrive-work\n---\n# Work\n\n## Documents\n\n* [Guide](guide.md) - all about the harbour\n",
        );
        drive.write("30-work/guide.md", "nothing here\n");
        drive
    }

    /// Acceptance 5: inside the vault the vault's own index ranks — with
    /// the query's vector when the model answers within 1 s, so a note that
    /// holds none of the words but means them is found; outside it, a
    /// bundle's listing line and the scan. A model that answers late leaves
    /// the vault lexical, and the result says so.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_vault_index_ranks_the_vault() {
        let drive = vault_drive();
        let data = tempfile::tempdir().expect("data");
        let quick = EmbedStub::start(0);
        let tools_quick = tools(
            &[&drive],
            Some(quick.configured(data.path(), keeper_core::bots::ProviderKind::Ollama)),
        );
        let got = search(&tools_quick, json!({"query": "harbour"}));
        let hits = found(&got);
        assert_eq!(
            hits[..2],
            ["tgdrive/10-notes/port.md", "tgdrive/10-notes/meaning.md"],
            "{got:?}"
        );
        assert!(
            hits.contains(&"tgdrive/30-work/guide.md".to_owned()),
            "{got:?}"
        );
        assert!(
            hits.contains(&"tgdrive/30-work/projects/harbour.md".to_owned()),
            "{got:?}"
        );
        assert!(
            !says(&got, |said| matches!(
                said,
                Said::EmbedLate | Said::EmbedFailed | Said::StaysLocal | Said::MeaningCapped
            )),
            "{:?}",
            got.said
        );
        assert_eq!(quick.hits.load(Ordering::SeqCst), 1);

        let late_data = tempfile::tempdir().expect("data");
        let late = EmbedStub::start(1_500);
        let tools_late = tools(
            &[&drive],
            Some(late.configured(late_data.path(), keeper_core::bots::ProviderKind::Ollama)),
        );
        let got = search(&tools_late, json!({"query": "harbour"}));
        let hits = found(&got);
        assert_eq!(hits[0], "tgdrive/10-notes/port.md", "{got:?}");
        assert!(
            !hits.contains(&"tgdrive/10-notes/meaning.md".to_owned()),
            "{got:?}"
        );
        assert!(got.said.contains(&Said::EmbedLate), "{:?}", got.said);
    }

    /// Acceptance 12 (NFR-115's model sink, R28 S-04): a `local_only`
    /// session's query never reaches an embeddings provider that is not
    /// local — lexical hits, said so, no request — while the same search in
    /// a session that is not `local_only` embeds it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_local_only_session_never_embeds_at_a_remote_provider() {
        let drive = vault_drive();
        let data = tempfile::tempdir().expect("data");
        let remote = EmbedStub::start(0);
        let tools = tools(
            &[&drive],
            Some(remote.configured(data.path(), keeper_core::bots::ProviderKind::OpenAi)),
        );
        let got = answer(&tools, json!({"query": "harbour"}), &label(&[TGORKA], true));
        assert_eq!(found(&got)[0], "tgdrive/10-notes/port.md", "{got:?}");
        assert!(got.said.contains(&Said::StaysLocal), "{:?}", got.said);
        assert_eq!(remote.hits.load(Ordering::SeqCst), 0);
        let got = search(&tools, json!({"query": "harbour"}));
        assert!(
            found(&got).contains(&"tgdrive/10-notes/meaning.md".to_owned()),
            "{got:?}"
        );
        assert_eq!(remote.hits.load(Ordering::SeqCst), 1);
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.org")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.org")
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Every path under `root` with its length and modification time.
    fn snapshot(root: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_owned()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("dir") {
                let path = entry.expect("entry").path();
                let meta = std::fs::symlink_metadata(&path).expect("meta");
                if meta.is_dir() {
                    stack.push(path.clone());
                }
                out.push((path, meta.len(), meta.modified().expect("mtime")));
            }
        }
        out.sort();
        out
    }

    /// Acceptance 11 (D-21, R211): fifty searches over a drive with a vault
    /// index, a listing and a pointer leave `git status --porcelain` empty
    /// and every file of the drive as it was — `.git/` and `.keeper/`
    /// included — but SQLite's own reader files beside the index,
    /// `search.db-wal` and `search.db-shm`, which may appear or change (and
    /// with them the time of the folder they appear in: its entries are
    /// compared one by one).
    #[tokio::test(flavor = "multi_thread")]
    async fn drive_search_writes_nothing() {
        let drive = vault_drive();
        drive.write("30-work/zebra-video.md", &pointer(13_000));
        drive.write(".gitignore", ".keeper/\n");
        git(drive.root(), &["init", "-q"]);
        git(drive.root(), &["add", "-A"]);
        git(drive.root(), &["commit", "-qm", "drive"]);
        // `git status` refreshes `.git/index` itself: the snapshot is taken
        // after one and compared before the next.
        git(drive.root(), &["status", "--porcelain"]);
        let before = snapshot(drive.root());
        let tools = tools(&[&drive], None);
        for n in 0..50 {
            let query = ["harbour", "zebra", "nothing", "a.*b"][n % 4];
            search(&tools, json!({"query": query}));
        }
        let after = snapshot(drive.root());
        let keeper = drive.root().join("10-notes/.keeper");
        let sidecars = [keeper.join("search.db-wal"), keeper.join("search.db-shm")];
        let same = |a: &(PathBuf, u64, std::time::SystemTime),
                    b: &(PathBuf, u64, std::time::SystemTime)| {
            if a.0 == keeper {
                a.0 == b.0
            } else {
                a == b
            }
        };
        let changed: Vec<_> = after
            .iter()
            .filter(|entry| !before.iter().any(|old| same(entry, old)))
            .chain(
                before
                    .iter()
                    .filter(|entry| !after.iter().any(|new| same(entry, new))),
            )
            .filter(|(path, ..)| !sidecars.contains(path))
            .collect();
        assert!(changed.is_empty(), "{changed:?}");
        assert_eq!(git(drive.root(), &["status", "--porcelain"]), "");
    }

    fn link(root: &Path, at: &str, to: &str) {
        let path = root.join(at);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let _ = std::fs::remove_file(&path);
        std::os::unix::fs::symlink(root.join(to), path).expect("link");
    }

    /// R95S-01, R95S-04: a listing is opened, and a bundle's folder walked,
    /// only where keeper's rules and the config let them be: an excluded
    /// bundle's `index.md` is never read (its line would name a document),
    /// nor a listing or a file under `.Keeper/`, `.GIT/` or a session's
    /// `Workspace/` in any case; a listing the config keeps in is read. The
    /// excluded bundle's folder and `.GIT/` are never listed either: under
    /// a clock that ticks at every entry looked at, their 1 600 entries
    /// each would run the call's time out.
    #[test]
    fn listings_and_bundle_folders_are_admitted_before_they_are_opened() {
        let drive = Fixture::new("tgdrive", &[TGORKA], false);
        drive.write(
            ".okf/config.yaml",
            "bundles:\n  - path: \".\"\n    name: root\n  - path: secret\n    name: secret\n  - path: 30-work/.Keeper\n    name: hidden\n  - path: .GIT\n    name: git\n  - path: docs\n    name: docs\nexclude:\n  - secret/**\n",
        );
        let listing = |link: &str, title: &str| {
            format!("---\nokf_bundle_name: x\n---\n# X\n\n## Documents\n\n* [{title}]({link}) - kingfisher\n")
        };
        drive.write("secret/index.md", &listing("../docs/a.md", "Secret"));
        drive.write(
            "30-work/.Keeper/index.md",
            &listing("../../docs/a.md", "Hidden"),
        );
        drive.write("docs/a.md", "nothing here\n");
        drive.write("docs/index.md", &listing("b.md", "Kingfisher notes"));
        drive.write("docs/b.md", "nothing either\n");
        for rel in [
            ".GIT/x.md",
            "30-work/.Keeper/y.md",
            "60-Sessions/active/s/Workspace/z.md",
            "60-sessions/active/s/WORKSPACE/w.md",
            "notes/.KEEPER/v.md",
        ] {
            drive.write(rel, "kingfisher\n");
        }
        for n in 0..1_600 {
            drive.write(&format!("secret/{n:04}.md"), "kingfisher\n");
            drive.write(&format!(".GIT/{n:04}.md"), "kingfisher\n");
        }
        let tools = tools(&[&drive], None).with_clock(ticking());
        let got = search(&tools, json!({"query": "kingfisher"}));
        assert_eq!(found(&got), ["tgdrive/docs/b.md"], "{got:?}");
        assert_eq!(got.hits[0].title, "Kingfisher notes");
        assert!(
            !says(&got, |said| scan_capped(said).is_some()),
            "{:?}",
            got.said
        );
    }

    /// R95S-09, R95S2-05: a drive's config is read within the call's bytes
    /// left, as every read is: once an earlier drive has read the call's
    /// 16 MiB, the next drive's config is not read, and that drive says
    /// nothing in it was searched. The clock stands still, so the bytes and
    /// not the host's speed end the first drive.
    #[test]
    fn a_config_is_read_within_the_calls_bytes_left() {
        let tgdrive = Fixture::new("tgdrive", &[TGORKA], true);
        let text_of = |len: usize| {
            let mut text = "needle\n".repeat(len / 7);
            text.push_str(&"x".repeat(len - text.len()));
            text
        };
        let mib = MAX_FILE_BYTES as usize;
        for n in 0..15 {
            tgdrive.write(&format!("30-work/f{n:02}.md"), &text_of(mib));
        }
        // Its config and these sixteen files are the call's 16 MiB exactly.
        tgdrive.write("30-work/f15.md", &text_of(mib - CONFIG.len()));
        let neuradrive = Fixture::new("neuradrive", &[TGORKA], true);
        neuradrive.write("30-work/n.md", "needle\n");
        let got = search(
            &tools(&[&tgdrive, &neuradrive], None).with_clock(frozen()),
            json!({"query": "needle"}),
        );
        assert!(
            says_of(&got, "neuradrive", unreadable_okf),
            "{:?}",
            got.said
        );
        assert!(!says_of(&got, "tgdrive", scan_capped), "{:?}", got.said);
    }

    /// A clock whose first look sets the call's deadline and whose every
    /// later look is past it.
    fn expired_after_first_look() -> Clock {
        let start = Instant::now();
        let looks = Arc::new(AtomicUsize::new(0));
        Arc::new(move || {
            if looks.fetch_add(1, Ordering::SeqCst) == 0 {
                start
            } else {
                start + Duration::from_secs(2)
            }
        })
    }

    /// R95S2-02: a direct read is admitted as a walked file is, by the
    /// call's time as well as its files and bytes: once the time is up, a
    /// drive's config is not opened and nothing of the drive is searched.
    #[test]
    fn a_config_is_not_opened_once_the_calls_time_is_up() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        drive.write("30-work/a.md", "needle\n");
        let got = search(
            &tools_with_clock(&drive, expired_after_first_look()),
            json!({"query": "needle"}),
        );
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(says_of(&got, "tgdrive", unreadable_okf), "{:?}", got.said);
    }

    /// R95S2-02: the call's 2 000 files are every file it opens, not each
    /// drive's: once one drive's scan has opened them, the next drive's
    /// config is not opened.
    #[test]
    fn a_later_drive_opens_nothing_once_the_calls_files_are_spent() {
        let tgdrive = Fixture::new("tgdrive", &[TGORKA], true);
        for n in 0..2_050 {
            tgdrive.write(&format!("30-work/{n:04}.md"), "x\n");
        }
        let neuradrive = Fixture::new("neuradrive", &[TGORKA], true);
        neuradrive.write("30-work/n.md", "needle\n");
        let got = search(
            &tools(&[&tgdrive, &neuradrive], None).with_clock(frozen()),
            json!({"query": "needle"}),
        );
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(
            says_of(&got, "neuradrive", unreadable_okf),
            "{:?}",
            got.said
        );
    }

    /// R95S2-02: a listing naming more matching documents than the call
    /// may open is read until its files are spent, and no further — the
    /// documents past the bound are unreadable, so an open would show as a
    /// file not searched — and the result says the rest were not opened.
    #[test]
    fn a_listing_longer_than_the_calls_files_stops_at_them_and_says_so() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        let mut listing =
            "---\nokf_bundle_name: tgdrive-work\n---\n# Work\n\n## Documents\n\n".to_owned();
        for n in 0..2_100 {
            let rel = format!("d/{n:04}.md");
            listing.push_str(&format!("* [Needle {n}]({rel}) - needle\n"));
            drive.write(&format!("30-work/{rel}"), "x\n");
            // The config and the listing are two of the call's 2 000.
            if n >= 1_998 {
                std::fs::set_permissions(
                    drive.root().join("30-work").join(&rel),
                    std::fs::Permissions::from_mode(0o000),
                )
                .expect("mode 000");
            }
        }
        drive.write("30-work/index.md", &listing);
        let got = search(
            &tools(&[&drive], None).with_clock(frozen()),
            json!({"query": "needle"}),
        );
        assert_eq!(found(&got).len(), 10);
        assert!(says_of(&got, "tgdrive", incomplete), "{:?}", got.said);
        assert!(
            !says(&got, |said| skipped(said).is_some()),
            "{:?}",
            got.said
        );
    }

    /// R95S2-01: only a config the disk says is not there lets a drive's
    /// notes stand for it: one in a folder that may not be searched, or a
    /// link to nothing, is not taken for absent — the vault's otherwise
    /// excluded note is not opened, and nothing of the drive is searched.
    #[test]
    fn a_config_folder_that_cannot_be_searched_searches_nothing() {
        let drive = Fixture::new("tgdrive", &[TGORKA], false);
        drive.write("10-notes/private/x.md", "plover\n");
        drive.write(
            ".okf/config.yaml",
            "bundles:\n  - path: \".\"\n    name: root\nexclude:\n  - 10-notes/private/**\n",
        );
        let okf = drive.root().join(".okf");
        std::fs::set_permissions(&okf, std::fs::Permissions::from_mode(0o000)).expect("mode 000");
        let got = search(&tools(&[&drive], None), json!({"query": "plover"}));
        std::fs::set_permissions(&okf, std::fs::Permissions::from_mode(0o755)).expect("mode 755");
        let refused =
            |got: &Answer| found(got).is_empty() && says_of(got, "tgdrive", unreadable_okf);
        assert!(refused(&got), "{got:?}");

        link(drive.root(), ".okf/config.yaml", ".okf/gone.yaml");
        let got = search(&tools(&[&drive], None), json!({"query": "plover"}));
        assert!(refused(&got), "{got:?}");
    }

    /// R95S2-02: the vault index's ranked notes, a bundle's listing and a
    /// listed document are read within the call's bounds as well: once the
    /// bytes left cannot hold the next one, it is not opened and the drive
    /// says the rest were not — also where it is the last one, with no
    /// candidate after it to ask the bounds again (a drive with no OKF
    /// config, a single bundle, a listing's last line).
    #[test]
    fn ranked_notes_and_listings_stop_at_the_calls_bytes_and_say_so() {
        let tgdrive = Fixture::new("tgdrive", &[TGORKA], true);
        let mib = MAX_FILE_BYTES as usize;
        for n in 0..15 {
            tgdrive.write(&format!("30-work/f{n:02}.md"), &"x".repeat(mib));
        }
        // Leaves the next drive its config and 4 KiB.
        tgdrive.write(
            "30-work/f15.md",
            &"x".repeat(mib - 2 * CONFIG.len() - 4_096),
        );
        let neuradrive = Fixture::new("neuradrive", &[TGORKA], false);
        index_vault(
            &neuradrive,
            &[("big.md", &format!("harbour {}\n", "y".repeat(8_192)))],
        );
        let got = search(
            &tools(&[&tgdrive, &neuradrive], None).with_clock(frozen()),
            json!({"query": "harbour"}),
        );
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(says_of(&got, "neuradrive", incomplete), "{:?}", got.said);

        let single = "bundles:\n  - path: 30-work\n    name: tgdrive-work\n";
        let listdrive = Fixture::new("listdrive", &[TGORKA], false);
        listdrive.write(".okf/config.yaml", single);
        listdrive.write(
            "30-work/index.md",
            &format!(
                "---\nokf_bundle_name: tgdrive-work\n---\n# Work\n\n## Documents\n\n* [Harbour](h.md) - harbour {}\n",
                "y".repeat(8_192)
            ),
        );
        listdrive.write("30-work/h.md", "harbour\n");
        let got = search(
            &tools(&[&tgdrive, &listdrive], None).with_clock(frozen()),
            json!({"query": "harbour"}),
        );
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(says_of(&got, "listdrive", incomplete), "{:?}", got.said);

        let docdrive = Fixture::new("docdrive", &[TGORKA], false);
        docdrive.write(".okf/config.yaml", single);
        docdrive.write(
            "30-work/index.md",
            "---\nokf_bundle_name: tgdrive-work\n---\n# Work\n\n## Documents\n\n* [Harbour](h.md) - harbour\n",
        );
        docdrive.write("30-work/h.md", &format!("harbour {}\n", "y".repeat(8_192)));
        let got = search(
            &tools(&[&tgdrive, &docdrive], None).with_clock(frozen()),
            json!({"query": "harbour"}),
        );
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(says_of(&got, "docdrive", incomplete), "{:?}", got.said);
    }

    /// R95S-02: a path is admitted where it lands as well as where it is
    /// asked for: a listed alias of an excluded file, a bundle that is a
    /// link to an excluded folder and an indexed alias of an excluded note
    /// are never read; an indexed alias of the inbox's guide is labelled as
    /// the guide; a vault index that is a link out of the drive, or a link
    /// to a file inside it, is not opened and is said, never taken for no
    /// index, and the vault is scanned instead.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_path_is_admitted_where_it_lands_as_well_as_where_it_is_asked() {
        let drive = Fixture::new("tgdrive", &[TGORKA], false);
        drive.write(
            ".okf/config.yaml",
            &CONFIG.replace(
                "  - path: 60-sessions\n",
                "  - path: 50-link\n    name: linked\n  - path: 60-sessions\n",
            ),
        );
        drive.write("00-inbox/secret.md", "osprey secret\n");
        drive.write("00-inbox/README.md", "osprey arrivals\n");
        drive.write("30-work/clients/c.md", "osprey client\n");
        link(drive.root(), "30-work/alias.md", "00-inbox/secret.md");
        drive.write(
            "30-work/index.md",
            "---\nokf_bundle_name: tgdrive-work\n---\n# Work\n\n## Documents\n\n* [Osprey](alias.md) - osprey\n",
        );
        link(drive.root(), "50-link", "00-inbox");
        index_vault(
            &drive,
            &[
                ("guide.md", "osprey guide\n"),
                ("client.md", "osprey client\n"),
            ],
        );
        link(drive.root(), "10-notes/guide.md", "00-inbox/README.md");
        link(drive.root(), "10-notes/client.md", "30-work/clients/c.md");
        let tools = tools(&[&drive], None);
        let got = search(&tools, json!({"query": "osprey"}));
        assert_eq!(found(&got), ["tgdrive/00-inbox/README.md"], "{got:?}");
        let reads = tools.take_reads();
        assert_eq!(reads[0].0.integrity, Integrity::Untrusted);

        let outside = tempfile::tempdir().expect("outside");
        let elsewhere = Fixture::new("tgdrive", &[TGORKA], true);
        index_vault(&elsewhere, &[("a.md", "osprey\n")]);
        std::fs::rename(
            elsewhere.root().join("10-notes/.keeper"),
            outside.path().join("keeper"),
        )
        .expect("move the index out");
        std::os::unix::fs::symlink(
            outside.path().join("keeper"),
            elsewhere.root().join("10-notes/.keeper"),
        )
        .expect("link");
        let got = search(&tools_of(&elsewhere), json!({"query": "osprey"}));
        assert_eq!(found(&got), ["tgdrive/10-notes/a.md"], "{got:?}");
        assert!(says_of(&got, "tgdrive", index_unusable), "{:?}", got.said);

        let inside = Fixture::new("tgdrive", &[TGORKA], true);
        index_vault(&inside, &[("a.md", "osprey\n")]);
        std::fs::create_dir_all(inside.root().join("00-inbox")).expect("mkdir");
        std::fs::rename(
            inside.root().join("10-notes/.keeper").join(SEARCH_DB_FILE),
            inside.root().join("00-inbox").join(SEARCH_DB_FILE),
        )
        .expect("move the index aside");
        link(
            inside.root(),
            &format!("10-notes/.keeper/{SEARCH_DB_FILE}"),
            &format!("00-inbox/{SEARCH_DB_FILE}"),
        );
        let got = search(&tools_of(&inside), json!({"query": "osprey"}));
        assert_eq!(found(&got), ["tgdrive/10-notes/a.md"], "{got:?}");
        assert!(says_of(&got, "tgdrive", index_unusable), "{:?}", got.said);
    }

    fn tools_of(drive: &Fixture) -> SearchTools {
        tools(&[drive], None)
    }

    /// R95S-05: a hit's label is the bytes it was made from, taken at the
    /// read: an untrusted-marked file changed or deleted after it was
    /// scanned (here, while the next drive's query is embedded) keeps its
    /// `untrusted` label and its text; a listed document carries its
    /// untrusted listing's label as well as its own.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_hits_label_is_the_bytes_it_was_made_from() {
        let tgdrive = Fixture::new("tgdrive", &[TGORKA], true);
        let marked = "---\nintegrity: untrusted\n---\nheron here\n";
        tgdrive.write("30-work/a.md", marked);
        tgdrive.write("30-work/b.md", marked);
        tgdrive.write(
            "30-work/index.md",
            "---\nokf_bundle_name: tgdrive-work\nintegrity: untrusted\n---\n# Work\n\n## Documents\n\n* [Heron guide](guide.md) - heron\n",
        );
        tgdrive.write("30-work/guide.md", "clean words\n");
        let neuradrive = Fixture::new("neuradrive", &[TGORKA], true);
        index_vault(&neuradrive, &[("n.md", "heron\n")]);
        let root = tgdrive.root().to_owned();
        let embeddings = Embeddings {
            model: "m".to_owned(),
            local: true,
            ask: Box::new(move |_| {
                std::fs::write(root.join("30-work/a.md"), "heron clean\n").expect("rewrite");
                std::fs::remove_file(root.join("30-work/b.md")).expect("delete");
                Ok(vec![1.0, 0.0])
            }),
        };
        let tools = tools(&[&tgdrive, &neuradrive], Some(embeddings));
        let got = search(&tools, json!({"query": "heron"}));
        assert_eq!(
            hit(&got, "tgdrive/30-work/a.md").lines,
            [(4, "heron here".to_owned())]
        );
        let reads = tools.take_reads();
        for path in [
            "tgdrive/30-work/a.md",
            "tgdrive/30-work/b.md",
            "tgdrive/30-work/guide.md",
        ] {
            let (label, _) = reads
                .iter()
                .find(|(_, at)| at == path)
                .unwrap_or_else(|| panic!("{path}: {got:?}"));
            assert_eq!(label.integrity, Integrity::Untrusted, "{path}");
        }
    }

    /// R95S-06, R95S-07: a listed document is read as the scan reads it — a
    /// FIFO is never waited on, a file over 1 MiB never read, neither is a
    /// result — and a pointer, listed or indexed, is a result only by its
    /// name, said to be not on this device with its size.
    #[tokio::test(flavor = "multi_thread")]
    async fn listed_and_indexed_pointers_and_odd_files_are_read_as_the_scan_reads_them() {
        let drive = vault_drive();
        drive.write(
            "30-work/index.md",
            "---\nokf_bundle_name: tgdrive-work\n---\n# Work\n\n## Documents\n\n* [Egret pipe](pipe.md) - egret\n* [Egret big](big.md) - egret\n* [Egret film](film.md) - egret\n* [Egret clip](egret-clip.md) - egret\n",
        );
        let made = std::process::Command::new("mkfifo")
            .arg(drive.root().join("30-work/pipe.md"))
            .status()
            .expect("mkfifo");
        assert!(made.success());
        drive.write("30-work/big.md", &"egret ".repeat(200_000));
        drive.write("30-work/film.md", &pointer(9_000));
        drive.write("30-work/egret-clip.md", &pointer(13_000));
        let tools = tools(&[&drive], None);
        let got = search(&tools, json!({"query": "egret"}));
        assert_eq!(found(&got), ["tgdrive/30-work/egret-clip.md"], "{got:?}");
        assert_eq!(got.hits[0].absent, Some(13_000));
        assert!(says_of(&got, "tgdrive", skipped), "{:?}", got.said);

        // The indexed note is released: its old words no longer find it,
        // its name does.
        drive.write("10-notes/port.md", &pointer(20_000));
        let got = search(&tools, json!({"query": "harbour"}));
        assert!(
            !found(&got).contains(&"tgdrive/10-notes/port.md".to_owned()),
            "{got:?}"
        );
        let got = search(&tools, json!({"query": "port"}));
        assert_eq!(found(&got)[0], "tgdrive/10-notes/port.md", "{got:?}");
        assert_eq!(got.hits[0].absent, Some(20_000));
    }

    /// R95S-08: a drive whose config is there and cannot be read, or
    /// cannot be interpreted, has nothing searched — not its notes either,
    /// since what it excludes is unknown — and says so.
    #[test]
    fn an_unreadable_or_unsupported_okf_config_searches_nothing() {
        let drive = Fixture::new("tgdrive", &[TGORKA], false);
        drive.write("10-notes/private/x.md", "plover\n");
        drive.write(
            ".okf/config.yaml",
            "bundles:\n  - path: \".\"\n    name: root\nexclude: &private\n  - 10-notes/private/**\n",
        );
        let got = search(&tools(&[&drive], None), json!({"query": "plover"}));
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(says_of(&got, "tgdrive", unreadable_okf), "{:?}", got.said);
        drive.write(".okf/config.yaml", CONFIG);
        std::fs::set_permissions(
            drive.root().join(".okf/config.yaml"),
            std::fs::Permissions::from_mode(0o000),
        )
        .expect("mode 000");
        let got = search(&tools(&[&drive], None), json!({"query": "plover"}));
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(says_of(&got, "tgdrive", unreadable_okf), "{:?}", got.said);
    }

    /// R95S-11: an index that opens and cannot answer — no tables, or no
    /// paths for its hits — leaves the vault to the scan and says so.
    #[test]
    fn an_index_that_cannot_answer_leaves_the_vault_scanned() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        drive.write("10-notes/a.md", "curlew\n");
        drive.write("10-notes/.keeper/search.db", "");
        let got = search(&tools(&[&drive], None), json!({"query": "curlew"}));
        assert_eq!(found(&got), ["tgdrive/10-notes/a.md"], "{got:?}");
        assert!(says_of(&got, "tgdrive", index_unusable), "{:?}", got.said);

        let indexed = Fixture::new("tgdrive", &[TGORKA], true);
        index_vault(&indexed, &[("a.md", "curlew\n")]);
        rusqlite::Connection::open(indexed.root().join("10-notes/.keeper").join(SEARCH_DB_FILE))
            .expect("db")
            .execute_batch("ALTER TABLE notes RENAME TO gone")
            .expect("rename");
        let got = search(&tools(&[&indexed], None), json!({"query": "curlew"}));
        assert_eq!(found(&got), ["tgdrive/10-notes/a.md"], "{got:?}");
        assert!(says_of(&got, "tgdrive", index_unusable), "{:?}", got.said);
    }

    /// R95S-14: a note the index ranks keeps its own title and the lines
    /// that hold a term though not every term is in it: a note found by
    /// meaning, one found by the index's any-word fallback.
    #[tokio::test(flavor = "multi_thread")]
    async fn ranked_notes_keep_their_title_and_lines() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        index_vault(
            &drive,
            &[
                ("port.md", "the harbour plan\n"),
                (
                    "meaning.md",
                    "---\ntitle: Docks and meaning\n---\nabout the meaning of docks\n",
                ),
            ],
        );
        let data = tempfile::tempdir().expect("data");
        let quick = EmbedStub::start(0);
        let tools = tools(
            &[&drive],
            Some(quick.configured(data.path(), keeper_core::bots::ProviderKind::Ollama)),
        );
        let got = search(&tools, json!({"query": "harbour"}));
        assert_eq!(
            hit(&got, "tgdrive/10-notes/meaning.md").title,
            "Docks and meaning"
        );
        let got = search(&tools, json!({"query": "harbour zebrafish"}));
        assert_eq!(
            hit(&got, "tgdrive/10-notes/port.md").lines,
            [(1, "the harbour plan".to_owned())]
        );
    }

    /// A vault index of `notes` notes holding "harbour", each chunk's
    /// vector `dim` wide.
    fn wide_index(drive: &Fixture, notes: usize, dim: usize) {
        let db = drive.root().join("10-notes/.keeper").join(SEARCH_DB_FILE);
        let mut index = SearchIndex::open(&db, "vault").expect("index");
        let fields = std::collections::BTreeMap::new();
        for n in 0..notes {
            let (id, path) = (format!("id-{n}"), format!("{n:03}.md"));
            let body = format!("harbour {n}\n");
            drive.write(&format!("10-notes/{path}"), &body);
            index
                .replace_note(&NoteDoc {
                    id: &id,
                    path: &path,
                    title: &path,
                    tags: &[],
                    fields: &fields,
                    body: &body,
                    stat: None,
                })
                .expect("indexed");
        }
        let rows: Vec<(i64, String, Vec<f32>)> = index
            .chunks_without_vectors("m", 10_000)
            .expect("pending")
            .into_iter()
            .map(|chunk| (chunk.rowid, chunk.text_hash, vec![1.0; dim]))
            .collect();
        index.put_vectors("m", &rows).expect("vectors");
    }

    fn local_embeddings(dim: usize, asked: impl Fn() + Send + Sync + 'static) -> Embeddings {
        Embeddings {
            model: "m".to_owned(),
            local: true,
            ask: Box::new(move |_| {
                asked();
                Ok(vec![1.0; dim])
            }),
        }
    }

    /// R95S2-04: the rows the vault's index hands over count against the
    /// call's 16 MiB: vectors past them end the ranking by meaning, which
    /// is said, and what the rows spent is not read elsewhere — the next
    /// folder's matching 512 KiB file no longer fits and is not opened.
    #[test]
    fn the_indexs_rows_count_against_the_calls_bytes() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        // 140 vectors of 128 KiB: 17.5 MiB.
        wide_index(&drive, 70, 32_768);
        drive.write(
            "30-work/x.md",
            &format!("harbour {}\n", "x".repeat(512 * 1024)),
        );
        let tools = tools(&[&drive], Some(local_embeddings(32_768, || {}))).with_clock(frozen());
        let got = search(&tools, json!({"query": "harbour"}));
        assert!(got.said.contains(&Said::MeaningCapped), "{:?}", got.said);
        // No vector past what was left was fetched, so the words still rank.
        assert!(!says_of(&got, "tgdrive", index_unusable), "{:?}", got.said);
        assert_eq!(hit(&got, "tgdrive/10-notes/000.md").found, Found::Index);
        assert!(
            !found(&got).contains(&"tgdrive/30-work/x.md".to_owned()),
            "{:?}",
            found(&got)
        );
        assert!(says_of(&got, "tgdrive", scan_capped), "{:?}", got.said);
    }

    /// R95S2-04: the vault's index reads within the call's time: a ranking
    /// by meaning still running when the time is up (here, it runs out
    /// while the query is embedded) is interrupted and said.
    #[test]
    fn the_indexs_ranking_stops_when_the_calls_time_is_up() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        wide_index(&drive, 4, 2);
        let late = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let start = Instant::now();
        let seen = Arc::clone(&late);
        let clock: Clock = Arc::new(move || {
            if seen.load(Ordering::SeqCst) {
                start + Duration::from_secs(2)
            } else {
                start
            }
        });
        let asked = Arc::clone(&late);
        let embeddings = local_embeddings(2, move || asked.store(true, Ordering::SeqCst));
        let tools = tools(&[&drive], Some(embeddings)).with_clock(clock);
        let got = search(&tools, json!({"query": "harbour"}));
        assert!(got.said.contains(&Said::MeaningCapped), "{:?}", got.said);
        assert!(found(&got).is_empty(), "{got:?}");
    }

    /// A clock that stands still for its first `looks` looks and is past
    /// any call's deadline at every later one.
    fn expires_after(looks: usize) -> Clock {
        let start = Instant::now();
        let seen = Arc::new(AtomicUsize::new(0));
        Arc::new(move || {
            if seen.fetch_add(1, Ordering::SeqCst) < looks {
                start
            } else {
                start + Duration::from_secs(2)
            }
        })
    }

    fn mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("mode");
    }

    /// `drive`'s vault indexed over one note holding "needle", the index
    /// file unreadable: SQLite opening it fails, which would be said as an
    /// index that could not answer.
    fn unreadable_index(drive: &Fixture) {
        index_vault(drive, &[("a.md", "needle\n")]);
        mode(
            &drive.root().join("10-notes/.keeper").join(SEARCH_DB_FILE),
            0o000,
        );
    }

    /// Whether `got` says `drive`'s index was not opened for the call's
    /// bounds — and not that it opened and could not answer.
    fn index_capped(got: &Answer, drive: &str) -> bool {
        got.said.contains(&Said::IndexCapped {
            drive: drive.to_owned(),
        }) && !says_of(got, drive, index_unusable)
    }

    /// R95S3-02: the vault index's open is admitted as every read is, so
    /// SQLite never opens it once the call's files or time are spent: not
    /// for a later drive with no config, not once a drive's config took the
    /// call's last file, not once the time is up.
    #[test]
    fn the_index_is_opened_only_within_the_calls_bounds() {
        let tgdrive = Fixture::new("tgdrive", &[TGORKA], true);
        for n in 0..2_050 {
            tgdrive.write(&format!("30-work/{n:04}.md"), "x\n");
        }
        let bare = Fixture::new("neuradrive", &[TGORKA], false);
        unreadable_index(&bare);
        let got = search(
            &tools(&[&tgdrive, &bare], None).with_clock(frozen()),
            json!({"query": "needle"}),
        );
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(index_capped(&got, "neuradrive"), "{:?}", got.said);

        // The config and 1 998 files are 1 999 of the call's 2 000.
        let spent = Fixture::new("tgdrive", &[TGORKA], true);
        for n in 0..1_998 {
            spent.write(&format!("30-work/{n:04}.md"), "x\n");
        }
        let configured = Fixture::new("neuradrive", &[TGORKA], true);
        unreadable_index(&configured);
        let got = search(
            &tools(&[&spent, &configured], None).with_clock(frozen()),
            json!({"query": "needle"}),
        );
        assert!(!says_of(&got, "tgdrive", scan_capped), "{:?}", got.said);
        assert!(
            !says_of(&got, "neuradrive", unreadable_okf),
            "{:?}",
            got.said
        );
        assert!(index_capped(&got, "neuradrive"), "{:?}", got.said);

        // The call's deadline and its config's admission are its first two
        // looks at the clock; the index's admission is past the deadline.
        let timed = Fixture::new("tgdrive", &[TGORKA], true);
        unreadable_index(&timed);
        let got = search(
            &tools_with_clock(&timed, expires_after(2)),
            json!({"query": "needle"}),
        );
        assert!(!says_of(&got, "tgdrive", unreadable_okf), "{:?}", got.said);
        assert!(index_capped(&got, "tgdrive"), "{:?}", got.said);
    }

    /// R95S3-03: listed and ranked candidates stop at the call's bounds
    /// themselves, not only where one is read: bundles whose listings are
    /// all missing, and a listing whose lines all name missing documents,
    /// run the time out under a clock a millisecond later at every look,
    /// and ranked notes that are all gone stop once the index's open took
    /// the call's last file — each says the rest were not opened.
    #[test]
    fn candidates_stop_at_the_bounds_though_none_is_read() {
        let listed = Fixture::new("tgdrive", &[TGORKA], true);
        let mut listing =
            "---\nokf_bundle_name: tgdrive-work\n---\n# Work\n\n## Documents\n\n".to_owned();
        for n in 0..3_000 {
            listing.push_str(&format!("* [Needle {n}](gone/{n:04}.md) - needle\n"));
        }
        listed.write("30-work/index.md", &listing);
        let got = search(
            &tools_with_clock(&listed, ticking()),
            json!({"query": "needle"}),
        );
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(says_of(&got, "tgdrive", incomplete), "{:?}", got.said);

        let bundled = Fixture::new("tgdrive", &[TGORKA], false);
        let mut config = "bundles:\n".to_owned();
        for n in 0..3_000 {
            config.push_str(&format!("  - path: b{n:04}\n    name: b{n:04}\n"));
        }
        bundled.write(".okf/config.yaml", &config);
        let got = search(
            &tools_with_clock(&bundled, ticking()),
            json!({"query": "needle"}),
        );
        assert!(says_of(&got, "tgdrive", incomplete), "{:?}", got.said);

        // The config and 1 998 files are 1 999 of the call's 2 000.
        let spent = Fixture::new("tgdrive", &[TGORKA], true);
        for n in 0..1_998 {
            spent.write(&format!("30-work/{n:04}.md"), "x\n");
        }
        let ranked = Fixture::new("neuradrive", &[TGORKA], false);
        index_vault(
            &ranked,
            &[
                ("a.md", "needle\n"),
                ("b.md", "needle\n"),
                ("c.md", "needle\n"),
            ],
        );
        for name in ["a.md", "b.md", "c.md"] {
            std::fs::remove_file(ranked.root().join("10-notes").join(name)).expect("gone");
        }
        let got = search(
            &tools(&[&spent, &ranked], None).with_clock(frozen()),
            json!({"query": "needle"}),
        );
        assert!(!says_of(&got, "tgdrive", scan_capped), "{:?}", got.said);
        assert!(says_of(&got, "neuradrive", incomplete), "{:?}", got.said);
    }

    /// R95S3-04: nothing says the vault's index holds every note as it is
    /// now, so it never stands for the vault alone: beside an index built
    /// over nothing, or one that predates a note or its new name, the
    /// vault's matching notes are found; a current index with nothing to
    /// find is a plain empty answer; and where the scan beside the index
    /// stops early, the vault is said to be covered only in part.
    #[test]
    fn an_index_that_may_be_stale_never_stands_for_the_vault() {
        for okf in [true, false] {
            let empty = Fixture::new("tgdrive", &[TGORKA], okf);
            index_vault(&empty, &[]);
            empty.write("10-notes/a.md", "plover\n");
            let got = search(&tools(&[&empty], None), json!({"query": "plover"}));
            assert_eq!(found(&got), ["tgdrive/10-notes/a.md"], "{okf}: {got:?}");
            assert!(!says_of(&got, "tgdrive", index_unusable), "{:?}", got.said);
        }

        let older = Fixture::new("tgdrive", &[TGORKA], true);
        index_vault(&older, &[("port.md", "the harbour plan\n")]);
        std::fs::rename(
            older.root().join("10-notes/port.md"),
            older.root().join("10-notes/moved.md"),
        )
        .expect("rename");
        older.write("10-notes/new.md", "a harbour since\n");
        let got = search(&tools(&[&older], None), json!({"query": "harbour"}));
        let mut hits = found(&got);
        hits.sort();
        assert_eq!(
            hits,
            ["tgdrive/10-notes/moved.md", "tgdrive/10-notes/new.md"],
            "{got:?}"
        );

        let current = Fixture::new("tgdrive", &[TGORKA], true);
        index_vault(&current, &[("port.md", "the harbour plan\n")]);
        let got = search(&tools(&[&current], None), json!({"query": "zebrafish"}));
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(
            !says(&got, |said| matches!(
                said,
                Said::IndexUnusable { .. }
                    | Said::IndexPartial { .. }
                    | Said::Incomplete { .. }
                    | Said::ScanCapped { .. }
            )),
            "{:?}",
            got.said
        );

        // The config and 1 994 files are 1 995 of the call's 2 000: the
        // next drive's config, its index, its ranked note and two more of
        // its notes are the rest.
        let spent = Fixture::new("tgdrive", &[TGORKA], true);
        for n in 0..1_994 {
            spent.write(&format!("30-work/{n:04}.md"), "x\n");
        }
        let partial = Fixture::new("neuradrive", &[TGORKA], true);
        index_vault(&partial, &[("port.md", "the harbour plan\n")]);
        for n in 0..10 {
            partial.write(&format!("10-notes/n{n}.md"), "x\n");
        }
        let got = search(
            &tools(&[&spent, &partial], None).with_clock(frozen()),
            json!({"query": "harbour"}),
        );
        assert_eq!(found(&got), ["neuradrive/10-notes/port.md"], "{got:?}");
        assert!(
            got.said.contains(&Said::IndexPartial {
                drive: "neuradrive".to_owned()
            }),
            "{:?}",
            got.said
        );
    }

    /// R95S3-05: a folder that may be searched and cannot be read is said,
    /// never taken for an empty one — not opened (mode 000), not listed
    /// (no search permission), a bundle that cannot be walked or whose
    /// landing cannot be asked about — and a listed document, or a
    /// bundle's listing, behind a folder that cannot be read is a file not
    /// searched; the same folders readable and empty say neither.
    #[test]
    fn folders_that_cannot_be_read_are_said_not_taken_for_empty() {
        let drive = Fixture::new("tgdrive", &[TGORKA], false);
        drive.write(
            ".okf/config.yaml",
            "bundles:\n  - path: \".\"\n    name: root\n  - path: docs\n    name: docs\n  - path: sealed\n    name: sealed\n  - path: secret/inner\n    name: inner\n",
        );
        drive.write(
            "docs/index.md",
            "---\nokf_bundle_name: docs\n---\n# Docs\n\n## Documents\n\n* [Needle](locked/doc.md) - needle\n",
        );
        let files = [
            "docs/locked/doc.md",
            "docs/noexec/n.md",
            "sealed/s.md",
            "secret/inner/i.md",
        ];
        for rel in files {
            drive.write(rel, "needle\n");
        }
        let modes = [
            ("docs/locked", 0o000),
            ("docs/noexec", 0o400),
            ("sealed", 0o100),
            ("secret", 0o000),
        ];
        for (rel, bits) in modes {
            mode(&drive.root().join(rel), bits);
        }
        let got = search(&tools(&[&drive], None), json!({"query": "needle"}));
        for (rel, _) in modes {
            mode(&drive.root().join(rel), 0o755);
        }
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(
            got.said.contains(&Said::Unlisted {
                drive: "tgdrive".to_owned(),
                folders: 5
            }),
            "{:?}",
            got.said
        );
        // `docs/locked/doc.md`, listed, and `secret/inner/index.md`.
        assert!(
            got.said.contains(&Said::Skipped {
                drive: "tgdrive".to_owned(),
                files: 2
            }),
            "{:?}",
            got.said
        );

        for rel in files {
            std::fs::remove_file(drive.root().join(rel)).expect("emptied");
        }
        let got = search(&tools(&[&drive], None), json!({"query": "needle"}));
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(
            !says(&got, |said| matches!(
                said,
                Said::Unlisted { .. } | Said::Skipped { .. }
            )),
            "{:?}",
            got.said
        );
    }

    /// R95S4-01: an index whose text is not stored as UTF-8 — whose stored
    /// lengths are not those of what it hands over — is not used, and that
    /// is said; the vault is scanned past it.
    #[test]
    fn an_index_not_stored_as_utf8_is_said_and_scanned_past() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        let db = drive.root().join("10-notes/.keeper").join(SEARCH_DB_FILE);
        std::fs::create_dir_all(db.parent().expect("parent")).expect("mkdir");
        let conn = rusqlite::Connection::open(&db).expect("db");
        conn.pragma_update(None, "encoding", "UTF-16le")
            .expect("encoding");
        conn.execute_batch(&format!(
            "CREATE TABLE meta(schema INTEGER, vault_id TEXT); INSERT INTO meta VALUES({}, 'vault');",
            search_index::SEARCH_SCHEMA
        ))
        .expect("meta");
        drop(conn);
        index_vault(&drive, &[("a.md", "needle\n")]);
        let got = search(&tools(&[&drive], None), json!({"query": "needle"}));
        assert!(says_of(&got, "tgdrive", index_unusable), "{:?}", got.said);
        assert_eq!(hit(&got, "tgdrive/10-notes/a.md").found, Found::Scan);
    }

    /// R95S4-03: a listing is read within the bounds a line at a time,
    /// whatever its lines are: 3 000 lines of delimiters that list nothing
    /// run the time out under a clock a millisecond later at every look,
    /// and the matching line after them is never acted on — its document,
    /// there and matching, is not read.
    #[test]
    fn a_listing_is_read_within_the_bounds_a_line_at_a_time() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        let mut listing = "# Work\n\n## Documents\n\n".to_owned();
        for _ in 0..3_000 {
            listing.push_str(&format!("* [{}\n", "](".repeat(50)));
        }
        listing.push_str("* [Needle](needle.md) - needle\n");
        drive.write("30-work/index.md", &listing);
        drive.write("30-work/needle.md", "needle\n");
        let got = search(
            &tools_with_clock(&drive, ticking()),
            json!({"query": "needle"}),
        );
        assert!(found(&got).is_empty(), "{got:?}");
        assert!(says_of(&got, "tgdrive", incomplete), "{:?}", got.said);
    }

    /// R95S4-04, R95S4-06: once the time is up no further bundle is
    /// resolved or walked — 3 000 bundles, each a folder holding a file,
    /// whose walks would each look at the clock, cost the call a handful of
    /// looks — and what the bundles not walked hold is not counted.
    #[test]
    fn no_bundle_is_walked_once_the_time_is_up() {
        let drive = Fixture::new("tgdrive", &[TGORKA], false);
        let mut config = "bundles:\n".to_owned();
        for n in 0..3_000 {
            config.push_str(&format!("  - path: b{n:04}\n    name: b{n:04}\n"));
            drive.write(&format!("b{n:04}/x.md"), "x\n");
        }
        drive.write(".okf/config.yaml", &config);
        // The deadline, the config's admission and the first listing's
        // check are the first three looks; every later one is past it.
        let looks = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();
        let counted = Arc::clone(&looks);
        let clock: Clock = Arc::new(move || {
            if counted.fetch_add(1, Ordering::SeqCst) < 3 {
                start
            } else {
                start + Duration::from_secs(2)
            }
        });
        let got = search(&tools_with_clock(&drive, clock), json!({"query": "x"}));
        let looked = looks.load(Ordering::SeqCst);
        assert!(looked < 50, "{looked} looks at the clock");
        assert!(
            says(&got, |said| matches!(
                said,
                Said::ScanCapped { drive, of: None, .. } if drive == "tgdrive"
            )),
            "{:?}",
            got.said
        );
    }

    /// R95S4-05: a landing is read once, and what that read came to is
    /// evaluated under every name that reaches it: a note the index still
    /// ranks but which is now a pointer is no result under its own name,
    /// and is one — by its name alone, with its size — under a listing's
    /// matching link to it; a binary note the index ranks is counted not
    /// searched once, not again under the listing's link to it.
    #[test]
    fn a_landing_read_under_one_name_is_evaluated_under_another() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        index_vault(&drive, &[("plain.md", "needle\n"), ("bin.md", "needle\n")]);
        drive.write("10-notes/plain.md", &pointer(4_242));
        std::fs::write(drive.root().join("10-notes/bin.md"), b"needle\0\n").expect("binary");
        drive.write(
            "30-work/index.md",
            "# Work\n\n## Documents\n\n* [Needle](needle.md) - needle\n* [Needle bin](bin.md) - needle\n",
        );
        link(drive.root(), "30-work/needle.md", "10-notes/plain.md");
        link(drive.root(), "30-work/bin.md", "10-notes/bin.md");
        let got = search(&tools(&[&drive], None), json!({"query": "needle"}));
        assert_eq!(found(&got), ["tgdrive/10-notes/plain.md"], "{got:?}");
        let pointed = hit(&got, "tgdrive/10-notes/plain.md");
        assert_eq!(pointed.absent, Some(4_242));
        assert!(pointed.lines.is_empty());
        assert_eq!(pointed.found, Found::Listing);
        assert!(
            got.said.contains(&Said::Skipped {
                drive: "tgdrive".to_owned(),
                files: 1
            }),
            "{:?}",
            got.said
        );
    }

    /// R95S4-06: a scan stopped at the call's files beside a folder it
    /// could not read does not claim a total — that folder may hold any
    /// number of files more — and says the folder.
    #[test]
    fn a_folder_not_read_leaves_the_capped_count_unknown() {
        let drive = Fixture::new("tgdrive", &[TGORKA], true);
        for n in 0..2_100 {
            drive.write(&format!("30-work/a/{n:04}.md"), "x\n");
        }
        drive.write("30-work/z/needle.md", "needle\n");
        mode(&drive.root().join("30-work/z"), 0o000);
        let got = search(
            &tools(&[&drive], None).with_clock(frozen()),
            json!({"query": "needle"}),
        );
        mode(&drive.root().join("30-work/z"), 0o755);
        assert!(
            says(&got, |said| matches!(
                said,
                Said::ScanCapped { drive, of: None, .. } if drive == "tgdrive"
            )),
            "{:?}",
            got.said
        );
        assert!(
            got.said.contains(&Said::Unlisted {
                drive: "tgdrive".to_owned(),
                folders: 1
            }),
            "{:?}",
            got.said
        );
    }

    /// A drive whose bundles are `order` (`needle` a link to the folder
    /// `plain`), and whose `catalog/index.md` links each of `pointers` in
    /// `plain` under a title holding "needle": the listing reads every
    /// pointer first, a result under no name that holds the term.
    fn aliased_pointers(order: &[&str], pointers: &[&str]) -> Fixture {
        let drive = Fixture::new("tgdrive", &[TGORKA], false);
        let mut config = "bundles:\n".to_owned();
        for bundle in order {
            config.push_str(&format!("  - path: {bundle}\n    name: {bundle}\n"));
        }
        drive.write(".okf/config.yaml", &config);
        let mut listing = "# Catalog\n\n## Documents\n\n".to_owned();
        for (n, name) in (0..).zip(pointers) {
            drive.write(&format!("plain/{name}"), &pointer(1_000 + n));
            listing.push_str(&format!("* [Needle](../plain/{name}) - x\n"));
        }
        drive.write("catalog/index.md", &listing);
        link(drive.root(), "needle", "plain");
        drive
    }

    /// R95S5-01: a pointer read already is evaluated where the walk meets
    /// it, under the time the walk asks before each entry it offers: once
    /// the time runs out among forty such pointers met through `needle`,
    /// the twenty met before it are results and none met after it is
    /// evaluated. The forty offers are the call's last forty looks at the
    /// clock, counted by a run whose clock stands still; the time runs out
    /// at the twenty-first.
    #[test]
    fn a_pointer_read_already_is_not_evaluated_once_the_time_is_up() {
        let names: Vec<String> = (0..40).map(|n| format!("{n:02}.md")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let drive = aliased_pointers(&["catalog", "plain", "needle"], &names);
        let args = json!({"query": "needle", "k": 25});
        let looks = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&looks);
        let start = Instant::now();
        let still: Clock = Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            start
        });
        let every = search(&tools_with_clock(&drive, still), args.clone());
        assert_eq!(found(&every).len(), 25, "{every:?}");
        let looked = looks.load(Ordering::SeqCst);
        let got = search(&tools_with_clock(&drive, expires_after(looked - 20)), args);
        let met: Vec<String> = names[..20]
            .iter()
            .map(|name| format!("tgdrive/plain/{name}"))
            .collect();
        assert_eq!(found(&got), met, "{got:?}");
        assert!(
            says(&got, |said| matches!(
                said,
                Said::ScanCapped { drive, of: None, .. } if drive == "tgdrive"
            )),
            "{:?}",
            got.said
        );
    }

    /// R95S5-02: a pointer read already keeps its place in the walk among
    /// the files the walk reads: through `needle`, the pointer `plain/a.md`
    /// comes before the text `plain/z.md`, both results by the name they
    /// are asked by, and is the one result kept at `k = 1`.
    #[test]
    fn a_pointer_read_already_keeps_its_place_in_the_walk() {
        let drive = aliased_pointers(&["needle", "plain", "catalog"], &["a.md"]);
        drive.write("plain/z.md", "x\n");
        let tools = tools(&[&drive], None);
        let got = search(&tools, json!({"query": "needle"}));
        assert_eq!(
            found(&got),
            ["tgdrive/plain/a.md", "tgdrive/plain/z.md"],
            "{got:?}"
        );
        assert_eq!(hit(&got, "tgdrive/plain/a.md").absent, Some(1_000));
        let got = search(&tools, json!({"query": "needle", "k": 1}));
        assert_eq!(found(&got), ["tgdrive/plain/a.md"], "{got:?}");
    }
}
