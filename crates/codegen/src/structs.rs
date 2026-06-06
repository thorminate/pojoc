use std::collections::HashMap;
use pojoc_schema::ir::types::*;
use super::types::rust_field_type;
use super::writer::CodeWriter;

pub fn emit_structs(schema: &ResolvedSchema, w: &mut CodeWriter) {
    // latest version of each user-defined type
    let mut latest: HashMap<String, (u32, &ResolvedType)> = HashMap::new();
    for (type_id, resolved) in &schema.types.types {
        let entry = latest.entry(type_id.name.clone()).or_insert((0, resolved));
        if type_id.version > entry.0 {
            *entry = (type_id.version, resolved);
        }
    }

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        emit_named_struct(name, &resolved.fields, w);
        w.blank();
    }

    // top-level schema struct
    let latest_version = schema.versions.last().expect("no versions");
    emit_named_struct(&schema.name_hint, &latest_version.fields, w);
    w.blank();
}

fn emit_named_struct(name: &str, fields: &[FieldIR], w: &mut CodeWriter) {
    w.line("#[derive(Debug, Clone, Default, Serialize, Deserialize)]");
    w.line(&format!("pub struct {name} {{"));
    w.indent();
    for field in fields {
        let ty = rust_field_type(&field.ty);
        w.line(&format!("pub {}: {ty},", field.name));
    }
    w.dedent();
    w.line("}");
}