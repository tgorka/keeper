use super::*;

struct LocalDir(PathBuf);
impl LocalDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("keeper-telemetry-{}", random_id::<16>()));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }
}
impl Drop for LocalDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn config() -> PublicConfig {
    PublicConfig::parse("https://us.i.posthog.com", "phc_test_public_only").expect("public fixture")
}
fn event(kind: TelemetryEventKind) -> TelemetryEventReq {
    TelemetryEventReq {
        kind,
        duration_ms: Some(12.0),
        surface: None,
    }
}
fn diagnostics() -> TelemetryConsentVm {
    TelemetryConsentVm {
        diagnostics: true,
        ..Default::default()
    }
}

#[test]
fn unknown_content_and_invalid_timings_cannot_enter_export_queue() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    for bytes in [
        r#"{"kind":"appReady","durationMs":null,"message":"PRIVATE_SENTINEL"}"#,
        r#"{"kind":"frontendError","durationMs":null}"#,
        r#"{"kind":"interaction","durationMs":1,"attributes":{"path":"PRIVATE_SENTINEL"}}"#,
        r#"{"kind":"recordingStarted","durationMs":1}"#,
    ] {
        assert!(serde_json::from_str::<TelemetryEventReq>(bytes).is_err());
    }
    for duration in [f64::NAN, f64::INFINITY, -1.0, MAX_DURATION_MS + 1.0] {
        assert!(telemetry
            .capture(TelemetryEventReq {
                kind: TelemetryEventKind::Interaction,
                duration_ms: Some(duration),
                surface: None,
            })
            .is_err());
    }
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
}

#[test]
fn consent_is_category_specific_revocation_discards_and_rotates_identity() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    assert!(telemetry
        .status()
        .expect("status")
        .installation_id
        .is_none());
    let first = telemetry
        .set_consent(TelemetryConsentVm {
            product_analytics: true,
            ..Default::default()
        })
        .expect("enable");
    for kind in [
        TelemetryEventKind::AppReady,
        TelemetryEventKind::Interaction,
        TelemetryEventKind::SettingsOpened,
        TelemetryEventKind::CommandPaletteOpened,
    ] {
        telemetry.capture(event(kind)).expect("capture");
    }
    {
        let inner = telemetry.inner.lock().expect("state");
        assert_eq!(inner.queue.len(), 2);
        assert!(inner.queue.iter().all(|record| !record.diagnostic()));
    }
    let enabled = Telemetry::open(Some(&local.0), Some(config()));
    assert_eq!(
        enabled.status().expect("restored").installation_id,
        first.installation_id
    );
    telemetry
        .set_consent(TelemetryConsentVm::default())
        .expect("revoke");
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
    assert!(telemetry.egress().is_none());
    assert!(Telemetry::open(Some(&local.0), Some(config()))
        .status()
        .expect("off")
        .installation_id
        .is_none());
    let next = telemetry.set_consent(diagnostics()).expect("reenable");
    assert_ne!(next.installation_id, first.installation_id);
    telemetry
        .capture(event(TelemetryEventKind::SettingsOpened))
        .expect("denied product");
    telemetry
        .capture(event(TelemetryEventKind::AppReady))
        .expect("diagnostic");
    let inner = telemetry.inner.lock().expect("state");
    assert_eq!(inner.queue.len(), 1);
    assert!(inner.queue[0].diagnostic());
}

#[test]
fn corrupted_version_or_synced_style_consent_fails_closed() {
    let local = LocalDir::new();
    let path = local.0.join("telemetry-consent-v1.json");
    for bytes in [
        "not json".to_owned(),
        r#"{"diagnostics":true,"productAnalytics":true,"remoteConfig":true}"#.to_owned(),
        r#"{"version":2,"consent":{"diagnostics":true,"productAnalytics":false,"remoteConfig":false},"installationId":"11111111111111111111111111111111"}"#.to_owned(),
        r#"{"version":1,"consent":{"diagnostics":true,"productAnalytics":false,"remoteConfig":false},"installationId":"@alice:private.example"}"#.to_owned(),
        " ".repeat(2049),
    ] {
        std::fs::write(&path, bytes).expect("write corrupt state");
        let telemetry = Telemetry::open(Some(&local.0), Some(config()));
        assert_eq!(telemetry.status().expect("status").consent, TelemetryConsentVm::default());
        assert!(telemetry.egress().is_none());
    }
    // R462: the account's settings table is the consent, and only `"1"` is on.
    std::fs::remove_file(&path).expect("remove corrupt state");
    crate::registry::set_setting(&local.0, "telemetry.diagnostics", "true").expect("row");
    assert!(!Telemetry::open(Some(&local.0), Some(config()))
        .status()
        .expect("status")
        .consent
        .any());
    crate::registry::set_telemetry_consent(&local.0, true, false).expect("rows");
    assert!(
        Telemetry::open(Some(&local.0), Some(config()))
            .status()
            .expect("status")
            .consent
            .diagnostics
    );
}

#[test]
fn failed_persistence_never_enables_and_revoke_failure_still_clears_memory() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry
        .set_consent(diagnostics())
        .expect("initial consent");
    telemetry
        .capture(event(TelemetryEventKind::AppReady))
        .expect("queued");
    let file = local.0.join("telemetry-consent-v1.json");
    std::fs::remove_file(&file).expect("replace file with directory");
    std::fs::create_dir(&file).expect("block atomic rename");
    assert!(telemetry
        .set_consent(TelemetryConsentVm::default())
        .is_err());
    assert!(!telemetry.status().expect("status").consent.any());
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
    assert!(telemetry.egress().is_none());
    assert!(telemetry.set_consent(diagnostics()).is_err());
    assert!(!telemetry.status().expect("status").consent.any());
    assert!(!Telemetry::open(Some(&local.0), Some(config()))
        .status()
        .expect("restored")
        .consent
        .any());
}

#[test]
fn public_config_refuses_privileged_tokens_and_credential_bearing_destinations() {
    for token in [
        "phx_synthetic_private",
        "phs_synthetic_secure",
        "phc_has whitespace",
        "phc_x",
    ] {
        assert!(PublicConfig::parse("https://us.i.posthog.com", token).is_none());
    }
    for host in [
        "http://us.i.posthog.com",
        "https://user:pass@us.i.posthog.com",
        "https://us.i.posthog.com/path",
        "https://us.i.posthog.com/?token=x",
        "https://us.i.posthog.com/#fragment",
    ] {
        assert!(PublicConfig::parse(host, "phc_test_public_only").is_none());
    }
    assert!(PublicConfig::parse("https://eu.i.posthog.com/", "phc_test_public_only").is_some());
}

#[test]
fn capture_is_bounded_even_when_worker_is_offline_or_caller_floods() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    for _ in 0..10_000 {
        telemetry
            .capture(event(TelemetryEventKind::Interaction))
            .expect("drop not block");
    }
    let mut inner = telemetry.inner.lock().expect("state");
    assert_eq!(inner.queue.len(), EVENTS_PER_MINUTE);
    assert_eq!(inner.admitted.len(), EVENTS_PER_MINUTE);
    // Simulate a later interval without clock sleeps: rate limit may reopen,
    // but the independent memory limit still refuses the next record.
    inner.admitted.clear();
    while inner.queue.len() < QUEUE_LIMIT {
        inner
            .queue
            .push_back(Record::frontend(event(TelemetryEventKind::Interaction)));
    }
    drop(inner);
    telemetry
        .capture(event(TelemetryEventKind::Interaction))
        .expect("full queue");
    let inner = telemetry.inner.lock().expect("state");
    assert_eq!(inner.queue.len(), QUEUE_LIMIT);
    for record in &inner.queue {
        assert!(wire::logs(record).len() < REQUEST_LIMIT);
        assert!(wire::traces(record).len() < REQUEST_LIMIT);
    }
}

fn flags(payload: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"flags": {"keeper-client-config": {
        "enabled": true, "metadata": {"payload": payload.to_string()}
    }}}))
    .expect("flags fixture")
}

#[test]
fn remote_config_cannot_change_collection_destinations_or_render_active_content() {
    let good = flags(serde_json::json!({"supportMessage":"Thanks for testing Keeper!"}));
    assert_eq!(
        parse_remote(&good)
            .expect("plain text")
            .support_message
            .as_deref(),
        Some("Thanks for testing Keeper!")
    );
    for payload in [
        serde_json::json!({"supportMessage":"Hello", "diagnostics":true}),
        serde_json::json!({"supportMessage":"Hello", "host":"https://elsewhere.example"}),
        serde_json::json!({"supportMessage":"<script>capture()</script>"}),
        serde_json::json!({"supportMessage":"See https://elsewhere.example"}),
        serde_json::json!({"supportMessage":"See www.example.com"}),
        serde_json::json!({"supportMessage":"private\nmultiline"}),
        serde_json::json!({"supportMessage":"a".repeat(201)}),
        serde_json::json!({"supportMessage":42}),
    ] {
        assert!(parse_remote(&flags(payload)).is_none());
    }
    let mut disabled: serde_json::Value = serde_json::from_slice(&good).expect("json");
    disabled["flags"]["keeper-client-config"]["enabled"] = false.into();
    assert!(parse_remote(&serde_json::to_vec(&disabled).expect("disabled")).is_none());
    assert!(parse_remote(&vec![b' '; RESPONSE_LIMIT + 1]).is_none());
}

