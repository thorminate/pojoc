use crate::core::types::{ResolvedTypeRef, TypeId, WireSize, primitive_info};
use crate::schema::ir::ir_types::{ResolvedSchema, ResolvedType};
use std::collections::{BTreeMap, HashMap};

pub(super) fn fixed_payload_sizes(schema: &ResolvedSchema) -> BTreeMap<i128, u32> {
    let mut widths = FixedWidths {
        records: &schema.types.types,
        cached_records: HashMap::new(),
    };
    schema
        .lineage
        .versions
        .iter()
        .filter_map(|version| {
            let size = version.fields.iter().try_fold(0u32, |size, field| {
                if field.lazy {
                    return None;
                }
                size.checked_add(widths.of(&field.source_ty)?)
            })?;
            Some((version.version, size))
        })
        .collect()
}

pub(super) fn version_bytes(mut version: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    while version >= 128 {
        bytes.push((version as u8) | 0x80);
        version >>= 7;
    }
    bytes.push(version as u8);
    bytes
}

struct FixedWidths<'a> {
    records: &'a HashMap<TypeId, ResolvedType>,
    cached_records: HashMap<TypeId, Option<u32>>,
}

impl FixedWidths<'_> {
    fn of(&mut self, ty: &ResolvedTypeRef) -> Option<u32> {
        match ty {
            ResolvedTypeRef::Scalar(id) => {
                if let Some(primitive) = primitive_info(&id.name) {
                    return match primitive.wire_size {
                        WireSize::Fixed(width) => u32::try_from(width).ok(),
                        WireSize::Variable => None,
                    };
                }
                let record = self.records.get(id)?;
                if let Some(&width) = self.cached_records.get(id) {
                    return width;
                }
                self.cached_records.insert(id.clone(), None);
                let width = record.fields.iter().try_fold(0u32, |size, field| {
                    if field.lazy {
                        return None;
                    }
                    size.checked_add(self.of(&field.ty)?)
                });
                self.cached_records.insert(id.clone(), width);
                width
            }
            ResolvedTypeRef::Enum(_, width) => Some(u32::from(*width)),
            ResolvedTypeRef::Bitset(_, width) | ResolvedTypeRef::FixedString(width) => {
                u32::try_from(*width).ok()
            }
            ResolvedTypeRef::VFloat { backing, .. } => u32::try_from(backing.wire_size()).ok(),
            ResolvedTypeRef::FixedArray(item, len) => {
                self.of(item)?.checked_mul(u32::try_from(*len).ok()?)
            }
            ResolvedTypeRef::FixedMap(key, value, len) => self
                .of(key)?
                .checked_add(self.of(value)?)?
                .checked_mul(u32::try_from(*len).ok()?),
            ResolvedTypeRef::Tuple(items) => items
                .iter()
                .try_fold(0u32, |size, item| size.checked_add(self.of(item)?)),
            ResolvedTypeRef::Boxed(inner) | ResolvedTypeRef::Constrained { inner, .. } => {
                self.of(inner)
            }
            _ => None,
        }
    }
}
