//! The drive [`ToolHost`]: where the three halves of a drive tool call meet
//! (Story 61.11, FR-388, FR-389, NFR-47).
//!
//! # There is no decision in this file, by rule
//!
//! Every question a tool call raises is answered elsewhere and this module
//! only sequences the answers (AD-55, AD-56):
//!
//! | question | answered by | crate |
//! |---|---|---|
//! | may this bot touch this path? | `bots::grant::{check, decide}`, through [`GrantSource`] | `keeper-core` |
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
//! 2. [`GrantSource::verdict`] — **every call, never once per conversation**.
//!    A grant revoked while a turn is in flight must stop the next call in
//!    that turn, which is only true if the source is asked again here (FR-386).
//!    An agent's host then classifies the call on that answer (AD-392,
//!    [`AgentDrive`]): its tier goes into the row, and a call that needs a
//!    person nobody can be asked, or is T5, is refused once the row exists.
//! 3. `audit::append_intent` — **before the effect**, so a crash mid-write
//!    leaves a row saying a write was starting. A row written afterwards
//!    records only the calls that survived, which is the opposite of an audit.
//! 4. The effect, through `keeper_sync::bots_fs`.
//! 5. `audit::complete` — the outcome, the byte count, and whether it was
//!    truncated.
//!
//! # What the approval port is for
//!
//! A verdict can be [`GrantVerdict::Ask`], and asking is an act this
//! crate cannot perform from inside a blocking tool call. So the ask is a
//! **port**: [`DriveToolHost::approve`] is built from the host process's
//! [`ApprovalPort`] — the app's is [`crate::approval::SinkApprover`], which
//! sends the ask down the turn's own stream, typed or spoken, and blocks
//! until the approval sheet answers. A host built with no approver
//! refuses every ask with [`UNATTENDED_REFUSAL`], which is the safe direction
//! — a missing person must never read as consent.

use std::path::PathBuf;
use std::sync::Arc;

use keeper_core::agents::tier::Classification;
use keeper_core::bots::audit::{self, AuditIntent, AuditOutcome};
use keeper_core::bots::chat::CancelSignal;
use keeper_core::bots::context_files::{self, ContextBundle, LoadedContext};
use keeper_core::bots::error::BotsError;
use keeper_core::bots::grant::{Grant, GrantVerdict, ToolTarget};
use keeper_core::bots::tools::{
    self, EntryLine, ToolArgs, ToolCall, ToolHost, ToolName, ToolOutcome,
};
use keeper_core::vm::BotApprovalRequestVm;
use keeper_sync::bots_fs::{self, FileRead, FsRefusal, Limits, LineRange};
use keeper_sync::files_write::WriteRoute;
use keeper_sync::SyncProfile;

use crate::grants::GrantSource;
use crate::ports::{ApprovalPort, VaultWriter};
use crate::turn::{new_id, now_ms, DrivePorts};

/// What a tool call that needs a person's approval answers when there is no
/// person to ask (F12). The model reads it, prefixed `Refused: `, as the
/// call's result, and the audit row closes `refused`.
pub const UNATTENDED_REFUSAL: &str = "This needs a person's approval, and there is no one here to ask, so keeper did not do it. Nothing was changed.";

/// What the drive contributes to one turn, decided while arming it.
pub struct ArmedDrive {
    /// The profiles a tool call may name. Empty where there is no drive.
    pub profiles: Vec<SyncProfile>,
    /// The bundle the model is shown, when tools were offered and the drive
    /// could read one. `None` is "keeper does not know", never "none".
    pub context: Option<ContextBundle>,
    /// How to build the host once the turn's task exists.
    pub host: Box<dyn TurnHost>,
}

impl ArmedDrive {
    /// The build without a drive: no profiles, no context, a refusing host.
    pub fn none() -> Self {
        Self {
            profiles: Vec::new(),
            context: None,
            host: Box::new(NoDrive),
        }
    }
}

/// The ids a host's audit rows and approval sheet name.
pub struct HostIds {
    /// Where `keeper.db` lives.
    pub data_dir: PathBuf,
    /// The provider.
    pub provider_id: String,
    /// The bot.
    pub bot_id: String,
    /// The conversation, for the audit row.
    pub session_id: String,
    /// The assistant message these calls belong to, where there is one.
    pub message_id: Option<String>,
}

