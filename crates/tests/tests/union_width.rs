use pojoc_tests::pojoc_union_width::runtime::Error;
use pojoc_tests::pojoc_union_width::*;

// Big@1 has 3 variants (width u8); Big@2 (latest) crosses 256, widening to u16.
// `x` is re-pinned to Big@2, so encode_for_version(..., 1) is needed to exercise Big@1's own codec.

#[test]
fn test_v1_known_variant_roundtrips_at_v1_width() {
    let original = UnionWidth { x: Big::V1(42) };
    let mut buf = Vec::new();
    encode_for_version(&mut buf, &original, 1).expect("encode at v1 failed");
    let decoded = decode(&buf).expect("decode failed");
    match decoded.x {
        Big::V1(v) => assert_eq!(v, 42),
        other => panic!("expected V1, got {other:?}"),
    }
}

#[test]
fn test_latest_variant_past_width_threshold_roundtrips() {
    // W259 pushes the union past 256 variants, only representable at u16.
    let original = UnionWidth { x: Big::W259(99) };
    let mut buf = Vec::new();
    encode(&mut buf, &original).expect("encode failed");
    let decoded = decode(&buf).expect("decode failed");
    match decoded.x {
        Big::W259(v) => assert_eq!(v, 99),
        other => panic!("expected W259, got {other:?}"),
    }
}

#[test]
fn test_variant_added_after_v1_downgrades_to_unknown_when_encoded_at_v1() {
    // W2 only exists as of Big@2, so encoding via v1's codec must downgrade to Unknown.
    let original = UnionWidth { x: Big::W2(7) };
    let mut buf = Vec::new();
    encode_for_version(&mut buf, &original, 1).expect("encode at v1 failed");
    let decoded = decode(&buf).expect("decode failed");
    match decoded.x {
        Big::Unknown { discriminant, .. } => assert_eq!(discriminant, 5),
        other => panic!("expected Unknown (downgrade), got {other:?}"),
    }
}

// regression test: a discriminant fitting the latest width (u16) but not an
// older pinned version's frozen width (u8) must fail loudly, not truncate.
#[test]
fn test_encode_at_older_version_rejects_discriminant_overflowing_that_versions_width() {
    let original = UnionWidth {
        x: Big::Unknown {
            discriminant: 9999, // fits u16 (latest), not u8 (Big@1's own width)
            data: vec![],
        },
    };
    let mut buf = Vec::new();
    let err = encode_for_version(&mut buf, &original, 1)
        .expect_err("9999 must not fit Big@1's own (u8) wire width");
    assert_eq!(err, Error::DiscriminantOverflow);

    // the same value encodes fine at latest, where it fits
    let mut buf2 = Vec::new();
    encode(&mut buf2, &original).expect("9999 fits Big's latest (u16) width");
}
