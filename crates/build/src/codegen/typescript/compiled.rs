use super::exports;
use super::literals::{PRINT_WIDTH, string_literal};
use super::writers::{CompiledWriter, wire_version};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

mod access;
mod evolution;
mod records;
mod row_access;
mod seek;
mod values;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Op {
    Read,
    Write,
    Skip,
    Default,
    Adapt,
    Project,
}

struct Layout<'a> {
    scope: usize,
    fields: &'a [Value],
    checks: bool,
    root: bool,
}

struct Program<'a> {
    plans: &'a [Value],
    layouts: Vec<Layout<'a>>,
    roots: BTreeMap<(usize, u64), usize>,
    records: BTreeMap<(usize, &'a str), usize>,
    ops: BTreeMap<(Op, usize, String), usize>,
    functions: Vec<String>,
    imports: BTreeSet<&'static str>,
}

impl<'a> Program<'a> {
    fn new(plans: &'a [Value]) -> Self {
        let mut program = Self {
            plans,
            layouts: Vec::new(),
            roots: BTreeMap::new(),
            records: BTreeMap::new(),
            ops: BTreeMap::new(),
            functions: Vec::new(),
            imports: BTreeSet::new(),
        };
        for (scope, plan) in plans.iter().enumerate() {
            for (number, root) in plan["roots"].as_object().unwrap() {
                let version = number.parse().unwrap();
                let id = program.layouts.len();
                program.roots.insert((scope, version), id);
                program.layouts.push(Layout {
                    scope,
                    fields: fields(root),
                    checks: *number == plan["latest"],
                    root: true,
                });
            }
            for (key, fields) in plan["records"].as_object().unwrap() {
                let id = program.layouts.len();
                program.records.insert((scope, key), id);
                program.layouts.push(Layout {
                    scope,
                    fields: fields.as_array().unwrap(),
                    // Named layouts are checked when read in their current representation.
                    checks: true,
                    root: false,
                });
            }
        }
        program
    }

    fn use_symbol(&mut self, name: &'static str) -> &'static str {
        self.imports.insert(name);
        name
    }

    fn reserve(&mut self, op: Op, scope: usize, key: String) -> (usize, bool) {
        // Reserve the name before walking dependencies so recursive records
        // can refer to their own reader or writer.
        let next = self.functions.len();
        let id = self.ops.entry((op, scope, key)).or_insert(next);
        if *id == next {
            self.functions.push(String::new());
            (next, true)
        } else {
            (*id, false)
        }
    }

    fn record(&self, scope: usize, ty: &Value, historical: bool) -> usize {
        let key = ty[if historical { 1 } else { 2 }].as_str().unwrap();
        self.records[&(scope, key)]
    }

    fn imported(&self, scope: usize, ty: &Value) -> (usize, u64) {
        let child = self.plans[scope]["imports"][ty[1].as_str().unwrap()]
            .as_u64()
            .unwrap() as usize;
        (child, ty[2].as_str().unwrap().parse().unwrap())
    }

    fn latest(&self, scope: usize) -> u64 {
        self.plans[scope]["latest"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    fn root(&self, scope: usize, version: u64) -> &'a Value {
        &self.plans[scope]["roots"][version.to_string()]
    }
}

fn fields(root: &Value) -> &[Value] {
    root["fields"].as_array().unwrap()
}

fn tag(ty: &Value) -> &str {
    ty.get(0).and_then(Value::as_str).unwrap_or_default()
}

fn optional(ty: &Value) -> bool {
    tag(ty) == "optional"
}

fn name(field: &Value) -> &str {
    field["name"].as_str().unwrap()
}

fn literal(field: &Value) -> String {
    string_literal(name(field))
}

fn cast(field: &Value) -> bool {
    field["cast"] == true
}

fn count(size: &Value) -> String {
    if size.is_null() {
        "readLength(r)".to_owned()
    } else {
        size.to_string()
    }
}

fn call(out: &mut String, indent: usize, prefix: &str, f: &str, args: &[&str], suffix: &str) {
    let line = format!("{:indent$}{prefix}{f}({}){suffix}", "", args.join(", "));
    if line.chars().count() <= PRINT_WIDTH {
        writeln!(out, "{line}").unwrap();
    } else {
        let child = indent + 2;
        writeln!(out, "{:indent$}{prefix}{f}(", "").unwrap();
        for arg in args {
            writeln!(out, "{:child$}{arg},", "").unwrap();
        }
        writeln!(out, "{:indent$}){suffix}", "").unwrap();
    }
}

pub(super) fn emit(
    out: &mut String,
    plans: &[Value],
    root: usize,
    profile: Option<&str>,
    compact: bool,
) -> BTreeSet<&'static str> {
    let mut p = Program::new(plans);
    let current = p.latest(root);
    let root_id = p.roots[&(root, current)];
    let root_plan = p.root(root, current);
    let compiled = profile
        .filter(|_| root_plan["fixedSize"].is_null())
        .and_then(|_| CompiledWriter::new(root_plan, &plans[root]["records"], current));

    out.push_str("\n// Codec entry points.\n");
    if let Some(writer) = &compiled {
        p.imports.extend(writer.imports());
        writer.emit(out, current, root_plan["intern"] == true);
    }
    p.emit_exports(out, root, current, compiled.is_some());
    p.emit_access(out, root_id, root_plan["intern"] == true, compact);
    out.push_str("\n// Generated wire layouts and version conversions.\n");
    for function in p.functions {
        out.push('\n');
        out.push_str(&function);
    }
    p.imports
}

impl Program<'_> {
    fn read_root(&mut self, scope: usize, version: u64) -> String {
        let layout = self.roots[&(scope, version)];
        let (id, build) = self.reserve(Op::Read, scope, format!("root:{version}"));
        let f = format!("__read{id}");
        if build {
            let mut out = format!("function {f}(r: Cursor): Obj {{\n");
            let read = self.read_layout(layout, false);
            let intern = self.root(scope, version)["intern"] == true;
            let table = if intern {
                self.use_symbol("readTable");
                "readTable(r)"
            } else {
                self.use_symbol("emptyTable")
            };
            let latest = self.latest(scope);
            if version == latest {
                call(&mut out, 2, "return ", &read, &["r", table], ";");
            } else {
                call(&mut out, 2, "const value = ", &read, &["r", table], ";");
                let latest = self.roots[&(scope, latest)];
                let project = self.project_layout(layout, latest, false);
                call(&mut out, 2, "return ", &project, &["value"], ";");
            }
            out.push_str("}\n");
            self.functions[id] = out;
        }
        f
    }

