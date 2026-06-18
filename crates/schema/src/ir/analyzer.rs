use super::id_gen::*;
use super::lineage::SchemaLineage;
use super::resolver::*;
use super::ir_types::*;
use crate::ast::*;
use crate::error::AnalysisError;
use crate::span::Span;
use pojoc_core::types::*;

#[derive(Debug)]
pub struct SchemaAnalyzer<'a> {
    ast: &'a SchemaAst,
    resolver: Resolver<'a>,
    type_registry: TypeRegistry,
    enum_registry: EnumRegistry,
    union_registry: UnionRegistry,
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
            union_registry: UnionRegistry::default(),
            bitset_registry: BitsetRegistry::default(),
            version_states: Vec::new(),
            id_gen: IdGen::new(),
            current: None,
        }
    }

    pub fn run(&mut self) -> Result<(), AnalysisError> {
        // we preregister unions so they can be referenced in types with no issues
        // enums and bitsets dont depend on any external types so they dont need any special handling.
        self.collect_enums()?;
        self.collect_bitsets()?;
        self.preregister_union_ids();
        self.collect_types()?;
        self.collect_unions()?;
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

                    let id = TypeId { name: td.name.clone(), version: version.version };
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
                            let mut resolved = vec![EnumVariant { name: "Unknown".into(), wire_value: 0 }];
                            for (i, variant_node) in variants.iter().enumerate() {
                                if variant_node.name == "Unknown" {
                                    return Err(AnalysisError::ReservedVariantName {
                                        name: "Unknown".into(),
                                        type_name: ed.name().to_string(),
                                        version: version.version,
                                        span: variant_node.span,
                                        line: variant_node.line,
                                    });
                                }
                                resolved.push(EnumVariant {
                                    name: variant_node.name.clone(),
                                    wire_value: (i + 1) as u32,
                                });
                            }
                            ResolvedEnum { variants: resolved }
                        }

                        EnumDefAst::Extension { name, base, ops, .. } => {
                            if base.version >= version.version {
                                return Err(AnalysisError::UnknownParentType {
                                    child: name.clone(),
                                    parent: format!("{}@{}", base.name, base.version),
                                    version: version.version,
                                    span: base.span,
                                    line: base.line,
                                });
                            }

                            let parent = self
                                .enum_registry
                                .enums
                                .get(&TypeId { name: base.name.clone(), version: base.version })
                                .ok_or_else(|| AnalysisError::UnknownParentType {
                                    child: name.clone(),
                                    parent: format!("{}@{}", base.name, base.version),
                                    version: version.version,
                                    span: base.span,
                                    line: base.line,
                                })?;

                            let mut variants = parent.variants.clone();

                            for op in ops {
                                match op {
                                    EnumVariantOpAst::Rename { from, to, span, line } => {
                                        let v = variants
                                            .iter_mut()
                                            .find(|v| v.name == *from)
                                            .ok_or_else(|| AnalysisError::FieldNotFound {
                                                op: "rename",
                                                field: from.clone(),
                                                type_name: name.clone(),
                                                version: version.version,
                                                span: *span,
                                                line: *line,
                                            })?;
                                        v.name = to.clone();
                                    }
                                    EnumVariantOpAst::Add { name: variant_name, .. } => {
                                        let wire_value = variants
                                            .iter()
                                            .map(|v| v.wire_value)
                                            .max()
                                            .map(|m| m + 1)
                                            .unwrap_or(0);
                                        variants.push(EnumVariant { name: variant_name.clone(), wire_value });
                                    }
                                }
                            }

                            ResolvedEnum { variants }
                        }
                    };

                    let id = TypeId { name: ed.name().to_string(), version: version.version };
                    self.enum_registry.enums.insert(id, resolved);
                }
            }
        }
        Ok(())
    }

    fn preregister_union_ids(&mut self) {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::UnionDef(ud) = block {
                    let id = TypeId { name: ud.name().to_string(), version: version.version };
                    // empty variants — collect_unions overwrites this with the real data
                    self.union_registry.unions.entry(id).or_insert(ResolvedUnion { variants: vec![] });
                }
            }
        }
    }

    fn collect_unions(&mut self) -> Result<(), AnalysisError> {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::UnionDef(ud) = block {
                    let resolved = match ud {
                        UnionDefAst::Definition { name: _, variants, .. } => ResolvedUnion {
                            variants: self.resolve_union_variants(variants, version.version)?,
                        },

                        UnionDefAst::Extension { name, base, ops, .. } => {
                            if base.version >= version.version {
                                return Err(AnalysisError::UnknownParentType {
                                    child: name.clone(),
                                    parent: format!("{}@{}", base.name, base.version),
                                    version: version.version,
                                    span: base.span,
                                    line: base.line,
                                });
                            }

                            let parent = self
                                .union_registry
                                .unions
                                .get(&TypeId { name: base.name.clone(), version: base.version })
                                .ok_or_else(|| AnalysisError::UnknownParentType {
                                    child: name.clone(),
                                    parent: format!("{}@{}", base.name, base.version),
                                    version: version.version,
                                    span: base.span,
                                    line: base.line,
                                })?;

                            let mut variants = parent.variants.clone();

                            for op in ops {
                                match op {
                                    UnionVariantOpAst::Add { name: vname, payload_ty, span, line } => {
                                        if variants.iter().any(|v| &v.name == vname) {
                                            return Err(AnalysisError::FieldAlreadyExists {
                                                version: version.version,
                                                field: vname.clone(),
                                                span: *span,
                                                line: *line,
                                            });
                                        }

                                        let payload_id = self
                                            .resolver
                                            .resolve_type(payload_ty, version.version)
                                            .ok_or_else(|| AnalysisError::UnknownType {
                                                name: payload_ty.clone(),
                                                version: version.version,
                                                span: *span,
                                                line: *line,
                                            })?;

                                        let discriminant = variants.iter().map(|v| v.discriminant).max().map_or(0, |m| m + 1);

                                        variants.push(UnionVariant { name: vname.clone(), payload: payload_id, discriminant });
                                    }
                                }
                            }

                            ResolvedUnion { variants }
                        }
                    };

                    let id = TypeId { name: ud.name().to_string(), version: version.version };
                    self.union_registry.unions.insert(id, resolved);
                }
            }
        }
        Ok(())
    }

    fn resolve_union_variants(
        &self,
        variants: &[UnionVariantAst],
        version: i128,
    ) -> Result<Vec<UnionVariant>, AnalysisError> {
        variants
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let payload = self.resolver.resolve_type(&v.payload_ty, version).ok_or_else(|| {
                    AnalysisError::UnknownType {
                        name: v.payload_ty.clone(),
                        version,
                        span: v.span,
                        line: v.line,
                    }
                })?;
                Ok(UnionVariant { name: v.name.clone(), payload, discriminant: i as u64 })
            })
            .collect()
    }

    fn collect_bitsets(&mut self) -> Result<(), AnalysisError> {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::BitsetDef(bd) = block {
                    let resolved = match bd {
                        BitsetDefAst::Definition { variants, .. } => ResolvedBitset { variants: variants.clone() },
                        BitsetDefAst::Extension { name, base, ops, .. } => {
                            if base.version >= version.version {
                                return Err(AnalysisError::UnknownParentType {
                                    child: name.clone(),
                                    parent: format!("{}@{}", base.name, base.version),
                                    version: version.version,
                                    span: base.span,
                                    line: base.line,
                                });
                            }

                            let parent = self
                                .bitset_registry
                                .bitsets
                                .get(&TypeId { name: base.name.clone(), version: base.version })
                                .ok_or_else(|| AnalysisError::UnknownParentType {
                                    child: name.clone(),
                                    parent: format!("{}@{}", base.name, base.version),
                                    version: version.version,
                                    span: base.span,
                                    line: base.line,
                                })?;

                            let mut variants = parent.variants.clone();

                            for op in ops {
                                match op {
                                    BitsetOpAst::Add { name: v_name, .. } => {
                                        variants.push(v_name.clone());
                                    }
                                    BitsetOpAst::Remove { name: v_name, span, line } => {
                                        if let Some(idx) = variants.iter().position(|v| v == v_name) {
                                            variants[idx] = format!("__deprecated_{}", v_name);
                                        } else {
                                            return Err(AnalysisError::FieldNotFound {
                                                op: "remove",
                                                field: v_name.clone(),
                                                type_name: name.clone(),
                                                version: version.version,
                                                span: *span,
                                                line: *line,
                                            });
                                        }
                                    }
                                }
                            }

                            ResolvedBitset { variants }
                        }
                    };

                    let id = TypeId { name: bd.name().to_string(), version: version.version };
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
                    span: td.span,
                    line: td.line,
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
                // was `.expect("type must be resolved")` — silently panicked on a real
                // resolution failure instead of returning the (now-spanned) error.
                let ty = self.resolve(&f.ty, version, f.span, f.line)?;
                let default = coerce_default(
                    f.default.as_ref().map(DefaultValue::from),
                    &ty,
                    &f.name,
                    version,
                    &self.bitset_registry,
                    f.span,
                    f.line,
                )?;
                Ok(FieldIR { id: self.id_gen.next(), name: f.name.clone(), ty, default, lazy: f.lazy })
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
                    span: td.span,
                    line: td.line,
                });
            }
        };

        if extends.version >= version {
            return Err(AnalysisError::UnknownParentType {
                child: td.name.clone(),
                parent: format!("{}@{}", extends.name, extends.version),
                version,
                span: extends.span,
                line: extends.line,
            });
        }

        let parent = self
            .type_registry
            .types
            .get(&TypeId { name: extends.name.clone(), version: extends.version })
            .ok_or_else(|| AnalysisError::UnknownParentType {
                child: td.name.clone(),
                parent: format!("{}@{}", extends.name, extends.version),
                version,
                span: extends.span,
                line: extends.line,
            })?;

        let mut fields: Vec<FieldIR> = parent.fields.clone();
        let mut consts: Vec<ResolvedConst> = parent.const_fields.clone();

        for op in ops {
            match op {
                DiffAst::Add { field } => {
                    let ty = self.resolve(&field.ty, version, field.span, field.line)?;

                    if field.lazy && !matches!(ty, ResolvedTypeRef::Optional(_)) {
                        return Err(AnalysisError::LazyDiffFieldMustBeOptional {
                            field: field.name.clone(),
                            version,
                            span: field.span,
                            line: field.line,
                        });
                    }

                    let default = match &ty {
                        ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) => Some(DefaultValue::Struct),
                        ResolvedTypeRef::Union(_) => Some(DefaultValue::Struct),
                        ResolvedTypeRef::Optional(_) if field.default.is_none() => Some(DefaultValue::None),
                        _ => {
                            let raw = field.default.as_ref().map(DefaultValue::from).ok_or_else(|| {
                                AnalysisError::MissingDefault {
                                    field: field.name.clone(),
                                    version,
                                    span: field.span,
                                    line: field.line,
                                }
                            })?;
                            coerce_default(
                                Some(raw),
                                &ty,
                                &field.name,
                                version,
                                &self.bitset_registry,
                                field.span,
                                field.line,
                            )?
                        }
                    };

                    fields.push(FieldIR { id: self.id_gen.next(), name: field.name.clone(), ty, default, lazy: field.lazy });
                }

                DiffAst::AddConst { field } => {
                    if consts.iter().any(|c| c.name == field.name) || fields.iter().any(|f| f.name == field.name) {
                        return Err(AnalysisError::FieldAlreadyExists {
                            version,
                            field: field.name.clone(),
                            span: field.span,
                            line: field.line,
                        });
                    }

                    consts.push(resolve_const(field, version)?);
                }

                DiffAst::Remove { name, span, line } => {
                    let existed_in_fields = fields.iter().any(|f| f.name == *name);
                    let existed_in_consts = consts.iter().any(|c| c.name == *name);

                    if !existed_in_fields && !existed_in_consts {
                        return Err(AnalysisError::FieldNotFound {
                            op: "remove",
                            field: name.clone(),
                            type_name: td.name.clone(),
                            version,
                            span: *span,
                            line: *line,
                        });
                    }
                    fields.retain(|f| f.name != *name);
                    consts.retain(|c| c.name != *name);
                }

                DiffAst::Rename { from, to, span, line } => {
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
                            span: *span,
                            line: *line,
                        });
                    }
                }

                DiffAst::UpdateType { name, ty, lazy, span, line } => {
                    if let Some(f) = fields.iter_mut().find(|f| f.name == *name) {
                        let new_ty = self.resolve(ty, version, *span, *line)?;
                        check_type_update(&f.ty, &new_ty, version)?;
                        f.ty = new_ty;
                        f.lazy = *lazy;
                    } else if consts.iter().any(|c| c.name == *name) {
                    } else {
                        return Err(AnalysisError::FieldNotFound {
                            op: "update type",
                            field: name.clone(),
                            type_name: td.name.clone(),
                            version,
                            span: *span,
                            line: *line,
                        });
                    }
                }

                DiffAst::Transform { from, to, ty, lazy, span, line } => {
                    if let Some(f) = fields.iter_mut().find(|f| f.name == *from) {
                        f.name = to.clone();
                        if let Some(ty) = ty {
                            // was `.expect("type must be resolved")` — same panic bug as above.
                            f.ty = self.resolve(ty, version, *span, *line)?;
                        }
                        f.lazy = *lazy;
                    } else if let Some(c) = consts.iter_mut().find(|c| c.name == *from) {
                        c.name = to.clone();
                    } else {
                        return Err(AnalysisError::FieldNotFound {
                            op: "transform",
                            field: from.clone(),
                            type_name: td.name.clone(),
                            version,
                            span: *span,
                            line: *line,
                        });
                    }
                }

                DiffAst::UpdateConst { name, ty, value, span, line } => {
                    if let Some(c) = consts.iter_mut().find(|c| c.name == *name) {
                        let updated = resolve_const(
                            &ConstFieldAst { name: name.clone(), ty: ty.clone(), value: value.clone(), span: *span, line: *line },
                            version,
                        )?;
                        c.rust_type = updated.rust_type;
                        c.value = updated.value;
                    } else {
                        return Err(AnalysisError::FieldNotFound {
                            op: "update const",
                            field: name.clone(),
                            type_name: td.name.clone(),
                            version,
                            span: *span,
                            line: *line,
                        });
                    }
                }

                DiffAst::TransformConst { from, to, ty, value, span, line } => {
                    if let Some(c) = consts.iter_mut().find(|c| c.name == *from) {
                        let updated = resolve_const(
                            &ConstFieldAst { name: to.clone(), ty: ty.clone(), value: value.clone(), span: *span, line: *line },
                            version,
                        )?;
                        c.name = updated.name;
                        c.rust_type = updated.rust_type;
                        c.value = updated.value;
                    } else {
                        return Err(AnalysisError::FieldNotFound {
                            op: "transform const",
                            field: from.clone(),
                            type_name: td.name.clone(),
                            version,
                            span: *span,
                            line: *line,
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
                VersionBlockAst::TypeDef(_) => continue,
                VersionBlockAst::EnumDef(_) => continue,
                VersionBlockAst::UnionDef(_) => continue,
                VersionBlockAst::BitsetDef(_) => continue,
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
            let ty = self.resolve(&field.ty, version, field.span, field.line)?;
            let default = coerce_default(
                field.default.as_ref().map(DefaultValue::from),
                &ty,
                &field.name,
                version,
                &self.bitset_registry,
                field.span,
                field.line,
            )?;
            ctx.fields.push(FieldIR { id: self.id_gen.next(), name: field.name.clone(), ty, default, lazy: field.lazy });
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
                        return Err(AnalysisError::FieldAlreadyExists {
                            version,
                            field: field.name.clone(),
                            span: field.span,
                            line: field.line,
                        });
                    }

                    let ty = self.resolve(&field.ty, version, field.span, field.line)?;

                    if field.lazy && !matches!(ty, ResolvedTypeRef::Optional(_)) {
                        return Err(AnalysisError::LazyDiffFieldMustBeOptional {
                            field: field.name.clone(),
                            version,
                            span: field.span,
                            line: field.line,
                        });
                    }

                    let default = match &ty {
                        ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) => Some(DefaultValue::Struct),
                        ResolvedTypeRef::Union(_) => Some(DefaultValue::Struct),
                        ResolvedTypeRef::Optional(_) if field.default.is_none() => Some(DefaultValue::None),
                        _ => {
                            let raw = field.default.as_ref().map(DefaultValue::from).ok_or_else(|| {
                                AnalysisError::MissingDefault {
                                    field: field.name.clone(),
                                    version,
                                    span: field.span,
                                    line: field.line,
                                }
                            })?;
                            coerce_default(
                                Some(raw),
                                &ty,
                                &field.name,
                                version,
                                &self.bitset_registry,
                                field.span,
                                field.line,
                            )?
                        }
                    };

                    ctx.fields.push(FieldIR { id: self.id_gen.next(), name: field.name.clone(), ty, default, lazy: field.lazy });
                }

                DiffAst::AddConst { field } => {
                    if ctx.const_fields.iter().any(|c| c.name == field.name) || ctx.fields.iter().any(|f| f.name == field.name) {
                        return Err(AnalysisError::FieldAlreadyExists {
                            version,
                            field: field.name.clone(),
                            span: field.span,
                            line: field.line,
                        });
                    }

                    ctx.const_fields.push(resolve_const(field, version)?);
                }

                DiffAst::Remove { name, .. } => {
                    ctx.fields.retain(|f| f.name != *name);
                    ctx.const_fields.retain(|c| c.name != *name);
                }

                DiffAst::Rename { from, to, .. } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *from) {
                        f.name = to.clone();
                    } else if let Some(c) = ctx.const_fields.iter_mut().find(|c| c.name == *from) {
                        c.name = to.clone();
                    }
                }

                DiffAst::UpdateType { name, ty, lazy, span, line } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *name) {
                        let new_ty = self.resolve(ty, version, *span, *line)?;
                        check_type_update(&f.ty, &new_ty, version)?;
                        f.ty = new_ty;
                        f.lazy = *lazy;
                    }
                }

                DiffAst::Transform { from, to, ty, lazy, span, line } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *from) {
                        f.name = to.clone();
                        if let Some(ty) = ty {
                            f.ty = self.resolve(ty, version, *span, *line)?;
                        }
                        f.lazy = *lazy;
                    } else if let Some(c) = ctx.const_fields.iter_mut().find(|c| c.name == *from) {
                        c.name = to.clone();
                    }
                }

                DiffAst::UpdateConst { name, ty, value, span, line } => {
                    if let Some(c) = ctx.const_fields.iter_mut().find(|c| c.name == *name) {
                        let updated = resolve_const(
                            &ConstFieldAst { name: name.clone(), ty: ty.clone(), value: value.clone(), span: *span, line: *line },
                            version,
                        )?;
                        c.rust_type = updated.rust_type;
                        c.value = updated.value;
                    } else {
                        return Err(AnalysisError::FieldNotFound {
                            op: "update const",
                            field: name.clone(),
                            type_name: "<root>".to_string(),
                            version,
                            span: *span,
                            line: *line,
                        });
                    }
                }
                DiffAst::TransformConst { from, to, ty, value, span, line } => {
                    if let Some(c) = ctx.const_fields.iter_mut().find(|c| c.name == *from) {
                        let updated = resolve_const(
                            &ConstFieldAst { name: to.clone(), ty: ty.clone(), value: value.clone(), span: *span, line: *line },
                            version,
                        )?;
                        c.name = updated.name;
                        c.rust_type = updated.rust_type;
                        c.value = updated.value;
                    } else {
                        return Err(AnalysisError::FieldNotFound {
                            op: "transform const",
                            field: from.clone(),
                            type_name: "<root>".to_string(),
                            version,
                            span: *span,
                            line: *line,
                        });
                    }
                }
            }
        }

        Ok(())
    }

    fn resolve(&self, ty: &TypeAst, version: i128, span: Span, line: u32) -> Result<ResolvedTypeRef, AnalysisError> {
        match ty {
            TypeAst::Named(name) => {
                if is_primitive(name) {
                    return Ok(ResolvedTypeRef::Scalar(TypeId { name: name.clone(), version }));
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

                if let Some((id, _)) = self.union_registry.latest_before(name, version + 1) {
                    return Ok(ResolvedTypeRef::Union(id.clone()));
                }

                if let Some(type_id) = self.resolver.resolve_type(name, version) {
                    return Ok(ResolvedTypeRef::Scalar(type_id));
                }

                Err(AnalysisError::UnknownType { name: name.clone(), version, span, line })
            }
            TypeAst::Array(inner) => {
                let inner_ref = self.resolve(inner, version, span, line)?;
                Ok(ResolvedTypeRef::Array(Box::new(inner_ref)))
            }
            TypeAst::FixedArray(inner, n) => {
                if *n > 256 {
                    return Err(AnalysisError::FixedSizeTooLarge { kind: "array", n: *n, version, span, line });
                }
                let inner_ref = self.resolve(inner, version, span, line)?;
                Ok(ResolvedTypeRef::FixedArray(Box::new(inner_ref), *n))
            }
            TypeAst::DeltaArray(inner) => {
                let inner_ref = self.resolve(inner, version, span, line)?;
                if !is_delta_eligible(&inner_ref) {
                    return Err(AnalysisError::InvalidDeltaElementType {
                        type_desc: format!("{:?}", inner_ref),
                        version,
                        span,
                        line,
                    });
                }
                Ok(ResolvedTypeRef::DeltaArray(Box::new(inner_ref)))
            }
            TypeAst::FixedDeltaArray(inner, n) => {
                if *n > 256 {
                    return Err(AnalysisError::FixedSizeTooLarge { kind: "array", n: *n, version, span, line });
                }
                let inner_ref = self.resolve(inner, version, span, line)?;
                if !is_delta_eligible(&inner_ref) {
                    return Err(AnalysisError::InvalidDeltaElementType {
                        type_desc: format!("{:?}", inner_ref),
                        version,
                        span,
                        line,
                    });
                }
                Ok(ResolvedTypeRef::FixedDeltaArray(Box::new(inner_ref), *n))
            }
            TypeAst::FixedString(n) => Ok(ResolvedTypeRef::FixedString(*n)),
            TypeAst::Map(k, v) => Ok(ResolvedTypeRef::Map(
                Box::new(self.resolve(k, version, span, line)?),
                Box::new(self.resolve(v, version, span, line)?),
            )),
            TypeAst::FixedMap(k, v, n) => {
                if *n > 1024 {
                    return Err(AnalysisError::FixedSizeTooLarge { kind: "map", n: *n, version, span, line });
                }
                Ok(ResolvedTypeRef::FixedMap(
                    Box::new(self.resolve(k, version, span, line)?),
                    Box::new(self.resolve(v, version, span, line)?),
                    *n,
                ))
            }
            TypeAst::Tuple(elements) => {
                let resolved = elements
                    .iter()
                    .map(|t| self.resolve(t, version, span, line))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(ResolvedTypeRef::Tuple(resolved))
            }
            TypeAst::VFloat { min, max, step } => {
                if !step.is_finite() || *step <= 0.0 {
                    return Err(AnalysisError::InvalidVFloat { reason: "step must be > 0".into(), version, span, line });
                }
                if !min.is_finite() || !max.is_finite() || max <= min {
                    return Err(AnalysisError::InvalidVFloat {
                        reason: "max must be finite and greater than min".into(),
                        version,
                        span,
                        line,
                    });
                }

                // renamed from `span` (f64) to `range` — `span: Span` is now a parameter here.
                let range = (max - min) / step;

                let backing = if range <= u16::MAX as f64 {
                    VFloatBacking::U16
                } else if range <= u32::MAX as f64 {
                    VFloatBacking::U32
                } else {
                    return Err(AnalysisError::VFloatRangeTooLarge { range, version, span, line });
                };

                Ok(ResolvedTypeRef::VFloat { min: *min, max: *max, step: *step, backing })
            }
            TypeAst::Optional(v) => {
                let inner_ref = self.resolve(v, version, span, line)?;
                Ok(ResolvedTypeRef::Optional(Box::new(inner_ref)))
            }
        }
    }

    fn snapshot(&self, ctx: &VersionContext, version: i128) -> ResolvedVersion {
        ResolvedVersion { version, fields: ctx.fields.clone(), const_fields: ctx.const_fields.clone() }
    }

    pub fn finish(self) -> Result<ResolvedSchema, AnalysisError> {
        if self.version_states.is_empty() {
            return Err(AnalysisError::NoVersions { span: self.ast.span, line: self.ast.line });
        }

        let lineage = SchemaLineage::build_from(&self.version_states);

        Ok(ResolvedSchema {
            name_hint: self.ast.name.clone(),
            versions: self.version_states,
            types: self.type_registry,
            unions: self.union_registry,
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
    bitset_registry: &BitsetRegistry,
    span: Span,
    line: u32,
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
                        span,
                        line,
                    });
                }
                Ok(Some(DefaultValue::FixedBytes(bytes)))
            }
            other => Ok(other),
        },
        ResolvedTypeRef::Bitset(type_id, _) => match default {
            Some(DefaultValue::BitsetLiteral { ref ty_name, ref kvs }) => {
                if ty_name != &type_id.name {
                    return Err(AnalysisError::TypeMismatch {
                        expected: type_id.name.clone(),
                        got: ty_name.clone(),
                        version,
                        span,
                        line,
                    });
                }

                let bitset_def = bitset_registry.bitsets.get(type_id).ok_or_else(|| AnalysisError::UnknownType {
                    name: type_id.name.clone(),
                    version,
                    span,
                    line,
                })?;

                for (flag_name, _) in kvs {
                    if !bitset_def.variants.contains(flag_name) {
                        return Err(AnalysisError::FieldNotFound {
                            op: "default assignment",
                            field: flag_name.clone(),
                            type_name: type_id.name.clone(),
                            version,
                            span,
                            line,
                        });
                    }
                }
                Ok(Some(DefaultValue::BitsetLiteral { ty_name: ty_name.clone(), kvs: kvs.clone() }))
            }
            Some(DefaultValue::Int(0)) => Ok(Some(DefaultValue::BitsetLiteral { ty_name: type_id.name.clone(), kvs: vec![] })),
            Some(DefaultValue::Int(n)) => Err(AnalysisError::TypeMismatch {
                expected: type_id.name.clone(),
                got: format!("{n}"),
                version,
                span,
                line,
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
                        span,
                        line,
                    })
                }
            };

            match as_f64 {
                Some(f) if f < *min || f > *max => Err(AnalysisError::VFloatDefaultOutOfRange {
                    field: field_name.to_string(),
                    value: f,
                    min: *min,
                    max: *max,
                    version,
                    span,
                    line,
                }),
                Some(f) => Ok(Some(DefaultValue::Float(f))),
                None => Ok(None),
            }
        }
        _ => Ok(default),
    }
}

