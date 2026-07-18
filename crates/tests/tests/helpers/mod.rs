use pojoc_tests::pojoc_edge::{runtime::*, *};

pub fn make_version_probe_edge() -> Edge<'static> {
    let mut e = Edge::default();
    e.u8_to_i64 = 100;
    e.i64_to_f32 = -987.65;
    e.bounds_enum = NumericBounds::ExtraVariant;
    e.system_perms = SystemPrivileges::ROOT | SystemPrivileges::NETWORK_ACCESS;
    e.nullified_str = Some("VersionTest");
    e.empty_arr.push("v");
    e.root_struct.leaf.leaf_val = "leaf";
    e.root_struct.leaf.leaf_numeric = 1;
    e.updated_imported_player.player_id = 12345.0;
    e.updated_imported_player.status = player::Status::Spectating;

    // Generics — monomorphized instantiations and the Mono/Duo/Mono
    // type-parameter-evolution chain.
    e.generic_box.value = 7;
    e.generic_box.label = "probe";
    e.generic_pair.first = 1;
    e.generic_pair.second = "probe-pair";
    e.generic_triple.first = 2;
    e.generic_triple.second = "probe-triple";
    e.generic_triple.third = true;
    e.generic_flag_box.value = true;
    e.generic_mono_v3.value = "probe-mono-v3";
    e.generic_duo_v4.value = "probe-duo-v4";
    e.generic_duo_v4.secondary = Some(9);
    e.generic_mono_v5.value = "probe-mono-v5";

    e
}

