use super::id_gen::*;
use super::ir_types::*;
use super::lineage::SchemaLineage;
use super::resolver::*;
use crate::ast::*;
use crate::error::AnalysisError;
use crate::span::Span;
use pojoc_core::types::*;
use std::collections::HashMap;
use std::sync::Arc;

/// Where a `TemplateField`'s default came from — determines how it gets
/// classified once the field's concrete type is known (see `instantiate_shape`).
#[derive(Debug, Clone)]
enum TemplateDefault {
    /// Declared directly in a `type Name { ... }` body: coerced literally
    /// against the resolved type, `None` if absent.
    Literal(Option<DefaultValueAst>),
    /// Introduced via a `diff { + field: Type [= value] }` op: needs the same
    /// "struct sentinel / optional-none sentinel / literal" classification
    /// `handle_diff`'s `DiffAst::Add` always applied, just deferred here until
    /// the field's type is fully resolved (it may still mention a template
    /// param at merge time).
    AddedViaDiff(Option<DefaultValueAst>),
}

/// A field within a not-yet-fully-resolved type template. Its `ty` may still
/// reference the template's own type parameters (or, transiently while an
/// `extends<...>` chain is being merged, a dropped ancestor parameter as
/// `TypeAst::Wildcard`).
#[derive(Debug, Clone)]
struct TemplateField {
    /// Stable across every version/instantiation this field is inherited into —
    /// assigned once, when the field is first declared or `+ Add`ed, and carried
    /// forward unchanged by `extends` (mirroring the old `parent.fields.clone()`
    /// behavior, so renames/retypes still track as "the same field").
    id: FieldId,
    name: String,
    ty: TypeAst,
    default: TemplateDefault,
    lazy: bool,
    span: Span,
    line: u32,
}

#[derive(Debug, Clone)]
struct TemplateConst {
    name: String,
    ty: TypeAst,
    value: DefaultValueAst,
    span: Span,
    line: u32,
}

/// The AST-level, unresolved shape of a `type` def at a given version: its own
/// declared type parameters plus its fully-merged (`extends`/`diff` applied)
/// field and const lists. Computing this never calls `resolve()`, so it can't
/// loop on self-referential *field* types — the only recursion is through the
/// `extends` chain, which is version-strictly-decreasing and therefore finite.
#[derive(Debug, Clone)]
struct TemplateShape {
    params: Vec<String>,
    fields: Vec<TemplateField>,
    consts: Vec<TemplateConst>,
}

/// A generic instantiation discovered while resolving a field type, queued so
/// its fields are computed *after* we've already committed to a `TypeId` for
/// it. This is what makes self-referential generics (`Node<T> { next: Node<T>? }`)
/// safe: the recursive field only ever needs the `TypeId` pointer, not the
/// fully-computed fields, exactly like ordinary struct field references.
#[derive(Debug)]
struct PendingGeneric {
    type_id: TypeId,
    shape: TemplateShape,
    subst: HashMap<String, ResolvedTypeRef>,
    found_version: i128,
}

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
    imports: HashMap<String, Arc<ResolvedSchema>>,
    id_gen: IdGen,
    /// Every generic `TypeId` handed out so far, mapped to its canonical
    /// auto-mangled identity (`Box<i32>` -> `"BoxI32"`) even when an explicit
    /// `as Alias` was used — lets a second request for the same alias with
    /// different args be rejected instead of silently aliasing to the wrong type.
    generic_identities: HashMap<TypeId, String>,
    pending_generics: Vec<PendingGeneric>,
    /// Memoizes `build_shape` by (type name, exact version) so that a type looked
    /// up repeatedly (as an `extends` ancestor from multiple descendants, or as a
    /// generic template instantiated with different args) gets the exact same
    /// `FieldId`s every time, instead of fresh ones per call.
    shape_cache: HashMap<(String, i128), TemplateShape>,
}