    fn write_root(&mut self, scope: usize, version: u64, payload: bool) -> String {
        let layout = self.roots[&(scope, version)];
        let (id, build) = self.reserve(Op::Write, scope, format!("root:{version}:{payload}"));
        let f = format!("__write{id}");
        if build {
            let intern = self.root(scope, version)["intern"].to_string();
            let writer = self.write_layout(layout, false);
            let latest = self.latest(scope);
            let mut out = if payload {
                format!("function {f}(w: WriteCursor, value: unknown): void {{\n")
            } else {
                format!("function {f}(value: unknown): Uint8Array<ArrayBuffer> {{\n")
            };
            self.use_symbol("object");
            let value = if version == latest {
                "object(value)"
            } else {
                let current = self.roots[&(scope, latest)];
                let project = self.project_layout(current, layout, true);
                call(
                    &mut out,
                    2,
                    "const wire = ",
                    &project,
                    &["object(value)"],
                    ";",
                );
                "wire"
            };
            if payload {
                let write = self.use_symbol("writePayloadWithTable");
                call(&mut out, 2, "", write, &["w", value, &intern, &writer], ";");
            } else {
                self.emit_frame_write(&mut out, scope, version, value, &writer);
            }
            out.push_str("}\n");
            self.functions[id] = out;
        }
        f
    }

    fn emit_frame_write(
        &mut self,
        out: &mut String,
        scope: usize,
        version: u64,
        value: &str,
        writer: &str,
    ) {
        let root = self.root(scope, version);
        if let Some(payload_size) = root["fixedSize"].as_u64() {
            let header = super::super::envelope::version_bytes(version);
            let size = payload_size + header.len() as u64;
            let encode = self.use_symbol("encodeFixedPayload");
            call(
                out,
                2,
                "return ",
                encode,
                &[
                    value,
                    &format!("__wireHeader{version}"),
                    &size.to_string(),
                    writer,
                ],
                ";",
            );
        } else {
            let intern = root["intern"].to_string();
            let encode = self.use_symbol("encodeCompiledPayload");
            call(
                out,
                2,
                "return ",
                encode,
                &[value, &wire_version(version), &intern, writer],
                ";",
            );
        }
    }

    fn skip_root(&mut self, scope: usize, version: u64) -> String {
        let (id, build) = self.reserve(Op::Skip, scope, format!("root:{version}"));
        let f = format!("__skip{id}");
        if build {
            let mut out = format!("function {f}(r: Cursor): void {{\n");
            if self.root(scope, version)["intern"] == true {
                let skip = self.use_symbol("skipTable");
                writeln!(out, "  {skip}(r);").unwrap();
            }
            let skip = self.skip_layout(self.roots[&(scope, version)]);
            writeln!(out, "  {skip}(r);\n}}").unwrap();
            self.functions[id] = out;
        }
        f
    }

