mod helpers;
use helpers::*;
use pojoc::{LazyView, pojvec};
use pojoc_tests::pojoc_edge::*;

#[test]
fn test_roundtrip_default() {
    let original = Edge::default();
    let mut buf = Vec::new();
    encode(&mut buf, &original);
    let decoded = decode(&buf).expect("decode failed");
    assert_edge_eq(&original, &decoded);
}

#[test]
fn test_roundtrip_populated() {
    let original = make_populated_edge();
    let mut buf = Vec::new();
    encode(&mut buf, &original);
    let decoded = decode(&buf).expect("decode failed");
    assert_edge_eq(&original, &decoded);
}

#[test]
fn test_encode_for_version_default_decodes_all() {
    let original = Edge::default();

    for &version in supported_versions() {
        let mut buf = Vec::new();
        encode_for_version(&mut buf, &original, version)
            .unwrap_or_else(|e| panic!("v{version}: encode_for_version failed: {e:?}"));
        decode(&buf).unwrap_or_else(|e| panic!("v{version}: decode failed: {e:?}"));
    }
}

#[test]
fn test_encode_for_version_populated_decodes_all() {
    let original = make_version_probe_edge();

    for &version in supported_versions() {
        let mut buf = Vec::new();
        encode_for_version(&mut buf, &original, version)
            .unwrap_or_else(|e| panic!("v{version}: encode_for_version failed: {e:?}"));
        decode(&buf).unwrap_or_else(|e| panic!("v{version}: decode failed: {e:?}"));
    }
}

#[test]
fn test_encode_for_version_populated_stable_fields_survive_all_versions() {
    let original = make_version_probe_edge();

    for &version in supported_versions() {
        let mut buf = Vec::new();
        encode_for_version(&mut buf, &original, version)
            .unwrap_or_else(|e| panic!("v{version}: encode_for_version failed: {e:?}"));
        let decoded = decode(&buf).unwrap_or_else(|e| panic!("v{version}: decode failed: {e:?}"));

        assert_eq!(
            decoded.u8_to_i64, original.u8_to_i64,
            "v{version}: u8_to_i64 mismatch"
        );
        assert_eq!(
            decoded.nullified_str, original.nullified_str,
            "v{version}: nullified_str mismatch"
        );
        assert_eq!(
            decoded.root_struct.leaf.leaf_val, original.root_struct.leaf.leaf_val,
            "v{version}: root_struct.leaf.leaf_val mismatch"
        );
        assert_eq!(
            decoded.root_struct.leaf.leaf_numeric, original.root_struct.leaf.leaf_numeric,
            "v{version}: root_struct.leaf.leaf_numeric mismatch"
        );
    }
}

#[test]
fn test_encode_for_version_latest_version_fields_survive() {
    let mut original = make_version_probe_edge();
    original.action = Payload::Heal(HealPayload {
        target_id: 2,
        amount: 8.0,
        overheal: false,
        splash_radius: 3.0,
    });
    original.control = ControlSignal::Disconnect(DisconnectPayload { reason_code: 1 });

    let latest = *supported_versions().last().expect("no supported versions");

    let mut buf = Vec::new();
    encode_for_version(&mut buf, &original, latest)
        .unwrap_or_else(|e| panic!("v{latest}: encode_for_version failed: {e:?}"));
    let decoded = decode(&buf).unwrap_or_else(|e| panic!("v{latest}: decode failed: {e:?}"));

    assert_eq!(
        decoded.bounds_enum, original.bounds_enum,
        "v{latest}: bounds_enum mismatch"
    );
    assert_eq!(
        decoded.system_perms, original.system_perms,
        "v{latest}: system_perms mismatch"
    );
    assert_payload_eq(&decoded.action, &original.action);
    assert_control_signal_eq(&decoded.control, &original.control);
}
#[test]
fn test_encode_for_version_latest_is_byte_identical_to_encode() {
    let original = make_version_probe_edge();
    let latest = *supported_versions().last().expect("no supported versions");

    let mut buf_encode = Vec::new();
    encode(&mut buf_encode, &original);

    let mut buf_versioned = Vec::new();
    encode_for_version(&mut buf_versioned, &original, latest)
        .expect("encode_for_version failed for latest");

    assert_eq!(
        buf_encode, buf_versioned,
        "encode() and encode_for_version(latest) produced different bytes"
    );
}

