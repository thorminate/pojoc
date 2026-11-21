mod common;
use common::*;
use pojoc_build::schema::{AnalysisError, SchemaError, SchemaWarning};

fn generate(src: &str) -> String {
    let ast = parse_schema(src).expect("parse failed");
    let resolved = analyze_schema(&ast).expect("analyze failed");
    pojoc_build::codegen::generate(&resolved)
}

fn analysis_error(src: &str) -> SchemaError {
    let ast = match parse_schema(src) {
        Ok(ast) => ast,
        Err(e) => return e,
    };
    match analyze_schema(&ast) {
        Ok(_) => panic!("schema was accepted, expected a rejection"),
        Err(e) => e,
    }
}

fn warnings(src: &str) -> Vec<SchemaWarning> {
    let ast = parse_schema(src).expect("parse failed");
    analyze_schema(&ast).expect("analyze failed").warnings
}

// ---- placement rules ----

#[test]
fn seekable_in_a_type_body_is_rejected() {
    let err = analysis_error(
        r#"
        schema S {
          version 1 {
            type Inner { x: seekable u32 = 0 }
            fields { a: Inner }
          }
        }
        "#,
    );
    assert!(
        matches!(
            err,
            SchemaError::Analysis(AnalysisError::SeekableOnlyOnRootFields { .. })
        ),
        "expected SeekableOnlyOnRootFields, got: {err}"
    );
}

#[test]
fn seekable_added_to_a_type_via_diff_is_rejected() {
    let err = analysis_error(
        r#"
        schema S {
          version 1 {
            type Inner { x: u32 = 0 }
            fields { a: Inner }
          }
          version 2 {
            type Inner extends Inner@1 {
              + y: seekable u32 = 0
            }
          }
        }
        "#,
    );
    assert!(
        matches!(
            err,
            SchemaError::Analysis(AnalysisError::SeekableOnlyOnRootFields { .. })
        ),
        "expected SeekableOnlyOnRootFields, got: {err}"
    );
}

#[test]
fn lazy_before_seekable_is_a_parse_error() {
    let err = analysis_error(
        r#"
        schema S {
          version 1 {
            fields { a: lazy seekable [u8] = [] }
          }
        }
        "#,
    );
    assert!(
        err.to_string()
            .contains("`seekable` must come before `lazy`"),
        "expected an ordering error, got: {err}"
    );
    // the canonical order parses
    generate(
        r#"
        schema S {
          version 1 {
            fields { a: seekable lazy [u8] = [] }
          }
        }
        "#,
    );
}

#[test]
fn const_and_seekable_together_are_rejected() {
    let err = analysis_error(
        r#"
        schema S {
          version 1 {
            fields { a: const seekable u32 = 1 }
          }
        }
        "#,
    );
    assert!(
        err.to_string()
            .contains("cannot be both `const` and `seekable`"),
        "expected a const/seekable rejection, got: {err}"
    );
}

// ---- generated shape ----

#[test]
fn a_schema_without_seekable_emits_no_seek_functions() {
    let code = generate(
        r#"
        schema Plain {
          version 1 {
            fields {
              a: u32 = 0
              b: string = ""
            }
          }
        }
        "#,
    );
    assert!(
        !code.contains("fn seek_"),
        "unmarked schemas must not pay for the feature:\n{code}"
    );
    assert!(
        !code.contains("PlainSeek"),
        "no batch struct either:\n{code}"
    );
}

#[test]
fn a_fixed_width_target_reads_at_a_constant_offset() {
    let code = generate(
        r#"
        schema S {
          version 1 {
            fields {
              lead: u32 = 0
              tag: seekable u32 = 0
              trail: string = ""
            }
          }
        }
        "#,
    );
    let seek = extract_fn(&code, "pub fn seek_v1_tag");
    assert!(
        seek.contains("_unchecked"),
        "a blastable target should use the unchecked read:\n{seek}"
    );
    assert!(
        !seek.contains("skip_"),
        "a blastable target should not skip anything:\n{seek}"
    );
    // lead occupies the first 4 bytes, so tag starts at 4
    assert!(
        seek.contains("*__pos += 4;"),
        "expected a constant offset of 4:\n{seek}"
    );
}

