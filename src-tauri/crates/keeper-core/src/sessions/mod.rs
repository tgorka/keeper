//! The pure sessions domain (Phase 7, AD-108).
//!
//! A session is a **directory with a contract**: `active/YYYY-MM-DD-<slug>/`
//! or `archive/YYYY/YYYY-MM-DD-<slug>/` inside a sessions-flagged synced
//! folder's zone (default `60-sessions/`), shaped by the zone's `_template/`
//! — `README.md`, `workspace/` (unversioned scratch), `artifacts/` (promoted
//! output), `refs/`, `prompts/`. Both live drives (tgdrive, neuradrive)
//! already run this layout; keeper adopts it, never invents it.
//!
//! Everything here is a *rule* rather than an *effect*, exactly as
//! [`crate::notes`] is: which directory names are sessions, what folder name a
//! title produces, what the README's `## Promote` table says, how two READMEs
//! reference each other as a lineage, and what a session row derives from
//! plain facts. It takes bytes and paths and returns values. It never opens a
//! file, never spawns a task, and never learns that a profile id means
//! anything to git — session IO lives in the `keeper` shell on `keeper-sync`'s
//! watcher (AD-108). Frontmatter is [`crate::notes::frontmatter`]'s — one
//! parser, one writer, byte-preserving (AD-109); a fork here would be a
//! defect.
//!
//! The one hard invariant, stated once and enforced everywhere: **files are
//! the only truth** (AD-110). Status is folder location, freshness is a fold
//! over supplied `(path, mtime)` facts, lineage is frontmatter on both ends,
//! promotion is the README's own table. Any state a Finder edit could desync
//! is a design defect, not a feature.

pub mod model;
pub mod promote;
pub mod vm;

/// Everything the sessions domain can refuse to do.
#[derive(Debug, thiserror::Error)]
pub enum SessionsError {
    #[error("no such session: {0}")]
    NotFound(String),
    #[error("sessions root {0} is not indexed")]
    RootUnknown(String),
    #[error("invalid session name: {0}")]
    Name(String),
}
