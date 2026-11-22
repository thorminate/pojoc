//! Partial reads: accessors that pull one `seekable` field out of an encoded
//! payload without decoding the rest.
//!
//! A target in the fixed block sits at a constant offset from its start.
//! Reaching it takes one read after a bounds check. Anything else is
//! reached by skipping the tail field-by-field, which `wire_rank` keeps short by
//! hoisting seekable fields to the front of that tail.
//!
//! Accessors are emitted for every version, whatever that version's rank for the
//! field is — a version where it was not yet marked simply skips more. The hoist
//! is an optimization, not a correctness precondition.
//!
//! Every accessor produces exactly what `decode_vN` would have put in the struct,
//! including a version's cast, its missing-field default, and its constraint
//! checks. That agreement is the invariant the whole feature rests on.

use super::decode::{
    emit_field_mapping_arm, emit_field_read, emit_fixed_field_read, emit_optional_header_read,
    emit_ordered_skip, get_field_default_expr,
};
use super::writer::CodeWriter;
use super::{
    emit_constraint_guard, field_is_lifetime_infected, is_fixed_wire_field, ordered_wire_fields,
    version_uses_intern, wire_fixed_size, wire_rank,
};
use crate::core::types::{ResolvedTypeRef, type_info};
use crate::schema::ir::ir_types::{FieldIR, ResolvedSchema, type_reaches_intern_table};
use crate::schema::ir::lineage::{FieldLineage, FieldMapping, VersionLineage};
use heck::{ToSnakeCase, ToUpperCamelCase};
use std::collections::HashSet;

pub fn emit_seek_functions(
    schema: &ResolvedSchema,
    infected: &HashSet<String>,
    export_pluck: bool,
    w: &mut CodeWriter,
) {
    let targets = seek_targets(schema);
    if targets.is_empty() {
        return;
    }

    for target in &targets {
        if let Some(alias) = seek_type_alias(target) {
            let ty = raw_seek_type(target);
            let lifetime = if ty.contains("'buf") { "<'buf>" } else { "" };
            w.line("#[doc(hidden)]");
            w.line(&format!("pub type {alias}{lifetime} = {ty};"));
            w.blank();
        }
    }

    for target in &targets {
        for vl in &schema.lineage.versions {
            emit_seek_vn_field(schema, vl, target, infected, w);
            w.blank();
        }
        emit_seek_dispatcher(schema, target, infected, w);
        w.blank();
    }

    emit_seek_all(schema, &targets, infected, w);

    if export_pluck {
        emit_seek_subset(schema, &targets, infected, w);
    }
}

// the latest version names the accessors, so a field that stops being seekable
// there loses them even where an older version still hoists it
fn seek_targets(schema: &ResolvedSchema) -> Vec<&FieldIR> {
    schema
        .versions
        .last()
        .map(|v| v.fields.iter().filter(|f| f.seekable).collect())
        .unwrap_or_default()
}

// the lineage entry feeding a latest-version field, or none when this version
// predates it (the value then lives in vl.missing)
fn target_name_of(fl: &FieldLineage) -> Option<&str> {
    match &fl.mapping {
        FieldMapping::PassThrough { target_name } | FieldMapping::Cast { target_name, .. } => {
            Some(target_name)
        }
        FieldMapping::Discard => None,
    }
}

fn lineage_for<'a>(vl: &'a VersionLineage, target: &str) -> Option<&'a FieldLineage> {
    vl.fields.iter().find(|fl| match &fl.mapping {
        FieldMapping::PassThrough { target_name } | FieldMapping::Cast { target_name, .. } => {
            target_name == target
        }
        FieldMapping::Discard => false,
    })
}

fn seek_type_alias(target: &FieldIR) -> Option<String> {
    // Keep repeated signatures readable when the field contains several
    // nested types. Plain fields still expose their types directly.
    let mut nodes = 0;
    target.ty.for_each_node(&mut |_| nodes += 1);
    if nodes < 5 {
        return None;
    }
    let name = target.name.to_upper_camel_case();
    let id = target.id.0;
    Some(format!("__Seek{name}Field{id}"))
}

fn raw_seek_type(target: &FieldIR) -> String {
    if target.lazy {
        format!("LazyView<'buf, {}>", type_info(&target.ty).rust_type)
    } else {
        type_info(&target.ty).rust_type
    }
}

