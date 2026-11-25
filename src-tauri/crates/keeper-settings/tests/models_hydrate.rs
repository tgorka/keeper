use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_ports::transfer::{Downloaded, FileTransfer, Progress, Uploaded};
use keeper_ports::{Replica, ReplicaError, RowOp, StoreOrigin, StoreSlot};
use keeper_replica::transfer::{BlobRouteGrants, StoreTransfer, TransferConfig};
use keeper_replica::SyncularOpener;
use keeper_settings::models::{
    active_set, hydrate, is_current, ready_digest, ready_version, ActiveSet, Folders, Group,
    ModelsError, SetFile,
};
use keeper_settings_api::models::{file_key, manifest_key, version_id, ModelFile, ACTIVE_KEY};
use keeper_testkit::harness::{harness_of, Harness, Over};
use keeper_testkit::store::StandIn;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

const DATABASE: &str = "makidrive";
const SAMPLE_MODELS_TOML: &str =
    "version = 1\n\n[asr]\npath = \"asr\"\n\n[vad]\npath = \"vad/model.onnx\"\n";
const ENCODER_BYTES: usize = 2 * 1024 * 1024;

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn pseudo_random(mut state: u64, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        let bytes = z.to_le_bytes();
        let take = (len - out.len()).min(bytes.len());
        out.extend_from_slice(&bytes[..take]);
    }
    out
}

fn local_replica() -> Harness {
    harness_of(DATABASE, Over::Local(&SyncularOpener))
}

fn put_row(replica: &dyn Replica, key: &str, value: Value, blob: Option<Value>) {
    let mut values = Map::new();
    values.insert("key".to_owned(), json!(key));
    values.insert("dir".to_owned(), json!(DATABASE));
    // JSON and BLOB_REF columns take their raw JSON text.
    values.insert("value".to_owned(), Value::String(value.to_string()));
    values.insert(
        "value_blob".to_owned(),
        blob.map_or(Value::Null, |blob| Value::String(blob.to_string())),
    );
    values.insert("revision".to_owned(), json!(1));
    values.insert("issuer".to_owned(), json!("admin"));
    values.insert("updated_at_ms".to_owned(), json!(0));
    values.insert("author_actor".to_owned(), json!("admin"));
    values.insert("author_client".to_owned(), json!("admin"));
    replica
        .mutate(vec![RowOp::Upsert {
            table: "config_values".to_owned(),
            values,
            base_version: None,
        }])
        .expect("upsert config_values row");
}

struct PublishOpts {
    write_files: bool,
    write_manifest: bool,
    set_active: bool,
    file_count_override: Option<u64>,
    version_override: Option<String>,
}

impl Default for PublishOpts {
    fn default() -> Self {
        Self {
            write_files: true,
            write_manifest: true,
            set_active: true,
            file_count_override: None,
            version_override: None,
        }
    }
}

fn publish_model_files(
    replica: &dyn Replica,
    files: &[ModelFile],
    models_toml: &str,
    opts: PublishOpts,
) -> String {
    let version = opts
        .version_override
        .unwrap_or_else(|| version_id(files, models_toml).expect("valid version id"));
    if opts.write_files {
        for file in files {
            let blob = json!({
                "blobId": format!("sha256:{}", file.sha256),
                "byteLength": file.size,
            });
            put_row(
                replica,
                &file_key(&version, &file.path),
                json!({ "size": file.size, "sha256": file.sha256 }),
                Some(blob),
            );
        }
    }
    if opts.write_manifest {
        let file_count = opts.file_count_override.unwrap_or(files.len() as u64);
        put_row(
            replica,
            &manifest_key(&version),
            json!({ "models_toml": models_toml, "file_count": file_count }),
            None,
        );
    }
    if opts.set_active {
        put_row(replica, ACTIVE_KEY, json!(version), None);
    }
    version
}

fn publish(
    replica: &dyn Replica,
    files: &[(&str, &[u8])],
    models_toml: &str,
) -> (String, Vec<(String, Vec<u8>)>) {
    publish_with(replica, files, models_toml, PublishOpts::default())
}

fn publish_with(
    replica: &dyn Replica,
    files: &[(&str, &[u8])],
    models_toml: &str,
    opts: PublishOpts,
) -> (String, Vec<(String, Vec<u8>)>) {
    let mut model_files = Vec::with_capacity(files.len());
    let mut blobs = Vec::with_capacity(files.len());
    for (path, bytes) in files {
        let digest = sha256_hex(bytes);
        model_files.push(ModelFile {
            path: (*path).to_owned(),
            size: bytes.len() as u64,
            sha256: digest.clone(),
        });
        blobs.push((format!("sha256:{digest}"), (*bytes).to_vec()));
    }
    let version = publish_model_files(replica, &model_files, models_toml, opts);
    (version, blobs)
}

