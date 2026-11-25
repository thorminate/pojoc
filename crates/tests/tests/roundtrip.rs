mod helpers;
use helpers::*;
use pojoc::{LazyView, pojvec};
use pojoc_tests::pojoc_edge::runtime::Error;
use pojoc_tests::pojoc_edge::*;

// leaks the buffer so the decoded &'buf str fields become 'static, matching a 'static-built original for assert_eq!
fn decode_static(buf: &[u8]) -> Edge<'static> {
    decode(Vec::leak(buf.to_vec())).expect("decode failed")
}

#[test]
fn test_roundtrip_default() {
    let original = Edge::default();
    let mut buf = Vec::new();
    encode(&mut buf, &original).expect("encode failed");
    let decoded = decode_static(&buf);
    assert_edge_eq(&original, &decoded);
}

#[test]
fn test_roundtrip_populated() {
    let original = make_populated_edge();
    let mut buf = Vec::new();
    encode(&mut buf, &original).expect("encode failed");
    let decoded = decode_static(&buf);
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
        let decoded = decode_static(&buf);

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
        assert_eq!(
            decoded.generic_box.value, original.generic_box.value,
            "v{version}: generic_box.value mismatch"
        );
        assert_eq!(
            decoded.generic_triple.first, original.generic_triple.first,
            "v{version}: generic_triple.first mismatch"
        );
        assert_eq!(
            decoded.generic_triple.second, original.generic_triple.second,
            "v{version}: generic_triple.second mismatch"
        );
        assert_eq!(
            decoded.generic_triple.third, original.generic_triple.third,
            "v{version}: generic_triple.third mismatch"
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
    assert_eq!(
        decoded.generic_box.label, original.generic_box.label,
        "v{latest}: generic_box.label mismatch"
    );
    assert_payload_eq(&decoded.action, &original.action);
    assert_control_signal_eq(&decoded.control, &original.control);
    assert_mono_string_eq(&decoded.generic_mono_v3, &original.generic_mono_v3);
    assert_duo_string_i32_eq(&decoded.generic_duo_v4, &original.generic_duo_v4);
    assert_mono_string_eq(&decoded.generic_mono_v5, &original.generic_mono_v5);

    assert_eq!(
        decoded.interned_label, original.interned_label,
        "v{latest}: interned_label mismatch"
    );
    assert_eq!(
        decoded.interned_tags, original.interned_tags,
        "v{latest}: interned_tags mismatch"
    );
    match (&decoded.linked_list, &original.linked_list) {
        (Some(x), Some(y)) => assert_linked_node_eq(x, y),
        (None, None) => {}
        _ => panic!("v{latest}: mismatch in optional presence for field 'linked_list'"),
    }
}

#[test]
fn test_encode_for_version_pre_v5_omits_new_fields() {
    // these fields were added in v5's diff, so encoding at an earlier version must drop them and decode to zero-value defaults
    let original = make_version_probe_edge();

    for &version in supported_versions() {
        if version >= 5 {
            continue;
        }
        let mut buf = Vec::new();
        encode_for_version(&mut buf, &original, version)
            .unwrap_or_else(|e| panic!("v{version}: encode_for_version failed: {e:?}"));
        let decoded = decode_static(&buf);

        assert_eq!(
            decoded.interned_label, "",
            "v{version}: interned_label should default"
        );
        assert!(
            decoded.interned_tags.is_empty(),
            "v{version}: interned_tags should default to empty"
        );
        assert!(
            decoded.linked_list.is_none(),
            "v{version}: linked_list should default to None"
        );
    }
}
#[test]
fn test_encode_for_version_latest_is_byte_identical_to_encode() {
    let original = make_version_probe_edge();
    let latest = *supported_versions().last().expect("no supported versions");

    let mut buf_encode = Vec::new();
    encode(&mut buf_encode, &original).expect("encode failed");

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
        Payload::Message("a borrowed string payload round-tripping through a union"),
        Payload::Bounded(50),
        Payload::BoundedStruct(BoundedPayload { value: 50 }),
        Payload::Rich(RichPayload {
            note: "rich-note",
            blob: LazyView::Owned(pojvec![1, 2, 3]),
        }),
    ];

    // final_action tracks Payload's latest shape; `action` is frozen at
    // Payload@2 and has its own test below.
    for variant in variants {
        let e = Edge {
            final_action: variant,
            ..Default::default()
        };
        let mut buf = Vec::new();
        encode(&mut buf, &e).expect("encode failed");
        let decoded = decode(&buf).expect("decode failed");
        assert_payload_eq(&e.final_action, &decoded.final_action);
    }
}

