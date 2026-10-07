//! The Mac's own `[[mcp]]` servers and `[sandbox]` table (96.2 #11, Q9(a),
//! R149, R213): what agentd reads from `agentd.toml`, this Mac keeps in
//! `keeper.db` — device-local, never in `settings.toml`, a device file or
//! the account's repository — and checks with the same functions
//! ([`mcp::check`], [`SandboxTable::check`]).
//!
//! Tables beside the pins' ([`crate::agents::pins`]), under their rules:
//! WAL, a busy timeout, `CREATE TABLE IF NOT EXISTS` on every open,
//! normalized child tables, no JSON blob in a row — and no credential
//! column (AD-139): a server's bearer token is in the keychain under
//! [`token_key`] of a generation of its own; its row says that it is
//! saved with one and names that generation, and its entry then names it
//! `secret:<generation>`.
//!
//! **One snapshot, one revision.** Every save is one transaction that makes
//! the store's revision greater; every read — the host's, a listing's — is
//! one read transaction, so it sees a save whole or not at all, and the
//! revision it reads is the one its rows were saved under.
//!
//! **A token is chosen by the row that publishes it (R228, R276).** Each
//! token a person saves is kept under a new generation's key, never under
//! its server's name, before the row naming that generation is committed
//! in the same transaction as the rest of the row. So a read of the tables
//! — however old — reaches only the token its own rows published: a URL
//! moved with a new token never sends that token to the old URL. A save
//! that does not commit names nothing new, so the token it staged is never
//! sent, whether or not the keychain then deletes it. After a commit, an
//! operation deletes only the generation it retired (Forget, Remove, a URL
//! made a program, a token replaced): a newer save's token is never
//! another operation's to delete. A keychain that cannot be read is an
//! error, never "no token": its server is not connected and its listing
//! says why. A token an earlier keeper kept under the server's name is
//! refused until a token is saved again ([`LEGACY_TOKEN`]).
//!
//! Only the person writes here, from Settings › Agents: *Trust this
//! server's annotations* and the tier rows are never an agent's (S-14).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use matrix_sdk::ruma::{OwnedUserId, UserId};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::agents::approval_card::tier_word;
use crate::agents::copy::AgentPersonVm;
use crate::agents::mcp::{self, Hints, HostRules, McpEntry, Program, RawMcp, RawTier};
use crate::agents::run::SandboxTable;
use crate::platform::Platform;

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The Mac: it may start a `command` server (a phone never hosts) and it
/// offers its own screen (96.4).
pub const MAC: HostRules = HostRules {
    may_spawn: !cfg!(target_os = "ios"),
    screen: true,
};

/// What a server's answer reads before this Mac has asked it.
pub const NOT_ASKED: &str = "This Mac has not asked it yet: it asks once an agent signed in here has found its control room, and again every minute.";

/// What a server's answer reads after a save, until the host is built on
/// what was saved: what it heard before was of other settings.
pub const NOT_ASKED_SINCE_SAVE: &str =
    "This Mac asks it again once its host is built on what was saved, within seconds.";

/// What the sandbox's line reads before this Mac has probed it.
pub const NOT_PROBED: &str =
    "This Mac checks its sandbox when it hosts an agent: sign one in above.";

/// What the sandbox's line reads after a save, until the host has probed
/// what was saved.
pub const NOT_PROBED_SINCE_SAVE: &str =
    "This Mac probes it again once its host is built on what was saved, within seconds.";

/// Beside a `command` server's tier rows: the floor [`mcp::tier`] applies.
pub const COMMAND_FLOOR: &str = "Never below T2: a program keeper starts runs with this Mac's rights, so each of its tools asks before it changes anything.";

/// Beside a `role` server's tools: its role's table, which nobody edits.
pub const ROLE_TABLE: &str =
    "A role server's tools take its role's table; their tiers are not set here.";

/// Beside the tier rows a draft names that its role does not take: what
/// the person does about them before it can be saved.
pub const ROLE_DROPS: &str = "A role server's tools take its role's table, so these tier rows cannot be saved with it: drop them to save it with this role, or set the role back to keep them.";

/// Why a server whose token an earlier keeper kept under the server's own
/// name is not connected: that token is never read (R276).
pub const LEGACY_TOKEN: &str = "its token was kept by an earlier keeper in a way this one no longer reads: save its token again, or forget it";

/// The keychain key of the token saved as `generation`: its own namespace,
/// as [`crate::bots::provider_token_key`]'s. Every token a person saves is
/// a new generation, named only by the row that publishes it (R276).
pub fn token_key(generation: &str) -> String {
    format!("agent_mcp_bearer/{generation}")
}

/// Where an earlier keeper kept server `name`'s token: never read, and
/// deleted when the row naming it stops doing so.
fn legacy_key(name: &str) -> String {
    format!("agent_mcp_token/{name}")
}

/// A new generation: 128 random bits, as a `secret:<name>` names it.
fn generation() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// The bearer token `entry` is sent, from the keychain: `None` when it
/// names none. Its credential names the generation the tables it was read
/// from published, so no later save's token is ever this entry's. `Err` —
/// the token it names is not there, or the keychain cannot be read — is
/// why it is not connected: never sent without it.
pub fn bearer(platform: &dyn Platform, entry: &McpEntry) -> Result<Option<String>, String> {
    let Some(secret) = entry.credential.as_ref() else {
        return Ok(None);
    };
    match platform.keychain_get(&token_key(secret.name())) {
        Ok(Some(token)) if !token.is_empty() => Ok(Some(token)),
        Ok(_) => Err(format!(
            "{} is saved with a token that is not in this Mac's keychain: save a new one, or forget it.",
            entry.name
        )),
        Err(error) => Err(format!(
            "{}'s token could not be read from this Mac's keychain, so this Mac does not connect to it: {error}",
            entry.name
        )),
    }
}

// --- the view models -------------------------------------------------------

/// One `[[mcp.tier]]` row: a tool and its tier as written (`"T0"`…`"T5"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMcpRowVm {
    pub tool: String,
    pub tier: String,
}

/// A tier a row may set, as the cards name it (93.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMcpTierVm {
    /// `"T0"`…`"T5"`.
    pub code: String,
    pub word: String,
}

/// One tool of a server's sheet, as a save of the entry being edited would
/// offer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMcpToolVm {
    /// Its name exactly as listed — what a tier row names — when it travels
    /// as a function name; `None` when it cannot, and no row can name it.
    pub tool: Option<String>,
    /// Its name as the sheet shows it — its accessible name too: redacted
    /// and bounded as everything keeper writes of a server is (R225, R278),
    /// whether the server listed it or a row names it.
    pub shown: String,
    /// Its tier as the cards name it; `None` when it is not offered.
    pub word: Option<String>,
    /// Why it is not offered.
    pub refusal: Option<String>,
}

/// The program a `command` server was started as: the absolute path its
/// argv resolved to and the SHA-256 of the bytes started — what an
/// approval of its tools binds (R225).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMcpStartedVm {
    pub path: String,
    pub sha256: String,
}

/// One server of this Mac's, as Settings › Agents › *MCP servers* lists it
/// (UX-DR140). Never its token: only whether it is saved with one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMcpServerVm {
    pub name: String,
    pub url: Option<String>,
    /// The program and its arguments, element by element; empty for a
    /// `url` server.
    pub command: Vec<String>,
    pub role: Option<String>,
    pub fingerprint: Option<String>,
    /// Who reads what reaches it, by name; empty when anyone does.
    pub readers: Vec<AgentPersonVm>,
    /// Anyone reads what reaches it (no `readers`, or `"*"`).
    pub anyone: bool,
    pub trust_annotations: bool,
    /// It is saved with a bearer token, which is in the keychain.
    pub token: bool,
    /// The person's `[[mcp.tier]]` rows.
    pub rows: Vec<AgentMcpRowVm>,
    /// [`COMMAND_FLOOR`] for a `command` server.
    pub floor: Option<String>,
    /// [`ROLE_TABLE`] for a `role` server.
    pub fixed: Option<String>,
    /// It answered its last `tools/list` to a host built on these settings.
    pub answers: bool,
    /// *answers*, or *does not answer — <why>*.
    pub answer: String,
    /// The program a host built on these settings started it as; `None`
    /// for a `url` server and until it answers such a host.
    pub started: Option<AgentMcpStartedVm>,
    /// Why the stored entry is not offered: it no longer checks, or its
    /// token cannot be read.
    pub refusal: Option<String>,
}

/// Settings › Agents › *MCP servers*: every server and the tiers a row may
/// set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMcpListVm {
    pub servers: Vec<AgentMcpServerVm>,
    pub tiers: Vec<AgentMcpTierVm>,
}

/// Add or replace server `name`. `token` is a new bearer token (write-only;
/// `None` keeps the one saved), `forget_token` removes it and wins over a
/// `token` sent with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMcpServerReq {
    pub name: String,
    pub url: Option<String>,
    /// Empty for a `url` server; otherwise the argv, element by element,
    /// stored as given.
    pub command: Vec<String>,
    pub role: Option<String>,
    pub fingerprint: Option<String>,
    /// Empty: anyone reads what reaches it (`["*"]`, the grammar's default).
    pub readers: Vec<String>,
    pub trust_annotations: bool,
    pub rows: Vec<AgentMcpRowVm>,
    pub token: Option<String>,
    pub forget_token: bool,
}

/// A tier row a role does not take, as the sheet shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMcpConflictVm {
    /// The row's tool exactly as written: what dropping it matches.
    pub tool: String,
    /// Its name as the sheet shows it, redacted and bounded as a listed
    /// tool's is (R278).
    pub shown: String,
    pub tier: String,
}

/// What a server's sheet says for the entry as it is being edited
/// (UX-DR140): a program's floor, a role's fixed table, each tool with the
/// tier a save of it would give, and the tier rows its role does not take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMcpDraftVm {
    /// [`COMMAND_FLOOR`] for a `command` server.
    pub floor: Option<String>,
    /// [`ROLE_TABLE`] for a `role` server: its tiers are not set here.
    pub fixed: Option<String>,
    /// Every tool the server listed to a host built on what is saved, when
    /// the draft still reaches that server, then every tier row the draft
    /// keeps — each with its tier by the draft's role, rows and trust.
    pub tools: Vec<AgentMcpToolVm>,
    /// The draft's tier rows its role does not take: Save refuses them
    /// until the person drops them.
    pub conflicts: Vec<AgentMcpConflictVm>,
    /// [`ROLE_DROPS`] when there are such rows.
    pub drop: Option<String>,
}

/// One `[sandbox] env` variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSandboxEnvVm {
    pub name: String,
    pub path: String,
}

/// This Mac's `[sandbox]` table and what its probe found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSandboxVm {
    pub read_exec: Vec<String>,
    pub env: Vec<AgentSandboxEnvVm>,
    /// `sandbox-exec ok` or `unavailable — <why>` from a probe of this
    /// table, [`NOT_PROBED_SINCE_SAVE`] until the host has probed what was
    /// saved, or [`NOT_PROBED`].
    pub status: String,
    /// Why the stored table no longer checks.
    pub refusal: Option<String>,
}