#[test]
fn test_roundtrip_payload_variants() {
    let variants = vec![
        Payload::Move(MovePayload { dx: 7, dy: -2 }),
        Payload::Attack(AttackPayload {
            target_id: 11,
            damage: 33.3,
            knockback: 0.5,
        }),
        Payload::Heal(HealPayload {
            target_id: 4,
            amount: 50.0,
            overheal: true,
            splash_radius: 1.5,
        }),
        Payload::Despawn(DespawnPayload { entity_id: 808 }),
    ];

    for variant in variants {
        let mut e = Edge::default();
        e.action = variant;
        let mut buf = Vec::new();
        encode(&mut buf, &e);
        let decoded = decode(&buf).expect("decode failed");
        assert_payload_eq(&e.action, &decoded.action);
    }
}

#[test]
fn test_roundtrip_control_signal_variants() {
    let variants = vec![
        ControlSignal::Ping(PingPayload {}),
        ControlSignal::Pong(PongPayload { latency_ms: 250 }),
        ControlSignal::Disconnect(DisconnectPayload { reason_code: 7 }),
    ];

    for variant in variants {
        let mut e = Edge::default();
        e.control = variant;
        let mut buf = Vec::new();
        encode(&mut buf, &e);
        let decoded = decode(&buf).expect("decode failed");
        assert_control_signal_eq(&e.control, &decoded.control);
    }
}

#[test]
fn test_roundtrip_unknown_union_variant_is_lossless() {
    // Simulates a proxy/middleware scenario: a peer running a newer schema
    // sends a Payload variant this binary doesn't recognize. The decoder
    // should preserve it as Unknown { discriminant, data } rather than
    // erroring, and re-encoding must reproduce the exact same bytes.
    let mut e = Edge::default();
    e.action = Payload::Unknown {
        discriminant: 9999,
        data: vec![0xDE, 0xAD, 0xBE, 0xEF],
    };

    let mut buf = Vec::new();
    encode(&mut buf, &e);
    let decoded = decode(&buf).expect("decode failed");

    match &decoded.action {
        Payload::Unknown { discriminant, data } => {
            assert_eq!(*discriminant, 9999);
            assert_eq!(data, &vec![0xDE, 0xAD, 0xBE, 0xEF]);
        }
        other => panic!("expected Unknown variant to survive roundtrip, got {other:?}"),
    }
}

#[test]
fn test_raw_passthrough_is_byte_identical() {
    let original = make_populated_edge();
    let mut buf = Vec::new();
    encode(&mut buf, &original);

    let decoded = decode(&buf).expect("decode failed");
    let mut reencoded = Vec::new();
    encode(&mut reencoded, &decoded);

    assert_eq!(
        buf, reencoded,
        "lazy Raw fields did not pass through byte-identical"
    );
}

#[test]
fn test_lazy_field_owned_roundtrip() {
    let mut e = Edge::default();
    let expected = Some(pojvec!("wow"));
    e.lazy_audit_log = LazyView::Owned(Some(pojvec!("wow")));

    let mut buf = Vec::new();
    encode(&mut buf, &e);
    let decoded = decode(&buf).expect("decode failed");

    let value = decoded.lazy_audit_log.get().expect("lazy get failed");
    assert_eq!(value, expected);
}

#[test]
fn test_lazy_field_default_is_owned_not_raw() {
    let e = Edge::default();
    match &e.lazy_audit_log {
        LazyView::Owned(_) => {}
        LazyView::Raw { .. } => panic!("default lazy field should be Owned, not Raw"),
    }
}
