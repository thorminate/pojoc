use super::decode::{emit_field_type_skip, emit_optional_header_read, emit_type_field_read};
use super::seek::{emit_bare_interned_read, emit_prologue, raw_seek_type, seek_type_alias};
use super::writer::{CodeWriter, VersionBodies, version_pattern};
use super::{emit_constraint_guard, ordered_wire_fields, version_uses_intern, wire_fixed_size};
use crate::core::types::{ResolvedTypeRef, TypeId, is_intern_infected, is_union_intern_infected};
use crate::schema::ir::ir_types::{FieldIR, ResolvedSchema, ResolvedType, latest_by_name};
use crate::schema::ir::lineage::{FieldLineage, FieldMapping, VersionLineage};
use std::collections::{HashMap, HashSet};

type NamedTypes<'a> = HashMap<String, (i128, &'a ResolvedType)>;

struct Selector<'a> {
    field: &'a FieldIR,
    reader: String,
    children: Vec<Selector<'a>>,
}

// Only direct, required local struct fields are containers. A leaf may have
// any seekable type. This also keeps recursive boxed edges out of the tree.
fn container(field: &FieldIR) -> Option<&TypeId> {
    match &field.ty {
        ResolvedTypeRef::Scalar(id) if !field.lazy => Some(id),
        _ => None,
    }
}

fn children<'a>(
    field: &'a FieldIR,
    latest: &NamedTypes<'a>,
    ancestors: &mut HashSet<&'a str>,
    next: &mut usize,
) -> Vec<Selector<'a>> {
    let Some(id) = container(field) else {
        return Vec::new();
    };
    let Some((_, shape)) = latest.get(&id.name) else {
        return Vec::new();
    };
    if !ancestors.insert(&id.name) {
        return Vec::new();
    }
    let children = shape
        .fields
        .iter()
        .filter(|field| field.seekable)
        .map(|field| {
            let reader = format!("__pluck_path_{next}");
            *next += 1;
            Selector {
                field,
                reader,
                children: children(field, latest, ancestors, next),
            }
        })
        .collect();
    ancestors.remove(id.name.as_str());
    children
}

pub(super) fn emit_paths(
    schema: &ResolvedSchema,
    targets: &[&FieldIR],
    w: &mut CodeWriter,
) -> bool {
    let latest = latest_by_name(&schema.types.types);
    let mut next = 0;
    let mut ancestors = HashSet::new();
    let roots: Vec<_> = targets
        .iter()
        .map(|field| Selector {
            field,
            reader: format!("seek_{}", field.name),
            children: children(field, &latest, &mut ancestors, &mut next),
        })
        .collect();
    if next == 0 {
        return false;
    }

    fn emit_readers<'a>(
        node: &Selector<'a>,
        path: &mut Vec<&'a FieldIR>,
        schema: &ResolvedSchema,
        latest: &NamedTypes<'_>,
        w: &mut CodeWriter,
    ) {
        path.push(node.field);
        if path.len() > 1 {
            emit_reader(schema, latest, path, &node.reader, w);
        }
        for child in &node.children {
            emit_readers(child, path, schema, latest, w);
        }
        path.pop();
    }

    let mut path = Vec::new();
    for root in &roots {
        emit_readers(root, &mut path, schema, &latest, w);
    }
    w.line("#[doc(hidden)]");
    w.line("#[allow(unused_imports)]");
    w.line("pub mod __pluck_paths {");
    w.indent();
    for root in &roots {
        emit_path_module(root, 2, w);
    }
    w.dedent();
    w.line("}");
    w.blank();
    true
}

fn emit_path_module(node: &Selector<'_>, depth: usize, w: &mut CodeWriter) {
    w.line(&format!("pub mod {} {{", node.field.name));
    w.indent();
    w.line(&format!(
        "pub use {}{} as read;",
        "super::".repeat(depth),
        node.reader
    ));
    for child in &node.children {
        emit_path_module(child, depth + 1, w);
    }
    w.dedent();
    w.line("}");
}

struct Hop<'a> {
    owner: &'a str,
    fields: &'a [FieldIR],
    field: &'a FieldIR,
    cast_reader: bool,
}

struct ReadPlan<'a> {
    prefix: Vec<&'a FieldLineage>,
    hops: Vec<Hop<'a>>,
    needs_table: bool,
}