#[test]
fn study_is_ephemeral_and_does_not_link_or_enable_normal_collection() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    assert!(telemetry.study_config().expect("study").is_some());
    assert!(telemetry.egress().is_some());
    let status = telemetry.status().expect("status");
    assert!(!status.consent.any());
    assert!(status.installation_id.is_none());
    telemetry
        .capture(event(TelemetryEventKind::AppReady))
        .expect("no normal consent");
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
    assert!(!local.0.join("telemetry-consent-v1.json").exists());
    telemetry.study_stop().expect("stop");
    assert!(telemetry.egress().is_none());
}

#[tokio::test]
async fn fresh_install_and_study_config_make_no_backend_network_request() {
    let local = LocalDir::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let host = format!("https://{}", listener.local_addr().expect("address"));
    let telemetry = Telemetry::open(
        Some(&local.0),
        PublicConfig::parse(&host, "phc_test_public_only"),
    );
    let worker = tokio::spawn(Arc::clone(&telemetry).run());
    telemetry
        .capture(event(TelemetryEventKind::AppReady))
        .expect("off capture");
    assert_eq!(
        telemetry.remote_config().await,
        TelemetryRemoteConfigVm::default()
    );
    assert!(telemetry
        .study_config()
        .expect("public config only")
        .is_some());
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
    worker.abort();
    let _ = worker.await;
}

#[tokio::test]
async fn revocation_cancels_pending_http_and_prevents_a_second_export() {
    use tokio::io::AsyncReadExt;
    let local = LocalDir::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let host = format!("https://{}", listener.local_addr().expect("address"));
    let telemetry = Telemetry::open(
        Some(&local.0),
        PublicConfig::parse(&host, "phc_test_public_only"),
    );
    telemetry.set_consent(diagnostics()).expect("consent");
    let worker = tokio::spawn(Arc::clone(&telemetry).run());
    telemetry
        .capture(event(TelemetryEventKind::AppReady))
        .expect("capture");
    let mut streams = Vec::new();
    let mut hello = [0u8; 4096];
    for _ in 0..3 {
        let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
            .await
            .expect("bounded connect")
            .expect("accept");
        assert!(stream.read(&mut hello).await.expect("TLS hello") > 0);
        streams.push(stream);
    }
    telemetry
        .capture(event(TelemetryEventKind::Interaction))
        .expect("second event");
    telemetry
        .set_consent(TelemetryConsentVm::default())
        .expect("revoke");
    for mut stream in streams {
        let closed = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut hello))
            .await
            .expect("revocation closes every concurrent request");
        assert!(matches!(closed, Ok(0) | Err(_)));
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
    worker.abort();
    let _ = worker.await;
}

#[test]
fn closed_events_carry_no_exception_and_a_problem_leaves_as_one_closed_exception() {
    for kind in [
        TelemetryEventKind::AppReady,
        TelemetryEventKind::Interaction,
        TelemetryEventKind::SettingsOpened,
        TelemetryEventKind::CommandPaletteOpened,
    ] {
        let record = Record::frontend(event(kind));
        let payload = batch_payload(&config(), "synthetic-installation", &record).expect("batch");
        let batch = payload["batch"].as_array().expect("event array");
        let timestamp = chrono::DateTime::parse_from_rfc3339(
            batch[0]["timestamp"].as_str().expect("event time"),
        )
        .expect("RFC3339 event timestamp");
        assert_eq!(
            timestamp.timestamp_millis(),
            (record.end / 1_000_000) as i64
        );
        assert_eq!(batch.len(), 1);
        assert_ne!(batch[0]["event"], "$exception");
    }
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    telemetry.capture_log(
        LogSite {
            level: tracing::Level::ERROR,
            target: "keeper::sync",
            file: Some("crates/keeper/src/sync.rs"),
            line: Some(42),
        },
        "push refused for @alice:example.org".to_owned(),
    );
    let inner = telemetry.inner.lock().expect("state");
    let record = inner.queue.front().expect("a queued problem");
    let payload = batch_payload(&config(), "synthetic-installation", record).expect("batch");
    let batch = payload["batch"].as_array().expect("event array");
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0]["event"], "$exception");
    let props = batch[0]["properties"].as_object().expect("properties");
    let keys: std::collections::BTreeSet<_> = props.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        std::collections::BTreeSet::from([
            "distinct_id",
            "consent_category",
            "event_source",
            "synthetic",
            "$process_person_profile",
            "$geoip_disable",
            "trace_id",
            "span_id",
            "$exception_list",
            "$exception_fingerprint",
            "$exception_level",
            "log_level",
            "log_target",
            "log_file",
            "log_line",
            "os",
            "app_version",
        ])
    );
    assert_eq!(props["$process_person_profile"], false);
    assert_eq!(props["$geoip_disable"], true);
    assert_eq!(props["consent_category"], "diagnostics");
    assert_eq!(props["$exception_level"], "error");
    assert_eq!(props["log_level"], "ERROR");
    assert_eq!(props["log_target"], "keeper::sync");
    assert_eq!(props["log_file"], "crates/keeper/src/sync.rs");
    assert_eq!(props["log_line"], 42);
    assert_eq!(
        props["$exception_list"],
        serde_json::json!([{
            "type": "keeper::sync",
            "value": "push refused for <matrix-id>",
            "mechanism": {"handled": true, "type": "generic"}
        }])
    );
    assert_eq!(props["trace_id"], record.trace_id);
}

#[test]
fn study_preview_neither_exposes_identity_nor_activates_egress() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    let preview = serde_json::to_value(telemetry.study_preview()).expect("preview");
    assert_eq!(
        preview,
        serde_json::json!({"configured": true, "host": "https://us.i.posthog.com"})
    );
    assert!(telemetry.egress().is_none());
    assert!(telemetry
        .status()
        .expect("status")
        .installation_id
        .is_none());
}

/// Every operation this module can export, by the name it is exported under.
/// No wildcard: an operation added to the module stops this file compiling
/// until its author says here what it carries — the closed set is the whole
/// of what can leave (ruling D4).
fn exported_name(operation: Operation) -> &'static str {
    match operation {
        Operation::Frontend(kind) => match kind {
            TelemetryEventKind::AppReady => "keeper_app_ready",
            TelemetryEventKind::CommandPaletteOpened => "keeper_command_palette_opened",
            TelemetryEventKind::SettingsOpened => "keeper_settings_opened",
            TelemetryEventKind::Interaction => "keeper_interaction",
        },
        Operation::MicLost(_) => "keeper_recording_mic_lost",
        Operation::Log => "$exception",
        Operation::FrontendError => "keeper_frontend_error",
        Operation::ProblemsDropped(_) => "keeper_problems_dropped",
        Operation::RemoteConfig => "keeper.remote_config",
        Operation::FlagsRequest => "keeper.flags_request",
    }
}

/// Runs `emit` under the shell's own subscriber shape ([`log_subscriber`]):
/// `keeper.log`'s writer under `env_filter` (here into a sink) beside the
/// export layer attached to `telemetry` under its own filter.
fn forwarding_under(telemetry: &Arc<Telemetry>, env_filter: &str, emit: impl FnOnce()) {
    let export = LogExport::new();
    export.attach(telemetry);
    let filter = tracing_subscriber::EnvFilter::new(env_filter);
    tracing::subscriber::with_default(log_subscriber(filter, std::io::sink, export), emit);
}

/// [`forwarding_under`] with the shell's default `keeper.log` filter.
fn forwarding(telemetry: &Arc<Telemetry>, emit: impl FnOnce()) {
    forwarding_under(telemetry, &keeper_sync_default_filter(), emit);
}

/// `keeper_sync::logfile::default_filter("info")`, spelled out: keeper-core
/// cannot depend on keeper-sync.
fn keeper_sync_default_filter() -> String {
    "info,gix_attributes=error,gix_worktree_state=error,gix_dir=warn,rmcp=off".to_owned()
}

/// Every queued record's `/batch/` body, as text.
fn queued_payloads(telemetry: &Telemetry) -> Vec<String> {
    let inner = telemetry.inner.lock().expect("state");
    inner
        .queue
        .iter()
        .map(|record| {
            assert_eq!(exported_name(record.operation), record.operation.name());
            batch_payload(&config(), "synthetic-installation", record)
                .expect("batch")
                .to_string()
        })
        .collect()
}

