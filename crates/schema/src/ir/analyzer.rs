use super::id_gen::*;
use super::lineage::SchemaLineage;
use super::resolver::*;
use super::types::*;
use crate::ast::*;
use crate::error::AnalysisError;
use pojoc_core::types::*;

#[derive(Debug)]
pub struct SchemaAnalyzer<'a> {
    ast: &'a SchemaAst,
    resolver: Resolver<'a>,
    type_registry: TypeRegistry,
    enum_registry: EnumRegistry,
    bitset_registry: BitsetRegistry,
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
            enum_registry: EnumRegistry::default(),
            bitset_registry: BitsetRegistry::default(),
            version_states: Vec::new(),
            id_gen: IdGen::new(),
            current: None,
        }
    }

    pub fn run(&mut self) -> Result<(), AnalysisError> {
        self.collect_enums()?;
        self.collect_types()?;
        self.collect_bitsets()?;
        for version in &self.ast.versions {
            self.process_version(version)?;
        }
        Ok(())
    }

    fn collect_types(&mut self) -> Result<(), AnalysisError> {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::TypeDef(td) = block {
                    let (fields, const_fields) = match &td.extends {
                        None => self.collect_full_type(td, version.version)?,
                        Some(extends_ast) => {
                            self.collect_extended_type(td, extends_ast, version.version)?
                        }
                    };

                    let id = TypeId {
                        name: td.name.clone(),
                        version: version.version,
                    };

                    self.type_registry.types.insert(id, ResolvedType { fields, const_fields });
                }
            }
        }
        Ok(())
    }

    fn collect_enums(&mut self) -> Result<(), AnalysisError> {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::EnumDef(ed) = block {
                    let resolved = match ed {
                        EnumDefAst::Definition { variants, .. } => {
                            let variants = variants
                                .iter()
                                .enumerate()
                                .map(|(i, name)| EnumVariant {
                                    name: name.clone(),
                                    wire_value: i as u32,
                                })
                                .collect();
                            ResolvedEnum { variants }
                        }

                        EnumDefAst::Extension { name, base, ops } => {
                            if base.version >= version.version {
                                return Err(AnalysisError::UnknownParentType {
                                    child: name.clone(),
                                    parent: format!("{}@{}", base.name, base.version),
                                    version: version.version,
                                });
                            }

                            let parent = self
                                .enum_registry
                                .enums
                                .get(&TypeId {
                                    name: base.name.clone(),
                                    version: base.version,
                                })
                                .ok_or_else(|| AnalysisError::UnknownParentType {
                                    child: name.clone(),
                                    parent: format!("{}@{}", base.name, base.version),
                                    version: version.version,
                                })?;

                            let mut variants = parent.variants.clone();

                            for op in ops {
                                match op {
                                    EnumVariantOpAst::Rename { from, to } => {
                                        let v = variants
                                            .iter_mut()
                                            .find(|v| v.name == *from)
                                            .ok_or_else(|| AnalysisError::FieldNotFound {
                                                op: "rename",
                                                field: from.clone(),
                                                type_name: name.clone(),
                                                version: version.version,
                                            })?;
                                        v.name = to.clone();
                                    }
                                    EnumVariantOpAst::Add(variant_name) => {
                                        let wire_value = variants
                                            .iter()
                                            .map(|v| v.wire_value)
                                            .max()
                                            .map(|m| m + 1)
                                            .unwrap_or(0);
                                        variants.push(EnumVariant {
                                            name: variant_name.clone(),
                                            wire_value,
                                        });
                                    }
                                }
                            }

                            ResolvedEnum { variants }
                        }
                    };

                    let id = TypeId {
                        name: ed.name().to_string(),
                        version: version.version,
                    };
                    self.enum_registry.enums.insert(id, resolved);
                }
            }
        }
        Ok(())
    }

    fn collect_bitsets(&mut self) -> Result<(), AnalysisError> {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::BitsetDef(bd) = block {
                    let resolved = match bd {
                        BitsetDefAst::Definition { variants, .. } => ResolvedBitset {
                            variants: variants.clone(),
                        },
                        BitsetDefAst::Extension { name, base, ops } => {
                            if base.version >= version.version {
                                return Err(AnalysisError::UnknownParentType {
                                    child: name.clone(),
                                    parent: format!("{}@{}", base.name, base.version),
                                    version: version.version,
                                });
                            }

                            let parent = self
                                .bitset_registry
                                .bitsets
                                .get(&TypeId {
                                    name: base.name.clone(),
                                    version: base.version,
                                })
                                .ok_or_else(|| AnalysisError::UnknownParentType {
                                    child: name.clone(),
                                    parent: format!("{}@{}", base.name, base.version),
                                    version: version.version,
                                })?;

                            let mut variants = parent.variants.clone();

                            for op in ops {
                                match op {
                                    BitsetOpAst::Add(v_name) => {
                                        variants.push(v_name.clone());
                                    }
                                    BitsetOpAst::Remove(v_name) => {
                                        if let Some(idx) = variants.iter().position(|v| v == v_name)
                                        {
                                            variants[idx] = format!("__deprecated_{}", v_name);
                                        } else {
                                            return Err(AnalysisError::FieldNotFound {
                                                op: "remove",
                                                field: v_name.clone(),
                                                type_name: name.clone(),
                                                version: version.version,
                                            });
                                        }
                                    }
                                }
                            }

                            ResolvedBitset { variants }
                        }
                    };

                    let id = TypeId {
                        name: bd.name().to_string(),
                        version: version.version,
                    };
                    self.bitset_registry.bitsets.insert(id, resolved);
                }
            }
        }
        Ok(())
    }

    fn collect_full_type(
        &mut self,
        td: &TypeDefAst,
        version: i128,
    ) -> Result<(Vec<FieldIR>, Vec<ResolvedConst>), AnalysisError> {
        let fields_ast = match &td.body {
            TypeBody::Fields(f) => f,
            TypeBody::Diff(_) => {
                return Err(AnalysisError::ExtendsWithFullDefinition {
                    name: td.name.clone(),
                    version,
                });
            }
        };

        let const_fields = fields_ast
            .const_fields
            .iter()
            .map(|cf| resolve_const(cf, version))
            .collect::<Result<Vec<_>, _>>()?;

        let fields = fields_ast
            .fields
            .iter()
            .map(|f| {
                let ty = self.resolve(&f.ty, version).expect("type must be resolved");
                let default = coerce_default(
                    f.default.as_ref().map(DefaultValue::from),
                    &ty,
                    &f.name,
                    version,
                    &self.bitset_registry,
                )?;
                Ok(FieldIR {
                    id: self.id_gen.next(),
                    name: f.name.clone(),
                    ty,
                    default,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((fields, const_fields))
    }

    fn collect_extended_type(
        &mut self,
        td: &TypeDefAst,
        extends: &ExtendsAst,
        version: i128,
    ) -> Result<(Vec<FieldIR>, Vec<ResolvedConst>), AnalysisError> {
        let ops = match &td.body {
            TypeBody::Diff(ops) => ops,
            TypeBody::Fields(_) => {
                return Err(AnalysisError::ExtendsWithFullDefinition {
                    name: td.name.clone(),
                    version,
                });
            }
        };

        if extends.version >= version {
            return Err(AnalysisError::UnknownParentType {
                child: td.name.clone(),
                parent: format!("{}@{}", extends.name, extends.version),
                version,
            });
        }

        let parent = self
            .type_registry
            .types
            .get(&TypeId {
                name: extends.name.clone(),
                version: extends.version,
            })
            .ok_or_else(|| AnalysisError::UnknownParentType {
                child: td.name.clone(),
                parent: format!("{}@{}", extends.name, extends.version),
                version,
            })?;

        let mut fields: Vec<FieldIR> = parent.fields.clone();
        let mut consts: Vec<ResolvedConst> = parent.const_fields.clone();

        for op in ops {
            match op {
                DiffAst::Add { field } => {
                    let ty = self.resolve(&field.ty, version)?;

                    let default = match &ty {
                        ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) => {
                            Some(DefaultValue::Struct)
                        }
                        ResolvedTypeRef::Optional(_) if field.default.is_none() => {
                            Some(DefaultValue::None)
                        }
                        _ => {
                            let raw = field.default.as_ref().map(DefaultValue::from).ok_or_else(
                                || AnalysisError::MissingDefault {
                                    field: field.name.clone(),
                                    version,
                                },
                            )?;
                            coerce_default(
                                Some(raw),
                                &ty,
                                &field.name,
                                version,
                                &self.bitset_registry,
                            )?
                        }
                    };

                    fields.push(FieldIR {
                        id: self.id_gen.next(),
                        name: field.name.clone(),
                        ty,
                        default,
                    });
                }

                DiffAst::AddConst { field } => {
                    if consts.iter().any(|c| c.name == field.name) || fields.iter().any(|f| f.name == field.name) {
                        return Err(AnalysisError::FieldAlreadyExists(
                            version,
                            field.name.clone(),
                        ));
                    }

                    consts.push(resolve_const(field, version)?);
                }

                DiffAst::Remove { name } => {
                    let existed_in_fields = fields.iter().any(|f| f.name == *name);
                    let existed_in_consts = consts.iter().any(|c| c.name == *name);

                    if !existed_in_fields && !existed_in_consts {
                        return Err(AnalysisError::FieldNotFound {
                            op: "remove",
                            field: name.clone(),
                            type_name: td.name.clone(),
                            version,
                        });
                    }
                    fields.retain(|f| f.name != *name);
                    consts.retain(|c| c.name != *name);
                }

                DiffAst::Rename { from, to } => {
                    if let Some(f) = fields.iter_mut().find(|f| f.name == *from) {
                        f.name = to.clone();
                    } else if let Some(c) = consts.iter_mut().find(|c| c.name == *from) {
                        c.name = to.clone();
                    } else {
                        return Err(AnalysisError::FieldNotFound {
                            op: "rename",
                            field: from.clone(),
                            type_name: td.name.clone(),
                            version,
                        });
                    }
                }

                DiffAst::UpdateType { name, ty } => {
                    if let Some(f) = fields.iter_mut().find(|f| f.name == *name) {
                        let new_ty = self.resolve(ty, version)?;
                        check_type_update(&f.ty, &new_ty, version)?;
                        f.ty = new_ty;
                    } else if consts.iter().any(|c| c.name == *name) {
                    } else {
                        return Err(AnalysisError::FieldNotFound {
                            op: "update type",
                            field: name.clone(),
                            type_name: td.name.clone(),
                            version,
                        });
                    }
                }

                DiffAst::Transform { from, to, ty } => {
                    if let Some(f) = fields.iter_mut().find(|f| f.name == *from) {
                        f.name = to.clone();
                        if let Some(ty) = ty {
                            f.ty = self.resolve(ty, version).expect("type must be resolved");
                        }
                    } else if let Some(c) = consts.iter_mut().find(|c| c.name == *from) {
                        c.name = to.clone();
                    } else {
                        return Err(AnalysisError::FieldNotFound {
                            op: "transform",
                            field: from.clone(),
                            type_name: td.name.clone(),
                            version,
                        });
                    }
                }
            }
        }

        Ok((fields, consts))
    }

    fn process_version(&mut self, v: &VersionAst) -> Result<(), AnalysisError> {
        let mut ctx = self.current.take().unwrap_or(VersionContext {
            version: v.version,
            fields: Default::default(),
            const_fields: Default::default(),
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

                VersionBlockAst::EnumDef(_) => {
                    // handled in collect_enums()
                    continue;
                }

                VersionBlockAst::BitsetDef(_) => {
                    // handled in collect_bitsets()
                    continue;
                }
            }
        }

        let snapshot = self.snapshot(&ctx, v.version);
        self.version_states.push(snapshot);

        self.current = Some(ctx);

        Ok(())
    }

    fn handle_fields(
        &mut self,
        f: &FieldsAst,
        version: i128,
        ctx: &mut VersionContext,
    ) -> Result<(), AnalysisError> {
        for field in &f.fields {
            let ty = self.resolve(&field.ty, version)?;
            let default = coerce_default(
                field.default.as_ref().map(DefaultValue::from),
                &ty,
                &field.name,
                version,
                &self.bitset_registry,
            )?;
            ctx.fields.push(FieldIR {
                id: self.id_gen.next(),
                name: field.name.clone(),
                ty,
                default,
            });
        }
        for cf in &f.const_fields {
            ctx.const_fields.push(resolve_const(cf, version)?);
        }
        Ok(())
    }

    fn handle_diff(
        &mut self,
        diff: &[DiffAst],
        version: i128,
        ctx: &mut VersionContext,
    ) -> Result<(), AnalysisError> {
        for op in diff {
            match op {
                DiffAst::Add { field } => {
                    if ctx.fields.iter().any(|f| f.name == field.name) || ctx.const_fields.iter().any(|c| c.name == field.name) {
                        return Err(AnalysisError::FieldAlreadyExists(
                            version,
                            field.name.clone(),
                        ));
                    }

                    let ty = self.resolve(&field.ty, version)?;

                    let default = match &ty {
                        ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) => {
                            Some(DefaultValue::Struct)
                        }
                        ResolvedTypeRef::Optional(_) if field.default.is_none() => {
                            Some(DefaultValue::None)
                        }
                        _ => {
                            let raw = field.default.as_ref().map(DefaultValue::from).ok_or_else(
                                || AnalysisError::MissingDefault {
                                    field: field.name.clone(),
                                    version,
                                },
                            )?;
                            coerce_default(
                                Some(raw),
                                &ty,
                                &field.name,
                                version,
                                &self.bitset_registry,
                            )?
                        }
                    };

                    ctx.fields.push(FieldIR {
                        id: self.id_gen.next(),
                        name: field.name.clone(),
                        ty,
                        default,
                    });
                }

                // Fixed variant to match your exact ResolvedConst definition
                DiffAst::AddConst { field } => {
                    if ctx.const_fields.iter().any(|c| c.name == field.name) || ctx.fields.iter().any(|f| f.name == field.name) {
                        return Err(AnalysisError::FieldAlreadyExists(
                            version,
                            field.name.clone(),
                        ));
                    }

                    ctx.const_fields.push(resolve_const(field, version)?);
                }

                DiffAst::Remove { name } => {
                    ctx.fields.retain(|f| f.name != *name);
                    ctx.const_fields.retain(|c| c.name != *name);
                }

                DiffAst::Rename { from, to } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *from) {
                        f.name = to.clone();
                    } else if let Some(c) = ctx.const_fields.iter_mut().find(|c| c.name == *from) {
                        c.name = to.clone();
                    }
                }

                DiffAst::UpdateType { name, ty } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *name) {
                        let new_ty = self.resolve(ty, version)?;
                        check_type_update(&f.ty, &new_ty, version)?;
                        f.ty = new_ty;
                    }
                }

                DiffAst::Transform { from, to, ty } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *from) {
                        f.name = to.clone();
                        if let Some(ty) = ty {
                            f.ty = self.resolve(ty, version).expect("type must be resolved");
                        }
                    } else if let Some(c) = ctx.const_fields.iter_mut().find(|c| c.name == *from) {
                        c.name = to.clone();
                    }
                }
            }
        }

        Ok(())
    }

    fn resolve(&self, ty: &TypeAst, version: i128) -> Result<ResolvedTypeRef, AnalysisError> {
        match ty {
            TypeAst::Named(name) => {
                if is_primitive(name) {
                    return Ok(ResolvedTypeRef::Scalar(TypeId {
                        name: name.clone(),
                        version,
                    }));
                }

                if let Some((id, bs)) = self
                    .bitset_registry
                    .bitsets
                    .iter()
                    .filter(|(id, _)| id.name == *name && id.version <= version)
                    .max_by_key(|(id, _)| id.version)
                {
                    return Ok(ResolvedTypeRef::Bitset(id.clone(), bs.byte_width()));
                }

                if let Some((id, _)) = self.enum_registry.latest_before(name, version + 1) {
                    return Ok(ResolvedTypeRef::Enum(id.clone()));
                }

                if let Some(type_id) = self.resolver.resolve_type(name, version) {
                    return Ok(ResolvedTypeRef::Scalar(type_id));
                }

                Err(AnalysisError::UnknownType {
                    name: name.clone(),
                    version,
                })
            }
            TypeAst::Array(inner) => {
                let inner_ref = self.resolve(inner, version)?;
                Ok(ResolvedTypeRef::Array(Box::new(inner_ref)))
            }
            TypeAst::FixedArray(inner, n) => {
                if *n > 256 {
                    return Err(AnalysisError::FixedSizeTooLarge {
                        kind: "array",
                        n: *n,
                        version,
                    });
                }
                let inner_ref = self.resolve(inner, version)?;
                Ok(ResolvedTypeRef::FixedArray(Box::new(inner_ref), *n))
            }
            TypeAst::DeltaArray(inner) => {
                let inner_ref = self.resolve(inner, version)?;
                if !is_delta_eligible(&inner_ref) {
                    return Err(AnalysisError::InvalidDeltaElementType {
                        type_desc: format!("{:?}", inner_ref),
                        version,
                    });
                }
                Ok(ResolvedTypeRef::DeltaArray(Box::new(inner_ref)))
            }
            TypeAst::FixedDeltaArray(inner, n) => {
                if *n > 256 {
                    return Err(AnalysisError::FixedSizeTooLarge {
                        kind: "array",
                        n: *n,
                        version,
                    });
                }
                let inner_ref = self.resolve(inner, version)?;
                if !is_delta_eligible(&inner_ref) {
                    return Err(AnalysisError::InvalidDeltaElementType {
                        type_desc: format!("{:?}", inner_ref),
                        version,
                    });
                }
                Ok(ResolvedTypeRef::FixedDeltaArray(Box::new(inner_ref), *n))
            }
            TypeAst::FixedString(n) => Ok(ResolvedTypeRef::FixedString(*n)),
            TypeAst::Map(k, v) => Ok(ResolvedTypeRef::Map(
                Box::new(self.resolve(k, version)?),
                Box::new(self.resolve(v, version)?),
            )),
            TypeAst::FixedMap(k, v, n) => {
                if *n > 1024 {
                    return Err(AnalysisError::FixedSizeTooLarge {
                        kind: "map",
                        n: *n,
                        version,
                    });
                }
                Ok(ResolvedTypeRef::FixedMap(
                    Box::new(self.resolve(k, version)?),
                    Box::new(self.resolve(v, version)?),
                    *n,
                ))
            }
            TypeAst::Tuple(elements) => {
                let resolved = elements
                    .iter()
                    .map(|t| self.resolve(t, version))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(ResolvedTypeRef::Tuple(resolved))
            }
            TypeAst::VFloat { min, max, step } => {
                if !step.is_finite() || *step <= 0.0 {
                    return Err(AnalysisError::InvalidVFloat {
                        reason: "step must be > 0".into(),
                        version,
                    });
                }
                if !min.is_finite() || !max.is_finite() || max <= min {
                    return Err(AnalysisError::InvalidVFloat {
                        reason: "max must be finite and greater than min".into(),
                        version,
                    });
                }

                let span = (max - min) / step;

                let backing = if span <= u16::MAX as f64 {
                    VFloatBacking::U16
                } else if span <= u32::MAX as f64 {
                    VFloatBacking::U32
                } else {
                    return Err(AnalysisError::VFloatRangeTooLarge { span, version });
                };

                Ok(ResolvedTypeRef::VFloat {
                    min: *min,
                    max: *max,
                    step: *step,
                    backing,
                })
            }
            TypeAst::Optional(v) => {
                let inner_ref = self.resolve(v, version)?;
                Ok(ResolvedTypeRef::Optional(Box::new(inner_ref)))
            }
        }
    }


    fn snapshot(&self, ctx: &VersionContext, version: i128) -> ResolvedVersion {
        ResolvedVersion {
            version,
            fields: ctx.fields.clone(),
            const_fields: ctx.const_fields.clone(),
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
            enums: self.enum_registry,
            bitsets: self.bitset_registry,
            lineage,
        })
    }
}