pub fn make_populated_edge() -> Edge<'static> {
    let mut e = Edge::default();

    // Primitive and float scalars
    e.u8_to_i64 = 100;
    e.i64_to_f32 = -987.65;
    e.i64_min = i64::MIN;
    e.varint32_max = u32::MAX;
    e.varint64_min = u64::MIN;
    e.f32_nan = f32::NAN;
    e.f32_inf = f32::INFINITY;
    e.f64_neg_inf = f64::NEG_INFINITY;

    // Strings
    e.nullified_str = Some("PojocSerialization");
    e.spaces_str = "    ";
    e.fixed_str_min = [10, 20, 30, 40, 50, 60, 70, 80];

    // Collections
    e.empty_arr.push("FirstElement");
    e.empty_arr.push("SecondElement");
    e.array_to_map.insert(5, 25);
    e.lazy_audit_log = LazyView::Owned(Some(pojvec!("AuditEntryOne", "AuditEntryTwo")));

    // Delta sequences
    e.delta_positions.push(1000);
    e.delta_positions.push(1010);
    e.delta_i64_seq.push(-50);
    e.delta_i64_seq.push(-45);
    e.lazy_delta_log = LazyView::Owned(Some(pojvec!(100, 150, 120)));

    // Nested struct
    e.root_struct.leaf.leaf_val = "LeafNode";
    e.root_struct.leaf.leaf_numeric = 777;
    e.root_struct.weight = 3.14;
    e.root_struct.leaf_arr.push(NestedLeaf {
        leaf_val: "ArrayLeaf",
        leaf_numeric: 11,
        leaf_rotation: 0f32,
    });
    e.root_struct.leaf_opt = Some(NestedLeaf {
        leaf_val: "OptionalLeaf",
        leaf_numeric: 22,
        leaf_rotation: 180f32,
    });

    // Generics — three distinct monomorphizations of the same struct-shape
    // machinery (Box<i32>, Pair<i32, string>, Triple<i32, string, bool>),
    // an explicitly-named instantiation (`Box<bool> as FlagBox`), plus the
    // Mono<A> -> Duo<A, B> -> Mono<A> type-parameter-evolution chain (v3/v4/v5).
    e.generic_box.value = 42;
    e.generic_box.label = "meaning-of-life";
    e.generic_pair.first = 3;
    e.generic_pair.second = "third";
    e.generic_triple.first = -8;
    e.generic_triple.second = "triple";
    e.generic_triple.third = false;
    e.generic_flag_box.value = false;
    e.generic_mono_v3.value = "mono-v3";
    e.generic_duo_v4.value = "duo-v4";
    e.generic_duo_v4.secondary = Some(-17);
    e.generic_mono_v5.value = "mono-v5";

    // Enum and fixed arrays
    e.bounds_enum = NumericBounds::ExtraVariant;
    e.newly_added_optional = 999999;
    e.u32_delta_seq = [500; 16];

    // Maps
    e.basic_map.insert("ConfigKey", "ConfigValue");
    e.fixed_map_populated = pojmap!("FixedMapKey1" => 5, "FixedMapKey2" => 30; 2);
    e.delta_value_map = pojmap!("DeltaMapKey1" => pojvec![10], "DeltaMapKey2" => pojvec![20]; 2);

    // Sensor log
    let mut frame = SensorFrame::default();
    frame.readings.push(55);
    frame.readings.push(60);
    frame.sample_ids = [100, 200, 300, 400, 500, 600, 700, 800];
    e.sensor_log.push(frame);

    // Optional primitives
    e.opt_u8 = Some(255);
    e.opt_i16 = Some(-32768);
    e.opt_u32 = Some(400000);
    e.opt_i64 = Some(-900000);
    e.opt_f32 = Some(12.34);
    e.opt_f64 = Some(56.78);
    e.opt_bool = Some(false);
    e.opt_fixed_str = Some([0xAA, 0xBB, 0xCC, 0xDD]);
    e.opt_bitset = Some(SystemPrivileges::READ | SystemPrivileges::WRITE);

    // Bitset and flags
    e.system_perms =
        SystemPrivileges::ROOT | SystemPrivileges::NETWORK_ACCESS | SystemPrivileges::EXECUTE;
    e.legacy_hw_flags = 0xFFFFFFFF;

    // Optional delta array
    let mut delta_arr = PojocVec::new();
    delta_arr.push(42);
    e.opt_delta_arr = Some(delta_arr);

    // Deep nested structure
    e.ultimate_boss_structure.frame_deltas.push(8888);

    // Tagged unions — scalar, array, optional, and map-value positions,
    // spread across the variants each union has accumulated over its history.
    e.action = Payload::Attack(AttackPayload {
        target_id: 42,
        damage: 17.5,
        knockback: 2.6,
    });
    e.action_log
        .push(Payload::Move(MovePayload { dx: 3, dy: -3 }));
    e.action_log.push(Payload::Heal(HealPayload {
        target_id: 7,
        amount: 25.0,
        overheal: true,
        splash_radius: 5.2,
    }));
    e.action_log
        .push(Payload::Despawn(DespawnPayload { entity_id: 900 }));
    e.deferred_action = Some(Payload::Heal(HealPayload {
        target_id: 1,
        amount: 10.0,
        overheal: false,
        splash_radius: 1.5,
    }));
    e.final_action = Payload::Despawn(DespawnPayload { entity_id: 12345 });

    e.control = ControlSignal::Pong(PongPayload { latency_ms: 42 });
    e.control_log.push(ControlSignal::Ping(PingPayload {}));
    e.control_log
        .push(ControlSignal::Disconnect(DisconnectPayload {
            reason_code: 4,
        }));
    e.control_map.insert(
        "primary",
        Payload::Attack(AttackPayload {
            target_id: 5,
            damage: 99.9,
            knockback: 1.2,
        }),
    );

    e.updated_imported_player = make_player_value();

    e
}