/// S-19 as amended by R461: a line under `keeper_agent…`,
/// `keeper_core::agents…` or `keeper_core::hq…` (since 108.2 the agents'
/// targets are the database's and the served turn's) leaves with its level, target, callsite and
/// fingerprint — never its message or its fields (spans, tool arguments,
/// paths, provider errors) — at either boundary: the log layer, and
/// [`Telemetry::capture_log`] handed a non-empty private message directly.
/// An ordinary line beside them keeps its redacted message.
#[test]
fn agent_target_lines_leave_without_their_message() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry
        .set_consent(TelemetryConsentVm {
            diagnostics: true,
            product_analytics: true,
            remote_config: true,
        })
        .expect("consent");
    forwarding(&telemetry, || {
        tracing::error!(target: "keeper_agent::served", path = "60-sessions/active/plan", "agentd: PRIVATE_SENTINEL turn");
        tracing::warn!(target: "keeper_core::hq::spoken", error = "PRIVATE_SENTINEL", "a provider error");
        tracing::warn!(target: "keeper::sync", "push held for PUBLIC_VISIBLE");
        tracing::warn!(target: "keeper_core::hq::syncular", why = "PRIVATE_SENTINEL", "agents server not reached");
    });
    telemetry.capture_log(
        LogSite {
            level: tracing::Level::WARN,
            target: "keeper_agent::tools",
            file: Some("crates/keeper-agent/src/tools.rs"),
            line: Some(9),
        },
        "tool read 60-sessions/active/plan: PRIVATE_SENTINEL error=\"provider error\"".to_owned(),
    );
    let payloads = queued_payloads(&telemetry);
    assert_eq!(payloads.len(), 5);
    for payload in [&payloads[0], &payloads[1], &payloads[3], &payloads[4]] {
        assert!(!payload.contains("PRIVATE_SENTINEL"), "{payload}");
        assert!(!payload.contains("60-sessions"), "{payload}");
        assert!(!payload.contains("provider error"), "{payload}");
        assert!(payload.contains("\"log_line\""), "{payload}");
        assert!(payload.contains("$exception_fingerprint"), "{payload}");
    }
    assert!(payloads[0].contains("\"keeper_agent::served\""));
    assert!(payloads[1].contains("\"keeper_core::hq::spoken\""));
    assert!(payloads[3].contains("\"keeper_core::hq::syncular\""));
    assert!(payloads[4].contains("\"keeper_agent::tools\""));
    assert!(payloads[2].contains("push held for PUBLIC_VISIBLE"));
}

/// R1082-03: the shell's agents-server lines stay home too. A spoken
/// question refused because its agent works in several databases names them;
/// logged by the shell's `hq_ipc` (its module target) or by another shell
/// module under [`SHELL_AGENTS_TARGET`], the line leaves with its target and
/// callsite and none of the names, at the log layer and at `capture_log`.
#[test]
fn the_shells_agents_server_lines_leave_without_the_persons_databases() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry
        .set_consent(TelemetryConsentVm {
            diagnostics: true,
            product_analytics: true,
            remote_config: true,
        })
        .expect("consent");
    let refusal = crate::hq::spoken::unchosen_sentence(
        "Nixie",
        &["cancer-care".to_owned(), "private-employer".to_owned()],
    );
    forwarding(&telemetry, || {
        tracing::warn!(target: "keeper_lib::hq_ipc", message = %refusal, "agents: a spoken question was not sent");
        tracing::warn!(target: SHELL_AGENTS_TARGET, why = %refusal, "push: the agents item could not be read");
        tracing::warn!(target: "keeper_lib::hq_blob_protocol", "not served: {refusal}");
        tracing::warn!(target: "keeper_lib::sync_ipc", "push held for PUBLIC_VISIBLE");
    });
    telemetry.capture_log(
        LogSite {
            level: tracing::Level::WARN,
            target: "keeper_lib::hq_ipc",
            file: Some("crates/keeper/src/hq_ipc.rs"),
            line: Some(920),
        },
        refusal.clone(),
    );
    let payloads = queued_payloads(&telemetry);
    assert_eq!(payloads.len(), 5);
    for payload in [&payloads[0], &payloads[1], &payloads[2], &payloads[4]] {
        assert!(!payload.contains("cancer-care"), "{payload}");
        assert!(!payload.contains("private-employer"), "{payload}");
        assert!(!payload.contains("Nixie"), "{payload}");
        assert!(payload.contains("$exception_fingerprint"), "{payload}");
    }
    assert!(payloads[0].contains("\"keeper_lib::hq_ipc\""));
    assert!(payloads[1].contains("\"keeper_lib::hq\""));
    assert!(payloads[2].contains("\"keeper_lib::hq_blob_protocol\""));
    assert!(
        payloads[3].contains("push held for PUBLIC_VISIBLE"),
        "an ordinary shell line keeps its words"
    );
}

/// R461: every WARN and ERROR — a panic line included — is forwarded with
/// its fields, redacted; INFO and below never are.
#[test]
fn every_warning_and_error_is_forwarded_redacted_and_nothing_quieter() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    forwarding(&telemetry, || {
        tracing::info!(target: "keeper::sync", "pulled");
        tracing::debug!(target: "keeper::sync", "pulled");
        tracing::warn!(target: "keeper::sync", room = %"!r:example.org", path = ?"/Users/m/x y", "push held");
        tracing::error!(target: "keeper::panic", location = "crates/keeper/src/x.rs:1:2", thread = %"main", reason = %"index out of bounds", "keeper panicked");
    });
    let inner = telemetry.inner.lock().expect("state");
    let problems: Vec<&Exception> = inner
        .queue
        .iter()
        .map(|record| record.exception.as_deref().expect("a problem"))
        .collect();
    assert_eq!(problems.len(), 2);
    assert_eq!(problems[0].level, tracing::Level::WARN);
    assert_eq!(
        problems[0].message.as_deref(),
        Some(r#"push held room=<matrix-id> path="<path>"#)
    );
    assert_eq!(problems[1].target, "keeper::panic");
    assert_eq!(
        problems[1].message.as_deref(),
        Some(
            r#"keeper panicked location="crates/keeper/src/x.rs:1:2" thread=main reason=index out of bounds"#
        )
    );
}

/// R495 08: the export layer has its own level policy. Under the shell's
/// composed subscriber, a WARN that `keeper.log`'s `EnvFilter` holds down
/// (`gix_attributes=error`, or `RUST_LOG=error` for everything) is still
/// forwarded, while `rmcp` — which may carry an MCP server's words — never
/// is, at any level `RUST_LOG` asks for.
#[test]
fn the_export_layer_keeps_its_own_level_policy_under_the_shells_filter() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    forwarding(&telemetry, || {
        tracing::warn!(target: "gix_attributes::search", "held down in keeper.log");
    });
    forwarding_under(&telemetry, "error,rmcp=off", || {
        tracing::warn!(target: "keeper::sync", "a warning RUST_LOG=error hides");
    });
    forwarding_under(&telemetry, "trace", || {
        tracing::error!(target: "rmcp::service", "an MCP server said PRIVATE_SENTINEL");
        tracing::info!(target: "keeper::sync", "still not a problem");
    });
    let targets: Vec<String> = telemetry
        .inner
        .lock()
        .expect("state")
        .queue
        .iter()
        .filter_map(|record| record.exception.as_deref())
        .map(|problem| problem.target.clone())
        .collect();
    assert_eq!(targets, ["gix_attributes::search", "keeper::sync"]);
}

/// R498, R511 (P2-05): only what the exporter itself produces is held back —
/// this module's lines, and whatever its HTTP stack logs on the exporter's
/// own threads, a task that work SPAWNS included (hyper-util's
/// `TokioExecutor` runs each HTTP/2 connection's dispatcher, whose h2 PING
/// handling warns, through exactly this `tokio::spawn`) — plus the webview's
/// report line, which leaves as its own record. The same HTTP crates' lines
/// from any other runtime's tasks are problems like any other, redacted.
/// Diagnostics off forwards nothing at all.
///
/// A test subscriber is thread-local, so each task installs it while it
/// logs; the app's is global.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_exporters_own_lines_are_held_back() {
    use tracing::instrument::WithSubscriber as _;
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    forwarding(&telemetry, || {
        tracing::warn!(target: "keeper::sync", "off: not forwarded");
    });
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
    telemetry.set_consent(diagnostics()).expect("consent");
    let export = LogExport::new();
    export.attach(&telemetry);
    let dispatch = tracing::Dispatch::new(log_subscriber(
        tracing_subscriber::EnvFilter::new(keeper_sync_default_filter()),
        std::io::sink,
        export,
    ));
    let inside = dispatch.clone();
    telemetry
        .transport
        .run(async move {
            let connection = inside.clone();
            tokio::spawn(
                async {
                    tracing::warn!(target: "h2::proto::ping_pong", "the export's connection: unsolicited PING ack from https://us.i.posthog.com/i/v1/logs");
                }
                .with_subscriber(connection),
            )
            .await
            .expect("the connection task");
            tokio::task::spawn_blocking(move || {
                tracing::dispatcher::with_default(&inside, || {
                    tracing::warn!(target: "reqwest::connect", "the export's own resolver failed");
                });
            })
            .await
            .expect("the resolver");
        })
        .await
        .expect("the transport runs");
    let outside = dispatch.clone();
    tokio::spawn(
        async {
            tracing::warn!(target: "h2::proto::ping_pong", "a sync connection: unsolicited PING ack from https://forge.example/u/alice/private.git");
            tracing::warn!(target: "reqwest::connect", "a sync request's connect error");
        }
        .with_subscriber(outside),
    )
    .await
    .expect("a sync task");
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::error!(target: "keeper_core::telemetry", "export failed");
        tracing::error!(target: "keeper::frontend", "frontend error");
    });
    let payloads = queued_payloads(&telemetry);
    let targets: Vec<String> = telemetry
        .inner
        .lock()
        .expect("state")
        .queue
        .iter()
        .filter_map(|record| record.exception.as_deref())
        .map(|problem| problem.target.clone())
        .collect();
    assert_eq!(targets, ["h2::proto::ping_pong", "reqwest::connect"]);
    let all = payloads.join("\n");
    assert!(all.contains("a sync connection"), "{all}");
    for private in ["the export's", "alice", "private.git"] {
        assert!(!all.contains(private), "{private} left in {all}");
    }
}

