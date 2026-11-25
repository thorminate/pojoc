use super::super::literals::{PRINT_WIDTH, string_literal};
use super::super::readers::fixed_scalar;
use super::records::{bitmap, members};
use super::{Op, Program, call, cast, literal, name, tag};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

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

impl<'a> Program<'a> {
    pub(super) fn emit_seek_versions(
        &mut self,
        out: &mut String,
        current: usize,
        intern: bool,
        seek: &str,
        compact: bool,
    ) {
        let scope = self.layouts[current].scope;
        let versions: Vec<_> = self
            .roots
            .iter()
            .filter_map(|(&(s, version), &layout)| {
                (s == scope && layout != current).then_some((version, layout))
            })
            .collect();
        let mut current_aliases = Vec::new();
        let mut historical = BTreeMap::<_, Vec<u64>>::new();
        let mut paths = None;
        let current_table = self.seek_table(intern);
        for (version, old) in versions {
            let old_intern = self.root(scope, version)["intern"] == true;
            if old_intern == intern && self.seek_equivalent(old, current) {
                current_aliases.push(version);
            } else if !compact {
                let paths = paths.get_or_insert_with(|| self.seek_paths(current));
                let cases: Vec<_> = paths
                    .iter()
                    .filter_map(|(path, fields)| {
                        let route = self.historical_route(old, current, fields)?;
                        let reader = self.emit_historical_reader(scope, &route);
                        Some((path.clone(), reader))
                    })
                    .collect();
                if !cases.is_empty() {
                    let table = self.seek_table(old_intern);
                    historical.entry((table, cases)).or_default().push(version);
                }
            }
        }

        if current_aliases.is_empty() && historical.is_empty() {
            let read = format!("return {seek}(r, {current_table}, names, 0);");
            let line = format!("  if (r.version === version) {read}");
            if line.len() <= PRINT_WIDTH {
                writeln!(out, "{line}").unwrap();
            } else {
                writeln!(out, "  if (r.version === version)\n    {read}").unwrap();
            }
            return;
        }
        out.push_str("  switch (r.version) {\n    case version:\n");
        for version in current_aliases {
            writeln!(out, "    case {version}n:").unwrap();
        }
        call(
            out,
            6,
            "return ",
            seek,
            &["r", current_table, "names", "0"],
            ";",
        );
        for ((table, cases), versions) in historical {
            for version in versions {
                writeln!(out, "    case {version}n:").unwrap();
            }
            out.push_str("      switch (path) {\n");
            for (path, reader) in cases {
                writeln!(out, "        case {}:", string_literal(&path)).unwrap();
                call(out, 10, "return ", &reader, &["r", table], ";");
            }
            out.push_str("      }\n      break;\n");
        }
        out.push_str("  }\n");
    }

    fn seek_table(&mut self, intern: bool) -> &'static str {
        if intern {
            self.use_symbol("readTableOffsets");
            "readTableOffsets(r)"
        } else {
            "emptyTable"
        }
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

    fn historical_route(
        &self,
        old: usize,
        current: usize,
        path: &[&'a Value],
    ) -> Option<Route<'a>> {
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

    fn emit_historical_reader(&mut self, scope: usize, route: &Route<'a>) -> String {
        let mut body = String::new();
        for &position in &route.prefix {
            let advance = self.emit_seek_position(position);
            writeln!(body, "  {advance}(r);").unwrap();
        }
        let target_ty = &route.target["ty"];
        match route.leaf {
            Leaf::Default(supplied) => {
                let default = self.default_expr(scope, target_ty, supplied);
                writeln!(body, "  const value = {default};").unwrap();
            }
            Leaf::Read {
                position,
                validate_source,
                convert_from,
            } => {
                let fields = members(self.layouts[position.layout].fields);
                let field = &fields[position.index];
                let reader = self.reader(scope, field.ty, cast(field.spec));
                let advance = self.emit_seek_position(position);
                if let Some(bit) = field.bit {
                    self.use_symbol("hasField");
                    writeln!(body, "  const header = {advance}(r);").unwrap();
                    writeln!(
                        body,
                        "  let source: unknown = null;\n  if (hasField(r, header, {bit})) {{"
                    )
                    .unwrap();
                    call(
                        &mut body,
                        4,
                        "source = ",
                        &reader,
                        self.read_args(field.ty),
                        ";",
                    );
                    if validate_source {
                        self.validate(&mut body, 4, field.ty, "source", &literal(field.spec));
                    }
                    body.push_str("  }\n");
                } else {
                    writeln!(body, "  {advance}(r);").unwrap();
                    call(
                        &mut body,
                        2,
                        "const source = ",
                        &reader,
                        self.read_args(field.ty),
                        ";",
                    );
                    if validate_source {
                        self.validate(&mut body, 2, field.ty, "source", &literal(field.spec));
                    }
                }
                if let Some(from) = convert_from {
                    let adapt = self.adapt(scope, from, target_ty, false);
                    writeln!(body, "  const value = {adapt}(source);").unwrap();
                } else {
                    body.push_str("  const value = source;\n");
                }
            }
        }
        if route.validate_target {
            self.validate(&mut body, 2, target_ty, "value", &literal(route.target));
        }
        body.push_str("  return value;\n");
        let (id, build) = self.reserve(Op::Read, scope, format!("historical-seek:{body}"));
        let reader = format!("__historySeek{id}");
        if build {
            self.functions[id] =
                format!("function {reader}(r: Cursor, table: ReadTable): unknown {{\n{body}}}\n");
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