fn coerce_default(
    default: Option<DefaultValue>,
    ty: &ResolvedTypeRef,
    field_name: &str,
    version: i128,
    bitset_registry: &BitsetRegistry, // <-- Pass the registry reference here
) -> Result<Option<DefaultValue>, AnalysisError> {
    match ty {
        ResolvedTypeRef::FixedString(n) => match default {
            Some(DefaultValue::Str(s)) => {
                let bytes = s.into_bytes();
                if bytes.len() != *n {
                    return Err(AnalysisError::FixedStringDefaultLengthMismatch {
                        field: field_name.to_string(),
                        expected: *n,
                        got: bytes.len(),
                        version,
                    });
                }
                Ok(Some(DefaultValue::FixedBytes(bytes)))
            }
            other => Ok(other),
        },
        ResolvedTypeRef::Bitset(type_id, _) => match default {
            Some(DefaultValue::BitsetLiteral {
                ref ty_name,
                ref kvs,
            }) => {
                if ty_name != &type_id.name {
                    return Err(AnalysisError::TypeMismatch {
                        expected: type_id.name.clone(),
                        got: ty_name.clone(),
                        version,
                    });
                }

                let bitset_def = bitset_registry.bitsets.get(type_id).ok_or_else(|| {
                    AnalysisError::UnknownType {
                        name: type_id.name.clone(),
                        version,
                    }
                })?;

                for (flag_name, _) in kvs {
                    if !bitset_def.variants.contains(flag_name) {
                        return Err(AnalysisError::FieldNotFound {
                            op: "default assignment",
                            field: flag_name.clone(),
                            type_name: type_id.name.clone(),
                            version,
                        });
                    }
                }
                Ok(Some(DefaultValue::BitsetLiteral {
                    ty_name: ty_name.clone(),
                    kvs: kvs.clone(),
                }))
            }
            Some(DefaultValue::Int(0)) => Ok(Some(DefaultValue::BitsetLiteral {
                ty_name: type_id.name.clone(),
                kvs: vec![],
            })),
            Some(DefaultValue::Int(n)) => Err(AnalysisError::TypeMismatch {
                expected: type_id.name.clone(),
                got: format!("{n}"),
                version,
            }),
            other => Ok(other),
        },
        ResolvedTypeRef::VFloat { min, max, .. } => {
            let as_f64 = match &default {
                Some(DefaultValue::Float(f)) => Some(*f),
                Some(DefaultValue::Int(i)) => Some(*i as f64),
                None => None,
                Some(other) => {
                    return Err(AnalysisError::TypeMismatch {
                        expected: "vfloat (number)".into(),
                        got: format!("{:?}", other),
                        version,
                    })
                }
            };

            match as_f64 {
                Some(f) if f < *min || f > *max => {
                    Err(AnalysisError::VFloatDefaultOutOfRange {
                        field: field_name.to_string(),
                        value: f,
                        min: *min,
                        max: *max,
                        version,
                    })
                }
                Some(f) => Ok(Some(DefaultValue::Float(f))),
                None => Ok(None),
            }
        }
        _ => Ok(default),
    }
}

