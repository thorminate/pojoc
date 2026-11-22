use super::writer::{CodeWriter, VersionBodies};
use crate::codegen::{
    can_read_unchecked, emit_constraint_guard, emit_default, is_fixed_wire_field,
    ordered_wire_fields, wire_fixed_size,
};
use crate::core::types::*;
use crate::schema::ir::ir_types::*;
use crate::schema::ir::lineage::*;
use heck::ToSnakeCase;
use std::collections::{HashMap, HashSet};

pub fn emit_decode_functions(
    schema: &ResolvedSchema,
    infected: &HashSet<String>,
    w: &mut CodeWriter,
) -> Vec<Vec<i128>> {
    emit_type_readers(schema, w);
    emit_type_lazy_helpers(schema, w);
    emit_type_skippers(schema, w);
    emit_bitset_readers(schema, w);
    emit_union_readers(schema, infected, w);
    emit_union_skippers(schema, w);
    emit_lazy_helpers(schema, w);
    emit_skip_vn_functions(schema, w);

    // Compare complete bodies so defaults, casts and validation are part of
    // the identity. A matching wire layout alone is insufficient.
    let mut decoders = VersionBodies::default();
    for version in &schema.lineage.versions {
        let mut body = CodeWriter::default();
        body.indent = w.indent + 1;
        emit_decode_body(schema, version, &mut body);
        decoders.insert(version.version, body);
    }
    let mut dispatch = Vec::new();
    for (body, versions) in decoders.into_groups() {
        let first = versions[0];
        emit_decode_fn(schema, first, infected, &body, w);
        for v in &versions[1..] {
            w.line("#[allow(unused_imports)]");
            w.line(&format!("pub use self::decode_v{first} as decode_v{v};"));
        }
        w.blank();
        dispatch.push(versions);
    }
    dispatch
}

fn emit_type_readers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = latest_by_name(&schema.types.types);

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        let fn_name = format!("read_{}", name.to_snake_case());
        let (lt, buf_ty) = if is_infected(name) {
            ("<'buf>", "&'buf [u8]")
        } else {
            ("", "&[u8]")
        };
        let interning = is_intern_infected(name);
        let extra_param = if interning {
            ", __table: &[&'buf str]"
        } else {
            ""
        };
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}{lt}(__buf: {buf_ty}, __pos: &mut usize{extra_param}) -> PojocResult<{name}{lt}> {{"
        ));
        w.indent();

        let optional_count = resolved
            .fields
            .iter()
            .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
            .count();
        emit_optional_header_read(optional_count, w);

        let mut optional_counter = 0;
        for field in &resolved.fields {
            emit_type_field_read(name, field, &mut optional_counter, w);
        }

        for field in &resolved.fields {
            emit_constraint_guard(field, &field.name, w);
        }

        w.line(&format!("Ok({name} {{"));
        w.indent();
        for field in &resolved.fields {
            w.line(&format!("{},", field.name));
        }
        w.dedent();
        w.line("})");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

pub(crate) fn emit_type_field_read(
    name: &str,
    field: &FieldIR,
    optional_counter: &mut usize,
    w: &mut CodeWriter,
) {
    match &field.ty {
        ResolvedTypeRef::Optional(inner) => {
            let byte_idx = *optional_counter / 8;
            let bit_idx = *optional_counter % 8;
            *optional_counter += 1;
            if field.lazy {
                let fname = &field.name;
                let helper_name = lazy_field_helper_name(name, fname);
                let default_expr = type_info(&field.ty).default_expr;
                w.line(&format!("let __{fname}_start = *__pos;"));
                if let Some(stmt) = emit_skip_stmt(inner) {
                    w.line(&format!(
                        "if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 {{"
                    ));
                    w.indent();
                    w.line(&stmt);
                    w.dedent();
                    w.line("}");
                }
                w.line(&format!(
                    "let {fname} = if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 {{ \
                     LazyView::new(&__buf[__{fname}_start..*__pos], {helper_name}) \
                     }} else {{ LazyView::Owned({default_expr}) }};"
                ));
            } else {
                let inner_expr = emit_read_expr(inner);
                w.line(&format!(
                    "let {} = if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 {{ Some({inner_expr}) }} else {{ None }};",
                    field.name
                ));
            }
        }
        _ if field.lazy => {
            let fname = &field.name;
            let helper_name = lazy_field_helper_name(name, fname);
            w.line(&format!("let __{fname}_start = *__pos;"));
            if let Some(stmt) = emit_skip_stmt(&field.ty) {
                w.line(&stmt);
            }
            w.line(&format!(
                "let {fname} = LazyView::new(&__buf[__{fname}_start..*__pos], {helper_name});"
            ));
        }
        _ => {
            let expr = emit_read_expr(&field.ty);
            w.line(&format!("let {} = {expr};", field.name));
        }
    }
}

fn emit_type_skippers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = latest_by_name(&schema.types.types);

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        let fn_name = format!("skip_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}(__buf: &[u8], __pos: &mut usize) -> PojocResult<()> {{"
        ));
        w.indent();

        let optional_count = resolved
            .fields
            .iter()
            .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
            .count();
        emit_optional_header_read(optional_count, w);

        let mut optional_counter = 0;
        for field in &resolved.fields {
            match &field.ty {
                ResolvedTypeRef::Optional(inner) => {
                    let byte_idx = optional_counter / 8;
                    let bit_idx = optional_counter % 8;
                    optional_counter += 1;
                    w.line(&format!(
                        "if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 {{"
                    ));
                    w.indent();
                    if let Some(stmt) = emit_skip_stmt(inner) {
                        w.line(&stmt);
                    }
                    w.dedent();
                    w.line("}");
                }
                _ => {
                    if let Some(stmt) = emit_skip_stmt(&field.ty) {
                        w.line(&stmt);
                    }
                }
            }
        }

        w.line("Ok(())");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

// maps a historical discriminant to its current (possibly renamed) variant name.
fn latest_variant_name(latest: &ResolvedUnion, discriminant: u64) -> &str {
    latest
        .variants
        .iter()
        .find(|v| v.discriminant == discriminant)
        .map(|v| v.name.as_str())
        .expect("a discriminant known to an earlier version must still exist in the latest one")
}