/// How a turn makes the host its tool calls run against.
///
/// A method rather than a value on [`ArmedDrive`] because the cancel signal an
/// approval waits on exists only inside the spawned task.
pub trait TurnHost: Send + Sync {
    /// Build the host over `profiles`.
    fn host(
        &self,
        ids: HostIds,
        profiles: Vec<SyncProfile>,
        signal: CancelSignal,
    ) -> Box<dyn ToolHost>;

    /// The host for an agent's turn: its writes never reach the sessions
    /// zone, which the agent writes through its session tools only (R51),
    /// and every call is classified before its audit row (93.4). A ⌘9 bot's
    /// host is never built this way.
    fn agent_host(
        &self,
        ids: HostIds,
        profiles: Vec<SyncProfile>,
        signal: CancelSignal,
    ) -> Box<dyn AgentDrive>;
}

/// What an agent's host made of one drive call once its grant answered:
/// the call's classification (AD-392), the sentence refusing it when it
/// does not run — a block, a person's approval nobody can give, or T5 —
/// and the approval record it parks on or was approved by.
pub struct Classified {
    pub classification: Classification,
    pub refusal: Option<String>,
    pub approval: Option<Approval>,
}

/// A call's approval record (AD-393).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// It waits for a person on this record: its row is written, marked
    /// and left pending, and nothing runs (R73).
    Park(ulid::Ulid),
    /// A person approved it on this record and the approval was consumed:
    /// it runs, closing the row its park left pending on this machine.
    Approved(ulid::Ulid),
}

impl Approval {
    pub fn id(self) -> String {
        match self {
            Approval::Park(id) | Approval::Approved(id) => id.to_string(),
        }
    }
}

/// An agent's drive host. `classify` is asked once per call, after the
/// grant and before the audit row, which carries its tier (R90); a refusal
/// closes that row `refused` and nothing runs.
pub trait AgentDrive: Send + Sync {
    fn run_classified(
        &self,
        call: &ToolCall,
        classify: &dyn Fn(&GrantVerdict) -> Classified,
    ) -> Result<ToolOutcome, BotsError>;
}

/// The drive port on a host with no drive.
///
/// Holds no profiles, loads no context, and refuses every call by name —
/// which `tools::offer_tools` makes unreachable in practice, since a grant
/// cannot be created where `CapabilitiesVm.botTools` is false. The refusal
/// exists so that a model which calls a tool anyway gets a sentence rather
/// than a panic.
pub struct NoDrive;

impl TurnHost for NoDrive {
    fn host(&self, _: HostIds, _: Vec<SyncProfile>, _: CancelSignal) -> Box<dyn ToolHost> {
        Box::new(NoDrive)
    }

    fn agent_host(&self, _: HostIds, _: Vec<SyncProfile>, _: CancelSignal) -> Box<dyn AgentDrive> {
        Box::new(NoDrive)
    }
}

impl ToolHost for NoDrive {
    fn run(&self, call: &ToolCall) -> Result<ToolOutcome, BotsError> {
        Ok(ToolOutcome::Refused {
            reason: format!(
                "{} needs the drive, and this build of keeper has none: the drive tools live on the Mac.",
                call.name.as_wire()
            ),
        })
    }
}

impl AgentDrive for NoDrive {
    fn run_classified(
        &self,
        call: &ToolCall,
        _: &dyn Fn(&GrantVerdict) -> Classified,
    ) -> Result<ToolOutcome, BotsError> {
        self.run(call)
    }
}

/// The drive port on a host with a drive: the vault writer and the approval
/// port, held until the turn's task exists and the host can be built.
pub struct DriveTurnHost {
    vault: Option<Arc<dyn VaultWriter>>,
    approval: Option<Arc<dyn ApprovalPort>>,
    grants: Arc<dyn GrantSource>,
}

impl TurnHost for DriveTurnHost {
    fn host(
        &self,
        ids: HostIds,
        profiles: Vec<SyncProfile>,
        signal: CancelSignal,
    ) -> Box<dyn ToolHost> {
        let approve = self.approval.as_ref().map(|port| {
            approver(
                Arc::clone(port),
                signal,
                ids.provider_id.clone(),
                ids.bot_id.clone(),
            )
        });
        Box::new(self.drive_host(ids, profiles, approve, false))
    }

