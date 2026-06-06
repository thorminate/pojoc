use crate::ast::*;
use crate::error::AnalysisError;
use super::lineage::SchemaLineage;
use super::id_gen::*;
use super::types::*;
use super::resolver::*;

#[derive(Debug)]
pub struct SchemaAnalyzer<'a> {
    ast: &'a SchemaAst,

    resolver: Resolver<'a>,

    type_registry: TypeRegistry,

    version_states: Vec<ResolvedVersion>,

    current: Option<VersionContext>,

    id_gen: IdGen,
}

impl<'a> SchemaAnalyzer<'a> {
    pub fn new(ast: &'a SchemaAst) -> Self {
        Self {
            resolver: Resolver { ast },
            ast,
            type_registry: TypeRegistry::default(),
            version_states: Vec::new(),
            id_gen: IdGen::new(),
            current: None,
        }
    }

    pub fn run(&mut self) -> Result<(), AnalysisError> {
        self.collect_types()?;

        for version in &self.ast.versions {
            self.process_version(version)?;
        }

        Ok(())
    }

    fn collect_types(&mut self) -> Result<(), AnalysisError> {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::TypeDef(td) = block {
                    let fields = match &td.extends {
                        None => self.collect_full_type(td, version.version)?,
                        Some(parent_name) => self.collect_extended_type(td, parent_name, version.version)?,
                    };

                    let id = TypeId {
                        name: td.name.clone(),
                        version: version.version,
                    };

                    self.type_registry.types.insert(id, ResolvedType { fields });
                }
            }
        }

        Ok(())
    }

    fn collect_full_type(
        &mut self,
        td: &TypeDefAst,
        version: u32,
    ) -> Result<Vec<FieldIR>, AnalysisError> {
        let fields = match &td.body {
            TypeBody::Fields(fields) => fields,
            TypeBody::Diff(_) => {
                return Err(AnalysisError::ExtendsWithFullDefinition {
                    name: td.name.clone(),
                    version,
                });
            }
        };

        Ok(fields.iter().map(|f| FieldIR {
            id: self.id_gen.next(),
            name: f.name.clone(),
            ty: self.resolve(&f.ty, version).expect("type must be resolved"),
            default: f.default.as_ref().map(DefaultValue::from),
        }).collect())
    }

    fn collect_extended_type(
        &mut self,
        td: &TypeDefAst,
        parent_name: &str,
        version: u32,
    ) -> Result<Vec<FieldIR>, AnalysisError> {
        // Body must be a diff when extends is present
        let ops = match &td.body {
            TypeBody::Diff(ops) => ops,
            TypeBody::Fields(_) => {
                return Err(AnalysisError::ExtendsWithFullDefinition {
                    name: td.name.clone(),
                    version,
                });
            }
        };

        // Look up parent — must exist in a prior version
        let parent = self.type_registry
            .latest_before(parent_name, version)
            .ok_or_else(|| AnalysisError::UnknownParentType {
                child: td.name.clone(),
                parent: parent_name.to_string(),
                version,
            })?;

        // Clone parent fields, preserving IDs
        let mut fields: Vec<FieldIR> = parent.fields.clone();

        // Apply diff ops — same logic as handle_diff but scoped to type fields
        for op in ops {
            match op {
                DiffAst::Add { field } => {
                    let ty = self.resolve(&field.ty, version)?;

                    let default = match &ty {
                        // structs always get Default::default(), no schema default needed
                        ResolvedTypeRef::Scalar(id) if !self.resolver.is_primitive(&id.name) => {
                            Some(DefaultValue::Struct)
                        }
                        // primitives and arrays must have an explicit default
                        _ => {
                            Some(field.default.as_ref()
                                .map(DefaultValue::from)
                                .ok_or_else(|| AnalysisError::MissingDefault {
                                    field: field.name.clone(),
                                    version,
                                })?)
                        }
                    };

                    fields.push(FieldIR {
                        id: self.id_gen.next(),
                        name: field.name.clone(),
                        ty,
                        default,
                    });
                }

                DiffAst::Remove { name } => {
                    let existed = fields.iter().any(|f| f.name == *name);
                    if !existed {
                        return Err(AnalysisError::FieldNotFound {
                            op: "remove",
                            field: name.clone(),
                            type_name: td.name.clone(),
                            version,
                        });
                    }
                    fields.retain(|f| f.name != *name);
                }

                DiffAst::Rename { from, to } => {
                    let f = fields.iter_mut().find(|f| f.name == *from)
                        .ok_or_else(|| AnalysisError::FieldNotFound {
                            op: "rename",
                            field: from.clone(),
                            type_name: td.name.clone(),
                            version,
                        })?;
                    f.name = to.clone();
                }

                DiffAst::UpdateType { name, ty } => {
                    let f = fields.iter_mut().find(|f| f.name == *name)
                        .ok_or_else(|| AnalysisError::FieldNotFound {
                            op: "update type",
                            field: name.clone(),
                            type_name: td.name.clone(),
                            version,
                        })?;
                    f.ty = self.resolve(ty, version).expect("type must be resolved");
                }

                DiffAst::Transform { from, to, ty } => {
                    let f = fields.iter_mut().find(|f| f.name == *from)
                        .ok_or_else(|| AnalysisError::FieldNotFound {
                            op: "transform",
                            field: from.clone(),
                            type_name: td.name.clone(),
                            version,
                        })?;
                    f.name = to.clone();
                    if let Some(ty) = ty {
                        f.ty = self.resolve(ty, version).expect("type must be resolved");
                    }
                }
            }
        }

        Ok(fields)
    }

    fn process_version(&mut self, v: &VersionAst) -> Result<(), AnalysisError> {
        let mut ctx = self.current.take().unwrap_or(VersionContext {
            version: v.version,
            fields: Default::default(),
        });
        ctx.version = v.version;

        for block in &v.blocks {
            match block {
                VersionBlockAst::Fields(f) => {
                    self.handle_fields(f, v.version, &mut ctx)?;
                }

                VersionBlockAst::Diff(diff) => {
                    self.handle_diff(diff, v.version, &mut ctx)?;
                }

                VersionBlockAst::TypeDef(_) => {
                    // handled in collect_types()
                    continue;
                }
            }
        }

        let snapshot = self.snapshot(&ctx, v.version);
        self.version_states.push(snapshot);

        self.current = Some(ctx);

        Ok(())
    }

    fn handle_fields(&mut self, f: &FieldsAst, version: u32, ctx: &mut VersionContext) -> Result<(), AnalysisError> {
        for field in &f.fields {
            ctx.fields.push(FieldIR {
                id: self.id_gen.next(),
                name: field.name.clone(),
                ty: self.resolve(&field.ty, version)?,
                default: field.default.as_ref().map(DefaultValue::from),
            });
        }
        Ok(())
    }

    fn handle_diff(
        &mut self,
        diff: &[DiffAst],
        version: u32,
        ctx: &mut VersionContext,
    ) -> Result<(), AnalysisError> {
        for op in diff {
            match op {
                DiffAst::Add { field } => {
                    let ty = self.resolve(&field.ty, version)?;

                    let default = match &ty {
                        // structs always get Default::default(), no schema default needed
                        ResolvedTypeRef::Scalar(id) if !self.resolver.is_primitive(&id.name) => {
                            Some(DefaultValue::Struct)
                        }
                        // primitives and arrays must have an explicit default
                        _ => {
                            Some(field.default.as_ref()
                                .map(DefaultValue::from)
                                .ok_or_else(|| AnalysisError::MissingDefault {
                                    field: field.name.clone(),
                                    version,
                                })?)
                        }
                    };

                    ctx.fields.push(FieldIR {
                        id: self.id_gen.next(),
                        name: field.name.clone(),
                        ty,
                        default,
                    });
                }

                DiffAst::Remove { name } => {
                    ctx.fields.retain(|f| f.name != *name);
                }

                DiffAst::Rename { from, to } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *from) {
                        f.name = to.clone();
                    }
                }

                DiffAst::UpdateType { name, ty } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *name) {
                        f.ty = self.resolve(ty, version).expect("type must be resolved");
                    }
                }

                DiffAst::Transform { from, to, ty } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *from) {
                        f.name = to.clone();
                        if let Some(ty) = ty {
                            f.ty = self.resolve(ty, version).expect("type must be resolved");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn resolve(&self, ty: &TypeAst, version: u32) -> Result<ResolvedTypeRef, AnalysisError> {
        match ty {
            TypeAst::Named(name) => {
                let type_id = if self.resolver.is_primitive(name) {
                    TypeId { name: name.clone(), version }
                } else if let Some(type_id) = self.resolver.resolve_type(name, version) {
                    type_id
                } else {
                    return Err(AnalysisError::UnknownType {
                        name: name.clone(),
                        version,
                    });
                };
                Ok(ResolvedTypeRef::Scalar(type_id))
            }
            TypeAst::Array(inner) => {
                let inner_ref = self.resolve(inner, version)?;
                Ok(ResolvedTypeRef::Array(Box::new(inner_ref)))
            }
        }
    }
    fn snapshot(&self, ctx: &VersionContext, version: u32) -> ResolvedVersion {
        ResolvedVersion {
            version,
            fields: ctx.fields.clone(),
        }
    }

    pub fn finish(self) -> Result<ResolvedSchema, AnalysisError> {
        if self.version_states.is_empty() {
            return Err(AnalysisError::NoVersions);
        }

        let lineage = SchemaLineage::build_from(&self.version_states);

        Ok(ResolvedSchema {
            name_hint: self.ast.name.clone(),
            versions: self.version_states,
            types: self.type_registry,
            lineage,
        })
    }
}