fn emit_union_readers(schema: &ResolvedSchema, infected: &HashSet<String>, w: &mut CodeWriter) {
    let latest_by_name = latest_by_name(&schema.unions.unions);

    let mut entries: Vec<(&TypeId, &ResolvedUnion)> = schema.unions.unions.iter().collect();
    entries
        .sort_by(|(a, _), (b, _)| (a.name.as_str(), a.version).cmp(&(b.name.as_str(), b.version)));

    for (id, resolved) in entries {
        let name = &id.name;
        let (_, latest) = latest_by_name[name];

        let needs_lifetime = infected.contains(name.as_str());
        let (lt, buf_ty) = if needs_lifetime {
            ("<'buf>", "&'buf [u8]")
        } else {
            ("", "&[u8]")
        };
        let interning = is_union_intern_infected(id);
        let extra_param = if interning {
            ", __table: &[&'buf str]"
        } else {
            ""
        };
        let (_, read_prim, _) = width_prim_names(resolved.discriminant_width());
        let fn_name = format!("read_{}_v{}", name.to_snake_case(), id.version);
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}{lt}(__buf: {buf_ty}, __pos: &mut usize{extra_param}) -> PojocResult<{name}{lt}> {{"
        ));
        w.indent();
        w.line(&format!(
            "let __discriminant = {read_prim}(__buf, __pos)? as u64;"
        ));
        w.line("let __len = read_varint32(__buf, __pos)? as usize;");
        w.line("let __payload_start = *__pos;");
        w.line(
            "let __payload_end = __payload_start.checked_add(__len).ok_or(Error::InvalidLength)?;",
        );
        w.line("if __payload_end > __buf.len() { return Err(Error::InvalidLength); }");
        w.line("let __result = match __discriminant {");
        w.indent();
        for variant in &resolved.variants {
            let read_expr = emit_read_expr(&variant.payload);
            let current_name = latest_variant_name(latest, variant.discriminant);
            w.line(&format!(
                "{} => {name}::{current_name}({read_expr}),",
                variant.discriminant
            ));
        }
        w.line("other => {");
        w.indent();
        w.line("let data = __buf[__payload_start..__payload_end].to_vec();");
        w.line("*__pos = __payload_end;");
        // widths only ever grow, so upcasting to the latest width is lossless.
        let (unknown_rust_int, _, _) = width_prim_names(latest.discriminant_width());
        w.line(&format!(
            "{name}::Unknown {{ discriminant: other as {unknown_rust_int}, data }}"
        ));
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("};");
        w.line("if *__pos != __payload_end { return Err(Error::InvalidLength); }");
        w.line("Ok(__result)");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_union_skippers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let mut entries: Vec<(&TypeId, &ResolvedUnion)> = schema.unions.unions.iter().collect();
    entries
        .sort_by(|(a, _), (b, _)| (a.name.as_str(), a.version).cmp(&(b.name.as_str(), b.version)));

    for (id, resolved) in entries {
        let (_, read_prim, _) = width_prim_names(resolved.discriminant_width());
        let fn_name = format!("skip_{}_v{}", id.name.to_snake_case(), id.version);
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}(__buf: &[u8], __pos: &mut usize) -> PojocResult<()> {{"
        ));
        w.indent();
        w.line(&format!("let _ = {read_prim}(__buf, __pos)?;"));
        w.line("let __len = read_varint32(__buf, __pos)? as usize;");
        w.line("let __end = __pos.checked_add(__len).ok_or(Error::InvalidLength)?;");
        w.line("if __end > __buf.len() { return Err(Error::InvalidLength); }");
        w.line("*__pos = __end;");
        w.line("Ok(())");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_bitset_readers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = latest_by_name(&schema.bitsets.bitsets);

    let mut versions: Vec<_> = schema.bitsets.bitsets.iter().collect();
    versions.sort_by(|(a, _), (b, _)| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
    for (id, bitset) in versions {
        let name = &id.name;
        let computed_len = bitset.byte_width();
        let latest_len = latest[name].1.byte_width();
        let fn_name = format!("read_{}_v{}", name.to_snake_case(), id.version);
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}(__buf: &[u8], __pos: &mut usize) -> PojocResult<{name}> {{"
        ));
        w.indent();
        w.line(&format!(
            "let bytes = read_fixed_bytes::<{computed_len}>(__buf, __pos)?;"
        ));
        emit_bitset_value(name, computed_len, latest_len, true, w);
        w.dedent();
        w.line("}");
        w.blank();

        // unchecked variant used by the single-checked fixed decode block
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "unsafe fn {fn_name}_unchecked(__buf: &[u8], __pos: &mut usize) -> {name} {{"
        ));
        w.indent();
        w.line("// SAFETY: caller validated the fixed region before calling.");
        w.line(&format!(
            "let bytes = unsafe {{ read_fixed_bytes_unchecked::<{computed_len}>(__buf, __pos) }};"
        ));
        emit_bitset_value(name, computed_len, latest_len, false, w);
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_bitset_value(
    name: &str,
    width: usize,
    latest_width: usize,
    checked: bool,
    w: &mut CodeWriter,
) {
    let bytes = if width == latest_width {
        "bytes"
    } else {
        let count = width.min(latest_width);
        w.line(&format!("let mut flags = [0; {latest_width}];"));
        w.line(&format!(
            "flags[..{count}].copy_from_slice(&bytes[..{count}]);"
        ));
        "flags"
    };
    if checked {
        w.line(&format!("Ok({name}({bytes}))"));
    } else {
        w.line(&format!("{name}({bytes})"));
    }
}