    /// No approver: an agent's call needing a person is decided by its
    /// classification, never by ⌘9's port.
    fn agent_host(
        &self,
        ids: HostIds,
        profiles: Vec<SyncProfile>,
        _: CancelSignal,
    ) -> Box<dyn AgentDrive> {
        Box::new(self.drive_host(ids, profiles, None, true))
    }
}

impl DriveTurnHost {
    fn drive_host(
        &self,
        ids: HostIds,
        profiles: Vec<SyncProfile>,
        approve: Option<Arc<Approver>>,
        sessions_closed: bool,
    ) -> DriveToolHost {
        DriveToolHost {
            data_dir: ids.data_dir,
            provider_id: ids.provider_id,
            bot_id: Some(ids.bot_id),
            session_id: ids.session_id,
            message_id: ids.message_id,
            profiles,
            vault: self.vault.clone(),
            approve,
            grants: Arc::clone(&self.grants),
            sessions_closed,
        }
    }
}

/// The approver one turn's host calls: compose the ask and hand it to the port.
fn approver(
    port: Arc<dyn ApprovalPort>,
    signal: CancelSignal,
    provider_id: String,
    bot_id: String,
) -> Arc<Approver> {
    Arc::new(move |call: &ToolCall, reason: &str| -> bool {
        let request =
            BotApprovalRequestVm::compose(&new_id(), &provider_id, Some(&bot_id), call, reason);
        port.ask(request, &signal)
    })
}

/// Arm the drive half of one turn.
///
/// Two reads, and one decision that is `keeper-core`'s: the sync profiles,
/// then — only when `offered` says tools went in the request — the context
/// files [`context_files::context_targets`] picks from the live grants,
/// loaded through [`load_context`] and merged into the bundle the model is
/// shown. `source` is what every call of the turn is checked against.
pub fn arm_drive(
    ports: &DrivePorts,
    source: Arc<dyn GrantSource>,
    grants: &[Grant],
    offered: bool,
) -> ArmedDrive {
    let profiles = ports.profiles.profiles();
    let context = offered.then(|| {
        let profile_ids: Vec<&str> = profiles.iter().map(|profile| profile.id.as_str()).collect();
        let targets = context_files::context_targets(grants, &profile_ids);
        context_files::merge(load_context(&profiles, &targets))
    });
    ArmedDrive {
        profiles,
        context,
        host: Box::new(DriveTurnHost {
            vault: ports.vault.clone(),
            approval: ports.approval.clone(),
            grants: source,
        }),
    }
}

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

/// The filesystem tool host for one conversation.
///
/// Holds the profiles by value rather than an `Engine` handle for the same
/// reason `browse` takes a `&SyncProfile`: a host that could reach the engine
/// is a host that will eventually spend something on a model's behalf.
pub struct DriveToolHost {
    /// Where `keeper.db` lives — the audit log.
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
    /// How a write inside a notes vault lands. `None` routes every write as
    /// outside any vault.
    pub vault: Option<Arc<dyn VaultWriter>>,
    /// The approval port. `None` refuses every ask with
    /// [`UNATTENDED_REFUSAL`].
    pub approve: Option<Arc<Approver>>,
    /// What every call is checked against, re-read per call.
    pub grants: Arc<dyn GrantSource>,
    /// Whether this host's writes keep out of the sessions zone (R51): an
    /// agent's host, never a ⌘9 bot's.
    pub sessions_closed: bool,
}

impl DriveToolHost {
    fn profile(&self, profile_id: &str) -> Option<&SyncProfile> {
        self.profiles
            .iter()
            .find(|profile| profile.id == profile_id)
    }
}

impl ToolHost for DriveToolHost {
    fn run(&self, call: &ToolCall) -> Result<ToolOutcome, BotsError> {
        self.run_with(call, None)
    }
}

impl AgentDrive for DriveToolHost {
    fn run_classified(
        &self,
        call: &ToolCall,
        classify: &dyn Fn(&GrantVerdict) -> Classified,
    ) -> Result<ToolOutcome, BotsError> {
        self.run_with(call, Some(classify))
    }
}

impl DriveToolHost {
    /// The one sequence both kinds of host run; `classify` is an agent's
    /// (step 2½), `None` for a ⌘9 bot, whose rows and behaviour it leaves
    /// as they were.
    fn run_with(
        &self,
        call: &ToolCall,
        classify: Option<&dyn Fn(&GrantVerdict) -> Classified>,
    ) -> Result<ToolOutcome, BotsError> {
        let effect = call.name.effect();
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
            let reason = format!(
                "keeper holds no sync folder called \"{}\". The folders it holds are: {known}.",
                call.target.profile_id
            );
            self.unanswered(call, classify, &reason, AuditOutcome::Refused)?;
            return Ok(ToolOutcome::Refused { reason });
        };

