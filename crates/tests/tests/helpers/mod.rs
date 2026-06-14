use pojoc_tests::pojoc_edge::*;

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

pub fn assert_edge_eq(a: &Edge, b: &Edge) {
    // Scalar integer and varint conversions
    assert_eq!(a.u8_to_i64, b.u8_to_i64);
    assert_eq!(a.i64_to_f32, b.i64_to_f32);
    assert_eq!(a.i64_min, b.i64_min);
    assert_eq!(a.varint32_max, b.varint32_max);
    assert_eq!(a.varint64_min, b.varint64_min);

    // Floating point bounds and special values
    if a.f32_nan.is_nan() {
        assert!(b.f32_nan.is_nan(), "Expected f32_nan to be NaN");
    } else {
        assert_eq!(a.f32_nan, b.f32_nan);
    }
    assert_eq!(a.f32_inf, b.f32_inf);
    assert_eq!(a.f64_neg_inf, b.f64_neg_inf);

    // Strings & arrays
    assert_eq!(a.nullified_str, b.nullified_str);
    assert_eq!(a.spaces_str, b.spaces_str);
    assert_eq!(a.fixed_str_min, b.fixed_str_min);
    assert_eq!(a.empty_arr, b.empty_arr);
    assert_eq!(a.fixed_arr_empty, b.fixed_arr_empty);
    assert_eq!(a.array_to_map, b.array_to_map);

    // Delta sequence collections
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

    // Maps & Nested Maps
    assert_eq!(a.basic_map, b.basic_map);
    assert_eq!(a.fixed_map_empty, b.fixed_map_empty);
    assert_eq!(a.fixed_map_populated, b.fixed_map_populated);
    assert_eq!(a.nested_map, b.nested_map);

    // Vector of Structs
    assert_eq!(a.sensor_log.len(), b.sensor_log.len());
    for (i, j) in a.sensor_log.iter().zip(b.sensor_log.iter()) {
        assert_sensor_frame_eq(i, j);
    }
    assert_eq!(a.delta_value_map, b.delta_value_map);

    // Primitive Optional Fields
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

    // Bitset fields
    assert_eq!(a.opt_bitset, b.opt_bitset);
    assert_eq!(a.system_perms, b.system_perms);
    assert_eq!(a.legacy_hw_flags, b.legacy_hw_flags);
    assert_eq!(a.opt_delta_arr, b.opt_delta_arr);

    // Top-level deep wrapper
    assert_deep_complex_wrapper_eq(&a.ultimate_boss_structure, &b.ultimate_boss_structure);
}