pub(crate) fn emit_skip_stmt(ty: &ResolvedTypeRef) -> Option<String> {
    let info = type_info(ty);
    if let WireSize::Fixed(n) = info.wire_size
        && n > 0
    {
        return Some(format!(
            "{{ let __end = __pos.checked_add({n}).ok_or(Error::UnexpectedEof)?; \
             if __end > __buf.len() {{ return Err(Error::UnexpectedEof); }} \
             *__pos = __end; }}"
        ));
    }
    if let ResolvedTypeRef::FixedArray(_, n) = ty
        && *n == 0
    {
        return None;
    }
    if let ResolvedTypeRef::FixedMap(_, _, n) = ty
        && *n == 0
    {
        return None;
    }
    if let ResolvedTypeRef::FixedDeltaArray(_, n) = ty
        && *n == 0
    {
        return None;
    }
    if let ResolvedTypeRef::FixedString(n) = ty
        && *n == 0
    {
        return None;
    }

    Some(info.skip_stmt)
}

fn emit_decode_fn(
    schema: &ResolvedSchema,
    v: i128,
    infected: &HashSet<String>,
    body: &str,
    w: &mut CodeWriter,
) {
    let name = &schema.name_hint;
    let needs_lifetime = infected.contains(name.as_str());
    let lifetime = if needs_lifetime { "<'buf>" } else { "" };
    let buf_ty = if needs_lifetime {
        "&'buf [u8]"
    } else {
        "&[u8]"
    };
    w.line(&format!(
        "pub fn decode_v{v}{lifetime}(__buf: {buf_ty}, __pos: &mut usize) -> PojocResult<{name}{lifetime}> {{"
    ));
    w.append(body);
    w.line("}");
}

fn emit_decode_body(schema: &ResolvedSchema, vl: &VersionLineage, w: &mut CodeWriter) {
    let name = &schema.name_hint;
    let interning = crate::codegen::version_uses_intern(schema, &vl.fields);
    if interning {
        // table precedes every field on the wire, read once up front then threaded through nested read_* calls
        w.line("let __table_owned = read_intern_table(__buf, __pos)?;");
        w.line("let __table = __table_owned.as_slice();");
    }

    let optional_count = vl
        .fields
        .iter()
        .filter(|fl| matches!(fl.source_ty, ResolvedTypeRef::Optional(_)))
        .count();
    emit_optional_header_read(optional_count, w);

    // blastable fixed-width fields form a leading block read behind one bounds check; the rest follows the normal checked path
    let ordered = ordered_wire_fields(vl);
    let fixed_prefix = ordered.partition_point(|fl| is_fixed_wire_field(fl));
    let (blast, rest) = ordered.split_at(fixed_prefix);

    let fixed_total: usize = blast
        .iter()
        .map(|fl| wire_fixed_size(&fl.source_ty).unwrap_or(0))
        .sum();
    if fixed_total > 0 {
        w.line(&format!(
            "let __fixed_end = __pos.checked_add({fixed_total}).ok_or(Error::UnexpectedEof)?;"
        ));
        w.line("if __fixed_end > __buf.len() { return Err(Error::UnexpectedEof); }");
        w.line("// SAFETY: the check above validated __pos..__fixed_end; every read below stays within it.");
    }
    for &fl in blast {
        emit_fixed_field_read(schema, fl, w);
    }

    let mut optional_counter = 0;
    for &fl in rest {
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
    }

    if !vl.missing.is_empty() {
        w.blank();
        for mf in &vl.missing {
            if mf.lazy {
                let default_expr = type_info(&mf.ty).default_expr;
                w.line(&format!(
                    "let {} = LazyView::Owned({default_expr});",
                    mf.target_name
                ));
            } else {
                let default = get_field_default_expr(mf.default.as_ref(), &mf.ty, schema);
                w.line(&format!("let {} = {default};", mf.target_name));
            }
        }
    }

    w.blank();

    let latest = schema.versions.last().unwrap();
    for field in &latest.fields {
        emit_constraint_guard(field, &field.name, w);
    }
    w.line(&format!("Ok({name} {{"));
    w.indent();
    for field in &latest.fields {
        w.line(&format!("{},", field.name));
    }
    w.dedent();
    w.line("})");
}

pub(crate) fn emit_field_read(schema: &ResolvedSchema, fl: &FieldLineage, w: &mut CodeWriter) {
    match &fl.mapping {
        FieldMapping::Discard => {
            if let ResolvedTypeRef::Array(inner) = &fl.source_ty
                && let WireSize::Fixed(stride) = type_info(inner).wire_size
            {
                w.line(&format!(
                    "{{ let __n = read_array_len(__buf, __pos)?; \
                     let __bytes = __n.checked_mul({stride}).ok_or(Error::InvalidLength)?; \
                     let __end = __pos.checked_add(__bytes).ok_or(Error::UnexpectedEof)?; \
                     if __end > __buf.len() {{ return Err(Error::UnexpectedEof); }} \
                     *__pos = __end; }}"
                ));
                return;
            }
            if let Some(stmt) = emit_skip_stmt(&fl.source_ty) {
                w.line(&stmt);
            }
        }
        FieldMapping::PassThrough { target_name } => {
            let target_field = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap();
            if target_field.lazy {
                let some_fn = lazy_field_helper_name(&schema.name_hint, target_name);
                w.line(&format!("let __{target_name}_start = *__pos;"));
                if let Some(stmt) = emit_skip_stmt(&fl.source_ty) {
                    w.line(&stmt);
                }
                w.line(&format!("let {target_name} = LazyView::new(&__buf[__{target_name}_start..*__pos], {some_fn});"));
                return;
            }
            let expr = emit_read_expr(&fl.source_ty);
            w.line(&format!("let {target_name} = {expr};"));
        }
        FieldMapping::Cast {
            target_name,
            from,
            to,
        } => {
            let target_field = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap();
            if target_field.lazy {
                if let Some(stmt) = emit_skip_stmt(&fl.source_ty) {
                    w.line(&stmt);
                }
                let default_expr = type_info(&target_field.ty).default_expr;
                w.line(&format!(
                    "let {target_name} = LazyView::Owned({default_expr});"
                ));
                return;
            }
            let rhs = match emit_cast_value(schema, from, to, &w.indent) {
                CastExpr::Inline(e) | CastExpr::Block(e) => e,
            };
            w.line(&format!("let {target_name} = {rhs};"));
        }
    }
}

