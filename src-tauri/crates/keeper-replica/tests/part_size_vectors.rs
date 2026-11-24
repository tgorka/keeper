use keeper_replica::limits::MAX_PARTS;
use keeper_replica::transfer::TransferConfig;
use serde_json::Value;

const VECTORS_JSON: &str = include_str!("../../../../syncular/object-layout.vectors.json");

#[test]
fn the_upload_part_size_matches_every_vector() {
    let vectors: Value = serde_json::from_str(VECTORS_JSON).expect("parse object-layout vectors");
    let limits = &vectors["limits"];

    let config = TransferConfig::contract(std::env::temp_dir());
    assert_eq!(
        config.threshold,
        limits["filePartThresholdBytes"]
            .as_u64()
            .expect("filePartThresholdBytes"),
    );
    assert_eq!(
        config.part_bytes,
        limits["filePartBytes"].as_u64().expect("filePartBytes"),
    );
    assert_eq!(MAX_PARTS, limits["maxParts"].as_u64().expect("maxParts"));

    let cases = limits["partBytesOf"].as_array().expect("partBytesOf");
    assert!(!cases.is_empty());
    for case in cases {
        let size = case["size"].as_u64().expect("size");
        let expected = case["partBytes"].as_u64().expect("partBytes");
        assert_eq!(config.part_size(size), expected, "size={size}");
    }
}
