use std::fmt::Write;

const MODULES: &[(&str, &str)] = &[
    ("Shared wire types", include_str!("runtime/types.ts")),
    (
        "Byte cursors and envelopes",
        include_str!("runtime/wire.ts"),
    ),
    ("Flat write cursor", include_str!("runtime/writer.ts")),
    ("Scalar writers", include_str!("runtime/scalar_writes.ts")),
    (
        "Scalar and packed values",
        include_str!("runtime/scalars.ts"),
    ),
    ("Payload operations", include_str!("runtime/compiled.ts")),
    ("Field access", include_str!("runtime/field_access.ts")),
];

pub(super) fn emit(out: &mut String) {
    for (index, (title, source)) in MODULES.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        writeln!(out, "// {title}.").unwrap();

        // Each module keeps its imports and exports outside its declarations
        // so standalone output can put the declarations in one shared scope.
        let (body, _) = source
            .split_once("\nexport ")
            .expect("runtime module must end with an export list");
        let mut lines = body.trim_end().lines();
        if source.starts_with("import ") {
            for line in lines.by_ref() {
                if line.is_empty() {
                    break;
                }
            }
        }
        for line in lines {
            writeln!(out, "{line}").unwrap();
        }
    }
}
