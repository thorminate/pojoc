mod common;
use common::*;
use pojoc_build::core::types::ResolvedTypeRef;
use pojoc_build::schema::ast::*;

const SAMPLE_SCHEMA: &str = r#"
schema Player {

  version 1 {
    fields {
      id: string = "player_1"
      name: string = "Player 1"
      level: int = 1
      inventory: [string] = []
    }
  }

  version 2 {
    type Vector3 {
      x: float = 0.0
      y: float = 0.0
      z: float = 0.0
    }

    diff {
      + position: Vector3
      ~ level: float
    }
  }

  version 3 {
    type Vector3 extends Vector3@2 {
      + w: float = 1.0
    }

    diff {
      ~ id -> player_id
      ~ position: Vector3
      - name
      + tags: [string] = []
    }
  }
}
"#;

#[test]
fn parses_full_test_schema() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();

    assert_eq!(ast.name, "Player");
    assert_eq!(ast.versions.len(), 3);
}

#[test]
fn parses_versions_correctly() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();

    assert_eq!(ast.versions[0].version, 1);
    assert_eq!(ast.versions[1].version, 2);
    assert_eq!(ast.versions[2].version, 3);
}

#[test]
fn parses_typedef_vector3() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();
    let v2 = &ast.versions[1];

    let typedefs: Vec<_> = v2
        .blocks
        .iter()
        .filter_map(|b| {
            if let VersionBlockAst::TypeDef(t) = b {
                Some(t)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(typedefs.len(), 1);
    assert_eq!(typedefs[0].name, "Vector3");
    assert!(typedefs[0].extends.is_none());

    match &typedefs[0].body {
        TypeBody::Fields(fields) => assert_eq!(fields.fields.len(), 3),
        TypeBody::Diff(_) => panic!("expected Fields body"),
    }
}

#[test]
fn parses_diff_operations() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();
    let v2 = &ast.versions[1];

    let diff = v2
        .blocks
        .iter()
        .find_map(|b| {
            if let VersionBlockAst::Diff(d) = b {
                Some(d)
            } else {
                None
            }
        })
        .unwrap();

    assert_eq!(diff.len(), 2);

    match &diff[0] {
        DiffAst::Add { field } => {
            assert_eq!(field.name, "position");
        }
        _ => panic!("expected Add"),
    }

    match &diff[1] {
        DiffAst::UpdateType { name, ty, .. } => {
            assert_eq!(name, "level");
            assert!(matches!(ty, TypeAst::Named(t) if t == "float"));
        }
        _ => panic!("expected UpdateType"),
    }
}

#[test]
fn parses_rename_operation() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();
    let v3 = &ast.versions[2];

    let diff = v3
        .blocks
        .iter()
        .find_map(|b| {
            if let VersionBlockAst::Diff(d) = b {
                Some(d)
            } else {
                None
            }
        })
        .unwrap();

    let rename = diff
        .iter()
        .find_map(|op| {
            if let DiffAst::Rename { from, to, .. } = op {
                Some((from, to))
            } else {
                None
            }
        })
        .unwrap();

    assert_eq!(rename.0, "id");
    assert_eq!(rename.1, "player_id");
}

#[test]
fn parses_array_types() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();

    let v1 = &ast.versions[0];

    let fields = v1
        .blocks
        .iter()
        .find_map(|b| {
            if let VersionBlockAst::Fields(f) = b {
                Some(f)
            } else {
                None
            }
        })
        .expect("no fields block");

    let inventory = fields
        .fields
        .iter()
        .find(|f| f.name == "inventory")
        .expect("no inventory field");

    match &inventory.ty {
        TypeAst::Array(inner) => {
            assert!(matches!(inner.as_ref(), TypeAst::Named(s) if s == "string"));
        }
        _ => panic!("expected array"),
    }
}

#[test]
fn rejects_invalid_syntax() {
    let input = "schema Player { version 1 {";

    assert!(parse_schema(input).is_err());
}