struct SampleFiles {
    encoder: Vec<u8>,
    decoder: Vec<u8>,
    vad: Vec<u8>,
    license: Vec<u8>,
}

impl SampleFiles {
    fn new() -> Self {
        Self {
            encoder: pseudo_random(1, ENCODER_BYTES),
            decoder: pseudo_random(2, 64 * 1024),
            vad: pseudo_random(3, 32 * 1024),
            license: b"Apache-2.0\nAll rights reserved.\n".to_vec(),
        }
    }

    fn as_slices(&self) -> [(&'static str, &[u8]); 4] {
        [
            ("asr/encoder.bin", self.encoder.as_slice()),
            ("asr/decoder.bin", self.decoder.as_slice()),
            ("vad/model.onnx", self.vad.as_slice()),
            ("LICENSE", self.license.as_slice()),
        ]
    }

    fn seed_all(&self, stand_in: &StandIn) {
        for (_, bytes) in self.as_slices() {
            stand_in.seed_blob(bytes);
        }
    }
}

fn make_transfer(stand_in: &StandIn, plan_dir: &Path) -> StoreTransfer {
    let slot = StoreSlot::default();
    slot.set(Some(
        StoreOrigin::parse(&stand_in.store_origin()).expect("stand-in store origin"),
    ));
    let grants = BlobRouteGrants::new(
        &stand_in.base_url(),
        stand_in.token(),
        Duration::from_secs(10),
    )
    .expect("blob route grants");
    StoreTransfer::new(
        Arc::new(grants),
        &stand_in.base_url(),
        slot,
        TransferConfig::contract(plan_dir.to_path_buf()),
    )
    .expect("store transfer")
}

fn transcription_group(vad: &[String]) -> Group<'_> {
    Group {
        name: "transcription",
        folders: Folders::AllBut(vad),
        fingerprint: None,
    }
}

fn turn_group(vad: &[String]) -> Group<'_> {
    Group {
        name: "turn",
        folders: Folders::Only(vad),
        fingerprint: None,
    }
}

fn other_version_id(id: &str) -> String {
    let first = if id.starts_with('0') { '1' } else { '0' };
    format!("{first}{}", &id[1..])
}

#[test]
fn desktop_fetches_transcription_group() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());

    let sample = SampleFiles::new();
    sample.seed_all(&stand_in);
    let (version, _) = publish(
        harness.source.replica.as_ref(),
        &sample.as_slices(),
        SAMPLE_MODELS_TOML,
    );

    let set = active_set(harness.source.replica.as_ref())
        .expect("active_set succeeds")
        .expect("active_set present");
    assert_eq!(set.version, version);

    let vad = ["vad".to_owned()];
    let group = transcription_group(&vad);
    let interrupt = AtomicBool::new(false);
    let report = hydrate(&set, group, &transfer, dest.path(), &interrupt).expect("hydrate");
    assert_eq!(report.downloaded, 3);

    assert_eq!(
        std::fs::read(dest.path().join("asr/encoder.bin")).expect("encoder"),
        sample.encoder
    );
    assert_eq!(
        std::fs::read(dest.path().join("asr/decoder.bin")).expect("decoder"),
        sample.decoder
    );
    assert_eq!(
        std::fs::read(dest.path().join("LICENSE")).expect("license"),
        sample.license
    );
    assert_eq!(
        std::fs::read_to_string(dest.path().join("models.toml")).expect("models.toml"),
        SAMPLE_MODELS_TOML
    );
    assert!(!dest.path().join("vad").exists());
    assert_eq!(ready_version(dest.path(), "transcription"), Some(version));
    assert!(is_current(&set, group, dest.path()));
}

#[test]
fn phone_fetches_only_turn_group() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());

    let sample = SampleFiles::new();
    sample.seed_all(&stand_in);
    let (version, _) = publish(
        harness.source.replica.as_ref(),
        &sample.as_slices(),
        SAMPLE_MODELS_TOML,
    );

    let set = active_set(harness.source.replica.as_ref())
        .expect("active_set succeeds")
        .expect("active_set present");
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    let interrupt = AtomicBool::new(false);
    hydrate(&set, group, &transfer, dest.path(), &interrupt).expect("hydrate turn");

    assert_eq!(
        std::fs::read(dest.path().join("vad/model.onnx")).expect("vad model"),
        sample.vad
    );
    assert_eq!(
        std::fs::read(dest.path().join("LICENSE")).expect("license"),
        sample.license
    );
    assert_eq!(
        std::fs::read_to_string(dest.path().join("models.toml")).expect("models.toml"),
        SAMPLE_MODELS_TOML
    );
    assert!(!dest.path().join("asr").exists());
    assert_eq!(
        stand_in.store_bytes_served(),
        (sample.vad.len() + sample.license.len()) as u64
    );
    assert_eq!(ready_version(dest.path(), "turn"), Some(version));
    assert!(is_current(&set, group, dest.path()));
}

