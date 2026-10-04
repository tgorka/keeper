//! The app's side of `keeper_agent`'s ports (AD-367): where a turn's stream
//! goes, who approves, how a vault file is written, which drives exist, and
//! how an `AgentError` becomes the `IpcError` the same failure always was.
//!
//! The sinks compile on every target, because ⌘9 runs on a phone too; the
//! drive half — the approval sheet, the notes vault, the sync engine — is
//! desktop-only, and a phone's turn is armed with `drive: None`, which
//! `keeper_agent` answers with a host that refuses every tool by name. That
//! choice is made here at compile time, so a forgotten port is a compile error
//! rather than a turn that silently has no drive.

use std::path::Path;
use std::sync::{Arc, Mutex};

use keeper_agent::ports::{TurnEnd, TurnSink};
use keeper_agent::turn::{AccountCredential, AgentError, OpenedTurn, TurnEnv, TurnOrigin};
#[cfg(desktop)]
use keeper_core::platform::Platform;
use keeper_core::vm::{BotStreamEvent, IpcError, IpcErrorCode};
use keeper_core::voice::speech::Segmenter;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter};

use crate::ipc::{to_ipc_error, AppState};

/// The Tauri event every stream event of a spoken turn is forwarded on
/// (Epic 67, AD-205). The turn opened its stream in Rust with no webview
/// channel to hand it, so the pane — when there is one — observes the answer
/// arriving through this event instead, and applies each [`BotStreamEvent`]
/// exactly as it applies the ones on its own channel. That includes
/// `ApprovalAsked`: a spoken turn's approval sheet opens from this event and
/// is answered through `bots_approval_answer` like a typed one's.
pub const SPOKEN_STREAM_EVENT: &str = "keeper://bots-spoken-stream";

/// Fold an `AgentError` into the envelope the same failure carried before the
/// turn loop left this crate: store and platform failures through
/// [`to_ipc_error`], credential failures through the account mapper, and an
/// unknown id as `internal`.
pub(crate) fn agent_error(error: AgentError) -> IpcError {
    match error {
        AgentError::Core(error) => to_ipc_error(error),
        AgentError::Account(error) => crate::account_ipc::account_ipc_error(error),
        AgentError::NoSuch { what, id } => crate::bots_ipc::no_such(what, &id),
        AgentError::Unsupported(message) => IpcError {
            code: IpcErrorCode::Unsupported,
            message,
            account_id: None,
            retriable: false,
        },
        AgentError::Refused(message) => IpcError {
            code: IpcErrorCode::Internal,
            message,
            account_id: None,
            retriable: false,
        },
    }
}

/// The configured account as a credential source, read now.
fn account() -> Option<AccountCredential> {
    crate::account_ipc::descriptor().map(|descriptor| AccountCredential {
        descriptor,
        http: crate::account_ipc::http().cloned(),
    })
}

/// The environment for a call that reaches a provider but runs no tool: a
/// probe, a model list, a session list.
pub(crate) fn plain_env(state: &AppState) -> TurnEnv {
    TurnEnv {
        platform: Arc::clone(&state.platform),
        account: account(),
        drive: None,
    }
}

/// The environment for one ⌘9 turn whose approvals go down `approval`: the
/// sink the turn streams into, so an ask reaches whoever watches the stream.
#[cfg(desktop)]
pub(crate) fn turn_env(state: &AppState, approval: Option<Arc<dyn TurnSink>>) -> TurnEnv {
    TurnEnv {
        drive: Some(drive::ports(
            Arc::clone(&state.platform),
            approval.map(|sink| {
                Arc::new(keeper_agent::approval::SinkApprover::new(sink))
                    as Arc<dyn keeper_agent::ports::ApprovalPort>
            }),
        )),
        ..plain_env(state)
    }
}

/// The environment for one turn on a build with no drive.
#[cfg(not(desktop))]
pub(crate) fn turn_env(state: &AppState, _approval: Option<Arc<dyn TurnSink>>) -> TurnEnv {
    plain_env(state)
}

/// Whether a send made now belongs to the voice turn: its answer, read at the
/// moment arming reaches it.
pub(crate) fn origin_of(dir: &Path) -> TurnOrigin {
    match crate::voice_ipc::spoken_turn(dir) {
        Some(language) => TurnOrigin::Spoken { language },
        None => TurnOrigin::Typed,
    }
}

/// The sink a turn streams into: `base`, wrapped in the voice turn's hooks
/// when the turn is spoken — for `question`, the voice turn's question the
/// send was made for (`voice_ipc::question_now` as it armed, or the one the
/// voice turn handed over with the text). A spoken turn with no question
/// has no voice turn to report to.
pub(crate) fn sink_for(
    opened: &OpenedTurn,
    base: Arc<dyn TurnSink>,
    question: Option<u64>,
) -> Arc<dyn TurnSink> {
    match (&opened.turn.origin, question) {
        (TurnOrigin::Spoken { .. }, Some(question)) => Arc::new(SpokenSink {
            inner: base,
            segmenter: Mutex::new(Segmenter::new()),
            bot_name: opened.bot_name.clone(),
            question,
        }),
        (TurnOrigin::Spoken { .. }, None)
        | (TurnOrigin::Typed | TurnOrigin::Task | TurnOrigin::Agent { .. }, _) => base,
    }
}