fn make_player_value() -> player::Player<'static> {
    let mut p = player::Player::default();

    p.player_id = 42.0;
    p.level = 17.5;
    p.status = player::Status::Spectating;
    p.class = player::Class::Necromancer;
    p.region = player::Region::Void;

    p.inventory.push("Sword");
    p.inventory.push("Shield");
    p.callsign = "Ghost";

    p.stats = player::Stats {
        strength: 10,
        agility: 12,
        intelligence: 8,
        endurance: 15,
        charisma: 6,
        resistance: 3.5,
    };

    p.hotbar = std::array::from_fn(|_| "");
    p.hotbar[0] = "sword";
    p.hotbar[1] = "shield";

    p.session_token = *b"PLAYERTOKEN12345";
    p.coordinates = (1.5, 2.5);
    p.position = player::Vector3 {
        x: 1.0,
        y: 2.0,
        z: 3.0,
        w: 1.0,
    };
    p.kill_death = (5, 2);

    p.tags.push("vip");

    p.transform = player::Transform {
        position: player::Vector3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        bounds: player::AABB {
            min_x: -1.0,
            min_y: -1.0,
            max_x: 1.0,
            max_y: 1.0,
        },
    };

    p.recent_zones = std::array::from_fn(|_| "");
    p.recent_zones[0] = "zoneA";

    p.velocity = (0.1, 0.2, 0.3);
    p.status_code = *b"OK000000";
    p.is_nauseous = false;
    p.guild_tag = *b"WLF\0";
    p.spawn_point = (10.0, 0.0, 10.0);

    p.achievement_ids.push(101);
    p.achievement_ids.push(202);

    p.active_perks = player::Perks::DOUBLE_JUMP | player::Perks::TELEKINESIS;
    p.account_flags = player::Flags::IS_VERIFIED | player::Flags::IS_DEVELOPER;

    p.loadout = std::array::from_fn(|_| ("", 0i32));
    p.loadout[0] = ("sword", 1);
    p.loadout[1] = ("shield", 1);

    p.party_members = [1, 2, 3, 4];
    p.last_position = (5.0, 5.0, 5.0);

    p
}

pub fn assert_nested_leaf_eq(a: &NestedLeaf, b: &NestedLeaf) {
    assert_eq!(a.leaf_val, b.leaf_val);
    assert_eq!(a.leaf_numeric, b.leaf_numeric);
}

pub fn assert_middle_layer_eq(a: &MiddleLayer, b: &MiddleLayer) {
    assert_nested_leaf_eq(&a.leaf, &b.leaf);
    assert_eq!(a.leaf_arr.len(), b.leaf_arr.len());
    for (i, j) in a.leaf_arr.iter().zip(b.leaf_arr.iter()) {
        assert_nested_leaf_eq(i, j);
    }
    match (&a.leaf_opt, &b.leaf_opt) {
        (Some(x), Some(y)) => assert_nested_leaf_eq(x, y),
        (None, None) => {}
        _ => panic!("Mismatch in optional presence for field 'leaf_opt'"),
    }
    assert_eq!(a.weight, b.weight);
    match (&a.secondary_leaf, &b.secondary_leaf) {
        (Some(x), Some(y)) => assert_nested_leaf_eq(x, y),
        (None, None) => {}
        _ => panic!("Mismatch in optional presence for field 'secondary_leaf'"),
    }
}

pub fn assert_box_i32_eq(a: &BoxI32, b: &BoxI32) {
    assert_eq!(a.value, b.value);
    assert_eq!(a.label, b.label);
}

pub fn assert_pair_i32_string_eq(a: &PairI32String, b: &PairI32String) {
    assert_eq!(a.first, b.first);
    assert_eq!(a.second, b.second);
}

pub fn assert_triple_i32_string_bool_eq(a: &TripleI32StringBool, b: &TripleI32StringBool) {
    assert_eq!(a.first, b.first);
    assert_eq!(a.second, b.second);
    assert_eq!(a.third, b.third);
}

pub fn assert_flag_box_eq(a: &FlagBox, b: &FlagBox) {
    assert_eq!(a.value, b.value);
}

pub fn assert_mono_string_eq(a: &MonoString, b: &MonoString) {
    assert_eq!(a.value, b.value);
}

pub fn assert_duo_string_i32_eq(a: &DuoStringI32, b: &DuoStringI32) {
    assert_eq!(a.value, b.value);
    assert_eq!(a.secondary, b.secondary);
}

pub fn assert_sensor_frame_eq(a: &SensorFrame, b: &SensorFrame) {
    assert_eq!(a.readings, b.readings);
    assert_eq!(a.sample_ids, b.sample_ids);
}