#[test]
fn planted_wrong_body_is_refused() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());

    let vad_bytes = pseudo_random(10, 16 * 1024);
    let wrong_bytes = pseudo_random(11, vad_bytes.len());
    assert_ne!(vad_bytes, wrong_bytes);
    let license_bytes = b"MIT\n".to_vec();

    let (_, blobs) = publish(
        harness.source.replica.as_ref(),
        &[
            ("vad/model.onnx", vad_bytes.as_slice()),
            ("LICENSE", license_bytes.as_slice()),
        ],
        SAMPLE_MODELS_TOML,
    );
    stand_in.seed_blob_as(&blobs[0].0, &wrong_bytes);
    stand_in.seed_blob(&license_bytes);

    let set = active_set(harness.source.replica.as_ref())
        .expect("active_set succeeds")
        .expect("active_set present");
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    let interrupt = AtomicBool::new(false);
    let err = hydrate(&set, group, &transfer, dest.path(), &interrupt)
        .expect_err("planted wrong body must fail");

    assert!(matches!(
        err,
        ModelsError::Integrity { .. } | ModelsError::Transfer { .. }
    ));
    assert!(ready_digest(dest.path(), "turn").is_none());
    assert!(!dest.path().join("vad/model.onnx").exists());
    assert!(!dest.path().join("vad").exists());
}

#[test]
fn row_size_mismatch_with_blob_is_refused() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());

    let real_bytes = pseudo_random(20, 4096);
    stand_in.seed_blob(&real_bytes);
    let declared_file = ModelFile {
        path: "vad/model.onnx".to_owned(),
        size: (real_bytes.len() + 1024) as u64,
        sha256: sha256_hex(&real_bytes),
    };
    publish_model_files(
        harness.source.replica.as_ref(),
        &[declared_file],
        SAMPLE_MODELS_TOML,
        PublishOpts::default(),
    );

    let set = active_set(harness.source.replica.as_ref())
        .expect("active_set accepts self-consistent rows")
        .expect("active_set present");
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    let interrupt = AtomicBool::new(false);
    let err = hydrate(&set, group, &transfer, dest.path(), &interrupt)
        .expect_err("size mismatch must fail");

    assert!(matches!(
        err,
        ModelsError::Integrity { .. } | ModelsError::Transfer { .. }
    ));
    assert!(ready_digest(dest.path(), "turn").is_none());
    assert!(!dest.path().join("vad/model.onnx").exists());
}

#[test]
fn incomplete_versions_and_absent_active_are_handled() {
    // No models.active row: no active set.
    let empty = local_replica();
    assert_eq!(
        active_set(empty.source.replica.as_ref()).expect("absent active_set"),
        None
    );

    let vad_bytes = pseudo_random(30, 1024);
    let license_bytes = b"MIT\n".to_vec();

    // models.active set, manifest missing.
    let no_manifest = local_replica();
    publish_with(
        no_manifest.source.replica.as_ref(),
        &[("vad/model.onnx", vad_bytes.as_slice())],
        SAMPLE_MODELS_TOML,
        PublishOpts {
            write_manifest: false,
            ..Default::default()
        },
    );
    assert!(matches!(
        active_set(no_manifest.source.replica.as_ref()),
        Err(ModelsError::Incomplete { .. })
    ));

    // Manifest present with file_count higher than present file rows.
    let missing_file = local_replica();
    publish_with(
        missing_file.source.replica.as_ref(),
        &[
            ("vad/model.onnx", vad_bytes.as_slice()),
            ("LICENSE", license_bytes.as_slice()),
        ],
        SAMPLE_MODELS_TOML,
        PublishOpts {
            file_count_override: Some(3),
            ..Default::default()
        },
    );
    assert!(matches!(
        active_set(missing_file.source.replica.as_ref()),
        Err(ModelsError::Incomplete { .. })
    ));
}

#[test]
fn tampered_version_id_is_refused() {
    let harness = local_replica();
    let vad_bytes = pseudo_random(40, 1024);
    let real_file = ModelFile {
        path: "vad/model.onnx".to_owned(),
        size: vad_bytes.len() as u64,
        sha256: sha256_hex(&vad_bytes),
    };
    let real_id =
        version_id(std::slice::from_ref(&real_file), SAMPLE_MODELS_TOML).expect("real id");
    let tampered_id = other_version_id(&real_id);

    publish_model_files(
        harness.source.replica.as_ref(),
        &[real_file],
        SAMPLE_MODELS_TOML,
        PublishOpts {
            version_override: Some(tampered_id),
            ..Default::default()
        },
    );

    assert!(matches!(
        active_set(harness.source.replica.as_ref()),
        Err(ModelsError::Incomplete { .. })
    ));
}