fn direct_plan<'a>(
    schema: &'a ResolvedSchema,
    latest: &NamedTypes<'a>,
    version: &'a VersionLineage,
    path: &[&'a FieldIR],
) -> Option<ReadPlan<'a>> {
    let ordered = ordered_wire_fields(version);
    let root_index = ordered
        .iter()
        .position(|field| field.field_id == path[0].id)?;
    let (prefix, root) = ordered.split_at(root_index);
    let root = root[0];
    if root.lazy
        || prefix.iter().any(|field| {
            matches!(field.mapping, FieldMapping::Cast { .. })
            && field.source_ty.any_node(&mut |ty| {
                matches!(ty, ResolvedTypeRef::Scalar(id) if schema.types.types.contains_key(id))
            })
        })
    {
        return None;
    }

    let latest_id = container(path[0])?;
    let (shape, cast_reader) = match &root.mapping {
        FieldMapping::PassThrough { .. } => (latest.get(&latest_id.name)?.1, false),
        FieldMapping::Cast {
            from: ResolvedTypeRef::Scalar(from),
            to: ResolvedTypeRef::Scalar(to),
            ..
        } if from.name == to.name && to == latest_id && to.version == latest.get(&to.name)?.0 => {
            (schema.types.types.get(from)?, true)
        }
        _ => return None,
    };
    let mut fields = shape.fields.as_slice();
    let mut owner = latest_id.name.as_str();
    let mut cast_reader = cast_reader;
    let mut hops = Vec::with_capacity(path.len() - 1);
    for (index, target) in path.iter().enumerate().skip(1) {
        // Historical struct casts bind fields by ID, including renamed fields.
        // A missing field or a changed value type is handled by the decoder.
        let field = fields.iter().find(|field| field.id == target.id)?;
        if field.lazy != target.lazy
            || (cast_reader && (field.lazy || (index + 1 == path.len() && field.ty != target.ty)))
        {
            return None;
        }
        hops.push(Hop {
            owner,
            fields,
            field,
            cast_reader,
        });
        if index + 1 < path.len() {
            let id = container(field)?;
            let target_id = container(target)?;
            if id.name != target_id.name {
                return None;
            }
            // A struct cast's nested fields go through the regular named
            // reader, just like fields of a non-cast named struct.
            fields = latest.get(&id.name)?.1.fields.as_slice();
            owner = id.name.as_str();
            cast_reader = false;
        }
    }
    let leaf = hops.last()?;
    let needs_table = leaf.field.ty.any_node(&mut |ty| match ty {
        ResolvedTypeRef::Interned(_) => true,
        ResolvedTypeRef::Scalar(id) => is_intern_infected(&id.name),
        ResolvedTypeRef::Union(id) => is_union_intern_infected(id),
        _ => false,
    });
    if needs_table && !version_uses_intern(schema, &version.fields) {
        return None;
    }
    Some(ReadPlan {
        prefix: prefix.to_vec(),
        hops,
        needs_table,
    })
}

fn emit_reader(
    schema: &ResolvedSchema,
    latest: &NamedTypes<'_>,
    path: &[&FieldIR],
    reader: &str,
    w: &mut CodeWriter,
) {
    let leaf = path.last().unwrap();
    let ty = raw_seek_type(leaf);
    let ret = if seek_type_alias(leaf).is_some() {
        let alias = format!(
            "__PluckPath{}Value",
            reader.trim_start_matches("__pluck_path_")
        );
        let lifetime = if ty.contains("'buf") { "<'buf>" } else { "" };
        w.line("#[doc(hidden)]");
        w.line(&format!("pub type {alias}{lifetime} = {ty};"));
        format!("{alias}{lifetime}")
    } else {
        ty
    };
    let ret = ret.replace("'buf", "'_");
    w.line("#[doc(hidden)]");
    w.line("#[allow(dead_code)]");
    w.line(&format!(
        "pub fn {reader}(buf: &[u8]) -> PojocResult<{ret}> {{"
    ));
    w.indent();
    w.line("let envelope = open_envelope(buf, &mut 0)?;");
    w.line("let __buf = envelope.payload;");
    w.line("match envelope.version {");
    w.indent();

    let projection = path
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>()
        .join(".");
    let mut readers = VersionBodies::default();
    for version in &schema.lineage.versions {
        let mut body = CodeWriter::default();
        body.indent = w.indent + 1;
        if let Some(plan) = direct_plan(schema, latest, version, path) {
            emit_read_plan(schema, version, &plan, leaf, &mut body);
        } else {
            body.line(&format!(
                "Ok(decode_payload(envelope.version, __buf)?.{projection})"
            ));
        }
        readers.insert(version.version, body);
    }
    for (body, versions) in readers.into_groups() {
        w.line(&format!("{} => {{", version_pattern(&versions)));
        w.append(&body);
        w.line("},");
    }
    w.line("version => Err(Error::UnsupportedVersion(version)),");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();
}

fn emit_read_plan(
    schema: &ResolvedSchema,
    version: &VersionLineage,
    plan: &ReadPlan<'_>,
    leaf: &FieldIR,
    w: &mut CodeWriter,
) {
    w.line("let __start = 0;");
    let bare_intern = matches!(leaf.ty, ResolvedTypeRef::Interned(_));
    emit_prologue(schema, version, plan.needs_table, bare_intern, w);
    emit_prefix(plan.prefix.iter().map(|field| &field.source_ty), w);
    for (index, hop) in plan.hops.iter().enumerate() {
        let optional_count = hop
            .fields
            .iter()
            .filter(|field| matches!(field.ty, ResolvedTypeRef::Optional(_)))
            .count();
        emit_optional_header_read(optional_count, w);
        let mut optional_counter = emit_prefix(
            hop.fields
                .iter()
                .take_while(|field| field.id != hop.field.id)
                .map(|field| &field.ty),
            w,
        );
        if index + 1 == plan.hops.len() {
            if bare_intern {
                emit_bare_interned_read(&hop.field.name, w);
            } else {
                emit_type_field_read(hop.owner, hop.field, &mut optional_counter, w);
            }
            if !hop.cast_reader {
                emit_constraint_guard(leaf, &hop.field.name, w);
            }
            w.line(&format!("Ok({})", hop.field.name));
        }
    }
}

fn emit_prefix<'a>(
    fields: impl Iterator<Item = &'a ResolvedTypeRef> + Clone,
    w: &mut CodeWriter,
) -> usize {
    let fixed = fields
        .clone()
        .try_fold(0usize, |bytes, ty| bytes.checked_add(wire_fixed_size(ty)?));
    if let Some(bytes) = fixed {
        if bytes > 0 {
            w.line(&format!(
                "let __end = __pos.checked_add({bytes}).ok_or(Error::UnexpectedEof)?;"
            ));
            w.line("if __end > __buf.len() { return Err(Error::UnexpectedEof); }");
            w.line("*__pos = __end;");
        }
        return 0;
    }
    let mut optional_counter = 0;
    for ty in fields {
        emit_field_type_skip(ty, &mut optional_counter, w);
    }
    optional_counter
}