#[test]
fn rejects_unknown_tokens() {
    let input = r#"
schema Player {
  version 1 {
    fields {
      id ??? string
    }
  }
}
"#;

    assert!(parse_schema(input).is_err());
}

#[test]
fn ast_snapshot_stable() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();

    let debug = format!("{:#?}", ast);

    assert!(debug.contains("Player"));
    assert!(debug.contains("Vector3"));
    assert!(debug.contains("version: 3"));
}

#[test]
fn rejects_duplicated_sections() {
    let input = r#"
schema Player {
  version 1 {
    fields {}
    fields {}
  }
}
"#;

    assert!(parse_schema(input).is_err());
}

#[test]
fn rejects_duplicated_fields() {
    let input = r#"
schema Player {
  version 1 {
    fields {
       id: string
       id: string
    }
  }
}
"#;

    assert!(parse_schema(input).is_err());
}

#[test]
fn rejects_duplicated_version() {
    let input = r#"
schema Player {
  version 1 {
    fields {}
  }
  version 1 {
    fields {}
  }
}
"#;

    assert!(parse_schema(input).is_err());
}

#[test]
fn rejects_nonsensical_diff() {
    let input = r#"
schema Player {
  version 1 {
    fields {
        wow: string
    }
  }
  version 2 {
    diff {
      + field: string
      - field
      ~ field: int
    }
  }
}
"#;

    assert!(parse_schema(input).is_err());
}

#[test]
fn parses_extends_on_type() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();
    let v3 = &ast.versions[2];

    let typedef = v3
        .blocks
        .iter()
        .find_map(|b| {
            if let VersionBlockAst::TypeDef(t) = b {
                Some(t)
            } else {
                None
            }
        })
        .expect("no typedef in v3");

    assert_eq!(typedef.name, "Vector3");

    let ext = typedef
        .extends
        .as_ref()
        .expect("expected explicit version extension");
    assert_eq!(ext.name, "Vector3");
    assert_eq!(ext.version, 2);
}

#[test]
fn extended_type_body_is_diff() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();
    let v3 = &ast.versions[2];

    let typedef = v3
        .blocks
        .iter()
        .find_map(|b| {
            if let VersionBlockAst::TypeDef(t) = b {
                Some(t)
            } else {
                None
            }
        })
        .unwrap();

    match &typedef.body {
        TypeBody::Diff(ops) => {
            assert_eq!(ops.len(), 1);
            match &ops[0] {
                DiffAst::Add { field } => {
                    assert_eq!(field.name, "w");
                    assert!(matches!(&field.ty, TypeAst::Named(t) if t == "float"));
                }
                _ => panic!("expected Add op"),
            }
        }
        TypeBody::Fields(_) => panic!("expected Diff body for extended type"),
    }
}

#[test]
fn non_extended_type_body_is_fields() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();
    let v2 = &ast.versions[1];

    let typedef = v2
        .blocks
        .iter()
        .find_map(|b| {
            if let VersionBlockAst::TypeDef(t) = b {
                Some(t)
            } else {
                None
            }
        })
        .unwrap();

    assert!(typedef.extends.is_none());
    assert!(matches!(typedef.body, TypeBody::Fields(_)));
}

#[test]
fn rejects_extends_with_unknown_parent() {
    let input = r#"
schema Player {
  version 1 {
    fields { id: string }
  }
  version 2 {
    type Vector3 extends Phantom@1 {
      + x: float
    }
    diff {
      + position: Vector3
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    assert!(analyze_schema(&ast).is_err());
}

#[test]
fn rejects_extends_with_no_version_tag() {
    let input = r#"
schema Player {
  version 1 {
    fields { id: string }
  }
  version 2 {
    type Vector3 extends Phantom {
      + x: float
    }
    diff {
      + position: Vector3
    }
  }
}
"#;

    assert!(parse_schema(input).is_err());
}

#[test]
fn extended_type_preserves_field_ids() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    let v2_vec3 = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "Vector3" && id.version == 2)
        .map(|(_, t)| t)
        .expect("Vector3 v2 not found");

    let v3_vec3 = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "Vector3" && id.version == 3)
        .map(|(_, t)| t)
        .expect("Vector3 v3 not found");

    for name in ["x", "y", "z"] {
        let id_v2 = v2_vec3.fields.iter().find(|f| f.name == name).map(|f| f.id);
        let id_v3 = v3_vec3.fields.iter().find(|f| f.name == name).map(|f| f.id);
        assert_eq!(
            id_v2, id_v3,
            "field '{name}' has different ID across Vector3 versions"
        );
    }

    assert!(v2_vec3.fields.iter().all(|f| f.name != "w"));
    assert!(v3_vec3.fields.iter().any(|f| f.name == "w"));
}