/// A file row whose blob names other content than its `{size, sha256}`
/// refuses the whole version: the device would fetch bytes the row does not
/// describe.
#[test]
fn a_file_row_whose_blob_names_other_content_is_refused() {
    let harness = local_replica();
    let replica = harness.source.replica.as_ref();
    let bytes = pseudo_random(41, 1024);
    let (version, _) = publish(
        replica,
        &[("vad/model.onnx", bytes.as_slice())],
        SAMPLE_MODELS_TOML,
    );
    assert!(active_set(replica).expect("complete").is_some());

    let digest = sha256_hex(&bytes);
    put_row(
        replica,
        &file_key(&version, "vad/model.onnx"),
        json!({ "size": bytes.len(), "sha256": digest }),
        Some(
            json!({ "blobId": format!("sha256:{}", sha256_hex(b"other")), "byteLength": bytes.len() }),
        ),
    );
    assert!(matches!(
        active_set(replica),
        Err(ModelsError::Incomplete { .. })
    ));

    put_row(
        replica,
        &file_key(&version, "vad/model.onnx"),
        json!({ "size": bytes.len(), "sha256": digest }),
        Some(json!({ "blobId": format!("sha256:{digest}"), "byteLength": bytes.len() + 1 })),
    );
    assert!(matches!(
        active_set(replica),
        Err(ModelsError::Incomplete { .. })
    ));
}

#[test]
fn interrupted_publication_keeps_v1_then_recovers_to_v2() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    let interrupt = AtomicBool::new(false);

    // Publish and hydrate v1.
    let vad_v1 = pseudo_random(50, 8 * 1024);
    let lic_v1 = b"v1 license\n".to_vec();
    stand_in.seed_blob(&vad_v1);
    stand_in.seed_blob(&lic_v1);
    let (v1, _) = publish(
        harness.source.replica.as_ref(),
        &[
            ("vad/model.onnx", vad_v1.as_slice()),
            ("LICENSE", lic_v1.as_slice()),
        ],
        SAMPLE_MODELS_TOML,
    );
    let set_v1 = active_set(harness.source.replica.as_ref())
        .expect("v1 active_set")
        .expect("v1 present");
    hydrate(&set_v1, group, &transfer, dest.path(), &interrupt).expect("hydrate v1");
    assert_eq!(ready_version(dest.path(), "turn"), Some(v1.clone()));
    assert!(is_current(&set_v1, group, dest.path()));

    // Write v2 file rows without manifest or models.active flip.
    let vad_v2 = pseudo_random(51, 8 * 1024);
    let lic_v2 = b"v2 license\n".to_vec();
    let v2_files = [
        ("vad/model.onnx", vad_v2.as_slice()),
        ("LICENSE", lic_v2.as_slice()),
    ];
    let (v2, v2_blobs) = publish_with(
        harness.source.replica.as_ref(),
        &v2_files,
        SAMPLE_MODELS_TOML,
        PublishOpts {
            write_manifest: false,
            set_active: false,
            ..Default::default()
        },
    );
    assert_ne!(v1, v2);
    let still_v1 = active_set(harness.source.replica.as_ref())
        .expect("still v1")
        .expect("v1 present");
    assert_eq!(still_v1.version, v1);
    assert!(is_current(&still_v1, group, dest.path()));

    // Complete v2 publication, but plant a wrong body for vad/model.onnx.
    publish_with(
        harness.source.replica.as_ref(),
        &v2_files,
        SAMPLE_MODELS_TOML,
        PublishOpts {
            write_files: false,
            write_manifest: true,
            set_active: true,
            ..Default::default()
        },
    );
    let wrong_vad_v2 = pseudo_random(52, vad_v2.len());
    stand_in.seed_blob_as(&v2_blobs[0].0, &wrong_vad_v2);
    stand_in.seed_blob(&lic_v2);

    let set_v2 = active_set(harness.source.replica.as_ref())
        .expect("v2 active_set")
        .expect("v2 present");
    assert_eq!(set_v2.version, v2);
    hydrate(&set_v2, group, &transfer, dest.path(), &interrupt)
        .expect_err("planted v2 body must fail");
    assert!(ready_digest(dest.path(), "turn").is_none());
    assert!(ready_version(dest.path(), "turn").is_none());

    // Fix the body and rerun: group becomes ready on v2.
    stand_in.seed_blob(&vad_v2);
    hydrate(&set_v2, group, &transfer, dest.path(), &interrupt).expect("hydrate v2");
    assert_eq!(ready_version(dest.path(), "turn"), Some(v2));
    assert!(is_current(&set_v2, group, dest.path()));
    assert_eq!(
        std::fs::read(dest.path().join("vad/model.onnx")).expect("v2 vad"),
        vad_v2
    );
    assert_eq!(
        std::fs::read(dest.path().join("LICENSE")).expect("v2 license"),
        lic_v2
    );
}

