use super::types::*;
use pojoc_core::types::*;

#[derive(Debug, Clone)]
pub enum FieldMapping {
    PassThrough {
        target_name: String,
    },
    Cast {
        target_name: String,
        from: ResolvedTypeRef,
        to: ResolvedTypeRef,
    },
    Discard,
}
#[derive(Debug, Clone)]
pub struct FieldLineage {
    pub field_id: FieldId,
    pub source_name: String,
    pub source_ty: ResolvedTypeRef,
    pub mapping: FieldMapping,
}

#[derive(Debug, Clone)]
pub struct MissingField {
    pub field_id: FieldId,
    pub target_name: String,
    pub ty: ResolvedTypeRef,
    pub default: Option<DefaultValue>,
}

#[derive(Debug)]
pub struct VersionLineage {
    pub version: i128,
    pub fields: Vec<FieldLineage>,
    pub missing: Vec<MissingField>,
}

#[derive(Debug)]
pub struct SchemaLineage {
    pub latest_version: i128,
    pub versions: Vec<VersionLineage>,
}

impl SchemaLineage {
    pub fn build(schema: &ResolvedSchema) -> Self {
        let latest = schema
            .versions
            .last()
            .expect("schema must have at least one version");

        let versions = schema
            .versions
            .iter()
            .map(|ver| build_version_lineage(ver, latest))
            .collect();

        SchemaLineage {
            latest_version: latest.version,
            versions,
        }
    }

    pub fn build_from(versions: &[ResolvedVersion]) -> Self {
        let latest = versions
            .last()
            .expect("schema must have at least one version");

        let version_lineages = versions
            .iter()
            .map(|ver| build_version_lineage(ver, latest))
            .collect();

        SchemaLineage {
            latest_version: latest.version,
            versions: version_lineages,
        }
    }
}

fn build_version_lineage(ver: &ResolvedVersion, latest: &ResolvedVersion) -> VersionLineage {
    let latest_by_id: std::collections::HashMap<FieldId, &FieldIR> =
        latest.fields.iter().map(|f| (f.id, f)).collect();

    let ver_ids: std::collections::HashSet<FieldId> = ver.fields.iter().map(|f| f.id).collect();

    let fields = ver
        .fields
        .iter()
        .map(|src| {
            let mapping = match latest_by_id.get(&src.id) {
                None => FieldMapping::Discard,

                Some(dst) => {
                    if src.ty == dst.ty {
                        FieldMapping::PassThrough {
                            target_name: dst.name.clone(),
                        }
                    } else {
                        FieldMapping::Cast {
                            target_name: dst.name.clone(),
                            from: src.ty.clone(),
                            to: dst.ty.clone(),
                        }
                    }
                }
            };

            FieldLineage {
                field_id: src.id,
                source_name: src.name.clone(),
                source_ty: src.ty.clone(),
                mapping,
            }
        })
        .collect();

    let missing = latest
        .fields
        .iter()
        .filter(|f| !ver_ids.contains(&f.id))
        .map(|f| MissingField {
            field_id: f.id,
            target_name: f.name.clone(),
            ty: f.ty.clone(),
            default: f.default.clone(),
        })
        .collect();

    VersionLineage {
        version: ver.version,
        fields,
        missing,
    }
}