/// Replace this Mac's `[sandbox]` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSandboxReq {
    pub read_exec: Vec<String>,
    pub env: Vec<AgentSandboxEnvVm>,
}

// --- the store -------------------------------------------------------------

fn store(what: &str) -> impl Fn(rusqlite::Error) -> String + '_ {
    move |error| format!("This Mac's agent settings could not be {what}: {error}")
}

fn open(data_dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("This Mac's data folder could not be made: {e}"))?;
    let conn = Connection::open(data_dir.join("keeper.db")).map_err(store("opened"))?;
    conn.busy_timeout(BUSY_TIMEOUT).map_err(store("opened"))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(store("opened"))?;
    // No foreign keys: rusqlite leaves `foreign_keys` off, so each write
    // replaces a server's child rows itself, in its transaction. `token`:
    // the row is saved with a token; `token_ref`: the generation of it the
    // row publishes (R276) — `NULL` with `token` set is an earlier keeper's.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_mcp_servers(\
            name TEXT PRIMARY KEY, \
            url TEXT, \
            role TEXT, \
            fingerprint TEXT, \
            trust_annotations INTEGER NOT NULL, \
            token INTEGER NOT NULL, \
            token_ref TEXT\
        );\
        CREATE TABLE IF NOT EXISTS agent_mcp_server_argv(\
            name TEXT NOT NULL, \
            position INTEGER NOT NULL, \
            arg TEXT NOT NULL, \
            PRIMARY KEY(name, position)\
        );\
        CREATE TABLE IF NOT EXISTS agent_mcp_server_readers(\
            name TEXT NOT NULL, \
            reader TEXT NOT NULL, \
            PRIMARY KEY(name, reader)\
        );\
        CREATE TABLE IF NOT EXISTS agent_mcp_server_tiers(\
            name TEXT NOT NULL, \
            tool TEXT NOT NULL, \
            tier TEXT NOT NULL, \
            PRIMARY KEY(name, tool)\
        );\
        CREATE TABLE IF NOT EXISTS agent_sandbox_read_exec(\
            position INTEGER PRIMARY KEY, \
            path TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS agent_sandbox_env(\
            name TEXT PRIMARY KEY, \
            path TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS agent_mac_revision(\
            id INTEGER PRIMARY KEY CHECK (id = 1), \
            revision INTEGER NOT NULL\
        );",
    )
    .map_err(store("opened"))?;
    // Tables an earlier keeper made have no `token_ref`. Another opening
    // may add it first: then it is there.
    if !has_token_ref(&conn)? {
        if let Err(error) =
            conn.execute_batch("ALTER TABLE agent_mcp_servers ADD COLUMN token_ref TEXT")
        {
            if !has_token_ref(&conn)? {
                return Err(store("opened")(error));
            }
        }
    }
    Ok(conn)
}

fn has_token_ref(conn: &Connection) -> Result<bool, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('agent_mcp_servers') WHERE name = 'token_ref'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .map_err(store("opened"))
}

/// The token a server's row publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Saved {
    None,
    /// The token saved as this generation, under [`token_key`].
    Generation(String),
    /// A token an earlier keeper kept under [`legacy_key`]: never read.
    Legacy,
}

impl Saved {
    fn of(token: bool, token_ref: Option<String>) -> Saved {
        match (token, token_ref) {
            (false, _) => Saved::None,
            (true, Some(generation)) => Saved::Generation(generation),
            (true, None) => Saved::Legacy,
        }
    }

    fn is_some(&self) -> bool {
        *self != Saved::None
    }

    /// The keychain key it names, for server `name`.
    fn key(&self, name: &str) -> Option<String> {
        match self {
            Saved::None => None,
            Saved::Generation(generation) => Some(token_key(generation)),
            Saved::Legacy => Some(legacy_key(name)),
        }
    }
}

/// Every stored server as written, by name, with `credential` left out,
/// and the token its row publishes. `between` runs after each table is
/// read.
fn raw_servers(
    conn: &Connection,
    between: &mut dyn FnMut(),
) -> Result<Vec<(RawMcp, Saved)>, String> {
    let read = store("read");
    let mut argv: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut readers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut tiers: BTreeMap<String, Vec<RawTier>> = BTreeMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT name, arg FROM agent_mcp_server_argv ORDER BY name, position")
            .map_err(&read)?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?)))
            .map_err(&read)?;
        for row in rows {
            let (name, arg) = row.map_err(&read)?;
            argv.entry(name).or_default().push(arg);
        }
        between();
        let mut stmt = conn
            .prepare("SELECT name, reader FROM agent_mcp_server_readers ORDER BY name, reader")
            .map_err(&read)?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?)))
            .map_err(&read)?;
        for row in rows {
            let (name, reader) = row.map_err(&read)?;
            readers.entry(name).or_default().push(reader);
        }
        between();
        let mut stmt = conn
            .prepare("SELECT name, tool, tier FROM agent_mcp_server_tiers ORDER BY name, tool")
            .map_err(&read)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(&read)?;
        for row in rows {
            let (name, tool, tier) = row.map_err(&read)?;
            tiers.entry(name).or_default().push(RawTier { tool, tier });
        }
        between();
    }
    let mut stmt = conn
        .prepare(
            "SELECT name, url, role, fingerprint, trust_annotations, token, token_ref FROM agent_mcp_servers ORDER BY name",
        )
        .map_err(&read)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, bool>(4)?,
                Saved::of(row.get(5)?, row.get(6)?),
            ))
        })
        .map_err(&read)?;
    let mut servers = Vec::new();
    for row in rows {
        let (name, url, role, fingerprint, trust_annotations, saved) = row.map_err(&read)?;
        servers.push((
            RawMcp {
                command: argv.remove(&name),
                readers: readers.remove(&name),
                tier: tiers.remove(&name).unwrap_or_default(),
                url,
                credential: None,
                role,
                fingerprint,
                trust_annotations,
                name,
            },
            saved,
        ));
    }
    between();
    Ok(servers)
}

/// `raw` with its credential — `secret:<generation>` naming the token its
/// row publishes — as the shared check reads it. `Err` for a token an
/// earlier keeper kept, which is never read ([`LEGACY_TOKEN`]).
fn credentialed(mut raw: RawMcp, saved: &Saved) -> Result<RawMcp, String> {
    raw.credential = match saved {
        Saved::None => None,
        Saved::Generation(generation) => Some(format!("secret:{generation}")),
        Saved::Legacy => {
            return Err(refused((
                format!("[[mcp]] \"{}\" `credential`", raw.name),
                LEGACY_TOKEN.to_owned(),
            )))
        }
    };
    Ok(raw)
}

/// `(at, why)` as the sentence a person reads.
fn refused((at, why): (String, String)) -> String {
    format!("{at} is refused: {why}")
}

/// Check one server alone, as this Mac reads it.
fn check_one(raw: RawMcp) -> Result<McpEntry, String> {
    // The Mac names no `[[kvm]]`: a `kvm:<id>` role is refused by the
    // check's own sentence.
    mcp::check(vec![raw], &[], MAC)
        .map(|mut checked| checked.remove(0))
        .map_err(refused)
}

/// A `[sandbox]` table as stored: its `read_exec` folders and its `env`
/// rows.
type SandboxRows = (Vec<String>, Vec<(String, String)>);

fn sandbox_rows(conn: &Connection, between: &mut dyn FnMut()) -> Result<SandboxRows, String> {
    let read = store("read");
    let mut stmt = conn
        .prepare("SELECT path FROM agent_sandbox_read_exec ORDER BY position")
        .map_err(&read)?;
    let read_exec = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(&read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(&read)?;
    between();
    let mut stmt = conn
        .prepare("SELECT name, path FROM agent_sandbox_env ORDER BY name")
        .map_err(&read)?;
    let env = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?)))
        .map_err(&read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(&read)?;
    Ok((read_exec, env))
}

/// Everything stored, as one save left it.
#[derive(Debug)]
struct Stored {
    revision: i64,
    /// Each server as written, and the token its row publishes.
    servers: Vec<(RawMcp, Saved)>,
    sandbox: SandboxRows,
}

/// The store now, from one SQLite snapshot (R228): its revision and every
/// table, so a save made meanwhile is seen whole or not at all.
fn snapshot(data_dir: &Path) -> Result<Stored, String> {
    snapshot_with(data_dir, &mut || {})
}

/// [`snapshot`], running `between` after each of its reads.
fn snapshot_with(data_dir: &Path, between: &mut dyn FnMut()) -> Result<Stored, String> {
    let read = store("read");
    let mut conn = open(data_dir)?;
    // A deferred transaction: under WAL its first read takes the snapshot
    // every later read in it sees, and a save meanwhile is not blocked.
    let tx = conn.transaction().map_err(&read)?;
    let revision = tx
        .query_row(
            "SELECT revision FROM agent_mac_revision WHERE id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(&read)?
        .unwrap_or(0);
    between();
    let servers = raw_servers(&tx, between)?;
    let sandbox = sandbox_rows(&tx, between)?;
    tx.commit().map_err(&read)?;
    Ok(Stored {
        revision,
        servers,
        sandbox,
    })
}

/// The one check of a `[sandbox]` table ([`SandboxTable::check`]), over
/// rows: a variable is named once, as a TOML table names it.
fn check_sandbox(
    read_exec: Vec<String>,
    env: Vec<(String, String)>,
) -> Result<SandboxTable, String> {
    let mut named = BTreeMap::new();
    for (name, path) in env {
        if named.insert(name.clone(), path).is_some() {
            return Err(refused((
                format!("[sandbox] env `{name}`"),
                "another row names this variable".to_owned(),
            )));
        }
    }
    SandboxTable::check(read_exec, named).map_err(refused)
}

/// What this Mac's host is built from (96.2 #11, R213): its servers that
/// check, each naming the generation of the token its row publishes, and
/// its `[sandbox]` table or why it does not check, all of one revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacTables {
    pub mcp: Vec<McpEntry>,
    pub sandbox: Result<SandboxTable, String>,
    /// The store's revision: every save makes it greater, so a host built
    /// on an older one is built again.
    pub revision: i64,
}

impl Default for MacTables {
    fn default() -> Self {
        MacTables {
            mcp: Vec::new(),
            sandbox: Ok(SandboxTable::default()),
            revision: 0,
        }
    }
}

/// This Mac's tables now, from one snapshot. A server that no longer
/// checks is left out and its listing says why; a store that cannot be
/// read offers no server and no sandbox, saying why.
pub fn read(data_dir: &Path) -> MacTables {
    let stored = match snapshot(data_dir) {
        Ok(stored) => stored,
        Err(sentence) => {
            tracing::warn!(%sentence, "agents: this Mac's MCP servers and sandbox table could not be read");
            return MacTables {
                sandbox: Err(sentence),
                ..MacTables::default()
            };
        }
    };
    let mcp = stored
        .servers
        .into_iter()
        .filter_map(|(raw, saved)| match credentialed(raw, &saved).and_then(check_one) {
            Ok(entry) => Some(entry),
            Err(sentence) => {
                tracing::warn!(%sentence, "agents: a server of this Mac's no longer checks; it is not offered");
                None
            }
        })
        .collect();
    let (read_exec, env) = stored.sandbox;
    MacTables {
        mcp,
        sandbox: check_sandbox(read_exec, env),
        revision: stored.revision,
    }
}