fn rust_type_of(target: &FieldIR) -> String {
    let ty = raw_seek_type(target);
    match seek_type_alias(target) {
        Some(alias) if ty.contains("'buf") => format!("{alias}<'buf>"),
        Some(alias) => alias,
        None => ty,
    }
}

// `__start: usize` leaves the buffer as the only reference parameter, so the
// output lifetime elides — naming it would just draw clippy::needless_lifetimes
// in the generated crate
fn seek_return_type(target: &FieldIR, infected: &HashSet<String>) -> String {
    let ty = rust_type_of(target);
    if target.lazy || field_is_lifetime_infected(&target.ty, target.lazy, infected) {
        ty.replace("'buf", "'_")
    } else {
        ty
    }
}

// An unchanged bare interned field can resolve one index without building
// the table. Retypes go through the ordinary reader so it applies the cast.
fn can_read_interned_directly(field: &FieldLineage) -> bool {
    matches!(field.source_ty, ResolvedTypeRef::Interned(_))
        && matches!(field.mapping, FieldMapping::PassThrough { .. })
}

/// emits the cursor, intern-table handling and optional header. leaves `__pos`
/// positioned at the start of the fixed block.
fn emit_prologue(
    schema: &ResolvedSchema,
    vl: &VersionLineage,
    needs_table: bool,
    table_fast_path: bool,
    w: &mut CodeWriter,
) {
    w.line("let mut __cursor = __start;");
    w.line("let __pos = &mut __cursor;");

    if version_uses_intern(schema, &vl.fields) {
        if table_fast_path {
            w.line("let __table_start = *__pos;");
            w.line("skip_intern_table(__buf, __pos)?;");
        } else if needs_table {
            w.line("let __table_owned = read_intern_table(__buf, __pos)?;");
            w.line("let __table = __table_owned.as_slice();");
        } else {
            w.line("skip_intern_table(__buf, __pos)?;");
        }
    }

    let optional_count = vl
        .fields
        .iter()
        .filter(|fl| matches!(fl.source_ty, ResolvedTypeRef::Optional(_)))
        .count();
    emit_optional_header_read(optional_count, w);
}

fn blast_split(vl: &VersionLineage) -> (Vec<&FieldLineage>, Vec<&FieldLineage>) {
    let ordered = ordered_wire_fields(vl);
    let split = ordered.partition_point(|fl| is_fixed_wire_field(fl));
    let (blast, rest) = ordered.split_at(split);
    (blast.to_vec(), rest.to_vec())
}

fn fixed_total(blast: &[&FieldLineage]) -> usize {
    blast
        .iter()
        .map(|fl| wire_fixed_size(&fl.source_ty).unwrap_or(0))
        .sum()
}

fn emit_seek_vn_field(
    schema: &ResolvedSchema,
    vl: &VersionLineage,
    target: &FieldIR,
    infected: &HashSet<String>,
    w: &mut CodeWriter,
) {
    let v = vl.version;
    let name = &target.name;
    let ret = seek_return_type(target, infected);

    let found = lineage_for(vl, name);

    // __start rather than &mut usize: a seek does not consume the block, so
    // handing back a cursor would only invite misuse
    w.line("#[allow(dead_code)]");
    w.line(&format!(
        "pub fn seek_v{v}_{name}(__buf: &[u8], __start: usize) -> PojocResult<{ret}> {{"
    ));
    w.indent();

    match found {
        // this version predates the field: same default decode_vN would produce,
        // read from nothing
        None => {
            let missing = vl.missing.iter().find(|m| m.target_name == *name);
            let expr = match missing {
                Some(m) if target.lazy => {
                    format!("LazyView::Owned({})", type_info(&m.ty).default_expr)
                }
                Some(m) => get_field_default_expr(m.default.as_ref(), &m.ty, schema),
                None if target.lazy => {
                    format!("LazyView::Owned({})", type_info(&target.ty).default_expr)
                }
                None => type_info(&target.ty).default_expr,
            };
            w.line(&format!("Ok({expr})"));
        }
        Some(fl) => {
            let fast = can_read_interned_directly(fl);
            let needs_table = type_reaches_intern_table(
                &schema.types,
                &schema.unions,
                &fl.source_ty,
                &mut HashSet::new(),
            );
            emit_prologue(schema, vl, needs_table, fast && needs_table, w);

            let (blast, rest) = blast_split(vl);

            if wire_rank(fl) == 0 {
                emit_fixed_target_read(schema, &blast, fl, w);
            } else {
                let total = fixed_total(&blast);
                if total > 0 {
                    w.line(&format!(
                        "{{ let __end = __pos.checked_add({total}).ok_or(Error::UnexpectedEof)?; \
                         if __end > __buf.len() {{ return Err(Error::UnexpectedEof); }} *__pos = __end; }}"
                    ));
                }
                emit_tail_walk_to(schema, &rest, fl, name, fast, w);
            }

            emit_constraint_guard(target, name, w);
            w.line(&format!("Ok({name})"));
        }
    }

    w.dedent();
    w.line("}");
}

