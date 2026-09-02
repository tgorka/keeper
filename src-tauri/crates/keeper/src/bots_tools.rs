//! The shell's [`ToolHost`]: where the three halves of a drive tool call meet
//! (Story 61.11, FR-388, FR-389, NFR-47).
//!
//! # There is no decision in this file, by rule
//!
//! This crate does not build on a Linux developer machine, so anything decided
//! here is decided somewhere nobody can test it until macOS (AD-55, AD-56).
//! So every question a tool call raises is answered elsewhere and this module
//! only sequences the answers:
//!
//! | question | answered by | crate |
//! |---|---|---|
//! | may this bot touch this path? | `bots::grant::check` | `keeper-core` |
//! | is this path inside the profile? | `browse::resolve` / `plain_segments` | `keeper-sync` |
//! | which writer owns it? | `WriteScope::route` | `keeper-sync` |
//! | how many bytes may come back? | `bots::tools`' caps | `keeper-core` |
//! | what does the model read? | `bots::tools::render_result` | `keeper-core` |
//! | is this note reviewed by a person? | `bots::tools::okf_facts` | `keeper-core` |
//!
//! **There is no path arithmetic in this file and there must never be any.**
//!
//! # The order inside [`DriveToolHost::run`], which is the whole of NFR-47
//!
//! 1. Find the profile the target names. An unknown profile is a refusal, not
//!    a panic.
//! 2. `grant::check` — **every call, never once per conversation**. A grant
//!    revoked while a turn is in flight must stop the next call in that turn,
//!    which is only true if the store is re-read here (FR-386).
//! 3. `audit::append_intent` — **before the effect**, so a crash mid-write
//!    leaves a row saying a write was starting. A row written afterwards
//!    records only the calls that survived, which is the opposite of an audit.
//! 4. The effect, through `keeper_sync::bots_fs`.
//! 5. `audit::complete` — the outcome, the byte count, and whether it was
//!    truncated.
//!
//! # What the approval port is for
//!
//! `grant::check` can answer [`GrantVerdict::Ask`], and asking is a UI act this
//! crate cannot perform from inside a blocking tool call. So the ask is a
//! **port**: [`DriveToolHost::approve`] is supplied by whoever built the host.
//! `bots_ipc::approver` fills it in for a live turn — it sends the ask down the
//! stream channel and blocks until `bots_approval_answer` names it — and Story
//! 61.10's approval sheet is the other end. A host built with no approver
//! declines every ask, which is the safe direction — a missing UI must never
//! read as consent.

use std::path::PathBuf;
use std::sync::Arc;

use keeper_core::bots::audit::{self, AuditIntent, AuditOutcome};
use keeper_core::bots::context_files::{self, LoadedContext};
use keeper_core::bots::error::BotsError;
use keeper_core::bots::grant::{self, GrantVerdict, ToolTarget};
use keeper_core::bots::tools::{
    self, EntryLine, ToolArgs, ToolCall, ToolHost, ToolName, ToolOutcome,
};
use keeper_sync::bots_fs::{self, FileRead, FsRefusal, Limits, LineRange};
use keeper_sync::files_write::WriteRoute;
use keeper_sync::SyncProfile;

/// The caps, taken from `keeper-core` and never restated here.
///
/// One function, so the numbers the model was promised in the tool schema and
/// the numbers the filesystem enforces are the same numbers.
fn limits() -> Limits {
    Limits {
        max_read_bytes: tools::MAX_READ_BYTES,
        max_entries: tools::MAX_LIST_ENTRIES,
        max_matches: tools::MAX_GREP_MATCHES,
        max_paths: tools::MAX_GLOB_PATHS,
        max_walk_entries: tools::MAX_WALK_ENTRIES,
        max_write_bytes: tools::MAX_WRITE_BYTES,
        max_match_line_bytes: tools::MAX_MATCH_LINE_BYTES,
    }
}

/// Asked when a grant says a write needs a person. `true` is consent.
pub type Approver = dyn Fn(&ToolCall, &str) -> bool + Send + Sync;

/// The shell's filesystem tool host for one conversation.
///
/// Holds the profiles by value rather than an `Engine` handle for the same
/// reason `browse` takes a `&SyncProfile`: a host that could reach the engine
/// is a host that will eventually spend something on a model's behalf.
pub struct DriveToolHost {
    /// Where `keeper.db` lives — the grant store and the audit log.
    pub data_dir: PathBuf,
    /// Which provider this conversation is with.
    pub provider_id: String,
    /// Which bot, when the grant is bot-specific.
    pub bot_id: Option<String>,
    /// The conversation, for the audit row.
    pub session_id: String,
    /// The assistant message these calls belong to, where there is one.
    pub message_id: Option<String>,
    /// The profiles a call may name.
    pub profiles: Vec<SyncProfile>,
    /// The approval port. `None` declines every ask.
    pub approve: Option<Arc<Approver>>,
}