/// R495 01–03, through the layer and out as the exported JSON: a credential
/// inside a string field (escaped by `Debug`), a home path in every shape a
/// field takes — `%path.display()` with spaces as `archive/ingest.rs` logs
/// it, single-quoted, `Debug`-escaped Windows separators — a serverless
/// event id and an all-lowercase base64 run. None of it is anywhere in the
/// payload.
#[test]
fn adversarial_fields_leave_nothing_private_in_the_payload() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    let root = PathBuf::from("/Users/alice/My Drive/private note.md");
    let login_error = String::from(r#"login failed: password="two private words""#);
    forwarding(&telemetry, || {
        tracing::warn!(target: "keeper::account", error = %login_error, "sign-in refused");
        tracing::warn!(target: "keeper::account", error = ?login_error, "sign-in refused again");
        tracing::warn!(target: "keeper::account", "body {}", r#"{"token": "two private words"}"#);
        tracing::warn!(
            target: "keeper_core::archive::ingest",
            root = %root.display(),
            root_kind = %"recordings",
            "archive: could not rebuild the recordings index"
        );
        tracing::warn!(target: "keeper::files", path = "'/Users/alice/My Drive/x y.md'", "quoted");
        tracing::warn!(target: "keeper::files", path = ?r"C:\Users\alice\private dir\x.txt", "windows");
        tracing::warn!(target: "keeper::rooms", event = %"$abcdefghijklmnopqrstuvwxyzabcdefghijklmnopq", "serverless");
        tracing::warn!(target: "keeper::rooms", nonce = %"abcdefghijklmnopqrstuvwxyzab", "lowercase run");
    });
    let payloads = queued_payloads(&telemetry);
    assert_eq!(payloads.len(), 8);
    let all = payloads.join("\n");
    for secret in [
        "two private words",
        "private",
        "alice",
        "My Drive",
        "note.md",
        "x y.md",
        "abcdefghijklmnopqrstuvwxyz",
    ] {
        assert!(!all.contains(secret), "{secret} left in {all}");
    }
    assert!(all.contains("archive: could not rebuild the recordings index root=<path>"));
}

/// R515, through the layer and out as the exported JSON: from a home root
/// to the end of its field value or message, nothing is kept. A real path
/// whose filename holds a quote and ends in a literal backslash, written
/// the way `Debug` (`?path`, `{:?}`), `Debug` of that `Debug` and a
/// hand-written `'{}'` / `"{}"` of its `display()` give it; two paths in
/// one message; a path followed by a quoted secret; a path inside a
/// `Debug`-quoted field.
#[test]
fn nothing_after_a_home_root_leaves_in_the_payload() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    let quoted = PathBuf::from("/Users/alice/a\" private note.md");
    let apostrophe = PathBuf::from("/Users/alice/Marta's private plan.md");
    let backslash = PathBuf::from(r"/Users/alice/a' private note\");
    let double = PathBuf::from(r#"/Users/alice/a" private note\"#);
    let secret = "two private words";
    forwarding(&telemetry, || {
        tracing::warn!(target: "keeper::files", path = ?quoted, kind = "NotFound", "open failed");
        tracing::warn!(target: "keeper::files", "open {:?} failed: denied", quoted);
        tracing::warn!(target: "keeper::files", error = ?format!("read {quoted:?}: denied"), "nested");
        tracing::warn!(target: "keeper::files", "copy '{}' to the drive", apostrophe.display());
        tracing::warn!(target: "keeper::files", "copy '{}' to /tmp/public", backslash.display());
        tracing::warn!(target: "keeper::files", "copy \"{}\" to /tmp/public", double.display());
        tracing::warn!(target: "keeper::files", path = %format!("'{}'", backslash.display()), "field");
        tracing::warn!(
            target: "keeper::files",
            "copy '/tmp/source' to \"{}\" then '{}'",
            double.display(),
            apostrophe.display()
        );
        tracing::warn!(target: "keeper::files", "open {:?} with {secret:?}", quoted);
        tracing::warn!(
            target: "keeper::files",
            error = ?format!("open {:?} with {secret:?}", backslash),
            "debug field"
        );
    });
    let payloads = queued_payloads(&telemetry);
    assert_eq!(payloads.len(), 10);
    let all = payloads.join("\n");
    for private in [
        "private",
        "plan.md",
        "alice",
        "Marta",
        "words",
        "the drive",
        "/tmp/public",
    ] {
        assert!(!all.contains(private), "{private} left in {all}");
    }
    assert!(all.contains(r#"open failed path=\"<path>"#), "{all}");
    assert!(all.contains("copy '/tmp/source' to \\\"<path>"), "{all}");
}

/// R518, through the layer and out as the exported JSON: a home root spelled
/// in another case — `/users/` names `/Users/` on a case-insensitive macOS
/// volume, and `account_restore.rs` logs a drive's configured folder as
/// typed, `%error, path = %folder.display()` — and a home root inside a
/// `file://` URL both take the rest of their field or message. The private
/// filename is short and spaced, so no hex or base64 run can hide it by
/// accident.
#[test]
fn a_home_root_in_any_case_or_inside_a_file_url_leaves_nothing_in_the_payload() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    let folders = [
        PathBuf::from("/users/alice/a b.md"),
        PathBuf::from("/uSeRs/alice/a b.md"),
        PathBuf::from(r"C:\users\alice\a b.md"),
        PathBuf::from("c:/USERS/alice/a b.md"),
    ];
    forwarding(&telemetry, || {
        for folder in &folders {
            tracing::warn!(
                target: "keeper::account_restore",
                %error,
                path = %folder.display(),
                "account: a drive's folder could not be made; it waits"
            );
            tracing::warn!(target: "keeper_sync::watch", root = ?folder, "watch: the drive's folder is gone");
            tracing::warn!(target: "keeper::files", "open {} then {:?}", folder.display(), "private tail");
        }
        tracing::warn!(target: "keeper::files", "open file:///Users/alice/a b.md then \"private tail\"");
        tracing::warn!(target: "keeper::files", "open file:///users/alice/a b.md then \"private tail\"");
        tracing::warn!(target: "keeper::files", "open FILE:///home/alice/a b.md then \"private tail\"");
        tracing::warn!(target: "keeper::files", "open file:///C:/Users/alice/a b.md then \"private tail\"");
        tracing::warn!(
            target: "keeper::files",
            url = %"file:///c:/users/alice/a b.md then \"private tail\"",
            "open failed"
        );
    });
    let payloads = queued_payloads(&telemetry);
    assert_eq!(payloads.len(), 17);
    let all = payloads.join("\n");
    for private in ["alice", "a b.md", "b.md", "private tail"] {
        assert!(!all.contains(private), "{private} left in {all}");
    }
    assert!(
        all.contains("account: a drive's folder could not be made; it waits error=permission denied path=<path>"),
        "{all}"
    );
    assert!(all.contains("open file://<path>"), "{all}");
}

/// R495 04: a webview report is redacted before any cap — a secret just
/// past the log line's 200 characters, one just under the 1,000-character
/// export cap, and an identifier the transport bound cut inside its syntax
/// leave no fragment. The first is the fixture `crash-report.test.ts`'s
/// "the export path hands Rust a secret whole" sends uncut.
#[test]
fn a_webview_report_is_redacted_before_any_cap() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    let hex = "0123456789abcdef0123456789abcdef";
    let base64 = "dGhpcyBpcyBhIHNlY3JldCB2YWx1ZQ";
    telemetry.capture_frontend_error("console.error", &format!("{}{hex}", "x ".repeat(95)), false);
    telemetry.capture_frontend_error(
        "console.error",
        &format!("{}{base64}", "w ".repeat(495)),
        false,
    );
    // Past the input bound, the cut lands nine digits into the hex run.
    telemetry.capture_frontend_error(
        "console.warn",
        &format!("{} {hex}", "A".repeat(FRONTEND_INPUT_CAP - 10)),
        true,
    );
    let payloads = queued_payloads(&telemetry);
    assert_eq!(payloads.len(), 3);
    let all = payloads.join("\n");
    for fragment in ["012345", "dGhpcyBp"] {
        assert!(!all.contains(fragment), "{fragment} left in {all}");
    }
}

/// A fingerprint is the target and the callsite: one callsite is one issue,
/// another line is another, and a dependency's file never carries the
/// builder's home.
#[test]
fn a_fingerprint_is_the_target_and_the_callsite() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    forwarding(&telemetry, || {
        for round in 0..2 {
            tracing::warn!(target: "keeper::sync", round, "same callsite");
        }
        tracing::warn!(target: "keeper::sync", "another callsite");
        tracing::warn!(target: "keeper::other", "another target");
    });
    let inner = telemetry.inner.lock().expect("state");
    let prints: Vec<&str> = inner
        .queue
        .iter()
        .map(|record| {
            record
                .exception
                .as_deref()
                .expect("problem")
                .fingerprint
                .as_str()
        })
        .collect();
    assert_eq!(prints.len(), 4);
    assert_eq!(prints[0], prints[1]);
    assert_ne!(prints[1], prints[2]);
    assert_ne!(prints[2], prints[3]);
    drop(inner);
    telemetry.capture_log(
        LogSite {
            level: tracing::Level::WARN,
            target: "matrix_sdk::client",
            file: Some(
                "/Users/runner/.cargo/registry/src/index.crates.io-1949cf8c/matrix-sdk-0.18.0/src/client.rs",
            ),
            line: Some(7),
        },
        "a dependency's warning".to_owned(),
    );
    let inner = telemetry.inner.lock().expect("state");
    let exported = inner
        .queue
        .back()
        .and_then(|record| record.exception.as_deref());
    assert_eq!(
        exported.and_then(|problem| problem.file.as_deref()),
        Some("matrix-sdk-0.18.0/src/client.rs")
    );
    assert_eq!(
        callsite_file("/home/u/.cargo/git/checkouts/ruma-abc/1234567/crates/ruma/src/lib.rs"),
        "crates/ruma/src/lib.rs"
    );
    assert_eq!(
        callsite_file("crates/keeper/src/ipc.rs"),
        "crates/keeper/src/ipc.rs"
    );
    assert_eq!(callsite_file("/Users/marta/keeper/src/x.rs"), "<path>");
}

fn site(line: u32) -> LogSite<'static> {
    LogSite {
        level: tracing::Level::WARN,
        target: "keeper::sync",
        file: Some("crates/keeper/src/sync.rs"),
        line: Some(line),
    }
}