#[test]
fn second_run_fetches_nothing_and_leaves_unmanaged_files() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());

    let sample = SampleFiles::new();
    sample.seed_all(&stand_in);
    publish(
        harness.source.replica.as_ref(),
        &sample.as_slices(),
        SAMPLE_MODELS_TOML,
    );
    let set = active_set(harness.source.replica.as_ref())
        .expect("active_set")
        .expect("present");
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    let interrupt = AtomicBool::new(false);

    hydrate(&set, group, &transfer, dest.path(), &interrupt).expect("first hydrate");
    let extra_path = dest.path().join("extra-local.bin");
    std::fs::write(&extra_path, b"keep me").expect("write extra");
    let served_before = stand_in.store_bytes_served();

    let second = hydrate(&set, group, &transfer, dest.path(), &interrupt).expect("second hydrate");
    assert_eq!(second.downloaded, 0);
    assert_eq!(second.downloaded_bytes, 0);
    assert_eq!(second.copied, 0);
    assert_eq!(second.skipped, 3);
    assert_eq!(stand_in.store_bytes_served(), served_before);
    assert_eq!(
        std::fs::read(&extra_path).expect("extra intact"),
        b"keep me"
    );
}

#[test]
fn caseless_collision_is_refused_and_writes_nothing() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest_root = tempfile::tempdir().expect("dest root");
    let dest = dest_root.path().join("models");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());

    let upper = pseudo_random(60, 1024);
    let lower = pseudo_random(61, 1024);
    stand_in.seed_blob(&upper);
    stand_in.seed_blob(&lower);

    let (version, _) = publish(
        harness.source.replica.as_ref(),
        &[
            ("asr/Encoder.bin", upper.as_slice()),
            ("asr/encoder.bin", lower.as_slice()),
        ],
        SAMPLE_MODELS_TOML,
    );
    assert!(matches!(
        active_set(harness.source.replica.as_ref()),
        Err(ModelsError::Collision { .. })
    ));

    let colliding = ActiveSet {
        version,
        models_toml: SAMPLE_MODELS_TOML.to_owned(),
        files: vec![
            SetFile {
                path: "asr/Encoder.bin".to_owned(),
                size: upper.len() as u64,
                sha256: sha256_hex(&upper),
            },
            SetFile {
                path: "asr/encoder.bin".to_owned(),
                size: lower.len() as u64,
                sha256: sha256_hex(&lower),
            },
        ],
    };
    let vad = ["vad".to_owned()];
    let group = transcription_group(&vad);
    let interrupt = AtomicBool::new(false);
    assert!(matches!(
        hydrate(&colliding, group, &transfer, &dest, &interrupt),
        Err(ModelsError::Collision { .. })
    ));
    assert!(!dest.exists());
}

#[test]
fn fetches_use_blob_route_and_refuse_foreign_origin() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());

    let sample = SampleFiles::new();
    sample.seed_all(&stand_in);
    publish(
        harness.source.replica.as_ref(),
        &sample.as_slices(),
        SAMPLE_MODELS_TOML,
    );
    let set = active_set(harness.source.replica.as_ref())
        .expect("active_set")
        .expect("present");
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    let interrupt = AtomicBool::new(false);

    hydrate(&set, group, &transfer, dest.path(), &interrupt).expect("hydrate");
    let requests = stand_in.grant_requests();
    assert!(!requests.is_empty());
    assert!(requests
        .iter()
        .all(|req| req.starts_with("GET /makidrive/blobs/sha256:")));
    assert!(requests.iter().all(|req| !req.contains("/files/")));

    // Foreign origin announced while slot holds the real store origin: refused.
    stand_in.announce_foreign(true);
    let dest_foreign = tempfile::tempdir().expect("foreign dest");
    let err = hydrate(&set, group, &transfer, dest_foreign.path(), &interrupt)
        .expect_err("foreign origin must be refused");
    assert!(matches!(err, ModelsError::Transfer { .. }));
    assert!(!dest_foreign.path().join("vad/model.onnx").exists());
    assert!(!dest_foreign.path().join("LICENSE").exists());
    assert!(ready_digest(dest_foreign.path(), "turn").is_none());
}