// regression test: a union variant's payload struct holding a `lazy` field
// used to reach codegen and panic (only the root's per-version decode_vN
// understood `lazy`, not a nested type's own read_foo/write_foo) — this
// exercises the Raw (decoded-then-re-encoded, never materialized) path
// specifically, complementing test_roundtrip_payload_variants's Owned path.
#[test]
fn test_rich_payload_lazy_field_raw_reencode_is_byte_identical() {
    let e = Edge {
        final_action: Payload::Rich(RichPayload {
            note: "rich-note",
            blob: LazyView::Owned(pojvec![9, 8, 7]),
        }),
        ..Default::default()
    };

    let mut buf = Vec::new();
    encode(&mut buf, &e).expect("encode failed");
    let decoded = decode_static(&buf);

    match &decoded.final_action {
        Payload::Rich(r) => match &r.blob {
            LazyView::Raw { .. } => {}
            LazyView::Owned(_) => panic!("decoded lazy union payload field should be Raw"),
        },
        other => panic!("expected Rich, got {other:?}"),
    }

    let mut buf2 = Vec::new();
    encode(&mut buf2, &decoded).expect("re-encode failed");
    assert_eq!(
        buf, buf2,
        "re-encoding a Raw lazy union payload field must be byte-identical"
    );
}

// regression test: a union variant's payload struct holding an `intern`
// field used to be rejected at analysis time (union codecs never threaded
// the shared string table) — this checks it actually dedupes against the
// same table as the rest of the message, not just that it compiles.
#[test]
fn test_rich_payload_interned_field_dedups_with_root_table() {
    let make = |note: &'static str| Edge {
        interned_label: note,
        final_action: Payload::Rich(RichPayload {
            note,
            blob: LazyView::Owned(pojvec![]),
        }),
        ..Default::default()
    };

    let mut shared_buf = Vec::new();
    encode(&mut shared_buf, &make("shared-interned-value")).expect("encode failed");

    let mut distinct_buf = Vec::new();
    encode(
        &mut distinct_buf,
        &Edge {
            interned_label: "aaaaaaaaaaaaaaaaaaaaa",
            final_action: Payload::Rich(RichPayload {
                note: "bbbbbbbbbbbbbbbbbbbbb",
                blob: LazyView::Owned(pojvec![]),
            }),
            ..Default::default()
        },
    )
    .expect("encode failed");

    assert!(
        shared_buf.len() < distinct_buf.len(),
        "identical root/union-payload interned strings should dedupe into one table entry, \
         shrinking the buffer versus two distinct strings of the same length"
    );
}

// regression test: `action` stays frozen at Payload@2 forever since it's never
// re-pinned, so Despawn (added at v5) must decode/encode as Unknown, not the real variant.
#[test]
fn test_field_frozen_at_older_union_pin_downgrades_newer_variants_to_unknown() {
    let e = Edge {
        action: Payload::Despawn(DespawnPayload { entity_id: 808 }),
        ..Default::default()
    };

    let mut buf = Vec::new();
    encode(&mut buf, &e).expect("encode failed");
    let decoded = decode(&buf).expect("decode failed");

    match &decoded.action {
        Payload::Unknown { discriminant, .. } => {
            assert_eq!(*discriminant, 3, "Despawn's real discriminant");
        }
        other => panic!(
            "expected Despawn to downgrade to Unknown through the frozen `action` field, got {other:?}"
        ),
    }
}

