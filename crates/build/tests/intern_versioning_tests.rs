mod common;
use common::*;

/// Regression coverage for a real bug: `intern_infected` used to be computed
/// once per type (from the *latest* version's fields only) and applied
/// uniformly to every version's `encode_vN`/`decode_vN`. That meant adding an
/// `intern` field in a later version retroactively put an intern-table
/// header on every earlier version's wire format too — breaking decode of
/// genuinely old data written before `intern` existed in the schema, which
/// is exactly the guarantee schema versioning exists to provide.
///
/// The fix scopes the intern-table header per version, based on that
/// version's own (historical) field types, not the latest struct's shape.
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
    // Mirror case: a field is interned at v1 but the field is dropped
    // entirely by v2. Re-encoding at v1 from the latest struct must still
    // emit a table (v1's own historical wire format used one), even though
    // the latest struct itself carries no interned data anymore.
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

/// Slices out a single top-level function's source by name, from its
/// signature line to the matching closing brace at column 0, so assertions
/// can target one `encode_vN`/`decode_vN` without being confused by sibling
/// functions containing similar substrings.
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
