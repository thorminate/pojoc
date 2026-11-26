use super::literals::PRINT_WIDTH;
use std::collections::BTreeSet;
use std::fmt::Write;

pub(super) const DECODE: &str = r#"
export function decode(input: Input): __Current {
  return __decodeFrame(__openFrame(input));
}

export function decodeFrom(input: Input, offset = 0) {
  const r = __openFrame(input, offset);
  return { value: __decodeFrame(r), version: r.version, offset: r.end };
}
"#;

pub(super) fn emit_imports(out: &mut String, imports: &BTreeSet<&str>) {
    let inline = format!(
        "import {{ {} }} from \"pojoc/v1/compiler\";",
        imports.iter().copied().collect::<Vec<_>>().join(", ")
    );
    if inline.len() <= PRINT_WIDTH {
        writeln!(out, "{inline}").unwrap();
    } else {
        out.push_str("import {\n");
        for item in imports {
            writeln!(out, "  {item},").unwrap();
        }
        out.push_str("} from \"pojoc/v1/compiler\";\n");
    }
}