/// A write, one at a time: the write lock is held from the first read of
/// what it replaces to its commit, the keychain's step included.
fn begin_write(conn: &mut Connection) -> Result<Transaction<'_>, String> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(store("saved"))
}

/// Make the store's revision greater, in the write's transaction.
fn bump(tx: &Transaction<'_>) -> Result<(), String> {
    tx.execute(
        "INSERT INTO agent_mac_revision(id, revision) VALUES (1, 1) \
         ON CONFLICT(id) DO UPDATE SET revision = revision + 1",
        [],
    )
    .map(|_| ())
    .map_err(store("saved"))
}

/// The token server `name`'s row publishes; `None` when there is no such
/// server.
fn saved_token(tx: &Transaction<'_>, name: &str) -> Result<Option<Saved>, String> {
    tx.query_row(
        "SELECT token, token_ref FROM agent_mcp_servers WHERE name = ?1",
        params![name],
        |row| Ok(Saved::of(row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(store("saved"))
}

fn replace_server(tx: &Transaction<'_>, raw: &RawMcp, saved: &Saved) -> Result<(), String> {
    let write = store("saved");
    delete_server(tx, &raw.name)?;
    let token_ref = match saved {
        Saved::Generation(generation) => Some(generation.as_str()),
        Saved::None | Saved::Legacy => None,
    };
    tx.execute(
        "INSERT INTO agent_mcp_servers(name, url, role, fingerprint, trust_annotations, token, token_ref) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            raw.name,
            raw.url,
            raw.role,
            raw.fingerprint,
            raw.trust_annotations,
            token_ref.is_some() || *saved == Saved::Legacy,
            token_ref
        ],
    )
    .map_err(&write)?;
    for (position, arg) in raw.command.iter().flatten().enumerate() {
        tx.execute(
            "INSERT INTO agent_mcp_server_argv(name, position, arg) VALUES (?1, ?2, ?3)",
            params![raw.name, position as i64, arg],
        )
        .map_err(&write)?;
    }
    for reader in raw.readers.iter().flatten() {
        tx.execute(
            "INSERT INTO agent_mcp_server_readers(name, reader) VALUES (?1, ?2)",
            params![raw.name, reader],
        )
        .map_err(&write)?;
    }
    for row in &raw.tier {
        tx.execute(
            "INSERT INTO agent_mcp_server_tiers(name, tool, tier) VALUES (?1, ?2, ?3)",
            params![raw.name, row.tool, row.tier],
        )
        .map_err(&write)?;
    }
    Ok(())
}

fn delete_server(tx: &Transaction<'_>, name: &str) -> Result<usize, String> {
    let write = store("saved");
    for table in [
        "agent_mcp_server_argv",
        "agent_mcp_server_readers",
        "agent_mcp_server_tiers",
    ] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE name = ?1"),
            params![name],
        )
        .map_err(&write)?;
    }
    tx.execute(
        "DELETE FROM agent_mcp_servers WHERE name = ?1",
        params![name],
    )
    .map_err(&write)
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

/// `req` as a `[[mcp]]` entry is written, `credential` left out; its argv
/// exactly as sent.
fn raw_of(req: AgentMcpServerReq) -> RawMcp {
    RawMcp {
        url: non_empty(req.url),
        command: (!req.command.is_empty()).then_some(req.command),
        credential: None,
        readers: (!req.readers.is_empty()).then_some(req.readers),
        role: non_empty(req.role),
        fingerprint: non_empty(req.fingerprint),
        trust_annotations: req.trust_annotations,
        tier: req
            .rows
            .into_iter()
            .map(|row| RawTier {
                tool: row.tool,
                tier: row.tier,
            })
            .collect(),
        name: req.name,
    }
}

/// What a sheet says beside `raw`'s tier rows: the floor of a program
/// keeper starts ([`mcp::tier`]'s), a role's table nobody edits.
fn presentation(raw: &RawMcp) -> (Option<String>, Option<String>) {
    (
        raw.command.is_some().then(|| COMMAND_FLOOR.to_owned()),
        raw.role.is_some().then(|| ROLE_TABLE.to_owned()),
    )
}

/// One tool of a draft: its tier as `entry` — the draft as it would be
/// saved, `None` while it does not check — gives it, and why not, as
/// `said` writes it.
fn draft_tool(
    entry: Option<&McpEntry>,
    tool: &str,
    shown: String,
    hints: Option<Hints>,
    said: &dyn Fn(&str) -> String,
) -> AgentMcpToolVm {
    let tier = entry.map(|entry| mcp::tier(entry, tool, hints));
    AgentMcpToolVm {
        tool: Some(tool.to_owned()),
        shown,
        word: tier
            .as_ref()
            .and_then(|tier| tier.as_ref().ok())
            .map(|tier| tier_word(tier.as_u8())),
        refusal: tier.and_then(Result::err).map(|why| said(&why)),
    }
}

/// What the sheet says for `req` as it is being edited, before it is saved
/// (UX-DR140, R276): the floor and fixed table a saved server's listing
/// gives; every tool the saved server listed to the running host — `hosted`,
/// shown only when built on the stored tables — while the draft still
/// reaches it at the same URL or as the same program, then every tier row
/// the draft keeps, each with the tier its role, rows and trust would give;
/// and the rows a role does not take, which Save refuses until the person
/// drops them. Every name and refusal composed here — a row's tool, the
/// refusal a role gives a listed tool — is written as `said` writes it:
/// the host's `keeper_agent::mcp::diagnostic`, as `hosted`'s own names
/// and refusals already are (R278). A tool's exact name stays in
/// [`AgentMcpToolVm::tool`] and [`AgentMcpConflictVm::tool`], for matching.
pub fn draft(
    data_dir: &Path,
    hosted: Option<Hosted<'_>>,
    req: AgentMcpServerReq,
    said: &dyn Fn(&str) -> String,
) -> Result<AgentMcpDraftVm, String> {
    let raw = raw_of(req);
    let (floor, fixed) = presentation(&raw);
    let conflicts: Vec<AgentMcpConflictVm> = if raw.role.is_some() {
        raw.tier
            .iter()
            .map(|row| AgentMcpConflictVm {
                tool: row.tool.clone(),
                shown: said(&row.tool),
                tier: row.tier.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let kept = RawMcp {
        tier: if raw.role.is_some() {
            Vec::new()
        } else {
            raw.tier.clone()
        },
        ..raw.clone()
    };
    let entry = check_one(kept.clone()).ok();
    let stored = snapshot(data_dir)?;
    let same_server = stored.servers.iter().any(|(saved, _)| {
        saved.name == raw.name && saved.url == raw.url && saved.command == raw.command
    });
    let listed = hosted
        .filter(|hosted| same_server && hosted.revision == stored.revision)
        .and_then(|hosted| match hosted.heard.get(&raw.name) {
            Some(Heard::Answers { tools, .. }) => Some(tools.as_slice()),
            _ => None,
        })
        .unwrap_or_default();
    let mut tools: Vec<AgentMcpToolVm> = listed
        .iter()
        .map(|heard| match &heard.tool {
            Ok(tool) => draft_tool(entry.as_ref(), tool, heard.shown.clone(), heard.hints, said),
            Err(why) => AgentMcpToolVm {
                tool: None,
                shown: heard.shown.clone(),
                word: None,
                refusal: Some(why.clone()),
            },
        })
        .collect();
    for row in &kept.tier {
        if !tools
            .iter()
            .any(|tool| tool.tool.as_deref() == Some(row.tool.as_str()))
        {
            tools.push(draft_tool(
                entry.as_ref(),
                &row.tool,
                said(&row.tool),
                None,
                said,
            ));
        }
    }
    Ok(AgentMcpDraftVm {
        floor,
        fixed,
        tools,
        drop: (!conflicts.is_empty()).then(|| ROLE_DROPS.to_owned()),
        conflicts,
    })
}

/// Delete the token staged for a save that did not commit. No row names
/// it, so a delete the keychain refuses leaves a token nothing sends.
fn discard(platform: &dyn Platform, key: &str) {
    if let Err(error) = platform.keychain_delete(key) {
        tracing::warn!(%error, "agents: a token staged for a save that did not complete is still in this Mac's keychain; no row names it, so it is never sent");
    }
}

/// Add or replace `req`'s server, checked by the one `[[mcp]]` check
/// ([`mcp::check`]) before anything is written. A new token goes to the
/// keychain as a new generation, never to `keeper.db`; a `command` server
/// keeps none (R220); `forget_token` removes it, whatever token came with
/// it. `Err` is the sentence a person reads: a refusal published nothing,
/// and a retired token the keychain would not delete is said so — no row
/// names it.
pub fn save_server(
    data_dir: &Path,
    platform: &dyn Platform,
    req: AgentMcpServerReq,
) -> Result<(), String> {
    save_server_with(data_dir, platform, req, |tx| tx.commit())
}

/// [`save_server`], committing with `commit`.
fn save_server_with(
    data_dir: &Path,
    platform: &dyn Platform,
    req: AgentMcpServerReq,
    commit: impl FnOnce(Transaction<'_>) -> rusqlite::Result<()>,
) -> Result<(), String> {
    let forget = req.forget_token;
    // Forgetting is the person's last word on the token (UX-DR140).
    let token = if forget {
        None
    } else {
        non_empty(req.token.clone())
    };
    let raw = raw_of(req);
    let mut conn = open(data_dir)?;
    let tx = begin_write(&mut conn)?;
    let prior = saved_token(&tx, &raw.name)?.unwrap_or(Saved::None);
    let kept = match &token {
        Some(_) => Saved::Generation(generation()),
        None if forget || raw.command.is_some() => Saved::None,
        None => prior.clone(),
    };
    check_one(credentialed(raw.clone(), &kept)?)?;
    replace_server(&tx, &raw, &kept)?;
    bump(&tx)?;
    // The token under its own generation's key before the row naming it is
    // committed: until then nothing names that key, and afterwards only
    // this row does.
    let staged = match (&token, &kept) {
        (Some(token), Saved::Generation(generation)) => {
            let key = token_key(generation);
            if let Err(error) = platform.keychain_set(&key, token) {
                discard(platform, &key);
                return Err(format!(
                    "{} was not saved: its token could not be kept in this Mac's keychain: {error}",
                    raw.name
                ));
            }
            Some(key)
        }
        _ => None,
    };
    if let Err(error) = commit(tx) {
        if let Some(key) = &staged {
            discard(platform, key);
        }
        return Err(store("saved")(error));
    }
    // Only what this save retired: a later save's token has a key of its
    // own.
    if prior != kept {
        if let Some(retired) = prior.key(&raw.name) {
            platform.keychain_delete(&retired).map_err(|error| {
                format!(
                    "{} was saved, but the token it no longer uses is still in this Mac's keychain, where nothing names it: {error}",
                    raw.name
                )
            })?;
        }
    }
    Ok(())
}

/// Remove server `name`, then the token its row named. `Err` is the
/// sentence a person reads: no such server, or a token the keychain would
/// not delete — the server is gone either way, and no row names that
/// token.
pub fn remove_server(data_dir: &Path, platform: &dyn Platform, name: &str) -> Result<(), String> {
    let mut conn = open(data_dir)?;
    let tx = begin_write(&mut conn)?;
    let Some(prior) = saved_token(&tx, name)? else {
        return Err(format!("This Mac has no MCP server named {name}."));
    };
    delete_server(&tx, name)?;
    bump(&tx)?;
    tx.commit().map_err(store("saved"))?;
    if let Some(retired) = prior.key(name) {
        platform.keychain_delete(&retired).map_err(|error| {
            format!("{name} was removed, but its token is still in this Mac's keychain, where nothing names it: {error}")
        })?;
    }
    Ok(())
}

/// Replace this Mac's `[sandbox]` table with `req`'s, checked by the one
/// `[sandbox]` check before anything is written. `Err` is the sentence a
/// person reads, and nothing changed.
pub fn save_sandbox(data_dir: &Path, req: AgentSandboxReq) -> Result<(), String> {
    let env: Vec<(String, String)> = req
        .env
        .into_iter()
        .map(|row| (row.name, row.path))
        .collect();
    check_sandbox(req.read_exec.clone(), env.clone())?;
    let write = store("saved");
    let mut conn = open(data_dir)?;
    let tx = begin_write(&mut conn)?;
    tx.execute("DELETE FROM agent_sandbox_read_exec", [])
        .map_err(&write)?;
    tx.execute("DELETE FROM agent_sandbox_env", [])
        .map_err(&write)?;
    for (position, path) in req.read_exec.iter().enumerate() {
        tx.execute(
            "INSERT INTO agent_sandbox_read_exec(position, path) VALUES (?1, ?2)",
            params![position as i64, path],
        )
        .map_err(&write)?;
    }
    for (name, path) in &env {
        tx.execute(
            "INSERT INTO agent_sandbox_env(name, path) VALUES (?1, ?2)",
            params![name, path],
        )
        .map_err(&write)?;
    }
    bump(&tx)?;
    tx.commit().map_err(&write)
}

// --- what Settings shows ---------------------------------------------------

/// One listed tool as the running host found it, as Settings may show it:
/// whatever the server wrote reaches Settings redacted and bounded, as the
/// host's status writes it (R225, R276).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeardTool {
    /// Its name exactly as listed when it travels as a function name — a
    /// tier row's key, ≤ [`mcp::WIRE_MAX`] bytes of `[A-Za-z0-9_-]` — or,
    /// redacted and bounded, why it cannot.
    pub tool: Result<String, String>,
    /// Its name redacted and bounded.
    pub shown: String,
    /// What its annotations hint, read only when the person trusts it.
    pub hints: Option<Hints>,
}

/// What the running host last heard from a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// It answered: the program its connection started (a `command`
    /// server's) and what it listed.
    Answers {
        started: Option<Program>,
        tools: Vec<HeardTool>,
    },
    /// It did not answer: why.
    Silent(String),
}

/// What the running host heard, and the revision of the tables it was
/// built on: what it heard is shown only beside those tables.
#[derive(Debug, Clone, Copy)]
pub struct Hosted<'a> {
    pub revision: i64,
    pub heard: &'a BTreeMap<String, Heard>,
}

/// The tiers a row may set, as the cards name them.
pub fn tier_choices() -> Vec<AgentMcpTierVm> {
    (0..=5)
        .map(|n| AgentMcpTierVm {
            code: format!("T{n}"),
            word: tier_word(n),
        })
        .collect()
}

/// Everyone the stored servers' readers name, for a display-name lookup.
pub fn people(data_dir: &Path) -> Vec<OwnedUserId> {
    let servers = snapshot(data_dir)
        .map(|stored| stored.servers)
        .unwrap_or_default();
    let people: BTreeSet<OwnedUserId> = servers
        .iter()
        .flat_map(|(raw, _)| raw.readers.iter().flatten())
        .filter_map(|reader| OwnedUserId::try_from(reader.as_str()).ok())
        .collect();
    people.into_iter().collect()
}

/// Settings › Agents › *MCP servers* (UX-DR140): each stored server as
/// written, from one snapshot, and whether it answers a host built on these
/// very tables. `hosted` is the running host's, `None` while this Mac hosts
/// nothing; `name` a person's display name. What each lists, with its
/// tiers, is its sheet's ([`draft`]).
pub fn listing(
    data_dir: &Path,
    platform: &dyn Platform,
    hosted: Option<Hosted<'_>>,
    name: &dyn Fn(&UserId) -> Option<String>,
) -> Result<AgentMcpListVm, String> {
    let stored = snapshot(data_dir)?;
    let since_save = hosted.is_some_and(|hosted| hosted.revision != stored.revision);
    let heard = hosted
        .filter(|hosted| hosted.revision == stored.revision)
        .map(|hosted| hosted.heard);
    let servers = stored
        .servers
        .into_iter()
        .map(|(raw, saved)| {
            let checked = credentialed(raw.clone(), &saved)
                .and_then(check_one)
                .and_then(|entry| bearer(platform, &entry).map(|_| entry));
            let answer = match heard {
                Some(heard) => Answered::Heard(heard.get(&raw.name)),
                None if since_save => Answered::SinceSave,
                None => Answered::Heard(None),
            };
            server_vm(raw, saved.is_some(), checked, answer, name)
        })
        .collect();
    Ok(AgentMcpListVm {
        servers,
        tiers: tier_choices(),
    })
}

/// What a listing says a server answered.
enum Answered<'a> {
    /// What the host built on the listed tables heard: `None`, not asked.
    Heard(Option<&'a Heard>),
    /// The host was built on other tables: it asks again once rebuilt.
    SinceSave,
}

fn server_vm(
    raw: RawMcp,
    token: bool,
    checked: Result<McpEntry, String>,
    answered: Answered<'_>,
    name: &dyn Fn(&UserId) -> Option<String>,
) -> AgentMcpServerVm {
    let readers = raw.readers.clone().unwrap_or_default();
    let anyone = readers.is_empty() || readers.iter().any(|reader| reader == "*");
    let person = |reader: &String| AgentPersonVm {
        display_name: <&UserId>::try_from(reader.as_str()).ok().and_then(name),
        matrix_id: reader.clone(),
    };
    let (floor, fixed) = presentation(&raw);
    let (answers, answer, started) = match (&checked, answered) {
        (Err(_), _) => (
            false,
            "does not answer — this Mac does not ask a server it does not offer".to_owned(),
            None,
        ),
        (Ok(_), Answered::SinceSave) => (
            false,
            format!("does not answer — {NOT_ASKED_SINCE_SAVE}"),
            None,
        ),
        (Ok(_), Answered::Heard(None)) => (false, format!("does not answer — {NOT_ASKED}"), None),
        (Ok(_), Answered::Heard(Some(Heard::Silent(why)))) => {
            (false, format!("does not answer — {why}"), None)
        }
        (Ok(_), Answered::Heard(Some(Heard::Answers { started, .. }))) => (
            true,
            "answers".to_owned(),
            started.as_ref().map(|program| AgentMcpStartedVm {
                path: program.path.clone(),
                sha256: program.sha256.clone(),
            }),
        ),
    };
    AgentMcpServerVm {
        readers: if anyone {
            Vec::new()
        } else {
            readers.iter().map(person).collect()
        },
        anyone,
        url: raw.url,
        command: raw.command.unwrap_or_default(),
        role: raw.role,
        fingerprint: raw.fingerprint,
        trust_annotations: raw.trust_annotations,
        token,
        rows: raw
            .tier
            .into_iter()
            .map(|row| AgentMcpRowVm {
                tool: row.tool,
                tier: row.tier,
            })
            .collect(),
        floor,
        fixed,
        answers,
        answer,
        started,
        refusal: checked.err(),
        name: raw.name,
    }
}

/// This Mac's `[sandbox]` table as stored, and the line a probe of it
/// wrote: `probed` is the running host's line and the revision it was
/// built on, `None` while it hosts nothing. A line probed of other tables
/// is not shown.
pub fn sandbox_listing(
    data_dir: &Path,
    probed: Option<(i64, &str)>,
) -> Result<AgentSandboxVm, String> {
    let stored = snapshot(data_dir)?;
    let (read_exec, env) = stored.sandbox;
    let refusal = check_sandbox(read_exec.clone(), env.clone()).err();
    let status = match probed {
        None => NOT_PROBED,
        Some((revision, line)) if revision == stored.revision => line,
        Some(_) => NOT_PROBED_SINCE_SAVE,
    };
    Ok(AgentSandboxVm {
        read_exec,
        env: env
            .into_iter()
            .map(|(name, path)| AgentSandboxEnvVm { name, path })
            .collect(),
        status: status.to_owned(),
        refusal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;
    use crate::forges::testing::FakePlatform;
    use crate::vm::NotifyTarget;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    fn req(name: &str) -> AgentMcpServerReq {
        AgentMcpServerReq {
            name: name.to_owned(),
            url: Some("https://notes.example.org/mcp".to_owned()),
            command: Vec::new(),
            role: None,
            fingerprint: None,
            readers: Vec::new(),
            trust_annotations: false,
            rows: Vec::new(),
            token: None,
            forget_token: false,
        }
    }

    fn row(tool: &str, tier: &str) -> AgentMcpRowVm {
        AgentMcpRowVm {
            tool: tool.to_owned(),
            tier: tier.to_owned(),
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    /// What the one `[[mcp]]` check makes of `toml`, as agentd.toml.
    fn as_agentd_reads(toml: &str) -> Vec<McpEntry> {
        #[derive(Deserialize)]
        struct File {
            mcp: Vec<RawMcp>,
        }
        let file: File = toml::from_str(toml).expect("toml");
        mcp::check(file.mcp, &[], MAC).expect("checks")
    }

    /// Every byte `keeper.db` holds, its write-ahead log included.
    fn db_bytes(dir: &Path) -> Vec<u8> {
        ["keeper.db", "keeper.db-wal"]
            .iter()
            .flat_map(|file| std::fs::read(dir.join(file)).unwrap_or_default())
            .collect()
    }

    /// Every token the keychain holds, sorted: whatever key it is under.
    fn held(p: &FakePlatform) -> Vec<String> {
        let mut held: Vec<String> = p
            .keychain
            .lock()
            .expect("keychain")
            .values()
            .cloned()
            .collect();
        held.sort();
        held
    }

    /// The keychain key server `name`'s row names, as a read publishes it.
    fn key_of(dir: &Path, name: &str) -> String {
        let tables = read(dir);
        let entry = tables
            .mcp
            .iter()
            .find(|entry| entry.name == name)
            .expect(name);
        token_key(entry.credential.as_ref().expect("a token").name())
    }

    /// Each server of `tables` as the host connects it: where it is
    /// reached, and the bearer it is sent or why it is not connected.
    fn pairs(p: &dyn Platform, tables: &MacTables) -> Vec<(String, Result<Option<String>, ()>)> {
        tables
            .mcp
            .iter()
            .map(|entry| {
                let at = match &entry.transport {
                    mcp::McpTransport::Url(url) => url.clone(),
                    mcp::McpTransport::Command(argv) => argv.join(" "),
                };
                (at, bearer(p, entry).map_err(|_| ()))
            })
            .collect()
    }

    type Hook = Box<dyn FnMut(&FakePlatform) + Send>;

    /// A keychain that can be made to fail each of its three calls, records
    /// every delete it is asked for, and runs `after_set` once a token is
    /// written and `before_delete` (once) before a delete — a slow keychain,
    /// during which another reader or writer acts.
    #[derive(Default)]
    struct Keychain {
        inner: FakePlatform,
        fail_get: AtomicBool,
        fail_set: AtomicBool,
        fail_delete: AtomicBool,
        deletes: Mutex<Vec<String>>,
        after_set: Mutex<Option<Hook>>,
        before_delete: Mutex<Option<Hook>>,
    }

    impl Keychain {
        fn dir(&self) -> PathBuf {
            self.inner.data_dir.clone()
        }

        fn deleted(&self) -> Vec<String> {
            self.deletes.lock().expect("deletes").clone()
        }
    }

    fn locked(what: &str) -> CoreError {
        CoreError::Internal(format!("the keychain is locked ({what})"))
    }

    impl Platform for Keychain {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            self.inner.data_dir()
        }
        fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
            if self.fail_set.load(Ordering::SeqCst) {
                return Err(locked("set"));
            }
            self.inner.keychain_set(key, value)?;
            if let Some(hook) = self.after_set.lock().expect("after_set").as_mut() {
                hook(&self.inner);
            }
            Ok(())
        }
        fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
            if self.fail_get.load(Ordering::SeqCst) {
                return Err(locked("get"));
            }
            self.inner.keychain_get(key)
        }
        fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
            self.deletes.lock().expect("deletes").push(key.to_owned());
            let hook = self.before_delete.lock().expect("before_delete").take();
            if let Some(mut hook) = hook {
                hook(&self.inner);
            }
            if self.fail_delete.load(Ordering::SeqCst) {
                return Err(locked("delete"));
            }
            self.inner.keychain_delete(key)
        }
        fn open_url(&self, url: &str) -> Result<(), CoreError> {
            self.inner.open_url(url)
        }
        fn notify(&self, title: &str, body: &str, target: &NotifyTarget) -> Result<(), CoreError> {
            self.inner.notify(title, body, target)
        }
        fn sidecar_path(&self, name: &str) -> Result<PathBuf, CoreError> {
            self.inner.sidecar_path(name)
        }
        fn exclude_from_backup(&self, path: &Path) -> Result<(), CoreError> {
            self.inner.exclude_from_backup(path)
        }
        fn set_badge_count(&self, count: Option<u32>) -> Result<(), CoreError> {
            self.inner.set_badge_count(count)
        }
    }

    /// 96.2 #11: the Mac's servers survive a reopen as exactly what the one
    /// `[[mcp]]` check makes of the same entries in agentd.toml, argv
    /// element by element; the token is the keychain's and never a byte of
    /// `keeper.db`; a save replaces a server's rows, a kept token stays, a
    /// forgotten one goes, and a removed server is gone with its token.
    #[test]
    fn agent_mcp_servers_round_trip() {
        let p = FakePlatform::default();
        let dir = p.data_dir.clone();
        assert_eq!(read(&dir), MacTables::default());

        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                readers: strings(&["@tgorka:example.org"]),
                trust_annotations: true,
                rows: vec![row("search", "T0"), row("delete", "T4")],
                token: Some("tok-sekret-1".to_owned()),
                ..req("notes")
            },
        )
        .expect("url server");
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                url: None,
                command: strings(&["/opt/bin/kid", "", "  two  ", "a\nb"]),
                ..req("kid")
            },
        )
        .expect("command server");

        let tables = read(&dir);
        let generation = tables.mcp[1]
            .credential
            .as_ref()
            .expect("a token")
            .name()
            .to_owned();
        assert_eq!(
            tables.mcp,
            as_agentd_reads(&format!(
                "[[mcp]]\nname = \"kid\"\ncommand = [\"/opt/bin/kid\", \"\", \"  two  \", \"a\\nb\"]\n\n\
                 [[mcp]]\nname = \"notes\"\nurl = \"https://notes.example.org/mcp\"\n\
                 credential = \"secret:{generation}\"\nreaders = [\"@tgorka:example.org\"]\n\
                 trust_annotations = true\n\
                 [[mcp.tier]]\ntool = \"delete\"\ntier = \"T4\"\n\
                 [[mcp.tier]]\ntool = \"search\"\ntier = \"T0\"\n"
            ))
        );
        assert_eq!(
            bearer(&p, &tables.mcp[1]).as_ref().map(Option::as_deref),
            Ok(Some("tok-sekret-1"))
        );
        assert_eq!(bearer(&p, &tables.mcp[0]), Ok(None));
        let bytes = db_bytes(&dir);
        assert!(!bytes.is_empty());
        assert!(
            !bytes.windows(12).any(|window| window == b"tok-sekret-1"),
            "the token is the keychain's, never keeper.db's"
        );

        // A save replaces the rows and keeps the token it was not given.
        save_server(&dir, &p, req("notes")).expect("re-save");
        let notes = read(&dir).mcp.remove(1);
        assert!(
            notes.tiers.is_empty() && !notes.trust_annotations,
            "{notes:?}"
        );
        assert_eq!(
            notes.credential.as_ref().map(|secret| secret.name()),
            Some(generation.as_str())
        );
        assert_eq!(held(&p), ["tok-sekret-1"]);

        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                forget_token: true,
                ..req("notes")
            },
        )
        .expect("forget");
        assert!(held(&p).is_empty());
        assert_eq!(read(&dir).mcp[1].credential, None);

        // A url server that becomes a program keeps no token (R220).
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                token: Some("tok-2".to_owned()),
                ..req("notes")
            },
        )
        .expect("token again");
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                url: None,
                command: strings(&["/opt/bin/notes"]),
                ..req("notes")
            },
        )
        .expect("now a program");
        assert!(held(&p).is_empty());

        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                token: Some("tok-3".to_owned()),
                ..req("gone")
            },
        )
        .expect("third");
        remove_server(&dir, &p, "gone").expect("remove");
        assert!(held(&p).is_empty());
        let names: Vec<String> = read(&dir).mcp.into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["kid", "notes"]);
        assert!(remove_server(&dir, &p, "gone").is_err());

        // A token left in the keychain under a name no row is saved with
        // is never picked up by a server added under that name.
        for key in [token_key("gone"), legacy_key("gone")] {
            p.keychain_set(&key, "tok-left").expect("keychain");
        }
        save_server(&dir, &p, req("gone")).expect("added again");
        let gone = read(&dir).mcp.remove(0);
        assert_eq!((gone.name.as_str(), gone.credential), ("gone", None));
    }

    /// 96.2 #2 on the Mac: every refusal is the one check's sentence, and a
    /// refused save writes nothing — not the row, not the token, not the
    /// revision.
    #[test]
    fn a_refused_server_is_not_stored() {
        let p = FakePlatform::default();
        let dir = p.data_dir.clone();
        save_server(&dir, &p, req("kept")).expect("kept");
        let before = read(&dir);
        let both = AgentMcpServerReq {
            command: strings(&["/bin/x"]),
            ..req("x")
        };
        let cases: Vec<(AgentMcpServerReq, &str)> = vec![
            (both, "exactly one of `url` and `command`"),
            (
                AgentMcpServerReq {
                    url: None,
                    ..req("x")
                },
                "exactly one of `url` and `command`",
            ),
            (req("a__b"), "never `__`"),
            (
                AgentMcpServerReq {
                    url: Some("ftp://h/x".to_owned()),
                    ..req("x")
                },
                "http:// or https://",
            ),
            (
                AgentMcpServerReq {
                    role: Some("kvm:desk".to_owned()),
                    ..req("x")
                },
                "names [[kvm]] id \"desk\", and there is none",
            ),
            (
                AgentMcpServerReq {
                    role: Some("boss".to_owned()),
                    ..req("x")
                },
                "is not a role",
            ),
            (
                AgentMcpServerReq {
                    role: Some("paseo".to_owned()),
                    rows: vec![row("list_agents", "T0")],
                    ..req("x")
                },
                "a role's tiers are fixed",
            ),
            (
                AgentMcpServerReq {
                    rows: vec![row("t", "T9")],
                    ..req("x")
                },
                "is not a tier",
            ),
            (
                AgentMcpServerReq {
                    rows: vec![row("t", "T1"), row("t", "T2")],
                    ..req("x")
                },
                "another [[mcp.tier]] row names this tool",
            ),
            (
                AgentMcpServerReq {
                    fingerprint: Some("sha256:abc".to_owned()),
                    ..req("x")
                },
                "sha256: and 64 hex digits",
            ),
            (
                AgentMcpServerReq {
                    readers: strings(&["tgorka"]),
                    ..req("x")
                },
                "is not a Matrix user id",
            ),
            (
                AgentMcpServerReq {
                    url: None,
                    command: strings(&["/bin/x"]),
                    token: Some("tok-x".to_owned()),
                    ..req("x")
                },
                "a program you start reads its own secrets",
            ),
        ];
        for (case, said) in cases {
            let sentence = save_server(&dir, &p, case.clone()).expect_err(said);
            assert!(sentence.contains(said), "{case:?}: {sentence}");
            assert!(sentence.contains(" is refused: "), "{sentence}");
        }
        assert_eq!(read(&dir), before);
        assert!(held(&p).is_empty());

        // The screen is the Mac's: its role reads here as it never does on
        // agentd.
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                role: Some("screen".to_owned()),
                ..req("screen")
            },
        )
        .expect("the Mac's screen");
    }

    /// R96MM-01: a read is one snapshot. A save that lands between the
    /// tables a read reads — a server's tiers and readers replaced, its
    /// URL moved, the sandbox's folders and variables replaced — is seen
    /// whole or not at all, by the host's read and by a listing alike.
    #[test]
    fn a_read_sees_a_concurrent_save_whole_or_not_at_all() {
        let p = FakePlatform::default();
        let dir = p.data_dir.clone();
        let version = |n: usize| {
            (
                AgentMcpServerReq {
                    url: Some(format!("https://notes.example.org/v{n}")),
                    readers: if n == 1 {
                        strings(&["@tgorka:example.org"])
                    } else {
                        strings(&["@marta:example.org"])
                    },
                    rows: if n == 1 {
                        vec![row("search", "T0")]
                    } else {
                        vec![row("delete", "T4")]
                    },
                    ..req("notes")
                },
                AgentSandboxReq {
                    read_exec: vec![format!("/opt/v{n}/bin")],
                    env: vec![AgentSandboxEnvVm {
                        name: "CARGO_HOME".to_owned(),
                        path: format!("/opt/v{n}/cargo"),
                    }],
                },
            )
        };
        let save = |n: usize| {
            let (server, sandbox) = version(n);
            save_server(&dir, &p, server).expect("server");
            save_sandbox(&dir, sandbox).expect("sandbox");
        };
        // `RawMcp` has no `PartialEq`: a read is compared by all it holds.
        let whole = |stored: Stored| format!("{stored:?}");
        save(1);
        let first = snapshot(&dir).expect("first");
        save(2);
        let second = snapshot(&dir).expect("second");
        assert!(
            format!("{:?}", first.servers) != format!("{:?}", second.servers)
                && first.sandbox != second.sandbox
        );

        // At every point between two of the read's own reads, the other
        // version is saved: the read is all of what was before, or all of
        // what is after.
        for at in 0..6 {
            save(1);
            let before = whole(snapshot(&dir).expect("before"));
            let mut step = 0;
            let read = whole(
                snapshot_with(&dir, &mut || {
                    if step == at {
                        save(2);
                    }
                    step += 1;
                })
                .expect("read"),
            );
            let after = whole(snapshot(&dir).expect("after"));
            assert!(
                step > at,
                "the read has {step} steps; a save at step {at} never ran"
            );
            assert_ne!(before, after);
            assert!(
                read == before || read == after,
                "a save at step {at} was read in part: {read:?}"
            );
        }
    }

    /// One server as the host connects it: where, and the bearer or not.
    type Pair = (String, Result<Option<String>, ()>);

    fn pair(at: &str, token: &str) -> Pair {
        (at.to_owned(), Ok(Some(token.to_owned())))
    }

    /// R96MM2-01, R276: a token is chosen by the rows that publish it. A
    /// host's read taken before a save moves the server and gives it a new
    /// token connects — while the keychain writes, once the save committed
    /// and after it — to the old URL with the old token or not at all,
    /// never with the new one; a read after the commit pairs the new URL
    /// with the new token. A rotation alone makes a new revision; a
    /// keychain that refuses the write leaves row, revision and token as
    /// they were.
    #[test]
    fn a_read_sends_only_the_token_its_rows_published() {
        let p = Keychain::default();
        let dir = p.dir();
        let (a, b) = (
            "https://notes.example.org/mcp",
            "https://elsewhere.example.org/mcp",
        );
        let save = |url: &str, token: &str| {
            save_server(
                &dir,
                &p,
                AgentMcpServerReq {
                    url: Some(url.to_owned()),
                    token: Some(token.to_owned()),
                    ..req("notes")
                },
            )
        };
        save(a, "tok-1").expect("first token");
        let old = read(&dir);
        assert_eq!(pairs(&p, &old), [pair(a, "tok-1")]);

        // What the old read and a read now connect to, at each point.
        type Seen = std::sync::Arc<Mutex<Vec<(&'static str, Vec<Pair>, Vec<Pair>)>>>;
        let seen = Seen::default();
        let look = |when: &'static str| -> Hook {
            let (seen, old, dir) = (std::sync::Arc::clone(&seen), old.clone(), dir.clone());
            Box::new(move |keychain: &FakePlatform| {
                seen.lock().expect("seen").push((
                    when,
                    pairs(keychain, &old),
                    pairs(keychain, &read(&dir)),
                ));
            })
        };
        *p.after_set.lock().expect("after_set") = Some(look("the keychain wrote"));
        *p.before_delete.lock().expect("before_delete") = Some(look("the save committed"));
        save(b, "tok-2").expect("moved with a new token");
        *p.after_set.lock().expect("after_set") = None;
        seen.lock().expect("seen").push((
            "the save returned",
            pairs(&p, &old),
            pairs(&p, &read(&dir)),
        ));
        assert_eq!(
            *seen.lock().expect("seen"),
            [
                (
                    "the keychain wrote",
                    vec![pair(a, "tok-1")],
                    vec![pair(a, "tok-1")]
                ),
                (
                    "the save committed",
                    vec![pair(a, "tok-1")],
                    vec![pair(b, "tok-2")]
                ),
                (
                    "the save returned",
                    vec![(a.to_owned(), Err(()))],
                    vec![pair(b, "tok-2")]
                ),
            ]
        );

        // A rotation with nothing else changed is a new revision, and a
        // new entry: only its credential differs.
        let before = read(&dir);
        save(b, "tok-3").expect("rotated");
        let after = read(&dir);
        assert!(after.revision > before.revision);
        assert_ne!(after.mcp, before.mcp);
        let without = |tables: &MacTables| {
            let mut entry = tables.mcp[0].clone();
            entry.credential = None;
            entry
        };
        assert_eq!(without(&after), without(&before), "only the token changed");
        assert_eq!(pairs(&p, &after), [pair(b, "tok-3")]);

        // A refused keychain write changes nothing: not the row, not the
        // revision, not the token — for a rotation and for a new server.
        p.fail_set.store(true, Ordering::SeqCst);
        let refused = save(a, "tok-4").expect_err("keychain refuses");
        assert!(refused.contains("was not saved"), "{refused}");
        assert!(save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                token: Some("tok-new".to_owned()),
                ..req("new")
            },
        )
        .is_err());
        p.fail_set.store(false, Ordering::SeqCst);
        assert_eq!(read(&dir), after);
        assert_eq!(held(&p.inner), ["tok-3"]);
    }

    /// R96MM2-01, R276: a save that does not commit publishes nothing. Its
    /// token was staged under a key no row names, so reads taken before and
    /// after it pair the URL with the old token — and still do when the
    /// keychain then refuses to delete what was staged: a token left so is
    /// never sent. A delete the keychain allows leaves nothing behind.
    #[test]
    fn a_save_that_does_not_commit_publishes_nothing() {
        let p = Keychain::default();
        let dir = p.dir();
        let a = "https://notes.example.org/mcp";
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                token: Some("tok-1".to_owned()),
                ..req("notes")
            },
        )
        .expect("first token");
        let before = read(&dir);
        for (refuse_delete, left) in [(false, vec!["tok-1"]), (true, vec!["tok-1", "tok-2"])] {
            p.fail_delete.store(refuse_delete, Ordering::SeqCst);
            let said = save_server_with(
                &dir,
                &p,
                AgentMcpServerReq {
                    url: Some("https://elsewhere.example.org/mcp".to_owned()),
                    token: Some("tok-2".to_owned()),
                    ..req("notes")
                },
                |_| Err(rusqlite::Error::InvalidQuery),
            )
            .expect_err("not committed");
            p.fail_delete.store(false, Ordering::SeqCst);
            assert!(said.contains("could not be saved"), "{said}");
            assert_eq!(read(&dir), before);
            assert_eq!(pairs(&p, &before), [pair(a, "tok-1")]);
            assert_eq!(pairs(&p, &read(&dir)), [pair(a, "tok-1")]);
            assert_eq!(held(&p.inner), left, "the delete refused: {refuse_delete}");
        }
    }

    /// R96MM2-02, R276: an operation deletes only the token it retired. A
    /// Forget, a Remove and a URL made a program each commit; then, before
    /// their delete reaches the keychain, a newer save gives the server a
    /// new token. The newer row's token survives, and it is the one sent.
    #[test]
    fn an_older_forget_or_remove_never_deletes_a_newer_token() {
        type Older = Box<dyn Fn(&Path, &dyn Platform) -> Result<(), String>>;
        let p = Keychain::default();
        let dir = p.dir();
        let cases: Vec<(&str, Older)> = vec![
            (
                "forget",
                Box::new(|dir, p| {
                    save_server(
                        dir,
                        p,
                        AgentMcpServerReq {
                            forget_token: true,
                            ..req("notes")
                        },
                    )
                }),
            ),
            ("remove", Box::new(|dir, p| remove_server(dir, p, "notes"))),
            (
                "made a program",
                Box::new(|dir, p| {
                    save_server(
                        dir,
                        p,
                        AgentMcpServerReq {
                            url: None,
                            command: strings(&["/opt/bin/notes"]),
                            ..req("notes")
                        },
                    )
                }),
            ),
        ];
        for (n, (what, older)) in cases.into_iter().enumerate() {
            let new = format!("tok-new-{n}");
            save_server(
                &dir,
                &p,
                AgentMcpServerReq {
                    token: Some(format!("tok-old-{n}")),
                    ..req("notes")
                },
            )
            .expect(what);
            {
                let (dir, new) = (dir.clone(), new.clone());
                *p.before_delete.lock().expect("before_delete") =
                    Some(Box::new(move |keychain: &FakePlatform| {
                        save_server(
                            &dir,
                            keychain,
                            AgentMcpServerReq {
                                token: Some(new.clone()),
                                ..req("notes")
                            },
                        )
                        .expect("the newer save");
                    }));
            }
            older(&dir, &p).expect(what);
            assert_eq!(
                pairs(&p, &read(&dir)),
                [pair("https://notes.example.org/mcp", &new)],
                "{what}"
            );
            assert_eq!(held(&p.inner), [new], "{what}: only the retired token went");
        }
    }

    /// R276's migration: a token an earlier keeper kept under its server's
    /// name is never read. Its server is not connected and its listing
    /// says why; saving it without a token is refused, saying so, until a
    /// token is saved again or forgotten — either deletes the old one.
    #[test]
    fn a_token_an_earlier_keeper_kept_is_refused_until_saved_again() {
        let p = FakePlatform::default();
        let dir = p.data_dir.clone();
        std::fs::create_dir_all(&dir).expect("dir");
        Connection::open(dir.join("keeper.db"))
            .and_then(|conn| {
                conn.execute_batch(
                    "CREATE TABLE agent_mcp_servers(name TEXT PRIMARY KEY, url TEXT, role TEXT, \
                     fingerprint TEXT, trust_annotations INTEGER NOT NULL, token INTEGER NOT NULL);\
                     INSERT INTO agent_mcp_servers VALUES ('notes', 'https://notes.example.org/mcp', NULL, NULL, 0, 1);\
                     INSERT INTO agent_mcp_servers VALUES ('wiki', 'https://wiki.example.org/mcp', NULL, NULL, 0, 1);",
                )
            })
            .expect("an earlier keeper's table");
        for name in ["notes", "wiki"] {
            p.keychain_set(&legacy_key(name), &format!("tok-{name}-earlier"))
                .expect("keychain");
        }
        assert!(read(&dir).mcp.is_empty(), "never connected");
        let listed = listing(&dir, &p, None, &|_| None).expect("listing");
        assert_eq!(listed.servers.len(), 2);
        for server in &listed.servers {
            assert!(server.token && !server.answers, "{server:?}");
            assert!(
                server
                    .refusal
                    .as_deref()
                    .is_some_and(|why| why.contains(LEGACY_TOKEN)),
                "{server:?}"
            );
        }
        let said = save_server(&dir, &p, req("notes")).expect_err("refused until saved again");
        assert!(said.contains(LEGACY_TOKEN), "{said}");

        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                token: Some("tok-notes-now".to_owned()),
                ..req("notes")
            },
        )
        .expect("saved again");
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                url: Some("https://wiki.example.org/mcp".to_owned()),
                forget_token: true,
                ..req("wiki")
            },
        )
        .expect("forgotten");
        assert_eq!(
            pairs(&p, &read(&dir)),
            [
                pair("https://notes.example.org/mcp", "tok-notes-now"),
                ("https://wiki.example.org/mcp".to_owned(), Ok(None)),
            ]
        );
        assert_eq!(held(&p), ["tok-notes-now"]);
    }

    /// R96MM-04: a keychain that cannot be read is an error, never "no
    /// token": the bearer is refused and the listing says why, while a
    /// server saved without a token reads on. Forget and Remove delete the
    /// token without reading it first; a delete the keychain refuses is
    /// said, after the row stopped naming the token. A row saved with a
    /// token the keychain does not hold fails closed the same way.
    #[test]
    fn keychain_errors_are_errors() {
        let p = Keychain::default();
        let dir = p.dir();
        for name in ["notes", "wiki", "docs"] {
            save_server(
                &dir,
                &p,
                AgentMcpServerReq {
                    token: Some(format!("tok-{name}")),
                    ..req(name)
                },
            )
            .expect(name);
        }
        save_server(&dir, &p, req("plain")).expect("plain");
        let no_names = |_: &UserId| None;
        let by_name = |list: &AgentMcpListVm, name: &str| {
            list.servers
                .iter()
                .find(|server| server.name == name)
                .expect(name)
                .clone()
        };

        let retired = [key_of(&dir, "notes"), key_of(&dir, "wiki")];
        p.fail_get.store(true, Ordering::SeqCst);
        let tables = read(&dir);
        let entry = |name: &str| {
            tables
                .mcp
                .iter()
                .find(|entry| entry.name == name)
                .expect(name)
        };
        assert!(bearer(&p, entry("notes")).is_err());
        assert_eq!(bearer(&p, entry("plain")), Ok(None));
        let listed = listing(&dir, &p, None, &no_names).expect("listing");
        let notes = by_name(&listed, "notes");
        assert!(notes.token && !notes.answers, "{notes:?}");
        assert!(notes.refusal.is_some(), "{notes:?}");
        assert_eq!(by_name(&listed, "plain").refusal, None);

        // Forget and Remove go to the keychain without reading it.
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                forget_token: true,
                ..req("notes")
            },
        )
        .expect("forgotten without a read");
        remove_server(&dir, &p, "wiki").expect("removed without a read");
        assert_eq!(p.deleted(), retired, "each delete was asked for");
        p.fail_get.store(false, Ordering::SeqCst);
        assert_eq!(held(&p.inner), ["tok-docs"]);

        // A delete the keychain refuses is said; the row is gone and no
        // row names the token left behind.
        p.fail_delete.store(true, Ordering::SeqCst);
        let said = remove_server(&dir, &p, "docs").expect_err("delete refused");
        assert!(said.contains("still in this Mac's keychain"), "{said}");
        p.fail_delete.store(false, Ordering::SeqCst);
        assert!(read(&dir).mcp.iter().all(|entry| entry.name != "docs"));

        // The control: a row saved with a token the keychain does not hold.
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                token: Some("tok-plain".to_owned()),
                ..req("plain")
            },
        )
        .expect("token");
        p.inner
            .keychain_delete(&key_of(&dir, "plain"))
            .expect("gone behind keeper's back");
        let plain = read(&dir)
            .mcp
            .into_iter()
            .find(|entry| entry.name == "plain")
            .expect("plain");
        assert!(bearer(&p, &plain).is_err());
        let listed = listing(&dir, &p, None, &no_names).expect("listing");
        assert!(by_name(&listed, "plain").refusal.is_some());
    }

    /// R96MM-05: a token typed into the sheet and *Forget the token*
    /// ticked after it: the token is forgotten, the typed one never kept.
    #[test]
    fn forgetting_wins_over_a_typed_token() {
        let p = FakePlatform::default();
        let dir = p.data_dir.clone();
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                token: Some("tok-old".to_owned()),
                ..req("notes")
            },
        )
        .expect("token");
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                token: Some("tok-typed".to_owned()),
                forget_token: true,
                ..req("notes")
            },
        )
        .expect("forget");
        assert!(held(&p).is_empty());
        assert_eq!(read(&dir).mcp[0].credential, None);
    }

    /// R213, R148 on the Mac: the stored `[sandbox]` table reads back as
    /// the one `[sandbox]` check makes it; a refused save (and a variable
    /// named twice, which a TOML table cannot say) writes nothing.
    #[test]
    fn the_mac_sandbox_table_round_trip() {
        let p = FakePlatform::default();
        let dir = p.data_dir.clone();
        assert_eq!(read(&dir).sandbox, Ok(SandboxTable::default()));
        let env = |pairs: &[(&str, &str)]| -> Vec<AgentSandboxEnvVm> {
            pairs
                .iter()
                .map(|(name, path)| AgentSandboxEnvVm {
                    name: (*name).to_owned(),
                    path: (*path).to_owned(),
                })
                .collect()
        };
        let table = AgentSandboxReq {
            read_exec: strings(&["/opt/homebrew/bin", "/Users/tg/.cargo/bin"]),
            env: env(&[
                ("RUSTUP_HOME", "/Users/tg/.rustup"),
                ("CARGO_HOME", "/Users/tg/.cargo"),
            ]),
        };
        save_sandbox(&dir, table.clone()).expect("table");
        let tables = read(&dir);
        assert_eq!(
            tables.sandbox,
            SandboxTable::check(
                table.read_exec.clone(),
                BTreeMap::from([
                    ("CARGO_HOME".to_owned(), "/Users/tg/.cargo".to_owned()),
                    ("RUSTUP_HOME".to_owned(), "/Users/tg/.rustup".to_owned()),
                ])
            )
            .map_err(refused)
        );
        assert_eq!(
            sandbox_listing(&dir, None).expect("listing").read_exec,
            table.read_exec
        );

        for (refused, said) in [
            (
                AgentSandboxReq {
                    read_exec: strings(&["bin"]),
                    env: Vec::new(),
                },
                "is not an absolute path",
            ),
            (
                AgentSandboxReq {
                    read_exec: Vec::new(),
                    env: env(&[("KEEPER_X", "/x")]),
                },
                "keeper sets this variable itself",
            ),
            (
                AgentSandboxReq {
                    read_exec: Vec::new(),
                    env: env(&[("A", "/a"), ("A", "/b")]),
                },
                "another row names this variable",
            ),
        ] {
            let sentence = save_sandbox(&dir, refused).expect_err(said);
            assert!(sentence.contains(said), "{sentence}");
        }
        assert_eq!(read(&dir), tables);

        save_sandbox(
            &dir,
            AgentSandboxReq {
                read_exec: Vec::new(),
                env: Vec::new(),
            },
        )
        .expect("emptied");
        assert_eq!(read(&dir).sandbox, Ok(SandboxTable::default()));
    }

    /// R96MM-07: what the host probed is shown beside the table it probed.
    /// After a save its line is no longer shown until a host built on the
    /// saved table probes it; nothing hosted, nothing probed.
    #[test]
    fn the_sandbox_line_is_of_the_table_it_probed() {
        let p = FakePlatform::default();
        let dir = p.data_dir.clone();
        save_sandbox(
            &dir,
            AgentSandboxReq {
                read_exec: strings(&["/opt/homebrew/bin"]),
                env: Vec::new(),
            },
        )
        .expect("table");
        let probed_on = read(&dir).revision;
        let line = "sandbox-exec ok";
        assert_eq!(
            sandbox_listing(&dir, Some((probed_on, line)))
                .expect("probed")
                .status,
            line
        );
        save_sandbox(
            &dir,
            AgentSandboxReq {
                read_exec: strings(&["/opt/homebrew/bin", "/Users/tg/.cargo/bin"]),
                env: Vec::new(),
            },
        )
        .expect("saved again");
        assert_eq!(
            sandbox_listing(&dir, Some((probed_on, line)))
                .expect("not probed since")
                .status,
            NOT_PROBED_SINCE_SAVE
        );
        assert_eq!(
            sandbox_listing(&dir, None).expect("idle").status,
            NOT_PROBED
        );
    }

    /// UX-DR140: each server as written, whether a host built on the listed
    /// tables heard it answer, a program's floor beside its rows, a role's
    /// fixed table, readers by name, a stored entry that no longer checks
    /// with the check's own sentence — and never a token. After a save,
    /// what the host heard of the settings before is not shown: a URL moved
    /// never answers as the old one did.
    #[test]
    fn the_listing_says_what_the_host_heard() {
        let p = FakePlatform::default();
        let dir = p.data_dir.clone();
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                token: Some("tok-sekret-2".to_owned()),
                ..req("notes")
            },
        )
        .expect("notes");
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                url: None,
                command: strings(&["/opt/bin/kid"]),
                readers: strings(&["@marta:example.org"]),
                rows: vec![row("echo", "T0")],
                ..req("kid")
            },
        )
        .expect("kid");
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                role: Some("paseo".to_owned()),
                ..req("paseo")
            },
        )
        .expect("paseo");
        // A row written by an older keeper that no longer checks.
        open(&dir)
            .and_then(|conn| {
                conn.execute_batch(
                    "INSERT INTO agent_mcp_servers(name, url, role, fingerprint, trust_annotations, token) \
                     VALUES ('odd', 'https://odd.example.org', NULL, NULL, 0, 0);\
                     INSERT INTO agent_mcp_server_argv VALUES ('odd', 0, '/bin/odd');",
                )
                .map_err(store("written"))
            })
            .expect("odd row");
        let tables = read(&dir);
        assert_eq!(tables.mcp.len(), 3, "a refused row is never offered");

        let heard = BTreeMap::from([
            (
                "notes".to_owned(),
                Heard::Answers {
                    started: None,
                    tools: vec![HeardTool {
                        tool: Ok("search".to_owned()),
                        shown: "search".to_owned(),
                        hints: None,
                    }],
                },
            ),
            (
                "kid".to_owned(),
                Heard::Silent("it did not answer within 10 s".to_owned()),
            ),
        ]);
        let hosted = Hosted {
            revision: tables.revision,
            heard: &heard,
        };
        let marta =
            |user: &UserId| (user.as_str() == "@marta:example.org").then(|| "Marta".to_owned());
        let listed = listing(&dir, &p, Some(hosted), &marta).expect("listing");
        let by = |list: &AgentMcpListVm, name: &str| {
            list.servers
                .iter()
                .find(|s| s.name == name)
                .expect(name)
                .clone()
        };

        let notes = by(&listed, "notes");
        assert!(notes.answers);
        assert!(notes.anyone && notes.readers.is_empty() && notes.token);
        assert_eq!((notes.floor.clone(), notes.fixed.clone()), (None, None));

        let kid = by(&listed, "kid");
        assert!(!kid.answers);
        assert!(kid.floor.is_some() && kid.fixed.is_none());
        assert_eq!(kid.command, ["/opt/bin/kid"]);
        assert_eq!(kid.rows, vec![row("echo", "T0")]);
        assert!(!kid.anyone && !kid.token);
        assert_eq!(kid.readers[0].display_name.as_deref(), Some("Marta"));

        let paseo = by(&listed, "paseo");
        assert!(paseo.fixed.is_some() && paseo.floor.is_none());
        assert!(!paseo.answers);

        let odd = by(&listed, "odd");
        assert!(!odd.answers);
        assert!(
            odd.refusal
                .as_deref()
                .is_some_and(|why| why.contains("exactly one of `url` and `command`")),
            "{odd:?}"
        );

        assert_eq!(listed.tiers.len(), 6);
        assert_eq!(
            listed.tiers[2],
            AgentMcpTierVm {
                code: "T2".to_owned(),
                word: tier_word(2)
            }
        );
        let json = serde_json::to_string(&listed).expect("json");
        assert!(!json.contains("tok-sekret-2"), "never a token: {json}");

        // notes moves elsewhere: what the host heard of the old URL is not
        // shown beside the new one until a host built on it asks.
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                url: Some("https://elsewhere.example.org/mcp".to_owned()),
                ..req("notes")
            },
        )
        .expect("moved");
        let moved = listing(&dir, &p, Some(hosted), &marta).expect("after the save");
        let notes = by(&moved, "notes");
        assert!(!notes.answers, "{notes:?}");
        assert!(moved.servers.iter().all(|s| !s.answers));

        // Nothing hosted: every server says so.
        let idle = listing(&dir, &p, None, &marta).expect("idle");
        assert!(idle.servers.iter().all(|s| !s.answers));
        assert_eq!(
            people(&dir),
            [OwnedUserId::try_from("@marta:example.org").expect("user")]
        );
    }

    /// Q10, R225: a program server is listed as the host started it — the
    /// absolute path its argv resolved to and the SHA-256 of its bytes —
    /// beside the argv as written; only while it answers a host built on
    /// the listed tables, never a program heard of other settings.
    #[test]
    fn a_program_is_listed_as_it_was_started() {
        let p = FakePlatform::default();
        let dir = p.data_dir.clone();
        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                url: None,
                command: strings(&["peekaboo", "mcp"]),
                role: Some("screen".to_owned()),
                ..req("screen")
            },
        )
        .expect("screen");
        let tables = read(&dir);
        let started = Program {
            path: "/opt/homebrew/bin/peekaboo".to_owned(),
            sha256: "5f".repeat(32),
        };
        let answers = BTreeMap::from([(
            "screen".to_owned(),
            Heard::Answers {
                started: Some(started.clone()),
                tools: Vec::new(),
            },
        )]);
        let nobody = |_: &UserId| None;
        let screen = |heard: &BTreeMap<String, Heard>, revision| {
            let hosted = Hosted { revision, heard };
            listing(&dir, &p, Some(hosted), &nobody)
                .expect("listing")
                .servers[0]
                .clone()
        };

        let listed = screen(&answers, tables.revision);
        assert_eq!(
            listed.started,
            Some(AgentMcpStartedVm {
                path: started.path.clone(),
                sha256: started.sha256.clone(),
            })
        );
        assert_eq!(listed.command, ["peekaboo", "mcp"], "the argv as written");

        let silent = BTreeMap::from([("screen".to_owned(), Heard::Silent("gone".to_owned()))]);
        assert_eq!(screen(&silent, tables.revision).started, None);

        save_server(
            &dir,
            &p,
            AgentMcpServerReq {
                url: None,
                command: strings(&["/opt/homebrew/bin/peekaboo", "mcp"]),
                role: Some("screen".to_owned()),
                ..req("screen")
            },
        )
        .expect("saved again");
        assert_eq!(
            screen(&answers, tables.revision).started,
            None,
            "a program started for other settings is not shown"
        );
    }

    /// One tool of a draft as the sheet shows it: its row key, its name, its
    /// tier's word, and whether it is refused.
    type Shown = (Option<String>, String, Option<String>, bool);

    fn shown(vm: &AgentMcpDraftVm) -> Vec<Shown> {
        vm.tools
            .iter()
            .map(|tool| {
                (
                    tool.tool.clone(),
                    tool.shown.clone(),
                    tool.word.clone(),
                    tool.refusal.is_some(),
                )
            })
            .collect()
    }

    fn offered(tool: &str, tier: u8) -> Shown {
        (
            Some(tool.to_owned()),
            tool.to_owned(),
            Some(tier_word(tier)),
            false,
        )
    }

    fn refused_tool(tool: &str) -> Shown {
        (Some(tool.to_owned()), tool.to_owned(), None, true)
    }

    fn untravelled(shown: &str) -> Shown {
        (None, shown.to_owned(), None, true)
    }

    /// What a server answered: `names`, then one whose name cannot travel.
    fn listed(names: &[&str]) -> Heard {
        Heard::Answers {
            started: None,
            tools: names
                .iter()
                .map(|name| HeardTool {
                    tool: Ok((*name).to_owned()),
                    shown: (*name).to_owned(),
                    hints: None,
                })
                .chain(std::iter::once(HeardTool {
                    tool: Err("`get file` cannot travel as a function name".to_owned()),
                    shown: "get file".to_owned(),
                    hints: None,
                }))
                .collect(),
        }
    }

    /// R96MM-09, R96MM2-04, R276: the sheet describes the entry being
    /// edited as a save of it would be. An ordinary server with tier rows
    /// made a paseo server: every tool it listed takes paseo's table —
    /// `search`, which paseo has not, is not offered — and its rows are the
    /// draft's conflicts, which Save refuses until they are dropped;
    /// dropped, it saves as a role server with no row. Made ordinary again,
    /// its tools take the rows and keeper's rule, and it saves. A URL made a
    /// program: what the URL listed is not shown, its rows take the
    /// program's floor, and it saves without its token. What a host built
    /// on other tables heard is no draft's.
    #[test]
    fn the_draft_says_what_will_be_saved() {
        let p = FakePlatform::default();
        let dir = p.data_dir.clone();
        let notes = AgentMcpServerReq {
            rows: vec![row("search", "T0"), row("delete", "T4")],
            token: Some("tok-notes".to_owned()),
            ..req("notes")
        };
        save_server(&dir, &p, notes.clone()).expect("notes");
        let heard = BTreeMap::from([(
            "notes".to_owned(),
            listed(&["search", "delete", "list_agents"]),
        )]);
        let draft_now = |req: AgentMcpServerReq| {
            let hosted = Hosted {
                revision: read(&dir).revision,
                heard: &heard,
            };
            draft(&dir, Some(hosted), req, &str::to_owned).expect("draft")
        };

        let same = draft_now(AgentMcpServerReq {
            token: None,
            ..notes.clone()
        });
        assert_eq!((same.floor.clone(), same.fixed.clone()), (None, None));
        assert_eq!(
            shown(&same),
            [
                offered("search", 0),
                offered("delete", 4),
                offered("list_agents", 3),
                untravelled("get file"),
            ]
        );
        assert!(same.conflicts.is_empty() && same.drop.is_none());

        // Made a paseo server with its rows still set.
        let role = AgentMcpServerReq {
            role: Some("paseo".to_owned()),
            token: None,
            ..notes.clone()
        };
        let as_role = draft_now(role.clone());
        assert_eq!(as_role.fixed.as_deref(), Some(ROLE_TABLE));
        assert_eq!(
            shown(&as_role),
            [
                refused_tool("search"),
                refused_tool("delete"),
                offered("list_agents", 0),
                untravelled("get file"),
            ]
        );
        let conflicts: Vec<(&str, &str)> = as_role
            .conflicts
            .iter()
            .map(|row| (row.tool.as_str(), row.tier.as_str()))
            .collect();
        let rows: Vec<(&str, &str)> = notes
            .rows
            .iter()
            .map(|row| (row.tool.as_str(), row.tier.as_str()))
            .collect();
        assert_eq!(conflicts, rows);
        assert_eq!(as_role.drop.as_deref(), Some(ROLE_DROPS));
        let said = save_server(&dir, &p, role.clone()).expect_err("rows not dropped");
        assert!(said.contains("a role's tiers are fixed"), "{said}");
        let dropped = AgentMcpServerReq {
            rows: Vec::new(),
            ..role
        };
        let ready = draft_now(dropped.clone());
        assert!(ready.conflicts.is_empty() && ready.drop.is_none());
        save_server(&dir, &p, dropped.clone()).expect("saved as a role server");
        let saved = read(&dir);
        assert_eq!(saved.mcp[0].role, Some(mcp::McpRole::Paseo));
        assert!(saved.mcp[0].tiers.is_empty());

        // The paseo server made ordinary again.
        let ordinary = AgentMcpServerReq {
            role: None,
            ..dropped
        };
        let back = draft_now(ordinary.clone());
        assert_eq!(back.fixed, None);
        assert_eq!(
            shown(&back),
            [
                offered("search", 3),
                offered("delete", 3),
                offered("list_agents", 3),
                untravelled("get file"),
            ]
        );
        assert!(back.conflicts.is_empty());
        save_server(&dir, &p, ordinary.clone()).expect("saved as ordinary");
        assert_eq!(read(&dir).mcp[0].role, None);

        // The URL made a program.
        let program = AgentMcpServerReq {
            url: None,
            command: strings(&["/opt/bin/notes"]),
            rows: vec![row("search", "T0")],
            ..ordinary
        };
        let as_program = draft_now(program.clone());
        assert_eq!(as_program.floor.as_deref(), Some(COMMAND_FLOOR));
        assert_eq!(shown(&as_program), [offered("search", 2)]);
        save_server(&dir, &p, program.clone()).expect("saved as a program");
        assert!(held(&p).is_empty());
        assert_eq!(
            pairs(&p, &read(&dir)),
            [("/opt/bin/notes".to_owned(), Ok(None))]
        );

        // Heard by a host built on these tables, and by one built before.
        let echo = BTreeMap::from([("notes".to_owned(), listed(&["echo"]))]);
        let revision = read(&dir).revision;
        let program = AgentMcpServerReq {
            rows: Vec::new(),
            ..program
        };
        let at = |revision| {
            let hosted = Hosted {
                revision,
                heard: &echo,
            };
            shown(&draft(&dir, Some(hosted), program.clone(), &str::to_owned).expect("draft"))
        };
        assert_eq!(at(revision), [offered("echo", 3), untravelled("get file")]);
        assert!(at(revision - 1).is_empty());
    }
}