pub(crate) fn emit_read_expr(ty: &ResolvedTypeRef) -> String {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(id) => {
            let extra = if is_intern_infected(&id.name) {
                ", __table"
            } else {
                ""
            };
            format!("{}(__buf, __pos{extra})?", info.read_fn)
        }
        ResolvedTypeRef::Enum(id, width) => {
            let (_, read_prim, _) = width_prim_names(*width);
            format!(
                "{{ let __raw = {read_prim}(__buf, __pos)? as u32; {}::try_from(__raw).map_err(|_| Error::InvalidEnumVariant)? }}",
                id.name
            )
        }
        ResolvedTypeRef::Union(id) => {
            let extra = if is_union_intern_infected(id) {
                ", __table"
            } else {
                ""
            };
            format!("{}(__buf, __pos{extra})?", info.read_fn)
        }
        ResolvedTypeRef::Bitset(_, _) => {
            format!("{}(__buf, __pos)?", info.read_fn)
        }
        ResolvedTypeRef::FixedString(n) => {
            if *n == 0 {
                return "&[]".to_string();
            }
            format!("{}(__buf, __pos)?", info.read_fn)
        }
        ResolvedTypeRef::Array(inner) => {
            if let ResolvedTypeRef::Scalar(id) = inner.as_ref()
                && is_bulk_castable_scalar(&id.name)
            {
                let rust_type = type_info(inner).rust_type;
                return format!("read_pod_array::<{rust_type}>(__buf, __pos)?");
            }
            let inner_expr = emit_read_expr(inner);
            format!(
                "{{ let __n = read_array_len(__buf, __pos)?; let mut __v = PojocVec::with_capacity(__n); for _ in 0..__n {{ __v.push({inner_expr}); }} __v }}"
            )
        }
        ResolvedTypeRef::FixedArray(inner, n) => {
            if *n == 0 {
                return "[]".to_string();
            }
            if let ResolvedTypeRef::Scalar(id) = inner.as_ref()
                && is_bulk_castable_scalar(&id.name)
            {
                let rust_type = type_info(inner).rust_type;
                return format!("read_fixed_pod_array::<{rust_type}, {n}>(__buf, __pos)?");
            }
            let inner_expr = emit_read_expr(inner);
            let init = if let WireSize::Fixed(_) = type_info(inner).wire_size {
                let default = type_info(inner).default_expr;
                format!("[{default}; {n}]")
            } else {
                "std::array::from_fn(|_| Default::default())".to_string()
            };
            format!(
                "{{ let mut __arr: [_; {n}] = {init}; for __slot in __arr.iter_mut() {{ *__slot = {inner_expr}; }} __arr }}"
            )
        }
        ResolvedTypeRef::DeltaArray(inner) => {
            let inner_rust = type_info(inner).rust_type;
            format!("read_delta_array::<{inner_rust}>(__buf, __pos)?")
        }
        ResolvedTypeRef::FixedDeltaArray(inner, n) => {
            if *n == 0 {
                return "[]".to_string();
            }
            let inner_rust = type_info(inner).rust_type;
            format!("read_fixed_delta_array::<{inner_rust}, {n}>(__buf, __pos)?")
        }
        ResolvedTypeRef::Map(k_ty, v_ty) => {
            let ke = emit_read_expr(k_ty);
            let ve = emit_read_expr(v_ty);
            format!(
                "{{ let __n = read_array_len(__buf, __pos)?; let mut __m = PojocMap::with_capacity_and_hasher(__n, Default::default()); for _ in 0..__n {{ let __k = {ke}; let __v = {ve}; __m.insert(__k, __v); }} __m }}"
            )
        }
        ResolvedTypeRef::FixedMap(k_ty, v_ty, n) => {
            if *n == 0 {
                return "{ PojocFixedMap::with_capacity(0) }".to_string();
            }
            let ke = emit_read_expr(k_ty);
            let ve = emit_read_expr(v_ty);
            format!(
                "{{ let mut __m = PojocFixedMap::with_capacity({n}); \
         for _ in 0..{n} {{ let __k = {ke}; let __v = {ve}; __m.push((__k, __v)); }} \
         __m }}"
            )
        }
        ResolvedTypeRef::Tuple(elements) => {
            let inner = elements
                .iter()
                .map(emit_read_expr)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({inner})")
        }
        ResolvedTypeRef::VFloat {
            min, step, backing, ..
        } => {
            format!(
                "(({}(__buf, __pos)? as f64) * {}f64 + {}f64) as f32",
                backing.read_fn(),
                step,
                min
            )
        }
        ResolvedTypeRef::Optional(inner) => {
            let inner_expr = emit_read_expr(inner);
            format!("if read_u8(__buf, __pos)? != 0 {{ Some({inner_expr}) }} else {{ None }}")
        }
        ResolvedTypeRef::Boxed(inner) => {
            let inner_expr = emit_read_expr(inner);
            format!("Box::new({inner_expr})")
        }
        // transparent on the wire; the constraint check is emitted separately by the caller
        ResolvedTypeRef::Constrained { inner, .. } => emit_read_expr(inner),
        ResolvedTypeRef::Interned(_) => "read_interned_string_ref(__table, __buf, __pos)?".into(),
        ResolvedTypeRef::ImportedSchema { .. } => {
            format!("{}(__buf, __pos)?", info.read_fn)
        }
    }
}

// region is already bounds-validated here, so pass-throughs use unchecked accessors and discards just advance the cursor
pub(crate) fn emit_fixed_field_read(
    schema: &ResolvedSchema,
    fl: &FieldLineage,
    w: &mut CodeWriter,
) {
    if !can_read_unchecked(schema, fl) {
        emit_field_read(schema, fl, w);
        return;
    }
    let source_ty = &fl.source_ty;
    match &fl.mapping {
        FieldMapping::Discard => {
            let size = wire_fixed_size(source_ty).unwrap_or(0);
            if size > 0 {
                w.line(&format!("*__pos += {size};"));
            }
        }
        FieldMapping::PassThrough { target_name } => {
            let rust = type_info(source_ty).rust_type;
            let expr = emit_read_expr_unchecked(source_ty);
            w.line(&format!("let {target_name}: {rust} = {expr};"));
        }
        FieldMapping::Cast { .. } => unreachable!("casts are never blastable"),
    }
}