#[derive(Deserialize)]
struct SettingsVectors {
    #[serde(rename = "modelVersions")]
    model_versions: Vec<ModelVersionVector>,
}

#[derive(Deserialize)]
struct ModelVersionVector {
    name: String,
    files: Vec<ModelFile>,
    models_toml: String,
    version: Option<String>,
}

#[test]
fn version_id_vectors_replay_and_reject_mismatched_ids() {
    let vectors: SettingsVectors =
        serde_json::from_str(include_str!("../../../../syncular/settings.vectors.json"))
            .expect("parse settings.vectors.json");

    let mut checked = 0usize;
    for case in vectors.model_versions {
        let Some(expected) = case.version else {
            continue;
        };
        checked += 1;

        let harness = local_replica();
        publish_model_files(
            harness.source.replica.as_ref(),
            &case.files,
            &case.models_toml,
            PublishOpts {
                version_override: Some(expected.clone()),
                ..Default::default()
            },
        );
        let set = active_set(harness.source.replica.as_ref())
            .unwrap_or_else(|e| panic!("case '{}' failed: {e}", case.name))
            .expect("active set present");
        assert_eq!(set.version, expected, "case '{}'", case.name);

        let wrong_harness = local_replica();
        publish_model_files(
            wrong_harness.source.replica.as_ref(),
            &case.files,
            &case.models_toml,
            PublishOpts {
                version_override: Some(other_version_id(&expected)),
                ..Default::default()
            },
        );
        assert!(
            matches!(
                active_set(wrong_harness.source.replica.as_ref()),
                Err(ModelsError::Incomplete { .. })
            ),
            "case '{}' must refuse mismatched version id",
            case.name
        );
    }
    assert!(checked >= 6, "expected at least 6 valid vector cases");
}

#[test]
fn cut_download_resumes_on_retry() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());

    let encoder = pseudo_random(70, ENCODER_BYTES);
    stand_in.seed_blob(&encoder);
    let (version, _) = publish(
        harness.source.replica.as_ref(),
        &[("asr/encoder.bin", encoder.as_slice())],
        SAMPLE_MODELS_TOML,
    );
    let set = active_set(harness.source.replica.as_ref())
        .expect("active_set")
        .expect("present");
    let vad = ["vad".to_owned()];
    let group = transcription_group(&vad);
    let interrupt = AtomicBool::new(false);

    // The cut is resumed by `Range` from the bytes already staged, within the
    // run or at the next one.
    stand_in.cut_next(512 * 1024);
    let report = match hydrate(&set, group, &transfer, dest.path(), &interrupt) {
        Ok(report) => report,
        Err(err) => {
            assert!(matches!(err, ModelsError::Transfer { .. }), "{err}");
            assert!(ready_digest(dest.path(), "transcription").is_none());
            hydrate(&set, group, &transfer, dest.path(), &interrupt).expect("resumed hydrate")
        }
    };
    assert_eq!(report.downloaded, 1);
    assert_eq!(
        std::fs::read(dest.path().join("asr/encoder.bin")).expect("encoder"),
        encoder
    );
    assert_eq!(ready_version(dest.path(), "transcription"), Some(version));
    assert!(stand_in.store_bytes_served() < 2 * (encoder.len() as u64));
}

#[cfg(unix)]
#[test]
fn folder_symlink_escaping_dest_is_refused() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let outside = tempfile::tempdir().expect("outside dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());

    std::os::unix::fs::symlink(outside.path(), dest.path().join("vad")).expect("create symlink");

    let vad_bytes = pseudo_random(80, 2048);
    stand_in.seed_blob(&vad_bytes);
    publish(
        harness.source.replica.as_ref(),
        &[("vad/model.onnx", vad_bytes.as_slice())],
        SAMPLE_MODELS_TOML,
    );
    let set = active_set(harness.source.replica.as_ref())
        .expect("active_set")
        .expect("present");
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    let interrupt = AtomicBool::new(false);

    let err = hydrate(&set, group, &transfer, dest.path(), &interrupt)
        .expect_err("symlink escape must be refused");
    assert!(matches!(err, ModelsError::Path { .. }));
    assert!(!outside.path().join("model.onnx").exists());
    assert!(std::fs::read_dir(outside.path())
        .expect("read outside dir")
        .next()
        .is_none());
    assert!(ready_digest(dest.path(), "turn").is_none());
}

/// A transfer that calls `on_download` with the content id before passing
/// each download on: a window into the run between its decisions.
struct Watching<F: Fn(&str) + Send + Sync> {
    inner: StoreTransfer,
    on_download: F,
}