fn dropped_counts(telemetry: &Telemetry) -> Vec<u64> {
    let inner = telemetry.inner.lock().expect("state");
    inner
        .queue
        .iter()
        .filter_map(|record| match record.operation {
            Operation::ProblemsDropped(count) => Some(count),
            _ => None,
        })
        .collect()
}

/// R461: at most ten problems per fingerprint per minute and a session cap;
/// what the limits refuse is counted and reported as one record at most once
/// a minute.
#[test]
fn problems_are_limited_and_what_is_refused_is_reported_once_a_minute() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    for _ in 0..15 {
        telemetry.capture_log(site(1), "flood".to_owned());
    }
    telemetry.capture_log(site(2), "another fingerprint".to_owned());
    assert_eq!(telemetry.inner.lock().expect("state").queue.len(), 11);
    let now = Instant::now();
    telemetry.report_dropped(now);
    assert_eq!(dropped_counts(&telemetry), [5]);
    for _ in 0..3 {
        telemetry.capture_log(site(1), "flood".to_owned());
    }
    telemetry.report_dropped(now + Duration::from_secs(30));
    assert_eq!(dropped_counts(&telemetry), [5]);
    telemetry.report_dropped(now + DROP_REPORT_INTERVAL);
    assert_eq!(dropped_counts(&telemetry), [5, 3]);
    {
        let mut inner = telemetry.inner.lock().expect("state");
        inner.queue.clear();
        inner.problems_admitted = PROBLEMS_PER_SESSION - 1;
    }
    telemetry.capture_log(site(3), "the last one".to_owned());
    telemetry.capture_log(site(4), "over the session cap".to_owned());
    assert_eq!(telemetry.inner.lock().expect("state").queue.len(), 1);
    assert_eq!(telemetry.dropped.load(Ordering::Relaxed), 1);
}

/// R461: a log call never waits for the telemetry lock — held elsewhere, the
/// problem is counted as dropped and the call returns.
#[test]
fn a_problem_never_waits_on_a_held_lock() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    let held = telemetry.inner.lock().expect("state");
    telemetry.capture_log(site(1), "while held".to_owned());
    assert_eq!(telemetry.dropped.load(Ordering::Relaxed), 1);
    assert!(held.queue.is_empty());
}

/// 102.1 #3: a webview error leaves as `keeper_frontend_error` and a
/// `$exception` whose value is its redacted message and whose type is its
/// source; a hooked `console.warn` is a warning.
#[test]
fn a_webview_error_leaves_with_its_redacted_message() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.capture_frontend_error("window.error", "TypeError: off", false);
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
    telemetry.set_consent(diagnostics()).expect("consent");
    telemetry.capture_frontend_error(
        "window.error",
        "TypeError: cannot read /Users/marta/notes/a.md for @a:b.org",
        false,
    );
    telemetry.capture_frontend_error("console.warn", "deprecated prop", true);
    let inner = telemetry.inner.lock().expect("state");
    let bodies: Vec<serde_json::Value> = inner
        .queue
        .iter()
        .map(|record| batch_payload(&config(), "synthetic-installation", record).expect("batch"))
        .collect();
    assert_eq!(bodies.len(), 2);
    let batch = bodies[0]["batch"].as_array().expect("events");
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0]["event"], "keeper_frontend_error");
    assert_eq!(batch[1]["event"], "$exception");
    let error = &batch[1]["properties"];
    assert_eq!(error["$exception_list"][0]["type"], "window.error");
    assert_eq!(
        error["$exception_list"][0]["value"],
        // An unquoted home path runs to the end: nothing says where it stops.
        "TypeError: cannot read <path>"
    );
    assert_eq!(error["$exception_level"], "error");
    assert_eq!(
        bodies[1]["batch"][1]["properties"]["$exception_level"],
        "warning"
    );
}

/// R462: consent is the account's settings keys, synced in
/// `<login>/settings.toml`; a value a sync pulls takes effect on reload, and
/// turning it off there discards what was queued and the installation id.
#[test]
fn consent_is_the_accounts_synced_setting_and_a_pulled_change_takes_effect() {
    use crate::org_account::settings_sync::{synced_file, SyncedFile};
    for key in ["telemetry.diagnostics", "telemetry.product_analytics"] {
        assert_eq!(synced_file(key), Some(SyncedFile::Shared), "{key}");
    }
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    assert_eq!(
        crate::registry::get_telemetry_consent(&local.0).expect("rows"),
        (true, false)
    );
    telemetry.capture_log(site(1), "queued".to_owned());
    // What a sync applies: the row, written without the local observer.
    crate::registry::with_observer_suppressed(|| {
        crate::registry::set_telemetry_consent(&local.0, false, false)
    })
    .expect("pulled off");
    let off = telemetry.reload_consent().expect("reload");
    assert!(!off.consent.any());
    assert!(off.installation_id.is_none());
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
    assert!(!telemetry.wants_problems());
    crate::registry::with_observer_suppressed(|| {
        crate::registry::set_telemetry_consent(&local.0, true, true)
    })
    .expect("pulled on");
    let on = telemetry.reload_consent().expect("reload");
    assert!(on.consent.diagnostics && on.consent.product_analytics);
    assert!(on.installation_id.is_some());
    assert!(telemetry.wants_problems());
}

fn v1_store(dir: &Path, diagnostics: bool, remote_config: bool) {
    std::fs::write(
        dir.join(STORE_FILE),
        serde_json::json!({
            "version": 1,
            "consent": {"diagnostics": diagnostics, "productAnalytics": false, "remoteConfig": remote_config},
            "installationId": "0123456789abcdef0123456789abcdef",
        })
        .to_string(),
    )
    .expect("v1 store");
}

/// The merge of the shared file a settings sync plans: this device's rows
/// against `remote` and `base`, nothing applied yet.
fn merge_shared(
    dir: &Path,
    remote: &crate::org_account::settings_sync::Values,
    base: &crate::org_account::settings_sync::Values,
) -> crate::org_account::settings_sync::Merged {
    use crate::org_account::settings_sync::{self, Catalog, SyncedFile};
    let catalog = Catalog::default();
    let local =
        settings_sync::local_values(dir, SyncedFile::Shared, &catalog, Some(base)).expect("local");
    settings_sync::merge(Some(remote), None, &local, Some(base), &|key, portable| {
        settings_sync::from_portable(key, portable, &catalog)
    })
}

/// One settings sync of the shared file, the way the shell runs it: this
/// device's rows against `remote` and `base`, the pulled values applied
/// without the local observer. Returns the merge.
fn sync_shared(
    dir: &Path,
    remote: &crate::org_account::settings_sync::Values,
    base: &crate::org_account::settings_sync::Values,
) -> crate::org_account::settings_sync::Merged {
    let merged = merge_shared(dir, remote, base);
    crate::registry::with_observer_suppressed(|| {
        for (key, value) in &merged.apply {
            crate::registry::apply_synced_setting(dir, key, value.as_deref()).expect("apply");
        }
    });
    merged
}