pub fn assert_deep_complex_wrapper_eq<'buf>(
    a: &DeepComplexWrapper<'buf>,
    b: &DeepComplexWrapper<'buf>,
) {
    assert_eq!(a.frame_deltas, b.frame_deltas);
    assert_eq!(a.matrix, b.matrix);
}

// Unions don't derive PartialEq (their payload structs don't either, same as
// every other generated struct in this codebase), so comparison is by
// explicit variant match, mirroring assert_nested_leaf_eq's manual style.
pub fn assert_payload_eq(a: &Payload, b: &Payload) {
    match (a, b) {
        (Payload::Move(x), Payload::Move(y)) => {
            assert_eq!(x.dx, y.dx);
            assert_eq!(x.dy, y.dy);
        }
        (Payload::Attack(x), Payload::Attack(y)) => {
            assert_eq!(x.target_id, y.target_id);
            assert_eq!(x.damage, y.damage);
        }
        (Payload::Heal(x), Payload::Heal(y)) => {
            assert_eq!(x.target_id, y.target_id);
            assert_eq!(x.amount, y.amount);
            assert_eq!(x.overheal, y.overheal);
        }
        (Payload::Despawn(x), Payload::Despawn(y)) => {
            assert_eq!(x.entity_id, y.entity_id);
        }
        (
            Payload::Unknown {
                discriminant: d1,
                data: dt1,
            },
            Payload::Unknown {
                discriminant: d2,
                data: dt2,
            },
        ) => {
            assert_eq!(d1, d2);
            assert_eq!(dt1, dt2);
        }
        _ => panic!("Payload variant mismatch: {a:?} vs {b:?}"),
    }
}

pub fn assert_control_signal_eq(a: &ControlSignal, b: &ControlSignal) {
    match (a, b) {
        (ControlSignal::Ping(_), ControlSignal::Ping(_)) => {}
        (ControlSignal::Pong(x), ControlSignal::Pong(y)) => {
            assert_eq!(x.latency_ms, y.latency_ms);
        }
        (ControlSignal::Disconnect(x), ControlSignal::Disconnect(y)) => {
            assert_eq!(x.reason_code, y.reason_code);
        }
        (
            ControlSignal::Unknown {
                discriminant: d1,
                data: dt1,
            },
            ControlSignal::Unknown {
                discriminant: d2,
                data: dt2,
            },
        ) => {
            assert_eq!(d1, d2);
            assert_eq!(dt1, dt2);
        }
        _ => panic!("ControlSignal variant mismatch: {a:?} vs {b:?}"),
    }
}