impl DriveToolHost {
    fn profile(&self, profile_id: &str) -> Option<&SyncProfile> {
        self.profiles
            .iter()
            .find(|profile| profile.id == profile_id)
    }

    /// Ask the person, or decline for want of anyone to ask.
    fn ask(&self, call: &ToolCall, reason: &str) -> bool {
        self.approve
            .as_ref()
            .is_some_and(|approve| approve(call, reason))
    }
}

impl ToolHost for DriveToolHost {
    fn run(&self, call: &ToolCall) -> Result<ToolOutcome, BotsError> {
        let Some(profile) = self.profile(&call.target.profile_id) else {
            // Named rather than silently empty: a model that asked about a
            // folder keeper does not hold should be told so, and told what it
            // could ask about instead.
            let known = self
                .profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Ok(ToolOutcome::Refused {
                reason: format!(
                    "keeper holds no sync folder called \"{}\". The folders it holds are: {known}.",
                    call.target.profile_id
                ),
            });
        };

        let effect = call.name.effect();
        // Step 2 — every call, never once per conversation (FR-386).
        let verdict = grant::check(
            &self.data_dir,
            &self.provider_id,
            self.bot_id.as_deref(),
            &call.target,
            effect,
        )
        .map_err(|error| BotsError::Tool {
            detail: error.to_string(),
        })?;

        // Step 3 — before the effect (NFR-47). A row that cannot be written is
        // a refusal and never a silent proceed: an unauditable effect is one
        // this app does not perform.
        let started_ms = now_ms();
        let audit_id = audit::append_intent(
            &self.data_dir,
            &AuditIntent {
                started_ms,
                provider_id: &self.provider_id,
                bot_id: self.bot_id.as_deref(),
                session_id: &self.session_id,
                message_id: self.message_id.as_deref(),
                tool: call.name.as_wire(),
                target: &call.target,
                effect,
                verdict: &verdict,
            },
        )
        .map_err(|error| BotsError::Tool {
            detail: format!("keeper could not record this tool call, so it did not run: {error}"),
        })?;

        let close = |outcome: AuditOutcome, bytes: Option<i64>, truncated: bool| {
            if let Err(error) = audit::complete(
                &self.data_dir,
                audit_id,
                outcome,
                bytes,
                truncated,
                now_ms(),
            ) {
                tracing::warn!(%error, "bots: could not close a tool-call audit row");
            }
        };

        match &verdict {
            GrantVerdict::Allow { .. } => {}
            GrantVerdict::Ask { reason, .. } => {
                if !self.ask(call, reason) {
                    close(AuditOutcome::Refused, None, false);
                    return Err(BotsError::GrantDenied {
                        reason: (*reason).to_owned(),
                    });
                }
            }
            GrantVerdict::Deny { reason } => {
                close(AuditOutcome::Refused, None, false);
                return Err(BotsError::GrantDenied {
                    reason: reason.clone(),
                });
            }
        }

        // Step 4 — the effect. Every arm below is one `bots_fs` call plus the
        // projection into the vocabulary the model reads.
        let outcome = perform(profile, call);

        // Step 5 — the outcome, with the numbers.
        match &outcome {
            Ok(ToolOutcome::Text {
                body, truncated_at, ..
            }) => close(
                AuditOutcome::Ok,
                i64::try_from(body.len()).ok(),
                truncated_at.is_some(),
            ),
            Ok(ToolOutcome::Wrote { bytes, .. }) => {
                close(AuditOutcome::Ok, i64::try_from(*bytes).ok(), false);
            }
            Ok(ToolOutcome::Entries { truncated_at, .. }) => {
                close(AuditOutcome::Ok, None, truncated_at.is_some());
            }
            Ok(ToolOutcome::NotMaterialized { .. }) => close(AuditOutcome::Ok, None, false),
            Ok(ToolOutcome::Refused { .. }) => close(AuditOutcome::Refused, None, false),
            Err(_) => close(AuditOutcome::Failed, None, false),
        }
        outcome
    }
}