/// R462 as fixed by R495 07: an installation's epic-71 "on" is never in force
/// by itself and is adopted only once the account's settings are known —
/// and then only where the account has no value. Two devices with a
/// settings base from before these keys existed: A turns diagnostics off for
/// the account; B upgrades later with its old "on". B's sync pulls A's off,
/// B adopts nothing, and B's next sync pushes nothing over it. A third
/// device whose account never chose adopts its "on" and pushes it.
#[test]
fn an_old_installations_on_never_overrides_the_accounts_off() {
    use crate::org_account::settings_sync::Values;
    let base = Values::default();
    let empty = Values::default();

    let a = LocalDir::new();
    crate::registry::set_telemetry_consent(&a.0, false, false).expect("A turns it off");
    let account = sync_shared(&a.0, &empty, &base).file;
    assert_eq!(
        account
            .values
            .get("telemetry.diagnostics")
            .map(String::as_str),
        Some("0")
    );

    let b = LocalDir::new();
    v1_store(&b.0, true, true);
    let telemetry = Telemetry::open(Some(&b.0), Some(config()));
    let status = telemetry.status().expect("status");
    assert!(
        !status.consent.diagnostics,
        "not in force before the account is known"
    );
    assert!(status.consent.remote_config);
    assert_eq!(
        status.installation_id.as_deref(),
        Some("0123456789abcdef0123456789abcdef")
    );
    assert!(
        crate::registry::stored_settings(&b.0, &["telemetry.diagnostics"])
            .expect("rows")
            .is_empty()
    );
    let pulled = sync_shared(&b.0, &account, &base);
    let status = telemetry
        .account_settled(AccountConsentKnown::from_sync(&pulled.file, &|_| false))
        .expect("settled");
    assert!(!status.consent.diagnostics);
    assert!(!telemetry.wants_problems());
    assert_eq!(
        crate::registry::get_telemetry_consent(&b.0).expect("rows"),
        (false, false)
    );
    let next = sync_shared(&b.0, &account, &pulled.base_if_pushed);
    assert!(!next.changed, "B pushes nothing over the account's off");
    assert_eq!(
        next.file
            .values
            .get("telemetry.diagnostics")
            .map(String::as_str),
        Some("0")
    );

    let c = LocalDir::new();
    v1_store(&c.0, true, false);
    let telemetry = Telemetry::open(Some(&c.0), Some(config()));
    assert!(!telemetry.wants_problems());
    let synced = sync_shared(&c.0, &empty, &base);
    let status = telemetry
        .account_settled(AccountConsentKnown::from_sync(&synced.file, &|_| false))
        .expect("settled");
    assert!(status.consent.diagnostics && !status.consent.product_analytics);
    assert!(telemetry.wants_problems());
    let next = sync_shared(&c.0, &empty, &synced.base_if_pushed);
    assert_eq!(
        next.file
            .values
            .get("telemetry.diagnostics")
            .map(String::as_str),
        Some("1")
    );
    let rewritten: serde_json::Value =
        serde_json::from_slice(&std::fs::read(c.0.join(STORE_FILE)).expect("store")).expect("json");
    assert_eq!(rewritten["version"], 2);
    assert!(rewritten.get("consent").is_none());
    assert!(rewritten.get("adopt").is_none());
}

/// R511 (P2-03): B upgrades with an epic-71 "on"; the account says off, and
/// B's apply of that off FAILS (the shell's `Unapplied.failed`), so B still
/// has no row. The error clears before adoption runs: adoption could write
/// now and must not — the key is not established, so the old "on" stays
/// pending and nothing is in force. B's next sync applies the off without
/// pushing anything over it, establishes the key, and the old "on" is
/// dropped for good.
#[test]
fn a_failed_apply_of_the_accounts_off_never_lets_the_old_on_in() {
    use crate::org_account::settings_sync::Values;
    let base = Values::default();
    let a = LocalDir::new();
    crate::registry::set_telemetry_consent(&a.0, false, false).expect("A turns it off");
    let account = sync_shared(&a.0, &Values::default(), &base).file;

    let b = LocalDir::new();
    v1_store(&b.0, true, false);
    let telemetry = Telemetry::open(Some(&b.0), Some(config()));
    let planned = merge_shared(&b.0, &account, &base);
    assert!(planned
        .apply
        .contains(&("telemetry.diagnostics".to_owned(), Some("0".to_owned()))));
    // The apply failed: nothing was written, and the shell's settled base
    // keeps this device's own absence for the key.
    let known =
        AccountConsentKnown::from_sync(&planned.file, &|key| key == "telemetry.diagnostics");
    assert_eq!(known.diagnostics, AccountKey::Unknown);
    let status = telemetry.account_settled(known).expect("settled");
    assert!(!status.consent.diagnostics);
    assert!(!telemetry.wants_problems());
    assert!(
        crate::registry::stored_settings(&b.0, &["telemetry.diagnostics"])
            .expect("rows")
            .is_empty(),
        "nothing adopted over a value the account has"
    );
    assert_eq!(
        telemetry.inner.lock().expect("state").adopt,
        Some(LegacyConsent {
            diagnostics: true,
            product_analytics: false
        }),
        "still pending"
    );

    let next = sync_shared(&b.0, &account, &base);
    assert!(!next.changed, "B pushes nothing over the account's off");
    let status = telemetry
        .account_settled(AccountConsentKnown::from_sync(&next.file, &|_| false))
        .expect("settled");
    assert!(!status.consent.diagnostics && !telemetry.wants_problems());
    assert_eq!(
        crate::registry::stored_settings(&b.0, &["telemetry.diagnostics"])
            .expect("rows")
            .get("telemetry.diagnostics")
            .map(String::as_str),
        Some("0")
    );
    let stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(b.0.join(STORE_FILE)).expect("store")).expect("json");
    assert!(stored.get("adopt").is_none(), "the account decided");
}

/// The account directory of a test: `keeper.toml` holding `settings`.
fn account_layers(dir: &Path, settings: &str) -> crate::config::AccountLayers {
    std::fs::write(dir.join("keeper.toml"), format!("[settings]\n{settings}")).expect("layer");
    crate::config::load_account_layers(&crate::config::AccountLayerSource {
        dir: dir.to_path_buf(),
        device: "laptop".to_owned(),
        account_name: "Acme".to_owned(),
        repo_host: "git.acme.dev".to_owned(),
    })
}

/// R496, R495 05: an account-layer change alone — no settings row moves —
/// takes effect at once through the layers observer: the account's
/// `keeper.toml` turning diagnostics off stops collection and drops what
/// was queued and in flight; clearing the layer (sign-out) brings the row's
/// answer back.
#[test]
fn an_account_layer_change_alone_takes_effect_and_drops_what_was_queued() {
    let local = LocalDir::new();
    let account = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    let observed = Arc::clone(&telemetry);
    crate::config::set_account_layers_observer(Box::new(move || {
        let observed = Arc::clone(&observed);
        Box::new(move || {
            let _ = observed.reload_consent();
        })
    }));
    telemetry.capture_log(site(1), "queued".to_owned());
    assert_eq!(telemetry.inner.lock().expect("state").queue.len(), 1);
    let in_flight = *telemetry.epoch.borrow();

    crate::config::install_account_layers(Some(account_layers(
        &account.0,
        "\"telemetry.diagnostics\" = false\n",
    )));
    assert_eq!(
        crate::registry::stored_settings(&local.0, &["telemetry.diagnostics"])
            .expect("rows")
            .get("telemetry.diagnostics")
            .map(String::as_str),
        Some("1"),
        "the row did not move"
    );
    assert!(!telemetry.wants_problems());
    assert!(!telemetry.status().expect("status").consent.diagnostics);
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
    assert_ne!(
        *telemetry.epoch.borrow(),
        in_flight,
        "in-flight exports cancelled"
    );
    telemetry.capture_log(site(2), "after the layer".to_owned());
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());

    crate::config::install_account_layers(None);
    assert!(telemetry.wants_problems());
}

/// R495 06: a local save takes the EFFECTIVE answer read back after the
/// write. A settings file pinning diagnostics off keeps it off after a save
/// of "on" (and says so); one pinning it on keeps it on after a save of
/// "off".
#[test]
fn a_local_save_takes_effect_only_as_far_as_a_settings_file_allows() {
    let local = LocalDir::new();
    let account = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));

    crate::config::install_account_layers(Some(account_layers(
        &account.0,
        "\"telemetry.diagnostics\" = false\n",
    )));
    let saved = telemetry.set_consent(diagnostics()).expect("saved");
    assert!(saved.consent_pinned);
    assert!(!saved.consent.diagnostics);
    assert!(!telemetry.wants_problems());
    assert_eq!(
        crate::registry::stored_settings(&local.0, &["telemetry.diagnostics"])
            .expect("rows")
            .get("telemetry.diagnostics")
            .map(String::as_str),
        Some("1"),
        "the choice is kept for when the file stops setting it"
    );

    crate::config::install_account_layers(Some(account_layers(
        &account.0,
        "\"telemetry.diagnostics\" = true\n",
    )));
    let saved = telemetry
        .set_consent(TelemetryConsentVm::default())
        .expect("saved");
    assert!(saved.consent_pinned && saved.consent.diagnostics);
    assert!(telemetry.wants_problems());
}