impl<F: Fn(&str) + Send + Sync> FileTransfer for Watching<F> {
    fn upload(
        &self,
        file: &Path,
        media_type: Option<&str>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Uploaded, ReplicaError> {
        self.inner.upload(file, media_type, progress)
    }

    fn download(
        &self,
        content_id: &str,
        size: u64,
        dest: &Path,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Downloaded, ReplicaError> {
        (self.on_download)(content_id);
        self.inner.download(content_id, size, dest, progress)
    }
}

struct TwoVersions {
    v1: String,
    v2: ActiveSet,
    vad_v1: Vec<u8>,
    vad_v2: Vec<u8>,
}

/// The turn group hydrated on v1 into `dest`, then v2 — both files changed —
/// published and active.
fn v1_ready_then_v2_active(harness: &Harness, stand_in: &StandIn, dest: &Path) -> TwoVersions {
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(stand_in, plan_dir.path());
    let replica = harness.source.replica.as_ref();
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);

    let vad_v1 = pseudo_random(60, 8 * 1024);
    let lic_v1 = b"v1 license\n".to_vec();
    stand_in.seed_blob(&vad_v1);
    stand_in.seed_blob(&lic_v1);
    let (v1, _) = publish(
        replica,
        &[
            ("vad/model.onnx", vad_v1.as_slice()),
            ("LICENSE", lic_v1.as_slice()),
        ],
        SAMPLE_MODELS_TOML,
    );
    let set_v1 = active_set(replica).expect("v1 active_set").expect("v1");
    hydrate(&set_v1, group, &transfer, dest, &AtomicBool::new(false)).expect("hydrate v1");
    assert_eq!(ready_version(dest, "turn"), Some(v1.clone()));

    let vad_v2 = pseudo_random(61, 8 * 1024);
    let lic_v2 = b"v2 license\n".to_vec();
    stand_in.seed_blob(&vad_v2);
    stand_in.seed_blob(&lic_v2);
    publish(
        replica,
        &[
            ("vad/model.onnx", vad_v2.as_slice()),
            ("LICENSE", lic_v2.as_slice()),
        ],
        SAMPLE_MODELS_TOML,
    );
    let v2 = active_set(replica).expect("v2 active_set").expect("v2");
    assert_ne!(v2.version, v1);
    TwoVersions {
        v1,
        v2,
        vad_v1,
        vad_v2,
    }
}

/// A body of the right size planted (or left by a run) at the staging path
/// is hashed, not believed: it is never placed or vouched for.
#[test]
fn a_wrong_staged_body_of_the_right_size_is_refused() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());

    let vad_bytes = pseudo_random(70, 16 * 1024);
    let license = b"MIT\n".to_vec();
    stand_in.seed_blob(&license);
    publish(
        harness.source.replica.as_ref(),
        &[
            ("vad/model.onnx", vad_bytes.as_slice()),
            ("LICENSE", license.as_slice()),
        ],
        SAMPLE_MODELS_TOML,
    );
    let staging = dest.path().join(".keeper-hydrate-blobs");
    std::fs::create_dir_all(&staging).expect("staging dir");
    let wrong = pseudo_random(71, vad_bytes.len());
    std::fs::write(staging.join(sha256_hex(&vad_bytes)), &wrong).expect("plant staged body");

    let set = active_set(harness.source.replica.as_ref())
        .expect("active_set")
        .expect("present");
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    let interrupt = AtomicBool::new(false);
    // The store does not serve the body, so only the planted one is at hand.
    let err = hydrate(&set, group, &transfer, dest.path(), &interrupt)
        .expect_err("a wrong staged body must not be placed");
    assert!(matches!(
        err,
        ModelsError::Transfer { .. } | ModelsError::Integrity { .. }
    ));
    assert!(!dest.path().join("vad/model.onnx").exists());
    assert!(ready_digest(dest.path(), "turn").is_none());

    // Once the store serves the published body, it is what lands.
    stand_in.seed_blob(&vad_bytes);
    hydrate(&set, group, &transfer, dest.path(), &interrupt).expect("hydrate");
    assert_eq!(
        std::fs::read(dest.path().join("vad/model.onnx")).expect("vad"),
        vad_bytes
    );
    assert_eq!(ready_version(dest.path(), "turn"), Some(set.version));
}

