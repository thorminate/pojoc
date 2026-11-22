use pojoc_build::core::types::*;
use pojoc_build::schema::ir::ir_types::{EnumVariant, ResolvedEnum};

fn id(name: &str) -> TypeId {
    TypeId {
        name: name.to_string(),
        version: 1,
    }
}

#[test]
fn normalize_type_maps_known_aliases() {
    assert_eq!(normalize_type("byte"), "u8");
    assert_eq!(normalize_type("uchar"), "u8");
    assert_eq!(normalize_type("ushort"), "u16");
    assert_eq!(normalize_type("uint"), "u32");
    assert_eq!(normalize_type("ulong"), "u64");
    assert_eq!(normalize_type("char"), "i8");
    assert_eq!(normalize_type("short"), "i16");
    assert_eq!(normalize_type("int"), "i32");
    assert_eq!(normalize_type("long"), "i64");
    assert_eq!(normalize_type("float"), "f32");
    assert_eq!(normalize_type("double"), "f64");
    assert_eq!(normalize_type("boolean"), "bool");
    assert_eq!(normalize_type("str"), "string");
}

#[test]
fn normalize_type_passes_through_canonical_and_unknown_names() {
    assert_eq!(normalize_type("u8"), "u8");
    assert_eq!(normalize_type("string"), "string");
    assert_eq!(normalize_type("MyStruct"), "MyStruct");
}

#[test]
fn is_primitive_recognizes_all_scalar_kinds_including_aliases() {
    for name in [
        "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "varint32", "varint64",
        "bool", "string", "byte", "uint", "double", "str",
    ] {
        assert!(is_primitive(name), "{name} should be primitive");
    }
}

#[test]
fn is_primitive_rejects_struct_names() {
    assert!(!is_primitive("MyStruct"));
    assert!(!is_primitive("Player"));
}

#[test]
fn type_id_unwraps_through_containers() {
    let scalar = ResolvedTypeRef::Scalar(id("i32"));
    assert_eq!(scalar.type_id(), Some(&id("i32")));

    let wrapped = ResolvedTypeRef::Optional(Box::new(ResolvedTypeRef::Array(Box::new(
        ResolvedTypeRef::Enum(id("Status"), 1),
    ))));
    assert_eq!(wrapped.type_id(), Some(&id("Status")));

    let fixed = ResolvedTypeRef::FixedArray(Box::new(ResolvedTypeRef::Union(id("Payload"))), 4);
    assert_eq!(fixed.type_id(), Some(&id("Payload")));

    let bitset = ResolvedTypeRef::Bitset(id("Flags"), 1);
    assert_eq!(bitset.type_id(), Some(&id("Flags")));
}

#[test]
fn type_id_is_none_for_shapeless_containers() {
    assert_eq!(ResolvedTypeRef::FixedString(4).type_id(), None);
    assert_eq!(
        ResolvedTypeRef::Tuple(vec![ResolvedTypeRef::Scalar(id("i32"))]).type_id(),
        None
    );
    assert_eq!(
        ResolvedTypeRef::VFloat {
            min: 0.0,
            max: 1.0,
            step: 0.1,
            backing: VFloatBacking::U16
        }
        .type_id(),
        None
    );
    assert_eq!(
        ResolvedTypeRef::ImportedSchema {
            alias: "Player".into(),
            root_name: "Player".into(),
            version: 1
        }
        .type_id(),
        None
    );
    // map only exposes the value type's id, not the key's, so use a value kind with no id at all
    assert_eq!(
        ResolvedTypeRef::Map(
            Box::new(ResolvedTypeRef::Scalar(id("string"))),
            Box::new(ResolvedTypeRef::FixedString(4))
        )
        .type_id(),
        None
    );
}

#[test]
fn vfloat_backing_reports_consistent_widths() {
    assert_eq!(VFloatBacking::U16.wire_size(), 2);
    assert_eq!(VFloatBacking::U16.rust_int_type(), "u16");
    assert_eq!(VFloatBacking::U16.read_fn(), "read_u16");
    assert_eq!(VFloatBacking::U16.write_fn(), "write_u16");

    assert_eq!(VFloatBacking::U32.wire_size(), 4);
    assert_eq!(VFloatBacking::U32.rust_int_type(), "u32");
    assert_eq!(VFloatBacking::U32.read_fn(), "read_u32");
    assert_eq!(VFloatBacking::U32.write_fn(), "write_u32");
}

