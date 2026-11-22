use super::writer::CodeWriter;
use super::{is_blastable, ordered_wire_fields, version_uses_intern, wire_fixed_size};
use crate::core::types::{ResolvedTypeRef, is_bulk_castable_scalar, normalize_type};
use crate::schema::ir::ir_types::{FieldId, ResolvedSchema};
use crate::schema::ir::lineage::{FieldLineage, FieldMapping, VersionLineage};

#[derive(Clone, Copy)]
enum Span {
    Fixed(usize),
    LengthPrefixed(usize),
    Varint,
}

struct FieldStep<'a> {
    field: &'a FieldLineage,
    presence_bit: Option<usize>,
    span: Span,
    array: bool,
}

struct Layout<'a> {
    fields: Vec<FieldStep<'a>>,
    interned: bool,
    presence_bytes: usize,
}

struct HybridVersion<'a> {
    version: i128,
    input: Layout<'a>,
    arrays: Vec<(FieldId, bool)>,
}

fn array_element(ty: &ResolvedTypeRef) -> Option<&str> {
    let ResolvedTypeRef::Array(inner) = ty else {
        return None;
    };
    match inner.as_ref() {
        ResolvedTypeRef::Scalar(id) if is_bulk_castable_scalar(&id.name) => {
            Some(normalize_type(&id.name))
        }
        _ => None,
    }
}

fn can_forward(source: &FieldStep<'_>, target: &FieldStep<'_>) -> bool {
    if !source.array || source.field.field_id != target.field.field_id {
        return false;
    }
    match &source.field.mapping {
        FieldMapping::PassThrough { .. } => true,
        FieldMapping::Cast { from, to, .. } if !target.field.lazy => {
            // Redeclaring an eager numeric array to mark it seekable can change
            // its TypeId. Its decoder still reads the same element type.
            let (from, to) = match (from, to) {
                (ResolvedTypeRef::Optional(from), ResolvedTypeRef::Optional(to)) => {
                    (from.as_ref(), to.as_ref())
                }
                _ => (from, to),
            };
            array_element(from).is_some_and(|element| Some(element) == array_element(to))
        }
        _ => false,
    }
}

fn span(
    schema: &ResolvedSchema,
    field: &FieldLineage,
    ty: &ResolvedTypeRef,
) -> Option<(Span, bool)> {
    if is_blastable(schema, field) {
        return Some((Span::Fixed(wire_fixed_size(ty)?), false));
    }
    match ty {
        ResolvedTypeRef::Scalar(id) => match normalize_type(&id.name) {
            "string" => Some((Span::LengthPrefixed(1), false)),
            "varint32" | "varint64" => Some((Span::Varint, false)),
            _ => None,
        },
        ResolvedTypeRef::Interned(_) => Some((Span::Varint, false)),
        ResolvedTypeRef::Array(inner) if array_element(ty).is_some() => {
            Some((Span::LengthPrefixed(wire_fixed_size(inner)?), true))
        }
        _ => None,
    }
}

impl<'a> Layout<'a> {
    fn new(schema: &ResolvedSchema, version: &'a VersionLineage) -> Self {
        let mut next_bit = 0;
        let mut fields = Vec::new();
        for field in ordered_wire_fields(schema, version) {
            let (ty, presence_bit) = if let ResolvedTypeRef::Optional(inner) = &field.source_ty {
                let bit = next_bit;
                next_bit += 1;
                (inner.as_ref(), Some(bit))
            } else {
                (&field.source_ty, None)
            };
            let Some((span, array)) = span(schema, field, ty) else {
                break;
            };
            fields.push(FieldStep {
                field,
                presence_bit,
                span,
                array,
            });
        }

        let optional_fields = version
            .fields
            .iter()
            .filter(|field| matches!(field.source_ty, ResolvedTypeRef::Optional(_)))
            .count();
        Self {
            fields,
            interned: version_uses_intern(schema, &version.fields),
            presence_bytes: optional_fields.div_ceil(8),
        }
    }

    fn emit(&self, arrays: &[(FieldId, bool)], w: &mut CodeWriter) {
        w.line(&format!(
            "HybridLayout {{ interned: {}, presence_bytes: {}, steps: &[",
            self.interned, self.presence_bytes
        ));
        w.indent();
        let last_array = self
            .fields
            .iter()
            .rposition(|step| arrays.iter().any(|(id, _)| *id == step.field.field_id))
            .unwrap();
        for step in &self.fields[..=last_array] {
            let layout = match step.span {
                Span::Fixed(bytes) => format!("StreamSpan::Fixed({bytes})"),
                Span::LengthPrefixed(width) => {
                    match arrays
                        .iter()
                        .enumerate()
                        .find(|(_, (id, _))| *id == step.field.field_id)
                    {
                        Some((index, (_, lazy))) => {
                            format!("StreamSpan::Array({index}, {width}, {lazy})")
                        }
                        None => format!("StreamSpan::LengthPrefixed({width})"),
                    }
                }
                Span::Varint => "StreamSpan::Varint".into(),
            };
            w.line(&format!("({:?}, {layout}),", step.presence_bit));
        }
        w.dedent();
        w.line("] },");
    }
}

// Own the layouts until all arms have been emitted.
pub(super) struct HybridPlans<'a> {
    reply: Layout<'a>,
    versions: Vec<HybridVersion<'a>>,
}

impl<'a> HybridPlans<'a> {
    pub(super) fn new(schema: &'a ResolvedSchema, trailing: &[(i128, usize)]) -> Self {
        let latest = schema.lineage.versions.last().unwrap();
        let reply = Layout::new(schema, latest);
        let mut versions = Vec::new();
        for version in &schema.lineage.versions {
            if trailing.iter().any(|&(id, _)| id == version.version) {
                continue;
            }
            let input = Layout::new(schema, version);
            let fields: Vec<_> = reply
                .fields
                .iter()
                .filter(|target| target.array)
                .filter_map(|target| {
                    input
                        .fields
                        .iter()
                        .find(|source| can_forward(source, target))?;
                    Some((target.field.field_id, target.field.lazy))
                })
                .collect();
            if !fields.is_empty() {
                versions.push(HybridVersion {
                    version: version.version,
                    input,
                    arrays: fields,
                });
            }
        }
        Self { reply, versions }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    pub(super) fn emit_arms(&self, w: &mut CodeWriter) {
        for plan in &self.versions {
            w.line(&format!(
                "{} if payload_len >= HYBRID_MIN_PAYLOAD_BYTES => {{",
                plan.version
            ));
            w.indent();
            w.line("hybrid.read_reply(input, output, payload_len, version, &mut response, &[");
            w.indent();
            plan.input.emit(&plan.arrays, w);
            self.reply.emit(&plan.arrays, w);
            w.dedent();
            w.line("])?;");
            w.line("None");
            w.dedent();
            w.line("},");
        }
    }
}