#[test]
fn a_variable_width_target_is_hoisted_ahead_of_earlier_fields() {
    let code = generate(
        r#"
        schema S {
          version 1 {
            fields {
              a: string = ""
              b: seekable string = ""
              c: string = ""
            }
          }
        }
        "#,
    );

    // b leads the tail on the wire, so reaching it skips nothing
    let seek = extract_fn(&code, "pub fn seek_v1_b");
    assert!(
        !seek.contains("skip_string"),
        "the hoisted target should be reached without skipping:\n{seek}"
    );

    // and encode agrees about that order
    let encode = extract_fn(&code, "pub fn encode_v1");
    let b_at = encode.find("__value.b").expect("b not written");
    let a_at = encode.find("__value.a").expect("a not written");
    assert!(b_at < a_at, "b must be written before a:\n{encode}");
}

#[test]
fn a_field_missing_from_a_version_returns_its_default_without_reading() {
    let code = generate(
        r#"
        schema S {
          version 1 {
            fields { a: u32 = 0 }
          }
          version 2 {
            diff { + tag: seekable u32 = 7 }
          }
        }
        "#,
    );
    let v1 = extract_fn(&code, "pub fn seek_v1_tag");
    assert!(v1.contains("Ok(7"), "expected the declared default:\n{v1}");
    assert!(
        !v1.contains("read_") && !v1.contains("__buf["),
        "a version predating the field must not touch the buffer:\n{v1}"
    );
}

#[test]
fn the_dispatcher_covers_every_version() {
    let code = generate(
        r#"
        schema S {
          version 1 { fields { tag: seekable u32 = 0 } }
          version 2 { diff { + extra: string = "" } }
          version 3 { diff { + more: string = "" } }
        }
        "#,
    );
    let dispatch = extract_fn(&code, "pub fn seek_tag");
    for v in 1..=3 {
        assert!(
            dispatch.contains(&format!("{v} => seek_v{v}_tag")),
            "dispatcher missing v{v}:\n{dispatch}"
        );
    }
    assert!(
        dispatch.contains("UnsupportedVersion"),
        "dispatcher needs an unknown-version arm:\n{dispatch}"
    );
}

#[test]
fn a_seekable_string_getter_is_lifetime_infected_independently_of_the_root() {
    let code = generate(
        r#"
        schema S {
          version 1 {
            fields {
              n: seekable u32 = 0
              s: seekable string = ""
            }
          }
        }
        "#,
    );
    // the root type is not itself infected here, but the getter still borrows
    // the buffer. `__start: usize` leaves one input lifetime, so it elides.
    let seek = extract_fn(&code, "pub fn seek_v1_s");
    assert!(
        seek.contains("PojocResult<&'_ str>"),
        "a borrowed-string getter must tie its result to the buffer:\n{seek}"
    );
    let plain = extract_fn(&code, "pub fn seek_v1_n");
    let sig = plain.lines().next().unwrap();
    assert!(
        !sig.contains('\''),
        "a plain u32 getter needs no lifetime at all:\n{sig}"
    );
}

// ---- intern: the three branches ----

#[test]
fn a_target_that_ignores_the_table_only_skips_it() {
    let code = generate(
        r#"
        schema S {
          version 1 {
            fields {
              label: intern string = ""
              tag: seekable u32 = 0
            }
          }
        }
        "#,
    );
    let seek = extract_fn(&code, "pub fn seek_v1_tag");
    assert!(
        seek.contains("skip_intern_table"),
        "should skip the table without building it:\n{seek}"
    );
    assert!(
        !seek.contains("read_intern_table"),
        "should not materialize the table:\n{seek}"
    );
}

#[test]
fn a_bare_interned_target_resolves_one_index_without_allocating() {
    let code = generate(
        r#"
        schema S {
          version 1 {
            fields { label: seekable intern string = "" }
          }
        }
        "#,
    );
    let seek = extract_fn(&code, "pub fn seek_v1_label");
    assert!(
        seek.contains("read_interned_string_at"),
        "expected the targeted lookup:\n{seek}"
    );
    assert!(
        !seek.contains("read_intern_table"),
        "the fast path must not materialize the table:\n{seek}"
    );
}

#[test]
fn a_containered_interned_target_falls_back_to_the_whole_table() {
    let src = r#"
        schema S {
          version 1 {
            fields { tags: seekable [intern string] = [] }
          }
        }
        "#;
    let seek = {
        let code = generate(src);
        extract_fn(&code, "pub fn seek_v1_tags").to_string()
    };
    assert!(
        seek.contains("read_intern_table"),
        "nested readers need the slice:\n{seek}"
    );

    let ws = warnings(src);
    assert!(
        ws.iter()
            .any(|w| matches!(w, SchemaWarning::SeekMaterializesInternTable { field, .. } if field == "tags")),
        "the fallback should warn about its cost, got: {ws:?}"
    );
}

