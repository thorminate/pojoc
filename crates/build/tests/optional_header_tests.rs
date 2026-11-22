mod common;
use common::*;

fn generate(src: &str) -> String {
    let ast = parse_schema(src).expect("parse failed");
    let resolved = analyze_schema(&ast).expect("analyze failed");
    pojoc_build::codegen::generate(&resolved)
}

/// regression test: an Optional whose inner type is zero-size emits no skip
/// statement, but encode still writes its presence bit and decode_vN still
/// counts it. skip_vN used to not count it, so every later optional's presence
/// was read from the wrong bit and the cursor desynced.
#[test]
fn zero_size_optional_still_consumes_a_presence_bit_when_skipping() {
    let code = generate(
        r#"
        schema Zopt {
          version 1 {
            fields {
              a: [i32](0)?
              b: u32?
            }
          }
        }
        "#,
    );

    let decode = extract_fn(&code, "pub fn decode_v1");
    let skip = extract_fn(&code, "pub fn skip_v1");

    // `a` is zero-size so only `b` produces a gated skip — it must be bit 1,
    // the same bit decode_v1 reads it from
    assert!(
        decode.contains("(1 << 1)"),
        "decode_v1 should read `b` from bit 1:\n{decode}"
    );
    assert!(
        skip.contains("(1 << 1)"),
        "skip_v1 must read `b` from bit 1, matching decode_v1:\n{skip}"
    );
    assert!(
        !skip.contains("(1 << 0)"),
        "skip_v1 must not reuse bit 0 for `b` — that bit belongs to the zero-size `a`:\n{skip}"
    );
}

#[test]
fn presence_bits_stay_aligned_across_a_byte_boundary() {
    let mut fields = String::new();
    for i in 0..10 {
        fields.push_str(&format!("      f{i}: u32?\n"));
    }
    let code = generate(&format!(
        "schema Many {{\n  version 1 {{\n    fields {{\n{fields}    }}\n  }}\n}}"
    ));

    let decode = extract_fn(&code, "pub fn decode_v1");
    let skip = extract_fn(&code, "pub fn skip_v1");

    // the 9th and 10th optionals live in the second header byte
    for probe in ["__header[1] & (1 << 0)", "__header[1] & (1 << 1)"] {
        assert!(
            decode.contains(probe),
            "decode_v1 missing {probe}:\n{decode}"
        );
        assert!(skip.contains(probe), "skip_v1 missing {probe}:\n{skip}");
    }
}

/// slices one top-level fn's source by signature, to the closing brace at column 0
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