        // Step 2 — every call, never once per conversation (FR-386).
        let verdict = match self.grants.verdict(&call.target, effect) {
            Ok(verdict) => verdict,
            Err(error) => {
                let detail = error.to_string();
                self.unanswered(call, classify, &detail, AuditOutcome::Failed)?;
                return Err(BotsError::Tool { detail });
            }
        };

        // Step 2½ — an agent's call is classified on the grant's answer, so
        // its row carries the tier (R90).
        let classified = classify.map(|classify| classify(&verdict));
        let approval = classified
            .as_ref()
            .and_then(|classified| classified.approval);

        // Step 3 — before the effect (NFR-47). A row that cannot be written is
        // a refusal and never a silent proceed: an unauditable effect is one
        // this app does not perform. An approved call closes the row its
        // park left pending, so a call keeps one row.
        let parked_row = match approval {
            Some(Approval::Approved(id)) => {
                audit::parked_row(&self.data_dir, &id.to_string()).unwrap_or(None)
            }
            _ => None,
        };
        let audit_id = match parked_row {
            Some(row) => row,
            None => self.intent(
                call,
                &verdict,
                classified.as_ref().map(|c| &c.classification),
            )?,
        };
        if let Some(approval) = approval {
            audit::mark_approval(&self.data_dir, audit_id, &approval.id()).map_err(|error| {
                BotsError::Tool {
                    detail: format!(
                        "keeper could not record this tool call, so it did not run: {error}"
                    ),
                }
            })?;
        }
        let close = |outcome: AuditOutcome, bytes: Option<i64>, truncated: bool| {
            self.close(audit_id, outcome, bytes, truncated);
        };

        if let GrantVerdict::Deny { reason } = &verdict {
            close(AuditOutcome::Refused, None, false);
            return Err(BotsError::GrantDenied {
                reason: reason.clone(),
            });
        }
        let agent = classified.is_some();
        if let Some(reason) = classified.and_then(|classified| classified.refusal) {
            close(AuditOutcome::Refused, None, false);
            return Err(BotsError::GrantDenied { reason });
        }
        if let Some(Approval::Park(approval)) = approval {
            return Ok(ToolOutcome::Parked { approval });
        }
        // An agent's ask is its tier's (a grant's `Ask` is at least T2):
        // decided above. A ⌘9 bot asks its approver.
        if let (GrantVerdict::Ask { reason, .. }, false) = (&verdict, agent) {
            // No one to ask is not a "no": the model is told there was
            // nobody here, while a person's no keeps the grant's sentence.
            let refusal = match &self.approve {
                None => Some(UNATTENDED_REFUSAL.to_owned()),
                Some(approve) if !approve(call, reason) => Some((*reason).to_owned()),
                Some(_) => None,
            };
            if let Some(reason) = refusal {
                close(AuditOutcome::Refused, None, false);
                return Err(BotsError::GrantDenied { reason });
            }
        }