// reads via *_unchecked accessors under the blastable block's single upfront
// bounds check. Enum still needs its fallible TryFrom, `?`-propagated normally.
pub(crate) fn emit_read_expr_unchecked(ty: &ResolvedTypeRef) -> String {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(_) => {
            format!("unsafe {{ {}_unchecked(__buf, __pos) }}", info.read_fn)
        }
        ResolvedTypeRef::Enum(id, width) => {
            let (_, read_prim, _) = width_prim_names(*width);
            format!(
                "{{ let __raw = unsafe {{ {read_prim}_unchecked(__buf, __pos) }} as u32; {}::try_from(__raw).map_err(|_| Error::InvalidEnumVariant)? }}",
                id.name
            )
        }
        ResolvedTypeRef::Bitset(_, _) => {
            format!("unsafe {{ {}_unchecked(__buf, __pos) }}", info.read_fn)
        }
        ResolvedTypeRef::FixedString(n) => {
            if *n == 0 {
                "[]".to_string()
            } else {
                format!("unsafe {{ read_fixed_bytes_unchecked::<{n}>(__buf, __pos) }}")
            }
        }
        ResolvedTypeRef::VFloat {
            min, step, backing, ..
        } => {
            format!(
                "(( unsafe {{ {}_unchecked(__buf, __pos) }} as f64) * {step}f64 + {min}f64) as f32",
                backing.read_fn()
            )
        }
        ResolvedTypeRef::FixedArray(inner, n) => {
            if *n == 0 {
                "[]".to_string()
            } else if let ResolvedTypeRef::Scalar(id) = inner.as_ref()
                && is_bulk_castable_scalar(&id.name)
            {
                let rust_type = type_info(inner).rust_type;
                format!(
                    "unsafe {{ read_fixed_pod_array_unchecked::<{rust_type}, {n}>(__buf, __pos) }}"
                )
            } else {
                let inner_expr = emit_read_expr_unchecked(inner);
                format!("std::array::from_fn(|_| {inner_expr})")
            }
        }
        ResolvedTypeRef::Tuple(elems) => {
            let parts: Vec<String> = elems.iter().map(emit_read_expr_unchecked).collect();
            if parts.len() == 1 {
                format!("({},)", parts[0])
            } else {
                format!("({})", parts.join(", "))
            }
        }
        // transparent on the wire; constraint check happens later once the blastable block's fields are bound
        ResolvedTypeRef::Constrained { inner, .. } => emit_read_expr_unchecked(inner),
        _ => unreachable!("emit_read_expr_unchecked called on a non-blastable type"),
    }
}

pub(crate) fn emit_optional_header_read(optional_count: usize, w: &mut CodeWriter) {
    let header_bytes = optional_count.div_ceil(8);
    if header_bytes > 0 {
        w.line(&format!(
            "let __header = read_fixed_bytes::<{header_bytes}>(__buf, __pos)?;"
        ));
    }
}

pub(crate) fn emit_field_mapping_arm(
    schema: &ResolvedSchema,
    fl: &FieldLineage,
    inner: &ResolvedTypeRef,
    __present: &str,
    w: &mut CodeWriter,
) {
    match &fl.mapping {
        FieldMapping::Discard => {
            if let Some(stmt) = emit_skip_stmt(inner) {
                w.line(&format!("if {__present} {{"));
                w.indent();
                w.line(&stmt);
                w.dedent();
                w.line("}");
            }
        }
        FieldMapping::PassThrough { target_name } => {
            let target_field = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap();
            if target_field.lazy {
                let some_fn = lazy_field_helper_name(&schema.name_hint, target_name);
                let default_expr = type_info(&target_field.ty).default_expr;
                w.line(&format!("let __{target_name}_start = *__pos;"));
                if let Some(stmt) = emit_skip_stmt(inner) {
                    w.line(&format!("if {__present} {{"));
                    w.indent();
                    w.line(&stmt);
                    w.dedent();
                    w.line("}");
                    w.line(&format!(
                        "let {target_name} = if {__present} {{ \
                         LazyView::new(&__buf[__{target_name}_start..*__pos], {some_fn}) \
                         }} else {{ \
                         LazyView::Owned({default_expr}) \
                         }};"
                    ));
                }
                return;
            }
            let inner_expr = emit_read_expr(inner);
            w.line(&format!(
                "let {target_name} = if {__present} {{ Some({inner_expr}) }} else {{ None }};"
            ));
        }
        FieldMapping::Cast {
            target_name,
            from: _,
            to,
        } => {
            let target_field = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap();
            if target_field.lazy {
                if let Some(stmt) = emit_skip_stmt(inner) {
                    w.line(&format!("if {__present} {{"));
                    w.indent();
                    w.line(&stmt);
                    w.dedent();
                    w.line("}");
                    let default_expr = type_info(&target_field.ty).default_expr;
                    w.line(&format!(
                        "let {target_name} = LazyView::Owned({default_expr});"
                    ));
                }
                return;
            }
            let (target_inner, optional_out) = match to {
                ResolvedTypeRef::Optional(t) => (&**t, true),
                t => (t, false),
            };
            let rhs = match emit_cast_value(schema, inner, target_inner, &w.indent) {
                CastExpr::Inline(e) | CastExpr::Block(e) => e,
            };
            if optional_out {
                w.line(&format!(
                    "let {target_name} = if {__present} {{ Some({rhs}) }} else {{ None }};"
                ));
            } else {
                let default = type_info(to).default_expr;
                w.line(&format!(
                    "let {target_name} = if {__present} {{ {rhs} }} else {{ {default} }};"
                ));
            }
        }
    }
}

