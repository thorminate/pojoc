use super::super::readers::fixed_scalar;
use super::access::Access;
use super::records::bitmap;
use super::{Op, Program, call, cast, count, literal, tag};
use std::fmt::Write;

impl Program<'_> {
    pub(super) fn emit_row_access(
        &mut self,
        out: &mut String,
        layout: usize,
        access: &Access<'_>,
        intern: bool,
    ) {
        let scope = self.layouts[layout].scope;
        let rows: Vec<_> = access
            .fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| {
                if field.bit.is_some()
                    || cast(field.spec)
                    || tag(field.ty) != "array"
                    || tag(&field.ty[1]) != "record"
                {
                    return None;
                }
                let record = self.record(scope, &field.ty[1], false);
                Some((index, field, record, self.fixed_layout(record)?))
            })
            .collect();
        if rows.is_empty() {
            return;
        }

        self.imports.extend([
            "fixedRecordView",
            "materializedRecordView",
            "seekField",
            "startFields",
            "type Envelope",
            "type FixedRecordView",
        ]);
        let names = rows
            .iter()
            .map(|(_, field, _, _)| literal(field.spec))
            .collect::<Vec<_>>()
            .join(" | ");
        if names.chars().count() + "type __RowField = ;".len()
            <= super::super::literals::PRINT_WIDTH
        {
            writeln!(out, "\ntype __RowField = {names};").unwrap();
        } else {
            out.push_str("\ntype __RowField =\n");
            for (i, (_, field, _, _)) in rows.iter().enumerate() {
                let end = if i + 1 == rows.len() { ";" } else { "" };
                writeln!(out, "  | {}{end}", literal(field.spec)).unwrap();
            }
        }

        out.push_str(
            r#"
export function openRows<K extends __RowField>(
  input: Input,
  field: K,
  offset = 0,
): FixedRecordView<__Current[K][number]> {
  return __openRows(__openFrame(input, offset), field) as FixedRecordView<
    __Current[K][number]
  >;
}

function __openRows(r: Envelope, field: __RowField) {
  switch (field) {
"#,
        );
        let table = if intern {
            self.use_symbol("readTableOffsets");
            "readTableOffsets(r)"
        } else {
            "emptyTable"
        };
        for (index, field, record, width) in rows {
            let key = literal(field.spec);
            let read = self.fixed_row_field(record);
            writeln!(out, "    case {key}: {{").unwrap();
            out.push_str(
                "      if (r.version !== version) {\n        return materializedRecordView(__decodeFrame(r)[field]);\n      }\n",
            );
            let bitmap = bitmap(&access.fields).to_string();
            let prefix = access.width.to_string();
            call(
                out,
                6,
                "const scan = ",
                "startFields",
                &["r", table, &bitmap, &prefix],
                ";",
            );
            call(
                out,
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
            let size = &field.ty[2];
            if size.is_null() {
                self.use_symbol("readLength");
            }
            call(
                out,
                6,
                "return ",
                "fixedRecordView",
                &["r", &count(size), &width.to_string(), &read],
                ";",
            );
            out.push_str("    }\n");
        }
        out.push_str("  }\n  return invalid(`Unknown fixed Pojoc row array ${field}`);\n}\n");
    }

    fn fixed_row_field(&mut self, layout: usize) -> String {
        let scope = self.layouts[layout].scope;
        let (id, build) = self.reserve(Op::Read, scope, format!("row-field:{layout}"));
        let f = format!("__rowField{id}");
        if build {
            self.imports.extend(["cursorView", "readFixedScalar"]);
            let mut out = format!(
                "function {f}(r: Cursor, pos: number, key: PropertyKey): unknown {{\n  switch (key) {{\n"
            );
            let mut offset = 0;
            for field in self.layouts[layout].fields {
                let ty = &field["ty"];
                let (_, width) = fixed_scalar(ty).unwrap();
                let position = if offset == 0 {
                    "pos".to_owned()
                } else {
                    format!("pos + {offset}")
                };
                writeln!(out, "    case {}:", literal(field)).unwrap();
                call(
                    &mut out,
                    6,
                    "return ",
                    "readFixedScalar",
                    &["cursorView(r)", &position, &ty.to_string()],
                    ";",
                );
                offset += width;
            }
            out.push_str("  }\n  return invalid(\"Unknown Pojoc row field\");\n}\n");
            self.functions[id] = out;
        }
        f
    }
}