        // Step 4 — the effect. Every arm below is one `bots_fs` call plus the
        // projection into the vocabulary the model reads.
        let outcome = perform(profile, self.vault.as_deref(), call, self.sessions_closed);

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
            Ok(ToolOutcome::NotMaterialized { .. } | ToolOutcome::Answered { .. }) => {
                close(AuditOutcome::Ok, None, false)
            }
            Ok(ToolOutcome::Refused { .. }) => close(AuditOutcome::Refused, None, false),
            Ok(ToolOutcome::Parked { .. }) | Err(_) => close(AuditOutcome::Failed, None, false),
        }
        outcome
    }

    /// The audit row of `call` before any effect (NFR-47): its grant's
    /// `verdict`, and an agent's `classification`.
    fn intent(
        &self,
        call: &ToolCall,
        verdict: &GrantVerdict,
        classification: Option<&Classification>,
    ) -> Result<i64, BotsError> {
        audit::append_intent(
            &self.data_dir,
            &AuditIntent {
                started_ms: now_ms(),
                provider_id: &self.provider_id,
                bot_id: self.bot_id.as_deref(),
                session_id: &self.session_id,
                message_id: self.message_id.as_deref(),
                tool: call.name.as_wire(),
                target: &call.target,
                effect: call.name.effect(),
                verdict,
                classified: classification,
            },
        )
        .map_err(|error| BotsError::Tool {
            detail: format!("keeper could not record this tool call, so it did not run: {error}"),
        })
    }

    fn close(&self, row: i64, outcome: AuditOutcome, bytes: Option<i64>, truncated: bool) {
        if let Err(error) =
            audit::complete(&self.data_dir, row, outcome, bytes, truncated, now_ms())
        {
            tracing::warn!(%error, "bots: could not close a tool-call audit row");
        }
    }

    /// An agent's call that ends before its grant answers still has its one
    /// row (R90): `Deny` with `reason`, classified on that, closed
    /// `outcome` — an approved call's the row its park left pending here,
    /// else a new one carrying the approval (R172). A ⌘9 bot's (`classify`
    /// is `None`) has none, as before.
    fn unanswered(
        &self,
        call: &ToolCall,
        classify: Option<&dyn Fn(&GrantVerdict) -> Classified>,
        reason: &str,
        outcome: AuditOutcome,
    ) -> Result<(), BotsError> {
        let Some(classify) = classify else {
            return Ok(());
        };
        let verdict = GrantVerdict::Deny {
            reason: reason.to_owned(),
        };
        let classified = classify(&verdict);
        let approved = match classified.approval {
            Some(Approval::Approved(id)) => Some(id.to_string()),
            _ => None,
        };
        let parked = approved
            .as_deref()
            .and_then(|id| audit::parked_row(&self.data_dir, id).unwrap_or(None));
        let row = match parked {
            Some(row) => row,
            None => {
                let row = self.intent(call, &verdict, Some(&classified.classification))?;
                if let Some(id) = &approved {
                    if let Err(error) = audit::mark_approval(&self.data_dir, row, id) {
                        tracing::warn!(%error, "bots: could not mark a tool-call audit row");
                    }
                }
                row
            }
        };
        self.close(row, outcome, None, false);
        Ok(())
    }
}

/// The dispatch. One arm per verb, each one call into `keeper-sync`.
fn perform(
    profile: &SyncProfile,
    vault: Option<&dyn VaultWriter>,
    call: &ToolCall,
    sessions_closed: bool,
) -> Result<ToolOutcome, BotsError> {
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
            write_through(profile, vault, subpath, &content, &limits, sessions_closed)
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
                Ok(next) => write_through(profile, vault, subpath, &next, &limits, sessions_closed),
                Err(refusal) => refused(refusal),
            }
        }
    }
}

