//! R461, R511 (P2-04): a panic's message reaches the export boundary only as
//! the real hook (`keeper_sync::logfile::install_panic_hook`) bounds it, and
//! that bound never leaves a fragment of a token it cut — a fragment shorter
//! than any class the redactor recognises would leave this device as it is.
//!
//! Here because keeper-agent is the one crate built on Linux that links both
//! the producer (keeper-sync, core-free by AD-40) and the boundary
//! (keeper-core's `LogExport` and `Telemetry`). One test in its own binary:
//! a panic hook is process-global.

use keeper_core::telemetry::{
    log_subscriber, LogExport, PublicConfig, Telemetry, TelemetryConsentVm,
};

#[test]
fn a_panic_cut_through_a_secret_leaves_no_fragment_of_it() {
    let dir = tempfile::tempdir().expect("data directory");
    let telemetry = Telemetry::open(
        Some(dir.path()),
        PublicConfig::parse("https://us.i.posthog.com", "phc_test_public_only"),
    );
    telemetry
        .set_consent(TelemetryConsentVm {
            diagnostics: true,
            ..TelemetryConsentVm::default()
        })
        .expect("consent");
    let export = LogExport::new();
    export.attach(&telemetry);
    let _subscriber = tracing::subscriber::set_default(log_subscriber(
        tracing_subscriber::EnvFilter::new("info"),
        std::io::sink,
        export,
    ));
    keeper_sync::logfile::install_panic_hook();

    // Each secret starts ten characters before the hook's 500-character
    // bound, so a plain cut would keep its first ten: too short for the
    // 16-digit hex class, the 24-character base64 class and the 16-character
    // serverless Matrix id class alike.
    let padding = "x ".repeat(245);
    let secrets = [
        "0123456789abcdef0123456789abcdef",
        "dGhpcyBpcyBhIHNlY3JldCB2YWx1ZQ",
        "$Rqnc-F-dvnEYJTyHq_iKxU2bZ1CI92-kuZq3a5lr5Zg",
    ];
    for secret in secrets {
        let message = format!("{padding}{secret} and more");
        let panicked = std::panic::catch_unwind(|| panic!("{message}"));
        assert!(panicked.is_err());
    }

    let payloads = telemetry.queued_batch_payloads();
    assert_eq!(payloads.len(), 3, "{payloads:?}");
    let all = payloads.join("\n");
    assert!(all.contains("keeper panicked"), "{all}");
    for secret in secrets {
        let fragment = &secret[..6];
        assert!(!all.contains(fragment), "{fragment} left in {all}");
    }
}

/// Live: the same real producer to the configured PostHog project, a panic
/// whose 32-digit hex secret starts ten characters before the hook's bound.
/// Needs `KEEPER_POSTHOG_HOST`, `KEEPER_POSTHOG_PROJECT_TOKEN` (the public
/// pair) and `KEEPER_SMOKE_MARKER`; the caller reads the panic back by HogQL
/// and counts every event holding the secret's first ten digits (R511).
#[tokio::test]
#[ignore = "live: sends one panic record to PostHog; run by hand with the public pair"]
async fn live_a_panic_cut_through_a_secret_reaches_posthog_without_it() {
    let (Ok(host), Ok(token), Ok(marker)) = (
        std::env::var("KEEPER_POSTHOG_HOST"),
        std::env::var("KEEPER_POSTHOG_PROJECT_TOKEN"),
        std::env::var("KEEPER_SMOKE_MARKER"),
    ) else {
        panic!("set KEEPER_POSTHOG_HOST, KEEPER_POSTHOG_PROJECT_TOKEN and KEEPER_SMOKE_MARKER");
    };
    let dir = tempfile::tempdir().expect("data directory");
    let telemetry = Telemetry::open(
        Some(dir.path()),
        Some(PublicConfig::parse(&host, &token).expect("the public pair")),
    );
    telemetry
        .set_consent(TelemetryConsentVm {
            diagnostics: true,
            ..TelemetryConsentVm::default()
        })
        .expect("consent");
    let worker = tokio::spawn(std::sync::Arc::clone(&telemetry).run());
    let export = LogExport::new();
    export.attach(&telemetry);
    let _subscriber = tracing::subscriber::set_default(log_subscriber(
        tracing_subscriber::EnvFilter::new("info"),
        std::io::sink,
        export,
    ));
    keeper_sync::logfile::install_panic_hook();
    let mut message = format!("panic smoke {marker} ");
    while message.chars().count() + 2 <= 490 {
        message.push_str("x ");
    }
    while message.chars().count() < 490 {
        message.push(' ');
    }
    message.push_str("9f8e7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c5b4a39281706f5e4d3c2b1a0 more");
    let panicked = std::panic::catch_unwind(|| panic!("{message}"));
    assert!(panicked.is_err());
    let queued = telemetry.queued_batch_payloads();
    assert_eq!(queued.len(), 1, "{queued:?}");
    assert!(!queued[0].contains("9f8e7d"), "{}", queued[0]);
    for _ in 0..50 {
        if telemetry.queued_batch_payloads().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    // The exporter's requests are bounded by its five-second timeout.
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    worker.abort();
    let _ = worker.await;
}
