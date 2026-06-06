mod common;
use pojoc_schema::ast::*;
use common::*;

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
    type Vector3 extends Vector3 {
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

    let typedefs: Vec<_> = v2.blocks.iter().filter_map(|b| {
        if let VersionBlockAst::TypeDef(t) = b { Some(t) } else { None }
    }).collect();

    assert_eq!(typedefs.len(), 1);
    assert_eq!(typedefs[0].name, "Vector3");
    assert!(typedefs[0].extends.is_none());

    match &typedefs[0].body {
        TypeBody::Fields(fields) => assert_eq!(fields.len(), 3),
        TypeBody::Diff(_) => panic!("expected Fields body"),
    }
}
#[test]
fn parses_diff_operations() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();
    let v2 = &ast.versions[1];

    let diff = v2.blocks.iter().find_map(|b| {
        if let VersionBlockAst::Diff(d) = b {
            Some(d)
        } else {
            None
        }
    }).unwrap();

    assert_eq!(diff.len(), 2);

    match &diff[0] {
        DiffAst::Add { field } => {
            assert_eq!(field.name, "position");
        }
        _ => panic!("expected Add"),
    }

    match &diff[1] {
        DiffAst::UpdateType { name, ty } => {
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

    let diff = v3.blocks.iter().find_map(|b| {
        if let VersionBlockAst::Diff(d) = b {
            Some(d)
        } else {
            None
        }
    }).unwrap();

    let rename = diff.iter().find_map(|op| {
        if let DiffAst::Rename { from, to } = op {
            Some((from, to))
        } else {
            None
        }
    }).unwrap();

    assert_eq!(rename.0, "id");
    assert_eq!(rename.1, "player_id");
}

#[test]
fn parses_array_types() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();

    let v1 = &ast.versions[0];

    let fields = v1.blocks.iter().find_map(|b| {
        if let VersionBlockAst::Fields(f) = b {
            Some(f)
        } else {
            None
        }
    }).expect("no fields block");

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
fn rejects_confusing_diff() {
    let input = r#"
schema Player {
  version 1 {
    fields {
        wow: string
    }
  }
  version 2 {
    diff {
      + wowies: string
      - wowies
      ~ wowies: int
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

    let typedef = v3.blocks.iter().find_map(|b| {
        if let VersionBlockAst::TypeDef(t) = b { Some(t) } else { None }
    }).expect("no typedef in v3");

    assert_eq!(typedef.name, "Vector3");
    assert_eq!(typedef.extends.as_deref(), Some("Vector3"));
}

#[test]
fn extended_type_body_is_diff() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();
    let v3 = &ast.versions[2];

    let typedef = v3.blocks.iter().find_map(|b| {
        if let VersionBlockAst::TypeDef(t) = b { Some(t) } else { None }
    }).unwrap();

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

    let typedef = v2.blocks.iter().find_map(|b| {
        if let VersionBlockAst::TypeDef(t) = b { Some(t) } else { None }
    }).unwrap();

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
    type Vector3 extends Phantom {
      + x: float
    }
    diff {
      + position: Vector3
    }
  }
}
"#;
    // parse succeeds, analyzer should fail
    let ast = parse_schema(input).unwrap();
    assert!(analyze_schema(&ast).is_err());
}

#[test]
fn extended_type_preserves_field_ids() {
    let ast = parse_schema(SAMPLE_SCHEMA).unwrap();
    let schema = analyze_schema(&ast).unwrap();

    let v2_vec3 = schema.types.types.iter()
        .find(|(id, _)| id.name == "Vector3" && id.version == 2)
        .map(|(_, t)| t)
        .expect("Vector3 v2 not found");

    let v3_vec3 = schema.types.types.iter()
        .find(|(id, _)| id.name == "Vector3" && id.version == 3)
        .map(|(_, t)| t)
        .expect("Vector3 v3 not found");

    // x, y, z should have matching IDs across versions
    for name in ["x", "y", "z"] {
        let id_v2 = v2_vec3.fields.iter().find(|f| f.name == name).map(|f| f.id);
        let id_v3 = v3_vec3.fields.iter().find(|f| f.name == name).map(|f| f.id);
        assert_eq!(id_v2, id_v3, "field '{name}' has different ID across Vector3 versions");
    }

    // w is new — should not exist in v2
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
    type Vec3 extends Vec3 {
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

    let v2_vec3 = schema.types.types.iter()
        .find(|(id, _)| id.name == "Vec3" && id.version == 2)
        .map(|(_, t)| t).unwrap();

    let v3_vec3 = schema.types.types.iter()
        .find(|(id, _)| id.name == "Vec3" && id.version == 3)
        .map(|(_, t)| t).unwrap();

    let x_id = v2_vec3.fields.iter().find(|f| f.name == "x").map(|f| f.id).unwrap();
    let pos_x_id = v3_vec3.fields.iter().find(|f| f.name == "pos_x").map(|f| f.id).unwrap();

    // renamed field should keep its ID
    assert_eq!(x_id, pos_x_id, "renamed field 'x -> pos_x' should retain its FieldId");
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
    type Vec3 extends Vec3 {
      x: float
      y: float
    }
    diff {}
  }
}
"#;
    // parse may succeed (fields look like valid tokens),
    // but analyzer must reject it
    if let Ok(ast) = parse_schema(input) {
        assert!(analyze_schema(&ast).is_err());
    }
}