/// The dispatch. One arm per verb, each one call into `keeper-sync`.
fn perform(profile: &SyncProfile, call: &ToolCall) -> Result<ToolOutcome, BotsError> {
    let root = profile.local_path.as_path();
    let subpath = call.target.subpath.as_str();
    let limits = limits();
    let ToolArgs {
        start_line,
        line_count,
        pattern,
        needle,
        case_sensitive,
        content,
        old_text,
        new_text,
    } = call.args.clone();

    let refused = |refusal: FsRefusal| {
        Ok(ToolOutcome::Refused {
            reason: refusal.to_string(),
        })
    };

    match call.name {
        ToolName::List => match bots_fs::list(root, subpath, &limits) {
            Ok(listing) => Ok(ToolOutcome::Entries {
                subpath: listing.subpath,
                entries: listing
                    .entries
                    .into_iter()
                    .map(|entry| EntryLine {
                        subpath: entry.subpath,
                        is_dir: entry.is_dir,
                        bytes: entry.bytes,
                        is_virtual: entry.is_virtual,
                    })
                    .collect(),
                truncated_at: listing.truncated_at,
                of_entries: listing.of_entries,
            }),
            Err(refusal) => refused(refusal),
        },
        ToolName::Read => {
            let range = LineRange {
                start_line,
                line_count,
            };
            match bots_fs::read(root, subpath, range, &limits) {
                Ok(FileRead::Text {
                    body,
                    of_bytes,
                    truncated_at,
                    ..
                }) => Ok(ToolOutcome::Text {
                    // The provenance half, decided in `keeper-core` over the
                    // text this crate just read — so a note's OKF type and
                    // trust actor are asserted on Linux even though the read
                    // itself is not.
                    okf: tools::okf_facts(&body),
                    body,
                    truncated_at,
                    of_bytes: Some(of_bytes),
                }),
                Ok(FileRead::Pointer { oid, of_bytes }) => Ok(ToolOutcome::NotMaterialized {
                    subpath: subpath.to_owned(),
                    of_bytes,
                    oid,
                }),
                Err(refusal) => refused(refusal),
            }
        }
        ToolName::Glob => {
            let Some(pattern) = pattern else {
                return Ok(ToolOutcome::Refused {
                    reason: "drive_glob needs a \"pattern\" argument.".to_owned(),
                });
            };
            match bots_fs::glob(root, subpath, &pattern, &limits) {
                Ok(found) => Ok(ToolOutcome::Entries {
                    subpath: subpath.to_owned(),
                    of_entries: found.of_paths,
                    truncated_at: found.truncated_at,
                    entries: found
                        .paths
                        .into_iter()
                        .map(|subpath| EntryLine {
                            subpath,
                            is_dir: false,
                            bytes: None,
                            is_virtual: false,
                        })
                        .collect(),
                }),
                Err(refusal) => refused(refusal),
            }
        }
        ToolName::Grep => {
            let Some(needle) = needle else {
                return Ok(ToolOutcome::Refused {
                    reason: "drive_grep needs a \"needle\" argument.".to_owned(),
                });
            };
            match bots_fs::grep(root, subpath, &needle, case_sensitive, &limits) {
                Ok(found) => {
                    let mut body = String::new();
                    for hit in &found.matches {
                        body.push_str(&format!("{}:{}: {}\n", hit.subpath, hit.line, hit.text));
                    }
                    if found.files_skipped > 0 {
                        body.push_str(&format!(
                            "({} files were not searched: binary, not downloaded, or larger \
                             than the read limit.)\n",
                            found.files_skipped
                        ));
                    }
                    if found.walk_capped {
                        body.push_str(
                            "(The search stopped early: this subtree is larger than keeper will \
                             walk in one call. Search a narrower folder.)\n",
                        );
                    }
                    Ok(ToolOutcome::Text {
                        body,
                        truncated_at: found.truncated_at.map(|shown| shown as u64),
                        of_bytes: None,
                        okf: None,
                    })
                }
                Err(refusal) => refused(refusal),
            }
        }
        ToolName::Stat => match bots_fs::stat(root, subpath) {
            Ok(stat) => Ok(ToolOutcome::Text {
                body: format!(
                    "{}: {}, {} bytes{}{}\n",
                    stat.subpath,
                    if stat.is_dir { "folder" } else { "file" },
                    stat.bytes,
                    stat.modified_ms
                        .map_or_else(String::new, |ms| format!(", modified {ms} ms since epoch")),
                    if stat.is_virtual {
                        ", content not downloaded to this computer"
                    } else {
                        ""
                    }
                ),
                truncated_at: None,
                of_bytes: None,
                okf: None,
            }),
            Err(refusal) => refused(refusal),
        },
        ToolName::Write => {
            let Some(content) = content else {
                return Ok(ToolOutcome::Refused {
                    reason: "drive_write needs a \"content\" argument.".to_owned(),
                });
            };
            write_through(profile, subpath, &content, &limits)
        }
        ToolName::Edit => {
            let (Some(old_text), Some(new_text)) = (old_text, new_text) else {
                return Ok(ToolOutcome::Refused {
                    reason: "drive_edit needs \"old_text\" and \"new_text\" arguments.".to_owned(),
                });
            };
            match bots_fs::edited_text(
                profile.local_path.as_path(),
                subpath,
                &old_text,
                &new_text,
                &limits,
            ) {
                // One writer for both verbs: an edit is not a second way to
                // put bytes on the drive, it is a way to compose the bytes a
                // write puts there.
                Ok(next) => write_through(profile, subpath, &next, &limits),
                Err(refusal) => refused(refusal),
            }
        }
    }
}