fn resolve_const(field: &ConstFieldAst, version: i128) -> Result<ResolvedConst, AnalysisError> {
    let span = field.span;
    let line = field.line;

    let rust_type = match &field.ty {
        TypeAst::Named(n) => match normalize_type(n) {
            "u8" => "u8", "u16" => "u16", "u32" => "u32", "u64" => "u64",
            "i8" => "i8", "i16" => "i16", "i32" => "i32", "i64" => "i64",
            "f32" => "f32", "f64" => "f64", "bool" => "bool",
            "string" => "&'static str",
            "varint32" | "varint64" => return Err(AnalysisError::VarintsCannotBeConst { version, span, line }),
            other => return Err(AnalysisError::TypeMismatch {
                expected: "primitive type".into(),
                got: other.to_string(),
                version,
                span,
                line,
            }),
        },
        _ => return Err(AnalysisError::TypeMismatch {
            expected: "primitive type".into(),
            got: format!("{:?}", field.ty),
            version,
            span,
            line,
        }),
    };

    let value = DefaultValue::from(&field.value);
    match (&value, rust_type) {
        (DefaultValue::Bool(_), "bool") => {}
        (DefaultValue::Int(_), "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64") => {}
        (DefaultValue::Float(_), "f32" | "f64") => {}
        (DefaultValue::Str(_), "&'static str") => {}
        _ => return Err(AnalysisError::TypeMismatch {
            expected: rust_type.into(),
            got: format!("{:?}", value),
            version,
            span,
            line,
        }),
    }

    Ok(ResolvedConst { name: field.name.clone(), rust_type, value })
}

fn check_type_update(
    _old_ty: &ResolvedTypeRef,
    _new_ty: &ResolvedTypeRef,
    _version: i128,
) -> Result<(), AnalysisError> {
    // TODO: validate the cast is sound. Doesn't error today, so no span needed
    // yet — if it starts erroring, thread span/line through like everywhere else.
    Ok(())
}