#[test]
fn the_intern_fast_path_does_not_warn() {
    let ws = warnings(
        r#"
        schema S {
          version 1 {
            fields { label: seekable intern string = "" }
          }
        }
        "#,
    );
    assert!(
        ws.is_empty(),
        "a bare seekable intern string costs nothing extra: {ws:?}"
    );
}

// ---- toggling across versions ----

#[test]
fn dropping_seekable_in_a_diff_warns_and_removes_the_accessors() {
    let src = r#"
        schema S {
          version 1 {
            fields { tag: seekable u32 = 0 }
          }
          version 2 {
            diff { ~ tag: u32 }
          }
        }
        "#;

    let ws = warnings(src);
    assert!(
        ws.iter().any(|w| matches!(
            w,
            SchemaWarning::SeekableDroppedByDiff { field, was, version, .. }
                if field == "tag" && *was == 1 && *version == 2
        )),
        "expected a dropped-seekable warning, got: {ws:?}"
    );

    let code = generate(src);
    assert!(
        !code.contains("fn seek_v1_tag") && !code.contains("fn seek_tag"),
        "the latest version no longer marks it, so no accessors:\n{code}"
    );
}

#[test]
fn restating_seekable_in_a_diff_keeps_it_without_warning() {
    let src = r#"
        schema S {
          version 1 {
            fields { tag: seekable u32 = 0 }
          }
          version 2 {
            diff { ~ tag: seekable u64 }
          }
        }
        "#;
    assert!(warnings(src).is_empty(), "restating it is not a drop");

    let code = generate(src);
    assert!(code.contains("fn seek_v1_tag"));
    assert!(code.contains("fn seek_v2_tag"));
}

#[test]
fn marking_a_field_seekable_later_leaves_earlier_versions_laid_out_as_before() {
    let before = generate(
        r#"
        schema S {
          version 1 {
            fields {
              a: string = ""
              b: string = ""
            }
          }
        }
        "#,
    );
    let after = generate(
        r#"
        schema S {
          version 1 {
            fields {
              a: string = ""
              b: string = ""
            }
          }
          version 2 {
            diff { ~ b: seekable string }
          }
        }
        "#,
    );

    // v1 predates the marking, so its wire order must be untouched
    assert_eq!(
        extract_fn(&before, "pub fn encode_v1"),
        extract_fn(&after, "pub fn encode_v1"),
        "marking a field seekable in v2 relayouted v1"
    );
}