/// The previous version's marker is gone before the first file of the new
/// one is fetched, so a run killed mid-way never leaves a group ready on
/// files of both versions.
#[test]
fn the_old_marker_is_gone_while_a_new_version_downloads() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let two = v1_ready_then_v2_active(&harness, &stand_in, dest.path());

    let plan_dir = tempfile::tempdir().expect("plan dir");
    let seen = Mutex::new(Vec::new());
    let transfer = Watching {
        inner: make_transfer(&stand_in, plan_dir.path()),
        on_download: |_: &str| {
            seen.lock()
                .expect("seen")
                .push(ready_digest(dest.path(), "turn"));
        },
    };
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    hydrate(
        &two.v2,
        group,
        &transfer,
        dest.path(),
        &AtomicBool::new(false),
    )
    .expect("hydrate v2");

    assert_eq!(*seen.lock().expect("seen"), vec![None, None]);
    assert_eq!(ready_version(dest.path(), "turn"), Some(two.v2.version));
}

/// An interrupt raised during a run stops it before the next file: the run
/// is cancelled, nothing is vouched for, and the next run fetches only what
/// the cancelled one did not place.
#[test]
fn an_interrupt_cancels_the_run() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let two = v1_ready_then_v2_active(&harness, &stand_in, dest.path());

    let plan_dir = tempfile::tempdir().expect("plan dir");
    let interrupt = AtomicBool::new(false);
    let downloads = Mutex::new(0_u32);
    let transfer = Watching {
        inner: make_transfer(&stand_in, plan_dir.path()),
        on_download: |_: &str| {
            *downloads.lock().expect("downloads") += 1;
            interrupt.store(true, Ordering::Relaxed);
        },
    };
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    let err = hydrate(&two.v2, group, &transfer, dest.path(), &interrupt)
        .expect_err("an interrupted run must not finish");
    assert!(matches!(err, ModelsError::Cancelled));
    assert_eq!(*downloads.lock().expect("downloads"), 1);
    assert!(ready_digest(dest.path(), "turn").is_none());

    interrupt.store(false, Ordering::Relaxed);
    let report = hydrate(&two.v2, group, &transfer, dest.path(), &interrupt).expect("resumed run");
    assert_eq!(report.downloaded, 1);
    assert_eq!(ready_version(dest.path(), "turn"), Some(two.v2.version));
    assert_eq!(
        std::fs::read(dest.path().join("vad/model.onnx")).expect("v2 vad"),
        two.vad_v2
    );
}

/// A run cancelled before it changed anything still leaves the set
/// unvouched for: the old version's marker does not outlive a run that
/// was asked to bring in a new one.
#[test]
fn a_cancelled_run_leaves_the_set_unvouched_for() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let dest = tempfile::tempdir().expect("dest dir");
    let two = v1_ready_then_v2_active(&harness, &stand_in, dest.path());
    assert_eq!(ready_version(dest.path(), "turn"), Some(two.v1.clone()));

    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());
    let served_before = stand_in.store_bytes_served();
    let vad = ["vad".to_owned()];
    let group = turn_group(&vad);
    let err = hydrate(
        &two.v2,
        group,
        &transfer,
        dest.path(),
        &AtomicBool::new(true),
    )
    .expect_err("a cancelled run must not finish");
    assert!(matches!(err, ModelsError::Cancelled));
    assert!(ready_digest(dest.path(), "turn").is_none());
    assert!(ready_version(dest.path(), "turn").is_none());
    assert_eq!(stand_in.store_bytes_served(), served_before);
    assert_eq!(
        std::fs::read(dest.path().join("vad/model.onnx")).expect("v1 vad"),
        two.vad_v1
    );
}

/// A group name is the marker's and state's file name: anything but
/// lowercase ASCII letters is refused before the folder is touched.
#[test]
fn a_group_name_that_is_not_letters_is_refused() {
    let harness = local_replica();
    let stand_in = StandIn::start(DATABASE);
    let root = tempfile::tempdir().expect("root dir");
    let plan_dir = tempfile::tempdir().expect("plan dir");
    let transfer = make_transfer(&stand_in, plan_dir.path());
    let sample = SampleFiles::new();
    sample.seed_all(&stand_in);
    publish(
        harness.source.replica.as_ref(),
        &sample.as_slices(),
        SAMPLE_MODELS_TOML,
    );
    let set = active_set(harness.source.replica.as_ref())
        .expect("active_set")
        .expect("present");
    let vad = ["vad".to_owned()];
    let dest = root.path().join("models");

    for name in ["", "Turn", "turn-1", "turn.x", "tür", "../turn"] {
        let group = Group {
            name,
            folders: Folders::Only(&vad),
            fingerprint: None,
        };
        let err = hydrate(&set, group, &transfer, &dest, &AtomicBool::new(false))
            .expect_err("a group name that is not letters must be refused");
        assert!(
            matches!(&err, ModelsError::Group(refused) if refused == name),
            "{name:?}: {err:?}"
        );
        assert!(!dest.exists(), "{name:?} touched the folder");
    }
    assert_eq!(stand_in.store_bytes_served(), 0);
}
