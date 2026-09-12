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
        r#"{"kind":"frontendError","durationMs":null,"message":"PRIVATE_SENTINEL"}"#,
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
        TelemetryEventKind::FrontendError,
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
    // Ordinary app settings cannot enable this store.
    std::fs::remove_file(&path).expect("remove corrupt state");
    std::fs::write(
        local.0.join("config.json"),
        r#"{"telemetry.diagnostics":true}"#,
    )
    .expect("layer fixture");
    assert!(!Telemetry::open(Some(&local.0), Some(config()))
        .status()
        .expect("status")
        .consent
        .any());
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
fn only_error_events_emit_a_native_issue_with_closed_properties() {
    for kind in [
        TelemetryEventKind::AppReady,
        TelemetryEventKind::FrontendError,
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
        let issues: Vec<_> = batch
            .iter()
            .filter(|e| e["event"] == "$exception")
            .collect();
        if kind != TelemetryEventKind::FrontendError {
            assert!(issues.is_empty());
            continue;
        }
        assert_eq!(issues.len(), 1);
        let props = issues[0]["properties"].as_object().expect("properties");
        let keys: std::collections::BTreeSet<_> = props.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from([
                "distinct_id",
                "consent_category",
                "event_source",
                "synthetic",
                "duration_ms",
                "$process_person_profile",
                "$geoip_disable",
                "trace_id",
                "span_id",
                "$exception_list",
                "$exception_fingerprint",
                "$exception_level",
            ])
        );
        assert_eq!(props["$process_person_profile"], false);
        assert_eq!(props["$geoip_disable"], true);
        assert_eq!(props["consent_category"], "diagnostics");
        assert_eq!(
            props["$exception_list"],
            serde_json::json!([{
                "type": "KeeperFrontendError",
                "value": "A frontend operation failed; private details are not collected",
                "mechanism": {"handled": true, "type": "generic"}
            }])
        );
        assert_eq!(props["trace_id"], record.trace_id);
        assert_eq!(props["span_id"], record.span_id);
    }
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
