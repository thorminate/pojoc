use super::{scope_of, type_name};
use crate::core::types::{ResolvedTypeRef, primitive_info};
use crate::schema::ir::ir_types::ResolvedSchema;
use std::fmt::{self, Write};

use super::literals::PRINT_WIDTH;

pub(super) enum TypeExpr {
    Name(String),
    Generic(&'static str, Vec<TypeExpr>),
    Tuple(Vec<TypeExpr>),
    Nullable(Box<TypeExpr>),
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (open, items, close) = match self {
            Self::Name(name) => return f.write_str(name),
            Self::Nullable(inner) => return write!(f, "{inner} | null"),
            Self::Generic(name, items) => {
                write!(f, "{name}")?;
                ("<", items, ">")
            }
            Self::Tuple(items) => ("[", items, "]"),
        };
        f.write_str(open)?;
        for (index, item) in items.iter().enumerate() {
            if index != 0 {
                f.write_str(", ")?;
            }
            write!(f, "{item}")?;
        }
        f.write_str(close)
    }
}

impl TypeExpr {
    pub(super) fn emit(&self, out: &mut String, indent: usize, suffix: &str) {
        if let Self::Nullable(inner) = self {
            inner.emit(out, indent, &format!(" | null{suffix}"));
            return;
        }

        let inline = self.to_string();
        let column = out.rsplit('\n').next().unwrap_or_default().len();
        if column + inline.len() + suffix.len() > PRINT_WIDTH {
            let (open, items, close, trailing_comma) = match self {
                Self::Generic(name, items) if !matches!(items.as_slice(), [Self::Name(_)]) => {
                    (format!("{name}<"), items, ">", false)
                }
                Self::Tuple(items) if !items.is_empty() => ("[".into(), items, "]", true),
                _ => {
                    write!(out, "{inline}{suffix}").unwrap();
                    return;
                }
            };
            writeln!(out, "{open}").unwrap();
            let child = indent + 2;
            for (index, item) in items.iter().enumerate() {
                let separator = if trailing_comma || index + 1 < items.len() {
                    ","
                } else {
                    ""
                };
                write!(out, "{:child$}", "").unwrap();
                item.emit(out, child, separator);
                out.push('\n');
            }
            write!(out, "{:indent$}{close}{suffix}", "").unwrap();
        } else {
            write!(out, "{inline}{suffix}").unwrap();
        }
    }
}

pub(super) fn type_expr(
    ty: &ResolvedTypeRef,
    schemas: &[&ResolvedSchema],
    scope: usize,
) -> TypeExpr {
    use ResolvedTypeRef::*;
    use TypeExpr::{Generic, Name, Nullable};
    let child = |ty| type_expr(ty, schemas, scope);
    match ty {
        Scalar(id) => Name(primitive_info(&id.name).map_or_else(
            || type_name(scope, &id.name),
            |info| info.typescript_type.to_owned(),
        )),
        Enum(id, _) | Bitset(id, _) | Union(id) => Name(type_name(scope, &id.name)),
        ImportedSchema {
            alias, root_name, ..
        } => Name(type_name(
            scope_of(schemas, &schemas[scope].imports[alias]),
            root_name,
        )),
        Array(inner) | FixedArray(inner, _) | DeltaArray(inner) | FixedDeltaArray(inner, _) => {
            let packed = match inner.as_ref() {
                Scalar(id) => primitive_info(&id.name).and_then(|info| info.typescript_array),
                _ => None,
            };
            packed.map_or_else(
                || Generic("Array", vec![child(inner)]),
                |name| Name(name.into()),
            )
        }
        Map(key, value) => Generic("Map", vec![child(key), child(value)]),
        FixedMap(key, value, _) => Generic(
            "Array",
            vec![TypeExpr::Tuple(vec![child(key), child(value)])],
        ),
        Tuple(items) => TypeExpr::Tuple(items.iter().map(child).collect()),
        Optional(inner) => Nullable(Box::new(child(inner))),
        Boxed(inner) | Constrained { inner, .. } => child(inner),
        Interned(_) => Name("string".into()),
        FixedString(_) => Name("Uint8Array".into()),
        VFloat { .. } => Name("number".into()),
    }
}
