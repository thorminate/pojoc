use pojoc_tests::pojoc_edge::{runtime::*, *};


pub fn make_version_probe_edge() -> Edge<'static> {
    let mut e = Edge::default();
    e.u8_to_i64 = 100;
    e.i64_to_f32 = -987.65;
    e.bounds_enum = NumericBounds::ExtraVariant;
    e.system_perms = SystemPrivileges::ROOT | SystemPrivileges::NETWORK_ACCESS;
    e.nullified_str = Some("VersionTest".into());
    e.empty_arr.push(pojstr!("v"));
    e.root_struct.leaf.leaf_val = "leaf".into();
    e.root_struct.leaf.leaf_numeric = 1;
    e.updated_imported_player.player_id = 12345.0;
    e.updated_imported_player.status = player::Status::Spectating;
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
    e.nullified_str = Some("PojocSerialization".into());
    e.spaces_str = "    ".into();
    e.fixed_str_min = [10, 20, 30, 40, 50, 60, 70, 80];

    // Collections
    e.empty_arr.push(pojstr!("FirstElement"));
    e.empty_arr.push(pojstr!("SecondElement"));
    e.array_to_map.insert(5, 25);

    // Delta sequences
    e.delta_positions.push(1000);
    e.delta_positions.push(1010);
    e.delta_i64_seq.push(-50);
    e.delta_i64_seq.push(-45);

    // Nested struct
    e.root_struct.leaf.leaf_val = "LeafNode".into();
    e.root_struct.leaf.leaf_numeric = 777;
    e.root_struct.weight = 3.14;
    e.root_struct.leaf_arr.push(NestedLeaf { leaf_val: "ArrayLeaf".into(), leaf_numeric: 11, leaf_rotation: 0f32 });
    e.root_struct.leaf_opt = Some(NestedLeaf { leaf_val: "OptionalLeaf".into(), leaf_numeric: 22, leaf_rotation: 180f32 });

    // Enum and fixed arrays
    e.bounds_enum = NumericBounds::ExtraVariant;
    e.newly_added_optional = 999999;
    e.u32_delta_seq = [500; 16];

    // Maps
    e.basic_map.insert("ConfigKey".into(), "ConfigValue".into());
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
    e.system_perms = SystemPrivileges::ROOT | SystemPrivileges::NETWORK_ACCESS | SystemPrivileges::EXECUTE;
    e.legacy_hw_flags = 0xFFFFFFFF;

    // Optional delta array
    let mut delta_arr = PojocVec::new();
    delta_arr.push(42);
    e.opt_delta_arr = Some(delta_arr);

    // Deep nested structure
    e.ultimate_boss_structure.frame_deltas.push(8888);

    // Tagged unions — scalar, array, optional, and map-value positions,
    // spread across the variants each union has accumulated over its history.
    e.action = Payload::Attack(AttackPayload { target_id: 42, damage: 17.5, knockback: 2.6 });
    e.action_log.push(Payload::Move(MovePayload { dx: 3, dy: -3 }));
    e.action_log.push(Payload::Heal(HealPayload { target_id: 7, amount: 25.0, overheal: true, splash_radius: 5.2 }));
    e.action_log.push(Payload::Despawn(DespawnPayload { entity_id: 900 }));
    e.deferred_action = Some(Payload::Heal(HealPayload { target_id: 1, amount: 10.0, overheal: false, splash_radius: 1.5 }));
    e.final_action = Payload::Despawn(DespawnPayload { entity_id: 12345 });

    e.control = ControlSignal::Pong(PongPayload { latency_ms: 42 });
    e.control_log.push(ControlSignal::Ping(PingPayload {}));
    e.control_log.push(ControlSignal::Disconnect(DisconnectPayload { reason_code: 4 }));
    e.control_map.insert("primary".into(), Payload::Attack(AttackPayload { target_id: 5, damage: 99.9, knockback: 1.2 }));

    e.updated_imported_player = make_player_value();

    e
}