// regression test: a union variant payload's `(min:/max:)` constraint used to
// be silently ignored on encode — no error, out-of-range bytes written
// anyway — for both a bare constrained scalar payload (`Bounded`) and a named
// struct payload that's itself constraint-infected (`BoundedStruct`).
#[test]
fn test_encode_rejects_out_of_range_union_payload() {
    let e = Edge {
        action: Payload::Bounded(999),
        ..Default::default()
    };
    let mut buf = Vec::new();
    let err = encode(&mut buf, &e).expect_err("out-of-range union payload must be rejected");
    assert!(matches!(err, Error::ConstraintViolation { .. }));

    let e = Edge {
        action: Payload::BoundedStruct(BoundedPayload { value: 999 }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    let err = encode(&mut buf, &e).expect_err("out-of-range struct union payload must be rejected");
    assert!(matches!(err, Error::ConstraintViolation { .. }));
}

#[test]
fn test_roundtrip_control_signal_variants() {
    let variants = vec![
        ControlSignal::Ping(PingPayload {}),
        ControlSignal::Pong(PongPayload { latency_ms: 250 }),
        ControlSignal::Disconnect(DisconnectPayload { reason_code: 7 }),
    ];

    for variant in variants {
        let e = Edge {
            control: variant,
            ..Default::default()
        };
        let mut buf = Vec::new();
        encode(&mut buf, &e).expect("encode failed");
        let decoded = decode(&buf).expect("decode failed");
        assert_control_signal_eq(&e.control, &decoded.control);
    }
}

#[test]
fn test_roundtrip_unknown_union_variant_is_lossless() {
    // simulates a peer on a newer schema sending an unknown variant; must
    // round-trip as Unknown, not error. 200 fits Payload's u8 wire width.
    let e = Edge {
        action: Payload::Unknown {
            discriminant: 200,
            data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        },
        ..Default::default()
    };

    let mut buf = Vec::new();
    encode(&mut buf, &e).expect("encode failed");
    let decoded = decode(&buf).expect("decode failed");

    match &decoded.action {
        Payload::Unknown { discriminant, data } => {
            assert_eq!(*discriminant, 200);
            assert_eq!(data, &vec![0xDE, 0xAD, 0xBE, 0xEF]);
        }
        other => panic!("expected Unknown variant to survive roundtrip, got {other:?}"),
    }
}

#[test]
fn test_edge_schema_cases_across_union_width_change() {
    use pojoc_tests::{pluck_edge as pluck, pojoc_edge as schema};
    use union_width::Big;

    let mut original = make_populated_edge();
    for version in [7, 8] {
        let mut data = Vec::new();
        schema::encode_for_version(&mut data, &original, version).unwrap();
        let decoded = schema::decode(&data).unwrap();
        assert_eq!(
            pluck!(
                schema,
                &data,
                transform.position.x,
                transform.position.label,
                transform.position.prefix
            )
            .unwrap(),
            (0x1234_5678, "shared-tag", Some("point"))
        );
        assert_eq!(
            decoded.transform.description,
            original.transform.description
        );
        assert_eq!(decoded.transform.tail, original.transform.tail);
        let tree = decoded.tree.as_ref().unwrap();
        assert_eq!(tree.value, 1);
        assert_eq!(tree.left.as_ref().unwrap().value, 2);
        assert_eq!(tree.right.as_ref().unwrap().value, 3);
        let ping = decoded.ping.as_ref().unwrap();
        let pong = ping.other.as_ref().unwrap();
        let next_ping = pong.other.as_ref().unwrap();
        assert_eq!((ping.count, pong.count, next_ping.count), (10, 20, 30));
        assert_eq!(decoded.interned_generic.value, decoded.interned_label);
        assert_eq!(decoded.packed_varints, original.packed_varints);
        assert!(matches!(
            (version, decoded.union_width_probe.unwrap().x),
            (7, Big::V1(42)) | (8, Big::W259(99))
        ));
        original.union_width_probe.as_mut().unwrap().x = Big::W259(99);
    }
    assert_eq!(
        schema::encode_for_version(&mut Vec::new(), &original, 7),
        Err(Error::DiscriminantOverflow)
    );

    let size = schema::size_hint_v8(&original);
    original.packed_varints.clear();
    assert_eq!(size - schema::size_hint_v8(&original), 21);
}

#[test]
fn interned_envelope_uses_the_staged_payload_length() {
    use pojoc_tests::pojoc_intern_bench as schema;

    let long = "å".repeat(75);
    let value = schema::InternBench {
        tags: std::iter::repeat_n(long.as_str(), 2_000).collect(),
        ..Default::default()
    };

    let mut payload = Vec::new();
    schema::encode_v1(&mut payload, &value);
    assert_ne!(schema::size_hint_v1(&value), payload.len());

    let mut expected = vec![0xaa];
    expected.push(1);
    expected.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    expected.extend_from_slice(&payload);

    let mut current = vec![0xaa];
    schema::encode(&mut current, &value);
    assert_eq!(current, expected);

    let mut versioned = vec![0xaa];
    schema::encode_for_version(&mut versioned, &value, 1).unwrap();
    assert_eq!(versioned, expected);
    assert_eq!(schema::decode(&versioned[1..]).unwrap().tags, value.tags);
}

#[test]
fn test_raw_passthrough_is_byte_identical() {
    let original = make_populated_edge();
    let mut buf = Vec::new();
    encode(&mut buf, &original).expect("encode failed");

    let decoded = decode(&buf).expect("decode failed");
    let mut reencoded = Vec::new();
    encode(&mut reencoded, &decoded).expect("encode failed");

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
    encode(&mut buf, &e).expect("encode failed");
    let decoded = decode_static(&buf);

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