#[test]
fn type_info_scalar_primitives_have_fixed_or_variable_wire_size() {
    let i32_info = type_info(&ResolvedTypeRef::Scalar(id("i32")));
    assert_eq!(i32_info.wire_size, WireSize::Fixed(4));
    assert_eq!(i32_info.rust_type, "i32");
    assert_eq!(i32_info.read_fn, "read_i32");
    assert_eq!(i32_info.write_fn, "write_i32");

    let string_info = type_info(&ResolvedTypeRef::Scalar(id("string")));
    assert_eq!(string_info.wire_size, WireSize::Variable);
    assert_eq!(string_info.rust_type, "&'buf str");

    let varint_info = type_info(&ResolvedTypeRef::Scalar(id("varint32")));
    assert_eq!(varint_info.wire_size, WireSize::Variable);
    assert_eq!(varint_info.rust_type, "u32");
    assert!(varint_info.size_fn.is_some());
}

#[test]
fn type_info_scalar_struct_fallthrough_uses_name_verbatim() {
    let info = type_info(&ResolvedTypeRef::Scalar(id("BoxI32")));
    assert_eq!(info.rust_type, "BoxI32");
    assert_eq!(info.read_fn, "read_box_i32");
    assert_eq!(info.write_fn, "write_box_i32");
    assert_eq!(info.wire_size, WireSize::Variable);
}

#[test]
fn type_info_optional_wraps_inner_rust_type_and_skip_stmt() {
    let info = type_info(&ResolvedTypeRef::Optional(Box::new(
        ResolvedTypeRef::Scalar(id("u32")),
    )));
    assert_eq!(info.rust_type, "Option<u32>");
    assert_eq!(info.wire_size, WireSize::Variable);
    assert_eq!(info.default_expr, "None");
}

#[test]
fn type_info_array_and_fixed_array_wire_sizes() {
    let array = type_info(&ResolvedTypeRef::Array(Box::new(ResolvedTypeRef::Scalar(
        id("u32"),
    ))));
    assert_eq!(array.wire_size, WireSize::Variable);
    assert_eq!(array.rust_type, "PojocVec<u32>");

    let fixed = type_info(&ResolvedTypeRef::FixedArray(
        Box::new(ResolvedTypeRef::Scalar(id("u32"))),
        4,
    ));
    assert_eq!(fixed.wire_size, WireSize::Fixed(16));
    assert_eq!(fixed.rust_type, "[u32; 4]");

    let fixed_of_variable = type_info(&ResolvedTypeRef::FixedArray(
        Box::new(ResolvedTypeRef::Scalar(id("string"))),
        4,
    ));
    assert_eq!(fixed_of_variable.wire_size, WireSize::Variable);
}

#[test]
fn type_info_map_and_fixed_map() {
    let map = type_info(&ResolvedTypeRef::Map(
        Box::new(ResolvedTypeRef::Scalar(id("string"))),
        Box::new(ResolvedTypeRef::Scalar(id("i32"))),
    ));
    assert_eq!(map.rust_type, "PojocMap<&'buf str, i32>");
    assert_eq!(map.wire_size, WireSize::Variable);

    let fixed_map = type_info(&ResolvedTypeRef::FixedMap(
        Box::new(ResolvedTypeRef::Scalar(id("i32"))),
        Box::new(ResolvedTypeRef::Scalar(id("f32"))),
        3,
    ));
    assert_eq!(fixed_map.wire_size, WireSize::Fixed((4 + 4) * 3));
}

#[test]
fn type_info_tuple_combines_element_rust_types_and_defaults() {
    let tuple = type_info(&ResolvedTypeRef::Tuple(vec![
        ResolvedTypeRef::Scalar(id("i32")),
        ResolvedTypeRef::Scalar(id("bool")),
    ]));
    assert_eq!(tuple.rust_type, "(i32, bool)");
    assert_eq!(tuple.default_expr, "(0i32, false)");
    assert_eq!(tuple.wire_size, WireSize::Fixed(5));
}

