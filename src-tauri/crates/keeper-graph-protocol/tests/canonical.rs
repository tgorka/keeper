//! Independent oracle: RFC 8785 §3.2.3 and Appendix B, not serializer self-echo.
use keeper_graph_protocol::wire::canonical_bytes;
use serde_json::{json, Value};
#[test]
fn rfc_utf16_order_and_numeric_looking_keys() {
    let input: Value = serde_json::from_str(r#"{"\u20ac":"Euro Sign","\r":"Carriage Return","\ufb33":"Hebrew Letter Dalet With Dagesh","1":"One","\ud83d\ude00":"Emoji: Grinning Face","\u0080":"Control","\u00f6":"Latin Small Letter O With Diaeresis"}"#).expect("RFC JSON");
    let expected = "{\"\\r\":\"Carriage Return\",\"1\":\"One\",\"\u{80}\":\"Control\",\"ö\":\"Latin Small Letter O With Diaeresis\",\"€\":\"Euro Sign\",\"\u{1f600}\":\"Emoji: Grinning Face\",\"דּ\":\"Hebrew Letter Dalet With Dagesh\"}";
    assert_eq!(
        canonical_bytes(&input).expect("canonical"),
        expected.as_bytes()
    );
    assert_eq!(
        canonical_bytes(&json!({"2":0,"10":0,"1":0})).expect("canonical"),
        br#"{"1":0,"10":0,"2":0}"#
    );
}
#[test]
fn rfc_ieee_samples_and_kg_number_range() {
    for (bits, expected) in [
        (0x0000000000000000u64, "0"),
        (0x8000000000000000, "0"),
        (0x0000000000000001, "5e-324"),
        (0x8000000000000001, "-5e-324"),
        (0x3eb0c6f7a0b5ed8c, "9.999999999999997e-7"),
        (0x3eb0c6f7a0b5ed8d, "0.000001"),
        (0x41b3de4355555553, "333333333.3333332"),
        (0x41b3de4355555554, "333333333.33333325"),
        (0x41b3de4355555555, "333333333.3333333"),
        (0x41b3de4355555556, "333333333.3333334"),
        (0x41b3de4355555557, "333333333.33333343"),
        (0xbecbf647612f3696, "-0.0000033333333333333333"),
        (0x43143ff3c1cb0959, "1424953923781206.2"),
    ] {
        assert_eq!(
            canonical_bytes(&json!(f64::from_bits(bits))).expect("canonical"),
            expected.as_bytes(),
            "{bits:016x}"
        );
    }
    assert_eq!(
        canonical_bytes(&json!(1)).expect("integer"),
        canonical_bytes(&json!(1.0)).expect("float")
    );
    assert!(canonical_bytes(&json!(9007199254740992u64)).is_err());
    // Serializer-level RFC examples beyond KG's narrower application range.
    for (bits, expected) in [
        (0x44b52d02c7e14af6u64, "1e+23"),
        (0x444b1ae4d6e2ef50, "1e+21"),
        (0x7fefffffffffffff, "1.7976931348623157e+308"),
    ] {
        assert_eq!(
            serde_jcs::to_vec(&f64::from_bits(bits)).expect("RFC"),
            expected.as_bytes()
        );
    }
    assert!(serde_jcs::to_vec(&f64::INFINITY).is_err());
    assert!(serde_jcs::to_vec(&f64::NAN).is_err());
}
