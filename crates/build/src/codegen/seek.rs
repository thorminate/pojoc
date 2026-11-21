//! Partial reads: accessors that pull one `seekable` field out of an encoded
//! payload without decoding the rest.
//!
//! A blastable target sits at a constant offset from the start of the fixed
//! block, so reaching it is an offset plus one unchecked read. Anything else is
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
    emit_field_mapping_arm, emit_field_read, emit_optional_header_read, emit_ordered_skip,
    emit_read_expr_unchecked, get_field_default_expr,
};
use super::writer::CodeWriter;
use super::{
    emit_constraint_guard, field_is_lifetime_infected, is_blastable, ordered_wire_fields,
    version_uses_intern, wire_fixed_size, wire_rank,
};
use crate::core::types::{ResolvedTypeRef, type_info};
use crate::schema::ir::ir_types::type_reaches_intern_table;
use crate::schema::ir::ir_types::{FieldIR, ResolvedSchema};
use crate::schema::ir::lineage::{FieldLineage, FieldMapping, VersionLineage};
use heck::ToSnakeCase;
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
        for vl in &schema.lineage.versions {
            emit_seek_vn_field(schema, vl, target, infected, w);
            w.blank();
        }
        emit_seek_dispatcher(schema, target, infected, w);
        w.blank();
    }

    emit_seek_all(schema, &targets, infected, w);
    if export_pluck {
        w.blank();
        emit_pluck(schema, &targets, infected, w);
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
fn lineage_for<'a>(vl: &'a VersionLineage, target: &str) -> Option<&'a FieldLineage> {
    vl.fields.iter().find(|fl| match &fl.mapping {
        FieldMapping::PassThrough { target_name } | FieldMapping::Cast { target_name, .. } => {
            target_name == target
        }
        FieldMapping::Discard => false,
    })
}

fn rust_type_of(target: &FieldIR) -> String {
    if target.lazy {
        format!("LazyView<'buf, {}>", type_info(&target.ty).rust_type)
    } else {
        type_info(&target.ty).rust_type
    }
}

// `__start: usize` leaves the buffer as the only reference parameter, so the
// output lifetime elides — naming it would just draw clippy::needless_lifetimes
// in the generated crate
fn seek_return_type(target: &FieldIR, infected: &HashSet<String>) -> String {
    let ty = rust_type_of(target);
    if target.lazy || field_is_lifetime_infected(&target.ty, target.lazy, infected) {
        ty.replace("&'buf ", "&").replace("'buf", "'_")
    } else {
        ty
    }
}

