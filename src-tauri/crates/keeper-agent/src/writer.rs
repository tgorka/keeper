//! `SessionWriter`: the one way a host writes a session's log (AD-366, S-17).
//!
//! Over 89.5's `ChunkWriter`, so every line's free text is redacted before it
//! reaches a chunk; over the zone's index, so the board, the list and the
//! dedupe of incoming events read rows and never the log. Every line it
//! writes is the line the chunk holds (`AppendReceipt::line`) and goes into
//! the session's [`SessionContext`] the same way a replay would put it there,
//! so the context in memory always equals a fresh replay of the files.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use keeper_core::agents::index::{Index, IndexError};
use keeper_core::agents::log::writer::{rotate_at, ChunkWriter};
use keeper_core::agents::log::{HostSlug, LineBody, LogError, LogLine, LINE_VERSION};
use keeper_core::agents::session::SessionAgent;
use matrix_sdk::ruma::{EventId, OwnedEventId};
use ulid::Ulid;

use crate::agent::SessionContext;
use crate::claims::Lease;

/// Why a line could not be written.
#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    #[error(transparent)]
    Log(#[from] LogError),
    #[error(transparent)]
    Index(#[from] IndexError),
    /// The claim this writer wrote under is lost, or unconfirmed for too
    /// long (NFR-120): another host may hold the session now.
    #[error("this host no longer holds the session's claim, so it writes nothing more")]
    NoClaim,
}

/// The writer of one session's log on this host.
pub struct SessionWriter {
    chunks: ChunkWriter,
    index: Index,
    host: HostSlug,
    /// Zone-relative: the index's key.
    session: String,
    /// The claim every line is written under; `None` before claims (epoch 0).
    lease: Option<Arc<Lease>>,
    ids: ulid::Generator,
    last_ts: DateTime<Utc>,
    /// Where the last line this writer wrote went: its chunk, its id, and
    /// the chunk's length through it (an approval's checkpoint, AD-393).
    last_place: Option<LastPlace>,
}

/// Where a line landed: its chunk's file name, its id, and how many bytes
/// of that chunk run through it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastPlace {
    pub chunk: String,
    pub line: Ulid,
    pub through_bytes: u64,
}

impl SessionWriter {
    /// Open the writer of `session` (zone-relative) in the sessions zone at
    /// `zone`, as `host`, under `lease`: every line carries its epoch and
    /// claim event, and nothing is written once it may not write (epoch 0
    /// and no claim without one, C5). The session is added to the index if
    /// it is not there yet.
    pub fn open(
        zone: &Path,
        session: &str,
        agent: &SessionAgent,
        host: &HostSlug,
        lfs_threshold_bytes: u64,
        lease: Option<Arc<Lease>>,
    ) -> Result<SessionWriter, WriterError> {
        let mut index = Index::open(zone)?;
        if index.session(session)?.is_none() {
            index.add_session(session, agent)?;
        }
        let now = Utc::now();
        let chunks = ChunkWriter::open(
            &zone.join(session),
            host,
            rotate_at(lfs_threshold_bytes),
            now.date_naive(),
        )?;
        Ok(SessionWriter {
            chunks,
            index,
            host: host.clone(),
            session: session.to_owned(),
            lease,
            ids: ulid::Generator::new(),
            last_ts: DateTime::<Utc>::MIN_UTC,
            last_place: None,
        })
    }

    /// Whether `event` is already logged in this session (`seen_events`), so
    /// a redelivered event is never a second turn.
    pub fn seen(&self, event: &EventId) -> Result<bool, WriterError> {
        Ok(self.index.seen(&self.session, event.as_str())?)
    }

    /// The time the next line gets: now, in whole milliseconds as a chunk
    /// holds it, never before the last line's.
    pub fn next_ts(&self) -> DateTime<Utc> {
        let now = Utc::now();
        let now = DateTime::from_timestamp_millis(now.timestamp_millis()).unwrap_or(now);
        now.max(self.last_ts)
    }

    /// Whether a line — or a file effect beside it — may be written under
    /// the lease now; always before claims.
    pub fn may_write(&self) -> bool {
        self.lease.as_ref().is_none_or(|lease| lease.may_write())
    }

    /// The lease this writer writes under, for a file writer that must ask
    /// it right before its effect (R120).
    pub fn lease(&self) -> Option<Arc<Lease>> {
        self.lease.clone()
    }

    /// The lease's epoch and claim event, as a line carries them.
    fn fence_key(&self) -> (u64, Option<String>) {
        self.lease.as_ref().map_or((0, None), |lease| {
            (lease.epoch, Some(lease.claim_event.to_string()))
        })
    }

    /// Write one line and push it into `context`.
    ///
    /// Ids come from a monotonic generator and times never go back, so the
    /// reader's (`ts`, `host`, `id`) order is the order of these calls.
    pub fn write(
        &mut self,
        context: &mut SessionContext,
        parent: Option<Ulid>,
        matrix_event: Option<OwnedEventId>,
        body: LineBody,
    ) -> Result<LogLine, WriterError> {
        let ts = self.next_ts();
        self.write_at(context, ts, parent, matrix_event, body)
    }

    /// [`Self::write`] at `ts`, which a caller took from [`Self::next_ts`]
    /// because the line's body states that time (an `open` line's frame).
    pub fn write_at(
        &mut self,
        context: &mut SessionContext,
        ts: DateTime<Utc>,
        parent: Option<Ulid>,
        matrix_event: Option<OwnedEventId>,
        body: LineBody,
    ) -> Result<LogLine, WriterError> {
        if !self.may_write() {
            return Err(WriterError::NoClaim);
        }
        self.append(context, ts, parent, matrix_event, body)
    }

    /// Write a `claim` line whatever the lease says: a holder that lost its
    /// claim still records that it noticed (the fence never drops a claim
    /// transition).
    pub fn write_claim(
        &mut self,
        context: &mut SessionContext,
        body: keeper_core::agents::log::ClaimBody,
    ) -> Result<LogLine, WriterError> {
        let ts = self.next_ts();
        self.append(context, ts, None, None, LineBody::Claim(body))
    }

    fn append(
        &mut self,
        context: &mut SessionContext,
        ts: DateTime<Utc>,
        parent: Option<Ulid>,
        matrix_event: Option<OwnedEventId>,
        body: LineBody,
    ) -> Result<LogLine, WriterError> {
        let (epoch, claim) = self.fence_key();
        let ts = ts.max(self.last_ts);
        self.last_ts = ts;
        let id = self.ids.generate().unwrap_or_else(|_| Ulid::new());
        let line = LogLine {
            v: LINE_VERSION,
            id,
            parent,
            ts,
            host: self.host.clone(),
            epoch,
            claim,
            matrix_event,
            body,
        };
        let receipt = self.chunks.append(&line)?;
        self.index.apply(&self.session, &receipt)?;
        self.last_place = Some(LastPlace {
            chunk: receipt.chunk.to_string(),
            line: id,
            through_bytes: receipt.offset + receipt.bytes,
        });
        context.push(&receipt.line);
        Ok(receipt.line)
    }

    /// Where the last line this writer wrote went, if it wrote one.
    pub fn last_place(&self) -> Option<&LastPlace> {
        self.last_place.as_ref()
    }

    /// `fsync` what this writer wrote since the last sync: the end of a turn.
    pub fn sync(&mut self) -> Result<(), WriterError> {
        Ok(self.chunks.sync()?)
    }
}