pub(crate) fn get_field_default_expr(
    default_val: Option<&DefaultValue>,
    type_ref: &ResolvedTypeRef,
    schema: &ResolvedSchema,
) -> String {
    match default_val {
        Some(DefaultValue::None) if matches!(type_ref, ResolvedTypeRef::Optional(_)) => {
            "None".to_string()
        }

        Some(DefaultValue::None) if !matches!(type_ref, ResolvedTypeRef::Optional(_)) => {
            type_info(type_ref).default_expr
        }

        Some(DefaultValue::Map(pairs)) if matches!(type_ref, ResolvedTypeRef::FixedMap(..)) => {
            let ResolvedTypeRef::FixedMap(_, _, n) = type_ref else {
                unreachable!()
            };
            if pairs.is_empty() {
                return "{ PojocFixedMap::with_capacity(0) }".to_string();
            }
            let pushes = pairs
                .iter()
                .map(|(k, v)| {
                    format!(
                        "__m.push(({}, {}));",
                        emit_default(k, schema),
                        emit_default(v, schema)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            let fill = n.saturating_sub(pairs.len());
            let fill_str = if fill > 0 {
                format!(
                    " for _ in 0..{fill} {{ __m.push((Default::default(), Default::default())); }}"
                )
            } else {
                String::new()
            };
            format!("{{ let mut __m = PojocFixedMap::with_capacity({n}); {pushes}{fill_str} __m }}")
        }

        Some(DefaultValue::BitsetLiteral { ty_name, kvs }) if matches!(type_ref, ResolvedTypeRef::Scalar(t) if is_primitive(&t.name)) =>
        {
            let bits = compute_bitset_literal_value(ty_name, kvs, schema);
            let rust_type = type_info(type_ref).rust_type;
            format!("{bits}{rust_type}")
        }

        Some(DefaultValue::Array(els)) => match type_ref {
            ResolvedTypeRef::FixedArray(_, n) | ResolvedTypeRef::FixedDeltaArray(_, n) => {
                let mut parts: Vec<String> = els.iter().map(|e| emit_default(e, schema)).collect();
                parts.truncate(*n);
                while parts.len() < *n {
                    parts.push("Default::default()".to_string());
                }
                format!("[{}]", parts.join(", "))
            }
            _ if els.is_empty() => type_info(type_ref).default_expr,
            _ => emit_default(default_val.unwrap(), schema),
        },

        Some(DefaultValue::Map(pairs)) if pairs.is_empty() => type_info(type_ref).default_expr,
        Some(other) => emit_default(other, schema),
        None => type_info(type_ref).default_expr,
    }
}

fn compute_bitset_literal_value(
    ty_name: &str,
    kvs: &[(String, bool)],
    schema: &ResolvedSchema,
) -> u64 {
    let bs = schema
        .bitsets
        .bitsets
        .iter()
        .filter(|(id, _)| id.name == *ty_name)
        .max_by_key(|(id, _)| id.version)
        .map(|(_, bs)| bs);

    let mut value: u64 = 0;
    if let Some(bs) = bs {
        for (flag_name, set) in kvs {
            if *set && let Some(idx) = bs.variants.iter().position(|v| v.name == *flag_name) {
                value |= 1u64 << idx;
            }
        }
    }
    value
}

pub(crate) enum CastExpr {
    Inline(String),
    Block(String),
}

pub(crate) fn emit_cast_value(
    schema: &ResolvedSchema,
    from: &ResolvedTypeRef,
    to: &ResolvedTypeRef,
    indent: &usize,
) -> CastExpr {
    use ResolvedTypeRef::*;
    match (from, to) {
        (Scalar(f), Scalar(t)) if is_primitive(&f.name) && is_primitive(&t.name) => {
            let fi = type_info(from);
            let ti = type_info(to);
            if f.name == t.name {
                CastExpr::Inline(format!("{}(__buf, __pos)?", fi.read_fn))
            } else {
                CastExpr::Inline(format!("{}(__buf, __pos)? as {}", fi.read_fn, ti.rust_type))
            }
        }

        (Scalar(f), Scalar(t)) => CastExpr::Block(struct_cast_block(schema, f, t, indent)),

        (FixedString(from_n), FixedString(to_n)) => {
            let copy_n = (*from_n).min(*to_n);
            CastExpr::Block(format!(
                "{{ let __src = read_fixed_bytes::<{from_n}>(__buf, __pos)?; \
                 let mut __dst = [0u8; {to_n}]; \
                 __dst[..{copy_n}].copy_from_slice(&__src[..{copy_n}]); \
                 __dst }}"
            ))
        }

        (FixedMap(fk, fv, from_n), FixedMap(_, _, to_n)) => {
            let ke = emit_read_expr(fk);
            let ve = emit_read_expr(fv);
            if *to_n == 0 {
                return CastExpr::Inline("{ PojocFixedMap::with_capacity(0) }".to_string());
            }
            if from_n <= to_n {
                CastExpr::Block(format!(
                    "{{ let mut __m = PojocFixedMap::with_capacity({to_n}); \
                     for _ in 0..{from_n} {{ let __k = {ke}; let __v = {ve}; __m.push((__k, __v)); }} \
                     __m }}"
                ))
            } else {
                CastExpr::Block(format!(
                    "{{ let mut __m = PojocFixedMap::with_capacity({to_n}); \
                     for __i in 0..{from_n} {{ \
                       let __k = {ke}; let __v = {ve}; \
                       if __i < {to_n} {{ __m.push((__k, __v)); }} \
                     }} \
                     __m }}"
                ))
            }
        }

        (FixedArray(elem, n), Map(_, v_ty)) => {
            let elem_expr = emit_read_expr(elem);
            let v_rust = type_info(v_ty).rust_type;
            CastExpr::Block(format!(
                "{{ let mut __m = PojocMap::with_capacity_and_hasher({n}, Default::default()); \
                 for __i in 0i32..{n} {{ let __v: {v_rust} = {elem_expr}; __m.insert(__i, __v); }} \
                 __m }}"
            ))
        }

        (Bitset(_, width), Scalar(t)) if is_primitive(&t.name) => {
            let to_rust = type_info(to).rust_type;
            let expr = match *width {
                0 => format!("0u8 as {to_rust}"),
                1 => format!("read_u8(__buf, __pos)? as {to_rust}"),
                2 => format!("read_u16(__buf, __pos)? as {to_rust}"),
                4 => format!("read_u32(__buf, __pos)? as {to_rust}"),
                n => {
                    let copy = n.min(16);
                    format!(
                        "{{ let bytes = read_fixed_bytes::<{n}>(__buf, __pos)?; \
                              let mut packed = [0u8; 16]; packed[..{copy}].copy_from_slice(&bytes[..{copy}]); \
                              u128::from_le_bytes(packed) as {to_rust} }}"
                    )
                }
            };
            CastExpr::Inline(expr)
        }

        (FixedDeltaArray(from_elem, from_n), FixedDeltaArray(_, to_n)) => {
            let elem_rust = type_info(from_elem).rust_type;
            let elem_default = type_info(from_elem).default_expr;
            let copy_n = (*from_n).min(*to_n);
            CastExpr::Block(format!(
                "{{ let __src = read_fixed_delta_array::<{elem_rust}, {from_n}>(__buf, __pos)?; \
                let mut __dst = [{elem_default}; {to_n}]; \
                __dst[..{copy_n}].copy_from_slice(&__src[..{copy_n}]); \
                __dst }}"
            ))
        }

        (from_ty, Optional(to_inner)) if !matches!(from_ty, Optional(_)) => {
            let inner_expr = match emit_cast_value(schema, from_ty, to_inner, indent) {
                CastExpr::Inline(e) | CastExpr::Block(e) => e,
            };
            CastExpr::Inline(format!("Some({inner_expr})"))
        }

        (Boxed(f_inner), Boxed(t_inner)) => {
            let inner_expr = match emit_cast_value(schema, f_inner, t_inner, indent) {
                CastExpr::Inline(e) | CastExpr::Block(e) => e,
            };
            CastExpr::Inline(format!("Box::new({inner_expr})"))
        }

        (from_ty, Boxed(to_inner)) if !matches!(from_ty, Boxed(_)) => {
            let inner_expr = match emit_cast_value(schema, from_ty, to_inner, indent) {
                CastExpr::Inline(e) | CastExpr::Block(e) => e,
            };
            CastExpr::Inline(format!("Box::new({inner_expr})"))
        }

        (Boxed(from_inner), to_ty) if !matches!(to_ty, Boxed(_)) => {
            emit_cast_value(schema, from_inner, to_ty, indent)
        }

        _ => CastExpr::Inline(emit_read_expr(from)),
    }
}

// indent passed in so the sub writer matches the parent's indent
fn struct_cast_block(
    schema: &ResolvedSchema,
    from: &TypeId,
    to: &TypeId,
    indent: &usize,
) -> String {
    let mut sub = CodeWriter::default();
    sub.line("{");
    // set indent after the '{' so the brace itself isn't indented
    sub.indent = *indent;
    sub.indent();
    emit_struct_cast_body(schema, from, to, &mut sub);
    sub.dedent();
    sub.write("}");
    sub.finish()
}

fn emit_struct_cast_body(schema: &ResolvedSchema, from: &TypeId, to: &TypeId, w: &mut CodeWriter) {
    let from_type = schema
        .types
        .types
        .get(from)
        .expect("struct cast source not found");
    let to_type = schema
        .types
        .types
        .get(to)
        .expect("struct cast target not found");

    let to_by_id: HashMap<FieldId, &FieldIR> = to_type.fields.iter().map(|f| (f.id, f)).collect();
    let to_ids: HashSet<FieldId> = to_type.fields.iter().map(|f| f.id).collect();

    let optional_count = from_type
        .fields
        .iter()
        .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
        .count();
    emit_optional_header_read(optional_count, w);

    let mut optional_counter = 0;
    for src in &from_type.fields {
        let is_target_bound = to_ids.contains(&src.id);

        match &src.ty {
            ResolvedTypeRef::Optional(inner) => {
                let byte_idx = optional_counter / 8;
                let bit_idx = optional_counter % 8;
                optional_counter += 1;

                if is_target_bound {
                    let dst_name = &to_by_id[&src.id].name;
                    let expr = emit_read_expr(inner);
                    w.line(&format!(
                        "let {dst_name} = if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 \
                         {{ Some({expr}) }} else {{ None }};"
                    ));
                } else {
                    if let Some(stmt) = emit_skip_stmt(inner) {
                        w.line(&format!(
                            "if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 {{"
                        ));
                        w.indent();
                        w.line(&stmt);
                        w.dedent();
                        w.line("}");
                    }
                }
            }
            _ => {
                let binding_prefix = if is_target_bound {
                    let dst_name = &to_by_id[&src.id].name;
                    format!("let {dst_name} = ")
                } else {
                    "let _ = ".to_string()
                };
                let expr = emit_read_expr(&src.ty);
                w.line(&format!("{binding_prefix}{expr};"));
            }
        }
    }

    for dst in &to_type.fields {
        if !from_type.fields.iter().any(|f| f.id == dst.id) {
            let dst_name = &dst.name;
            let default_str = get_field_default_expr(dst.default.as_ref(), &dst.ty, schema);
            w.line(&format!("let {dst_name} = {default_str};"));
        }
    }

    w.line(&format!("{} {{", to.name));
    w.indent();
    for dst in &to_type.fields {
        w.line(&format!("{},", dst.name));
    }
    w.dedent();
    w.line("}");
}

// body of a lazy field's decode_fn: `fn(&'buf [u8], &mut usize) -> PojocResult<T>`
// where T is the field's declared (possibly Optional) type — the signature/name
// are written by the caller, this only fills in the `{ ... }` block.
fn emit_lazy_helper_body(ty: &ResolvedTypeRef, w: &mut CodeWriter) {
    if let ResolvedTypeRef::Optional(inner) = ty {
        let body = emit_read_expr(inner);
        w.line(&format!("Ok(Some({body}))"));
    } else {
        let body = emit_read_expr(ty);
        let result_body = body
            .strip_suffix('?')
            .map(|s| s.trim_end().to_string())
            .unwrap_or_else(|| format!("Ok({body})"));
        w.line(&result_body);
    }
}

fn emit_lazy_helpers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let mut emitted: HashSet<String> = HashSet::new();
    let latest = schema.versions.last().unwrap();

    for fl in &latest.fields {
        if !fl.lazy {
            continue;
        }
        let target_name = &fl.name;
        let ty = &fl.ty;
        let rust_ty = type_info(ty).rust_type;

        let some_name = lazy_field_helper_name(&schema.name_hint, target_name);
        // if the value borrows the buffer, the reader must be generic over the input lifetime so it coerces to LazyView's fn pointer type
        let (lt, buf_ty) = if rust_ty.contains("'buf") {
            ("<'buf>", "&'buf [u8]")
        } else {
            ("", "&[u8]")
        };
        if emitted.insert(some_name.clone()) {
            w.line("#[allow(dead_code)]");
            w.line(&format!(
                "fn {some_name}{lt}(__buf: {buf_ty}, __pos: &mut usize) -> PojocResult<{rust_ty}> {{"
            ));
            w.indent();
            emit_lazy_helper_body(ty, w);
            w.dedent();
            w.line("}");
            w.blank();
        }
    }
}

// name of a lazy field's decode_fn helper, namespaced by its declaring type
// (the root schema type or any named type) since field names collide across
// types — used consistently by both the root's per-version decode path and
// any named type's nested read_foo, plus serde (structs.rs) for either.
pub(crate) fn lazy_field_helper_name(type_name: &str, field_name: &str) -> String {
    format!("read_{}_{field_name}_lazy", type_name.to_snake_case())
}

// mirrors emit_lazy_helpers but for lazy fields on any named type nested
// elsewhere (e.g. a union payload struct) — read via emit_type_readers'
// `read_foo`, not the root's per-version decode_vN.
fn emit_type_lazy_helpers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = latest_by_name(&schema.types.types);
    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        for field in &resolved.fields {
            if !field.lazy {
                continue;
            }
            let ty = &field.ty;
            let rust_ty = type_info(ty).rust_type;
            let helper_name = lazy_field_helper_name(name, &field.name);
            let (lt, buf_ty) = if rust_ty.contains("'buf") {
                ("<'buf>", "&'buf [u8]")
            } else {
                ("", "&[u8]")
            };
            w.line("#[allow(dead_code)]");
            w.line(&format!(
                "fn {helper_name}{lt}(__buf: {buf_ty}, __pos: &mut usize) -> PojocResult<{rust_ty}> {{"
            ));
            w.indent();
            emit_lazy_helper_body(ty, w);
            w.dedent();
            w.line("}");
            w.blank();
        }
    }
}

