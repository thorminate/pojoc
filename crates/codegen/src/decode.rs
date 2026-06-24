use super::writer::CodeWriter;
use crate::{emit_default, get_latest_versions};
use heck::ToSnakeCase;
use pojoc_core::types::*;
use pojoc_schema::ir::ir_types::*;
use pojoc_schema::ir::lineage::*;
use std::collections::{HashMap, HashSet};

pub fn emit_decode_functions(
    schema: &ResolvedSchema,
    infected: &HashSet<String>,
    w: &mut CodeWriter,
) {
    emit_type_readers(schema, w);
    emit_type_skippers(schema, w);
    emit_bitset_readers(schema, w);
    emit_union_readers(schema, infected, w);
    emit_union_skippers(schema, w);
    emit_lazy_helpers(schema, w);
    emit_skip_vn_functions(schema, w);

    for vl in &schema.lineage.versions {
        emit_decode_fn(schema, vl, infected, w);
        w.blank();
    }
}

fn emit_type_readers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.types.types, |id| id.name.clone(), |id| id.version);

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        let buf_param = if resolved.fields.is_empty() {
            "_buf"
        } else {
            "buf"
        };
        let pos_param = if resolved.fields.is_empty() {
            "_pos"
        } else {
            "pos"
        };
        let fn_name = format!("read_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}({buf_param}: &[u8], {pos_param}: &mut usize) -> PojocResult<{name}> {{"
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
                    let inner_expr = emit_read_expr(inner);
                    w.line(&format!(
                        "let {} = if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 {{ Some({inner_expr}) }} else {{ None }};",
                        field.name
                    ));
                }
                _ => {
                    let expr = emit_read_expr(&field.ty);
                    w.line(&format!("let {} = {expr};", field.name));
                }
            }
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