impl<'a> SchemaAnalyzer<'a> {
    pub fn new(ast: &'a SchemaAst, imports: HashMap<String, Arc<ResolvedSchema>>) -> Self {
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
            imports,
            generic_identities: HashMap::new(),
            pending_generics: Vec::new(),
            shape_cache: HashMap::new(),
        }
    }

    #[allow(clippy::result_large_err)]
    pub fn run(&mut self) -> Result<(), AnalysisError> {
        // we preregister unions so they can be referenced in types with no issues
        // enums and bitsets don't depend on any external types, so they don't need any special handling.
        self.collect_enums()?;
        self.collect_bitsets()?;
        self.preregister_union_ids();
        self.collect_types()?;
        self.collect_unions()?;
        for version in &self.ast.versions {
            self.process_version(version)?;
        }
        self.drain_pending_generics()?;
        Ok(())
    }

    #[allow(clippy::result_large_err)]
    fn drain_pending_generics(&mut self) -> Result<(), AnalysisError> {
        while let Some(item) = self.pending_generics.pop() {
            let (fields, const_fields) =
                self.instantiate_shape(&item.shape, &item.subst, item.found_version)?;
            self.type_registry.types.insert(
                item.type_id,
                ResolvedType {
                    fields,
                    const_fields,
                },
            );
        }
        Ok(())
    }

    #[allow(clippy::result_large_err)]
    fn collect_types(&mut self) -> Result<(), AnalysisError> {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::TypeDef(td) = block {
                    // generic templates are only instantiated lazily, on first use
                    if !td.params.is_empty() {
                        continue;
                    }

                    let empty_subst = HashMap::new();
                    let (shape, found_version) =
                        self.template_shape(&td.name, version.version, td.span, td.line)?;
                    let (fields, const_fields) =
                        self.instantiate_shape(&shape, &empty_subst, found_version)?;

                    let id = TypeId {
                        name: td.name.clone(),
                        version: version.version,
                    };
                    self.type_registry.types.insert(
                        id,
                        ResolvedType {
                            fields,
                            const_fields,
                        },
                    );
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::result_large_err)]
    fn collect_enums(&mut self) -> Result<(), AnalysisError> {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::EnumDef(ed) = block {
                    let resolved = match ed {
                        EnumDefAst::Definition { variants, .. } => {
                            let mut resolved = vec![EnumVariant {
                                name: "Unknown".into(),
                                wire_value: 0,
                            }];
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

                        EnumDefAst::Extension {
                            name, base, ops, ..
                        } => {
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
                                .get(&TypeId {
                                    name: base.name.clone(),
                                    version: base.version,
                                })
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
                                    EnumVariantOpAst::Rename {
                                        from,
                                        to,
                                        span,
                                        line,
                                    } => {
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
                                    EnumVariantOpAst::Add {
                                        name: variant_name, ..
                                    } => {
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

    fn preregister_union_ids(&mut self) {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::UnionDef(ud) = block {
                    let id = TypeId {
                        name: ud.name().to_string(),
                        version: version.version,
                    };
                    // empty variants — collect_unions overwrites this with the real data
                    self.union_registry
                        .unions
                        .entry(id)
                        .or_insert(ResolvedUnion { variants: vec![] });
                }
            }
        }
    }

    #[allow(clippy::result_large_err)]
    fn collect_unions(&mut self) -> Result<(), AnalysisError> {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::UnionDef(ud) = block {
                    let resolved = match ud {
                        UnionDefAst::Definition {
                            name: _, variants, ..
                        } => ResolvedUnion {
                            variants: self.resolve_union_variants(variants, version.version)?,
                        },

                        UnionDefAst::Extension {
                            name, base, ops, ..
                        } => {
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
                                .get(&TypeId {
                                    name: base.name.clone(),
                                    version: base.version,
                                })
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
                                    UnionVariantOpAst::Add {
                                        name: vname,
                                        payload_ty,
                                        span,
                                        line,
                                    } => {
                                        if variants.iter().any(|v| &v.name == vname) {
                                            return Err(AnalysisError::FieldAlreadyExists {
                                                version: version.version,
                                                field: vname.clone(),
                                                span: *span,
                                                line: *line,
                                            });
                                        }

                                        let payload = self.resolve(
                                            payload_ty,
                                            version.version,
                                            *span,
                                            *line,
                                            &HashMap::new(),
                                        )?;

                                        let discriminant = variants
                                            .iter()
                                            .map(|v| v.discriminant)
                                            .max()
                                            .map_or(0, |m| m + 1);
                                        variants.push(UnionVariant {
                                            name: vname.clone(),
                                            payload,
                                            discriminant,
                                        });
                                    }
                                }
                            }

                            ResolvedUnion { variants }
                        }
                    };

                    let id = TypeId {
                        name: ud.name().to_string(),
                        version: version.version,
                    };
                    self.union_registry.unions.insert(id, resolved);
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::result_large_err)]
    fn resolve_union_variants(
        &mut self,
        variants: &[UnionVariantAst],
        version: i128,
    ) -> Result<Vec<UnionVariant>, AnalysisError> {
        variants
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let payload =
                    self.resolve(&v.payload_ty, version, v.span, v.line, &HashMap::new())?; // ← was resolver.resolve_type + ok_or
                Ok(UnionVariant {
                    name: v.name.clone(),
                    payload,
                    discriminant: i as u64,
                })
            })
            .collect()
    }

    #[allow(clippy::result_large_err)]
    fn collect_bitsets(&mut self) -> Result<(), AnalysisError> {
        for version in &self.ast.versions {
            for block in &version.blocks {
                if let VersionBlockAst::BitsetDef(bd) = block {
                    let resolved = match bd {
                        BitsetDefAst::Definition { variants, .. } => ResolvedBitset {
                            variants: variants.clone(),
                        },
                        BitsetDefAst::Extension {
                            name, base, ops, ..
                        } => {
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
                                .get(&TypeId {
                                    name: base.name.clone(),
                                    version: base.version,
                                })
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
                                    BitsetOpAst::Remove {
                                        name: v_name,
                                        span,
                                        line,
                                    } => {
                                        if let Some(idx) = variants.iter().position(|v| v == v_name)
                                        {
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

    /// Computes the AST-level, unresolved shape of type `name` at the latest version
    /// <= `version` (used for both plain type-name lookups and generic-template
    /// lookups). `span`/`line` are only used for the "no such type" error, so callers
    /// should pass the usage site's location.
    #[allow(clippy::result_large_err)]
    fn template_shape(
        &mut self,
        name: &str,
        version: i128,
        span: Span,
        line: u32,
    ) -> Result<(TemplateShape, i128), AnalysisError> {
        let (td, found_version) =
            self.resolver
                .resolve_type_def(name, version)
                .ok_or_else(|| AnalysisError::UnknownType {
                    name: name.to_string(),
                    version,
                    span,
                    line,
                })?;
        let shape = self.build_shape(td, found_version)?;
        Ok((shape, found_version))
    }

    /// Computes the shape of an already-located `TypeDefAst` at its exact `version`,
    /// memoized by (name, version) so repeated lookups of the same def (as an
    /// `extends` ancestor from multiple descendants, or as a generic template
    /// instantiated with different args) return the exact same `FieldId`s every
    /// time rather than assigning fresh ones. Recurses through `extends` chains
    /// only — never through field types — so a self-referential field
    /// (`Node<T> { next: Node<T>? }`) can never cause this to loop; the chain
    /// itself is strictly version-decreasing and finite.
    #[allow(clippy::result_large_err)]
    fn build_shape(
        &mut self,
        td: &TypeDefAst,
        version: i128,
    ) -> Result<TemplateShape, AnalysisError> {
        let cache_key = (td.name.clone(), version);
        if let Some(shape) = self.shape_cache.get(&cache_key) {
            return Ok(shape.clone());
        }

        let shape = match &td.body {
            TypeBody::Fields(f) => TemplateShape {
                params: td.params.clone(),
                fields: f
                    .fields
                    .iter()
                    .map(|fa| TemplateField {
                        id: self.id_gen.next_id(),
                        name: fa.name.clone(),
                        ty: fa.ty.clone(),
                        default: TemplateDefault::Literal(fa.default.clone()),
                        lazy: fa.lazy,
                        span: fa.span,
                        line: fa.line,
                    })
                    .collect(),
                consts: f
                    .const_fields
                    .iter()
                    .map(|cf| TemplateConst {
                        name: cf.name.clone(),
                        ty: cf.ty.clone(),
                        value: cf.value.clone(),
                        span: cf.span,
                        line: cf.line,
                    })
                    .collect(),
            },
            TypeBody::Diff(ops) => {
                let extends = td
                    .extends
                    .as_ref()
                    .expect("parser only produces TypeBody::Diff alongside an extends clause")
                    .clone();

                if extends.version >= version {
                    return Err(AnalysisError::UnknownParentType {
                        child: td.name.clone(),
                        parent: format!("{}@{}", extends.name, extends.version),
                        version,
                        span: extends.span,
                        line: extends.line,
                    });
                }

                let ancestor_td = self
                    .resolver
                    .resolve_type_def_exact(&extends.name, extends.version)
                    .ok_or_else(|| AnalysisError::UnknownParentType {
                        child: td.name.clone(),
                        parent: format!("{}@{}", extends.name, extends.version),
                        version,
                        span: extends.span,
                        line: extends.line,
                    })?;
                let ancestor = self.build_shape(ancestor_td, extends.version)?;

                if extends.args.len() != ancestor.params.len() {
                    return Err(AnalysisError::GenericArityMismatch {
                        name: extends.name.clone(),
                        expected: ancestor.params.len(),
                        found: extends.args.len(),
                        version,
                        span: extends.span,
                        line: extends.line,
                    });
                }

                let rename: HashMap<&str, &GenericArgAst> = ancestor
                    .params
                    .iter()
                    .map(String::as_str)
                    .zip(extends.args.iter())
                    .collect();

                let mut fields: Vec<TemplateField> = ancestor
                    .fields
                    .iter()
                    .map(|f| TemplateField {
                        id: f.id,
                        name: f.name.clone(),
                        ty: substitute_ast(&f.ty, &rename),
                        default: f.default.clone(),
                        lazy: f.lazy,
                        span: f.span,
                        line: f.line,
                    })
                    .collect();
                let mut consts: Vec<TemplateConst> = ancestor
                    .consts
                    .iter()
                    .map(|c| TemplateConst {
                        name: c.name.clone(),
                        ty: substitute_ast(&c.ty, &rename),
                        value: c.value.clone(),
                        span: c.span,
                        line: c.line,
                    })
                    .collect();

                apply_template_diff_ops(
                    &td.name,
                    ops,
                    version,
                    &mut fields,
                    &mut consts,
                    &mut self.id_gen,
                )?;

                for f in &fields {
                    if type_ast_contains_wildcard(&f.ty) {
                        return Err(AnalysisError::UnresolvedGenericWildcard {
                            field: f.name.clone(),
                            type_name: td.name.clone(),
                            version,
                            span: f.span,
                            line: f.line,
                        });
                    }
                }
                for c in &consts {
                    if type_ast_contains_wildcard(&c.ty) {
                        return Err(AnalysisError::UnresolvedGenericWildcard {
                            field: c.name.clone(),
                            type_name: td.name.clone(),
                            version,
                            span: c.span,
                            line: c.line,
                        });
                    }
                }

                TemplateShape {
                    params: td.params.clone(),
                    fields,
                    consts,
                }
            }
        };

        self.shape_cache.insert(cache_key, shape.clone());
        Ok(shape)
    }

    /// Resolves a fully-merged `TemplateShape` into concrete `FieldIR`/`ResolvedConst`
    /// values, substituting `subst` (template param name -> concrete `ResolvedTypeRef`)
    /// into every field type. `subst` is empty for non-generic types.
    #[allow(clippy::result_large_err)]
    fn instantiate_shape(
        &mut self,
        shape: &TemplateShape,
        subst: &HashMap<String, ResolvedTypeRef>,
        version: i128,
    ) -> Result<(Vec<FieldIR>, Vec<ResolvedConst>), AnalysisError> {
        let const_fields = shape
            .consts
            .iter()
            .map(|c| {
                resolve_const(
                    &ConstFieldAst {
                        name: c.name.clone(),
                        ty: c.ty.clone(),
                        value: c.value.clone(),
                        span: c.span,
                        line: c.line,
                    },
                    version,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let fields = shape
            .fields
            .iter()
            .map(|f| {
                let ty = self.resolve(&f.ty, version, f.span, f.line, subst)?;

                let default = match &f.default {
                    TemplateDefault::Literal(raw) => self.coerce_default(
                        raw.as_ref().map(DefaultValue::from),
                        &ty,
                        &f.name,
                        version,
                        f.span,
                        f.line,
                    )?,
                    TemplateDefault::AddedViaDiff(raw) => {
                        if f.lazy && !matches!(ty, ResolvedTypeRef::Optional(_)) {
                            return Err(AnalysisError::LazyDiffFieldMustBeOptional {
                                field: f.name.clone(),
                                version,
                                span: f.span,
                                line: f.line,
                            });
                        }

                        match &ty {
                            ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) => {
                                Some(DefaultValue::Struct)
                            }
                            ResolvedTypeRef::Union(_) => Some(DefaultValue::Struct),
                            ResolvedTypeRef::Optional(_) if raw.is_none() => {
                                Some(DefaultValue::None)
                            }
                            _ => {
                                let lit =
                                    raw.as_ref().map(DefaultValue::from).ok_or_else(|| {
                                        AnalysisError::MissingDefault {
                                            field: f.name.clone(),
                                            version,
                                            span: f.span,
                                            line: f.line,
                                        }
                                    })?;
                                self.coerce_default(
                                    Some(lit),
                                    &ty,
                                    &f.name,
                                    version,
                                    f.span,
                                    f.line,
                                )?
                            }
                        }
                    }
                };

                Ok(FieldIR {
                    id: f.id,
                    name: f.name.clone(),
                    ty,
                    default,
                    lazy: f.lazy,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((fields, const_fields))
    }

    #[allow(clippy::result_large_err)]
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

    #[allow(clippy::result_large_err)]
    fn handle_fields(
        &mut self,
        f: &FieldsAst,
        version: i128,
        ctx: &mut VersionContext,
    ) -> Result<(), AnalysisError> {
        for field in &f.fields {
            let ty = self.resolve(&field.ty, version, field.span, field.line, &HashMap::new())?;
            let default = self.coerce_default(
                field.default.as_ref().map(DefaultValue::from),
                &ty,
                &field.name,
                version,
                field.span,
                field.line,
            )?;
            ctx.fields.push(FieldIR {
                id: self.id_gen.next_id(),
                name: field.name.clone(),
                ty,
                default,
                lazy: field.lazy,
            });
        }
        for cf in &f.const_fields {
            ctx.const_fields.push(resolve_const(cf, version)?);
        }
        Ok(())
    }

    #[allow(clippy::result_large_err)]
    fn handle_diff(
        &mut self,
        diff: &[DiffAst],
        version: i128,
        ctx: &mut VersionContext,
    ) -> Result<(), AnalysisError> {
        for op in diff {
            match op {
                DiffAst::Add { field } => {
                    if ctx.fields.iter().any(|f| f.name == field.name)
                        || ctx.const_fields.iter().any(|c| c.name == field.name)
                    {
                        return Err(AnalysisError::FieldAlreadyExists {
                            version,
                            field: field.name.clone(),
                            span: field.span,
                            line: field.line,
                        });
                    }

                    let ty =
                        self.resolve(&field.ty, version, field.span, field.line, &HashMap::new())?;

                    if field.lazy && !matches!(ty, ResolvedTypeRef::Optional(_)) {
                        return Err(AnalysisError::LazyDiffFieldMustBeOptional {
                            field: field.name.clone(),
                            version,
                            span: field.span,
                            line: field.line,
                        });
                    }

                    let default = match &ty {
                        ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) => {
                            Some(DefaultValue::Struct)
                        }
                        ResolvedTypeRef::Union(_) => Some(DefaultValue::Struct),
                        ResolvedTypeRef::Optional(_) if field.default.is_none() => {
                            Some(DefaultValue::None)
                        }
                        _ => {
                            let raw = field.default.as_ref().map(DefaultValue::from).ok_or_else(
                                || AnalysisError::MissingDefault {
                                    field: field.name.clone(),
                                    version,
                                    span: field.span,
                                    line: field.line,
                                },
                            )?;
                            self.coerce_default(
                                Some(raw),
                                &ty,
                                &field.name,
                                version,
                                field.span,
                                field.line,
                            )?
                        }
                    };

                    ctx.fields.push(FieldIR {
                        id: self.id_gen.next_id(),
                        name: field.name.clone(),
                        ty,
                        default,
                        lazy: field.lazy,
                    });
                }

                DiffAst::AddConst { field } => {
                    if ctx.const_fields.iter().any(|c| c.name == field.name)
                        || ctx.fields.iter().any(|f| f.name == field.name)
                    {
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

                DiffAst::UpdateType {
                    name,
                    ty,
                    lazy,
                    span,
                    line,
                } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *name) {
                        let new_ty = self.resolve(ty, version, *span, *line, &HashMap::new())?;
                        check_type_update(&f.ty, &new_ty, version)?;
                        f.ty = new_ty;
                        f.lazy = *lazy;
                    }
                }

                DiffAst::Transform {
                    from,
                    to,
                    ty,
                    lazy,
                    span,
                    line,
                } => {
                    if let Some(f) = ctx.fields.iter_mut().find(|f| f.name == *from) {
                        f.name = to.clone();
                        if let Some(ty) = ty {
                            f.ty = self.resolve(ty, version, *span, *line, &HashMap::new())?;
                        }
                        f.lazy = *lazy;
                    } else if let Some(c) = ctx.const_fields.iter_mut().find(|c| c.name == *from) {
                        c.name = to.clone();
                    } else {
                        return Err(AnalysisError::FieldNotFound {
                            op: "transform",
                            field: from.clone(),
                            type_name: "<root>".to_string(),
                            version,
                            span: *span,
                            line: *line,
                        });
                    }
                }

                DiffAst::UpdateConst {
                    name,
                    ty,
                    value,
                    span,
                    line,
                } => {
                    if let Some(c) = ctx.const_fields.iter_mut().find(|c| c.name == *name) {
                        let updated = resolve_const(
                            &ConstFieldAst {
                                name: name.clone(),
                                ty: ty.clone(),
                                value: value.clone(),
                                span: *span,
                                line: *line,
                            },
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
                DiffAst::TransformConst {
                    from,
                    to,
                    ty,
                    value,
                    span,
                    line,
                } => {
                    if let Some(c) = ctx.const_fields.iter_mut().find(|c| c.name == *from) {
                        let updated = resolve_const(
                            &ConstFieldAst {
                                name: to.clone(),
                                ty: ty.clone(),
                                value: value.clone(),
                                span: *span,
                                line: *line,
                            },
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

    #[allow(clippy::result_large_err)]
    fn coerce_value(
        &self,
        value: DefaultValue,
        ty: &ResolvedTypeRef,
        field_name: &str,
        version: i128,
        span: Span,
        line: u32,
    ) -> Result<DefaultValue, AnalysisError> {
        if let DefaultValue::Repeat(elem) = value {
            return match ty {
                ResolvedTypeRef::FixedArray(inner, n)
                | ResolvedTypeRef::FixedDeltaArray(inner, n) => {
                    let coerced =
                        self.coerce_value(*elem, inner, field_name, version, span, line)?;
                    Ok(DefaultValue::Array(vec![coerced; *n]))
                }
                other => Err(AnalysisError::TypeMismatch {
                    expected: format!("{:?}", other),
                    got: "spread default (`..`)".into(),
                    version,
                    span,
                    line,
                }),
            };
        }

        // Sentinel for "wire-absent" — only legal under `Optional`.
        if matches!(value, DefaultValue::None) {
            return match ty {
                ResolvedTypeRef::Optional(_) => Ok(DefaultValue::None),
                other => Err(AnalysisError::TypeMismatch {
                    expected: format!("{:?}", other),
                    got: "none".into(),
                    version,
                    span,
                    line,
                }),
            };
        }

        // Sentinel for "use the type's own constructor" — only legal for
        // non-primitive scalars and unions, which have no literal-default grammar.
        if matches!(value, DefaultValue::Struct) {
            return match ty {
                ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) => Ok(DefaultValue::Struct),
                ResolvedTypeRef::Union(_) => Ok(DefaultValue::Struct),
                other => Err(AnalysisError::TypeMismatch {
                    expected: format!("{:?}", other),
                    got: "struct".into(),
                    version,
                    span,
                    line,
                }),
            };
        }

        match ty {
            ResolvedTypeRef::Optional(inner) => {
                self.coerce_value(value, inner, field_name, version, span, line)
            }

            ResolvedTypeRef::FixedString(n) => match value {
                DefaultValue::Str(s) => {
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
                    Ok(DefaultValue::FixedBytes(bytes))
                }
                other => type_mismatch("string", &other, version, span, line),
            },

            ResolvedTypeRef::Array(inner) | ResolvedTypeRef::DeltaArray(inner) => match value {
                DefaultValue::Array(elements) => Ok(DefaultValue::Array(
                    elements
                        .into_iter()
                        .map(|e| self.coerce_value(e, inner, field_name, version, span, line))
                        .collect::<Result<Vec<_>, _>>()?,
                )),
                other => type_mismatch("array", &other, version, span, line),
            },

            ResolvedTypeRef::FixedArray(inner, n) | ResolvedTypeRef::FixedDeltaArray(inner, n) => {
                match value {
                    DefaultValue::Array(elements) => {
                        if elements.len() != *n {
                            return Err(AnalysisError::FixedSizeDefaultLengthMismatch {
                                field: field_name.to_string(),
                                kind: "array",
                                expected: *n,
                                got: elements.len(),
                                version,
                                span,
                                line,
                            });
                        }
                        Ok(DefaultValue::Array(
                            elements
                                .into_iter()
                                .map(|e| {
                                    self.coerce_value(e, inner, field_name, version, span, line)
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                        ))
                    }
                    other => type_mismatch("array", &other, version, span, line),
                }
            }

            ResolvedTypeRef::Map(k, v) => match value {
                DefaultValue::Map(pairs) => Ok(DefaultValue::Map(
                    self.coerce_map_pairs(pairs, k, v, field_name, version, span, line)?,
                )),
                other => type_mismatch("map", &other, version, span, line),
            },

            ResolvedTypeRef::FixedMap(k, v, n) => match value {
                DefaultValue::Map(pairs) => {
                    if pairs.len() != *n {
                        return Err(AnalysisError::FixedSizeDefaultLengthMismatch {
                            field: field_name.to_string(),
                            kind: "map",
                            expected: *n,
                            got: pairs.len(),
                            version,
                            span,
                            line,
                        });
                    }
                    Ok(DefaultValue::Map(self.coerce_map_pairs(
                        pairs, k, v, field_name, version, span, line,
                    )?))
                }
                other => type_mismatch("map", &other, version, span, line),
            },

            ResolvedTypeRef::Tuple(elements) => match value {
                DefaultValue::Tuple(vals) => {
                    if vals.len() != elements.len() {
                        return Err(AnalysisError::FixedSizeDefaultLengthMismatch {
                            field: field_name.to_string(),
                            kind: "tuple",
                            expected: elements.len(),
                            got: vals.len(),
                            version,
                            span,
                            line,
                        });
                    }
                    Ok(DefaultValue::Tuple(
                        vals.into_iter()
                            .zip(elements.iter())
                            .map(|(v, t)| self.coerce_value(v, t, field_name, version, span, line))
                            .collect::<Result<Vec<_>, _>>()?,
                    ))
                }
                other => type_mismatch("tuple", &other, version, span, line),
            },

            ResolvedTypeRef::Bitset(type_id, _) => match value {
                DefaultValue::BitsetLiteral { ty_name, kvs } => {
                    if ty_name != type_id.name {
                        return Err(AnalysisError::TypeMismatch {
                            expected: type_id.name.clone(),
                            got: ty_name,
                            version,
                            span,
                            line,
                        });
                    }
                    let bitset_def =
                        self.bitset_registry.bitsets.get(type_id).ok_or_else(|| {
                            AnalysisError::UnknownType {
                                name: type_id.name.clone(),
                                version,
                                span,
                                line,
                            }
                        })?;
                    for (flag_name, _) in &kvs {
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
                    Ok(DefaultValue::BitsetLiteral { ty_name, kvs })
                }
                DefaultValue::Int(0) => Ok(DefaultValue::BitsetLiteral {
                    ty_name: type_id.name.clone(),
                    kvs: vec![],
                }),
                other => type_mismatch(&type_id.name, &other, version, span, line),
            },

            ResolvedTypeRef::VFloat { min, max, .. } => {
                let as_f64 = match &value {
                    DefaultValue::Float(f) => *f,
                    DefaultValue::Int(i) => *i as f64,
                    other => return type_mismatch("vfloat (number)", other, version, span, line),
                };
                if as_f64 < *min || as_f64 > *max {
                    return Err(AnalysisError::VFloatDefaultOutOfRange {
                        field: field_name.to_string(),
                        value: as_f64,
                        min: *min,
                        max: *max,
                        version,
                        span,
                        line,
                    });
                }
                Ok(DefaultValue::Float(as_f64))
            }

            ResolvedTypeRef::Enum(type_id) => match value {
                DefaultValue::EnumVariant { ty_name, variant } => {
                    if ty_name != type_id.name {
                        return Err(AnalysisError::TypeMismatch {
                            expected: type_id.name.clone(),
                            got: ty_name,
                            version,
                            span,
                            line,
                        });
                    }
                    let enum_def = self.enum_registry.enums.get(type_id).ok_or_else(|| {
                        AnalysisError::UnknownType {
                            name: type_id.name.clone(),
                            version,
                            span,
                            line,
                        }
                    })?;
                    if !enum_def.variants.iter().any(|v| v.name == variant) {
                        return Err(AnalysisError::UnknownEnumVariant {
                            type_name: type_id.name.clone(),
                            variant,
                            version,
                            span,
                            line,
                        });
                    }
                    Ok(DefaultValue::EnumVariant { ty_name, variant })
                }
                other => type_mismatch(&type_id.name, &other, version, span, line),
            },

            ResolvedTypeRef::Scalar(type_id) => {
                if is_primitive(&type_id.name) {
                    coerce_scalar(value, &type_id.name, field_name, version, span, line)
                } else {
                    type_mismatch(
                        &format!("no literal default for struct type `{}`", type_id.name),
                        &value,
                        version,
                        span,
                        line,
                    )
                }
            }

            ResolvedTypeRef::Union(type_id) => type_mismatch(
                &format!("no literal default for union `{}`", type_id.name),
                &value,
                version,
                span,
                line,
            ),
            ResolvedTypeRef::ImportedSchema { alias, .. } => type_mismatch(
                &format!("no literal default for imported type `{}`", alias),
                &value,
                version,
                span,
                line,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::result_large_err)]
    fn coerce_map_pairs(
        &self,
        pairs: Vec<(DefaultValue, DefaultValue)>,
        k: &ResolvedTypeRef,
        v: &ResolvedTypeRef,
        field_name: &str,
        version: i128,
        span: Span,
        line: u32,
    ) -> Result<Vec<(DefaultValue, DefaultValue)>, AnalysisError> {
        pairs
            .into_iter()
            .map(|(pk, pv)| {
                let pk = self.coerce_value(pk, k, field_name, version, span, line)?;
                let pv = self.coerce_value(pv, v, field_name, version, span, line)?;
                Ok((pk, pv))
            })
            .collect()
    }

    #[allow(clippy::result_large_err)]
    fn coerce_default(
        &self,
        default: Option<DefaultValue>,
        ty: &ResolvedTypeRef,
        field_name: &str,
        version: i128,
        span: Span,
        line: u32,
    ) -> Result<Option<DefaultValue>, AnalysisError> {
        let Some(value) = default else {
            return Ok(None);
        };
        self.coerce_value(value, ty, field_name, version, span, line)
            .map(Some)
    }

    #[allow(clippy::result_large_err)]
    fn resolve(
        &mut self,
        ty: &TypeAst,
        version: i128,
        span: Span,
        line: u32,
        subst: &HashMap<String, ResolvedTypeRef>,
    ) -> Result<ResolvedTypeRef, AnalysisError> {
        match ty {
            TypeAst::Named(name) => {
                if let Some(bound) = subst.get(name) {
                    return Ok(bound.clone());
                }

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

                if let Some((id, _)) = self.union_registry.latest_before(name, version + 1) {
                    return Ok(ResolvedTypeRef::Union(id.clone()));
                }

                if let Some((td, found_version)) = self.resolver.resolve_type_def(name, version) {
                    if !td.params.is_empty() {
                        return Err(AnalysisError::GenericArityMismatch {
                            name: name.clone(),
                            expected: td.params.len(),
                            found: 0,
                            version,
                            span,
                            line,
                        });
                    }
                    return Ok(ResolvedTypeRef::Scalar(TypeId {
                        name: name.clone(),
                        version: found_version,
                    }));
                }

                Err(AnalysisError::UnknownType {
                    name: name.clone(),
                    version,
                    span,
                    line,
                })
            }
            TypeAst::Generic(name, args, alias) => {
                let resolved_args = args
                    .iter()
                    .map(|a| self.resolve(a, version, span, line, subst))
                    .collect::<Result<Vec<_>, _>>()?;

                let (shape, found_version) = self.template_shape(name, version, span, line)?;
                if shape.params.len() != resolved_args.len() {
                    return Err(AnalysisError::GenericArityMismatch {
                        name: name.clone(),
                        expected: shape.params.len(),
                        found: resolved_args.len(),
                        version,
                        span,
                        line,
                    });
                }

                let new_subst: HashMap<String, ResolvedTypeRef> = shape
                    .params
                    .iter()
                    .cloned()
                    .zip(resolved_args.iter().cloned())
                    .collect();

                // The auto-mangled name is always computed, even when an
                // explicit `as Alias` is given: it's the canonical identity
                // of "this template instantiated with these exact args",
                // used to detect two DIFFERENT instantiations accidentally
                // requesting the same alias.
                let mangled = mangle_generic_name(name, &resolved_args);
                let display_name = alias.clone().unwrap_or_else(|| mangled.clone());
                let type_id = TypeId {
                    name: display_name,
                    version: found_version,
                };

                if let Some(existing_identity) = self.generic_identities.get(&type_id) {
                    if *existing_identity != mangled {
                        return Err(AnalysisError::GenericNameCollision {
                            name: type_id.name.clone(),
                            version: found_version,
                            span,
                            line,
                        });
                    }
                    return Ok(ResolvedTypeRef::Scalar(type_id));
                }
                if self.type_registry.types.contains_key(&type_id) {
                    return Err(AnalysisError::GenericNameCollision {
                        name: type_id.name.clone(),
                        version: found_version,
                        span,
                        line,
                    });
                }

                self.generic_identities.insert(type_id.clone(), mangled);
                self.pending_generics.push(PendingGeneric {
                    type_id: type_id.clone(),
                    shape,
                    subst: new_subst,
                    found_version,
                });

                Ok(ResolvedTypeRef::Scalar(type_id))
            }
            TypeAst::Wildcard => unreachable!(
                "TypeAst::Wildcard is an internal marker eliminated by template_shape \
                 before resolve() ever sees a field type"
            ),
            TypeAst::Array(inner) => {
                let inner_ref = self.resolve(inner, version, span, line, subst)?;
                Ok(ResolvedTypeRef::Array(Box::new(inner_ref)))
            }
            TypeAst::FixedArray(inner, n) => {
                if *n > 256 {
                    return Err(AnalysisError::FixedSizeTooLarge {
                        kind: "array",
                        n: *n,
                        version,
                        span,
                        line,
                    });
                }
                let inner_ref = self.resolve(inner, version, span, line, subst)?;
                Ok(ResolvedTypeRef::FixedArray(Box::new(inner_ref), *n))
            }
            TypeAst::DeltaArray(inner) => {
                let inner_ref = self.resolve(inner, version, span, line, subst)?;
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
                    return Err(AnalysisError::FixedSizeTooLarge {
                        kind: "array",
                        n: *n,
                        version,
                        span,
                        line,
                    });
                }
                let inner_ref = self.resolve(inner, version, span, line, subst)?;
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
                Box::new(self.resolve(k, version, span, line, subst)?),
                Box::new(self.resolve(v, version, span, line, subst)?),
            )),
            TypeAst::FixedMap(k, v, n) => {
                if *n > 1024 {
                    return Err(AnalysisError::FixedSizeTooLarge {
                        kind: "map",
                        n: *n,
                        version,
                        span,
                        line,
                    });
                }
                Ok(ResolvedTypeRef::FixedMap(
                    Box::new(self.resolve(k, version, span, line, subst)?),
                    Box::new(self.resolve(v, version, span, line, subst)?),
                    *n,
                ))
            }
            TypeAst::Tuple(elements) => {
                let resolved = elements
                    .iter()
                    .map(|t| self.resolve(t, version, span, line, subst))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(ResolvedTypeRef::Tuple(resolved))
            }
            TypeAst::VFloat { min, max, step } => {
                if !step.is_finite() || *step <= 0.0 {
                    return Err(AnalysisError::InvalidVFloat {
                        reason: "step must be > 0".into(),
                        version,
                        span,
                        line,
                    });
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
                    return Err(AnalysisError::VFloatRangeTooLarge {
                        range,
                        version,
                        span,
                        line,
                    });
                };

                Ok(ResolvedTypeRef::VFloat {
                    min: *min,
                    max: *max,
                    step: *step,
                    backing,
                })
            }
            TypeAst::Optional(v) => {
                let inner_ref = self.resolve(v, version, span, line, subst)?;
                Ok(ResolvedTypeRef::Optional(Box::new(inner_ref)))
            }
            TypeAst::Imported {
                alias,
                version: import_version,
            } => {
                let schema =
                    self.imports
                        .get(alias)
                        .ok_or_else(|| AnalysisError::UnknownImportAlias {
                            alias: alias.clone(),
                            span,
                            line,
                        })?;

                let max_ver = schema.versions.iter().map(|v| v.version).max().unwrap_or(0);
                if *import_version > max_ver {
                    return Err(AnalysisError::ImportVersionOutOfRange {
                        alias: alias.clone(),
                        version: *import_version,
                        max: max_ver,
                        span,
                        line,
                    });
                }

                Ok(ResolvedTypeRef::ImportedSchema {
                    alias: alias.clone(),
                    root_name: schema.name_hint.clone(),
                    version: *import_version,
                })
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

    #[allow(clippy::result_large_err)]
    pub fn finish(self) -> Result<ResolvedSchema, AnalysisError> {
        if self.version_states.is_empty() {
            return Err(AnalysisError::NoVersions {
                span: self.ast.span,
                line: self.ast.line,
            });
        }

        let lineage = SchemaLineage::build_from(&self.version_states);

        Ok(ResolvedSchema {
            name_hint: self.ast.name.clone(),
            versions: self.version_states,
            types: self.type_registry,
            unions: self.union_registry,
            enums: self.enum_registry,
            bitsets: self.bitset_registry,
            imports: self.imports,
            lineage,
        })
    }
}

#[allow(clippy::result_large_err)]
fn coerce_scalar(
    value: DefaultValue,
    name: &str,
    field_name: &str,
    version: i128,
    span: Span,
    line: u32,
) -> Result<DefaultValue, AnalysisError> {
    match normalize_type(name) {
        "bool" => match value {
            DefaultValue::Bool(_) => Ok(value),
            other => type_mismatch("bool", &other, version, span, line),
        },
        "string" => match value {
            DefaultValue::Str(_) => Ok(value),
            other => type_mismatch("string", &other, version, span, line),
        },
        ty @ ("f32" | "f64") => match value {
            DefaultValue::Float(f) => {
                if !f.is_finite() {
                    return type_mismatch(
                        &format!("finite {ty}"),
                        &DefaultValue::Float(f),
                        version,
                        span,
                        line,
                    );
                }
                Ok(DefaultValue::Float(f))
            }
            DefaultValue::Int(i) => Ok(DefaultValue::Float(i as f64)),
            other => type_mismatch(ty, &other, version, span, line),
        },
        int_ty @ ("u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" | "varint32"
        | "varint64") => match value {
            DefaultValue::Int(n) => {
                check_int_range(int_ty, n, field_name, version, span, line)?;
                Ok(DefaultValue::Int(n))
            }
            other => type_mismatch(int_ty, &other, version, span, line),
        },
        other_ty => type_mismatch(other_ty, &value, version, span, line),
    }
}

#[allow(clippy::result_large_err)]
fn check_int_range(
    ty: &str,
    n: i128,
    field_name: &str,
    version: i128,
    span: Span,
    line: u32,
) -> Result<(), AnalysisError> {
    let (lo, hi): (i128, i128) = match ty {
        "u8" => (u8::MIN as i128, u8::MAX as i128),
        "u16" => (u16::MIN as i128, u16::MAX as i128),
        "u32" => (u32::MIN as i128, u32::MAX as i128),
        "u64" => (u64::MIN as i128, u64::MAX as i128),
        "i8" => (i8::MIN as i128, i8::MAX as i128),
        "i16" => (i16::MIN as i128, i16::MAX as i128),
        "i32" => (i32::MIN as i128, i32::MAX as i128),
        "i64" => (i64::MIN as i128, i64::MAX as i128),
        "varint32" => (u32::MIN as i128, u32::MAX as i128),
        "varint64" => (u64::MIN as i128, u64::MAX as i128),
        _ => return Ok(()),
    };
    if n < lo || n > hi {
        return Err(AnalysisError::IntDefaultOutOfRange {
            field: field_name.to_string(),
            value: n,
            min: lo,
            max: hi,
            type_name: ty.to_string(),
            version,
            span,
            line,
        });
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn type_mismatch<T>(
    expected: &str,
    got: &DefaultValue,
    version: i128,
    span: Span,
    line: u32,
) -> Result<T, AnalysisError> {
    Err(AnalysisError::TypeMismatch {
        expected: expected.to_string(),
        got: format!("{:?}", got),
        version,
        span,
        line,
    })
}

#[allow(clippy::result_large_err)]
fn resolve_const(field: &ConstFieldAst, version: i128) -> Result<ResolvedConst, AnalysisError> {
    let span = field.span;
    let line = field.line;

    let rust_type = match &field.ty {
        TypeAst::Named(n) => match normalize_type(n) {
            "u8" => "u8",
            "u16" => "u16",
            "u32" => "u32",
            "u64" => "u64",
            "i8" => "i8",
            "i16" => "i16",
            "i32" => "i32",
            "i64" => "i64",
            "f32" => "f32",
            "f64" => "f64",
            "bool" => "bool",
            "string" => "&'static str",
            "varint32" | "varint64" => {
                return Err(AnalysisError::VarintsCannotBeConst {
                    version,
                    span,
                    line,
                });
            }
            other => {
                return Err(AnalysisError::TypeMismatch {
                    expected: "primitive type".into(),
                    got: other.to_string(),
                    version,
                    span,
                    line,
                });
            }
        },
        _ => {
            return Err(AnalysisError::TypeMismatch {
                expected: "primitive type".into(),
                got: format!("{:?}", field.ty),
                version,
                span,
                line,
            });
        }
    };

    let value = DefaultValue::from(&field.value);
    match (&value, rust_type) {
        (DefaultValue::Bool(_), "bool") => {}
        (DefaultValue::Int(_), "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64") => {}
        (DefaultValue::Float(_), "f32" | "f64") => {}
        (DefaultValue::Str(_), "&'static str") => {}
        _ => {
            return Err(AnalysisError::TypeMismatch {
                expected: rust_type.into(),
                got: format!("{:?}", value),
                version,
                span,
                line,
            });
        }
    }

    Ok(ResolvedConst {
        name: field.name.clone(),
        rust_type,
        value,
    })
}

#[allow(clippy::result_large_err)]
fn check_type_update(
    _old_ty: &ResolvedTypeRef,
    _new_ty: &ResolvedTypeRef,
    _version: i128,
) -> Result<(), AnalysisError> {
    // TODO: validate the cast is sound. Doesn't error today, so no span needed
    // yet — if it starts erroring, thread span/line like everywhere else.
    Ok(())
}

/// Applies a child type def's own `diff` ops to an inherited (and already
/// rename-substituted) field/const list, at the AST level — same op semantics as
/// the old `collect_extended_type`, just operating on unresolved `TypeAst` so it
/// can run before a generic template's params are known.
#[allow(clippy::result_large_err)]
fn apply_template_diff_ops(
    type_name: &str,
    ops: &[DiffAst],
    version: i128,
    fields: &mut Vec<TemplateField>,
    consts: &mut Vec<TemplateConst>,
    id_gen: &mut IdGen,
) -> Result<(), AnalysisError> {
    for op in ops {
        match op {
            DiffAst::Add { field } => {
                if fields.iter().any(|f| f.name == field.name)
                    || consts.iter().any(|c| c.name == field.name)
                {
                    return Err(AnalysisError::FieldAlreadyExists {
                        version,
                        field: field.name.clone(),
                        span: field.span,
                        line: field.line,
                    });
                }

                fields.push(TemplateField {
                    id: id_gen.next_id(),
                    name: field.name.clone(),
                    ty: field.ty.clone(),
                    default: TemplateDefault::AddedViaDiff(field.default.clone()),
                    lazy: field.lazy,
                    span: field.span,
                    line: field.line,
                });
            }

            DiffAst::AddConst { field } => {
                if consts.iter().any(|c| c.name == field.name)
                    || fields.iter().any(|f| f.name == field.name)
                {
                    return Err(AnalysisError::FieldAlreadyExists {
                        version,
                        field: field.name.clone(),
                        span: field.span,
                        line: field.line,
                    });
                }

                consts.push(TemplateConst {
                    name: field.name.clone(),
                    ty: field.ty.clone(),
                    value: field.value.clone(),
                    span: field.span,
                    line: field.line,
                });
            }

            DiffAst::Remove { name, span, line } => {
                let existed_in_fields = fields.iter().any(|f| f.name == *name);
                let existed_in_consts = consts.iter().any(|c| c.name == *name);

                if !existed_in_fields && !existed_in_consts {
                    return Err(AnalysisError::FieldNotFound {
                        op: "remove",
                        field: name.clone(),
                        type_name: type_name.to_string(),
                        version,
                        span: *span,
                        line: *line,
                    });
                }
                fields.retain(|f| f.name != *name);
                consts.retain(|c| c.name != *name);
            }

            DiffAst::Rename {
                from,
                to,
                span,
                line,
            } => {
                if let Some(f) = fields.iter_mut().find(|f| f.name == *from) {
                    f.name = to.clone();
                } else if let Some(c) = consts.iter_mut().find(|c| c.name == *from) {
                    c.name = to.clone();
                } else {
                    return Err(AnalysisError::FieldNotFound {
                        op: "rename",
                        field: from.clone(),
                        type_name: type_name.to_string(),
                        version,
                        span: *span,
                        line: *line,
                    });
                }
            }

            DiffAst::UpdateType {
                name,
                ty,
                lazy,
                span,
                line,
            } => {
                if let Some(f) = fields.iter_mut().find(|f| f.name == *name) {
                    f.ty = ty.clone();
                    f.lazy = *lazy;
                    f.span = *span;
                    f.line = *line;
                } else if consts.iter().any(|c| c.name == *name) {
                } else {
                    return Err(AnalysisError::FieldNotFound {
                        op: "update type",
                        field: name.clone(),
                        type_name: type_name.to_string(),
                        version,
                        span: *span,
                        line: *line,
                    });
                }
            }

            DiffAst::Transform {
                from,
                to,
                ty,
                lazy,
                span,
                line,
            } => {
                if let Some(f) = fields.iter_mut().find(|f| f.name == *from) {
                    f.name = to.clone();
                    if let Some(ty) = ty {
                        f.ty = ty.clone();
                        f.span = *span;
                        f.line = *line;
                    }
                    f.lazy = *lazy;
                } else if let Some(c) = consts.iter_mut().find(|c| c.name == *from) {
                    c.name = to.clone();
                } else {
                    return Err(AnalysisError::FieldNotFound {
                        op: "transform",
                        field: from.clone(),
                        type_name: type_name.to_string(),
                        version,
                        span: *span,
                        line: *line,
                    });
                }
            }

            DiffAst::UpdateConst {
                name,
                ty,
                value,
                span,
                line,
            } => {
                if let Some(c) = consts.iter_mut().find(|c| c.name == *name) {
                    c.ty = ty.clone();
                    c.value = value.clone();
                    c.span = *span;
                    c.line = *line;
                } else {
                    return Err(AnalysisError::FieldNotFound {
                        op: "update const",
                        field: name.clone(),
                        type_name: type_name.to_string(),
                        version,
                        span: *span,
                        line: *line,
                    });
                }
            }

            DiffAst::TransformConst {
                from,
                to,
                ty,
                value,
                span,
                line,
            } => {
                if let Some(c) = consts.iter_mut().find(|c| c.name == *from) {
                    c.name = to.clone();
                    c.ty = ty.clone();
                    c.value = value.clone();
                    c.span = *span;
                    c.line = *line;
                } else {
                    return Err(AnalysisError::FieldNotFound {
                        op: "transform const",
                        field: from.clone(),
                        type_name: type_name.to_string(),
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

/// AST-level substitution used while merging an `extends<...>` chain: replaces
/// occurrences of a renamed ancestor type parameter with either the child's
/// corresponding type expression, or `TypeAst::Wildcard` if it was dropped (`_`).
/// Names that aren't in `rename` (ordinary type references) are left untouched.
fn substitute_ast(ty: &TypeAst, rename: &HashMap<&str, &GenericArgAst>) -> TypeAst {
    match ty {
        TypeAst::Named(n) => match rename.get(n.as_str()) {
            Some(GenericArgAst::Type(replacement)) => replacement.clone(),
            Some(GenericArgAst::Wildcard) => TypeAst::Wildcard,
            None => TypeAst::Named(n.clone()),
        },
        TypeAst::Generic(n, args, alias) => TypeAst::Generic(
            n.clone(),
            args.iter().map(|a| substitute_ast(a, rename)).collect(),
            alias.clone(),
        ),
        TypeAst::Optional(inner) => TypeAst::Optional(Box::new(substitute_ast(inner, rename))),
        TypeAst::Array(inner) => TypeAst::Array(Box::new(substitute_ast(inner, rename))),
        TypeAst::FixedArray(inner, n) => {
            TypeAst::FixedArray(Box::new(substitute_ast(inner, rename)), *n)
        }
        TypeAst::DeltaArray(inner) => TypeAst::DeltaArray(Box::new(substitute_ast(inner, rename))),
        TypeAst::FixedDeltaArray(inner, n) => {
            TypeAst::FixedDeltaArray(Box::new(substitute_ast(inner, rename)), *n)
        }
        TypeAst::Map(k, v) => TypeAst::Map(
            Box::new(substitute_ast(k, rename)),
            Box::new(substitute_ast(v, rename)),
        ),
        TypeAst::FixedMap(k, v, n) => TypeAst::FixedMap(
            Box::new(substitute_ast(k, rename)),
            Box::new(substitute_ast(v, rename)),
            *n,
        ),
        TypeAst::Tuple(elements) => {
            TypeAst::Tuple(elements.iter().map(|e| substitute_ast(e, rename)).collect())
        }
        TypeAst::FixedString(n) => TypeAst::FixedString(*n),
        TypeAst::VFloat { min, max, step } => TypeAst::VFloat {
            min: *min,
            max: *max,
            step: *step,
        },
        TypeAst::Imported { alias, version } => TypeAst::Imported {
            alias: alias.clone(),
            version: *version,
        },
        TypeAst::Wildcard => TypeAst::Wildcard,
    }
}

/// True if a dropped ancestor type parameter (`_` in an `extends<...>` list)
/// still appears anywhere in this type, meaning the child's own `diff` didn't
/// fully remove/retype the fields that used it.
fn type_ast_contains_wildcard(ty: &TypeAst) -> bool {
    match ty {
        TypeAst::Wildcard => true,
        TypeAst::Named(_)
        | TypeAst::FixedString(_)
        | TypeAst::VFloat { .. }
        | TypeAst::Imported { .. } => false,
        TypeAst::Generic(_, args, _) => args.iter().any(type_ast_contains_wildcard),
        TypeAst::Tuple(elements) => elements.iter().any(type_ast_contains_wildcard),
        TypeAst::Optional(inner)
        | TypeAst::Array(inner)
        | TypeAst::FixedArray(inner, _)
        | TypeAst::DeltaArray(inner)
        | TypeAst::FixedDeltaArray(inner, _) => type_ast_contains_wildcard(inner),
        TypeAst::Map(k, v) | TypeAst::FixedMap(k, v, _) => {
            type_ast_contains_wildcard(k) || type_ast_contains_wildcard(v)
        }
    }
}

/// Synthesizes a stable, Rust-identifier-safe name for a monomorphized generic
/// instantiation, e.g. `Box<i32>` -> `BoxI32`, `Pair<i32, string>` -> `PairI32String`.
fn mangle_generic_name(base: &str, args: &[ResolvedTypeRef]) -> String {
    let mut name = base.to_string();
    for arg in args {
        name.push_str(&mangle_type_ref(arg));
    }
    name
}

fn mangle_type_ref(ty: &ResolvedTypeRef) -> String {
    fn pascalize(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut cap_next = true;
        for c in s.chars() {
            if c == '_' {
                cap_next = true;
            } else if cap_next {
                out.extend(c.to_uppercase());
                cap_next = false;
            } else {
                out.push(c);
            }
        }
        out
    }

    match ty {
        ResolvedTypeRef::Scalar(id) => pascalize(&id.name),
        ResolvedTypeRef::Enum(id) => id.name.clone(),
        ResolvedTypeRef::Union(id) => id.name.clone(),
        ResolvedTypeRef::Bitset(id, _) => id.name.clone(),
        ResolvedTypeRef::Array(inner) => format!("{}Array", mangle_type_ref(inner)),
        ResolvedTypeRef::FixedArray(inner, n) => format!("{}Array{n}", mangle_type_ref(inner)),
        ResolvedTypeRef::DeltaArray(inner) => format!("{}DeltaArray", mangle_type_ref(inner)),
        ResolvedTypeRef::FixedDeltaArray(inner, n) => {
            format!("{}DeltaArray{n}", mangle_type_ref(inner))
        }
        ResolvedTypeRef::FixedString(n) => format!("String{n}"),
        ResolvedTypeRef::Map(k, v) => {
            format!("MapOf{}To{}", mangle_type_ref(k), mangle_type_ref(v))
        }
        ResolvedTypeRef::FixedMap(k, v, n) => {
            format!("MapOf{}To{}{n}", mangle_type_ref(k), mangle_type_ref(v))
        }
        ResolvedTypeRef::Tuple(elements) => {
            elements.iter().map(mangle_type_ref).collect::<String>()
        }
        ResolvedTypeRef::VFloat { .. } => "VFloat".to_string(),
        ResolvedTypeRef::Optional(inner) => format!("Opt{}", mangle_type_ref(inner)),
        ResolvedTypeRef::ImportedSchema {
            alias, root_name, ..
        } => format!("{}{}", pascalize(alias), root_name),
    }
}
