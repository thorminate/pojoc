use super::super::literals::string_literal;
use super::super::readers::fixed_scalar;
use super::access::Access;
use super::records::{bitmap, members};
use super::{Op, Program, call, cast, literal, name, tag};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

const SEEK_PATH_ERROR: &str = "  return invalid(`Unknown seekable Pojoc path ${path}`);\n}\n";

fn emit_seek_cases(out: &mut String, cases: &[(String, String)]) {
    out.push_str("      switch (path) {\n");
    for (path, reader) in cases {
        writeln!(out, "        case {}:", string_literal(path)).unwrap();
        call(out, 10, "return ", reader, &["r"], ";");
    }
    out.push_str("      }\n      break;\n");
}

#[derive(Clone, Copy)]
struct Position {
    layout: usize,
    index: usize,
}

struct Route<'a> {
    prefix: Vec<Position>,
    leaf: Leaf<'a>,
    target: &'a Value,
    validate_target: bool,
}

enum Leaf<'a> {
    Default(&'a Value),
    Read {
        position: Position,
        validate_source: bool,
        convert_from: Option<&'a Value>,
    },
}

fn seek_needs_table(ty: &Value) -> bool {
    if let Some(scalar) = ty.as_str() {
        return scalar == "intern";
    }
    match tag(ty) {
        "optional" | "constraint" | "box" | "array" => seek_needs_table(&ty[1]),
        "bytes" | "bits" | "enum" | "vfloat" | "delta" | "import" => false,
        // Leave composite readers that may reach a nested interned field alone.
        _ => true,
    }
}

