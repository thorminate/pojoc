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
        !v1_encode.contains("encode_interned_v1") && !v1_encode.contains("InternBuilder"),
        "encode_v1 must not write an intern table — v1 predates `intern`:\n{v1_encode}"
    );

    let v2_decode = extract_fn(&code, "pub fn decode_v2");
    let v2_encode = extract_fn(&code, "pub fn encode_v2");
    let v2_interned = extract_fn(&code, "fn encode_interned_v2");
    assert!(
        v2_decode.contains("read_intern_table"),
        "decode_v2 must read an intern table — v2 introduces `intern`:\n{v2_decode}"
    );
    assert!(
        v2_encode.contains("encode_interned_v2(__buf, __value, &[])")
            && v2_interned.contains("write_interned_payload"),
        "encode_v2 must delegate to the table writer — v2 introduces `intern`:\n{v2_encode}\n{v2_interned}"
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
    let v1_interned = extract_fn(&code, "fn encode_interned_v1");
    assert!(
        v1_decode.contains("read_intern_table"),
        "decode_v1 must still read a table — v1's own field was interned:\n{v1_decode}"
    );
    assert!(
        v1_encode.contains("encode_interned_v1(__buf, __value, &[])")
            && v1_interned.contains("write_interned_payload"),
        "encode_v1 must still write a table — v1's own field was interned:\n{v1_encode}\n{v1_interned}"
    );

    let v2_decode = extract_fn(&code, "pub fn decode_v2");
    assert!(
        !v2_decode.contains("read_intern_table"),
        "decode_v2 must not read a table — no field at v2 is interned:\n{v2_decode}"
    );
}

/// a lazy field defers to a decode_fn with no table parameter, so anything it
/// defers must not reach the intern table. these two cases slipped past the
/// check and emitted generated code referencing an out-of-scope `__table`.
fn analysis_error(src: &str) -> String {
    let ast = parse_schema(src).expect("parse failed");
    match analyze_schema(&ast) {
        Ok(_) => panic!("schema was accepted, expected a `lazy`/`intern` rejection"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn lazy_field_of_a_struct_with_an_interned_field_is_rejected() {
    // the interning is reached through Inner's fields, not Holder's own type structure
    let err = analysis_error(
        r#"
        schema Holder {
          version 1 {
            type Inner {
              label: intern string = ""
            }

            fields {
              nested: lazy Inner
            }
          }
        }
        "#,
    );
    assert!(
        err.contains("'nested' cannot be both `lazy` and (transitively) interned"),
        "expected a lazy/intern rejection for 'nested', got: {err}"
    );
}

#[test]
fn diff_retype_to_lazy_intern_is_rejected() {
    // `~` sets lazy without going through the declaration-site check
    let err = analysis_error(
        r#"
        schema Holder {
          version 1 {
            fields {
              label: string = ""
            }
          }
          version 2 {
            diff {
              ~ label: lazy intern string
            }
          }
        }
        "#,
    );
    assert!(
        err.contains("'label' cannot be both `lazy` and (transitively) interned"),
        "expected a lazy/intern rejection for 'label', got: {err}"
    );
}

#[test]
fn diff_transform_to_lazy_intern_is_rejected() {
    let err = analysis_error(
        r#"
        schema Holder {
          version 1 {
            fields {
              label: string = ""
            }
          }
          version 2 {
            diff {
              ~ label -> tag: lazy intern string
            }
          }
        }
        "#,
    );
    assert!(
        err.contains("'tag' cannot be both `lazy` and (transitively) interned"),
        "expected a lazy/intern rejection for 'tag', got: {err}"
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
