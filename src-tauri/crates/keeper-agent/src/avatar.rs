//! The serving host's side of an agent's picture (story 100.4, R384, R399):
//! each copy's home `avatar.*` becomes the agent's Matrix profile avatar
//! when its content hash changes, so a device without the drive draws it
//! too.
//!
//! Each scan hands every copy to [`publish_all`], which runs one task per
//! copy beside the tick, never two at once for one copy. A task reads the
//! home's picture (`zone::home_avatar`), and asks the server only when the
//! hash is not the one its memory says the profile shows
//! (`avatar::PublishMemo`) — once after the host starts, then on a change.
//! It first reads the agent's record and profile: a file the profile shows
//! already (one put back to what was published included) settles with no
//! claim and no upload. Otherwise only the holder of the agent's picture
//! claim in the principal's control room (`claim::picture_key`, taken as
//! every keyed claim is: written, read back, settled) publishes: it reads
//! the record again under the claim, uploads, sets the profile, writes the
//! record and hands the claim back. A host that finds the claim held
//! elsewhere uploads nothing and looks again later, when the holder's
//! record answers it. A failure is a WARN line, a sentence in the agent's
//! Settings › Agents row on every device of the principal — carried in this
//! host's manifest — and is tried again after `avatar::RETRY_AFTER`; it
//! leaves nothing settled, since it may have changed the profile. Removing
//! the file leaves the profile avatar as it was (DW-1421).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use keeper_core::agents::avatar::{
    self, AvatarFile, AvatarPort, AvatarProblem, PublishMemo, Published,
};
use keeper_core::agents::claim::{self, Claimant};
use keeper_core::agents::log::HostSlug;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

use crate::claims::{acquire, release, Acquired, ClaimPort, Rtt, ServerClock};
use crate::hosts::CopyPort;
use crate::runtime::Copy;

/// Why publishing each agent's picture last failed on this host, by Matrix
/// user: the failure's clause (an `avatar::publish` error). The settings
/// rows say it (`desktop::listing`'s `pictures`), and this host's manifest
/// carries it to every other device (`HostManifest::picture_failures`).
pub type Failures = Arc<Mutex<HashMap<OwnedUserId, String>>>;

/// [`Failures`] as a manifest carries them: by the agent's user, as text.
pub(crate) fn reported(failures: &Failures) -> BTreeMap<String, String> {
    failures
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .map(|(user, error)| (user.to_string(), error.clone()))
        .collect()
}

/// What a host's publications need of its claims: the principal's control
/// room, this host's slug, and its server clock and round trips.
#[derive(Clone)]
pub(crate) struct PictureRound {
    pub(crate) control: OwnedRoomId,
    pub(crate) host: HostSlug,
    pub(crate) clock: Arc<ServerClock>,
    pub(crate) rtt: Arc<Rtt>,
}

/// One copy's publication state, held across scans.
#[derive(Debug, Default)]
pub(crate) struct Picture {
    memo: PublishMemo,
    /// The home's problems last warned, so a scan says each once.
    warned: Vec<AvatarProblem>,
}

/// What one attempt to publish came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// The profile shows the file now.
    Published(Published),
    /// Another host held the agent's picture claim: it publishes.
    HeldElsewhere,
    /// Why it failed: a clause.
    Failed(String),
}

/// Publish `file` through `port` as the holder of the agent's picture
/// claim `claims`, taken as `me`: nothing is claimed when the record and
/// the profile show the file already; otherwise the claim is taken, the
/// record read again and the file published under it, and the claim handed
/// back. A claim another host holds — or wins as both take it — uploads
/// nothing.
pub(crate) async fn publish_held(
    claims: &dyn ClaimPort,
    me: &Claimant,
    clock: &ServerClock,
    rtt: &Rtt,
    port: &impl AvatarPort,
    file: &AvatarFile,
) -> Attempt {
    match avatar::is_published(port, &file.hash).await {
        Ok(true) => return Attempt::Published(Published::Unchanged),
        Ok(false) => {}
        Err(error) => return Attempt::Failed(error),
    }
    let lease = match acquire(claims, me, clock, rtt, None).await {
        Ok(Acquired::Won { lease, .. }) => lease,
        Ok(Acquired::HeldElsewhere | Acquired::Yielded) => return Attempt::HeldElsewhere,
        Err(error) => {
            return Attempt::Failed(format!("its picture's claim could not be taken: {error}"))
        }
    };
    let published = avatar::publish(port, file).await;
    if let Err(error) = release(claims, me, &lease, clock, rtt).await {
        tracing::warn!(agent = %me.agent, %error, "agents: the picture's claim could not be handed back; it lapses");
    }
    match published {
        Ok(published) => Attempt::Published(published),
        Err(error) => Attempt::Failed(error),
    }
}

