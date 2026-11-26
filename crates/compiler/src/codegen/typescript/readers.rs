use crate::core::types::{WireSize, primitive_info};
use serde_json::Value;
use std::fmt::Write;

pub(super) fn fixed_scalar(ty: &Value) -> Option<(&'static str, usize)> {
    let info = primitive_info(ty.as_str()?)?;
    let WireSize::Fixed(width) = info.wire_size else {
        return None;
    };
    Some((info.typescript_reader?, width))
}

pub(super) fn emit_fixed_read(
    out: &mut String,
    ty: &Value,
    index: usize,
    start: usize,
    offset: usize,
) {
    let (accessor, width) = fixed_scalar(ty).unwrap();
    let position = |offset: usize| {
        if offset == 0 {
            format!("p{start}")
        } else {
            format!("p{start} + {offset}")
        }
    };
    let byte = |index| format!("r.data[{}]", position(offset + index));
    let first = byte(0);
    match ty.as_str().unwrap() {
        "u8" => writeln!(out, "  const v{index} = {first};").unwrap(),
        "i8" => writeln!(out, "  const v{index} = ({first} << 24) >> 24;").unwrap(),
        "bool" => writeln!(out, "  const v{index} = {first} !== 0;").unwrap(),
        "u16" => writeln!(out, "  const v{index} = {first} | ({} << 8);", byte(1)).unwrap(),
        "i16" => writeln!(
            out,
            "  const v{index} = (({first} | ({} << 8)) << 16) >> 16;",
            byte(1),
        )
        .unwrap(),
        // Keep each byte in a 32-bit word on its own line.
        "i32" | "u32" => {
            writeln!(out, "  const v{index} =").unwrap();
            let unsigned = ty.as_str() == Some("u32");
            if unsigned {
                writeln!(out, "    ({first} |").unwrap();
                for i in 1..width - 1 {
                    writeln!(out, "      ({} << {}) |", byte(i), i * 8).unwrap();
                }
                writeln!(out, "      ({} << 24)) >>>\n    0;", byte(3)).unwrap();
            } else {
                writeln!(out, "    {first} |").unwrap();
                for i in 1..width - 1 {
                    writeln!(out, "    ({} << {}) |", byte(i), i * 8).unwrap();
                }
                writeln!(out, "    ({} << 24);", byte(3)).unwrap();
            }
        }
        _ => writeln!(
            out,
            "  const v{index} = cursorView(r).{accessor}({}, true);",
            position(offset),
        )
        .unwrap(),
    }
}

pub(super) fn record_array(fields: &[Value]) -> Option<(&str, &str)> {
    let [field] = fields else { return None };
    let [kind, inner, count] = field["ty"].as_array()?.as_slice() else {
        return None;
    };
    if kind != "array" || !count.is_null() || field["cast"] != false || inner.get(0)? != "record" {
        return None;
    }
    Some((field["name"].as_str()?, inner.get(2)?.as_str()?))
}