/// regression test: an imported schema's warnings used to be dropped on the
/// floor, so a shared schema could carry a hidden per-seek cost that no root
/// ever reported. They now aggregate, each still rendering against its own file.
#[test]
fn warnings_from_imported_schemas_reach_the_root() {
    use pojoc_build::schema::ImportOrchestrator;

    let dir = std::env::temp_dir().join("pojoc_import_warning_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    std::fs::write(
        dir.join("inner.pojoc"),
        "schema Inner {\n  version 1 {\n    fields { tags: seekable [intern string] = [] }\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("outer.pojoc"),
        "schema Outer {\n  import \"inner.pojoc\" as Inner\n  version 1 {\n    fields { child: Inner@1 }\n  }\n}\n",
    )
    .unwrap();

    let outer = dir.join("outer.pojoc");
    let resolved = ImportOrchestrator::new()
        .resolve_root(&outer)
        .expect("resolve failed");

    assert!(
        resolved.warnings.is_empty(),
        "the root itself declares nothing warnable"
    );

    let all = resolved.warnings_recursive(&outer);
    assert_eq!(all.len(), 1, "expected the import's warning, got {all:?}");

    let (w, path) = all[0];
    assert!(
        matches!(w, SchemaWarning::SeekMaterializesInternTable { field, .. } if field == "tags"),
        "unexpected warning: {w:?}"
    );
    assert!(
        path.ends_with("inner.pojoc"),
        "must render against the file that declared it, got {}",
        path.display()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// regression test: an imported root that borrows the buffer used to infect only
/// the *rendered* type (`inner::Inner<'buf>`) and not the types holding it, so a
/// root whose sole borrow came through an import declared no `'buf` to bind it.
/// Nothing caught it because every schema in the suite already borrows on its own.
#[test]
fn an_imported_root_that_borrows_infects_the_types_holding_it() {
    use pojoc_build::codegen::generate;
    use pojoc_build::schema::ImportOrchestrator;

    let dir = std::env::temp_dir().join("pojoc_import_lifetime_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Inner borrows (a bare `string`); Outer has no borrow of its own, so the
    // only path to `'buf` is through the imported field
    std::fs::write(
        dir.join("inner.pojoc"),
        "schema Inner {\n  version 1 {\n    fields { name: string = \"\" }\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("outer.pojoc"),
        "schema Outer {\n  import \"inner.pojoc\" as Sub\n  version 1 {\n    fields {\n      id: u32 = 0\n      child: seekable Sub@1\n    }\n  }\n}\n",
    )
    .unwrap();

    let outer = dir.join("outer.pojoc");
    let resolved = ImportOrchestrator::new()
        .resolve_root(&outer)
        .expect("resolve failed");
    let code = generate(&resolved);

    assert!(
        code.contains("pub struct Outer<'buf> {"),
        "root struct holding an imported borrowing root must declare 'buf"
    );
    assert!(
        code.contains("pub struct OuterSeek<'buf> {"),
        "seek struct holding an imported borrowing root must declare 'buf"
    );
    // the accessors elide rather than name it, but they must still be lifetimed
    assert!(
        code.contains(
            "pub fn seek_v1_child(__buf: &[u8], __start: usize) -> PojocResult<sub::Inner<'_>>"
        ),
        "seek accessor must borrow the buffer, got:\n{}",
        extract_fn(&code, "pub fn seek_v1_child")
    );
    let _ = std::fs::remove_dir_all(&dir);
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

/// generated code lands in a consuming crate's lint scope, so a target at the
/// very start of the blast region must not emit `__blast_start + 0`
#[test]
fn a_zero_offset_blast_target_emits_no_identity_add() {
    let code = generate(
        r#"
        schema S {
          version 1 {
            fields {
              tag: seekable u32 = 0
              other: u32 = 0
            }
          }
        }
        "#,
    );
    assert!(
        !code.contains("__blast_start + 0"),
        "identity add reached the generated output:\n{}",
        extract_fn(&code, "pub fn seek_all_v1")
    );
    assert!(
        code.contains("*__pos = __blast_start;"),
        "expected the bare assignment for a zero offset:\n{}",
        extract_fn(&code, "pub fn seek_all_v1")
    );
}

// ---- subset reader ----

#[test]
fn the_subset_walk_guards_every_target_and_exits_after_the_last_requested() {
    let code = generate(
        r#"
        schema S {
          version 1 {
            fields {
              n: seekable u32 = 0
              a: seekable string = ""
              b: seekable string = ""
              c: seekable string = ""
              bulk: [u8] = []
            }
          }
        }
        "#,
    );

    // one index per target, in latest-version declaration order
    for (i, f) in ["n", "a", "b", "c"].iter().enumerate() {
        assert!(
            code.contains(&format!("pub const {f}: usize = {i};")),
            "missing mask index for {f}:\n{code}"
        );
    }
    // the mask API is an implementation detail of the macro
    assert!(code.contains("#[doc(hidden)]"));
    assert!(
        !code.contains("pub fn seek_subset"),
        "the subset walk should not be part of the public surface:\n{code}"
    );
    assert!(
        code.contains("macro_rules! pluck_s"),
        "the schema-qualified pluck macro should be emitted:\n{code}"
    );

    let f = extract_fn(&code, "pub fn __pluck_v1");
    // an unrequested tail field is stepped over, not read
    assert!(
        f.contains("if M0 & (1u128 << 1) != 0 {") && f.contains("skip_string(__buf, __pos)?;"),
        "each tail target needs a read/skip pair:\n{f}"
    );
    // the walk gives up as soon as nothing later is wanted
    assert!(
        f.contains("if M0 & ((1u128 << 2) | (1u128 << 3)) == 0 { break; }"),
        "expected an early exit keyed on the remaining targets:\n{f}"
    );
    // unrequested fields stay None rather than picking up a default
    assert!(
        f.contains("let mut __out_a = None;"),
        "targets start unbound:\n{f}"
    );
}

#[test]
fn a_requested_field_the_version_predates_still_gets_its_default() {
    let code = generate(
        r#"
        schema S {
          version 1 { fields { a: seekable u32 = 0 } }
          version 2 { diff { + b: seekable u32 = 9 } }
        }
        "#,
    );
    let v1 = extract_fn(&code, "pub fn __pluck_v1");
    // None must mean "not requested", never "missing in this version"
    assert!(
        v1.contains("if M0 & (1u128 << 1) != 0 { __out_b = Some(9"),
        "a requested-but-absent field should carry the version default:\n{v1}"
    );
}

#[test]
fn a_schema_without_seekable_emits_no_subset_reader() {
    let code = generate(
        r#"
        schema Plain {
          version 1 { fields { a: u32 = 0 } }
        }
        "#,
    );
    assert!(!code.contains("__pluck"));
    assert!(!code.contains("macro_rules! pluck"));
}