// rank 0: every preceding blastable field is fixed-width, so the target's offset
// is a constant and no presence bits intervene
fn emit_fixed_target_read(
    schema: &ResolvedSchema,
    blast: &[&FieldLineage],
    target_fl: &FieldLineage,
    w: &mut CodeWriter,
) {
    let offset: usize = blast
        .iter()
        .take_while(|fl| fl.field_id != target_fl.field_id)
        .map(|fl| wire_fixed_size(&fl.source_ty).unwrap_or(0))
        .sum();
    let size = wire_fixed_size(&target_fl.source_ty).unwrap_or(0);

    w.line(&format!(
        "let __end = __pos.checked_add({}).ok_or(Error::UnexpectedEof)?;",
        offset + size
    ));
    w.line("if __end > __buf.len() { return Err(Error::UnexpectedEof); }");
    if offset > 0 {
        w.line(&format!("*__pos += {offset};"));
    }
    w.line("// SAFETY: the check above validated the target's whole extent.");

    emit_fixed_field_read(schema, target_fl, w);
}

// skips the tail up to the target, then reads it exactly as decode_vN would.
// the optional counter must advance over every optional passed, since that is
// how encode assigned the presence bits.
fn emit_tail_walk_to(
    schema: &ResolvedSchema,
    rest: &[&FieldLineage],
    target_fl: &FieldLineage,
    name: &str,
    fast_intern: bool,
    w: &mut CodeWriter,
) {
    let mut optional_counter = 0usize;

    for fl in rest {
        if fl.field_id == target_fl.field_id {
            break;
        }
        emit_ordered_skip(fl, &mut optional_counter, w);
    }

    if fast_intern {
        emit_bare_interned_read(name, w);
        return;
    }

    if let ResolvedTypeRef::Optional(inner) = &target_fl.source_ty {
        let byte_idx = optional_counter / 8;
        let bit_idx = optional_counter % 8;
        w.line(&format!(
            "let __present = (__header[{byte_idx}] & (1 << {bit_idx})) != 0;"
        ));
        emit_field_mapping_arm(schema, target_fl, inner, "__present", w);
    } else {
        emit_field_read(schema, target_fl, w);
    }
}

// Resolve the field's varint index against the table we skipped.
fn emit_bare_interned_read(name: &str, w: &mut CodeWriter) {
    w.line(&format!(
        "let {name} = read_interned_string_at(__buf, __table_start, read_array_len(__buf, __pos)?)?;"
    ));
}

fn emit_seek_dispatcher(
    schema: &ResolvedSchema,
    target: &FieldIR,
    infected: &HashSet<String>,
    w: &mut CodeWriter,
) {
    let name = &target.name;
    let ret = seek_return_type(target, infected);

    w.line("#[allow(dead_code)]");
    w.line(&format!(
        "pub fn seek_{name}(buf: &[u8]) -> PojocResult<{ret}> {{"
    ));
    w.indent();
    w.line("let mut pos = 0;");
    w.line("let envelope = read_envelope(buf, &mut pos)?;");
    w.line("match envelope.version {");
    w.indent();
    for vl in &schema.lineage.versions {
        let v = vl.version;
        w.line(&format!("{v} => seek_v{v}_{name}(envelope.payload, 0),"));
    }
    w.line("v => Err(Error::UnsupportedVersion(v)),");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
}