/// The routed write: `WriteScope::route` picks the writer, and the vault arm
/// carries the live vault so it cannot be reached without one (AD-102).
fn write_through(
    profile: &SyncProfile,
    subpath: &str,
    content: &str,
    limits: &Limits,
) -> Result<ToolOutcome, BotsError> {
    // The LIVE vault and the scope built from it, in one lookup — the same
    // rule `sync_ipc::vault_and_scope` states: a scope built from
    // `profile.notes` claims a writability the registry may not have.
    let vault = crate::notes_vault::vault(&profile.id);
    let scope = keeper_sync::files_write::WriteScope::new(
        &profile.name,
        vault.as_ref().map(|vault| vault.config.subfolder.as_str()),
    )
    .with_sessions(
        profile
            .sessions
            .as_ref()
            .map(|sessions| sessions.subfolder.as_str()),
    );

    let route =
        match bots_fs::plan_write(&scope, vault.clone(), profile.local_path.as_path(), subpath) {
            Ok(route) => route,
            Err(refusal) => {
                return Ok(ToolOutcome::Refused {
                    reason: refusal.to_string(),
                })
            }
        };

    match route {
        WriteRoute::Vault { vault, path } => {
            let bytes = content.len() as u64;
            if bytes > limits.max_write_bytes {
                return Ok(ToolOutcome::Refused {
                    reason: format!(
                        "{subpath} would be {bytes} bytes and this surface writes at most {}",
                        limits.max_write_bytes
                    ),
                });
            }
            crate::notes_vault::write_vault_file(&vault, path.as_str(), content).map_err(
                |error| BotsError::Tool {
                    detail: error.to_string(),
                },
            )?;
            crate::notes_vault::touch(&vault.id, vec![path.as_str().to_owned()]);
            crate::notes_vault::mark_dirty(&vault.id);
            Ok(ToolOutcome::Wrote {
                subpath: subpath.to_owned(),
                bytes,
                managed: true,
            })
        }
        WriteRoute::Unmanaged(target) => match bots_fs::write_unmanaged(&target, content, limits) {
            Ok(wrote) => Ok(ToolOutcome::Wrote {
                subpath: wrote.subpath,
                bytes: wrote.bytes,
                managed: false,
            }),
            Err(refusal) => Ok(ToolOutcome::Refused {
                reason: refusal.to_string(),
            }),
        },
    }
}

/// Read the context files one turn may see, in the order `keeper-core` asked
/// for them (Story 61.11, FR-390, FR-391).
///
/// `targets` is [`keeper_core::bots::context_files::context_targets`]'s answer
/// — already grant-filtered and nearest-first — and this is one bounded
/// `bots_fs::read` per target through the same containment rule a tool call
/// takes. A target that names nothing, or a profile keeper does not hold, is
/// simply not loaded: the walk asks for every name a context file could have
/// and most of them do not exist. What was read is labelled with the display
/// path, because a drive-wide grant makes one bundle out of several profiles.
///
/// The pointer arm is skipped on purpose: a context file that is an LFS
/// pointer is not on this disk, and reading it must not fetch it.
pub fn load_context(profiles: &[SyncProfile], targets: &[ToolTarget]) -> Vec<LoadedContext> {
    let limits = Limits {
        max_read_bytes: context_files::MAX_CONTEXT_FILE_BYTES,
        ..limits()
    };
    let range = LineRange::default();
    targets
        .iter()
        .filter_map(|target| {
            let profile = profiles
                .iter()
                .find(|profile| profile.id == target.profile_id)?;
            match bots_fs::read(
                profile.local_path.as_path(),
                &target.subpath,
                range,
                &limits,
            ) {
                Ok(FileRead::Text { body, of_bytes, .. }) => Some(LoadedContext {
                    subpath: target.display_path(),
                    text: body,
                    of_bytes,
                }),
                Ok(FileRead::Pointer { .. }) | Err(_) => None,
            }
        })
        .collect()
}

/// Milliseconds since the Unix epoch. Falls back to zero rather than panicking:
/// a clock before 1970 must not be what stops an audit row being written.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_millis()).ok())
        .unwrap_or_default()
}