fn make_player_value() -> player::Player {
    let mut p = player::Player::default();

    p.player_id = 42.0;
    p.level = 17.5;
    p.status = player::Status::Spectating;
    p.class = player::Class::Necromancer;
    p.region = player::Region::Void;

    p.inventory.push(pojstr!("Sword"));
    p.inventory.push(pojstr!("Shield"));
    p.callsign = "Ghost".into();

    p.stats = player::Stats {
        strength: 10,
        agility: 12,
        intelligence: 8,
        endurance: 15,
        charisma: 6,
        resistance: 3.5,
    };

    p.hotbar = std::array::from_fn(|_| PojocString::default());
    p.hotbar[0] = pojstr!("sword");
    p.hotbar[1] = pojstr!("shield");

    p.session_token = *b"PLAYERTOKEN12345";
    p.coordinates = (1.5, 2.5);
    p.position = player::Vector3 { x: 1.0, y: 2.0, z: 3.0, w: 1.0 };
    p.kill_death = (5, 2);

    p.tags.push(pojstr!("vip"));

    p.transform = player::Transform {
        position: player::Vector3 { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
        bounds: player::AABB { min_x: -1.0, min_y: -1.0, max_x: 1.0, max_y: 1.0 },
    };

    p.recent_zones = std::array::from_fn(|_| PojocString::default());
    p.recent_zones[0] = pojstr!("zoneA");

    p.chat_history = std::array::from_fn(|_| PojocString::default());
    p.chat_history[0] = pojstr!("hello");

    p.velocity = (0.1, 0.2, 0.3);
    p.status_code = *b"OK000000";
    p.is_nauseous = false;
    p.guild_tag = *b"WLF\0";
    p.spawn_point = (10.0, 0.0, 10.0);

    p.achievement_ids.push(101);
    p.achievement_ids.push(202);

    p.active_perks = player::Perks::DOUBLE_JUMP | player::Perks::TELEKINESIS;
    p.account_flags = player::Flags::IS_VERIFIED | player::Flags::IS_DEVELOPER;

    p.quest_progress.insert("main_quest".into(), 3);
    p.quick_slots = pojmap!(0 => "potion", 1 => "scroll", 2 => "wow", 3 => "wow", 4 => "wow", 5 => "wow", 6 => "wow", 7 => "wow", 8 => "wow", 9 => "wow"; 10);
    p.skill_levels.insert("archery".into(), 7.5);

    p.loadout = std::array::from_fn(|_| (PojocString::default(), 0i32));
    p.loadout[0] = (pojstr!("sword"), 1);
    p.loadout[1] = (pojstr!("shield"), 1);

    p.leaderboard_scores.insert("season1".into(), 9001);
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

pub fn assert_sensor_frame_eq(a: &SensorFrame, b: &SensorFrame) {
    assert_eq!(a.readings, b.readings);
    assert_eq!(a.sample_ids, b.sample_ids);
}

pub fn assert_deep_complex_wrapper_eq(a: &DeepComplexWrapper, b: &DeepComplexWrapper) {
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
        (Payload::Unknown { discriminant: d1, data: dt1 }, Payload::Unknown { discriminant: d2, data: dt2 }) => {
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
        (ControlSignal::Unknown { discriminant: d1, data: dt1 }, ControlSignal::Unknown { discriminant: d2, data: dt2 }) => {
            assert_eq!(d1, d2);
            assert_eq!(dt1, dt2);
        }
        _ => panic!("ControlSignal variant mismatch: {a:?} vs {b:?}"),
    }
}

pub fn assert_edge_eq(a: &Edge, b: &Edge) {
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
        let other = b.control_map.get(k).expect("control_map key missing after roundtrip");
        assert_payload_eq(v, other);
    }
    
    
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
    assert_eq!(a.resistance, b.resistance);   // luck was removed in Stats@6
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
    assert_eq!(a.chat_history, b.chat_history);
    assert_eq!(a.velocity, b.velocity);
    assert_eq!(a.status_code, b.status_code);
    assert_eq!(a.is_nauseous, b.is_nauseous);
    assert_eq!(a.guild_tag, b.guild_tag);
    assert_eq!(a.spawn_point, b.spawn_point);
    assert_eq!(a.achievement_ids, b.achievement_ids);
    assert_eq!(a.active_perks, b.active_perks);
    assert_eq!(a.account_flags, b.account_flags);
    assert_eq!(a.quest_progress, b.quest_progress);
    assert_eq!(a.quick_slots, b.quick_slots);
    assert_eq!(a.skill_levels, b.skill_levels);
    assert_eq!(a.loadout, b.loadout);
    assert_eq!(a.leaderboard_scores, b.leaderboard_scores);
    assert_eq!(a.party_members, b.party_members);
    assert_eq!(a.last_position, b.last_position);
}