//! One drive-maintenance job per drive against a real homeserver (story
//! 95.2 acceptance 8, R207): two hosts — one starting the night's
//! consolidation, the other the weekly curator — take the drive's one
//! maintenance claim `maintain:<drive>` in one control room at once,
//! through `maintain::maintain` the hosts use — the read-back and the
//! re-read after the settle on Synapse itself. `#[ignore]`; endpoints and
//! secrets come from the environment, as in `live_claims.rs`:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_consolidate -- --ignored --nocapture --test-threads=1
//! ```

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::sync::Arc;

use keeper_agent::claims::{wall_ms, ClaimPort, RoomClaims, Rtt, ServerClock};
use keeper_agent::consolidate::night_window;
use keeper_agent::maintain::{maintain, Maintained};
use keeper_core::agents::claim::{self, Claimant, ServerClaim, RENEW_EVERY};
use keeper_core::agents::matrix::RoomKind;

mod common;

use common::Smoke;

const HOSTS: [&str; 2] = ["electra-sim", "hesperia-sim"];

/// Acceptance 8 and R207: at the night's window one host starts the
/// consolidation and another the curator, at once; exactly one runs and the
/// other is told who holds the drive. The holder keeps the claim through
/// renewals longer than the stop deadline, records its own window and
/// releases; its job is then done for the window, and the other job — whose
/// completion is its own — runs.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn one_consolidator_per_drive_on_synapse() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let agent = smoke.user("nixi-smoke");
    smoke.clear_devices(&agent).await;
    smoke.set_ratelimit(&agent, 0, 0).await;
    let password = smoke.secret("NIXI_SMOKE_PASSWORD").to_owned();
    let maker = smoke
        .client(&root.path().join("maker"), "nixi-smoke", &password)
        .await;
    let control = maker
        .create_room(RoomKind::Control, "consolidation", Vec::new(), &[])
        .await
        .expect("control room");
    let lease = claim::maintenance_key("smoke");
    let jobs = ["consolidate", "curate"];

    let mut copies = Vec::new();
    for (slug, job) in HOSTS.into_iter().zip(jobs) {
        let client = smoke
            .client(&root.path().join(slug), "nixi-smoke", &password)
            .await;
        let (rounds, seen) = tokio::sync::watch::channel(0u64);
        let syncer = client.client().clone();
        tokio::spawn(async move {
            let rounds = &rounds;
            let _ = syncer
                .sync_with_callback(
                    keeper_core::agents::matrix::sync_settings(),
                    |_| async move {
                        rounds.send_modify(|n| *n += 1);
                        matrix_sdk::LoopCtrl::Continue
                    },
                )
                .await;
        });
        let me = Claimant {
            host: slug.to_owned(),
            device: client.device_id().expect("device"),
            agent: agent.clone(),
        };
        let done = claim::completion_key(job, "smoke");
        copies.push((
            Arc::new(RoomClaims::keyed(
                client.clone(),
                control.clone(),
                &lease,
                seen.clone(),
            )) as Arc<dyn ClaimPort>,
            RoomClaims::keyed(client, control.clone(), &done, seen),
            me,
            Arc::new(ServerClock::default()),
            Arc::new(Rtt::default()),
        ));
    }
    let window = night_window(wall_ms() as i64, 0).expect("tonight's window");
    let run = |at: usize| {
        let (lease, done, me, clock, rtt) = &copies[at];
        maintain(
            lease,
            done,
            me,
            clock,
            rtt,
            Some(window),
            |fence| async move {
                // Longer than a holder may go unrenewed: renewed on the server.
                tokio::time::sleep(claim::STOP_WITHOUT_RENEWAL + RENEW_EVERY).await;
                (fence(), true)
            },
        )
    };
    let (a, b) = tokio::join!(run(0), run(1));
    let outcomes = [a.expect("electra-sim"), b.expect("hesperia-sim")];
    println!("one drive's maintenance taken by two hosts' jobs at once on Synapse: {outcomes:?}");
    let runs: Vec<usize> = (0..2)
        .filter(|at| matches!(outcomes[*at], Maintained::Ran { .. }))
        .collect();
    assert_eq!(runs.len(), 1, "one maintenance job: {outcomes:?}");
    let (holder, other) = (runs[0], 1 - runs[0]);
    match &outcomes[other] {
        Maintained::HeldBy(host) => assert_eq!(host, HOSTS[holder]),
        taken => panic!("the other job runs nothing: {taken:?}"),
    }
    assert!(
        matches!(
            outcomes[holder],
            Maintained::Ran {
                out: true,
                recorded: true
            }
        ),
        "renewed while it ran, recorded and released: {:?}",
        outcomes[holder]
    );
    let (lease_port, done_port, ..) = &copies[holder];
    let held = lease_port
        .read()
        .await
        .expect("read")
        .and_then(|state| ServerClaim::read(&state).ok())
        .expect("the claim");
    assert!(held.content.released);
    let recorded = done_port
        .read()
        .await
        .expect("read")
        .and_then(|state| ServerClaim::read(&state).ok())
        .expect("the completion");
    assert_eq!(
        recorded.content.window,
        Some(claim::rfc3339(u64::try_from(window).expect("after 1970")))
    );
    // The holder's job is done for the window; the other job's is its own,
    // and runs now that the drive is free.
    let (lease, done, me, clock, rtt) = &copies[holder];
    assert!(matches!(
        maintain(lease, done, me, clock, rtt, Some(window), |_| async {
            (false, true)
        })
        .await
        .expect("again"),
        Maintained::Done
    ));
    let (lease, done, me, clock, rtt) = &copies[other];
    assert!(matches!(
        maintain(lease, done, me, clock, rtt, Some(window), |_| async {
            (true, true)
        })
        .await
        .expect("its own"),
        Maintained::Ran { recorded: true, .. }
    ));
}
