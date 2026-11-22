use super::{is_fixed_wire_field, ordered_wire_fields, version_uses_intern, wire_fixed_size};
use super::{stream_hybrid::HybridPlans, writer::CodeWriter};
use crate::core::types::{ResolvedTypeRef, is_infected, normalize_type};
use crate::schema::ir::ir_types::ResolvedSchema;
use crate::schema::ir::lineage::{FieldLineage, FieldMapping, VersionLineage};

fn byte_array_layout<'a>(
    schema: &ResolvedSchema,
    version: &'a VersionLineage,
) -> Option<(&'a FieldLineage, usize)> {
    let ordered = ordered_wire_fields(version);
    let (&tail, prefix) = ordered.split_last()?;
    let ResolvedTypeRef::Array(element) = &tail.source_ty else {
        return None;
    };
    if !matches!(element.as_ref(), ResolvedTypeRef::Scalar(id) if normalize_type(&id.name) == "u8")
    {
        return None;
    }
    let prefix_len = prefix.iter().try_fold(0_usize, |size, field| {
        if !is_fixed_wire_field(field) {
            return None;
        }
        size.checked_add(wire_fixed_size(&field.source_ty)?)
    })?;
    if version_uses_intern(schema, &version.fields) {
        return None;
    }
    Some((tail, prefix_len))
}

pub(super) fn emit_serve(schema: &ResolvedSchema, fallible_encode: bool, w: &mut CodeWriter) {
    let latest = schema.lineage.versions.last().unwrap();
    let latest_tail = byte_array_layout(schema, latest).map(|(tail, _)| tail);
    let versions: Vec<_> = schema
        .lineage
        .versions
        .iter()
        .filter_map(|version| {
            let target = latest_tail?;
            let (tail, prefix_len) = byte_array_layout(schema, version)?;
            match &tail.mapping {
                FieldMapping::PassThrough { target_name } if *target_name == target.source_name => {
                    Some((version.version, prefix_len))
                }
                _ => None,
            }
        })
        .collect();

    let preserve_length = latest_tail.is_some_and(|tail| tail.lazy);
    let hybrid = HybridPlans::new(schema, &versions);
    let fallback = "read_buffered_reply(input, version, payload_len, &mut request, &mut response)?";
    let root = &schema.name_hint;
    let lifetime = if is_infected(root) { "<'_>" } else { "" };
    let stream_value = format!("{root}{lifetime}");
    for line in include_str!("stream.rs.in").lines() {
        match line.trim() {
            "// STREAM_READ_IMPORT" if !versions.is_empty() || !hybrid.is_empty() => {
                w.line("    use __pojoc_stream::read_stream_varint;");
                w.line("    use std::io::Read;");
            }
            "// STREAM_READ_IMPORT" => {}
            "// BYTE_ARRAY_STREAM" if !versions.is_empty() => {
                for line in include_str!("stream_byte_array.rs.in").lines() {
                    w.line(line);
                }
                w.line(&format!(
                    "    let mut byte_array = ByteArrayStream::new({}, {preserve_length});",
                    latest.version
                ));
            }
            "// BYTE_ARRAY_STREAM" => {}
            "// HYBRID_STREAM" if !hybrid.is_empty() => {
                for line in include_str!("stream_hybrid.rs.in").lines() {
                    w.line(line);
                }
                w.line("    let mut hybrid = HybridStream::default();");
            }
            "// HYBRID_STREAM" => {}
            "// STREAM_REPLY" if versions.is_empty() && hybrid.is_empty() => {
                w.line(&format!("        let reply_len = Some({fallback});"));
            }
            "// STREAM_REPLY" => {
                w.line("        let reply_len = match version {");
                for (version, prefix_len) in &versions {
                    w.line(&format!(
                        "            {version} if payload_len > {prefix_len} => Some(byte_array.read_reply("
                    ));
                    w.line(&format!(
                        "                input, payload_len, {prefix_len}, &mut response, |prefix, header| {{"
                    ));
                    w.line(&format!(
                        "                    let value = decode_v{version}(prefix, &mut 0)?;"
                    ));
                    let question = if fallible_encode { "?" } else { "" };
                    w.line(&format!(
                        "                    encode_v{}(header, &value){question};",
                        latest.version
                    ));
                    w.line("                    Ok(())");
                    w.line("                },");
                    w.line("            )?),");
                }
                w.indent();
                w.indent();
                w.indent();
                hybrid.emit_arms(w);
                w.dedent();
                w.dedent();
                w.dedent();
                w.line(&format!("            _ => Some({fallback}),"));
                w.line("        };");
            }
            _ if line.contains("StreamValue") => {
                w.line(&line.replace("StreamValue", &stream_value))
            }
            _ => w.line(line),
        }
    }
}
