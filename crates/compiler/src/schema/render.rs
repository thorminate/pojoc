//! Renders resolved IR back into `.pojoc` source syntax.
//!
//! Shared by the LSP (hover signatures) and `pojoc show`, so the two never
//! describe the same schema differently.

use crate::core::types::ResolvedTypeRef;
use crate::schema::ir::ir_types::{DefaultValue, FieldIR};

/// a resolved type as it would be written in a schema
pub fn render_type_ref(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Scalar(id) | ResolvedTypeRef::Union(id) => id.name.clone(),
        ResolvedTypeRef::Enum(id, _) | ResolvedTypeRef::Bitset(id, _) => id.name.clone(),
        ResolvedTypeRef::Array(inner) => format!("[{}]", render_type_ref(inner)),
        ResolvedTypeRef::FixedArray(inner, n) => format!("[{}]({n})", render_type_ref(inner)),
        ResolvedTypeRef::DeltaArray(inner) => format!("[{}](delta)", render_type_ref(inner)),
        ResolvedTypeRef::FixedDeltaArray(inner, n) => {
            format!("[{}](delta, {n})", render_type_ref(inner))
        }
        ResolvedTypeRef::FixedString(n) => format!("string({n})"),
        ResolvedTypeRef::Map(k, v) => {
            format!("map<{}, {}>", render_type_ref(k), render_type_ref(v))
        }
        ResolvedTypeRef::FixedMap(k, v, n) => {
            format!("map<{}, {}>({n})", render_type_ref(k), render_type_ref(v))
        }
        ResolvedTypeRef::Tuple(elems) => format!(
            "({})",
            elems
                .iter()
                .map(render_type_ref)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ResolvedTypeRef::VFloat { min, max, step, .. } => {
            format!("vfloat(min: {min}, max: {max}, step: {step})")
        }
        ResolvedTypeRef::Optional(inner) => format!("{}?", render_type_ref(inner)),
        ResolvedTypeRef::Boxed(inner) => format!("box<{}>", render_type_ref(inner)),
        ResolvedTypeRef::Constrained { inner, min, max } => {
            let bounds = match (min, max) {
                (Some(mn), Some(mx)) => format!("min: {mn}, max: {mx}"),
                (Some(mn), None) => format!("min: {mn}"),
                (None, Some(mx)) => format!("max: {mx}"),
                (None, None) => String::new(),
            };
            format!("{}({bounds})", render_type_ref(inner))
        }
        ResolvedTypeRef::Interned(inner) => format!("intern {}", render_type_ref(inner)),
        ResolvedTypeRef::ImportedSchema { alias, version, .. } => format!("{alias}@{version}"),
    }
}

/// a field's modifier keywords in canonical order, with a trailing space when
/// non-empty, so a signature reads as it would be written
pub fn render_modifiers(f: &FieldIR) -> String {
    let mut out = String::new();
    if f.seekable {
        out.push_str("seekable ");
    }
    if f.lazy {
        out.push_str("lazy ");
    }
    out
}

/// const fields carry a Rust type rather than a ResolvedTypeRef
pub fn rust_type_to_schema(rust_type: &str) -> &str {
    match rust_type {
        "&'static str" => "string",
        other => other,
    }
}

/// a literal as it would appear after `=`. non-primitive defaults have no single
/// literal form, so they render empty and callers omit the `=`.
pub fn render_default_value(value: &DefaultValue) -> String {
    match value {
        DefaultValue::Bool(b) => b.to_string(),
        DefaultValue::Int(i) => i.to_string(),
        DefaultValue::Float(f) => {
            if f.fract() == 0.0 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        DefaultValue::Str(s) => format!("\"{s}\""),
        _ => String::new(),
    }
}
