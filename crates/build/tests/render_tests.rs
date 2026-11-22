mod common;
use common::*;
use pojoc_build::schema::ir::ir_types::FieldIR;
use pojoc_build::schema::ir::ir_types::ResolvedSchema;
use pojoc_build::schema::render::{render_modifiers, render_type_ref};

/// what the CLI and LSP both compose out of the two renderers
fn signature(f: &FieldIR) -> String {
    format!(
        "{}: {}{}",
        f.name,
        render_modifiers(f),
        render_type_ref(&f.ty)
    )
}

fn resolve(src: &str) -> ResolvedSchema {
    let ast = parse_schema(src).expect("parse failed");
    analyze_schema(&ast).expect("analyze failed")
}

/// wraps each type in a field of the latest version, then renders it back
fn rendered_fields(fields_block: &str) -> Vec<String> {
    let schema = resolve(&format!(
        "schema R {{\n  version 1 {{\n    fields {{\n{fields_block}    }}\n  }}\n}}"
    ));
    schema
        .versions
        .last()
        .unwrap()
        .fields
        .iter()
        .map(signature)
        .collect()
}

#[test]
fn types_render_back_to_the_syntax_they_were_written_in() {
    let cases = [
        ("a: u32 = 0", "a: u32"),
        ("b: string = \"\"", "b: string"),
        ("c: string?", "c: string?"),
        ("d: [i32] = []", "d: [i32]"),
        ("e: [i32](4)", "e: [i32](4)"),
        ("f: [i32](delta) = []", "f: [i32](delta)"),
        ("g: [u8](delta, 8)", "g: [u8](delta, 8)"),
        ("h: string(16)", "h: string(16)"),
        ("i: map<string, i32> = {}", "i: map<string, i32>"),
        ("j: (u64, string)", "j: (u64, string)"),
        ("k: intern string = \"\"", "k: intern string"),
        ("l: u8(min: 0, max: 10) = 0", "l: u8(min: 0, max: 10)"),
        (
            "m: vfloat(min: 0, max: 1, step: 0.1) = 0.0",
            "m: vfloat(min: 0, max: 1, step: 0.1)",
        ),
    ];

    for (decl, expected) in cases {
        let block = format!("      {decl}\n");
        let got = rendered_fields(&block);
        assert_eq!(got[0], expected, "rendering `{decl}`");
    }
}

#[test]
fn modifiers_render_in_their_canonical_order() {
    let got = rendered_fields(
        "      a: seekable u32 = 0\n      b: lazy [u8] = []\n      c: seekable lazy [u8] = []\n",
    );
    assert_eq!(got[0], "a: seekable u32");
    assert_eq!(got[1], "b: lazy [u8]");
    assert_eq!(got[2], "c: seekable lazy [u8]");
}

#[test]
fn a_declared_type_renders_by_name() {
    let schema = resolve(
        r#"
        schema R {
          version 1 {
            type Inner { x: u32 = 0 }
            enum E { A, B }
            fields {
              a: Inner
              b: E
              c: box<Inner>?
            }
          }
        }
        "#,
    );
    let fields = &schema.versions.last().unwrap().fields;
    let rendered: Vec<String> = fields.iter().map(signature).collect();
    assert_eq!(rendered[0], "a: Inner");
    assert_eq!(rendered[1], "b: E");
    assert_eq!(rendered[2], "c: box<Inner>?");
}

/// what `pojoc show` relies on: the latest version reflects every applied diff,
/// not the shape any single version was declared with
#[test]
fn the_latest_version_reflects_applied_diffs() {
    let schema = resolve(
        r#"
        schema R {
          version 1 {
            fields {
              keep: u32 = 0
              drop_me: string = ""
              widen: u8 = 0
            }
          }
          version 2 {
            diff {
              - drop_me
              ~ widen: u64
              ~ keep -> renamed: u32
              + added: seekable string = ""
            }
          }
        }
        "#,
    );

    let rendered: Vec<String> = schema
        .versions
        .last()
        .unwrap()
        .fields
        .iter()
        .map(signature)
        .collect();

    assert!(
        !rendered.iter().any(|f| f.starts_with("drop_me")),
        "removed field still present: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|f| f == "renamed: u32"),
        "rename not applied: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|f| f == "widen: u64"),
        "retype not applied: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|f| f == "added: seekable string"),
        "added field missing: {rendered:?}"
    );
}

#[test]
fn a_union_payload_renders_by_its_type() {
    let schema = resolve(
        r#"
        schema R {
          version 1 {
            type P { x: u32 = 0 }
            union U {
              Struct: P
              Text: string
              Bounded: i32(min: 0, max: 10)
            }
            fields { u: U }
          }
        }
        "#,
    );
    let u = schema
        .unions
        .unions
        .iter()
        .max_by_key(|(id, _)| id.version)
        .map(|(_, u)| u)
        .expect("union not resolved");

    let payloads: Vec<String> = u
        .variants
        .iter()
        .map(|v| render_type_ref(&v.payload))
        .collect();
    assert!(payloads.contains(&"P".to_string()), "{payloads:?}");
    assert!(payloads.contains(&"string".to_string()), "{payloads:?}");
    assert!(
        payloads.contains(&"i32(min: 0, max: 10)".to_string()),
        "{payloads:?}"
    );
}