/// Publish each of `copies`' pictures beside the tick, under `round`'s
/// claims (`None`: no control room is known yet, so nothing is published
/// and the file is looked at again later). `failures` gathers why each
/// failed; `warn_files` says the home's file problems in the log —
/// agentd's job, since the desktop's draw scan (`AgentIcons::replace`)
/// already says them.
pub(crate) fn publish_all(
    copies: &[Arc<Copy>],
    round: Option<&PictureRound>,
    failures: &Failures,
    warn_files: bool,
) {
    for copy in copies {
        let Ok(held) = Arc::clone(&copy.picture).try_lock_owned() else {
            continue;
        };
        let copy = Arc::clone(copy);
        let failures = Arc::clone(failures);
        let round = round.cloned();
        tokio::spawn(async move {
            let mut held = held;
            let home = copy.deps.home.clone();
            let drive_root = copy.deps.drive_root.clone();
            let user = home.config.matrix_user.clone();
            let name = home.config.name.clone();
            let Ok(read) =
                tokio::task::spawn_blocking(move || crate::zone::home_avatar(&home, &drive_root))
                    .await
            else {
                return;
            };
            if warn_files && held.warned != read.scan.problems {
                for problem in &read.scan.problems {
                    if !held.warned.contains(problem) {
                        tracing::warn!(
                            agent = %user,
                            file = problem.file(),
                            problem = %problem.sentence(&name),
                            "agents: a file in an agent's home is not its picture"
                        );
                    }
                }
                held.warned = read.scan.problems.clone();
            }
            let forget = || {
                failures
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&user);
            };
            let Some(file) = read.scan.avatar else {
                forget();
                return;
            };
            let now = Instant::now();
            if !held.memo.due(&file.hash, now) {
                return;
            }
            let Some(round) = round.filter(|round| copy.joined(&round.control)) else {
                tracing::debug!(agent = %user, "agents: no control room to claim the picture in yet");
                held.memo.held_elsewhere(&file.hash, now);
                return;
            };
            let claims = copy.keyed_claims(&round.control, &claim::picture_key(&user));
            let me = crate::hosts::claimant(&round.host, &*copy);
            let attempt = publish_held(
                claims.as_ref(),
                &me,
                &round.clock,
                &round.rtt,
                copy.client.client(),
                &file,
            )
            .await;
            match attempt {
                Attempt::Published(published) => {
                    if let Published::Uploaded(_) = published {
                        tracing::info!(agent = %user, file = file.file, "agents: the agent's picture is its profile avatar now");
                    }
                    held.memo.settle(&file.hash);
                    forget();
                }
                Attempt::HeldElsewhere => {
                    tracing::debug!(agent = %user, "agents: another host holds the picture's claim; it publishes");
                    held.memo.held_elsewhere(&file.hash, now);
                }
                Attempt::Failed(error) => {
                    tracing::warn!(agent = %user, file = file.file, %error, "agents: the agent's picture could not be set as its profile avatar");
                    held.memo.fail(&file.hash, now);
                    failures
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert(user.clone(), error);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::Duration;

    use keeper_core::agents::avatar::AvatarRecordContent;
    use keeper_core::agents::matrix::{AgentMatrixError, ServerState};
    use matrix_sdk::ruma::{MilliSecondsSinceUnixEpoch, MxcUri, OwnedEventId, OwnedMxcUri, UInt};
    use serde_json::Value;
    use tokio::sync::Barrier;

    use super::*;
    use crate::claims::ClaimFuture;

    const T0: u64 = 1_790_000_000_000;

    fn nixi() -> OwnedUserId {
        OwnedUserId::try_from("@nixi:example.org").expect("user")
    }

    fn claimant(host: &str) -> Claimant {
        Claimant {
            host: host.to_owned(),
            device: host.to_uppercase(),
            agent: nixi(),
        }
    }

    /// The control room's picture claim as the homeserver holds it, with
    /// the race two takers inside one round trip meet: until both have
    /// waited a sync round, each one's read-back answers its own write.
    #[derive(Default)]
    struct ClaimServer {
        current: Mutex<Option<ServerState>>,
        sent: AtomicU32,
        racing: AtomicBool,
    }

    struct Claims {
        server: Arc<ClaimServer>,
        own: Mutex<Option<ServerState>>,
        /// Both takers read the claim before either writes.
        start: Option<Arc<Barrier>>,
        started: AtomicBool,
        /// Both takers wait their sync round together.
        round: Option<Arc<Barrier>>,
    }

    impl Claims {
        fn new(server: &Arc<ClaimServer>, race: Option<(Arc<Barrier>, Arc<Barrier>)>) -> Claims {
            let (start, round) = race.unzip();
            Claims {
                server: Arc::clone(server),
                own: Mutex::new(None),
                start,
                started: AtomicBool::new(false),
                round,
            }
        }
    }

    impl ClaimPort for Claims {
        fn send(&self, content: Value) -> ClaimFuture<'_, Result<OwnedEventId, AgentMatrixError>> {
            Box::pin(async move {
                let sent = self.server.sent.fetch_add(1, Ordering::SeqCst) + 1;
                let event =
                    OwnedEventId::try_from(format!("$claim{sent}:example.org")).expect("event");
                let state = ServerState {
                    event_id: event.clone(),
                    sender: nixi(),
                    origin_server_ts: MilliSecondsSinceUnixEpoch(
                        UInt::new(T0 + u64::from(sent)).expect("ts"),
                    ),
                    content,
                };
                *self.own.lock().expect("lock") = Some(state.clone());
                *self.server.current.lock().expect("lock") = Some(state);
                Ok(event)
            })
        }

        fn read(&self) -> ClaimFuture<'_, Result<Option<ServerState>, AgentMatrixError>> {
            Box::pin(async move {
                if let Some(start) = &self.start {
                    if !self.started.swap(true, Ordering::SeqCst) {
                        let current = self.server.current.lock().expect("lock").clone();
                        start.wait().await;
                        return Ok(current);
                    }
                }
                let own = self.own.lock().expect("lock").clone();
                if self.server.racing.load(Ordering::SeqCst) && own.is_some() {
                    return Ok(own);
                }
                Ok(self.server.current.lock().expect("lock").clone())
            })
        }

        fn next_sync(&self) -> ClaimFuture<'_, Duration> {
            Box::pin(async move {
                if let Some(round) = &self.round {
                    if round.wait().await.is_leader() {
                        self.server.racing.store(false, Ordering::SeqCst);
                    }
                    round.wait().await;
                }
                Duration::from_millis(30)
            })
        }
    }

    /// The agent's account on the homeserver: its record, its profile, and
    /// every upload, with a step that fails on demand.
    #[derive(Default)]
    struct Account {
        record: Mutex<Option<AvatarRecordContent>>,
        profile: Mutex<Option<OwnedMxcUri>>,
        uploads: AtomicU32,
        fail_upload: AtomicBool,
        fail_record: AtomicBool,
        /// How many record reads still answer as before another host's
        /// publication landed.
        stale: Mutex<u32>,
    }

    impl AvatarPort for Account {
        fn record(
            &self,
        ) -> impl Future<Output = Result<Option<AvatarRecordContent>, String>> + Send {
            let stale = {
                let mut stale = self.stale.lock().expect("lock");
                let was = *stale > 0;
                *stale = stale.saturating_sub(1);
                was
            };
            let record = if stale {
                None
            } else {
                self.record.lock().expect("lock").clone()
            };
            async move { Ok(record) }
        }

        fn profile(&self) -> impl Future<Output = Result<Option<OwnedMxcUri>, String>> + Send {
            let profile = self.profile.lock().expect("lock").clone();
            async move { Ok(profile) }
        }

        fn upload(
            &self,
            _mime: &'static str,
            _bytes: Vec<u8>,
        ) -> impl Future<Output = Result<OwnedMxcUri, String>> + Send {
            let answer = if self.fail_upload.load(Ordering::SeqCst) {
                Err("the upload failed".to_owned())
            } else {
                let n = self.uploads.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(OwnedMxcUri::from(format!("mxc://example.org/{n}")))
            };
            async move { answer }
        }

        fn set_profile(&self, mxc: &MxcUri) -> impl Future<Output = Result<(), String>> + Send {
            *self.profile.lock().expect("lock") = Some(mxc.to_owned());
            async { Ok(()) }
        }

        fn write_record(
            &self,
            record: AvatarRecordContent,
        ) -> impl Future<Output = Result<(), String>> + Send {
            let answer = if self.fail_record.load(Ordering::SeqCst) {
                Err("its record could not be written".to_owned())
            } else {
                *self.record.lock().expect("lock") = Some(record);
                Ok(())
            };
            async move { answer }
        }
    }

    fn picture(svg: &str) -> AvatarFile {
        avatar::scan(&[], |file| {
            if file == "avatar.svg" {
                avatar::Candidate::Bytes(svg.as_bytes().to_vec())
            } else {
                avatar::Candidate::Absent
            }
        })
        .avatar
        .expect("a picture")
    }

    async fn attempt(claims: &Claims, host: &str, account: &Account, file: &AvatarFile) -> Attempt {
        publish_held(
            claims,
            &claimant(host),
            &ServerClock::default(),
            &Rtt::default(),
            account,
            file,
        )
        .await
    }

    /// R399 #4: two hosts serving one agent take its picture claim at once;
    /// one holds it and uploads, the other uploads nothing — and when it
    /// looks again, the holder's record settles it with no upload either.
    #[tokio::test(start_paused = true)]
    async fn two_hosts_one_claim_holder_one_upload() {
        let server = Arc::new(ClaimServer::default());
        server.racing.store(true, Ordering::SeqCst);
        let (start, round) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
        let electra = Claims::new(&server, Some((Arc::clone(&start), Arc::clone(&round))));
        let hesperia = Claims::new(&server, Some((start, round)));
        let account = Account::default();
        let file = picture("<svg/>");
        let (on_electra, on_hesperia) = tokio::join!(
            attempt(&electra, "electra", &account, &file),
            attempt(&hesperia, "hesperia", &account, &file),
        );
        let outcomes = [on_electra, on_hesperia];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Attempt::Published(Published::Uploaded(_))))
                .count(),
            1,
            "{outcomes:?}"
        );
        assert!(outcomes.contains(&Attempt::HeldElsewhere), "{outcomes:?}");
        assert_eq!(account.uploads.load(Ordering::SeqCst), 1);

        // The other host looks again later: the record answers it.
        let again = Claims::new(&server, None);
        assert_eq!(
            attempt(&again, "hesperia", &account, &file).await,
            Attempt::Published(Published::Unchanged)
        );
        assert_eq!(account.uploads.load(Ordering::SeqCst), 1);
    }

    /// R399 #4: while another host holds the agent's picture claim, a host
    /// whose file differs uploads nothing.
    #[tokio::test(start_paused = true)]
    async fn a_claim_held_elsewhere_uploads_nothing() {
        let server = Arc::new(ClaimServer::default());
        let holder = Claims::new(&server, None);
        let lease = match acquire(
            &holder,
            &claimant("electra"),
            &ServerClock::default(),
            &Rtt::default(),
            None,
        )
        .await
        .expect("acquire")
        {
            Acquired::Won { lease, .. } => lease,
            other => panic!("electra takes the free claim, not {other:?}"),
        };
        assert!(lease.may_write());
        let account = Account::default();
        let other = Claims::new(&server, None);
        let clock = ServerClock::default();
        let wall = crate::claims::wall_ms();
        clock.observe(T0 + 1_000, wall, wall);
        let outcome = publish_held(
            &other,
            &claimant("hesperia"),
            &clock,
            &Rtt::default(),
            &account,
            &picture("<svg/>"),
        )
        .await;
        assert_eq!(outcome, Attempt::HeldElsewhere);
        assert_eq!(account.uploads.load(Ordering::SeqCst), 0);
    }

    /// R399 #4: the holder reads the record again under the claim — another
    /// host's publication of the same file that landed while this one took
    /// the claim settles it with no upload.
    #[tokio::test(start_paused = true)]
    async fn the_record_is_read_again_under_the_claim() {
        let server = Arc::new(ClaimServer::default());
        let claims = Claims::new(&server, None);
        let account = Account::default();
        let file = picture("<svg/>");
        let mxc = OwnedMxcUri::from("mxc://example.org/elsewhere");
        *account.record.lock().expect("lock") = Some(AvatarRecordContent {
            v: keeper_core::agents::events::CONTENT_VERSION,
            hash: file.hash.clone(),
            mxc: mxc.clone(),
        });
        *account.profile.lock().expect("lock") = Some(mxc);
        *account.stale.lock().expect("lock") = 1;
        assert_eq!(
            attempt(&claims, "electra", &account, &file).await,
            Attempt::Published(Published::Unchanged)
        );
        assert_eq!(account.uploads.load(Ordering::SeqCst), 0);
    }

    /// R399 #3: A is published; B fails — before the profile changed, or
    /// after it did — and nothing stays settled; the file put back to A is
    /// asked of the server at once and settles with no upload when the
    /// profile still shows A, and is uploaded again when it shows B.
    #[tokio::test(start_paused = true)]
    async fn a_failure_leaves_nothing_settled_and_the_file_put_back_is_checked() {
        let server = Arc::new(ClaimServer::default());
        let claims = Claims::new(&server, None);
        let account = Account::default();
        let (a, b) = (picture("<svg>a</svg>"), picture("<svg>b</svg>"));
        let mut memo = PublishMemo::default();
        let now = Instant::now();
        assert!(matches!(
            attempt(&claims, "electra", &account, &a).await,
            Attempt::Published(Published::Uploaded(_))
        ));
        memo.settle(&a.hash);

        // B's upload fails: the profile still shows A.
        account.fail_upload.store(true, Ordering::SeqCst);
        assert!(memo.due(&b.hash, now));
        assert!(matches!(
            attempt(&claims, "electra", &account, &b).await,
            Attempt::Failed(_)
        ));
        memo.fail(&b.hash, now);
        account.fail_upload.store(false, Ordering::SeqCst);
        assert!(memo.due(&a.hash, now), "the file put back is asked again");
        assert_eq!(
            attempt(&claims, "electra", &account, &a).await,
            Attempt::Published(Published::Unchanged)
        );
        memo.settle(&a.hash);
        assert_eq!(account.uploads.load(Ordering::SeqCst), 1);

        // B's record cannot be written after its profile was set: the
        // profile shows B, so A put back is published again.
        account.fail_record.store(true, Ordering::SeqCst);
        assert!(matches!(
            attempt(&claims, "electra", &account, &b).await,
            Attempt::Failed(_)
        ));
        memo.fail(&b.hash, now);
        account.fail_record.store(false, Ordering::SeqCst);
        assert!(memo.due(&a.hash, now));
        assert!(matches!(
            attempt(&claims, "electra", &account, &a).await,
            Attempt::Published(Published::Uploaded(_))
        ));
        assert_eq!(account.uploads.load(Ordering::SeqCst), 3);
        assert!(!memo.due(&b.hash, now), "B waits for its retry");
    }
}