impl<'a> Program<'a> {
    pub(super) fn emit_seek(
        &mut self,
        out: &mut String,
        current: usize,
        intern: bool,
        compact: bool,
    ) {
        out.push_str(
            r#"
export function seek<K extends keyof __Current & string>(
  input: Input,
  path: K,
): __Current[K];
export function seek(input: Input, path: string): unknown;
export function seek(input: Input, path: string): unknown {
"#,
        );
        let paths = self.seek_paths(current);
        if paths.is_empty() {
            out.push_str(SEEK_PATH_ERROR);
            return;
        }

        let scope = self.layouts[current].scope;
        let current_cases: Vec<_> = paths
            .iter()
            .map(|(path, fields)| {
                let route = self.seek_route(current, current, fields).unwrap();
                (
                    path.clone(),
                    self.emit_seek_reader(scope, &route, true, intern),
                )
            })
            .collect();
        let versions: Vec<_> = self
            .roots
            .iter()
            .filter_map(|(&(s, version), &layout)| {
                (s == scope && layout != current).then_some((version, layout))
            })
            .collect();
        let mut current_aliases = Vec::new();
        let mut historical = BTreeMap::<_, Vec<u64>>::new();
        let mut needs_fallback = false;
        for (version, old) in versions {
            let old_intern = self.root(scope, version)["intern"] == true;
            if old_intern == intern && self.seek_equivalent(old, current) {
                current_aliases.push(version);
            } else if compact {
                needs_fallback = true;
            } else {
                let cases: Vec<_> = paths
                    .iter()
                    .filter_map(|(path, fields)| {
                        let route = self.seek_route(old, current, fields)?;
                        let reader = self.emit_seek_reader(scope, &route, false, old_intern);
                        Some((path.clone(), reader))
                    })
                    .collect();
                needs_fallback |= cases.len() < paths.len();
                if !cases.is_empty() {
                    historical.entry(cases).or_default().push(version);
                }
            }
        }

        out.push_str(
            "  const r = __openFrame(input);\n  switch (r.version) {\n    case version:\n",
        );
        for version in current_aliases {
            writeln!(out, "    case {version}n:").unwrap();
        }
        emit_seek_cases(out, &current_cases);
        for (cases, versions) in historical {
            for version in versions {
                writeln!(out, "    case {version}n:").unwrap();
            }
            emit_seek_cases(out, &cases);
        }
        out.push_str("  }\n");
        if needs_fallback {
            out.push_str("  switch (path) {\n");
            for (index, (path, _)) in paths.iter().enumerate() {
                let block = if index + 1 == paths.len() { " {" } else { "" };
                writeln!(out, "    case {}:{block}", string_literal(path)).unwrap();
            }
            out.push_str(
                "      let value: unknown = __decodeFrame(r);\n      for (const name of path.split(\".\")) value = object(value)[name];\n      return value;\n    }\n  }\n",
            );
        }
        out.push_str(SEEK_PATH_ERROR);
    }

    fn seek_equivalent(&self, old: usize, current: usize) -> bool {
        let old = self.layouts[old].fields;
        let current = self.layouts[current].fields;
        old.len() == current.len()
            && old.iter().zip(current).all(|(old, new)| {
                old["id"] == new["id"]
                    && old["name"] == new["name"]
                    && old["ty"] == new["ty"]
                    && !cast(old)
                    && !cast(new)
            })
    }

    fn seek_paths(&self, root: usize) -> Vec<(String, Vec<&'a Value>)> {
        let mut paths = Vec::new();
        self.collect_seek_paths(root, &mut BTreeSet::new(), &mut Vec::new(), &mut paths);
        paths
    }

    fn collect_seek_paths(
        &self,
        layout: usize,
        visiting: &mut BTreeSet<usize>,
        path: &mut Vec<&'a Value>,
        paths: &mut Vec<(String, Vec<&'a Value>)>,
    ) {
        if !visiting.insert(layout) {
            return;
        }
        for field in self.layouts[layout].fields {
            if field["seek"] != true {
                continue;
            }
            path.push(field);
            paths.push((
                path.iter()
                    .map(|field| name(field))
                    .collect::<Vec<_>>()
                    .join("."),
                path.clone(),
            ));
            let ty = &field["ty"];
            if tag(ty) == "record" && field["lazy"] != true {
                let child = self.record(self.layouts[layout].scope, ty, false);
                self.collect_seek_paths(child, visiting, path, paths);
            }
            path.pop();
        }
        visiting.remove(&layout);
    }

    fn seek_route(&self, old: usize, current: usize, path: &[&'a Value]) -> Option<Route<'a>> {
        let scope = self.layouts[old].scope;
        let mut wire = old;
        let mut projection = Some(old);
        let mut checks = false;
        let mut prefix = Vec::new();
        for (depth, &target) in path.iter().enumerate() {
            let last = depth + 1 == path.len();
            let target_ty = &target["ty"];
            let validate_target = depth == 0 && self.layouts[current].root;
            let logical = if let Some(layout) = projection {
                let source = self.layouts[layout]
                    .fields
                    .iter()
                    .find(|f| f["id"] == target["id"]);
                let supplied = match source {
                    None => Some(&target["default"]),
                    Some(field) if cast(field) && target["lazy"] == true => Some(&Value::Null),
                    _ => None,
                };
                if let Some(supplied) = supplied {
                    return last.then_some(Route {
                        prefix,
                        leaf: Leaf::Default(supplied),
                        target,
                        validate_target,
                    });
                }
                source
            } else {
                None
            };
            // Version conversion looks up properties by the logical field's
            // name. Its record may have been read with the newest named layout.
            let key = logical.map_or_else(|| name(target), name);
            let fields = self.layouts[wire].fields;
            let index = fields.iter().position(|field| name(field) == key)?;
            let source = &fields[index];
            let source_ty = &source["ty"];
            let convert_from = logical
                .filter(|field| cast(field) || field["ty"] != *target_ty)
                .map(|field| &field["ty"]);
            let position = Position {
                layout: wire,
                index,
            };
            if last {
                return Some(Route {
                    prefix,
                    leaf: Leaf::Read {
                        position,
                        validate_source: checks,
                        convert_from,
                    },
                    target,
                    validate_target,
                });
            }
            if tag(source_ty) != "record" || source["lazy"] == true {
                return None;
            }
            prefix.push(position);
            checks = !cast(source);
            wire = self.record(scope, source_ty, cast(source));
            projection = match convert_from {
                Some(ty) if tag(ty) == "record" => Some(self.record(scope, ty, true)),
                Some(_) => return None,
                None => None,
            };
        }
        None
    }

    fn emit_seek_reader(
        &mut self,
        scope: usize,
        route: &Route<'a>,
        current: bool,
        intern: bool,
    ) -> String {
        let mut body = String::new();
        let needs_table = match route.leaf {
            Leaf::Default(_) => false,
            Leaf::Read { position, .. } => {
                seek_needs_table(&self.layouts[position.layout].fields[position.index]["ty"])
            }
        };
        if intern {
            if needs_table {
                self.use_symbol("readTableOffsets");
                body.push_str("  const table = readTableOffsets(r);\n");
            } else {
                self.use_symbol("skipTable");
                body.push_str("  skipTable(r);\n");
            }
        }
        for &position in &route.prefix {
            let advance = self.emit_seek_position(position);
            writeln!(body, "  {advance}(r);").unwrap();
        }
        let target_ty = &route.target["ty"];
        let value = match route.leaf {
            Leaf::Default(supplied) => {
                let default = self.default_expr(scope, target_ty, supplied);
                writeln!(body, "  const value = {default};").unwrap();
                "value"
            }
            Leaf::Read {
                position,
                validate_source,
                convert_from,
            } => {
                let access = Access::new(self.layouts[position.layout].fields);
                let field = &access.fields[position.index];
                if current && let Some(offset) = access.offsets.get(position.index) {
                    self.use_symbol("takeBytes");
                    let header = bitmap(&access.fields);
                    writeln!(
                        body,
                        "  const p0 = takeBytes(r, {});",
                        header + access.width
                    )
                    .unwrap();
                    self.emit_fixed(&mut body, field.ty, 0, 0, header + offset);
                    "v0"
                } else {
                    let reader = if current && field.ty.as_str() == Some("string") {
                        self.use_symbol("readSeekString").to_owned()
                    } else {
                        self.reader(scope, field.ty, cast(field.spec))
                    };
                    let mut args = self.read_args(field.ty);
                    if args.len() == 2 && !(intern && needs_table) {
                        self.use_symbol("emptyTable");
                        args = &["r", "emptyTable"];
                    }
                    let advance = self.emit_seek_position(position);
                    let indent = if let Some(bit) = field.bit {
                        self.use_symbol("hasField");
                        writeln!(body, "  const header = {advance}(r);").unwrap();
                        writeln!(
                            body,
                            "  let source: unknown = null;\n  if (hasField(r, header, {bit})) {{"
                        )
                        .unwrap();
                        call(&mut body, 4, "source = ", &reader, args, ";");
                        4
                    } else {
                        writeln!(body, "  {advance}(r);").unwrap();
                        call(&mut body, 2, "const source = ", &reader, args, ";");
                        2
                    };
                    if validate_source {
                        self.validate(&mut body, indent, field.ty, "source", &literal(field.spec));
                    }
                    if field.bit.is_some() {
                        body.push_str("  }\n");
                    }
                    if let Some(from) = convert_from {
                        let adapt = self.adapt(scope, from, target_ty, false);
                        writeln!(body, "  const value = {adapt}(source);").unwrap();
                        "value"
                    } else {
                        "source"
                    }
                }
            }
        };
        if route.validate_target {
            self.validate(&mut body, 2, target_ty, value, &literal(route.target));
        }
        writeln!(body, "  return {value};").unwrap();
        let (id, build) = self.reserve(Op::Read, scope, format!("seek-read:{body}"));
        let reader = format!("__seekRead{id}");
        if build {
            self.functions[id] = format!("function {reader}(r: Cursor): unknown {{\n{body}}}\n");
        }
        reader
    }

    fn emit_seek_position(&mut self, position: Position) -> String {
        let layout = &self.layouts[position.layout];
        let scope = layout.scope;
        let fields = members(layout.fields);
        let header = bitmap(&fields);
        let (id, build) = self.reserve(
            Op::Skip,
            scope,
            format!("seek-position:{}:{}", position.layout, position.index),
        );
        let f = format!("__seekPosition{id}");
        if build {
            let mut out = format!("function {f}(r: Cursor): number {{\n");
            if header == 0 {
                out.push_str("  const header = r.pos;\n");
            } else {
                self.use_symbol("takeBytes");
                writeln!(out, "  const header = takeBytes(r, {header});").unwrap();
            }
            let prefix = &fields[..position.index];
            let mut fixed = 0;
            let mut width = 0;
            for field in prefix {
                if field.bit.is_some() {
                    break;
                }
                if let Some((_, bytes)) = fixed_scalar(field.ty) {
                    fixed += 1;
                    width += bytes;
                } else {
                    break;
                }
            }
            if width > 0 {
                self.use_symbol("takeBytes");
                writeln!(out, "  takeBytes(r, {width});").unwrap();
            }
            for field in &prefix[fixed..] {
                self.skip_member(&mut out, 2, scope, field);
            }
            out.push_str("  return header;\n}\n");
            self.functions[id] = out;
        }
        f
    }
}