/// This thread's consent transitions call `hook` at each
/// [`TransitionPoint`]; other threads' do not.
fn hold_transitions(hook: impl FnMut(TransitionPoint) + 'static) {
    TRANSITION_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

/// What a revocation must leave: the account's rows off, this process off,
/// and a problem raised afterwards never queued.
#[track_caller]
fn revoked_everywhere(local: &LocalDir, telemetry: &Telemetry) {
    assert_eq!(
        crate::registry::get_telemetry_consent(&local.0).expect("rows"),
        (false, false),
        "the effective answer is off"
    );
    assert!(!telemetry.status().expect("status").consent.diagnostics);
    assert!(!telemetry.wants_problems(), "the cached answer is off");
    telemetry.capture_log(site(1), "after the revocation".to_owned());
    assert!(
        queued_payloads(telemetry).is_empty(),
        "nothing exports after it"
    );
}

/// R511 (P2-02): save A ("off") begins first and holds before its write;
/// save B ("on") writes and settles; A then writes last. A's revocation is
/// what is in force — in the rows, in this process and in what is queued.
#[test]
fn a_revocation_written_last_is_in_force_whichever_save_began_first() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    let (held, holding) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let save_a = {
        let telemetry = Arc::clone(&telemetry);
        std::thread::spawn(move || {
            hold_transitions(move |point| {
                if point == TransitionPoint::BeforeWrite {
                    let _ = held.send(());
                    let _ = released.recv();
                }
            });
            telemetry.set_consent(TelemetryConsentVm::default())
        })
    };
    holding.recv().expect("A holds before its write");
    let b = telemetry.set_consent(diagnostics()).expect("B saves on");
    assert!(b.consent.diagnostics && telemetry.wants_problems());
    release.send(()).expect("A resumes");
    let a = save_a.join().expect("A").expect("A saves off");
    assert!(!a.consent.diagnostics);
    revoked_everywhere(&local, &telemetry);
}

/// R511 (P2-02): a reload reads "on" and holds before installing it; a save
/// of "off" meanwhile writes and reads. The older read is never installed
/// over the newer one: the save waits for the reload to finish, then
/// installs its own answer. The reload is let go only once the save is
/// waiting for it — or, were nothing to make it wait, once the save is done.
#[test]
fn an_older_read_is_never_installed_over_a_newer_one() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("on");
    let (read, reading) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let reload = {
        let telemetry = Arc::clone(&telemetry);
        std::thread::spawn(move || {
            hold_transitions(move |point| {
                if point == TransitionPoint::AfterRead {
                    let _ = read.send(());
                    let _ = released.recv();
                }
            });
            telemetry.reload_consent()
        })
    };
    reading.recv().expect("the reload has read on");
    let waiting = release.clone();
    hold_transitions(move |point| {
        if point == TransitionPoint::Contended {
            let _ = waiting.send(());
        }
    });
    let saved = telemetry
        .set_consent(TelemetryConsentVm::default())
        .expect("the save");
    TRANSITION_HOOK.with(|slot| *slot.borrow_mut() = None);
    let _ = release.send(());
    assert!(!saved.consent.diagnostics);
    reload.join().expect("reload").expect("reloaded");
    revoked_everywhere(&local, &telemetry);
}

/// R515 (P3-02): an adoption that established the account has no value
/// holds before its write; the person meanwhile saves "off", and that save
/// completes. The adoption then resumes: its write finds the row and
/// leaves it — the off is what is stored, read, cached and in force.
#[test]
fn an_adoption_never_overwrites_an_off_saved_after_it_decided() {
    use crate::org_account::settings_sync::Values;
    let b = LocalDir::new();
    v1_store(&b.0, true, true);
    let telemetry = Telemetry::open(Some(&b.0), Some(config()));
    let synced = sync_shared(&b.0, &Values::default(), &Values::default());
    let known = AccountConsentKnown::from_sync(&synced.file, &|_| false);
    assert_eq!(
        known,
        AccountConsentKnown::NO_ACCOUNT,
        "the account has no value"
    );
    let (held, holding) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let adoption = {
        let telemetry = Arc::clone(&telemetry);
        std::thread::spawn(move || {
            hold_transitions(move |point| {
                if point == TransitionPoint::BeforeAdopt {
                    let _ = held.send(());
                    let _ = released.recv();
                }
            });
            telemetry.account_settled(known)
        })
    };
    holding.recv().expect("the adoption holds before its write");
    let saved = telemetry
        .set_consent(TelemetryConsentVm::default())
        .expect("the off save completes");
    assert!(!saved.consent.diagnostics && !saved.consent.product_analytics);
    release.send(()).expect("the adoption resumes");
    let adopted = adoption.join().expect("adoption").expect("settled");
    assert!(!adopted.consent.diagnostics && !adopted.consent.product_analytics);
    let rows = crate::registry::stored_settings(
        &b.0,
        &["telemetry.diagnostics", "telemetry.product_analytics"],
    )
    .expect("rows");
    assert_eq!(
        rows.get("telemetry.diagnostics").map(String::as_str),
        Some("0")
    );
    assert_eq!(
        rows.get("telemetry.product_analytics").map(String::as_str),
        Some("0")
    );
    revoked_everywhere(&b, &telemetry);
}

/// Epic 100, R381, 100.1 #9: an interaction names its declared surface from a
/// closed set, exported as one `surface` property; a surface on any other
/// kind, or a surface outside the set, never enters the queue.
#[test]
fn an_interaction_carries_only_a_declared_surface() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    assert!(serde_json::from_str::<TelemetryEventReq>(
        r#"{"kind":"interaction","durationMs":null,"surface":"slashMenuCommand:/invite"}"#
    )
    .is_err());
    assert!(telemetry
        .capture(TelemetryEventReq {
            kind: TelemetryEventKind::AppReady,
            duration_ms: None,
            surface: Some(TelemetrySurface::SlashMenuOpened),
        })
        .is_err());
    for (surface, exported) in [
        (TelemetrySurface::SlashMenuOpened, "slash_menu_opened"),
        (TelemetrySurface::SlashMenuChosen, "slash_menu_chosen"),
        (TelemetrySurface::LaunchDefaultAgent, "launch_default_agent"),
    ] {
        let wire: TelemetryEventReq = serde_json::from_value(serde_json::json!({
            "kind": "interaction", "durationMs": null, "surface": surface
        }))
        .expect("a declared surface");
        let record = Record::frontend(wire);
        let payload = batch_payload(&config(), "synthetic-installation", &record).expect("batch");
        assert_eq!(payload["batch"][0]["event"], "keeper_interaction");
        assert_eq!(payload["batch"][0]["properties"]["surface"], exported);
    }
    let plain = Record::frontend(event(TelemetryEventKind::Interaction));
    let payload = batch_payload(&config(), "synthetic-installation", &plain).expect("batch");
    assert!(payload["batch"][0]["properties"].get("surface").is_none());
}

/// A recording's lost microphone leaves as one shell-raised diagnostics count
/// whose only property is a closed cause: no duration, no device, no message.
#[test]
fn a_mic_loss_carries_only_a_closed_cause() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.set_consent(diagnostics()).expect("consent");
    assert!(serde_json::from_str::<TelemetryEventReq>(
        r#"{"kind":"recordingMicLost","durationMs":null}"#
    )
    .is_err());
    assert!(serde_json::from_str::<TelemetryEventReq>(
        r#"{"kind":"interaction","durationMs":null,"cause":"removed"}"#
    )
    .is_err());
    for (cause, exported) in [
        (MicLossCause::Removed, "removed"),
        (MicLossCause::Stalled, "stalled"),
        (MicLossCause::Silent, "silent"),
    ] {
        telemetry.capture_mic_lost(cause);
        let record = Record::mic_lost(cause);
        let payload = batch_payload(&config(), "synthetic-installation", &record).expect("batch");
        let batch = payload["batch"].as_array().expect("event array");
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0]["event"], "keeper_recording_mic_lost");
        let props = &batch[0]["properties"];
        assert_eq!(props["cause"], exported);
        assert_eq!(props["consent_category"], "diagnostics");
        assert_eq!(props["event_source"], "shell");
        assert!(props.get("duration_ms").is_none());
        assert!(props.get("surface").is_none());
    }
    let inner = telemetry.inner.lock().expect("state");
    let names: Vec<&str> = inner
        .queue
        .iter()
        .map(|record| exported_name(record.operation))
        .collect();
    assert_eq!(names, ["keeper_recording_mic_lost"; 3]);
    assert!(inner.queue.iter().all(Record::diagnostic));
}