// one walk binding every seekable field, so pulling several does not re-skip
// from the start per field
fn emit_seek_all(
    schema: &ResolvedSchema,
    targets: &[&FieldIR],
    infected: &HashSet<String>,
    w: &mut CodeWriter,
) {
    let name = &schema.name_hint;
    let struct_name = format!("{name}Seek");
    let needs_lt = targets
        .iter()
        .any(|t| t.lazy || field_is_lifetime_infected(&t.ty, t.lazy, infected));
    let (struct_lt, ret_lt) = if needs_lt {
        ("<'buf>", "<'_>")
    } else {
        ("", "")
    };

    w.line("#[derive(Debug, Clone)]");
    w.line(&format!("pub struct {struct_name}{struct_lt} {{"));
    w.indent();
    for t in targets {
        w.line(&format!("pub {}: {},", t.name, rust_type_of(t)));
    }
    w.dedent();
    w.line("}");
    w.blank();

    for vl in &schema.lineage.versions {
        emit_seek_all_vn(schema, vl, targets, &struct_name, ret_lt, w);
        w.blank();
    }

    w.line("#[allow(dead_code)]");
    w.line(&format!(
        "pub fn seek_all(buf: &[u8]) -> PojocResult<{struct_name}{ret_lt}> {{"
    ));
    w.indent();
    w.line("let mut pos = 0;");
    w.line("let envelope = read_envelope(buf, &mut pos)?;");
    w.line("match envelope.version {");
    w.indent();
    for vl in &schema.lineage.versions {
        let v = vl.version;
        w.line(&format!("{v} => seek_all_v{v}(envelope.payload, 0),"));
    }
    w.line("v => Err(Error::UnsupportedVersion(v)),");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
}

fn emit_seek_all_vn(
    schema: &ResolvedSchema,
    vl: &VersionLineage,
    targets: &[&FieldIR],
    struct_name: &str,
    ret_lt: &str,
    w: &mut CodeWriter,
) {
    let v = vl.version;

    w.line("#[allow(dead_code)]");
    w.line(&format!(
        "pub fn seek_all_v{v}(__buf: &[u8], __start: usize) -> PojocResult<{struct_name}{ret_lt}> {{"
    ));
    w.indent();

    let present: Vec<(&&FieldIR, &FieldLineage)> = targets
        .iter()
        .filter_map(|t| lineage_for(vl, &t.name).map(|fl| (t, fl)))
        .collect();

    // targets that reach the table at all, and whether every one of them is a
    // bare `intern string`. if so the walk resolves each by index and never
    // materializes the table, matching what the per-field accessors do; a single
    // container or named-type target forces the slice for all of them.
    let table_users: Vec<&FieldLineage> = present
        .iter()
        .map(|(_, fl)| *fl)
        .filter(|fl| {
            type_reaches_intern_table(
                &schema.types,
                &schema.unions,
                &fl.source_ty,
                &mut HashSet::new(),
            )
        })
        .collect();
    let needs_table = !table_users.is_empty();
    let fast_intern = needs_table && table_users.iter().all(|fl| can_read_interned_directly(fl));
    emit_prologue(schema, vl, needs_table, fast_intern, w);

    let (blast, rest) = blast_split(vl);

    // rank 0 targets: constant offsets into the fixed block, read in place
    let blast_ids: HashSet<_> = present
        .iter()
        .filter(|(_, fl)| wire_rank(fl) == 0)
        .map(|(_, fl)| fl.field_id)
        .collect();
    if !blast_ids.is_empty() {
        w.line("let __blast_start = *__pos;");
    }
    let mut offset = 0usize;
    for fl in &blast {
        let size = wire_fixed_size(&fl.source_ty).unwrap_or(0);
        if blast_ids.contains(&fl.field_id) {
            w.line(&format!(
                "let __end = __blast_start.checked_add({}).ok_or(Error::UnexpectedEof)?;",
                offset + size
            ));
            w.line("if __end > __buf.len() { return Err(Error::UnexpectedEof); }");
            // the first target sits at the block start; `+ 0` would trip
            // clippy::identity_op in the consuming crate
            if offset == 0 {
                w.line("*__pos = __blast_start;");
            } else {
                w.line(&format!("*__pos = __blast_start + {offset};"));
            }
            w.line("// SAFETY: checked directly above.");
            emit_fixed_field_read(schema, fl, w);
        }
        offset += size;
    }

    let total = fixed_total(&blast);
    if total > 0 {
        w.line(&format!(
            "{{ let __end = {}.checked_add({total}).ok_or(Error::UnexpectedEof)?; \
             if __end > __buf.len() {{ return Err(Error::UnexpectedEof); }} *__pos = __end; }}",
            // __pos auto-derefs for the method call; a leading `*` here would
            // bind to the whole call expression instead
            if blast_ids.is_empty() {
                "__pos"
            } else {
                "__blast_start"
            }
        ));
    }

    // one pass over the tail, binding targets as they go by. stops after the
    // last one, so rank 2 fields past it are never touched.
    let tail_ids: HashSet<_> = present
        .iter()
        .filter(|(_, fl)| wire_rank(fl) != 0)
        .map(|(_, fl)| fl.field_id)
        .collect();
    let last_needed = rest
        .iter()
        .rposition(|fl| tail_ids.contains(&fl.field_id))
        .map(|i| i + 1)
        .unwrap_or(0);

    let mut optional_counter = 0usize;
    for fl in rest.iter().take(last_needed) {
        if tail_ids.contains(&fl.field_id) {
            if fast_intern && can_read_interned_directly(fl) {
                let name = target_name_of(fl).expect("tail target has a mapping");
                emit_bare_interned_read(name, w);
            } else if let ResolvedTypeRef::Optional(inner) = &fl.source_ty {
                let byte_idx = optional_counter / 8;
                let bit_idx = optional_counter % 8;
                optional_counter += 1;
                w.line(&format!(
                    "let __present = (__header[{byte_idx}] & (1 << {bit_idx})) != 0;"
                ));
                emit_field_mapping_arm(schema, fl, inner, "__present", w);
            } else {
                emit_field_read(schema, fl, w);
            }
        } else {
            emit_ordered_skip(fl, &mut optional_counter, w);
        }
    }

    // targets this version predates
    for t in targets {
        if lineage_for(vl, &t.name).is_some() {
            continue;
        }
        let missing = vl.missing.iter().find(|m| m.target_name == t.name);
        let expr = match missing {
            Some(m) if t.lazy => format!("LazyView::Owned({})", type_info(&m.ty).default_expr),
            Some(m) => get_field_default_expr(m.default.as_ref(), &m.ty, schema),
            None if t.lazy => format!("LazyView::Owned({})", type_info(&t.ty).default_expr),
            None => type_info(&t.ty).default_expr,
        };
        w.line(&format!("let {} = {expr};", t.name));
    }

    for t in targets {
        emit_constraint_guard(t, &t.name, w);
    }

    w.line(&format!("Ok({struct_name} {{"));
    w.indent();
    for t in targets {
        w.line(&format!("{},", t.name));
    }
    w.dedent();
    w.line("})");
    w.dedent();
    w.line("}");
}