/// The routed write: `WriteScope::route` picks the writer, and the vault arm
/// is reachable only where the host's vault port names a live vault (AD-102).
///
/// The vault is looked up here, and again by [`VaultWriter::write`] when the
/// bytes land: a vault unregistered or moved in between fails the write
/// rather than writing through a stale handle.
fn write_through(
    profile: &SyncProfile,
    vault: Option<&dyn VaultWriter>,
    subpath: &str,
    content: &str,
    limits: &Limits,
    sessions_closed: bool,
) -> Result<ToolOutcome, BotsError> {
    // The LIVE vault and the scope built from it, in one lookup — the same
    // rule `sync_ipc::vault_and_scope` states: a scope built from
    // `profile.notes` claims a writability the registry may not have.
    let subfolder = vault.and_then(|vault| vault.subfolder(&profile.id));
    let scope = keeper_sync::files_write::WriteScope::new(&profile.name, subfolder.as_deref())
        .with_sessions(
            profile
                .sessions
                .as_ref()
                .map(|sessions| sessions.subfolder.as_str()),
        )
        .with_agents(profile.agents.as_ref().map(|a| a.subfolder.as_str()))
        .with_sessions_closed(sessions_closed);

    let live = subfolder.as_deref().zip(vault);
    let route = match bots_fs::plan_write(&scope, live, profile.local_path.as_path(), subpath) {
        Ok(route) => route,
        Err(refusal) => {
            return Ok(ToolOutcome::Refused {
                reason: refusal.to_string(),
            })
        }
    };

    match route {
        WriteRoute::Vault {
            vault: (subfolder, vault),
            path,
        } => {
            let bytes = content.len() as u64;
            if bytes > limits.max_write_bytes {
                return Ok(ToolOutcome::Refused {
                    reason: format!(
                        "{subpath} would be {bytes} bytes and this surface writes at most {}",
                        limits.max_write_bytes
                    ),
                });
            }
            vault
                .write(&profile.id, subfolder, path.as_str(), content)
                .map_err(|detail| BotsError::Tool { detail })?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// R51: an agent host's write never lands in the sessions zone — its
    /// session tools are the only door — while a ⌘9 bot's host writes there
    /// as it always has.
    #[test]
    fn an_agent_hosts_write_stops_at_the_sessions_zone() {
        let drive = tempfile::tempdir().expect("drive");
        let mut profile = SyncProfile::new("tgdrive", "tgdrive", drive.path(), "unused");
        profile.sessions = Some(Default::default());
        let zone = profile
            .sessions
            .as_ref()
            .expect("sessions")
            .subfolder
            .clone();
        let card = format!("{zone}/active/s/card.md");
        std::fs::create_dir_all(drive.path().join(&zone).join("active/s")).expect("session");
        std::fs::write(drive.path().join(&card), "old").expect("card");

        let agent = write_through(&profile, None, &card, "x", &limits(), true).expect("an outcome");
        assert!(matches!(agent, ToolOutcome::Refused { .. }), "{agent:?}");
        let on_disk = || std::fs::read_to_string(drive.path().join(&card)).expect("card");
        assert_eq!(on_disk(), "old");

        let bot = write_through(&profile, None, &card, "x", &limits(), false).expect("an outcome");
        assert!(matches!(bot, ToolOutcome::Wrote { .. }), "{bot:?}");
        assert_eq!(on_disk(), "x");
    }

    /// The shell's vault writer as far as a routed write reaches it: the
    /// live subfolder, joined under the drive natively, as `vault_root` is.
    #[cfg(unix)]
    struct NativeVault {
        root: PathBuf,
        subfolder: &'static str,
    }

    #[cfg(unix)]
    impl VaultWriter for NativeVault {
        fn subfolder(&self, _: &str) -> Option<String> {
            Some(self.subfolder.to_owned())
        }

        fn write(&self, _: &str, subfolder: &str, rel: &str, text: &str) -> Result<(), String> {
            std::fs::write(self.root.join(subfolder).join(rel), text).map_err(|e| e.to_string())
        }

        fn amend(
            &self,
            _: &str,
            _: &str,
            _: &dyn Fn(&str) -> Option<String>,
        ) -> Result<bool, String> {
            Err("a routed write never amends".to_owned())
        }
    }

    /// A vault configured `notes\.` on Unix is the folder of that name, and
    /// `notes/` beside it is not the vault: an agent's write to an existing
    /// `notes/both.md` lands there through the plain writer, one to an
    /// absent `notes/keep.md` is refused, and the files in `notes\./` the
    /// route never looked at keep their bytes.
    #[cfg(unix)]
    #[test]
    fn an_agent_hosts_write_lands_only_where_it_was_routed() {
        let drive = tempfile::tempdir().expect("drive");
        let profile = SyncProfile::new("tgdrive", "tgdrive", drive.path(), "unused");
        let vault = NativeVault {
            root: drive.path().to_owned(),
            subfolder: "notes\\.",
        };
        let theirs = drive.path().join("notes\\.");
        let mine = drive.path().join("notes");
        std::fs::create_dir_all(&theirs).expect("notes\\.");
        std::fs::create_dir_all(&mine).expect("notes");
        for name in ["keep.md", "both.md"] {
            std::fs::write(theirs.join(name), "theirs\n").expect("theirs");
        }
        std::fs::write(mine.join("both.md"), "mine\n").expect("mine");
        let write = |subpath: &str| {
            write_through(&profile, Some(&vault), subpath, "x", &limits(), true)
                .expect("an outcome")
        };

        let absent = write("notes/keep.md");
        assert!(matches!(absent, ToolOutcome::Refused { .. }), "{absent:?}");
        assert!(!mine.join("keep.md").exists());
        let both = write("notes/both.md");
        assert!(
            matches!(both, ToolOutcome::Wrote { managed: false, .. }),
            "{both:?}"
        );
        assert_eq!(
            std::fs::read_to_string(mine.join("both.md")).expect("mine"),
            "x"
        );
        for name in ["keep.md", "both.md"] {
            assert_eq!(
                std::fs::read_to_string(theirs.join(name)).expect("theirs"),
                "theirs\n",
                "{name}"
            );
        }
    }
}