pub fn assert_edge_eq<'buf>(a: &Edge<'buf>, b: &Edge<'buf>) {
    // Scalar integer and varint conversions
    assert_eq!(a.u8_to_i64, b.u8_to_i64);
    assert_eq!(a.i64_to_f32, b.i64_to_f32);
    assert_eq!(a.i64_min, b.i64_min);
    assert_eq!(a.varint32_max, b.varint32_max);
    assert_eq!(a.varint64_min, b.varint64_min);

    // Floating point special values
    if a.f32_nan.is_nan() {
        assert!(b.f32_nan.is_nan(), "Expected f32_nan to be NaN");
    } else {
        assert_eq!(a.f32_nan, b.f32_nan);
    }
    assert_eq!(a.f32_inf, b.f32_inf);
    assert_eq!(a.f64_neg_inf, b.f64_neg_inf);

    // Strings & fixed-length
    assert_eq!(a.nullified_str, b.nullified_str);
    assert_eq!(a.spaces_str, b.spaces_str);
    assert_eq!(a.fixed_str_min, b.fixed_str_min);
    assert_eq!(a.empty_arr, b.empty_arr);
    assert_eq!(a.fixed_arr_empty, b.fixed_arr_empty);
    assert_eq!(a.array_to_map, b.array_to_map);

    // Delta sequences
    assert_eq!(a.delta_positions, b.delta_positions);
    assert_eq!(a.delta_i64_seq, b.delta_i64_seq);
    assert_eq!(a.delta_u64_seq, b.delta_u64_seq);
    assert_eq!(a.delta_u16_seq, b.delta_u16_seq);
    assert_eq!(a.delta_i8_seq, b.delta_i8_seq);
    assert_eq!(a.delta_single, b.delta_single);
    assert_eq!(a.delta_fixed_u8, b.delta_fixed_u8);
    assert_eq!(a.delta_fixed_empty, b.delta_fixed_empty);
    assert_eq!(a.legacy_positions, b.legacy_positions);

    // Nested structs
    assert_middle_layer_eq(&a.root_struct, &b.root_struct);
    assert_eq!(a.bounds_enum, b.bounds_enum);
    assert_eq!(a.newly_added_optional, b.newly_added_optional);
    assert_eq!(a.u32_delta_seq, b.u32_delta_seq);

    // Generics
    assert_box_i32_eq(&a.generic_box, &b.generic_box);
    assert_pair_i32_string_eq(&a.generic_pair, &b.generic_pair);
    assert_triple_i32_string_bool_eq(&a.generic_triple, &b.generic_triple);
    assert_flag_box_eq(&a.generic_flag_box, &b.generic_flag_box);
    assert_mono_string_eq(&a.generic_mono_v3, &b.generic_mono_v3);
    assert_duo_string_i32_eq(&a.generic_duo_v4, &b.generic_duo_v4);
    assert_mono_string_eq(&a.generic_mono_v5, &b.generic_mono_v5);

    // Maps
    assert_eq!(a.basic_map, b.basic_map);
    assert_eq!(a.fixed_map_empty, b.fixed_map_empty);
    assert_eq!(a.fixed_map_populated, b.fixed_map_populated);
    assert_eq!(a.nested_map, b.nested_map);

    // Sensor log
    assert_eq!(a.sensor_log.len(), b.sensor_log.len());
    for (i, j) in a.sensor_log.iter().zip(b.sensor_log.iter()) {
        assert_sensor_frame_eq(i, j);
    }
    assert_eq!(a.delta_value_map, b.delta_value_map);

    // Optional primitives
    assert_eq!(a.opt_u8, b.opt_u8);
    assert_eq!(a.opt_i16, b.opt_i16);
    assert_eq!(a.opt_u32, b.opt_u32);
    assert_eq!(a.opt_i64, b.opt_i64);
    match (a.opt_f32, b.opt_f32) {
        (Some(x), Some(y)) => assert_eq!(x, y),
        (None, None) => {}
        _ => panic!("Mismatch in optional presence for field 'opt_f32'"),
    }
    match (a.opt_f64, b.opt_f64) {
        (Some(x), Some(y)) => assert_eq!(x, y),
        (None, None) => {}
        _ => panic!("Mismatch in optional presence for field 'opt_f64'"),
    }
    assert_eq!(a.opt_bool, b.opt_bool);
    assert_eq!(a.opt_fixed_str, b.opt_fixed_str);

    // Bitsets and flags
    assert_eq!(a.opt_bitset, b.opt_bitset);
    assert_eq!(a.system_perms, b.system_perms);
    assert_eq!(a.legacy_hw_flags, b.legacy_hw_flags);
    assert_eq!(a.opt_delta_arr, b.opt_delta_arr);

    // Deep wrapper
    assert_deep_complex_wrapper_eq(&a.ultimate_boss_structure, &b.ultimate_boss_structure);

    // Imports
    assert_player_eq(&a.updated_imported_player, &b.updated_imported_player);

    // Tagged unions
    assert_payload_eq(&a.action, &b.action);
    assert_eq!(a.action_log.len(), b.action_log.len());
    for (i, j) in a.action_log.iter().zip(b.action_log.iter()) {
        assert_payload_eq(i, j);
    }
    match (&a.deferred_action, &b.deferred_action) {
        (Some(x), Some(y)) => assert_payload_eq(x, y),
        (None, None) => {}
        _ => panic!("Mismatch in optional presence for field 'deferred_action'"),
    }
    assert_payload_eq(&a.final_action, &b.final_action);

    assert_control_signal_eq(&a.control, &b.control);
    assert_eq!(a.control_log.len(), b.control_log.len());
    for (i, j) in a.control_log.iter().zip(b.control_log.iter()) {
        assert_control_signal_eq(i, j);
    }
    assert_eq!(a.control_map.len(), b.control_map.len());
    for (k, v) in a.control_map.iter() {
        let other = b
            .control_map
            .get(k)
            .expect("control_map key missing after roundtrip");
        assert_payload_eq(v, other);
    }

    let a_audit = a
        .lazy_audit_log
        .clone()
        .get()
        .expect("a.lazy_audit_log decode failed");
    let b_audit = b
        .lazy_audit_log
        .clone()
        .get()
        .expect("b.lazy_audit_log decode failed");
    assert_eq!(a_audit, b_audit, "lazy_audit_log mismatch");

    let a_delta = a
        .lazy_delta_log
        .clone()
        .get()
        .expect("a.lazy_delta_log decode failed");
    let b_delta = b
        .lazy_delta_log
        .clone()
        .get()
        .expect("b.lazy_delta_log decode failed");
    assert_eq!(a_delta, b_delta, "lazy_delta_log mismatch");
}

