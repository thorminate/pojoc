use super::super::literals::data_key;
use super::super::readers::{emit_fixed_read, fixed_scalar};
use super::{Op, Program, call, cast, literal, name, optional};
use serde_json::Value;
use std::fmt::Write;

pub(super) struct Field<'a> {
    pub(super) spec: &'a Value,
    pub(super) ty: &'a Value,
    pub(super) bit: Option<usize>,
}

pub(super) fn members(fields: &[Value]) -> Vec<Field<'_>> {
    let mut bits = 0;
    fields
        .iter()
        .map(|spec| {
            let ty = &spec["ty"];
            if optional(ty) {
                let bit = bits;
                bits += 1;
                Field {
                    spec,
                    ty: &ty[1],
                    bit: Some(bit),
                }
            } else {
                Field {
                    spec,
                    ty,
                    bit: None,
                }
            }
        })
        .collect()
}

pub(super) fn bitmap(fields: &[Field<'_>]) -> usize {
    fields
        .iter()
        .filter(|f| f.bit.is_some())
        .count()
        .div_ceil(8)
}

fn layout_key(fields: &[Value], retain_names: bool) -> String {
    let mut key = String::new();
    for field in fields {
        if retain_names {
            write!(key, "{}:", field["name"]).unwrap();
        }
        write!(key, "{}:{};", field["ty"], cast(field)).unwrap();
    }
    key
}

impl Program<'_> {
    pub(super) fn fixed_layout(&self, layout: usize) -> Option<usize> {
        self.layouts[layout]
            .fields
            .iter()
            .map(|f| fixed_scalar(&f["ty"]).map(|(_, width)| width))
            .sum::<Option<usize>>()
            .filter(|width| *width > 0)
    }

    pub(super) fn read_fixed_layout(&mut self, layout: usize) -> String {
        let scope = self.layouts[layout].scope;
        let key = layout_key(self.layouts[layout].fields, true);
        let (id, build) = self.reserve(Op::Read, scope, format!("fixed:{key}"));
        let f = format!("__read{id}");
        if build {
            let fields = self.layouts[layout].fields;
            let mut out = format!("function {f}(r: Cursor, p0: number): Obj {{\n");
            let mut offset = 0;
            for (i, field) in fields.iter().enumerate() {
                let ty = &field["ty"];
                self.emit_fixed(&mut out, ty, i, 0, offset);
                offset += fixed_scalar(ty).unwrap().1;
            }
            emit_record(&mut out, fields);
            out.push_str("}\n");
            self.functions[id] = out;
        }
        f
    }

    pub(super) fn emit_fixed(
        &mut self,
        out: &mut String,
        ty: &Value,
        i: usize,
        start: usize,
        offset: usize,
    ) {
        if matches!(ty.as_str(), Some("u64" | "i64" | "f32" | "f64")) {
            self.use_symbol("cursorView");
        }
        emit_fixed_read(out, ty, i, start, offset);
    }

    pub(super) fn read_layout(&mut self, layout: usize, historical: bool) -> String {
        let scope = self.layouts[layout].scope;
        let checks = self.layouts[layout].checks && !historical;
        let key = layout_key(self.layouts[layout].fields, true);
        let (id, build) = self.reserve(Op::Read, scope, format!("layout:{checks}:{key}"));
        let f = format!("__read{id}");
        if !build {
            return f;
        }
        let fields = self.layouts[layout].fields;
        let mut out = format!("function {f}(r: Cursor, table: ReadTable): Obj {{\n");
        if let Some(width) = self.fixed_layout(layout) {
            self.use_symbol("takeBytes");
            let read = self.read_fixed_layout(layout);
            writeln!(out, "  return {read}(r, takeBytes(r, {width}));\n}}").unwrap();
            self.functions[id] = out;
            return f;
        }

        let layout_fields = members(fields);
        let header = bitmap(&layout_fields);
        if header > 0 {
            self.use_symbol("takeBytes");
            writeln!(out, "  const header = takeBytes(r, {header});").unwrap();
        }
        let mut index = 0;
        while index < fields.len() {
            let width = layout_fields[index..]
                .iter()
                .map_while(|field| {
                    if field.bit.is_some() {
                        return None;
                    }
                    fixed_scalar(field.ty).map(|(_, width)| width)
                })
                .sum::<usize>();
            if width != 0 {
                self.use_symbol("takeBytes");
                let start = index;
                writeln!(out, "  const p{start} = takeBytes(r, {width});").unwrap();
                let mut offset = 0;
                while offset < width {
                    let ty = layout_fields[index].ty;
                    self.emit_fixed(&mut out, ty, index, start, offset);
                    offset += fixed_scalar(ty).unwrap().1;
                    index += 1;
                }
            } else {
                let field = &layout_fields[index];
                let historical = cast(field.spec);
                let read = self.reader(scope, field.ty, historical);
                let args = self.read_args(field.ty);
                let validation = checks && self.has_validation(field.ty);
                if let Some(bit) = field.bit {
                    let byte = bit / 8;
                    let pos = if byte == 0 {
                        "header".to_owned()
                    } else {
                        format!("header + {byte}")
                    };
                    let mask = 1 << (bit % 8);
                    let present = format!("(r.data[{pos}] & {mask}) !== 0");
                    let expr = format!("{read}({})", args.join(", "));
                    let conditional = format!("{present} ? {expr} : null;");
                    let line = format!("  const v{index} = {conditional}");
                    if line.chars().count() <= 80 {
                        writeln!(out, "{line}").unwrap();
                    } else if conditional.chars().count() + 4 <= 80 {
                        writeln!(out, "  const v{index} =\n    {conditional}").unwrap();
                    } else {
                        writeln!(
                            out,
                            "  const v{index} =\n    {present}\n      ? {expr}\n      : null;"
                        )
                        .unwrap();
                    }
                    if validation {
                        self.validate(
                            &mut out,
                            2,
                            &fields[index]["ty"],
                            &format!("v{index}"),
                            &literal(field.spec),
                        );
                    }
                } else {
                    call(&mut out, 2, &format!("const v{index} = "), &read, args, ";");
                    if validation {
                        self.validate(
                            &mut out,
                            2,
                            field.ty,
                            &format!("v{index}"),
                            &literal(field.spec),
                        );
                    }
                }
                index += 1;
            }
        }
        emit_record(&mut out, fields);
        out.push_str("}\n");
        self.functions[id] = out;
        f
    }

    pub(super) fn write_layout(&mut self, layout: usize, historical: bool) -> String {
        let scope = self.layouts[layout].scope;
        let checks = self.layouts[layout].checks && !historical;
        let key = layout_key(self.layouts[layout].fields, true);
        let (id, build) = self.reserve(Op::Write, scope, format!("layout:{checks}:{key}"));
        let f = format!("__write{id}");
        if !build {
            return f;
        }
        let fields = members(self.layouts[layout].fields);
        let mut out =
            format!("function {f}(w: WriteCursor, value: Obj, table: WriteTable): void {{\n");
        if fields.is_empty() {
            out.truncate(out.len() - 1);
            out.push_str("}\n");
            self.functions[id] = out;
            return f;
        }
        let header = bitmap(&fields);
        if header > 0 {
            self.use_symbol("takeWrite");
            writeln!(out, "  const header = takeWrite(w, {header});").unwrap();
        }
        for (i, field) in fields.iter().enumerate() {
            let v = format!("v{i}");
            let key = literal(field.spec);
            writeln!(out, "  const {v} = value[{key}];").unwrap();
            let indent = if let Some(bit) = field.bit {
                writeln!(out, "  if ({v} != null) {{").unwrap();
                writeln!(
                    out,
                    "    w.data[header + {}] |= {};",
                    bit / 8,
                    1 << (bit % 8)
                )
                .unwrap();
                4
            } else {
                2
            };
            if checks {
                self.validate(&mut out, indent, field.ty, &v, &key);
            }
            self.write_value(&mut out, indent, scope, field.ty, &v, cast(field.spec));
            if field.bit.is_some() {
                out.push_str("  }\n");
            }
        }
        out.push_str("}\n");
        self.functions[id] = out;
        f
    }

    pub(super) fn skip_layout(&mut self, layout: usize) -> String {
        let scope = self.layouts[layout].scope;
        let key = layout_key(self.layouts[layout].fields, false);
        let (id, build) = self.reserve(Op::Skip, scope, format!("layout:{key}"));
        let f = format!("__skip{id}");
        if !build {
            return f;
        }
        let mut out = format!("function {f}(r: Cursor): void {{\n");
        if let Some(width) = self.fixed_layout(layout) {
            self.use_symbol("takeBytes");
            writeln!(out, "  takeBytes(r, {width});").unwrap();
        } else {
            let fields = members(self.layouts[layout].fields);
            let header = bitmap(&fields);
            if header > 0 {
                self.use_symbol("takeBytes");
                self.use_symbol("hasField");
                writeln!(out, "  const header = takeBytes(r, {header});").unwrap();
            }
            for field in &fields {
                self.skip_member(&mut out, 2, scope, field);
            }
        }
        out.push_str("}\n");
        self.functions[id] = out;
        f
    }

    pub(super) fn skip_member(
        &mut self,
        out: &mut String,
        indent: usize,
        scope: usize,
        field: &Field<'_>,
    ) {
        let skip = self.skip_statement(scope, field.ty, cast(field.spec));
        if let Some(bit) = field.bit {
            self.use_symbol("hasField");
            let line = format!("{:indent$}if (hasField(r, header, {bit})) {skip}", "");
            if line.len() <= 80 {
                writeln!(out, "{line}").unwrap();
            } else {
                writeln!(
                    out,
                    "{:indent$}if (hasField(r, header, {bit}))\n  {:indent$}{skip}",
                    "", ""
                )
                .unwrap();
            }
        } else {
            writeln!(out, "{:indent$}{skip}", "").unwrap();
        }
    }
}

fn emit_record(out: &mut String, fields: &[Value]) {
    if fields.is_empty() {
        out.push_str("  return {};\n");
        return;
    }
    out.push_str("  return {\n");
    for (i, field) in fields.iter().enumerate() {
        writeln!(out, "    {}: v{i},", data_key(name(field))).unwrap();
    }
    out.push_str("  };\n");
}