fn emit_type_skippers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.types.types, |id| id.name.clone(), |id| id.version);

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        let buf_param = if resolved.fields.is_empty() {
            "_buf"
        } else {
            "buf"
        };
        let pos_param = if resolved.fields.is_empty() {
            "_pos"
        } else {
            "pos"
        };
        let fn_name = format!("skip_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}({buf_param}: &[u8], {pos_param}: &mut usize) -> PojocResult<()> {{"
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

fn emit_union_readers(schema: &ResolvedSchema, infected: &HashSet<String>, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.unions.unions, |id| id.name.clone(), |id| id.version);
    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];

        for variant in &resolved.variants {
            let infected_name = match &variant.payload {
                ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) => Some(id.name.as_str()),
                _ => None,
            };
            if let Some(n) = infected_name {
                assert!(
                    !infected.contains(n),
                    "union `{name}` variant `{}` has a lazy-infected payload type `{n}` — \
             lazy fields inside union payloads aren't supported yet",
                    variant.name
                );
            }
        }

        let fn_name = format!("read_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}(buf: &[u8], pos: &mut usize) -> PojocResult<{name}> {{"
        ));
        w.indent();
        w.line("let __discriminant = read_varint64(buf, pos)?;");
        w.line("let __len = read_varint32(buf, pos)? as usize;");
        w.line("let __payload_start = *pos;");
        w.line(
            "let __payload_end = __payload_start.checked_add(__len).ok_or(Error::InvalidLength)?;",
        );
        w.line("if __payload_end > buf.len() { return Err(Error::InvalidLength); }");
        w.line("let __result = match __discriminant {");
        w.indent();
        for variant in &resolved.variants {
            let read_expr = emit_read_expr(&variant.payload);
            w.line(&format!(
                "{} => {name}::{}({read_expr}),",
                variant.discriminant, variant.name
            ));
        }
        w.line("other => {");
        w.indent();
        w.line("let data = buf[__payload_start..__payload_end].to_vec();");
        w.line("*pos = __payload_end;");
        w.line(&format!("{name}::Unknown {{ discriminant: other, data }}"));
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("};");
        w.line("if *pos != __payload_end { return Err(Error::InvalidLength); }");
        w.line("Ok(__result)");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_union_skippers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.unions.unions, |id| id.name.clone(), |id| id.version);
    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let fn_name = format!("skip_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}(buf: &[u8], pos: &mut usize) -> PojocResult<()> {{"
        ));
        w.indent();
        w.line("let _ = read_varint64(buf, pos)?;");
        w.line("let __len = read_varint32(buf, pos)? as usize;");
        w.line("let __end = pos.checked_add(__len).ok_or(Error::InvalidLength)?;");
        w.line("if __end > buf.len() { return Err(Error::InvalidLength); }");
        w.line("*pos = __end;");
        w.line("Ok(())");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_bitset_readers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(
        &schema.bitsets.bitsets,
        |id| id.name.clone(),
        |id| id.version,
    );

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, bs) = latest[name];
        let computed_len = bs.variants.len().div_ceil(8);
        let fn_name = format!("read_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}(buf: &[u8], pos: &mut usize) -> PojocResult<{name}> {{"
        ));
        w.indent();
        w.line(&format!(
            "let bytes = read_fixed_bytes::<{computed_len}>(buf, pos)?;"
        ));
        w.line(&format!("Ok({name}(bytes))"));
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_skip_stmt(ty: &ResolvedTypeRef) -> Option<String> {
    let info = type_info(ty);
    if let WireSize::Fixed(n) = info.wire_size {
        if n > 0 {
            return Some(format!(
                "{{ let __end = pos.checked_add({n}).ok_or(Error::UnexpectedEof)?; \
                 if __end > buf.len() {{ return Err(Error::UnexpectedEof); }} \
                 *pos = __end; }}"
            ));
        }
    }
    if let ResolvedTypeRef::FixedArray(_, n) = ty {
        if *n == 0 {
            return None;
        }
    }
    if let ResolvedTypeRef::FixedMap(_, _, n) = ty {
        if *n == 0 {
            return None;
        }
    }
    if let ResolvedTypeRef::FixedDeltaArray(_, n) = ty {
        if *n == 0 {
            return None;
        }
    }
    if let ResolvedTypeRef::FixedString(n) = ty {
        if *n == 0 {
            return None;
        }
    }

    Some(info.skip_stmt)
}