pub fn emit_skip_vn_functions(schema: &ResolvedSchema, w: &mut CodeWriter) {
    for vl in &schema.lineage.versions {
        emit_skip_vn_fn(schema, vl, w);
        w.blank();
    }
}

fn emit_skip_vn_fn(schema: &ResolvedSchema, vl: &VersionLineage, w: &mut CodeWriter) {
    let v = vl.version;

    w.line("#[allow(dead_code)]");
    w.line(&format!(
        "pub fn skip_v{v}(__buf: &[u8], __pos: &mut usize) -> PojocResult<()> {{"
    ));
    w.indent();

    // decode_vN reads the table before anything else, so skipping the payload
    // has to step over it too
    if crate::codegen::version_uses_intern(schema, &vl.fields) {
        w.line("skip_intern_table(__buf, __pos)?;");
    }

    let optional_count = vl
        .fields
        .iter()
        .filter(|fl| matches!(fl.source_ty, ResolvedTypeRef::Optional(_)))
        .count();
    emit_optional_header_read(optional_count, w);

    // mirrors the reordered wire layout: one bounds-checked advance over the blastable prefix, then skip the rest field-by-field
    let ordered = ordered_wire_fields(vl);
    let split = ordered.partition_point(|fl| is_fixed_wire_field(fl));
    let (blast, rest) = ordered.split_at(split);
    let fixed_total: usize = blast
        .iter()
        .map(|fl| wire_fixed_size(&fl.source_ty).unwrap_or(0))
        .sum();
    if fixed_total > 0 {
        w.line(&format!(
            "{{ let __end = __pos.checked_add({fixed_total}).ok_or(Error::UnexpectedEof)?; \
             if __end > __buf.len() {{ return Err(Error::UnexpectedEof); }} *__pos = __end; }}"
        ));
    }

    let mut optional_counter = 0;
    for &fl in rest {
        emit_ordered_skip(fl, &mut optional_counter, w);
    }

    w.line("Ok(())");
    w.dedent();
    w.line("}");
}

