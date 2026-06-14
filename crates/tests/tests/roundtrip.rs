mod helpers;
use pojoc_tests::pojoc_edge::{runtime::*, *};
use helpers::*;

#[test]
fn test_roundtrip_with_default_values() {
    // Verifies that unpopulated structures round-trip successfully with default states
    let original = Edge::default();
    let mut buf = Vec::new();
    encode(&mut buf, &original);

    let decoded = decode(&buf).expect("Failed to decode standard default Edge payload");
    assert_edge_eq(&original, &decoded);
}

#[test]
fn test_roundtrip_with_populated_values() {
    // Verifies that a complex, fully populated Edge structure round-trips correctly
    let mut original = Edge::default();

    // Primitive and Float scalars
    original.u8_to_i64 = 100;
    original.i64_to_f32 = -987.65;
    original.i64_min = i64::MIN;
    original.varint32_max = u32::MAX;
    original.varint64_min = u64::MIN;
    original.f32_nan = f32::NAN;
    original.f32_inf = f32::INFINITY;
    original.f64_neg_inf = f64::NEG_INFINITY;

    // Strings & fixed lengths
    original.nullified_str = Some("PojocSerialization".into());
    original.spaces_str = "    ".into();
    original.fixed_str_min = [10, 20, 30, 40, 50, 60, 70, 80];

    // Standard collections
    original.empty_arr.push(pojstr!("FirstElement"));
    original.empty_arr.push(pojstr!("SecondElement"));
    original.array_to_map.insert(5, 25);

    // Sequences
    original.delta_positions.push(1000);
    original.delta_positions.push(1010); // Compressed Delta relative sequence check
    original.delta_i64_seq.push(-50);
    original.delta_i64_seq.push(-45);

    // Nested Layer initialization
    original.root_struct.leaf.leaf_val = "LeafNode".into();
    original.root_struct.leaf.leaf_numeric = 777;
    original.root_struct.weight = 3.14;
    original.root_struct.leaf_arr.push(NestedLeaf {
        leaf_val: "ArrayLeaf".into(),
        leaf_numeric: 11,
    });
    original.root_struct.leaf_opt = Some(NestedLeaf {
        leaf_val: "OptionalLeaf".into(),
        leaf_numeric: 22,
    });

    // Enums and optionals
    original.bounds_enum = NumericBounds::ExtraVariant;
    original.newly_added_optional = 999999;
    original.u32_delta_seq = [500; 16];

    // Map testing
    original.basic_map.insert("ConfigKey".into(), "ConfigValue".into());
    original.fixed_map_populated = pojmap!(
        "FixedMapKey1" => 5,
        "FixedMapKey2" => 30;
        2
    );
    original.delta_value_map = pojmap!(
        "DeltaMapKey1" => pojvec![10],
        "DeltaMapKey2" => pojvec![20];
        2
    );

    // Struct layout nesting (Sensor frames)
    let mut frame = SensorFrame::default();
    frame.readings.push(55);
    frame.readings.push(60);
    frame.sample_ids = [100, 200, 300, 400, 500, 600, 700, 800];
    original.sensor_log.push(frame);

    // Option values
    original.opt_u8 = Some(255);
    original.opt_i16 = Some(-32768);
    original.opt_u32 = Some(400000);
    original.opt_i64 = Some(-900000);
    original.opt_f32 = Some(12.34);
    original.opt_f64 = Some(56.78);
    original.opt_bool = Some(false);
    original.opt_fixed_str = Some([0xAA, 0xBB, 0xCC, 0xDD]);
    original.opt_bitset = Some(SystemPrivileges::READ | SystemPrivileges::WRITE);

    // Native System Privileges Bitset configuration
    original.system_perms = SystemPrivileges::ROOT | SystemPrivileges::NETWORK_ACCESS | SystemPrivileges::EXECUTE;
    original.legacy_hw_flags = 0xFFFFFFFF;

    let mut delta_arr = PojocVec::new();
    delta_arr.push(42);
    original.opt_delta_arr = Some(delta_arr);

    // Complex top-level nested structure matrix
    original.ultimate_boss_structure.frame_deltas.push(8888);

    // Perform payload round-trip execution
    let mut buf = Vec::new();
    encode(&mut buf, &original);

    let decoded = decode(&buf).expect("Failed to decode completely populated Edge payload");
    assert_edge_eq(&original, &decoded);
}

#[test]
fn test_hardware_flags_defaults_and_operators() {
    // HardwareFlags internal value default is 0x05 (IS_CPU_BOUND | HAS_VULKAN)
    let mut flags = HardwareFlags::default();
    assert!(flags.is_cpu_bound(), "Default hardware flags should include IS_CPU_BOUND");
    assert!(!flags.is_gpu_bound(), "Default hardware flags should exclude IS_GPU_BOUND");
    assert!(flags.has_vulkan(), "Default hardware flags should include HAS_VULKAN");

    // Setter assertions
    flags.set_is_gpu_bound(true);
    assert!(flags.is_gpu_bound(), "Setting IS_GPU_BOUND should update the value");

    // Functional builder modifications
    let builder_flags = flags.with_is_cpu_bound(false);
    assert!(!builder_flags.is_cpu_bound(), "Builder should have set IS_CPU_BOUND to false");
    assert!(builder_flags.is_gpu_bound());

    // Operational Bitwise checks
    let f1 = HardwareFlags::IS_CPU_BOUND;
    let f2 = HardwareFlags::IS_GPU_BOUND;
    let combined = f1 | f2;
    assert!(combined.is_cpu_bound());
    assert!(combined.is_gpu_bound());
    assert!(!combined.has_vulkan());

    let intersection = combined & HardwareFlags::IS_CPU_BOUND;
    assert!(intersection.is_cpu_bound());
    assert!(!intersection.is_gpu_bound());
}

#[test]
fn test_system_privileges_bitmask_operations() {
    let mut perms = SystemPrivileges::default();
    assert!(perms.is_empty(), "Default system privileges mask should be empty");
    assert!(!perms.read());

    perms.set_read(true);
    perms.set_root(true);
    assert!(perms.read());
    assert!(perms.root());
    assert!(!perms.write());

    // Inversion check
    let inverted = !perms;
    assert!(!inverted.read());
    assert!(inverted.write());
    assert!(inverted.execute());
    assert!(inverted.network_access());
    assert!(!inverted.root());
}

#[test]
fn test_numeric_bounds_enum_conversions() {
    assert_eq!(NumericBounds::default(), NumericBounds::ResetZero);
    assert_eq!(NumericBounds::try_from(0), Ok(NumericBounds::ResetZero));
    assert_eq!(NumericBounds::try_from(2), Ok(NumericBounds::MinI64));
    assert_eq!(NumericBounds::try_from(4), Ok(NumericBounds::ExtraVariant));
    assert_eq!(NumericBounds::try_from(99), Err(99), "Invalid variant should return error variant index");
}

#[test]
fn test_static_associated_constants() {
    assert_eq!(Edge::PI_CONST, std::f64::consts::PI);
    assert!(Edge::FLAG_CONST);
}