#[test]
fn type_info_vfloat_uses_backing_width() {
    let narrow = type_info(&ResolvedTypeRef::VFloat {
        min: 0.0,
        max: 10.0,
        step: 0.1,
        backing: VFloatBacking::U16,
    });
    assert_eq!(narrow.wire_size, WireSize::Fixed(2));
    assert_eq!(narrow.rust_type, "f32");

    let wide = type_info(&ResolvedTypeRef::VFloat {
        min: 0.0,
        max: 10.0,
        step: 0.1,
        backing: VFloatBacking::U32,
    });
    assert_eq!(wide.wire_size, WireSize::Fixed(4));
}

#[test]
fn type_info_imported_schema_namespaces_by_alias() {
    let info = type_info(&ResolvedTypeRef::ImportedSchema {
        alias: "Player".into(),
        root_name: "Player".into(),
        version: 2,
    });
    assert_eq!(info.rust_type, "player::Player");
    assert_eq!(info.read_fn, "player::decode_v2");
    assert_eq!(info.write_fn, "player::encode_v2");
}

#[test]
fn size_expr_fixed_size_ignores_accessor() {
    let info = type_info(&ResolvedTypeRef::Scalar(id("u32")));
    assert_eq!(info.size_expr(true, "self.x"), "4");
}

#[test]
fn size_expr_variable_string_uses_len_plus_varint_size() {
    let info = type_info(&ResolvedTypeRef::Scalar(id("string")));
    assert_eq!(
        info.size_expr(true, "self.name"),
        "varint_size(self.name.len()) + self.name.len()"
    );
}

#[test]
fn size_expr_with_size_fn_calls_it_directly() {
    let info = type_info(&ResolvedTypeRef::Scalar(id("varint32")));
    assert_eq!(
        info.size_expr(false, "self.x"),
        "varint_size(self.x as usize)"
    );
    assert_eq!(info.size_expr(true, "item"), "varint_size(*item as usize)");
}

#[test]
fn size_expr_falls_back_to_snake_case_size_hint_and_adds_borrow_when_needed() {
    let info = type_info(&ResolvedTypeRef::Scalar(id("BoxI32")));
    assert_eq!(
        info.size_expr(true, "self.value"),
        "size_hint_box_i32(self.value)"
    );
    assert_eq!(
        info.size_expr(false, "self.value"),
        "size_hint_box_i32(&self.value)"
    );
}

#[test]
fn delta_eligible_integers_only() {
    for name in ["u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64"] {
        assert!(
            is_delta_eligible_str(name),
            "{name} should be delta-eligible"
        );
        assert!(is_delta_eligible(&ResolvedTypeRef::Scalar(id(name))));
    }
    for name in ["f32", "f64", "string", "bool", "varint32"] {
        assert!(
            !is_delta_eligible_str(name),
            "{name} should not be delta-eligible"
        );
    }
    assert!(!is_delta_eligible(&ResolvedTypeRef::Scalar(id("MyStruct"))));
    assert!(!is_delta_eligible(&ResolvedTypeRef::Enum(id("Status"), 1)));
}

fn enum_with_variants(count: usize) -> ResolvedEnum {
    ResolvedEnum {
        variants: (0..count)
            .map(|i| EnumVariant {
                name: format!("V{i}"),
                wire_value: i as u32,
                doc: Vec::new(),
            })
            .collect(),
        doc: Vec::new(),
    }
}

// regression test: enum wire discriminants are now a fixed-width u8/u16/u32
// (see docs/schema.md's "Enum discriminant width" note) instead of varint —
// pins the exact variant-count thresholds where the width must grow, since
// `variants` already includes the compiler-injected `Unknown = 0` slot.
#[test]
fn enum_byte_width_thresholds() {
    assert_eq!(enum_with_variants(1).byte_width(), 1);
    assert_eq!(enum_with_variants(256).byte_width(), 1);
    assert_eq!(enum_with_variants(257).byte_width(), 2);
    assert_eq!(enum_with_variants(65536).byte_width(), 2);
    assert_eq!(enum_with_variants(65537).byte_width(), 4);
}