// a bare `intern string` resolves one index by walking the table; anything else
// that reaches the table has to materialize it, since the nested read_* helpers
// take the &[&str] slice
fn is_bare_interned(ty: &ResolvedTypeRef) -> bool {
    matches!(ty, ResolvedTypeRef::Interned(_))
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

fn blast_split<'a>(
    schema: &ResolvedSchema,
    vl: &'a VersionLineage,
) -> (Vec<&'a FieldLineage>, Vec<&'a FieldLineage>) {
    let ordered = ordered_wire_fields(schema, vl);
    let split = ordered.partition_point(|fl| is_blastable(schema, fl));
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
    if found.is_none() {
        // reads nothing, but the signature stays uniform for the dispatcher
        w.line("#[allow(unused_variables)]");
    }
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
            let fast = is_bare_interned(&fl.source_ty);
            let needs_table = type_reaches_intern_table(
                &schema.types,
                &schema.unions,
                &fl.source_ty,
                &mut HashSet::new(),
            );
            emit_prologue(schema, vl, needs_table, fast && needs_table, w);

            let (blast, rest) = blast_split(schema, vl);

            if wire_rank(schema, fl) == 0 {
                emit_blastable_target_read(&blast, fl, name, w);
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
fn emit_blastable_target_read(
    blast: &[&FieldLineage],
    target_fl: &FieldLineage,
    name: &str,
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

    let rust = type_info(&target_fl.source_ty).rust_type;
    let expr = emit_read_expr_unchecked(&target_fl.source_ty);
    w.line(&format!("let {name}: {rust} = {expr};"));
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

// the fast path: the field is a varint index, resolved against the table we
// skipped rather than one we built. an interned string's only legal retype is to
// another string shape, which shares its Rust type, so the mapping needs no cast.
fn emit_bare_interned_read(name: &str, w: &mut CodeWriter) {
    w.line(&format!(
        "let {name} = read_interned_string_at(__buf, __table_start, read_varint64(__buf, __pos)? as usize)?;"
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

    // the whole walk needs the table if any target it binds does
    let needs_table = present.iter().any(|(_, fl)| {
        type_reaches_intern_table(
            &schema.types,
            &schema.unions,
            &fl.source_ty,
            &mut HashSet::new(),
        )
    });
    emit_prologue(schema, vl, needs_table, false, w);

    let (blast, rest) = blast_split(schema, vl);

    // rank 0 targets: constant offsets into the fixed block, read in place
    let blast_ids: HashSet<_> = present
        .iter()
        .filter(|(_, fl)| wire_rank(schema, fl) == 0)
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
            let rust = type_info(&fl.source_ty).rust_type;
            let expr = emit_read_expr_unchecked(&fl.source_ty);
            w.line(&format!("let {}: {rust} = {expr};", t.name));
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
        .filter(|(_, fl)| wire_rank(schema, fl) != 0)
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
            if let ResolvedTypeRef::Optional(inner) = &fl.source_ty {
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
        let expr = missing_default(schema, vl, t);
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

fn missing_default(schema: &ResolvedSchema, vl: &VersionLineage, target: &FieldIR) -> String {
    let missing = vl.missing.iter().find(|m| m.target_name == target.name);
    match missing {
        Some(m) if !target.lazy => get_field_default_expr(m.default.as_ref(), &m.ty, schema),
        _ => {
            let ty = missing.map_or(&target.ty, |m| &m.ty);
            let default = type_info(ty).default_expr;
            if target.lazy {
                format!("LazyView::Owned({default})")
            } else {
                default
            }
        }
    }
}

fn pluck_selected(index: usize) -> String {
    format!("__M{} & (1u64 << {}) != 0", index / 64, index % 64)
}

fn emit_pluck(
    schema: &ResolvedSchema,
    targets: &[&FieldIR],
    infected: &HashSet<String>,
    w: &mut CodeWriter,
) {
    let masks = targets.len().div_ceil(64);
    let params = (0..masks)
        .map(|i| format!("const __M{i}: u64"))
        .collect::<Vec<_>>()
        .join(", ");
    let args = (0..masks)
        .map(|i| format!("__M{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let has_lifetime = targets
        .iter()
        .any(|f| field_is_lifetime_infected(&f.ty, f.lazy, infected));
    let (decl, ret) = if has_lifetime {
        ("<'buf>", "<'_>")
    } else {
        ("", "")
    };

    w.line("#[doc(hidden)]");
    w.line("#[derive(Default)]");
    w.line(&format!("pub struct __PojocPlucked{decl} {{"));
    w.indent();
    for target in targets {
        w.line(&format!(
            "pub {}: Option<{}>,",
            target.name,
            rust_type_of(target)
        ));
    }
    w.dedent();
    w.line("}");

    w.line("#[doc(hidden)]");
    w.line("#[allow(non_camel_case_types)]");
    w.line("#[derive(Clone, Copy)]");
    w.line("pub enum __PojocSeekField {");
    w.indent();
    for target in targets {
        w.line(&format!("{},", target.name));
    }
    w.dedent();
    w.line("}");
    w.line("#[doc(hidden)]");
    w.line("pub const fn __pluck_mask(fields: &[__PojocSeekField], word: usize) -> u64 {");
    w.indent();
    w.line("let mut index = 0;");
    w.line("let mut mask = 0;");
    w.line("while index < fields.len() {");
    w.indent();
    w.line("let field = fields[index] as usize;");
    w.line("if field / 64 == word { mask |= 1u64 << (field % 64); }");
    w.line("index += 1;");
    w.dedent();
    w.line("}");
    w.line("mask");
    w.dedent();
    w.line("}");

    w.line("#[doc(hidden)]");
    w.line(&format!(
        "pub fn __pluck<{params}>(buf: &[u8]) -> PojocResult<__PojocPlucked{ret}> {{"
    ));
    w.indent();
    w.line("let envelope = read_envelope(buf, &mut 0)?;");
    w.line("match envelope.version {");
    w.indent();
    for vl in &schema.lineage.versions {
        let version = vl.version;
        w.line(&format!(
            "{version} => __pluck_v{version}::<{args}>(envelope.payload),"
        ));
    }
    w.line("v => Err(Error::UnsupportedVersion(v)),");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");

    for vl in &schema.lineage.versions {
        w.blank();
        let version = vl.version;
        w.line("#[allow(unused_mut, unused_variables)]");
        w.line(&format!(
            "fn __pluck_v{version}<{params}>(__buf: &[u8]) -> PojocResult<__PojocPlucked{ret}> {{"
        ));
        w.indent();
        emit_pluck_walk(schema, vl, targets, w);
        w.dedent();
        w.line("}");
    }

    let name = schema.name_hint.to_snake_case();
    w.line("#[macro_export]");
    w.line(&format!("macro_rules! pluck_{name} {{"));
    w.indent();
    for target in targets {
        let name = &target.name;
        w.line(&format!(
            "($module:ident, $buf:expr, {name} $(,)?) => {{ $module::seek_{name}($buf) }};"
        ));
    }
    w.line("($module:ident, $buf:expr, $first:ident, $($field:ident),+ $(,)?) => {");
    w.indent();
    w.line("$module::__pluck::<");
    w.indent();
    for word in 0..masks {
        w.line(&format!("{{ $module::__pluck_mask(&[$module::__PojocSeekField::$first, $($module::__PojocSeekField::$field),+], {word}) }},"));
    }
    w.dedent();
    w.line(">($buf).map(|r| (r.$first.unwrap(), $(r.$field.unwrap()),+))");
    w.dedent();
    w.line("};");
    w.dedent();
    w.line("}");
}

fn emit_pluck_walk(
    schema: &ResolvedSchema,
    vl: &VersionLineage,
    targets: &[&FieldIR],
    w: &mut CodeWriter,
) {
    w.line("let mut __result = __PojocPlucked::default();");
    let present: Vec<_> = targets
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            if let Some(fl) = lineage_for(vl, &t.name) {
                return Some((i, *t, fl));
            }
            let selected = pluck_selected(i);
            let value = missing_default(schema, vl, t);
            w.line(&format!(
                "if {selected} {{ __result.{} = Some({value}); }}",
                t.name
            ));
            None
        })
        .collect();
    if present.is_empty() {
        w.line("Ok(__result)");
        return;
    }

    let remaining = present
        .iter()
        .map(|(i, _, _)| format!("({}) as usize", pluck_selected(*i)))
        .collect::<Vec<_>>()
        .join(" + ");
    w.line(&format!("let mut __remaining = {remaining};"));
    w.line("if __remaining == 0 { return Ok(__result); }");
    w.line("let mut __cursor = 0usize;");
    w.line("let __pos = &mut __cursor;");
    if version_uses_intern(schema, &vl.fields) {
        let table = present
            .iter()
            .filter(|(_, _, fl)| {
                type_reaches_intern_table(
                    &schema.types,
                    &schema.unions,
                    &fl.source_ty,
                    &mut HashSet::new(),
                )
            })
            .map(|(i, _, _)| pluck_selected(*i))
            .collect::<Vec<_>>();
        if table.is_empty() {
            w.line("skip_intern_table(__buf, __pos)?;");
        } else {
            w.line(&format!("let __table_owned = if {} {{", table.join(" || ")));
            w.indent();
            w.line("read_intern_table(__buf, __pos)?");
            w.dedent();
            w.line("} else {");
            w.indent();
            w.line("skip_intern_table(__buf, __pos)?;");
            w.line("Vec::new()");
            w.dedent();
            w.line("};");
            w.line("let __table = __table_owned.as_slice();");
        }
    }
    let optional_count = vl
        .fields
        .iter()
        .filter(|fl| matches!(fl.source_ty, ResolvedTypeRef::Optional(_)))
        .count();
    emit_optional_header_read(optional_count, w);
    let (blast, tail) = blast_split(schema, vl);
    w.line("let __blast_start = *__pos;");
    for (i, target, fl) in &present {
        if wire_rank(schema, fl) == 0 {
            w.line(&format!("if {} {{", pluck_selected(*i)));
            w.indent();
            w.line("*__pos = __blast_start;");
            emit_blastable_target_read(&blast, fl, &target.name, w);
            emit_pluck_bind(target, w);
            w.dedent();
            w.line("}");
        }
    }
    let last_tail = tail
        .iter()
        .rposition(|fl| present.iter().any(|(_, _, p)| p.field_id == fl.field_id));
    if let Some(last_tail) = last_tail {
        let total = fixed_total(&blast);
        w.line(&format!("*__pos = __blast_start.checked_add({total}).filter(|&end| end <= __buf.len()).ok_or(Error::UnexpectedEof)?;"));
        let mut optional = 0;
        for fl in &tail[..=last_tail] {
            if let Some((i, target, _)) = present.iter().find(|(_, _, p)| p.field_id == fl.field_id)
            {
                w.line(&format!("if {} {{", pluck_selected(*i)));
                w.indent();
                if let ResolvedTypeRef::Optional(inner) = &fl.source_ty {
                    let (byte, bit) = (optional / 8, optional % 8);
                    w.line(&format!(
                        "let __present = (__header[{byte}] & (1 << {bit})) != 0;"
                    ));
                    emit_field_mapping_arm(schema, fl, inner, "__present", w);
                } else {
                    emit_field_read(schema, fl, w);
                }
                emit_pluck_bind(target, w);
                w.dedent();
                w.line("} else {");
                w.indent();
                emit_ordered_skip(fl, &mut optional, w);
                w.dedent();
                w.line("}");
            } else {
                emit_ordered_skip(fl, &mut optional, w);
            }
        }
    }
    w.line("Ok(__result)");
}

fn emit_pluck_bind(target: &FieldIR, w: &mut CodeWriter) {
    let name = &target.name;
    emit_constraint_guard(target, name, w);
    w.line(&format!("__result.{name} = Some({name});"));
    w.line("__remaining -= 1;");
    w.line("if __remaining == 0 { return Ok(__result); }");
}
