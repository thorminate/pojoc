use super::super::literals::string_literal;
use super::super::readers::fixed_scalar;
use super::super::writers::scalar_writer;
use super::{Op, Program, call, count, optional, tag};
use crate::core::types::primitive_info;
use serde_json::Value;
use std::fmt::Write;

pub(super) fn typed_array(ty: &Value) -> Option<&'static str> {
    primitive_info(ty.as_str()?)?.typescript_array
}

fn scalar_reader(ty: &Value) -> Option<&'static str> {
    match ty.as_str()? {
        "string" => Some("readString"),
        "varint32" => Some("readVarint32"),
        "varint64" => Some("readVarint"),
        "intern" => Some("readIntern"),
        _ => None,
    }
}

impl Program<'_> {
    pub(super) fn reader(&mut self, scope: usize, ty: &Value, historical: bool) -> String {
        if let Some(read) = scalar_reader(ty) {
            return self.use_symbol(read).into();
        }
        if tag(ty) == "record" {
            return self.read_layout(self.record(scope, ty, historical), historical);
        }
        self.read_op(scope, ty, historical)
    }

    pub(super) fn read_args(&self, ty: &Value) -> &'static [&'static str] {
        match ty.as_str() {
            Some("string" | "varint32" | "varint64") => &["r"],
            _ => &["r", "table"],
        }
    }

    fn read_op(&mut self, scope: usize, ty: &Value, historical: bool) -> String {
        if tag(ty) == "record" {
            return self.read_layout(self.record(scope, ty, historical), historical);
        }
        let (id, build) = self.reserve(Op::Read, scope, format!("value:{historical}:{ty}"));
        let f = format!("__read{id}");
        if !build {
            return f;
        }
        let mut out = format!("function {f}(r: Cursor, table: ReadTable): unknown {{\n");
        if let Some(primitive) = ty.as_str() {
            self.use_symbol("readScalar");
            writeln!(out, "  return readScalar(r, \"{primitive}\", table);").unwrap();
        } else {
            match tag(ty) {
                "bytes" => {
                    self.use_symbol("readBytes");
                    writeln!(out, "  return readBytes(r, {});", ty[1]).unwrap();
                }
                "bits" => {
                    self.use_symbol("readBytes");
                    self.use_symbol("resized");
                    writeln!(out, "  return resized(readBytes(r, {}), {});", ty[1], ty[2]).unwrap();
                }
                "enum" => {
                    self.use_symbol("readUint");
                    let valid = self.enum_check(scope, &ty[2]);
                    writeln!(out, "  const value = readUint(r, {});", ty[1]).unwrap();
                    writeln!(
                        out,
                        "  if (!{valid}(value)) invalid(\"Invalid Pojoc enum discriminant\");"
                    )
                    .unwrap();
                    out.push_str("  return value;\n");
                }
                "union" => self.read_union(&mut out, scope, ty),
                "import" => {
                    let (scope, version) = self.imported(scope, ty);
                    let read = self.read_root(scope, version);
                    writeln!(out, "  return {read}(r);").unwrap();
                }
                "optional" => {
                    self.use_symbol("readByte");
                    let read = self.reader(scope, &ty[1], historical);
                    writeln!(
                        out,
                        "  return readByte(r) ? {read}({}) : null;",
                        self.read_args(&ty[1]).join(", ")
                    )
                    .unwrap();
                }
                "constraint" | "box" => {
                    let read = self.reader(scope, &ty[1], historical);
                    call(&mut out, 2, "return ", &read, self.read_args(&ty[1]), ";");
                }
                "vfloat" => {
                    self.use_symbol("readUint");
                    writeln!(
                        out,
                        "  return Math.fround({} + readUint(r, {}) * {});",
                        ty[1], ty[3], ty[2]
                    )
                    .unwrap();
                }
                "tuple" => {
                    let items = ty[1].as_array().unwrap();
                    if items.is_empty() {
                        out.push_str("  return [];\n");
                    } else {
                        let values: Vec<_> = items
                            .iter()
                            .map(|item| {
                                let read = self.reader(scope, item, false);
                                format!("{read}({})", self.read_args(item).join(", "))
                            })
                            .collect();
                        let inline = format!("  return [{}];", values.join(", "));
                        if inline.len() <= 80 {
                            writeln!(out, "{inline}").unwrap();
                        } else {
                            out.push_str("  return [\n");
                            for value in values {
                                writeln!(out, "    {value},").unwrap();
                            }
                            out.push_str("  ];\n");
                        }
                    }
                }
                "delta" => {
                    self.use_symbol("decodeDelta");
                    let count = self.array_count(&ty[2]);
                    writeln!(out, "  return decodeDelta(r, {}, {count});", ty[1]).unwrap();
                }
                "array" => self.read_array(&mut out, scope, &ty[1], &ty[2]),
                "map" => {
                    self.use_symbol("readPairs");
                    let key = self.read_op(scope, &ty[1], false);
                    let item = self.read_op(scope, &ty[2], false);
                    call(
                        &mut out,
                        2,
                        "return ",
                        "readPairs",
                        &["r", &ty[3].to_string(), "table", &key, &item],
                        ";",
                    );
                }
                tag => unreachable!("unhandled TypeScript reader for {tag}"),
            }
        }
        out.push_str("}\n");
        self.functions[id] = out;
        f
    }

    fn array_count(&mut self, size: &Value) -> String {
        if size.is_null() {
            self.use_symbol("readLength");
        }
        count(size)
    }

    fn read_array(&mut self, out: &mut String, scope: usize, item: &Value, size: &Value) {
        let count = self.array_count(size);
        if let Some(ctor) = typed_array(item) {
            self.use_symbol("readPacked");
            call(out, 2, "return ", "readPacked", &["r", ctor, &count], ";");
        } else if item == "varint32" {
            self.use_symbol("readVarint32Array");
            call(out, 2, "return ", "readVarint32Array", &["r", &count], ";");
        } else if tag(item) == "record"
            && let record = self.record(scope, item, false)
            && let Some(width) = self.fixed_layout(record)
        {
            let read = self.read_fixed_layout(record);
            self.use_symbol("readFixedRecords");
            call(
                out,
                2,
                "return ",
                "readFixedRecords",
                &["r", &count, &width.to_string(), &read],
                ";",
            );
        } else {
            let read = self.read_op(scope, item, false);
            self.use_symbol("readItems");
            call(
                out,
                2,
                "return ",
                "readItems",
                &["r", &count, "table", &read],
                ";",
            );
        }
    }

    fn enum_check(&mut self, scope: usize, items: &Value) -> String {
        let (id, build) = self.reserve(Op::Read, scope, format!("enum:{items}"));
        let f = format!("__enum{id}");
        if build {
            self.use_symbol("invalid");
            let mut values = items
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect::<BTreeSet<_>>();
            values.insert(0);
            let last = *values.last().unwrap();
            let mut out = format!("function {f}(value: number): boolean {{\n");
            if last as usize == values.len() - 1 {
                writeln!(
                    out,
                    "  return Number.isInteger(value) && value >= 0 && value <= {last};"
                )
                .unwrap();
            } else {
                out.push_str("  switch (value) {\n");
                for value in values {
                    writeln!(out, "    case {value}:").unwrap();
                }
                out.push_str("      return true;\n    default:\n      return false;\n  }\n");
            }
            out.push_str("}\n");
            self.functions[id] = out;
        }
        f
    }

    fn read_union(&mut self, out: &mut String, scope: usize, ty: &Value) {
        self.imports
            .extend(["readBytes", "readLength", "readUint", "readVariant"]);
        writeln!(out, "  const discriminant = readUint(r, {});", ty[1]).unwrap();
        out.push_str("  const size = readLength(r);\n  switch (discriminant) {\n");
        for variant in ty[2].as_array().unwrap() {
            let kind = string_literal(variant[1].as_str().unwrap());
            let reader = self.variant_reader(scope, &variant[2], &kind);
            writeln!(out, "    case {}:", variant[0]).unwrap();
            call(
                out,
                6,
                "return ",
                "readVariant",
                &["r", "size", "table", &kind, &reader],
                ";",
            );
        }
        out.push_str("    default:\n      return { kind: \"Unknown\", discriminant, data: readBytes(r, size) };\n  }\n");
    }

    fn variant_reader(&mut self, scope: usize, ty: &Value, kind: &str) -> String {
        if !self.has_validation(ty) {
            return self.read_op(scope, ty, false);
        }
        let (id, build) = self.reserve(Op::Read, scope, format!("variant:{kind}:{ty}"));
        let f = format!("__read{id}");
        if build {
            let read = self.reader(scope, ty, false);
            let mut out = format!("function {f}(r: Cursor, table: ReadTable): unknown {{\n");
            call(
                &mut out,
                2,
                "const value = ",
                &read,
                self.read_args(ty),
                ";",
            );
            self.validate(&mut out, 2, ty, "value", kind);
            out.push_str("  return value;\n}\n");
            self.functions[id] = out;
        }
        f
    }

    pub(super) fn has_validation(&self, mut ty: &Value) -> bool {
        while optional(ty) {
            ty = &ty[1];
        }
        tag(ty) == "constraint"
    }

    pub(super) fn validate(
        &mut self,
        out: &mut String,
        indent: usize,
        mut ty: &Value,
        value: &str,
        field: &str,
    ) {
        if !self.has_validation(ty) {
            return;
        }
        let mut indent = indent;
        if optional(ty) {
            writeln!(out, "{:indent$}if ({value} != null)", "").unwrap();
            indent += 2;
            while optional(ty) {
                ty = &ty[1];
            }
        }
        self.use_symbol("validateRange");
        call(
            out,
            indent,
            "",
            "validateRange",
            &[value, &ty[2].to_string(), &ty[3].to_string(), field],
            ";",
        );
    }

    pub(super) fn write_value(
        &mut self,
        out: &mut String,
        indent: usize,
        scope: usize,
        ty: &Value,
        value: &str,
        historical: bool,
    ) {
        if let Some(primitive) = ty.as_str() {
            let write = self.use_symbol(scalar_writer(primitive).unwrap());
            let args = if primitive == "intern" {
                &["w", value, "table"][..]
            } else {
                &["w", value][..]
            };
            call(out, indent, "", write, args, ";");
        } else if tag(ty) == "record" {
            self.use_symbol("object");
            let write = self.write_layout(self.record(scope, ty, historical), historical);
            call(
                out,
                indent,
                "",
                &write,
                &["w", &format!("object({value})"), "table"],
                ";",
            );
        } else {
            let write = self.write_op(scope, ty, historical);
            call(out, indent, "", &write, &["w", value, "table"], ";");
        }
    }

    fn write_op(&mut self, scope: usize, ty: &Value, historical: bool) -> String {
        let (id, build) = self.reserve(Op::Write, scope, format!("value:{historical}:{ty}"));
        let f = format!("__write{id}");
        if !build {
            return f;
        }
        let mut out =
            format!("function {f}(w: WriteCursor, value: unknown, table: WriteTable): void {{\n");
        if ty.is_string() {
            self.write_value(&mut out, 2, scope, ty, "value", historical);
        } else {
            match tag(ty) {
                "bytes" | "bits" => {
                    self.use_symbol("writeFixedBytes");
                    let width = ty[1].to_string();
                    let bitset = ty[2].to_string();
                    let mut args = vec!["w", "value", &width];
                    if tag(ty) == "bits" {
                        args.push(&bitset);
                    }
                    call(&mut out, 2, "", "writeFixedBytes", &args, ";");
                }
                "enum" => {
                    self.use_symbol("writeUint");
                    let valid = self.enum_check(scope, &ty[2]);
                    writeln!(out, "  if (typeof value !== \"number\" || !{valid}(value))\n    invalid(\"Invalid enum value\");").unwrap();
                    writeln!(
                        out,
                        "  writeUint(w, {}, value <= {} ? value : 0);",
                        ty[1], ty[3]
                    )
                    .unwrap();
                }
                "union" => self.write_union(&mut out, scope, ty),
                "record" => self.write_value(&mut out, 2, scope, ty, "value", historical),
                "import" => {
                    let (scope, version) = self.imported(scope, ty);
                    let write = self.write_root(scope, version, true);
                    writeln!(out, "  {write}(w, value);").unwrap();
                }
                "optional" => {
                    self.use_symbol("writeByte");
                    out.push_str(
                        "  writeByte(w, value == null ? 0 : 1);\n  if (value != null) {\n",
                    );
                    self.write_value(&mut out, 4, scope, &ty[1], "value", historical);
                    out.push_str("  }\n");
                }
                "constraint" | "box" => {
                    self.write_value(&mut out, 2, scope, &ty[1], "value", historical)
                }
                "vfloat" => {
                    self.use_symbol("writeVFloat");
                    call(
                        &mut out,
                        2,
                        "",
                        "writeVFloat",
                        &[
                            "w",
                            "value",
                            &ty[3].to_string(),
                            &ty[1].to_string(),
                            &ty[2].to_string(),
                        ],
                        ";",
                    );
                }
                "tuple" => {
                    self.imports.extend(["invalid", "sequence"]);
                    let items = ty[1].as_array().unwrap();
                    out.push_str("  const items = sequence(value);\n");
                    writeln!(
                        out,
                        "  if (items.length !== {}) invalid(\"Invalid Pojoc tuple length\");",
                        items.len()
                    )
                    .unwrap();
                    for (i, item) in items.iter().enumerate() {
                        self.write_value(&mut out, 2, scope, item, &format!("items[{i}]"), false);
                    }
                }
                "array" | "delta" => self.write_array(&mut out, scope, ty),
                "map" => {
                    self.use_symbol("writePairs");
                    let key = self.write_op(scope, &ty[1], false);
                    let item = self.write_op(scope, &ty[2], false);
                    call(
                        &mut out,
                        2,
                        "",
                        "writePairs",
                        &["w", "value", "table", &ty[3].to_string(), &key, &item],
                        ";",
                    );
                }
                tag => unreachable!("unhandled TypeScript writer for {tag}"),
            }
        }
        out.push_str("}\n");
        self.functions[id] = out;
        f
    }

    fn write_union(&mut self, out: &mut String, scope: usize, ty: &Value) {
        self.imports.extend(["invalid", "object", "writeVariant"]);
        let width = ty[1].to_string();
        out.push_str(
            "  const item = object(value);\n  switch (item.kind) {\n    case \"Unknown\":\n",
        );
        call(
            out,
            6,
            "return ",
            "writeVariant",
            &["w", "item.data", "table", &width, "item.discriminant"],
            ";",
        );
        for var in ty[3].as_array().unwrap() {
            let kind = string_literal(var[1].as_str().unwrap());
            let write = self.write_op(scope, &var[2], false);
            writeln!(out, "    case {kind}:").unwrap();
            self.validate(out, 6, &var[2], "item.value", &kind);
            call(
                out,
                6,
                "return ",
                "writeVariant",
                &[
                    "w",
                    "item.value",
                    "table",
                    &width,
                    &var[0].to_string(),
                    &write,
                ],
                ";",
            );
        }
        out.push_str("    default:\n      return invalid(\"Invalid Pojoc union kind\");\n  }\n");
    }

    fn write_array(&mut self, out: &mut String, scope: usize, ty: &Value) {
        self.use_symbol("arrayItems");
        writeln!(out, "  const items = arrayItems(w, value, {});", ty[2]).unwrap();
        let item = &ty[1];
        if tag(ty) == "delta" {
            self.use_symbol("encodeDelta");
            writeln!(out, "  encodeDelta(w, {item}, items);").unwrap();
        } else if item == "varint32" {
            self.use_symbol("writeVarint32Array");
            out.push_str("  writeVarint32Array(w, items);\n");
        } else {
            if let Some(ctor) = typed_array(item) {
                self.use_symbol("writePacked");
                writeln!(out, "  if (writePacked(w, value, {ctor})) return;").unwrap();
            }
            out.push_str("  for (let i = 0; i < items.length; i++) {\n");
            self.write_value(out, 4, scope, item, "items[i]", false);
            out.push_str("  }\n");
        }
    }

    pub(super) fn skip_statement(&mut self, scope: usize, ty: &Value, historical: bool) -> String {
        if let Some((_, width)) = fixed_scalar(ty) {
            self.use_symbol("takeBytes");
            return format!("takeBytes(r, {width});");
        }
        match ty.as_str() {
            Some("string") => {
                self.imports.extend(["takeBytes", "readLength"]);
                return "takeBytes(r, readLength(r));".into();
            }
            Some("intern") => {
                self.use_symbol("readLength");
                return "readLength(r);".into();
            }
            Some("varint32" | "varint64") => {
                let name = self.use_symbol(scalar_reader(ty).unwrap());
                return format!("{name}(r);");
            }
            _ => {}
        }
        let width = match tag(ty) {
            "bytes" | "bits" | "enum" => Some(&ty[1]),
            "vfloat" => Some(&ty[3]),
            _ => None,
        };
        if let Some(width) = width {
            self.use_symbol("takeBytes");
            return format!("takeBytes(r, {width});");
        }
        match tag(ty) {
            "box" | "constraint" => self.skip_statement(scope, &ty[1], historical),
            "record" => {
                let skip = self.skip_layout(self.record(scope, ty, historical));
                format!("{skip}(r);")
            }
            "import" => {
                let (scope, version) = self.imported(scope, ty);
                let skip = self.skip_root(scope, version);
                format!("{skip}(r);")
            }
            _ => {
                let skip = self.skip_op(scope, ty, historical);
                format!("{skip}(r);")
            }
        }
    }

    fn skip_op(&mut self, scope: usize, ty: &Value, historical: bool) -> String {
        let (id, build) = self.reserve(Op::Skip, scope, format!("value:{historical}:{ty}"));
        let f = format!("__skip{id}");
        if !build {
            return f;
        }
        let mut out = format!("function {f}(r: Cursor): void {{\n");
        match tag(ty) {
            "optional" => {
                self.use_symbol("readByte");
                let skip = self.skip_statement(scope, &ty[1], historical);
                writeln!(out, "  if (readByte(r)) {skip}").unwrap();
            }
            "union" => {
                self.imports.extend(["takeBytes", "readLength"]);
                writeln!(
                    out,
                    "  takeBytes(r, {});\n  takeBytes(r, readLength(r));",
                    ty[1]
                )
                .unwrap();
            }
            "tuple" => {
                for item in ty[1].as_array().unwrap() {
                    let skip = self.skip_statement(scope, item, false);
                    writeln!(out, "  {skip}").unwrap();
                }
            }
            "delta" => {
                self.use_symbol("skipDelta");
                let count = self.array_count(&ty[2]);
                writeln!(out, "  skipDelta(r, {count});").unwrap();
            }
            "array" | "map" => self.skip_collection(&mut out, scope, ty),
            tag => unreachable!("unhandled TypeScript skip for {tag}"),
        }
        out.push_str("}\n");
        self.functions[id] = out;
        f
    }

    fn skip_collection(&mut self, out: &mut String, scope: usize, ty: &Value) {
        let map = tag(ty) == "map";
        let count = self.array_count(&ty[if map { 3 } else { 2 }]);
        let item = &ty[1];
        if !map && let Some(ctor) = typed_array(item) {
            self.use_symbol("takeBytes");
            writeln!(out, "  takeBytes(r, {count} * {ctor}.BYTES_PER_ELEMENT);").unwrap();
            return;
        }
        self.use_symbol("limitCollection");
        writeln!(out, "  const count = {count};\n  limitCollection(count);").unwrap();
        if !map
            && tag(item) == "record"
            && let Some(width) = self.fixed_layout(self.record(scope, item, false))
        {
            self.use_symbol("takeBytes");
            writeln!(out, "  takeBytes(r, count * {width});").unwrap();
            return;
        }
        out.push_str("  for (let i = 0; i < count; i++) {\n");
        let skip = self.skip_statement(scope, item, false);
        writeln!(out, "    {skip}").unwrap();
        if map {
            let skip = self.skip_statement(scope, &ty[2], false);
            writeln!(out, "    {skip}").unwrap();
        }
        out.push_str("  }\n");
    }
}

use std::collections::BTreeSet;
