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
    let fallback =
        "stream::read_buffered_reply(input, payload_len, &mut request, &mut response, transcode)?";

    w.line("/// Reply to each Pojoc envelope with one latest-version envelope.");
    w.line("/// Flush each reply and reject a declared payload above `max_payload_bytes`.");
    w.line("pub fn serve(");
    w.indent();
    w.line("input: &mut impl std::io::BufRead,");
    w.line("output: &mut impl std::io::Write,");
    w.line("max_payload_bytes: usize,");
    w.dedent();
    w.line(") -> std::io::Result<()> {");
    w.indent();
    w.line("use runtime::stream;");
    w.line("let mut request = Vec::new();");
    w.line("let mut response = Vec::new();");
    if !versions.is_empty() {
        w.line(&format!(
            "let mut byte_array = stream::ByteArrayStream::new({}, {preserve_length});",
            latest.version
        ));
    }
    if !hybrid.is_empty() {
        w.line("let mut hybrid = stream::HybridStream::default();");
    }
    w.line("stream::serve_frames(input, output, max_payload_bytes, |input, output, version, payload_len| {");
    w.indent();
    w.line("let transcode = |payload: &[u8], reply: &mut Vec<u8>| {");
    w.indent();
    w.line("let value = decode_payload(version, payload)?;");
    w.line(&format!(
        "encode_for_version(reply, &value, {})",
        latest.version
    ));
    w.dedent();
    w.line("};");

    if versions.is_empty() && hybrid.is_empty() {
        w.line(&format!("let reply_len = {fallback};"));
        w.line("output.write_all(&response[..reply_len])");
    } else {
        w.line("let reply_len = match version {");
        w.indent();
        for (version, prefix_len) in versions {
            w.line(&format!(
                "{version} if payload_len > {prefix_len} => Some(byte_array.read_reply("
            ));
            w.indent();
            w.line(&format!(
                "input, payload_len, {prefix_len}, &mut response, |prefix, header| {{"
            ));
            w.indent();
            w.line(&format!("let value = decode_v{version}(prefix, &mut 0)?;"));
            let question = if fallible_encode { "?" } else { "" };
            w.line(&format!(
                "encode_v{}(header, &value){question};",
                latest.version
            ));
            w.line("Ok(())");
            w.dedent();
            w.line("},");
            w.dedent();
            w.line(")?),");
        }
        hybrid.emit_arms(w);
        w.line(&format!("_ => Some({fallback}),"));
        w.dedent();
        w.line("};");
        w.line("if let Some(reply_len) = reply_len {");
        w.indent();
        w.line("output.write_all(&response[..reply_len])?;");
        w.dedent();
        w.line("}");
        w.line("Ok(())");
    }
    w.dedent();
    w.line("})");
    w.dedent();
    w.line("}");
    w.blank();

    let root = &schema.name_hint;
    let lifetime = if is_infected(root) { "<'_>" } else { "" };
    w.line("/// Visit the decoded value from each frame while its payload is available.");
    w.line("pub fn serve_decode(");
    w.indent();
    w.line("input: &mut impl std::io::BufRead,");
    w.line("max_payload_bytes: usize,");
    w.line(&format!(
        "mut visit: impl FnMut({root}{lifetime}) -> std::io::Result<()>,"
    ));
    w.dedent();
    w.line(") -> std::io::Result<()> {");
    w.indent();
    w.line("use runtime::stream;");
    w.line("stream::visit_payloads(input, max_payload_bytes, |version, payload| {");
    w.indent();
    w.line("visit(decode_payload(version, payload).map_err(stream::codec_error)?)");
    w.dedent();
    w.line("})");
    w.dedent();
    w.line("}");
}