// ---- pluck: reading a named subset of the targets in one walk ----

/// mask words this schema needs; the width is sized per schema so the number of
/// seekable fields is unbounded, rather than capped by one integer's width
fn mask_words(target_count: usize) -> usize {
    target_count.div_ceil(128).max(1)
}

/// shift-free for bit 0, which would otherwise draw clippy::identity_op in the
/// consuming crate
fn bit_literal(bit: usize) -> String {
    if bit == 0 {
        "1u128".to_string()
    } else {
        // parenthesised so the emitted test reads unambiguously next to `&`
        format!("(1u128 << {bit})")
    }
}

/// `M{word} & (1 << bit) != 0` for one target, folded away at monomorphisation
fn bit_test(index: usize) -> String {
    format!("M{} & {} != 0", index / 128, bit_literal(index % 128))
}

/// true if *none* of these targets is requested — the early-exit condition,
/// written in its already-minimal form rather than as `!(any)`, which would draw
/// clippy::nonminimal_bool. Grouped per mask word so a schema spanning several
/// words still folds to a constant.
fn no_bit_test(indices: &[usize]) -> String {
    let mut per_word: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in indices {
        per_word.entry(i / 128).or_default().push(i % 128);
    }
    per_word
        .into_iter()
        .map(|(word, bits)| {
            let ors: Vec<String> = bits.iter().map(|b| bit_literal(*b)).collect();
            // a lone bit is already parenthesised by bit_literal
            let group = if ors.len() == 1 {
                ors[0].clone()
            } else {
                format!("({})", ors.join(" | "))
            };
            format!("M{word} & {group} == 0")
        })
        .collect::<Vec<_>>()
        .join(" && ")
}