pub fn assert_vector3_eq(a: &player::Vector3, b: &player::Vector3) {
    assert_eq!(a.x, b.x);
    assert_eq!(a.y, b.y);
    assert_eq!(a.z, b.z);
    assert_eq!(a.w, b.w);
}

pub fn assert_aabb_eq(a: &player::AABB, b: &player::AABB) {
    assert_eq!(a.min_x, b.min_x);
    assert_eq!(a.min_y, b.min_y);
    assert_eq!(a.max_x, b.max_x);
    assert_eq!(a.max_y, b.max_y);
}

pub fn assert_transform_eq(a: &player::Transform, b: &player::Transform) {
    assert_vector3_eq(&a.position, &b.position);
    assert_aabb_eq(&a.bounds, &b.bounds);
}

pub fn assert_stats_eq(a: &player::Stats, b: &player::Stats) {
    assert_eq!(a.strength, b.strength);
    assert_eq!(a.agility, b.agility);
    assert_eq!(a.intelligence, b.intelligence);
    assert_eq!(a.endurance, b.endurance);
    assert_eq!(a.charisma, b.charisma);
    assert_eq!(a.resistance, b.resistance); // luck was removed in Stats@6
}

pub fn assert_player_eq(a: &player::Player, b: &player::Player) {
    assert_eq!(a.player_id, b.player_id);
    assert_eq!(a.level, b.level);
    assert_eq!(a.status, b.status);
    assert_eq!(a.class, b.class);
    assert_eq!(a.region, b.region);
    assert_eq!(a.inventory, b.inventory);
    assert_eq!(a.callsign, b.callsign);
    assert_stats_eq(&a.stats, &b.stats);
    assert_eq!(a.hotbar, b.hotbar);
    assert_eq!(a.session_token, b.session_token);
    assert_eq!(a.coordinates, b.coordinates);
    assert_vector3_eq(&a.position, &b.position);
    assert_eq!(a.kill_death, b.kill_death);
    assert_eq!(a.tags, b.tags);
    assert_transform_eq(&a.transform, &b.transform);
    assert_eq!(a.recent_zones, b.recent_zones);
    assert_eq!(a.velocity, b.velocity);
    assert_eq!(a.status_code, b.status_code);
    assert_eq!(a.is_nauseous, b.is_nauseous);
    assert_eq!(a.guild_tag, b.guild_tag);
    assert_eq!(a.spawn_point, b.spawn_point);
    assert_eq!(a.achievement_ids, b.achievement_ids);
    assert_eq!(a.active_perks, b.active_perks);
    assert_eq!(a.account_flags, b.account_flags);
    assert_eq!(a.loadout, b.loadout);
    assert_eq!(a.party_members, b.party_members);
    assert_eq!(a.last_position, b.last_position);
}