fn emit_decode_fn(
    schema: &ResolvedSchema,
    vl: &VersionLineage,
    infected: &HashSet<String>,
    w: &mut CodeWriter,
) {
    let name = &schema.name_hint;
    let v = vl.version;
    let needs_lifetime = infected.contains(name.as_str());
    let lifetime = if needs_lifetime { "<'buf>" } else { "" };
    let buf_ty = if needs_lifetime {
        "&'buf [u8]"
    } else {
        "&[u8]"
    };

    w.line(&format!(
        "pub fn decode_v{v}{lifetime}(buf: {buf_ty}, pos: &mut usize) -> PojocResult<{name}{lifetime}> {{"
    ));

    w.indent();

    let optional_count = vl
        .fields
        .iter()
        .filter(|fl| matches!(fl.source_ty, ResolvedTypeRef::Optional(_)))
        .count();
    emit_optional_header_read(optional_count, w);

    let mut optional_counter = 0;
    for fl in &vl.fields {
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
                let none_fn = format!("{}_none", mf.target_name);
                w.line(&format!(
                    "let {} = LazyView::new(&[], {none_fn});",
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
    w.line(&format!("Ok({name} {{"));
    w.indent();
    for field in &latest.fields {
        w.line(&format!("{},", field.name));
    }
    w.dedent();
    w.line("})");
    w.dedent();
    w.line("}");
}

fn emit_field_read(schema: &ResolvedSchema, fl: &FieldLineage, w: &mut CodeWriter) {
    match &fl.mapping {
        FieldMapping::Discard => {
            if let ResolvedTypeRef::Array(inner) = &fl.source_ty {
                if let WireSize::Fixed(stride) = type_info(inner).wire_size {
                    w.line(&format!(
                        "{{ let __n = read_array_len(buf, pos)? as usize; \
                 let __end = pos.checked_add(__n * {stride}).ok_or(Error::UnexpectedEof)?; \
                 if __end > buf.len() {{ return Err(Error::UnexpectedEof); }} \
                 *pos = __end; }}"
                    ));
                    return;
                }
            }
            if let Some(stmt) = emit_skip_stmt(&fl.source_ty) {
                w.line(&stmt);
            }
        }
        FieldMapping::PassThrough { target_name } => {
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if target_is_lazy {
                let some_fn = format!("{target_name}_some");
                w.line(&format!("let __{target_name}_start = *pos;"));
                if let Some(stmt) = emit_skip_stmt(&fl.source_ty) {
                    w.line(&stmt);
                }
                w.line(&format!("let {target_name} = LazyView::new(&buf[__{target_name}_start..*pos], {some_fn});"));
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
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if target_is_lazy {
                if let Some(stmt) = emit_skip_stmt(&fl.source_ty) {
                    w.line(&stmt);
                }
                let none_fn = format!("{target_name}_none");
                w.line(&format!(
                    "let {target_name} = LazyView::new(&[], {none_fn});"
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

fn emit_read_expr(ty: &ResolvedTypeRef) -> String {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(_) => format!("{}(buf, pos)?", info.read_fn),
        ResolvedTypeRef::Enum(id) => {
            format!("{{ let __raw = read_varint64(buf, pos)? as u32; {}::try_from(__raw).map_err(|_| Error::InvalidEnumVariant)? }}", id.name)
        }
        ResolvedTypeRef::Union(_) => format!("{}(buf, pos)?", info.read_fn),
        ResolvedTypeRef::Bitset(id, _) => {
            format!("read_{}(buf, pos)?", id.name.to_snake_case())
        }
        ResolvedTypeRef::FixedString(n) => {
            if *n == 0 {
                return "&[]".to_string();
            }
            format!("{}(buf, pos)?", info.read_fn)
        }
        ResolvedTypeRef::Array(inner) => {
            let inner_expr = emit_read_expr(inner);
            format!("{{ let __n = read_array_len(buf, pos)?; let mut __v = PojocVec::with_capacity(__n); for _ in 0..__n {{ __v.push({inner_expr}); }} __v }}")
        }
        ResolvedTypeRef::FixedArray(inner, n) => {
            if *n == 0 {
                return "[]".to_string();
            }
            let inner_expr = emit_read_expr(inner);
            let init = if let WireSize::Fixed(_) = type_info(inner).wire_size {
                let default = type_info(inner).default_expr;
                format!("[{default}; {n}]")
            } else {
                "std::array::from_fn(|_| Default::default())".to_string()
            };
            format!("{{ let mut __arr: [_; {n}] = {init}; for __slot in __arr.iter_mut() {{ *__slot = {inner_expr}; }} __arr }}")
        }
        ResolvedTypeRef::DeltaArray(inner) => {
            let inner_rust = type_info(inner).rust_type;
            format!("read_delta_array::<{inner_rust}>(buf, pos)?")
        }
        ResolvedTypeRef::FixedDeltaArray(inner, n) => {
            if *n == 0 {
                return "[]".to_string();
            }
            let inner_rust = type_info(inner).rust_type;
            format!("read_fixed_delta_array::<{inner_rust}, {n}>(buf, pos)?")
        }
        ResolvedTypeRef::Map(k_ty, v_ty) => {
            let ke = emit_read_expr(k_ty);
            let ve = emit_read_expr(v_ty);
            format!("{{ let __n = read_array_len(buf, pos)?; let mut __m = PojocMap::with_capacity(__n); for _ in 0..__n {{ let __k = {ke}; let __v = {ve}; __m.insert(__k, __v); }} __m }}")
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
                "(({}(buf, pos)? as f64) * {}f64 + {}f64) as f32",
                backing.read_fn(),
                step,
                min
            )
        }
        ResolvedTypeRef::Optional(inner) => {
            let inner_expr = emit_read_expr(inner);
            format!("if read_u8(buf, pos)? != 0 {{ Some({inner_expr}) }} else {{ None }}")
        }
        ResolvedTypeRef::ImportedSchema { .. } => {
            format!("{}(buf, pos)?", info.read_fn)
        }
    }
}

fn emit_optional_header_read(optional_count: usize, w: &mut CodeWriter) {
    let header_bytes = optional_count.div_ceil(8);
    if header_bytes > 0 {
        w.line(&format!(
            "let __header = read_fixed_bytes::<{header_bytes}>(buf, pos)?;"
        ));
    }
}

fn emit_field_mapping_arm(
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
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if target_is_lazy {
                let some_fn = format!("{target_name}_some");
                let none_fn = format!("{target_name}_none");
                w.line(&format!("let __{target_name}_start = *pos;"));
                if let Some(stmt) = emit_skip_stmt(inner) {
                    w.line(&format!("if {__present} {{"));
                    w.indent();
                    w.line(&stmt);
                    w.dedent();
                    w.line("}");
                    w.line(&format!(
                        "let {target_name} = if {__present} {{ \
                     LazyView::new(&buf[__{target_name}_start..*pos], {some_fn}) \
                     }} else {{ \
                     LazyView::new(&[], {none_fn}) \
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
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if target_is_lazy {
                if let Some(stmt) = emit_skip_stmt(inner) {
                    w.line(&format!("if {__present} {{"));
                    w.indent();
                    w.line(&stmt);
                    w.dedent();
                    w.line("}");
                    let none_fn = format!("{target_name}_none");
                    w.line(&format!(
                        "let {target_name} = LazyView::new(&[], {none_fn});"
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

fn get_field_default_expr(
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
            if *set {
                if let Some(idx) = bs.variants.iter().position(|v| v == flag_name) {
                    value |= 1u64 << idx;
                }
            }
        }
    }
    value
}

enum CastExpr {
    Inline(String),
    Block(String),
}

fn emit_cast_value(
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
                CastExpr::Inline(format!("{}(buf, pos)?", fi.read_fn))
            } else {
                CastExpr::Inline(format!("{}(buf, pos)? as {}", fi.read_fn, ti.rust_type))
            }
        }

        (Scalar(f), Scalar(t)) => CastExpr::Block(struct_cast_block(schema, f, t, indent)),

        (FixedString(from_n), FixedString(to_n)) => {
            let copy_n = (*from_n).min(*to_n);
            CastExpr::Block(format!(
                "{{ let __src = read_fixed_bytes::<{from_n}>(buf, pos)?; \
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
                "{{ let mut __m = PojocMap::with_capacity({n}); \
                 for __i in 0i32..{n} {{ let __v: {v_rust} = {elem_expr}; __m.insert(__i, __v); }} \
                 __m }}"
            ))
        }

        (Bitset(id, width), Scalar(t)) if is_primitive(&t.name) => {
            let to_rust = type_info(to).rust_type;
            let read_fn = format!("read_{}", id.name.to_snake_case());
            let expr = match width {
                1 => format!("{read_fn}(buf, pos)?.0[0] as {to_rust}"),
                2 => format!("u16::from_le_bytes({read_fn}(buf, pos)?.0) as {to_rust}"),
                _ => format!("u32::from_le_bytes({read_fn}(buf, pos)?.0) as {to_rust}"),
            };
            CastExpr::Inline(expr)
        }

        (FixedDeltaArray(from_elem, from_n), FixedDeltaArray(_, to_n)) => {
            let elem_rust = type_info(from_elem).rust_type;
            let elem_default = type_info(from_elem).default_expr;
            let copy_n = (*from_n).min(*to_n);
            CastExpr::Block(format!(
                "{{ let __src = read_fixed_delta_array::<{elem_rust}, {from_n}>(buf, pos)?; \
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

        _ => CastExpr::Inline(emit_read_expr(from)),
    }
}

// indent is brought in so the sub writer matches the parent writer's indent.
fn struct_cast_block(
    schema: &ResolvedSchema,
    from: &TypeId,
    to: &TypeId,
    indent: &usize,
) -> String {
    let mut sub = CodeWriter::default();
    sub.line("{");
    // set the indent after '{' so that the '{' doesn't get incorrectly indented.
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
            let default_str = if let Some(ref explicit_default) = dst.default {
                emit_default(explicit_default, schema)
            } else {
                type_info(&dst.ty).default_expr
            };
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

fn emit_lazy_helpers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let mut emitted: HashSet<String> = HashSet::new();
    let latest = schema.versions.last().unwrap();

    for fl in &latest.fields {
        if !fl.lazy {
            continue;
        }
        let target_name = &fl.name;
        let ty = &fl.ty;
        let is_optional = matches!(ty, ResolvedTypeRef::Optional(_));
        let rust_ty = type_info(ty).rust_type;

        let some_name = format!("{target_name}_some");
        if emitted.insert(some_name.clone()) {
            w.line("#[allow(dead_code)]");
            w.line(&format!(
                "fn {some_name}(buf: &[u8], pos: &mut usize) -> PojocResult<{rust_ty}> {{"
            ));
            w.indent();
            if is_optional {
                let inner = match ty {
                    ResolvedTypeRef::Optional(i) => &**i,
                    _ => unreachable!(),
                };
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
            w.dedent();
            w.line("}");
            w.blank();
        }

        let none_name = format!("{target_name}_none");
        if emitted.insert(none_name.clone()) {
            w.line("#[allow(dead_code)]");
            w.line(&format!(
                "fn {none_name}(_buf: &[u8], _pos: &mut usize) -> PojocResult<{rust_ty}> {{"
            ));
            w.indent();
            if is_optional {
                w.line("Ok(None)");
            } else {
                let default = type_info(ty).default_expr;
                w.line(&format!("Ok({default})"));
            }
            w.dedent();
            w.line("}");
            w.blank();
        }
    }
}

pub fn emit_skip_vn_functions(schema: &ResolvedSchema, w: &mut CodeWriter) {
    for vl in &schema.lineage.versions {
        emit_skip_vn_fn(vl, w);
        w.blank();
    }
}

fn emit_skip_vn_fn(vl: &VersionLineage, w: &mut CodeWriter) {
    let v = vl.version;
    let buf_param = if vl.fields.is_empty() { "_buf" } else { "buf" };
    let pos_param = if vl.fields.is_empty() { "_pos" } else { "pos" };

    w.line("#[allow(dead_code)]");
    w.line(&format!(
        "pub fn skip_v{v}({buf_param}: &[u8], {pos_param}: &mut usize) -> PojocResult<()> {{"
    ));
    w.indent();

    let optional_count = vl
        .fields
        .iter()
        .filter(|fl| matches!(fl.source_ty, ResolvedTypeRef::Optional(_)))
        .count();
    emit_optional_header_read(optional_count, w);

    let mut optional_counter = 0;
    for fl in &vl.fields {
        if let ResolvedTypeRef::Optional(inner) = &fl.source_ty {
            if let Some(stmt) = emit_skip_stmt(inner) {
                let byte_idx = optional_counter / 8;
                let bit_idx = optional_counter % 8;
                optional_counter += 1;
                w.line(&format!(
                    "if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 {{"
                ));
                w.indent();
                w.line(&stmt);
                w.dedent();
                w.line("}");
            }
        } else {
            if let Some(stmt) = emit_skip_stmt(&fl.source_ty) {
                w.line(&stmt);
            }
        }
    }

    w.line("Ok(())");
    w.dedent();
    w.line("}");
}