/// The subset walk, plus the `pluck!` macro that is its only intended entry
/// point. `seek_all` has to reach the *last* seekable field before it can
/// return, so on a schema with many targets it costs far more than the few a
/// caller wanted. This reads exactly the named fields: unrequested ones are
/// skipped rather than read, and the walk returns as soon as the last requested
/// one is bound.
///
/// The mask is a const generic so every test folds at monomorphisation — a
/// request for fixed-width fields only never enters the variable region at all.
/// It is `#[doc(hidden)]` and underscore-named because nobody should write one
/// by hand; `pluck!` builds it from field names.
fn emit_seek_subset(
    schema: &ResolvedSchema,
    targets: &[&FieldIR],
    infected: &HashSet<String>,
    w: &mut CodeWriter,
) {
    let words = mask_words(targets.len());
    let params: Vec<String> = (0..words).map(|i| format!("const M{i}: u128")).collect();
    let params = params.join(", ");
    let needs_lt = targets
        .iter()
        .any(|t| t.lazy || field_is_lifetime_infected(&t.ty, t.lazy, infected));
    let (struct_lt, ret_lt) = if needs_lt {
        ("<'buf>", "<'_>")
    } else {
        ("", "")
    };

    w.line("#[doc(hidden)]");
    w.line("#[allow(dead_code, non_upper_case_globals)]");
    w.line("pub mod __pluck_mask {");
    w.indent();
    w.line("// each target's global bit index, in latest-version order");
    for (i, t) in targets.iter().enumerate() {
        w.line(&format!("pub const {}: usize = {i};", t.name));
    }
    w.blank();
    w.line("/// the bit for `idx` when it lands in `word`, else nothing — lets the");
    w.line("/// macro OR each field into the right word without naming which one");
    w.line("pub const fn w(word: usize, idx: usize) -> u128 {");
    w.indent();
    w.line("if idx / 128 == word { 1u128 << (idx % 128) } else { 0 }");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    w.line("#[doc(hidden)]");
    w.line("#[derive(Debug, Clone)]");
    w.line(&format!("pub struct __PluckOut{struct_lt} {{"));
    w.indent();
    for t in targets {
        w.line(&format!("pub {}: Option<{}>,", t.name, rust_type_of(t)));
    }
    w.dedent();
    w.line("}");
    w.blank();

    for vl in &schema.lineage.versions {
        emit_seek_subset_vn(schema, vl, targets, &params, ret_lt, w);
        w.blank();
    }

    w.line("#[doc(hidden)]");
    w.line("#[allow(dead_code)]");
    w.line(&format!(
        "pub fn __pluck<{params}>(buf: &[u8]) -> PojocResult<__PluckOut{ret_lt}> {{"
    ));
    w.indent();
    w.line("let mut pos = 0;");
    w.line("let envelope = read_envelope(buf, &mut pos)?;");
    let args: Vec<String> = (0..words).map(|i| format!("M{i}")).collect();
    let args = args.join(", ");
    w.line("match envelope.version {");
    w.indent();
    for vl in &schema.lineage.versions {
        let v = vl.version;
        w.line(&format!(
            "{v} => __pluck_v{v}::<{args}>(envelope.payload, 0),"
        ));
    }
    w.line("v => Err(Error::UnsupportedVersion(v)),");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    emit_pluck_macro(&schema.name_hint, words, w);
}

