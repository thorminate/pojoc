use super::super::literals::{data_key, emit_value, string_literal};
use super::{Op, Program, call, cast, literal, name, tag};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;

impl Program<'_> {
    pub(super) fn defaults_layout(&mut self, layout: usize) -> String {
        let scope = self.layouts[layout].scope;
        let (id, build) = self.reserve(Op::Default, scope, format!("layout:{layout}"));
        let f = format!("__create{id}");
        if build {
            let fields = self.layouts[layout].fields;
            let mut out = format!("function {f}(value: Obj): Obj {{\n");
            if fields.is_empty() {
                out.push_str("  return {};\n");
            } else {
                for (i, field) in fields.iter().enumerate() {
                    let key = literal(field);
                    let default = self.default_expr(scope, &field["ty"], &field["default"]);
                    let conditional =
                        format!("Object.hasOwn(value, {key}) ? value[{key}] : {default};");
                    let line = format!("  const v{i} = {conditional}");
                    if line.chars().count() <= 80 {
                        writeln!(out, "{line}").unwrap();
                    } else {
                        writeln!(out, "  const v{i} = Object.hasOwn(value, {key})\n    ? value[{key}]\n    : {default};").unwrap();
                    }
                }
                return_fields(&mut out, fields);
            }
            out.push_str("}\n");
            self.functions[id] = out;
        }
        f
    }

    pub(super) fn default_expr(&mut self, scope: usize, ty: &Value, supplied: &Value) -> String {
        if let Some(primitive) = ty.as_str() {
            return primitive_default(primitive, supplied);
        }
        match tag(ty) {
            "optional" if supplied.is_null() => "null".into(),
            "optional" | "constraint" | "box" => self.default_expr(scope, &ty[1], supplied),
            "enum" | "vfloat" => numeric(supplied),
            "record" => {
                let create = self.defaults_layout(self.record(scope, ty, false));
                format!("{create}({{}})")
            }
            "import" => {
                let (scope, _) = self.imported(scope, ty);
                let create = self.defaults_layout(self.roots[&(scope, self.latest(scope))]);
                format!("{create}({{}})")
            }
            "bytes" | "bits" if tag(supplied) != "bytes" && tag(supplied) != "integer" => {
                let width = &ty[if tag(ty) == "bits" { 2 } else { 1 }];
                format!("new Uint8Array({width})")
            }
            _ => format!("{}()", self.default_factory(scope, ty, supplied)),
        }
    }

    fn default_factory(&mut self, scope: usize, ty: &Value, supplied: &Value) -> String {
        let (id, build) = self.reserve(Op::Default, scope, format!("type:{ty}:{supplied}"));
        let f = format!("__default{id}");
        if !build {
            return f;
        }
        let mut out = format!("function {f}(): unknown {{\n");
        match tag(ty) {
            "bytes" | "bits" => {
                let width = &ty[if tag(ty) == "bits" { 2 } else { 1 }];
                if tag(supplied) == "bytes" {
                    self.use_symbol("resized");
                    out.push_str("  const bytes = Uint8Array.from(");
                    emit_value(&mut out, &supplied[1], 2);
                    writeln!(out, ");\n  return resized(bytes, {width});").unwrap();
                } else {
                    self.use_symbol("unsignedBytes");
                    writeln!(
                        out,
                        "  return unsignedBytes({}n, {width});",
                        numeric(supplied)
                    )
                    .unwrap();
                }
            }
            "union" => {
                if let Some(first) = ty[3].get(0) {
                    let kind = string_literal(first[1].as_str().unwrap());
                    let value = self.default_expr(scope, &first[2], &Value::Null);
                    writeln!(out, "  return {{ kind: {kind}, value: {value} }};").unwrap();
                } else {
                    out.push_str("  return { kind: \"Unknown\", discriminant: 0, data: new Uint8Array() };\n");
                }
            }
            "tuple" => {
                let values: Vec<_> = ty[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(i, ty)| self.default_expr(scope, ty, &supplied[1][i]))
                    .collect();
                let line = format!("  return [{}];", values.join(", "));
                if line.len() <= 80 {
                    writeln!(out, "{line}").unwrap();
                } else {
                    out.push_str("  return [\n");
                    for value in values {
                        writeln!(out, "    {value},").unwrap();
                    }
                    out.push_str("  ];\n");
                }
            }
            "array" | "delta" => self.default_array(&mut out, scope, ty, supplied),
            "map" => self.default_map(&mut out, scope, ty, supplied),
            tag => unreachable!("unhandled TypeScript default for {tag}"),
        }
        out.push_str("}\n");
        self.functions[id] = out;
        f
    }

    fn default_array(&mut self, out: &mut String, scope: usize, ty: &Value, supplied: &Value) {
        let items = if tag(supplied) == "items" {
            supplied[1].as_array().unwrap().as_slice()
        } else {
            &[]
        };
        let count = ty[2].as_u64().unwrap_or(items.len() as u64);
        let packed = super::values::typed_array(&ty[1]);
        if count == 0 {
            if let Some(ctor) = packed {
                writeln!(out, "  return new {ctor}(0);").unwrap();
            } else {
                out.push_str("  return [];\n");
            }
            return;
        }
        if items.is_empty() {
            let value = self.default_expr(scope, &ty[1], &supplied[1]);
            self.use_symbol("arrayDefault");
            writeln!(out, "  const items = arrayDefault({count}, () => {value});").unwrap();
        } else {
            let values: Vec<_> = items
                .iter()
                .take(count as usize)
                .map(|item| self.default_expr(scope, &ty[1], item))
                .collect();
            let line = format!("  const items = [{}];", values.join(", "));
            if line.chars().count() <= 80 {
                writeln!(out, "{line}").unwrap();
            } else {
                out.push_str("  const items = [\n");
                for value in values {
                    writeln!(out, "    {value},").unwrap();
                }
                out.push_str("  ];\n");
            }
            if count as usize > items.len() {
                let value = self.default_expr(scope, &ty[1], &Value::Null);
                writeln!(out, "  while (items.length < {count}) items.push({value});").unwrap();
            }
        }
        if packed.is_some() {
            self.use_symbol("typedValues");
            writeln!(out, "  return typedValues({}, items);", ty[1]).unwrap();
        } else {
            out.push_str("  return items;\n");
        }
    }

    fn default_map(&mut self, out: &mut String, scope: usize, ty: &Value, supplied: &Value) {
        let pairs = if tag(supplied) == "pairs" {
            supplied[1].as_array().unwrap().as_slice()
        } else {
            &[]
        };
        let fixed = ty[3].as_u64().map(|n| n as usize);
        if pairs.is_empty() && fixed.is_none() {
            out.push_str("  return new Map();\n");
            return;
        }
        if pairs.is_empty() {
            out.push_str("  const pairs: [unknown, unknown][] = [];\n");
        } else {
            out.push_str("  const pairs: [unknown, unknown][] = [\n");
            for pair in pairs.iter().take(fixed.unwrap_or(pairs.len())) {
                let key = self.default_expr(scope, &ty[1], &pair[0]);
                let value = self.default_expr(scope, &ty[2], &pair[1]);
                writeln!(out, "    [{key}, {value}],").unwrap();
            }
            out.push_str("  ];\n");
        }
        if let Some(count) = fixed {
            if count > pairs.len() {
                let key = self.default_expr(scope, &ty[1], &Value::Null);
                let value = self.default_expr(scope, &ty[2], &Value::Null);
                writeln!(
                    out,
                    "  while (pairs.length < {count}) pairs.push([{key}, {value}]);"
                )
                .unwrap();
            }
            out.push_str("  return pairs;\n");
        } else {
            out.push_str("  return new Map(pairs);\n");
        }
    }

    pub(super) fn project_layout(&mut self, from: usize, to: usize, down: bool) -> String {
        let scope = self.layouts[to].scope;
        let (id, build) = self.reserve(Op::Project, scope, format!("{from}:{to}:{down}"));
        let f = format!("__convert{id}");
        if !build {
            return f;
        }
        let source = self.layouts[from].fields;
        let target = self.layouts[to].fields;
        let previous: BTreeMap<_, _> = source
            .iter()
            .map(|field| (field["id"].as_str().unwrap(), field))
            .collect();
        let mut out = format!("function {f}(value: Obj): Obj {{\n");
        if down && self.layouts[from].root {
            for field in source {
                let key = literal(field);
                self.validate(&mut out, 2, &field["ty"], &format!("value[{key}]"), &key);
            }
        }
        for (i, field) in target.iter().enumerate() {
            let ty = &field["ty"];
            let old = previous.get(field["id"].as_str().unwrap());
            let default = if down {
                &Value::Null
            } else {
                &field["default"]
            };
            let expr = if let Some(old) = old {
                let ret = if down { cast(field) } else { cast(old) };
                let lazy = if down {
                    old["lazy"] == true
                } else {
                    field["lazy"] == true
                };
                let prior = format!("value[{}]", literal(old));
                if ret && lazy {
                    self.default_expr(scope, ty, &Value::Null)
                } else if ret || old["ty"] != *ty {
                    let adapt = self.adapt(scope, &old["ty"], ty, down);
                    format!("{adapt}({prior})")
                } else {
                    prior
                }
            } else {
                self.default_expr(scope, ty, default)
            };
            writeln!(out, "  const v{i} = {expr};").unwrap();
            if !down && self.layouts[to].root {
                self.validate(&mut out, 2, ty, &format!("v{i}"), &literal(field));
            }
        }
        if target.is_empty() {
            out.push_str("  return {};\n");
        } else {
            return_fields(&mut out, target);
        }
        out.push_str("}\n");
        self.functions[id] = out;
        f
    }

    pub(super) fn adapt(&mut self, scope: usize, from: &Value, to: &Value, down: bool) -> String {
        let (id, build) = self.reserve(Op::Adapt, scope, format!("{down}:{from}:{to}"));
        let f = format!("__adapt{id}");
        if !build {
            return f;
        }
        let mut out = format!("function {f}(value: unknown): unknown {{\n");
        let default = self.default_expr(scope, to, &Value::Null);
        writeln!(out, "  if (value == null) return {default};").unwrap();
        self.adapt_value(&mut out, scope, from, to, down);
        out.push_str("}\n");
        self.functions[id] = out;
        f
    }

    fn adapt_value(
        &mut self,
        out: &mut String,
        scope: usize,
        mut from: &Value,
        mut to: &Value,
        down: bool,
    ) {
        while matches!(tag(from), "optional" | "box" | "constraint") {
            from = &from[1];
        }
        while matches!(tag(to), "optional" | "box" | "constraint") {
            to = &to[1];
        }
        let from_tag = tag(from);
        let to_tag = tag(to);
        if from.is_string() && to.is_string() && from != to {
            self.use_symbol("castScalar");
            let float = from == "f32" || from == "f64";
            writeln!(out, "  return castScalar(value, {float}, {to});").unwrap();
        } else if from_tag == "bits" && to.is_string() {
            self.imports.extend(["bytesAsInt", "castScalar"]);
            writeln!(
                out,
                "  return castScalar(bytesAsInt(value as Uint8Array), false, {to});"
            )
            .unwrap();
        } else if to_tag == "bits" && from.is_string() {
            self.use_symbol("unsignedBytes");
            writeln!(
                out,
                "  return unsignedBytes(BigInt(value as number | bigint), {});",
                to[2]
            )
            .unwrap();
        } else if from.is_string() || to.is_string() {
            out.push_str("  return value;\n");
        } else if from_tag == "record" && to_tag == "record" {
            self.use_symbol("object");
            let source = self.record(scope, from, !down);
            let target = self.record(scope, to, down);
            let project = self.project_layout(source, target, down);
            writeln!(out, "  return {project}(object(value));").unwrap();
        } else if matches!(to_tag, "bytes" | "bits") {
            self.use_symbol("resized");
            let width = &to[if to_tag == "bits" { 2 } else { 1 }];
            writeln!(out, "  return resized(value as Uint8Array, {width});").unwrap();
        } else if matches!(to_tag, "array" | "delta") {
            self.use_symbol("adaptArray");
            let default = self.default_expr(scope, &to[1], &Value::Null);
            let count = to[2].to_string();
            let factory = format!("() => {default}");
            if super::values::typed_array(&to[1]).is_some() {
                self.use_symbol("typedValues");
                call(
                    out,
                    2,
                    "const items = ",
                    "adaptArray",
                    &["value", &count, &factory],
                    ";",
                );
                writeln!(out, "  return typedValues({}, items);", to[1]).unwrap();
            } else {
                call(
                    out,
                    2,
                    "return ",
                    "adaptArray",
                    &["value", &count, &factory],
                    ";",
                );
            }
        } else if to_tag == "map" && from_tag == "array" {
            self.use_symbol("sequence");
            out.push_str("  const items = sequence(value);\n  return new Map(\n    Array.from({ length: items.length }, (_, i) => [i, items[i]] as const),\n  );\n");
        } else if to_tag == "map" && !to[3].is_null() {
            let key = self.default_expr(scope, &to[1], &Value::Null);
            let item = self.default_expr(scope, &to[2], &Value::Null);
            self.use_symbol("adaptMap");
            let pair = format!("() => [{key}, {item}]");
            call(
                out,
                2,
                "return ",
                "adaptMap",
                &[
                    "value",
                    &to[3].to_string(),
                    if down { "true" } else { "false" },
                    &pair,
                ],
                ";",
            );
        } else {
            out.push_str("  return value;\n");
        }
    }
}

fn numeric(value: &Value) -> String {
    let number = match tag(value) {
        "integer" => value[1].as_str().unwrap().into(),
        "bytes" => {
            let mut bytes = [0; 16];
            for (to, value) in bytes.iter_mut().zip(value[1].as_array().unwrap()) {
                *to = value.as_u64().unwrap() as u8;
            }
            u128::from_le_bytes(bytes).to_string()
        }
        _ if value.is_null() => "0".into(),
        _ => value.to_string(),
    };
    number.replace("e+", "e")
}

fn primitive_default(ty: &str, supplied: &Value) -> String {
    match ty {
        "string" | "intern" => string_literal(supplied.as_str().unwrap_or_default()),
        "bool" => supplied.as_bool().unwrap_or(false).to_string(),
        "u64" | "i64" | "varint64" => format!("{}n", numeric(supplied)),
        "f32" if !supplied.is_null() => format!("Math.fround({})", numeric(supplied)),
        _ => numeric(supplied),
    }
}

fn return_fields(out: &mut String, fields: &[Value]) {
    out.push_str("  return {\n");
    for (i, field) in fields.iter().enumerate() {
        writeln!(out, "    {}: v{i},", data_key(name(field))).unwrap();
    }
    out.push_str("  };\n");
}