#[test]
fn a_mic_loss_needs_diagnostics_consent() {
    let local = LocalDir::new();
    let telemetry = Telemetry::open(Some(&local.0), Some(config()));
    telemetry.capture_mic_lost(MicLossCause::Removed);
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
    telemetry
        .set_consent(TelemetryConsentVm {
            product_analytics: true,
            remote_config: true,
            ..Default::default()
        })
        .expect("product only");
    telemetry.capture_mic_lost(MicLossCause::Stalled);
    assert!(telemetry.inner.lock().expect("state").queue.is_empty());
    telemetry.set_consent(diagnostics()).expect("diagnostics");
    telemetry.capture_mic_lost(MicLossCause::Silent);
    assert_eq!(telemetry.inner.lock().expect("state").queue.len(), 1);
}

#[test]
fn only_microphone_warning_codes_map_to_a_cause() {
    for (code, cause) in [
        ("micLost", Some(MicLossCause::Removed)),
        ("micStalled", Some(MicLossCause::Stalled)),
        ("micSilent", Some(MicLossCause::Silent)),
        ("cameraLost", None),
        ("echoReferenceStale", None),
        ("unknown", None),
        ("MicLost", None),
        ("", None),
    ] {
        assert_eq!(MicLossCause::from_warning_code(code), cause, "{code}");
    }
}

/// 102.1 #7, live: WARNs through the real layer and the real exporter to
/// the configured PostHog project. Needs `KEEPER_POSTHOG_HOST`,
/// `KEEPER_POSTHOG_PROJECT_TOKEN` (the public pair) and
/// `KEEPER_SMOKE_MARKER`, a string the caller then reads back by HogQL —
/// along with proof that the lines' adversarial parts (a credential in a
/// field, a home path with spaces, a serverless event id, a home filename
/// holding a quote and a space, and one holding a quote and ending in a
/// literal backslash, hand-quoted in the message and in a field; a home
/// root in lowercase and one inside a `file://` URL) did not arrive (R495,
/// R511, R515, R518), and that a real transport failure on the
/// exporter's own runtime, logged from a task spawned there, never left
/// (`transport-<marker>`, R511).
#[tokio::test]
#[ignore = "live: sends three records to PostHog; run by hand with the public pair"]
async fn live_a_warning_reaches_posthog() {
    use tracing::instrument::WithSubscriber as _;
    let (Ok(host), Ok(token), Ok(marker)) = (
        std::env::var("KEEPER_POSTHOG_HOST"),
        std::env::var("KEEPER_POSTHOG_PROJECT_TOKEN"),
        std::env::var("KEEPER_SMOKE_MARKER"),
    ) else {
        panic!("set KEEPER_POSTHOG_HOST, KEEPER_POSTHOG_PROJECT_TOKEN and KEEPER_SMOKE_MARKER");
    };
    let local = LocalDir::new();
    let telemetry = Telemetry::open(
        Some(&local.0),
        Some(PublicConfig::parse(&host, &token).expect("the public pair")),
    );
    telemetry.set_consent(diagnostics()).expect("consent");
    let worker = tokio::spawn(Arc::clone(&telemetry).run());
    let export = LogExport::new();
    export.attach(&telemetry);
    let dispatch = tracing::Dispatch::new(log_subscriber(
        tracing_subscriber::EnvFilter::new(keeper_sync_default_filter()),
        std::io::sink,
        export,
    ));
    let transport_marker = format!("transport-{marker}");
    telemetry
        .transport
        .run(async move {
            let failed = reqwest::Client::new()
                .get("https://127.0.0.1:9/")
                .send()
                .await
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            tokio::spawn(
                async move {
                    tracing::warn!(target: "reqwest::connect", marker = %transport_marker, "the export's connection failed: {failed}");
                }
                .with_subscriber(dispatch),
            )
            .await
            .expect("the connection task");
        })
        .await
        .expect("the transport runs");
    assert!(
        telemetry.inner.lock().expect("state").queue.is_empty(),
        "the exporter's own failure is not a problem"
    );
    forwarding(&telemetry, || {
        tracing::warn!(
            target: "keeper::smoke",
            marker = %marker,
            error = %r#"login failed: password="two private words""#,
            root = %"/Users/marta/My Drive/private plan.md",
            event = %"$abcdefghijklmnopqrstuvwxyzabcdefghijklmnopq",
            path = ?PathBuf::from("/Users/marta/a\" quoted secret.md"),
            "live smoke for @alice:example.org"
        );
        let tail = PathBuf::from(r"/Users/marta/a' zebra quartz\");
        tracing::warn!(
            target: "keeper::smoke",
            marker = %marker,
            path = %format!("'{}'", tail.display()),
            "live smoke copy '{}' to /tmp/public",
            tail.display()
        );
        // R518: a home root in another case, the way `account_restore.rs`
        // logs a drive's folder as typed, and one inside a `file://` URL.
        let folder = PathBuf::from("/users/marta/a walnut.md");
        tracing::warn!(
            target: "keeper::smoke",
            marker = %marker,
            error = %std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            path = %folder.display(),
            url = %"file:///users/marta/b walnut.md then \"amber heron\"",
            "live smoke mkdir {} failed",
            folder.display()
        );
    });
    for _ in 0..50 {
        if telemetry.inner.lock().expect("state").queue.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // The exporter's three requests are bounded by REQUEST_TIMEOUT.
    tokio::time::sleep(REQUEST_TIMEOUT + Duration::from_secs(1)).await;
    worker.abort();
    let _ = worker.await;
}

/// R522 (R541-01), at both boundaries — `keeper.log` and the exported
/// payload: a sign-in page the opener could not open leaves a fixed line and
/// the opener's kind of failure, never the opener's words, however they carry
/// the page — quoted whole, percent-encoded with the state's first character
/// encoded, cut short, or nested in a forge's `redirect_to`.
#[test]
fn an_opener_that_quotes_the_sign_in_page_leaves_none_of_it_in_the_log_or_the_export() {
    use crate::error::{CoreError, NotOpened, PlatformError};
    use crate::sign_in_link::offer;
    use crate::sign_in_link::tests::{Recorder, PAGE};

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("captured").extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // A forge's sign-in page wraps the authorization request in `redirect_to`.
    let forge = format!(
        "https://forge.example/user/login?redirect_to={}",
        url::form_urlencoded::byte_serialize(PAGE.as_bytes()).collect::<String>()
    );
    let internal =
        |words: String| CoreError::Internal(format!("could not open the browser: {words}"));
    let openers = [
        (PAGE.to_owned(), internal(format!("Launcher \"/usr/bin/open\" \"{PAGE}\" failed with exit status 1"))),
        (PAGE.to_owned(), internal("xdg-open: https%3A%2F%2Fid.example%3A8451%2Fauthorize%3Fstate%3D%5Aq3xW9vLp2Rt8sYb".to_owned())),
        (PAGE.to_owned(), internal("xdg-open: https%3A%2F%2Fid.example%3A8451%2Fauthorize%3Fstate%3DZq3xW9vL...".to_owned())),
        (forge.clone(), internal(format!("xdg-open: {}", &forge[..forge.len() - 24]))),
        (PAGE.to_owned(), CoreError::Platform(PlatformError::NotOpened(NotOpened::NoHandler))),
    ];

    // One telemetry per opener: a callsite's problem is admitted once a
    // minute, and every opener's line must reach the export.
    let platform = Recorder::default();
    let mut log = String::new();
    let mut payloads = Vec::new();
    for (page, error) in openers {
        let local = LocalDir::new();
        let telemetry = Telemetry::open(Some(&local.0), Some(config()));
        telemetry.set_consent(diagnostics()).expect("consent");
        let export = LogExport::new();
        export.attach(&telemetry);
        let captured = Captured::default();
        let writer = captured.clone();
        let filter = tracing_subscriber::EnvFilter::new(keeper_sync_default_filter());
        tracing::subscriber::with_default(
            log_subscriber(filter, move || writer.clone(), export),
            || {
                let _shown = offer(&platform, &page, move || Err(error));
            },
        );
        let line = String::from_utf8(captured.0.lock().expect("captured").clone()).expect("utf-8");
        assert!(line.contains("the sign-in page did not open"), "{line}");
        let exported = queued_payloads(&telemetry);
        assert_eq!(exported.len(), 1, "{exported:?}");
        assert!(
            exported[0].contains("the sign-in page did not open"),
            "{exported:?}"
        );
        log.push_str(&line);
        payloads.extend(exported);
    }
    assert_eq!(log.matches("category=failed").count(), 4, "{log}");
    assert_eq!(log.matches("category=no handler").count(), 1, "{log}");
    // The page's own values, its hosts and the opener's words, in any form.
    for material in [
        "q3xW9vL",
        "E9Melhoa2Owv",
        "n0nce4Sign1n",
        "id.example",
        "forge.example",
        "authorize",
        "redirect_to",
        "Launcher",
        "xdg-open",
        "exit status",
        "could not open the browser",
    ] {
        assert!(
            !log.contains(material),
            "{material} reached keeper.log: {log}"
        );
        for payload in &payloads {
            assert!(
                !payload.contains(material),
                "{material} was exported: {payload}"
            );
        }
    }
}