/// The ergonomic front door: field names in, values out. Generated per schema
/// rather than living in the runtime crate because it has to know how many mask
/// words this schema needs.
fn emit_pluck_macro(schema_name: &str, words: usize, w: &mut CodeWriter) {
    // `macro_rules!` only crosses a crate boundary via #[macro_export], which
    // lands it at the including crate's root. It can't then be re-exported under
    // a nicer path, because generated code arrives through `include!` and rustc
    // refuses absolute paths to macro-expanded `macro_export`s — so the name is
    // qualified by schema instead, which also keeps several schemas in one crate
    // from colliding. Downstream crates can `use dep::pluck_foo as pluck;`.
    let exported = format!("pluck_{}", schema_name.to_snake_case());
    // `$m` is the generated module; macro_rules resolves paths at the call site,
    // so the caller has to name it once
    let mask_arg = |rep: bool| -> String {
        (0..words)
            .map(|k| {
                if rep {
                    format!("{{ 0u128 $(| $m::__pluck_mask::w({k}, $m::__pluck_mask::$f))+ }}")
                } else {
                    format!("{{ $m::__pluck_mask::w({k}, $m::__pluck_mask::$f) }}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let bound = "expect(\"pluck: this field was named in the call, so it is always bound\")";

    w.line("/// Read named `seekable` fields out of an encoded payload without decoding it.");
    w.line("///");
    w.line("/// One field yields the value, several yield a tuple in the order asked for.");
    w.line("/// Never `Option`: naming a field guarantees it is bound. A field the");
    w.line("/// payload's version predates yields that version's default, as `decode` would.");
    w.line("///");
    w.line("/// The first argument is this module, which must be a single ident in scope.");
    w.line("/// The macro itself lives at your crate root, named for the schema.");
    w.line("///");
    w.line("/// ```ignore");
    w.line(&format!("/// let id = {exported}!(edge, &buf, id)?;"));
    w.line(&format!(
        "/// let (id, name) = {exported}!(edge, &buf, id, name)?;"
    ));
    w.line("/// ```");
    w.line("#[macro_export]");
    w.line(&format!("macro_rules! {exported} {{"));
    w.indent();
    w.line("// one field: the value itself rather than a one-tuple");
    w.line("($m:ident, $buf:expr, $f:ident $(,)?) => {{");
    w.indent();
    w.line(&format!(
        "$m::__pluck::<{}>($buf).map(|__r| __r.$f.{bound})",
        mask_arg(false)
    ));
    w.dedent();
    w.line("}};");
    w.line("($m:ident, $buf:expr, $($f:ident),+ $(,)?) => {{");
    w.indent();
    w.line(&format!(
        "$m::__pluck::<{}>($buf).map(|__r| ($(__r.$f.{bound},)+))",
        mask_arg(true)
    ));
    w.dedent();
    w.line("}};");
    w.dedent();
    w.line("}");
}
fn emit_seek_subset_vn(
    schema: &ResolvedSchema,
    vl: &VersionLineage,
    targets: &[&FieldIR],
    params: &str,
    ret_lt: &str,
    w: &mut CodeWriter,
) {
    let v = vl.version;
    let index_of = |n: &str| targets.iter().position(|t| t.name == n).expect("target");
    let requested = |n: &str| bit_test(index_of(n));

    w.line("#[doc(hidden)]");
    w.line("#[allow(dead_code)]");
    w.line(&format!(
        "pub fn __pluck_v{v}<{params}>(__buf: &[u8], __start: usize) \
         -> PojocResult<__PluckOut{ret_lt}> {{"
    ));
    w.indent();

    for t in targets {
        w.line(&format!("let mut __out_{} = None;", t.name));
    }

    let present: Vec<(&&FieldIR, &FieldLineage)> = targets
        .iter()
        .filter_map(|t| lineage_for(vl, &t.name).map(|fl| (t, fl)))
        .collect();

    // the intern strategy can't depend on MASK (it decides the prologue before
    // any field is read), so it's chosen over every target the way seek_all does
    let table_users: Vec<&FieldLineage> = present
        .iter()
        .map(|(_, fl)| *fl)
        .filter(|fl| {
            type_reaches_intern_table(
                &schema.types,
                &schema.unions,
                &fl.source_ty,
                &mut HashSet::new(),
            )
        })
        .collect();
    let needs_table = !table_users.is_empty();
    let fast_intern = needs_table && table_users.iter().all(|fl| can_read_interned_directly(fl));
    emit_prologue(schema, vl, needs_table, fast_intern, w);

    let (blast, rest) = blast_split(vl);
    let blast_ids: HashSet<_> = present
        .iter()
        .filter(|(_, fl)| wire_rank(fl) == 0)
        .map(|(_, fl)| fl.field_id)
        .collect();
    let tail_ids: HashSet<_> = present
        .iter()
        .filter(|(_, fl)| wire_rank(fl) != 0)
        .map(|(_, fl)| fl.field_id)
        .collect();

    if !blast_ids.is_empty() {
        w.line("let __blast_start = *__pos;");
    }
    let mut offset = 0usize;
    for fl in &blast {
        let size = wire_fixed_size(&fl.source_ty).unwrap_or(0);
        if blast_ids.contains(&fl.field_id) {
            let (t, _) = present
                .iter()
                .find(|(_, f)| f.field_id == fl.field_id)
                .unwrap();
            w.line(&format!("if {} {{", requested(&t.name)));
            w.indent();
            w.line(&format!(
                "let __end = __blast_start.checked_add({}).ok_or(Error::UnexpectedEof)?;",
                offset + size
            ));
            w.line("if __end > __buf.len() { return Err(Error::UnexpectedEof); }");
            if offset == 0 {
                w.line("*__pos = __blast_start;");
            } else {
                w.line(&format!("*__pos = __blast_start + {offset};"));
            }
            w.line("// SAFETY: checked directly above.");
            emit_fixed_field_read(schema, fl, w);
            emit_constraint_guard(t, &t.name, w);
            w.line(&format!("__out_{} = Some({});", t.name, t.name));
            w.dedent();
            w.line("}");
        }
        offset += size;
    }

    // nothing past the fixed block was asked for
    let tail_bits: Vec<usize> = rest
        .iter()
        .filter(|fl| tail_ids.contains(&fl.field_id))
        .filter_map(|fl| target_name_of(fl).map(&index_of))
        .collect();
    if !tail_bits.is_empty() {
        w.line("'tail: {");
        w.indent();
        w.line(&format!(
            "if {} {{ break 'tail; }}",
            no_bit_test(&tail_bits)
        ));
    }

    let total = fixed_total(&blast);
    if total > 0 && !tail_bits.is_empty() {
        w.line(&format!(
            "{{ let __end = {}.checked_add({total}).ok_or(Error::UnexpectedEof)?; \
             if __end > __buf.len() {{ return Err(Error::UnexpectedEof); }} *__pos = __end; }}",
            if blast_ids.is_empty() {
                "__pos"
            } else {
                "__blast_start"
            }
        ));
    }

    let last_needed = rest
        .iter()
        .rposition(|fl| tail_ids.contains(&fl.field_id))
        .map(|i| i + 1)
        .unwrap_or(0);

    let mut optional_counter = 0usize;
    for (i, fl) in rest.iter().take(last_needed).enumerate() {
        if tail_ids.contains(&fl.field_id) {
            let name = target_name_of(fl).expect("tail target has a mapping");
            let t = targets.iter().find(|t| t.name == name).unwrap();

            w.line(&format!("if {} {{", requested(name)));
            w.indent();
            if fast_intern && can_read_interned_directly(fl) {
                emit_bare_interned_read(name, w);
            } else if let ResolvedTypeRef::Optional(inner) = &fl.source_ty {
                let byte_idx = optional_counter / 8;
                let bit_idx = optional_counter % 8;
                w.line(&format!(
                    "let __present = (__header[{byte_idx}] & (1 << {bit_idx})) != 0;"
                ));
                emit_field_mapping_arm(schema, fl, inner, "__present", w);
            } else {
                emit_field_read(schema, fl, w);
            }
            emit_constraint_guard(t, name, w);
            w.line(&format!("__out_{name} = Some({name});"));
            w.dedent();
            w.line("} else {");
            w.indent();
            // unrequested: still has to be stepped over for the cursor to stay
            // aligned for whatever comes after it
            let mut throwaway = optional_counter;
            emit_ordered_skip(fl, &mut throwaway, w);
            w.dedent();
            w.line("}");

            // the presence-bit index is positional, so it advances the same way
            // through both branches
            if matches!(fl.source_ty, ResolvedTypeRef::Optional(_)) {
                optional_counter += 1;
            }

            // everything the caller wanted is bound
            let remaining: Vec<usize> = rest
                .iter()
                .skip(i + 1)
                .filter(|f| tail_ids.contains(&f.field_id))
                .filter_map(|f| target_name_of(f).map(&index_of))
                .collect();
            if !remaining.is_empty() {
                w.line(&format!(
                    "if {} {{ break 'tail; }}",
                    no_bit_test(&remaining)
                ));
            }
        } else {
            emit_ordered_skip(fl, &mut optional_counter, w);
        }
    }

    if !tail_bits.is_empty() {
        w.dedent();
        w.line("}");
    }

    // requested targets this version predates get its default, matching decode
    for t in targets {
        if lineage_for(vl, &t.name).is_some() {
            continue;
        }
        let missing = vl.missing.iter().find(|m| m.target_name == t.name);
        let expr = match missing {
            Some(m) if t.lazy => format!("LazyView::Owned({})", type_info(&m.ty).default_expr),
            Some(m) => get_field_default_expr(m.default.as_ref(), &m.ty, schema),
            None if t.lazy => format!("LazyView::Owned({})", type_info(&t.ty).default_expr),
            None => type_info(&t.ty).default_expr,
        };
        w.line(&format!(
            "if {} {{ __out_{} = Some({expr}); }}",
            requested(&t.name),
            t.name
        ));
    }

    w.line("Ok(__PluckOut {");
    w.indent();
    for t in targets {
        w.line(&format!("{}: __out_{},", t.name, t.name));
    }
    w.dedent();
    w.line("})");
    w.dedent();
    w.line("}");
}