// ---- traversal ----

/// one value of every variant, so a variant added later fails these tests rather
/// than silently becoming a leaf in every walker built on `children`
fn one_of_each() -> Vec<ResolvedTypeRef> {
    use ResolvedTypeRef::*;
    let leaf = || Box::new(Scalar(id("u32")));
    vec![
        Scalar(id("u32")),
        Enum(id("E"), 1),
        Bitset(id("B"), 1),
        Union(id("U")),
        Array(leaf()),
        FixedArray(leaf(), 3),
        DeltaArray(leaf()),
        FixedDeltaArray(leaf(), 3),
        FixedString(8),
        Map(leaf(), leaf()),
        FixedMap(leaf(), leaf(), 3),
        Tuple(vec![Scalar(id("u32")), Scalar(id("f32"))]),
        VFloat {
            min: 0.0,
            max: 1.0,
            step: 0.1,
            backing: VFloatBacking::U16,
        },
        Optional(leaf()),
        Boxed(leaf()),
        Constrained {
            inner: leaf(),
            min: Some(0.0),
            max: None,
        },
        Interned(Box::new(Scalar(id("string")))),
        ImportedSchema {
            alias: "Sub".into(),
            root_name: "Inner".into(),
            version: 1,
        },
    ]
}

#[test]
fn children_yields_every_nested_ref_exactly_once() {
    use ResolvedTypeRef::*;
    for ty in one_of_each() {
        let n = ty.children().count();
        let expected = match &ty {
            Scalar(_)
            | Enum(..)
            | Bitset(..)
            | Union(_)
            | FixedString(_)
            | VFloat { .. }
            | ImportedSchema { .. } => 0,
            Map(..) | FixedMap(..) => 2,
            Tuple(elems) => elems.len(),
            // exhaustive on purpose: a new variant must be classified here, not
            // absorbed by a fallthrough that would hide it becoming a leaf
            Array(_)
            | FixedArray(..)
            | DeltaArray(_)
            | FixedDeltaArray(..)
            | Optional(_)
            | Boxed(_)
            | Constrained { .. }
            | Interned(_) => 1,
        };
        assert_eq!(n, expected, "wrong child count for {ty:?}");
    }
}

#[test]
fn map_children_are_key_then_value() {
    let ty = ResolvedTypeRef::Map(
        Box::new(ResolvedTypeRef::Scalar(id("string"))),
        Box::new(ResolvedTypeRef::Scalar(id("u32"))),
    );
    let kids: Vec<_> = ty.children().collect();
    assert_eq!(kids[0], &ResolvedTypeRef::Scalar(id("string")));
    assert_eq!(kids[1], &ResolvedTypeRef::Scalar(id("u32")));
}

#[test]
fn any_node_and_for_each_node_include_the_root_itself() {
    let leaf = ResolvedTypeRef::Scalar(id("u32"));
    assert!(leaf.any_node(&mut |t| matches!(t, ResolvedTypeRef::Scalar(_))));

    let mut seen = 0;
    leaf.for_each_node(&mut |_| seen += 1);
    assert_eq!(seen, 1, "a leaf must still visit itself");
}

#[test]
fn traversal_reaches_arbitrarily_deep_nesting() {
    // [ (string, {u32: box<T?>}) ] — every wrapper kind on one path
    let deep = ResolvedTypeRef::Array(Box::new(ResolvedTypeRef::Tuple(vec![
        ResolvedTypeRef::Scalar(id("string")),
        ResolvedTypeRef::Map(
            Box::new(ResolvedTypeRef::Scalar(id("u32"))),
            Box::new(ResolvedTypeRef::Boxed(Box::new(ResolvedTypeRef::Optional(
                Box::new(ResolvedTypeRef::Interned(Box::new(
                    ResolvedTypeRef::Scalar(id("string")),
                ))),
            )))),
        ),
    ])));

    assert!(contains_interned(&deep), "must find the buried Interned");

    let mut names = Vec::new();
    deep.for_each_node(&mut |t| {
        if let ResolvedTypeRef::Scalar(i) = t {
            names.push(i.name.clone());
        }
    });
    assert_eq!(names, vec!["string", "u32", "string"]);
}
