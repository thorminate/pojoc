use super::literals::{PRINT_WIDTH, string_literal};
use super::readers::record_array;
use crate::core::types::primitive_info;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::Write;

pub(super) fn wire_version(version: u64) -> String {
    if version < 128 {
        version.to_string()
    } else {
        format!("__wireVersion{version}")
    }
}

pub(super) enum CompiledWriter<'a> {
    Varints(&'a [Value]),
    Rows { name: &'a str, fields: &'a [Value] },
}

pub(super) fn scalar_writer(kind: &str) -> Option<&'static str> {
    if kind == "intern" {
        Some("writeIntern")
    } else {
        primitive_info(kind).map(|info| info.typescript_writer)
    }
}

fn scalar_kind(field: &Value) -> Option<&str> {
    let ty = &field["ty"];
    let ty = if optional(field) { &ty[1] } else { ty };
    ty.as_str()
}

fn scalar_fields(fields: &[Value]) -> bool {
    fields
        .iter()
        .all(|field| field["cast"] == false && scalar_kind(field).and_then(scalar_writer).is_some())
}

impl<'a> CompiledWriter<'a> {
    pub(super) fn new(root: &'a Value, records: &'a Value, version: u64) -> Option<Self> {
        let fields = root["fields"].as_array()?.as_slice();
        if fields.is_empty() {
            return None;
        }
        if version < 128
            && fields.len() <= 51
            && fields
                .iter()
                .all(|f| f["ty"] == "varint32" && f["cast"] == false)
        {
            return Some(Self::Varints(fields));
        }
        let (name, record) = record_array(fields)?;
        let fields = records[record].as_array()?.as_slice();
        scalar_fields(fields).then_some(Self::Rows { name, fields })
    }

    pub(super) fn imports(&self) -> BTreeSet<&'static str> {
        let fields = match self {
            Self::Varints(_) => {
                return [
                    "checkedVarint32",
                    "object",
                    "varint32Size",
                    "writeU32At",
                    "writeVarint32",
                ]
                .into();
            }
            Self::Rows { fields, .. } => *fields,
        };
        let mut imports = BTreeSet::from([
            "encodeCompiledPayload",
            "object",
            "sequence",
            "type WriteCursor",
            "type WriteTable",
            "writeLength",
        ]);
        if fields.iter().any(optional) {
            imports.insert("takeWrite");
        }
        imports.extend(
            fields
                .iter()
                .map(|field| scalar_writer(scalar_kind(field).unwrap()).unwrap()),
        );
        imports
    }

    pub(super) fn emit(&self, out: &mut String, version: u64, intern: bool) {
        out.push_str("\n// Current-version encoder.\n");
        match self {
            Self::Varints(fields) => {
                emit_varints(out, fields, version);
                return;
            }
            Self::Rows { name, fields } => {
                emit_scalars(out, "__writeRow", fields);
                emit_writer_signature(out, "__writePayload");
                let name = string_literal(name);
                writeln!(out, "  const rows = sequence(value[{name}]);").unwrap();
                out.push_str(concat!(
                    "  writeLength(w, rows.length);\n",
                    "  for (let i = 0; i < rows.length; i++) __writeRow(w, object(rows[i]), table);\n",
                    "}\n",
                ));
            }
        }
        let version = wire_version(version);
        out.push_str("export function encode(value: __Current): Uint8Array<ArrayBuffer> {\n");
        let statement =
            format!("  return encodeCompiledPayload(value, {version}, {intern}, __writePayload);");
        if statement.len() <= PRINT_WIDTH {
            writeln!(out, "{statement}").unwrap();
        } else {
            writeln!(out, "  return encodeCompiledPayload(\n    value,\n    {version},\n    {intern},\n    __writePayload,\n  );").unwrap();
        }
        out.push_str("}\n");
    }
}

fn emit_varints(out: &mut String, fields: &[Value], version: u64) {
    out.push_str(
        "export function encode(value: __Current): Uint8Array<ArrayBuffer> {\n  const fields = object(value);\n",
    );
    for (i, field) in fields.iter().enumerate() {
        let name = string_literal(field["name"].as_str().unwrap());
        writeln!(out, "  const v{i} = checkedVarint32(fields[{name}]);").unwrap();
    }
    let binding = if fields.len() == 1 { "const" } else { "let" };
    writeln!(out, "  {binding} length = varint32Size(v0);").unwrap();
    for i in 1..fields.len() {
        writeln!(out, "  length += varint32Size(v{i});").unwrap();
    }
    writeln!(out, "  const bytes = new Uint8Array(5 + length);\n  bytes[0] = {version};\n  writeU32At(bytes, 1, length);").unwrap();
    let payload_start = 5;
    if fields.len() > 1 {
        let binding = if fields.len() == 2 { "const" } else { "let" };
        writeln!(
            out,
            "  {binding} pos = writeVarint32(bytes, {payload_start}, v0);"
        )
        .unwrap();
        for i in 1..fields.len() - 1 {
            writeln!(out, "  pos = writeVarint32(bytes, pos, v{i});").unwrap();
        }
        writeln!(out, "  writeVarint32(bytes, pos, v{});", fields.len() - 1).unwrap();
    } else {
        writeln!(out, "  writeVarint32(bytes, {payload_start}, v0);").unwrap();
    }
    out.push_str("  return bytes;\n}\n");
}

fn emit_writer_signature(out: &mut String, name: &str) {
    writeln!(
        out,
        "function {name}(\n  w: WriteCursor,\n  value: Record<string, unknown>,\n  table: WriteTable,\n): void {{"
    ).unwrap();
}

fn optional(field: &Value) -> bool {
    field["ty"].get(0).and_then(Value::as_str) == Some("optional")
}

fn emit_scalars(out: &mut String, name: &str, fields: &[Value]) {
    emit_writer_signature(out, name);
    let header = fields
        .iter()
        .filter(|field| optional(field))
        .count()
        .div_ceil(8);
    if header > 0 {
        writeln!(out, "  const header = takeWrite(w, {header});").unwrap();
    }
    let mut bit = 0;
    for (i, field) in fields.iter().enumerate() {
        let name = string_literal(field["name"].as_str().unwrap());
        let is_optional = optional(field);
        let kind = scalar_kind(field).unwrap();
        let write = scalar_writer(kind).unwrap();
        let table = if kind == "intern" { ", table" } else { "" };
        writeln!(out, "  const v{i} = value[{name}];").unwrap();
        if is_optional {
            writeln!(out, "  if (v{i} != null) {{\n    w.data[header + {}] |= {};\n    {write}(w, v{i}{table});\n  }}", bit / 8, 1u8 << (bit % 8)).unwrap();
            bit += 1;
        } else {
            writeln!(out, "  {write}(w, v{i}{table});").unwrap();
        }
    }
    out.push_str("}\n");
}