/// A webview channel.
pub(crate) struct ChannelSink(pub(crate) Channel<BotStreamEvent>);

impl TurnSink for ChannelSink {
    fn event(&self, event: BotStreamEvent) -> bool {
        self.0.send(event).is_ok()
    }
}

/// The app-wide event a spoken turn's stream is forwarded on, for the pane
/// when there is one.
pub(crate) struct EventSink(pub(crate) AppHandle);

impl TurnSink for EventSink {
    fn event(&self, event: BotStreamEvent) -> bool {
        match self.0.emit(SPOKEN_STREAM_EVENT, &event) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "bots: could not forward a spoken stream event");
                false
            }
        }
    }
}

/// The voice turn's hooks around a spoken turn's stream (Epic 64, AD-186;
/// Epic 67, AD-205; Epic 68, AD-214).
///
/// The voice turn is told when the request leaves and when the first token
/// arrives, so its indicator has a middle; it is handed the answer sentence by
/// sentence as the stream closes each one, so the first is spoken when it
/// arrives; and the close is its cue — a clean finish hands it what the
/// segmenter had not closed, a Stop abandons the question, a failure ends the
/// turn on its sentence. Every report names `question`, so a stream for a
/// question the person has since stopped or replaced moves nothing.
struct SpokenSink {
    inner: Arc<dyn TurnSink>,
    segmenter: Mutex<Segmenter>,
    bot_name: String,
    question: u64,
}

impl SpokenSink {
    fn segmenter(&self) -> std::sync::MutexGuard<'_, Segmenter> {
        self.segmenter
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl TurnSink for SpokenSink {
    fn event(&self, event: BotStreamEvent) -> bool {
        if let BotStreamEvent::FirstToken { after_ms } = &event {
            crate::voice_ipc::note_answer_chunk(self.question, *after_ms);
        }
        self.inner.event(event)
    }

    fn request_sent(&self) {
        crate::voice_ipc::note_sent(self.question, &self.bot_name);
    }

    fn answer_text(&self, text: &str) {
        let sentences = self.segmenter().push(text);
        for sentence in sentences {
            crate::voice_ipc::answer_sentence(self.question, sentence);
        }
    }

    fn ended(&self, end: TurnEnd) {
        match end {
            TurnEnd::Complete => {
                let rest = self.segmenter().flush().unwrap_or_default();
                crate::voice_ipc::answer_complete(self.question, rest);
            }
            TurnEnd::Stopped => crate::voice_ipc::answer_stopped(self.question),
            TurnEnd::Failed(reason) => crate::voice_ipc::answer_failed(self.question, reason),
        }
    }
}

/// The scheduled bot task's environment: the drive, the vault, and nobody to
/// approve (AD-244).
#[cfg(desktop)]
pub(crate) fn task_env(platform: Arc<dyn Platform>) -> TurnEnv {
    TurnEnv {
        drive: Some(drive::ports(Arc::clone(&platform), None)),
        account: account(),
        platform,
    }
}

#[cfg(desktop)]
mod drive {
    use std::sync::Arc;

    use keeper_agent::ports::{ApprovalPort, ProfileSource, VaultWriter};
    use keeper_agent::turn::DrivePorts;
    use keeper_core::platform::Platform;
    use keeper_sync::SyncProfile;

    pub(super) fn ports(
        platform: Arc<dyn Platform>,
        approval: Option<Arc<dyn ApprovalPort>>,
    ) -> DrivePorts {
        DrivePorts {
            profiles: Arc::new(EngineProfiles(platform)),
            vault: Some(Arc::new(NotesVaultWriter)),
            approval,
        }
    }

    /// A write that lands in a registered notes vault: written, then the
    /// reconciler and the dirty mark told, as an edit in the editor would be.
    struct NotesVaultWriter;

    impl VaultWriter for NotesVaultWriter {
        fn subfolder(&self, profile_id: &str) -> Option<String> {
            crate::notes_vault::vault(profile_id).map(|vault| vault.config.subfolder)
        }

        fn write(&self, profile_id: &str, rel: &str, text: &str) -> Result<(), String> {
            let vault = crate::notes_vault::vault(profile_id)
                .ok_or_else(|| format!("the notes vault in {profile_id} is no longer open"))?;
            crate::notes_vault::write_vault_file(&vault, rel, text)
                .map_err(|error| error.to_string())?;
            crate::notes_vault::touch(&vault.id, vec![rel.to_owned()]);
            crate::notes_vault::mark_dirty(&vault.id);
            Ok(())
        }
    }

    /// Every sync profile keeper holds, or none when the engine is
    /// unavailable — every tool call is then refused as naming no folder.
    struct EngineProfiles(Arc<dyn Platform>);

    impl ProfileSource for EngineProfiles {
        fn profiles(&self) -> Vec<SyncProfile> {
            let Ok(engine) = crate::sync::engine(Arc::clone(&self.0)) else {
                return Vec::new();
            };
            engine.list_profiles().unwrap_or_default()
        }
    }
}
