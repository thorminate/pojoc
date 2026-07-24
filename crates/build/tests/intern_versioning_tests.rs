mod common;
use common::*;

/// regression test: intern table header must be scoped per version's own historical fields, not computed once from the latest version and applied to all
fn generate(src: &str) -> String {
    let ast = parse_schema(src).expect("parse failed");
    let resolved = analyze_schema(&ast).expect("analyze failed");
    pojoc_build::codegen::generate(&resolved)
}

#[test]
fn pre_intern_versions_get_no_table_header() {
    let code = generate(
        r#"
        schema Mini {
          version 1 {
            fields {
              name: string = ""
            }
          }
          version 2 {
            diff {
              + label: intern string = ""
            }
          }
        }
        "#,
    );

    let v1_decode = extract_fn(&code, "pub fn decode_v1");
    let v1_encode = extract_fn(&code, "pub fn encode_v1");
    assert!(
        !v1_decode.contains("read_intern_table"),
        "decode_v1 must not read an intern table — v1 predates `intern`:\n{v1_decode}"
    );
    assert!(
        !v1_encode.contains("write_intern_table") && !v1_encode.contains("InternBuilder"),
        "encode_v1 must not write an intern table — v1 predates `intern`:\n{v1_encode}"
    );

    let v2_decode = extract_fn(&code, "pub fn decode_v2");
    let v2_encode = extract_fn(&code, "pub fn encode_v2");
    assert!(
        v2_decode.contains("read_intern_table"),
        "decode_v2 must read an intern table — v2 introduces `intern`:\n{v2_decode}"
    );
    assert!(
        v2_encode.contains("write_intern_table") && v2_encode.contains("InternBuilder"),
        "encode_v2 must write an intern table — v2 introduces `intern`:\n{v2_encode}"
    );
}

#[test]
fn intern_removed_in_a_later_version_keeps_earlier_versions_tabled() {
    // mirror case: field interned at v1, dropped by v2 — re-encoding v1 must still emit a table
    let code = generate(
        r#"
        schema Mini {
          version 1 {
            fields {
              label: intern string = ""
            }
          }
          version 2 {
            diff {
              - label
              + other: string = ""
            }
          }
        }
        "#,
    );

    let v1_decode = extract_fn(&code, "pub fn decode_v1");
    let v1_encode = extract_fn(&code, "pub fn encode_v1");
    assert!(
        v1_decode.contains("read_intern_table"),
        "decode_v1 must still read a table — v1's own field was interned:\n{v1_decode}"
    );
    assert!(
        v1_encode.contains("write_intern_table"),
        "encode_v1 must still write a table — v1's own field was interned:\n{v1_encode}"
    );

    let v2_decode = extract_fn(&code, "pub fn decode_v2");
    assert!(
        !v2_decode.contains("read_intern_table"),
        "decode_v2 must not read a table — no field at v2 is interned:\n{v2_decode}"
    );
}

/// slices one top-level fn's source by signature, to the closing brace at column 0, so assertions target only that fn
fn extract_fn<'a>(code: &'a str, signature_prefix: &str) -> &'a str {
    let start = code
        .find(signature_prefix)
        .unwrap_or_else(|| panic!("function starting with `{signature_prefix}` not found"));
    let after_start = &code[start..];
    let end = after_start
        .find("\n}\n")
        .map(|i| i + 3)
        .unwrap_or(after_start.len());
    &after_start[..end]
}