fn resolve_const(
    field: &ConstFieldAst,
    version: i128,
) -> Result<ResolvedConst, AnalysisError> {
    let rust_type = match &field.ty {
        TypeAst::Named(n) => match normalize_type(n) {
            "u8"  => "u8",  "u16" => "u16", "u32" => "u32", "u64" => "u64",
            "i8"  => "i8",  "i16" => "i16", "i32" => "i32", "i64" => "i64",
            "f32" => "f32", "f64" => "f64", "bool" => "bool",
            "string" => "&'static str",
            "varint32" | "varint64" => return Err(AnalysisError::VarintsCannotBeConst {version}),
            other => return Err(AnalysisError::TypeMismatch {
                expected: "primitive type".into(),
                got: other.to_string(),
                version,
            }),
        },
        _ => return Err(AnalysisError::TypeMismatch {
            expected: "primitive type".into(),
            got: format!("{:?}", field.ty),
            version,
        }),
    };

    let value = DefaultValue::from(&field.value);
    match (&value, rust_type) {
        (DefaultValue::Bool(_), "bool") => {}
        (DefaultValue::Int(_), "u8"|"u16"|"u32"|"u64"|"i8"|"i16"|"i32"|"i64") => {}
        (DefaultValue::Float(_), "f32"|"f64") => {}
        (DefaultValue::Str(_), "&'static str") => {}
        _ => return Err(AnalysisError::TypeMismatch {
            expected: rust_type.into(),
            got: format!("{:?}", value),
            version,
        }),
    }

    Ok(ResolvedConst {
        name: field.name.clone(),
        rust_type,
        value,
    })
}

fn check_type_update(
    _old_ty: &ResolvedTypeRef,
    _new_ty: &ResolvedTypeRef,
    _version: i128,
) -> Result<(), AnalysisError> {
    // This will be used to facilitate the validity of a type cast, TODO.
    Ok(())
}