mod common;
use common::*;
use pojoc_core::types::ResolvedTypeRef;
use pojoc_schema::ast::*;

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
    let input = r#"
schema Test {
  version 1 {
    type Node<T> {
      value: T
      next: Node<T>?
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
        ResolvedTypeRef::Optional(inner) => {
            assert!(matches!(**inner, ResolvedTypeRef::Scalar(ref id) if id.name == "NodeI32"));
        }
        other => panic!("expected Optional<NodeI32>, got {other:?}"),
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
