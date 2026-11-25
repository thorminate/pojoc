use super::super::literals::data_key;
use super::super::readers::fixed_scalar;
use super::records::{Field, bitmap, members};
use super::{Op, Program, call, cast, literal, name, tag};
use std::fmt::Write;

pub(super) struct Access<'a> {
    pub(super) fields: Vec<Field<'a>>,
    pub(super) offsets: Vec<usize>,
    pub(super) width: usize,
}

impl<'a> Access<'a> {
    fn new(fields: &'a [serde_json::Value]) -> Self {
        let fields = members(fields);
        let mut width = 0;
        let offsets = fields
            .iter()
            .map_while(|f| {
                if f.bit.is_some() {
                    return None;
                }
                let (_, size) = fixed_scalar(f.ty)?;
                let offset = width;
                width += size;
                Some(offset)
            })
            .collect();
        Self {
            fields,
            offsets,
            width,
        }
    }
}

impl Program<'_> {
    pub(super) fn emit_access(
        &mut self,
        out: &mut String,
        layout: usize,
        intern: bool,
        compact: bool,
    ) {
        self.imports.extend([
            "emptyTable",
            "fieldView",
            "invalid",
            "object",
            "setField",
            "type FieldScan",
        ]);
        let fields = self.layouts[layout].fields;
        let access = Access::new(fields);
        self.access_layout(layout);
        let seek = self.seek_layout(layout);

        out.push_str("\n// Partial field access.\n");
        out.push_str("\nfunction __name(index: number): string {\n  switch (index) {\n");
        for (i, field) in fields.iter().enumerate() {
            writeln!(out, "    case {i}:\n      return {};", literal(field)).unwrap();
        }
        out.push_str(
            "    default:\n      return invalid(\"Invalid Pojoc field index\");\n  }\n}\n",
        );

        out.push_str("\nfunction __field(key: string): number {\n  switch (key) {\n");
        for (i, field) in fields.iter().enumerate() {
            writeln!(out, "    case {}:\n      return {i};", literal(field)).unwrap();
        }
        out.push_str("    default:\n      return invalid(`Unknown Pojoc field ${key}`);\n  }\n}\n");

        writeln!(out, "\nfunction __view(r: Cursor) {{").unwrap();
        call(
            out,
            2,
            "return ",
            "fieldView",
            &[
                "r",
                if intern { "true" } else { "false" },
                &bitmap(&access.fields).to_string(),
                &access.width.to_string(),
                &access.offsets.len().to_string(),
                &format!("__offset{layout}"),
                &format!("__skipField{layout}"),
                &format!("__get{layout}"),
            ],
            ";",
        );
        out.push_str("}\n");

        out.push_str("\nclass __View {\n  #read: (index: number) => unknown;\n\n  constructor(r: Cursor) {\n    this.#read = __view(r);\n  }\n");
        for (i, field) in fields.iter().enumerate() {
            let key = if name(field) == "constructor" {
                r#"["constructor"]"#.to_owned()
            } else {
                data_key(name(field))
            };
            writeln!(out, "\n  get {key}() {{\n    return this.#read({i});\n  }}").unwrap();
        }
        out.push_str("}\n");
        out.push_str(
            r#"
export function open(input: Input, offset = 0): Readonly<__Current> {
  const r = __openFrame(input, offset);
  if (r.version !== version) return Object.freeze(__decodeFrame(r));
  return new __View(r) as unknown as Readonly<__Current>;
}

export function select<K extends keyof __Current & string>(keys: readonly K[]) {
  const selected = Array.from(new Set(keys), __field).sort((a, b) => a - b);
  return function projectFields(input: Input, offset = 0): Pick<__Current, K> {
    const r = __openFrame(input, offset);
    const result: Obj = {};
    if (r.version !== version) {
      const value = object(__decodeFrame(r));
      for (const index of selected) {
        const key = __name(index);
        setField(result, key, value[key]);
      }
    } else if (selected.length > 0) {
      const read = __view(r);
      for (const index of selected)
        setField(result, __name(index), read(index));
    }
    return result as Pick<__Current, K>;
  };
}

export function seek<K extends keyof __Current & string>(
  input: Input,
  path: K,
): __Current[K];
export function seek(input: Input, path: string): unknown;
export function seek(input: Input, path: string): unknown {
  const names = path.split(".");
"#,
        );
        writeln!(out, "  {seek}(undefined, emptyTable, names, 0);").unwrap();
        out.push_str("  const r = __openFrame(input);\n");
        self.emit_seek_versions(out, layout, intern, &seek, compact);
        out.push_str("  let value: unknown = __decodeFrame(r);\n  for (const name of names) value = object(value)[name];\n  return value;\n}\n");
        self.emit_row_access(out, layout, &access, intern);
    }

    fn access_layout(&mut self, layout: usize) {
        let scope = self.layouts[layout].scope;
        let (id, build) = self.reserve(Op::Read, scope, format!("access:{layout}"));
        if !build {
            return;
        }
        let access = Access::new(self.layouts[layout].fields);
        let mut out = format!("function __offset{layout}(index: number): number {{\n");
        if !access.offsets.is_empty() {
            out.push_str("  switch (index) {\n");
            for (i, offset) in access.offsets.iter().enumerate() {
                writeln!(out, "    case {i}:\n      return {offset};").unwrap();
            }
            out.push_str("  }\n");
        }
        out.push_str("  return -1;\n}\n");

        writeln!(
            out,
            "\nfunction __skipField{layout}(scan: FieldScan, index: number): void {{"
        )
        .unwrap();
        if access.offsets.len() < access.fields.len() {
            out.push_str("  const { r, header } = scan;\n  switch (index) {\n");
            for (i, field) in access.fields.iter().enumerate().skip(access.offsets.len()) {
                writeln!(out, "    case {i}:").unwrap();
                self.skip_member(&mut out, 6, scope, field);
                out.push_str("      return;\n");
            }
            out.push_str("  }\n");
        }
        out.push_str("}\n");

        writeln!(
            out,
            "\nfunction __get{layout}(scan: FieldScan, index: number): unknown {{"
        )
        .unwrap();
        if !access.fields.is_empty() {
            out.push_str("  const { r, table, header } = scan;\n  switch (index) {\n");
            for (i, field) in access.fields.iter().enumerate() {
                let check = self.has_validation(field.ty);
                if check {
                    writeln!(out, "    case {i}: {{").unwrap();
                } else {
                    writeln!(out, "    case {i}:").unwrap();
                }
                if let Some(bit) = field.bit {
                    self.use_symbol("hasField");
                    writeln!(out, "      if (!hasField(r, header, {bit})) return null;").unwrap();
                }
                let read = self.reader(scope, field.ty, cast(field.spec));
                let prefix = if check { "const value = " } else { "return " };
                call(&mut out, 6, prefix, &read, self.read_args(field.ty), ";");
                if check {
                    self.validate(&mut out, 6, field.ty, "value", &literal(field.spec));
                    out.push_str("      return value;\n    }\n");
                }
            }
            out.push_str("  }\n");
        }
        out.push_str("  return invalid(\"Invalid Pojoc field index\");\n}\n");
        self.functions[id] = out;
    }

    fn seek_layout(&mut self, layout: usize) -> String {
        let scope = self.layouts[layout].scope;
        let (id, build) = self.reserve(Op::Read, scope, format!("seek:{layout}"));
        let f = format!("__seek{id}");
        if !build {
            return f;
        }
        let access = Access::new(self.layouts[layout].fields);
        let mut out = format!(
            "function {f}(\n  r: Cursor | undefined,\n  table: ReadTable,\n  names: readonly string[],\n  depth: number,\n): unknown {{\n"
        );
        let seekable: Vec<_> = access
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| field.spec["seek"] == true)
            .collect();
        if !seekable.is_empty() {
            self.imports.extend(["seekField", "startFields"]);
            out.push_str("  const last = depth + 1 === names.length;\n  switch (names[depth]) {\n");
            for (index, field) in seekable {
                let key = literal(field.spec);
                writeln!(out, "    case {key}: {{").unwrap();
                let child = (field.bit.is_none()
                    && tag(field.ty) == "record"
                    && field.spec["lazy"] != true)
                    .then(|| self.record(scope, field.ty, false));
                let seek = child.map(|child| {
                    self.access_layout(child);
                    self.seek_layout(child)
                });
                if let Some(seek) = &seek {
                    writeln!(out, "      if (!r) return last ? undefined : {seek}(r, table, names, depth + 1);").unwrap();
                } else {
                    out.push_str("      if (!last) return invalid(`Invalid nested path ${names.join(\".\")}`);\n      if (!r) return;\n");
                }
                writeln!(
                    out,
                    "      const scan = startFields(r, table, {}, {});",
                    bitmap(&access.fields),
                    access.width
                )
                .unwrap();
                call(
                    &mut out,
                    6,
                    "",
                    "seekField",
                    &[
                        "scan",
                        &index.to_string(),
                        &access.offsets.len().to_string(),
                        &format!("__offset{layout}"),
                        &format!("__skipField{layout}"),
                    ],
                    ";",
                );
                if let Some(seek) = seek {
                    writeln!(out, "      return last ? __get{layout}(scan, {index}) : {seek}(r, table, names, depth + 1);").unwrap();
                } else {
                    writeln!(out, "      return __get{layout}(scan, {index});").unwrap();
                }
                out.push_str("    }\n");
            }
            out.push_str("  }\n");
        }
        out.push_str("  return invalid(`Unknown seekable Pojoc path ${names.join(\".\")}`);\n}\n");
        self.functions[id] = out;
        f
    }
}