#[test]
fn extended_type_rename_tracked_correctly() {
    let input = r#"
schema Player {
  version 1 {
    fields { wow: string }
  }
  version 2 {
    type Vec3 {
      x: float
      y: float
    }
    diff {
      + position: Vec3
    }
  }
  version 3 {
    type Vec3 extends Vec3@2 {
      ~ x -> pos_x
    }
    diff {
      ~ position: Vec3
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    let v2_vec3 = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "Vec3" && id.version == 2)
        .map(|(_, t)| t)
        .unwrap();

    let v3_vec3 = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "Vec3" && id.version == 3)
        .map(|(_, t)| t)
        .unwrap();

    let x_id = v2_vec3
        .fields
        .iter()
        .find(|f| f.name == "x")
        .map(|f| f.id)
        .unwrap();
    let pos_x_id = v3_vec3
        .fields
        .iter()
        .find(|f| f.name == "pos_x")
        .map(|f| f.id)
        .unwrap();

    assert_eq!(
        x_id, pos_x_id,
        "renamed field 'x -> pos_x' should retain its FieldId"
    );
}

#[test]
fn rejects_extends_with_full_field_body() {
    let input = r#"
schema Player {
  version 1 {
    fields { id: string }
  }
  version 2 {
    type Vec3 {
      x: float
    }
    diff {}
  }
  version 3 {
    type Vec3 extends Vec3@2 {
      x: float
      y: float
    }
    diff {}
  }
}
"#;
    if let Ok(ast) = parse_schema(input) {
        assert!(analyze_schema(&ast).is_err());
    }
}

#[test]
fn rejects_extends_future_version() {
    let input = r#"
schema Player {
  version 1 {
    fields { id: string }
  }
  version 2 {
    type Vec3 extends Vec3@4 {
      + x: float
    }
    diff {}
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    assert!(analyze_schema(&ast).is_err());
}

#[test]
fn generic_type_instantiates_distinct_monomorphizations() {
    let input = r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
    fields {
      b1: Box<i32>
      b2: Box<string>
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    let box_i32 = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "BoxI32")
        .map(|(_, t)| t)
        .expect("BoxI32 not found");
    let box_string = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "BoxString")
        .map(|(_, t)| t)
        .expect("BoxString not found");

    assert!(matches!(
        box_i32.fields.iter().find(|f| f.name == "value").unwrap().ty,
        ResolvedTypeRef::Scalar(ref id) if id.name == "i32"
    ));
    assert!(matches!(
        box_string.fields.iter().find(|f| f.name == "value").unwrap().ty,
        ResolvedTypeRef::Scalar(ref id) if id.name == "string"
    ));
}

#[test]
fn generic_arity_mismatch_errors() {
    let input = r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
    fields {
      b: Box<i32, string>
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    assert!(analyze_schema(&ast).is_err());
}

#[test]
fn generic_used_without_args_errors() {
    let input = r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
    fields {
      b: Box
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    assert!(analyze_schema(&ast).is_err());
}

#[test]
fn self_referential_generic_resolves() {
    // A self-referential generic still needs `box<>` to break the cycle —
    // otherwise the analyzer's recursion-cycle check (which exists
    // specifically to catch this before it becomes an infinite-size `rustc`
    // failure) now rejects it, same as a plain non-generic self-reference.
    let input = r#"
schema Test {
  version 1 {
    type Node<T> {
      value: T
      next: box<Node<T>>?
    }
    fields {
      root: Node<i32>
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    let node_i32 = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "NodeI32")
        .map(|(_, t)| t)
        .expect("NodeI32 not found");

    let next = &node_i32
        .fields
        .iter()
        .find(|f| f.name == "next")
        .unwrap()
        .ty;
    match next {
        ResolvedTypeRef::Optional(inner) => match &**inner {
            ResolvedTypeRef::Boxed(boxed) => {
                assert!(matches!(**boxed, ResolvedTypeRef::Scalar(ref id) if id.name == "NodeI32"));
            }
            other => panic!("expected Boxed(NodeI32), got {other:?}"),
        },
        other => panic!("expected Optional<Boxed<NodeI32>>, got {other:?}"),
    }
}

#[test]
fn generic_param_evolution_with_wildcard_drop() {
    let input = r#"
schema Test {
  version 1 {
    type Mono<A> {
      value: A
    }
    fields {
      m: Mono<i32>
    }
  }
  version 2 {
    type Pair<A, B> extends Mono<A>@1 {
      + other: B?
    }
    diff {
      ~ m: Pair<i32, string>
    }
  }
  version 3 {
    type Mono<A> extends Pair<A, _>@2 {
      - other
    }
    diff {
      ~ m: Mono<i32>
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    // `m: Mono<i32>` at version 3 is a monomorphized instantiation, stored under
    // its mangled name rather than the bare template name "Mono".
    let mono_i32 = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "MonoI32")
        .map(|(_, t)| t)
        .expect("MonoI32 not found");
    assert!(mono_i32.fields.iter().any(|f| f.name == "value"));
    assert!(mono_i32.fields.iter().all(|f| f.name != "other"));
}

#[test]
fn generic_wildcard_drop_without_cleanup_errors() {
    let input = r#"
schema Test {
  version 1 {
    type Mono<A> {
      value: A
    }
    fields {
      m: Mono<i32>
    }
  }
  version 2 {
    type Pair<A, B> extends Mono<A>@1 {
      + other: B?
    }
    diff {
      ~ m: Pair<i32, string>
    }
  }
  version 3 {
    type Mono<A> extends Pair<A, _>@2 {
    }
    diff {
      ~ m: Mono<i32>
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    assert!(analyze_schema(&ast).is_err());
}

#[test]
fn plain_type_extends_specific_generic_instantiation() {
    let input = r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
    fields {
      b: Box<i32>
    }
  }
  version 2 {
    type ConcreteFoo extends Box<i32>@1 {
      + label: string = "x"
    }
    diff {
      + f: ConcreteFoo
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    let foo = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "ConcreteFoo")
        .map(|(_, t)| t)
        .expect("ConcreteFoo not found");
    assert!(foo.fields.iter().any(|f| f.name == "value"));
    assert!(foo.fields.iter().any(|f| f.name == "label"));
}

#[test]
fn generic_alias_names_the_monomorphized_type() {
    let input = r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
    fields {
      b: Box<i32> as MyInt
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    assert!(
        schema.types.types.iter().any(|(id, _)| id.name == "MyInt"),
        "expected a type named MyInt"
    );
    assert!(
        !schema.types.types.iter().any(|(id, _)| id.name == "BoxI32"),
        "auto-mangled name shouldn't also be registered once aliased"
    );
}

#[test]
fn generic_alias_dedupes_same_instantiation() {
    let input = r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
    fields {
      b1: Box<i32> as MyInt
      b2: Box<i32> as MyInt
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    let matches: Vec<_> = schema
        .types
        .types
        .iter()
        .filter(|(id, _)| id.name == "MyInt")
        .collect();
    assert_eq!(matches.len(), 1, "expected exactly one MyInt registration");
}

#[test]
fn generic_alias_collision_with_different_args_errors() {
    let input = r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
    fields {
      b1: Box<i32> as MyInt
      b2: Box<string> as MyInt
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    assert!(analyze_schema(&ast).is_err());
}

#[test]
fn generic_alias_matching_auto_mangled_name_dedupes_with_unaliased_usage() {
    let input = r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
    fields {
      b1: Box<i32>
      b2: Box<i32> as BoxI32
    }
  }
}
"#;
    let ast = parse_schema(input).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    let matches: Vec<_> = schema
        .types
        .types
        .iter()
        .filter(|(id, _)| id.name == "BoxI32")
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected a single, shared BoxI32 registration"
    );
}

const DOC_SCHEMA: &str = r#"
/// Root schema doc.
schema DocDemo {
  version 1 {
    /// Enum doc.
    enum Color {
      /// Variant doc.
      Red,
      Green,
    }

    /// Bitset doc.
    bitset Flags {
      /// Flag doc.
      Alpha,
      Beta,
    }

    type Payload {
      x: i32 = 0
    }

    /// Union doc.
    union Action {
      /// Union variant doc.
      Move: Payload,
    }

    /// Type doc.
    type Widget {
      /// Field doc.
      name: string = "w"
      /// Const doc.
      max_count: const i32 = 10
    }
  }

  version 2 {
    enum Color extends Color@1 {
      /// Newly added variant doc.
      + Blue
    }

    bitset Flags extends Flags@1 {
      /// Newly added flag doc.
      + Gamma
    }

    union Action extends Action@1 {
      /// Newly added union variant doc.
      + Jump: Payload
    }

    diff {
      /// Newly added root field doc.
      + extra: i32 = 0
    }
  }
}
"#;

#[test]
fn parses_schema_level_doc_comment() {
    let ast = parse_schema(DOC_SCHEMA).unwrap();
    assert_eq!(ast.doc, vec!["Root schema doc.".to_string()]);
}

#[test]
fn parses_type_and_field_doc_comments() {
    let ast = parse_schema(DOC_SCHEMA).unwrap();
    let v1 = &ast.versions[0];

    let widget = v1
        .blocks
        .iter()
        .find_map(|b| match b {
            VersionBlockAst::TypeDef(t) if t.name == "Widget" => Some(t),
            _ => None,
        })
        .expect("Widget typedef not found");

    assert_eq!(widget.doc, vec!["Type doc.".to_string()]);

    match &widget.body {
        TypeBody::Fields(fields) => {
            let name_field = fields.fields.iter().find(|f| f.name == "name").unwrap();
            assert_eq!(name_field.doc, vec!["Field doc.".to_string()]);

            let const_field = fields
                .const_fields
                .iter()
                .find(|c| c.name == "max_count")
                .unwrap();
            assert_eq!(const_field.doc, vec!["Const doc.".to_string()]);
        }
        TypeBody::Diff(_) => panic!("expected Fields body"),
    }
}

#[test]
fn parses_enum_and_variant_doc_comments() {
    let ast = parse_schema(DOC_SCHEMA).unwrap();
    let v1 = &ast.versions[0];

    let color = v1
        .blocks
        .iter()
        .find_map(|b| match b {
            VersionBlockAst::EnumDef(e) if e.name() == "Color" => Some(e),
            _ => None,
        })
        .expect("Color enum not found");

    match color {
        EnumDefAst::Definition { doc, variants, .. } => {
            assert_eq!(doc, &vec!["Enum doc.".to_string()]);
            let red = variants.iter().find(|v| v.name == "Red").unwrap();
            assert_eq!(red.doc, vec!["Variant doc.".to_string()]);
        }
        EnumDefAst::Extension { .. } => panic!("expected Definition"),
    }
}

#[test]
fn parses_bitset_and_flag_doc_comments() {
    let ast = parse_schema(DOC_SCHEMA).unwrap();
    let v1 = &ast.versions[0];

    let flags = v1
        .blocks
        .iter()
        .find_map(|b| match b {
            VersionBlockAst::BitsetDef(bd) if bd.name() == "Flags" => Some(bd),
            _ => None,
        })
        .expect("Flags bitset not found");

    match flags {
        BitsetDefAst::Definition { doc, variants, .. } => {
            assert_eq!(doc, &vec!["Bitset doc.".to_string()]);
            let alpha = variants.iter().find(|v| v.name == "Alpha").unwrap();
            assert_eq!(alpha.doc, vec!["Flag doc.".to_string()]);
        }
        BitsetDefAst::Extension { .. } => panic!("expected Definition"),
    }
}

#[test]
fn parses_union_and_variant_doc_comments() {
    let ast = parse_schema(DOC_SCHEMA).unwrap();
    let v1 = &ast.versions[0];

    let action = v1
        .blocks
        .iter()
        .find_map(|b| match b {
            VersionBlockAst::UnionDef(u) if u.name() == "Action" => Some(u),
            _ => None,
        })
        .expect("Action union not found");

    match action {
        UnionDefAst::Definition { doc, variants, .. } => {
            assert_eq!(doc, &vec!["Union doc.".to_string()]);
            let mv = variants.iter().find(|v| v.name == "Move").unwrap();
            assert_eq!(mv.doc, vec!["Union variant doc.".to_string()]);
        }
        UnionDefAst::Extension { .. } => panic!("expected Definition"),
    }
}

#[test]
fn parses_doc_comments_on_diff_add_ops() {
    let ast = parse_schema(DOC_SCHEMA).unwrap();
    let v2 = &ast.versions[1];

    let color = v2
        .blocks
        .iter()
        .find_map(|b| match b {
            VersionBlockAst::EnumDef(e) if e.name() == "Color" => Some(e),
            _ => None,
        })
        .expect("Color enum extension not found");
    match color {
        EnumDefAst::Extension { ops, .. } => {
            let add = ops
                .iter()
                .find(|op| matches!(op, EnumVariantOpAst::Add { name, .. } if name == "Blue"))
                .unwrap();
            match add {
                EnumVariantOpAst::Add { doc, .. } => {
                    assert_eq!(doc, &vec!["Newly added variant doc.".to_string()])
                }
                EnumVariantOpAst::Rename { .. } => panic!("expected Add"),
            }
        }
        EnumDefAst::Definition { .. } => panic!("expected Extension"),
    }

    let flags = v2
        .blocks
        .iter()
        .find_map(|b| match b {
            VersionBlockAst::BitsetDef(bd) if bd.name() == "Flags" => Some(bd),
            _ => None,
        })
        .expect("Flags bitset extension not found");
    match flags {
        BitsetDefAst::Extension { ops, .. } => {
            let add = ops
                .iter()
                .find(|op| matches!(op, BitsetOpAst::Add { name, .. } if name == "Gamma"))
                .unwrap();
            match add {
                BitsetOpAst::Add { doc, .. } => {
                    assert_eq!(doc, &vec!["Newly added flag doc.".to_string()])
                }
                BitsetOpAst::Remove { .. } => panic!("expected Add"),
            }
        }
        BitsetDefAst::Definition { .. } => panic!("expected Extension"),
    }

    let action = v2
        .blocks
        .iter()
        .find_map(|b| match b {
            VersionBlockAst::UnionDef(u) if u.name() == "Action" => Some(u),
            _ => None,
        })
        .expect("Action union extension not found");
    match action {
        UnionDefAst::Extension { ops, .. } => {
            let UnionVariantOpAst::Add { doc, .. } = ops
                .iter()
                .find(|op| matches!(op, UnionVariantOpAst::Add { name, .. } if name == "Jump"))
                .unwrap();
            assert_eq!(doc, &vec!["Newly added union variant doc.".to_string()]);
        }
        UnionDefAst::Definition { .. } => panic!("expected Extension"),
    }

    let diff = v2
        .blocks
        .iter()
        .find_map(|b| match b {
            VersionBlockAst::Diff(ops) => Some(ops),
            _ => None,
        })
        .expect("root diff not found");
    let add = diff
        .iter()
        .find(|op| matches!(op, DiffAst::Add { field } if field.name == "extra"))
        .unwrap();
    match add {
        DiffAst::Add { field } => {
            assert_eq!(field.doc, vec!["Newly added root field doc.".to_string()])
        }
        _ => panic!("expected Add"),
    }
}

#[test]
fn doc_comment_survives_analysis_and_reaches_resolved_ir() {
    let ast = parse_schema(DOC_SCHEMA).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    assert_eq!(schema.doc, vec!["Root schema doc.".to_string()]);

    let widget = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "Widget")
        .map(|(_, t)| t)
        .expect("Widget not found in resolved types");
    assert_eq!(widget.doc, vec!["Type doc.".to_string()]);
    let name_field = widget.fields.iter().find(|f| f.name == "name").unwrap();
    assert_eq!(name_field.doc, vec!["Field doc.".to_string()]);
    let const_field = widget
        .const_fields
        .iter()
        .find(|c| c.name == "max_count")
        .unwrap();
    assert_eq!(const_field.doc, vec!["Const doc.".to_string()]);
}

#[test]
fn intern_field_resolves_to_interned_type() {
    let src = r#"
schema Test {
  version 1 {
    fields {
      label: intern string = ""
    }
  }
}
"#;
    let ast = parse_schema(src).unwrap();
    let schema = analyze_schema(&ast).unwrap();
    let field = schema
        .versions
        .last()
        .unwrap()
        .fields
        .iter()
        .find(|f| f.name == "label")
        .unwrap();
    assert!(matches!(field.ty, ResolvedTypeRef::Interned(_)));
}

#[test]
fn intern_composes_inside_array() {
    let src = r#"
schema Test {
  version 1 {
    fields {
      tags: [intern string] = []
    }
  }
}
"#;
    let ast = parse_schema(src).unwrap();
    let schema = analyze_schema(&ast).unwrap();
    let field = schema
        .versions
        .last()
        .unwrap()
        .fields
        .iter()
        .find(|f| f.name == "tags")
        .unwrap();
    match &field.ty {
        ResolvedTypeRef::Array(inner) => {
            assert!(matches!(**inner, ResolvedTypeRef::Interned(_)))
        }
        other => panic!("expected Array(Interned(string)), got {other:?}"),
    }
}

#[test]
fn intern_composes_as_generic_arg() {
    let src = r#"
schema Test {
  version 1 {
    type Mono<T> {
      value: T
    }
    fields {
      wrapped: Mono<intern string>
    }
  }
}
"#;
    let ast = parse_schema(src).unwrap();
    let schema = analyze_schema(&ast).unwrap();
    let mono_interned_string = schema
        .types
        .types
        .iter()
        .find(|(id, _)| id.name == "MonoInternedString")
        .map(|(_, t)| t)
        .expect("MonoInternedString not found");
    let value_field = mono_interned_string
        .fields
        .iter()
        .find(|f| f.name == "value")
        .unwrap();
    assert!(matches!(value_field.ty, ResolvedTypeRef::Interned(_)));
}

#[test]
fn intern_cannot_combine_with_lazy() {
    let src = r#"
schema Test {
  version 1 {
    fields {
      label: lazy intern string
    }
  }
}
"#;
    let ast = parse_schema(src).unwrap();
    assert!(analyze_schema(&ast).is_err());
}

#[test]
fn intern_cannot_combine_with_const() {
    let src = r#"
schema Test {
  version 1 {
    fields {
      label: const intern string = ""
    }
  }
}
"#;
    let ast = parse_schema(src).unwrap();
    assert!(analyze_schema(&ast).is_err());
}

#[test]
fn intern_rejects_non_string_type() {
    let src = r#"
schema Test {
  version 1 {
    fields {
      count: intern i32 = 0
    }
  }
}
"#;
    let ast = parse_schema(src).unwrap();
    assert!(analyze_schema(&ast).is_err());
}
