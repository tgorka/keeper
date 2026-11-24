use keeper_ports::layout::{
    content_key, file_prefix, metadata_value, parse_metadata, staging_key, staging_prefix,
    ObjectMeta, METADATA_HEADER,
};
use serde_json::Value;

const VECTORS_JSON: &str = include_str!("../../../../syncular/object-layout.vectors.json");
const LIMITS_JSON: &str = include_str!("../../../../syncular/limits.json");

fn part_bytes(size: u64, threshold: u64, base_part: u64, max_parts: u64) -> u64 {
    if size <= threshold {
        return 0;
    }
    const MIB: u64 = 1 << 20;
    let least = size.div_ceil(max_parts).div_ceil(MIB) * MIB;
    least.max(base_part)
}

#[test]
fn object_layout_vectors_match() {
    let vectors: Value = serde_json::from_str(VECTORS_JSON).expect("parse object-layout vectors");
    assert_eq!(vectors["version"].as_u64(), Some(1));

    let db = vectors["database"].as_str().expect("database");
    assert_eq!(
        file_prefix(db),
        vectors["filePrefix"].as_str().expect("filePrefix")
    );
    assert_eq!(
        staging_prefix(db),
        vectors["stagingPrefix"].as_str().expect("stagingPrefix")
    );

    let content_keys = vectors["contentKeys"].as_array().expect("contentKeys");
    assert!(!content_keys.is_empty());
    for entry in content_keys {
        let id = entry["contentId"].as_str().expect("contentId");
        let key = entry["key"].as_str().expect("key");
        assert_eq!(content_key(db, id).as_deref(), Some(key));
    }

    let refused = vectors["refusedContentIds"]
        .as_array()
        .expect("refusedContentIds");
    assert!(!refused.is_empty());
    for entry in refused {
        let id = entry.as_str().expect("refused contentId");
        assert_eq!(content_key(db, id), None, "expected refusal for {id}");
    }

    let staging_keys = vectors["stagingKeys"].as_array().expect("stagingKeys");
    assert!(!staging_keys.is_empty());
    for entry in staging_keys {
        let plan_id = entry["planId"].as_str().expect("planId");
        let key = entry["key"].as_str().expect("key");
        assert_eq!(staging_key(db, plan_id), key);
    }

    let metadata = vectors["metadata"].as_array().expect("metadata");
    assert!(!metadata.is_empty());
    for entry in metadata {
        assert_eq!(entry["header"].as_str(), Some(METADATA_HEADER));
        let raw_meta = &entry["meta"];
        let meta = ObjectMeta {
            content_id: raw_meta["contentId"]
                .as_str()
                .expect("meta.contentId")
                .to_owned(),
            size: raw_meta["size"].as_u64().expect("meta.size"),
            media_type: raw_meta
                .get("mediaType")
                .map(|v| v.as_str().expect("mediaType").to_owned()),
            uploaded_at_ms: raw_meta["uploadedAtMs"]
                .as_i64()
                .expect("meta.uploadedAtMs"),
        };
        let expected = entry["value"].as_str().expect("value");
        assert_eq!(metadata_value(&meta), expected);
        assert_eq!(parse_metadata(expected), Some(meta));
    }
}

#[test]
fn limits_vectors_match_contract_limits() {
    let vectors: Value = serde_json::from_str(VECTORS_JSON).expect("parse object-layout vectors");
    let limits_file: Value = serde_json::from_str(LIMITS_JSON).expect("parse limits.json");
    let v_limits = &vectors["limits"];

    let threshold = v_limits["filePartThresholdBytes"]
        .as_u64()
        .expect("filePartThresholdBytes");
    let part = v_limits["filePartBytes"].as_u64().expect("filePartBytes");
    let max_parts = v_limits["maxParts"].as_u64().expect("maxParts");

    assert_eq!(
        Some(threshold),
        limits_file["filePartThresholdBytes"].as_u64()
    );
    assert_eq!(Some(part), limits_file["filePartBytes"].as_u64());
    assert_eq!(max_parts, 10_000);

    let cases = v_limits["partBytesOf"].as_array().expect("partBytesOf");
    assert!(!cases.is_empty());
    for case in cases {
        let size = case["size"].as_u64().expect("size");
        let expected = case["partBytes"].as_u64().expect("partBytes");
        assert_eq!(
            part_bytes(size, threshold, part, max_parts),
            expected,
            "size={size}"
        );
    }
}