// skips one field of the non-blastable tail, gating an Optional on its presence
// bit. `optional_counter` must be advanced across the tail in wire order, since
// that is how encode assigns the bits — seek walks the same tail, so it shares
// this rather than reimplementing the bit arithmetic.
//
// every Optional consumes a bit, including one whose inner type is zero-size and
// so emits no skip at all: encode still writes that bit and decode_vN still
// counts it, so bailing out early here would read every later optional's
// presence from the wrong bit.
pub(crate) fn emit_ordered_skip(
    fl: &FieldLineage,
    optional_counter: &mut usize,
    w: &mut CodeWriter,
) {
    emit_field_type_skip(&fl.source_ty, optional_counter, w);
}

pub(crate) fn emit_field_type_skip(
    ty: &ResolvedTypeRef,
    optional_counter: &mut usize,
    w: &mut CodeWriter,
) {
    if let ResolvedTypeRef::Optional(inner) = ty {
        let byte_idx = *optional_counter / 8;
        let bit_idx = *optional_counter % 8;
        *optional_counter += 1;
        if let Some(stmt) = emit_skip_stmt(inner) {
            w.line(&format!(
                "if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 {{"
            ));
            w.indent();
            w.line(&stmt);
            w.dedent();
            w.line("}");
        }
    } else if let Some(stmt) = emit_skip_stmt(ty) {
        w.line(&stmt);
    }
}
