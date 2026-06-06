use std::collections::{HashMap, HashSet};
use pojoc_schema::ir::types::*;
use pojoc_schema::ir::lineage::*;
use crate::emit_default;
use super::types::*;
use super::writer::CodeWriter;

pub fn emit_decode_functions(schema: &ResolvedSchema, w: &mut CodeWriter) {
    // emit a helper read function for each user-defined type, latest version
    emit_type_readers(schema, w);

    // emit per-version decode functions
    for vl in &schema.lineage.versions {
        emit_decode_fn(schema, vl, w);
        w.blank();
    }
}

fn emit_type_readers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let mut latest: HashMap<String, (u32, &ResolvedType)> = HashMap::new();

    for (type_id, resolved) in &schema.types.types {
        let entry = latest.entry(type_id.name.clone()).or_insert((0, resolved));
        if type_id.version > entry.0 {
            *entry = (type_id.version, resolved);
        }
    }

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        let fn_name = format!("read_{}", name.to_lowercase());
        w.line(&format!("fn {fn_name}(buf: &[u8], pos: &mut usize) -> Result<{name}> {{"));
        w.indent();
        for field in &resolved.fields {
            let expr = emit_read_expr(&field.ty);
            w.line(&format!("let {} = {expr};", field.name));
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

fn emit_decode_fn(schema: &ResolvedSchema, vl: &VersionLineage, w: &mut CodeWriter) {
    let name = &schema.name_hint;
    let v = vl.version;

    w.line(&format!("pub fn decode_v{v}(buf: &[u8], pos: &mut usize) -> Result<{name}> {{"));
    w.indent();

    for fl in &vl.fields {
        emit_field_read(schema, fl, w);
    }

    if !vl.missing.is_empty() {
        w.blank();
        for mf in &vl.missing {
            let default = mf.default.as_ref()
                .map(emit_default)
                .unwrap_or_else(|| default_value(&mf.ty).to_string()); // fallback for fields without explicit default
            w.line(&format!("let {} = {default};", mf.target_name));
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
            let expr = emit_read_expr(&fl.source_ty);
            w.line(&format!("let _ = {expr};"));
        }

        FieldMapping::PassThrough { target_name } => {
            let expr = emit_read_expr(&fl.source_ty);
            w.line(&format!("let {target_name} = {expr};"));
        }

        FieldMapping::Cast { target_name, from, to } => {
            if is_primitive(&from.name) && is_primitive(&to.name) {
                let expr = read_call(&from.name);
                let to_ty = rust_scalar_type(&to.name);
                w.line(&format!("let {target_name} = {expr} as {to_ty};"));
            } else {
                emit_struct_cast(schema, target_name, from, to, w);
            }
        }
    }
}

fn emit_struct_cast(
    schema: &ResolvedSchema,
    var_name: &str,
    from: &TypeId,
    to: &TypeId,
    w: &mut CodeWriter,
) {
    let from_type = schema.types.types.get(from)
        .expect("struct cast source not found");
    let to_type = schema.types.types.get(to)
        .expect("struct cast target not found");

    let to_by_id: HashMap<FieldId, &FieldIR> =
        to_type.fields.iter().map(|f| (f.id, f)).collect();
    let from_ids: HashSet<FieldId> =
        from_type.fields.iter().map(|f| f.id).collect();
    let to_ids: HashSet<FieldId> =
        to_type.fields.iter().map(|f| f.id).collect();

    w.line(&format!("let {var_name} = {{"));
    w.indent();

    // read wire fields (source shape)
    for src in &from_type.fields {
        let expr = emit_read_expr(&src.ty);
        if to_ids.contains(&src.id) {
            let dst_name = &to_by_id[&src.id].name;
            w.line(&format!("let {dst_name} = {expr};"));
        } else {
            w.line(&format!("let _ = {expr};"));
        }
    }

    // default fields new in target
    for dst in &to_type.fields {
        if !from_ids.contains(&dst.id) {
            let default = dst.default.as_ref()
                .map(emit_default)
                .unwrap_or_else(|| default_value(&dst.ty).to_string());
            w.line(&format!("let {} = {default};", dst.name));
        }
    }

    w.line(&format!("{} {{", to.name));
    w.indent();
    for f in &to_type.fields {
        w.line(&format!("{},", f.name));
    }
    w.dedent();
    w.line("}");

    w.dedent();
    w.line("};");
}

fn emit_read_expr(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Scalar(id) => {
            if is_primitive(&id.name) {
                read_call(&id.name).to_string() // already has?
            } else {
                format!("read_{}(buf, pos)?", id.name.to_lowercase())
            }
        }
        ResolvedTypeRef::Array(inner) => {
            let inner_expr = emit_read_expr_in_closure(inner);
            format!("{{ let n = read_array_len(buf, pos)? as usize; (0..n).map(|_| {inner_expr}).collect::<Result<PojocVec<_>>>()? }}")
        }
    }
}

fn emit_read_expr_in_closure(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Scalar(id) => {
            if normalize_type(&id.name) == "string" {
                "read_pojoc_string(buf, pos)".to_string() // returns Result, no?
            } else if is_primitive(&id.name) {
                format!("Ok({})", read_call(&id.name)) // wrap primitive in Ok
            } else {
                format!("read_{}(buf, pos)", id.name.to_lowercase()) // assume returns Result
            }
        }
        ResolvedTypeRef::Array(inner) => {
            // nested array still needs to return Result
            let inner_expr = emit_read_expr_in_closure(inner);
            format!("{{ let n = read_array_len(buf, pos)? as usize; (0..n).map(|_| {inner_expr}).collect::<Result<PojocVec<_>>>() }}")
        }
    }
}