    fn emit_exports(&mut self, out: &mut String, scope: usize, current: u64, compiled: bool) {
        self.imports.extend([
            "finishDecode",
            "openSchemaEnvelope",
            "type Cursor",
            "type Envelope",
            "type Input",
            "type Obj",
            "type ReadTable",
            "type Version",
            "type WriteCursor",
            "type WriteTable",
            "unsupported",
            "versionNumber",
        ]);
        let versions: Vec<_> = self
            .roots
            .keys()
            .filter_map(|(s, v)| (*s == scope && *v != current).then_some(*v))
            .collect();
        writeln!(out, "\nexport const version = {current}n;").unwrap();
        for v in std::iter::once(current).chain(versions.iter().copied()) {
            if !self.root(scope, v)["fixedSize"].is_null() {
                let bytes = super::super::envelope::version_bytes(v);
                let bytes: Vec<_> = bytes.iter().map(u8::to_string).collect();
                let args: Vec<_> = bytes.iter().map(String::as_str).collect();
                call(
                    out,
                    0,
                    &format!("const __wireHeader{v} = "),
                    "Uint8Array.of",
                    &args,
                    ";",
                );
            } else if v >= 128 {
                let bytes = super::super::envelope::version_bytes(v);
                writeln!(out, "const {} = {bytes:?} as const;", wire_version(v)).unwrap();
            }
        }
        let current_size = self.root(scope, current)["fixedSize"]
            .as_u64()
            .map_or_else(|| "undefined".to_owned(), |size| size.to_string());
        out.push_str(
            "\nfunction __fixedSize(value: bigint): number | undefined {\n  switch (value) {\n",
        );
        for v in std::iter::once(current).chain(versions.iter().copied()) {
            let size = self.root(scope, v)["fixedSize"]
                .as_u64()
                .map_or_else(|| "undefined".to_owned(), |size| size.to_string());
            writeln!(out, "    case {v}n:\n      return {size};").unwrap();
        }
        out.push_str("    default:\n      return unsupported(value);\n  }\n}\n");
        let short = if current < 128 { current as i16 } else { -1 };
        out.push_str("\nfunction __openFrame(input: Input, offset = 0): Envelope {\n");
        call(
            out,
            2,
            "return ",
            "openSchemaEnvelope",
            &[
                "input",
                "offset",
                "version",
                &short.to_string(),
                &current_size,
                "__fixedSize",
            ],
            ";",
        );
        out.push_str("}\n");
        let read = self.read_root(scope, current);
        out.push_str("\nfunction __decodeFrame(r: Envelope): __Current {\n");
        writeln!(
            out,
            "  if (r.version === version) return finishDecode(r, {read}(r));"
        )
        .unwrap();
        if versions.is_empty() {
            out.push_str("  return unsupported(r.version);\n");
        } else {
            out.push_str("  switch (r.version) {\n");
            for &v in &versions {
                let read = self.read_root(scope, v);
                writeln!(
                    out,
                    "    case {v}n:\n      return finishDecode(r, {read}(r));"
                )
                .unwrap();
            }
            out.push_str("    default:\n      return unsupported(r.version);\n  }\n");
        }
        out.push_str("}\n");

        if !compiled {
            let write = self.write_layout(self.roots[&(scope, current)], false);
            out.push_str("\nexport function encode(value: __Current): Uint8Array<ArrayBuffer> {\n");
            self.emit_frame_write(out, scope, current, "value", &write);
            out.push_str("}\n");
        }
        out.push_str("\nexport function encodeForVersion(\n  value: __Current,\n  target: Version,\n): Uint8Array<ArrayBuffer> {\n  const selected = versionNumber(target);\n  if (selected === version) return encode(value);\n");
        if versions.is_empty() {
            out.push_str("  return unsupported(selected);\n");
        } else {
            out.push_str("  switch (selected) {\n");
            for v in versions {
                let writer = self.write_root(scope, v, false);
                writeln!(out, "    case {v}n:\n      return {writer}(value);").unwrap();
            }
            out.push_str("    default:\n      return unsupported(selected);\n  }\n");
        }
        out.push_str("}\n");
        let create = self.defaults_layout(self.roots[&(scope, current)]);
        writeln!(out, "\nexport function create(value: Partial<__Current> = {{}}): __Current {{\n  return {create}(object(value)) as unknown as __Current;\n}}").unwrap();
        out.push_str(exports::DECODE);
    }
}
