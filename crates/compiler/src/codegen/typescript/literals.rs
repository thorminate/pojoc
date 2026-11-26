use serde_json::{Value, json};
use std::fmt::Write;

pub(super) const PRINT_WIDTH: usize = 80;

pub(super) fn string_literal(value: &str) -> String {
    let literal = json!(value).to_string();
    if value.matches('"').count() > value.matches('\'').count() {
        let value = &literal[1..literal.len() - 1];
        format!("'{}'", value.replace("\\\"", "\"").replace('\'', "\\'"))
    } else {
        literal
    }
}

pub(super) fn property_key(value: &str) -> String {
    if value.as_bytes().split_first().is_some_and(|(first, rest)| {
        (first.is_ascii_alphabetic() || matches!(first, b'_' | b'$'))
            && rest
                .iter()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'$'))
    }) {
        value.to_owned()
    } else {
        string_literal(value)
    }
}

pub(super) fn data_key(value: &str) -> String {
    if value == "__proto__" {
        r#"["__proto__"]"#.to_owned()
    } else {
        property_key(value)
    }
}

fn inline_value(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) => {
            if items.len() > 1
                && items
                    .iter()
                    .all(|item| item.as_array().is_some_and(|row| row.len() > 1))
            {
                return None;
            }
            let values = items.iter().map(inline_value).collect::<Option<Vec<_>>>()?;
            Some(format!("[{}]", values.join(", ")))
        }
        Value::Object(properties) if properties.is_empty() => Some("{}".to_owned()),
        Value::Object(_) => None,
        Value::String(value) => Some(string_literal(value)),
        Value::Number(value) => Some(value.to_string().replace("e+", "e")),
        _ => Some(value.to_string()),
    }
}

// Schema metadata uses ordinary TypeScript literals with one property per line.
// Keep short wire descriptors together and wrap long literal arrays in place.
pub(super) fn emit_value(out: &mut String, value: &Value, indent: usize) {
    let column = out.rsplit('\n').next().unwrap_or_default().chars().count();
    match value {
        Value::Object(properties) if !properties.is_empty() => {
            out.push_str("{\n");
            for (key, value) in properties {
                write!(
                    out,
                    "{:indent$}{}: ",
                    "",
                    property_key(key),
                    indent = indent + 2
                )
                .unwrap();
                emit_value(out, value, indent + 2);
                out.push_str(",\n");
            }
            write!(out, "{:indent$}}}", "").unwrap();
        }
        Value::Array(items) => {
            let numeric = items.iter().all(Value::is_number);
            if let Some(flat) = inline_value(value)
                && (items.is_empty() || column + flat.chars().count() < PRINT_WIDTH)
            {
                out.push_str(&flat);
                return;
            }
            out.push_str("[\n");
            let child_indent = indent + 2;
            if numeric {
                let mut column = child_indent;
                write!(out, "{:child_indent$}", "").unwrap();
                for item in items {
                    let literal = inline_value(item).unwrap();
                    let width = literal.len() + 1;
                    if column > child_indent && column + width + 1 > PRINT_WIDTH {
                        writeln!(out).unwrap();
                        write!(out, "{:child_indent$}", "").unwrap();
                        column = child_indent;
                    }
                    if column > child_indent {
                        out.push(' ');
                        column += 1;
                    }
                    write!(out, "{literal},").unwrap();
                    column += width;
                }
                out.push('\n');
            } else {
                for item in items {
                    write!(out, "{:child_indent$}", "").unwrap();
                    emit_value(out, item, child_indent);
                    out.push_str(",\n");
                }
            }
            write!(out, "{:indent$}]", "").unwrap();
        }
        _ => out.push_str(&inline_value(value).unwrap()),
    }
}
