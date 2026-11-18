// ─── 109.27: the drive a write lands in ──────────────────────────────────────

/// Why this device cannot carry out a write to a drive (contract § 7.3's
/// device reasons; [`crate::hq::approvals::write`] words them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveAbsent {
    /// No synced folder here declares the drive (`_drive.toml`).
    NotKept,
    /// The folder here only pulls: nothing is committed from it.
    PullsOnly,
    /// The folder's removable disk is not attached.
    NotAttached,
}

/// What a write does at a path: changes the file there, or makes one where
/// nothing is (`keeper_sync::files_write::AgentTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveTarget {
    Existing,
    New,
}

/// A path of a drive as this device's checkout holds it: the key the drive's
/// own path check answered (`agent_write_check`), and — for an existing file
/// — its git blob id at `HEAD` and its bytes and blob id on the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFile {
    pub key: String,
    pub head: Option<String>,
    pub disk: Option<(String, Vec<u8>)>,
}

/// One commit of exactly a write (contract § 5.2), over the keys
/// [`DriveWrites::target`] answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveCommit {
    /// Each file's new bytes; `None` deletes it.
    pub writes: Vec<(String, Option<Vec<u8>>)>,
    pub moves: Vec<(String, String)>,
    /// The blob id each path was read at; `None` where nothing was.
    pub guards: Vec<(String, Option<String>)>,
    pub subject: String,
    /// The approval the commit carries out: its `Approval-Record` trailer.
    pub approval: String,
}

/// What a [`DriveCommit`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveCommitted {
    /// One commit holds it; its id.
    Committed(String),
    /// Everything already was as asked: no commit.
    Unchanged,
    /// A guarded path is not what was read: nothing changed.
    Guarded(String),
}

/// The drives this device keeps, as a person's write lands in them
/// (109.27): its one adapter is the shell's (`hq_apply.rs`, over
/// `keeper_sync`). Decides nothing — every rule, order and sentence is
/// [`crate::hq::approvals::write`]'s; this answers what the checkout holds,
/// with keeper-sync's own path rule, and makes the commit. Blocking.
pub trait DriveWrites: Send + Sync {
    /// Whether this device keeps `drive` where a write can be committed.
    fn keeps(&self, drive: &str) -> Result<(), DriveAbsent>;
    /// Where `path` of `drive` may be written, and what is there, by
    /// `agent_write_check` over this checkout with an agent host's scope;
    /// its refusal mapped as [`crate::agents::drive_write::Refusal`] maps it.
    fn target(
        &self,
        drive: &str,
        path: &str,
        target: DriveTarget,
    ) -> Result<DriveFile, crate::agents::drive_write::Refusal>;
    /// Commit `commit` into `drive`'s checkout as one authored commit
    /// (`Engine::commit_paths`); an error is its sentence.
    fn commit(&self, drive: &str, commit: DriveCommit) -> Result<DriveCommitted, String>;
    /// The commit on `drive`'s branch that carries out `approval` (its
    /// `Approval-Record` trailer), the whole history walked by the commit
    /// graph, once the folder's authored-commit record is settled; an error
    /// is its sentence.
    fn carried(&self, drive: &str, approval: &str) -> Result<Option<String>, String>;
}
