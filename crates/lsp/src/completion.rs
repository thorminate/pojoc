use lsp_types::*;
use pojoc_core::types::is_delta_eligible_str;
use pojoc_schema::ast::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Default)]
pub struct SchemaIndex {
    pub type_names: HashSet<String>,
    pub enum_names: HashSet<String>,
    pub union_names: HashSet<String>,
    pub bitset_names: HashSet<String>,
    pub declared_versions: HashMap<String, Vec<i128>>,
    pub enum_variants: HashMap<String, Vec<String>>,
    pub bitset_variants: HashMap<String, Vec<String>>,
    pub union_variants: HashMap<String, Vec<String>>,
    pub enum_variants_at_version: HashMap<(String, i128), Vec<String>>,
    pub bitset_variants_at_version: HashMap<(String, i128), Vec<String>>,
    pub union_variants_at_version: HashMap<(String, i128), Vec<String>>,
    pub fields_before_version: HashMap<i128, Vec<String>>,
    pub type_fields_before_version: HashMap<(String, i128), Vec<String>>,
    pub import_aliases: HashSet<String>,
    pub import_versions: HashMap<String, Vec<i128>>,
    /// Type name -> its declared generic parameter names, if any (`type Box<T>` -> `["T"]`).
    pub generic_params: HashMap<String, Vec<String>>,
    /// `///` doc comment directly above the `schema` header, if any.
    pub schema_doc: Vec<String>,
    pub type_docs: HashMap<String, Vec<String>>,
    pub enum_docs: HashMap<String, Vec<String>>,
    pub union_docs: HashMap<String, Vec<String>>,
    pub bitset_docs: HashMap<String, Vec<String>>,
    /// (owning type name, or `None` for root fields) -> field/const name -> doc.
    pub field_docs: HashMap<(Option<String>, String), Vec<String>>,
    /// (enum/union/bitset name, variant name) -> doc.
    pub variant_docs: HashMap<(String, String), Vec<String>>,
    /// Type name -> its current (name, type, lazy) field shape as declared
    /// in the schema source. Unlike the fully-resolved `ResolvedSchema`,
    /// this is populated even for un-instantiated generic templates (which
    /// have no monomorphized `TypeId` of their own to look up) — the
    /// primary consumer is hover, which falls back to this raw shape when
    /// a type name has no corresponding resolved type.
    pub generic_field_asts: HashMap<String, Vec<(String, TypeAst, bool)>>,
}

impl SchemaIndex {
    pub fn build(ast: &SchemaAst) -> Self {
        let mut idx = SchemaIndex {
            import_aliases: ast.imports.iter().map(|i| i.alias.clone()).collect(),
            schema_doc: ast.doc.clone(),
            ..Default::default()
        };
        let mut running_fields: Vec<String> = Vec::new();
        let mut type_running: HashMap<String, Vec<String>> = HashMap::new();

        for v in &ast.versions {
            idx.fields_before_version
                .insert(v.version, running_fields.clone());

            for block in &v.blocks {
                match block {
                    VersionBlockAst::TypeDef(t) => {
                        idx.type_names.insert(t.name.clone());
                        idx.declared_versions
                            .entry(t.name.clone())
                            .or_default()
                            .push(v.version);
                        if !t.params.is_empty() {
                            idx.generic_params.insert(t.name.clone(), t.params.clone());
                        }
                        match &t.body {
                            TypeBody::Fields(f) => {
                                idx.type_docs.insert(t.name.clone(), t.doc.clone());
                                for fld in &f.fields {
                                    idx.field_docs.insert(
                                        (Some(t.name.clone()), fld.name.clone()),
                                        fld.doc.clone(),
                                    );
                                }
                                for c in &f.const_fields {
                                    idx.field_docs.insert(
                                        (Some(t.name.clone()), c.name.clone()),
                                        c.doc.clone(),
                                    );
                                }
                                idx.generic_field_asts.insert(
                                    t.name.clone(),
                                    f.fields
                                        .iter()
                                        .map(|fld| (fld.name.clone(), fld.ty.clone(), fld.lazy))
                                        .collect(),
                                );
                                let names = field_names(f);
                                type_running.insert(t.name.clone(), names);
                            }
                            TypeBody::Diff(ops) => {
                                if !t.doc.is_empty() {
                                    idx.type_docs.insert(t.name.clone(), t.doc.clone());
                                }
                                let entry = type_running.entry(t.name.clone()).or_default();
                                idx.type_fields_before_version
                                    .insert((t.name.clone(), v.version), entry.clone());
                                apply_diff(entry, ops);
                                apply_diff_docs(&mut idx, Some(&t.name), ops);
                                let field_ast_entry =
                                    idx.generic_field_asts.entry(t.name.clone()).or_default();
                                apply_diff_to_field_asts(field_ast_entry, ops);
                            }
                        }
                    }
                    VersionBlockAst::EnumDef(e) => {
                        idx.enum_names.insert(e.name().to_string());
                        idx.declared_versions
                            .entry(e.name().to_string())
                            .or_default()
                            .push(v.version);
                        apply_enum(&mut idx, e);
                    }
                    VersionBlockAst::UnionDef(u) => {
                        idx.union_names.insert(u.name().to_string());
                        idx.declared_versions
                            .entry(u.name().to_string())
                            .or_default()
                            .push(v.version);
                        apply_union(&mut idx, u);
                    }
                    VersionBlockAst::BitsetDef(b) => {
                        idx.bitset_names.insert(b.name().to_string());
                        idx.declared_versions
                            .entry(b.name().to_string())
                            .or_default()
                            .push(v.version);
                        apply_bitset(&mut idx, b);
                    }
                    VersionBlockAst::Fields(f) => {
                        for fld in &f.fields {
                            idx.field_docs
                                .insert((None, fld.name.clone()), fld.doc.clone());
                        }
                        for c in &f.const_fields {
                            idx.field_docs.insert((None, c.name.clone()), c.doc.clone());
                        }
                        running_fields = field_names(f);
                    }
                    VersionBlockAst::Diff(ops) => {
                        apply_diff(&mut running_fields, ops);
                        apply_diff_docs(&mut idx, None, ops);
                    }
                }
            }

            for (name, variants) in idx.enum_variants.clone() {
                idx.enum_variants_at_version
                    .insert((name, v.version), variants);
            }
            for (name, variants) in idx.bitset_variants.clone() {
                idx.bitset_variants_at_version
                    .insert((name, v.version), variants);
            }
            for (name, variants) in idx.union_variants.clone() {
                idx.union_variants_at_version
                    .insert((name, v.version), variants);
            }
        }

        idx
    }

    pub fn all_type_like_names(&self) -> impl Iterator<Item = &str> {
        self.type_names
            .iter()
            .map(String::as_str)
            .chain(self.enum_names.iter().map(String::as_str))
            .chain(self.union_names.iter().map(String::as_str))
            .chain(self.bitset_names.iter().map(String::as_str))
    }

    pub fn type_like_names_at(&self, version: Option<i128>) -> Vec<&str> {
        self.all_type_like_names()
            .filter(|name| match version {
                None => true,
                Some(ver) => self
                    .declared_versions
                    .get(*name)
                    .is_some_and(|vs| vs.iter().any(|&dv| dv <= ver)),
            })
            .collect()
    }

    pub fn versions_for(&self, name: &str) -> &[i128] {
        self.declared_versions
            .get(name)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

fn field_names(f: &FieldsAst) -> Vec<String> {
    f.fields
        .iter()
        .map(|fld| fld.name.clone())
        .chain(f.const_fields.iter().map(|c| c.name.clone()))
        .collect()
}

fn apply_enum(idx: &mut SchemaIndex, e: &EnumDefAst) {
    match e {
        EnumDefAst::Definition {
            name,
            variants,
            doc,
            ..
        } => {
            idx.enum_docs.insert(name.clone(), doc.clone());
            idx.enum_variants.insert(
                name.clone(),
                variants.iter().map(|v| v.name.clone()).collect(),
            );
            for v in variants {
                idx.variant_docs
                    .insert((name.clone(), v.name.clone()), v.doc.clone());
            }
        }
        EnumDefAst::Extension { name, ops, doc, .. } => {
            if !doc.is_empty() {
                idx.enum_docs.insert(name.clone(), doc.clone());
            }
            let list = idx.enum_variants.entry(name.clone()).or_default();
            for op in ops {
                match op {
                    EnumVariantOpAst::Add {
                        name: n, doc: vdoc, ..
                    } => {
                        list.push(n.clone());
                        idx.variant_docs
                            .insert((name.clone(), n.clone()), vdoc.clone());
                    }
                    EnumVariantOpAst::Rename { from, to, .. } => {
                        if let Some(slot) = list.iter_mut().find(|v| *v == from) {
                            *slot = to.clone();
                        }
                        if let Some(d) = idx.variant_docs.remove(&(name.clone(), from.clone())) {
                            idx.variant_docs.insert((name.clone(), to.clone()), d);
                        }
                    }
                }
            }
        }
    }
}

fn apply_bitset(idx: &mut SchemaIndex, b: &BitsetDefAst) {
    match b {
        BitsetDefAst::Definition {
            name,
            variants,
            doc,
            ..
        } => {
            idx.bitset_docs.insert(name.clone(), doc.clone());
            idx.bitset_variants.insert(
                name.clone(),
                variants.iter().map(|v| v.name.clone()).collect(),
            );
            for v in variants {
                idx.variant_docs
                    .insert((name.clone(), v.name.clone()), v.doc.clone());
            }
        }
        BitsetDefAst::Extension { name, ops, doc, .. } => {
            if !doc.is_empty() {
                idx.bitset_docs.insert(name.clone(), doc.clone());
            }
            let list = idx.bitset_variants.entry(name.clone()).or_default();
            for op in ops {
                match op {
                    BitsetOpAst::Add {
                        name: n, doc: vdoc, ..
                    } => {
                        list.push(n.clone());
                        idx.variant_docs
                            .insert((name.clone(), n.clone()), vdoc.clone());
                    }
                    BitsetOpAst::Remove { name: n, .. } => {
                        list.retain(|v| v != n);
                        idx.variant_docs.remove(&(name.clone(), n.clone()));
                    }
                }
            }
        }
    }
}

fn apply_union(idx: &mut SchemaIndex, u: &UnionDefAst) {
    match u {
        UnionDefAst::Definition {
            name,
            variants,
            doc,
            ..
        } => {
            idx.union_docs.insert(name.clone(), doc.clone());
            idx.union_variants.insert(
                name.clone(),
                variants.iter().map(|v| v.name.clone()).collect(),
            );
            for v in variants {
                idx.variant_docs
                    .insert((name.clone(), v.name.clone()), v.doc.clone());
            }
        }
        UnionDefAst::Extension { name, ops, doc, .. } => {
            if !doc.is_empty() {
                idx.union_docs.insert(name.clone(), doc.clone());
            }
            let list = idx.union_variants.entry(name.clone()).or_default();
            for op in ops {
                match op {
                    UnionVariantOpAst::Add {
                        name: n, doc: vdoc, ..
                    } => {
                        list.push(n.clone());
                        idx.variant_docs
                            .insert((name.clone(), n.clone()), vdoc.clone());
                    }
                }
            }
        }
    }
}

fn apply_diff_docs(idx: &mut SchemaIndex, owner: Option<&str>, ops: &[DiffAst]) {
    let key = |name: &str| (owner.map(str::to_string), name.to_string());
    for op in ops {
        match op {
            DiffAst::Add { field } => {
                idx.field_docs.insert(key(&field.name), field.doc.clone());
            }
            DiffAst::AddConst { field } => {
                idx.field_docs.insert(key(&field.name), field.doc.clone());
            }
            DiffAst::Remove { name, .. } => {
                idx.field_docs.remove(&key(name));
            }
            DiffAst::Rename { from, to, .. } | DiffAst::Transform { from, to, .. } => {
                if let Some(doc) = idx.field_docs.remove(&key(from)) {
                    idx.field_docs.insert(key(to), doc);
                }
            }
            DiffAst::TransformConst { from, to, .. } => {
                if let Some(doc) = idx.field_docs.remove(&key(from)) {
                    idx.field_docs.insert(key(to), doc);
                }
            }
            DiffAst::UpdateType { .. } | DiffAst::UpdateConst { .. } => {}
        }
    }
}

fn apply_diff(fields: &mut Vec<String>, ops: &[DiffAst]) {
    for op in ops {
        match op {
            DiffAst::Add { field } => fields.push(field.name.clone()),
            DiffAst::AddConst { field } => fields.push(field.name.clone()),
            DiffAst::Remove { name, .. } => fields.retain(|f| f != name),
            DiffAst::Rename { from, to, .. } | DiffAst::Transform { from, to, .. } => {
                if let Some(slot) = fields.iter_mut().find(|f| *f == from) {
                    *slot = to.clone();
                }
            }
            DiffAst::TransformConst { from, to, .. } => {
                if let Some(slot) = fields.iter_mut().find(|f| *f == from) {
                    *slot = to.clone();
                }
            }
            DiffAst::UpdateType { .. } | DiffAst::UpdateConst { .. } => {}
        }
    }
}

fn apply_diff_to_field_asts(fields: &mut Vec<(String, TypeAst, bool)>, ops: &[DiffAst]) {
    for op in ops {
        match op {
            DiffAst::Add { field } => {
                fields.push((field.name.clone(), field.ty.clone(), field.lazy));
            }
            DiffAst::Remove { name, .. } => fields.retain(|(n, _, _)| n != name),
            DiffAst::Rename { from, to, .. } => {
                if let Some(f) = fields.iter_mut().find(|(n, _, _)| n == from) {
                    f.0 = to.clone();
                }
            }
            DiffAst::UpdateType { name, ty, lazy, .. } => {
                if let Some(f) = fields.iter_mut().find(|(n, _, _)| n == name) {
                    f.1 = ty.clone();
                    f.2 = *lazy;
                }
            }
            DiffAst::Transform {
                from, to, ty, lazy, ..
            } => {
                if let Some(f) = fields.iter_mut().find(|(n, _, _)| n == from) {
                    f.0 = to.clone();
                    if let Some(ty) = ty {
                        f.1 = ty.clone();
                    }
                    f.2 = *lazy;
                }
            }
            DiffAst::AddConst { .. }
            | DiffAst::TransformConst { .. }
            | DiffAst::UpdateConst { .. } => {}
        }
    }
}

// --- cursor-position context detection ---------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tok {
    Ident(String),
    Number(String),
    Punct(char),
    DoubleColon,
    Newline,
}

pub(crate) fn tokenize_prefix(src: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '\n' {
            toks.push(Tok::Newline);
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == '"' {
            i += 1;
            while i < bytes.len() && bytes[i] as char != '"' {
                if bytes[i] as char == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len()
                && (bytes[i] as char == '_' || (bytes[i] as char).is_alphanumeric())
            {
                i += 1;
            }
            toks.push(Tok::Ident(src[start..i].to_string()));
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len()
                && ((bytes[i] as char).is_ascii_digit() || bytes[i] as char == '.')
            {
                i += 1;
            }
            toks.push(Tok::Number(src[start..i].to_string()));
            continue;
        }
        if c == ':' && bytes.get(i + 1) == Some(&b':') {
            toks.push(Tok::DoubleColon);
            i += 2;
            continue;
        }
        toks.push(Tok::Punct(c));
        i += 1;
    }
    toks
}

#[derive(Debug, Clone)]
pub(crate) enum BlockKind {
    Root,
    #[allow(dead_code)]
    Schema(String),
    Version(i128),
    TypeDef(String),
    EnumDef(String),
    UnionDef(String),
    BitsetDef(String),
    Fields,
    Diff,
    Other,
}

pub(crate) struct ScanState {
    pub(crate) stack: Vec<BlockKind>,
    pub(crate) pending: Vec<Tok>,
    consumed: Vec<HashSet<String>>,
}

pub(crate) fn scan(tokens: &[Tok]) -> ScanState {
    let mut stack = vec![BlockKind::Root];
    let mut consumed: Vec<HashSet<String>> = vec![HashSet::new()];
    let mut pending: Vec<Tok> = Vec::new();
    let mut depth = 0i32;

    for tok in tokens {
        match tok {
            Tok::Punct('{') => {
                stack.push(classify_header(&pending));
                consumed.push(HashSet::new());
                pending.clear();
                depth = 0;
            }
            Tok::Punct('}') => {
                if stack.len() > 1 {
                    stack.pop();
                    consumed.pop();
                }
                pending.clear();
                depth = 0;
            }
            Tok::Newline => {
                if depth == 0 {
                    if matches!(stack.last(), Some(BlockKind::Diff))
                        && let [Tok::Punct(op), Tok::Ident(name), ..] = pending.as_slice()
                        && matches!(op, '-' | '~')
                    {
                        consumed.last_mut().unwrap().insert(name.clone());
                    }
                    pending.clear();
                }
            }
            Tok::Punct(c @ ('(' | '[' | '<')) => {
                depth += 1;
                pending.push(Tok::Punct(*c));
            }
            Tok::Punct(c @ (')' | ']' | '>')) => {
                depth -= 1;
                pending.push(Tok::Punct(*c));
            }
            other => pending.push(other.clone()),
        }
        if pending.len() > 16 {
            pending.remove(0);
        }
    }

    ScanState {
        stack,
        pending,
        consumed,
    }
}

fn classify_header(pending: &[Tok]) -> BlockKind {
    match pending {
        [Tok::Ident(k), Tok::Ident(name), ..] if k == "schema" => BlockKind::Schema(name.clone()),
        [Tok::Ident(k), Tok::Number(n), ..] if k == "version" => {
            BlockKind::Version(n.parse().unwrap_or(0))
        }
        [Tok::Ident(k), Tok::Ident(name), ..] if k == "type" => BlockKind::TypeDef(name.clone()),
        [Tok::Ident(k), Tok::Ident(name), ..] if k == "enum" => BlockKind::EnumDef(name.clone()),
        [Tok::Ident(k), Tok::Ident(name), ..] if k == "union" => BlockKind::UnionDef(name.clone()),
        [Tok::Ident(k), Tok::Ident(name), ..] if k == "bitset" => {
            BlockKind::BitsetDef(name.clone())
        }
        [Tok::Ident(k), ..] if k == "fields" => BlockKind::Fields,
        [Tok::Ident(k), ..] if k == "diff" => BlockKind::Diff,
        _ => BlockKind::Other,
    }
}

fn current_version(stack: &[BlockKind]) -> Option<i128> {
    stack.iter().rev().find_map(|f| {
        if let BlockKind::Version(v) = f {
            Some(*v)
        } else {
            None
        }
    })
}

/// The generic parameters of the `type` def we're currently nested inside
/// (its own `<T, U>` list), if any — so a field type inside `type Box<T> { ... }`
/// can suggest `T` alongside ordinary types.
fn enclosing_type_params(stack: &[BlockKind], idx: &SchemaIndex) -> Vec<String> {
    stack
        .iter()
        .rev()
        .find_map(|frame| match frame {
            BlockKind::TypeDef(name) => idx.generic_params.get(name),
            _ => None,
        })
        .cloned()
        .unwrap_or_default()
}

fn find_diff_context(stack: &[BlockKind]) -> Option<(Option<String>, i128)> {
    if !matches!(stack.last(), Some(BlockKind::Diff)) {
        return None;
    }
    let mut owner = None;
    let mut version = None;
    for frame in stack.iter().rev() {
        match frame {
            BlockKind::TypeDef(name) if owner.is_none() => owner = Some(name.clone()),
            BlockKind::Version(v) if version.is_none() => version = Some(*v),
            _ => {}
        }
    }
    version.map(|v| (owner, v))
}

/// Identifier immediately preceding the innermost still-open bracket, if any,
/// plus whether that identifier was itself immediately preceded by `extends`
/// (i.e. this bracket is an `extends Name<...>` argument list, not some other
/// use of `<...>` such as a field-type instantiation or `map<K, V>`).
fn enclosing_call(pending: &[Tok]) -> Option<(Option<String>, char, bool)> {
    let mut stack: Vec<(Option<String>, char, bool)> = Vec::new();
    let mut last_ident: Option<String> = None;
    let mut prev_ident: Option<String> = None;
    for tok in pending {
        match tok {
            Tok::Ident(s) => {
                prev_ident = last_ident.take();
                last_ident = Some(s.clone());
            }
            Tok::Punct(c @ ('(' | '[' | '<')) => {
                let is_extends_args = matches!(&prev_ident, Some(p) if p == "extends");
                stack.push((last_ident.take(), *c, is_extends_args));
                prev_ident = None;
            }
            Tok::Punct(')' | ']' | '>') => {
                stack.pop();
            }
            Tok::Punct(',') | Tok::Punct(':') => {}
            _ => {
                last_ident = None;
                prev_ident = None;
            }
        }
    }
    stack.last().cloned()
}

enum Ctx {
    FileRoot,
    SchemaBody {
        next_version: i128,
    },
    VersionBody(i128),
    DiffLineStart,
    DiffOldFieldName {
        owner_type: Option<String>,
        version: i128,
        already_used: HashSet<String>,
    },
    TypePosition {
        version: Option<i128>,
        /// The enclosing `type Name<...>`'s own parameters, if we're
        /// currently nested inside one — valid types at this position too.
        own_params: Vec<String>,
    },
    ExtendsName(Option<i128>),
    /// Inside the `<...>` argument list of an `extends Name<...>@V` clause —
    /// like `TypePosition`, but `_` (drop this ancestor parameter) is also valid.
    ExtendsGenericArgs {
        version: Option<i128>,
        own_params: Vec<String>,
    },
    ArrayElementType {
        delta: bool,
        version: Option<i128>,
        own_params: Vec<String>,
    },
    ExtendsVersion {
        name: String,
        before_version: Option<i128>,
    },
    ImportVersion {
        alias: String,
    },
    ImportPath {
        dir_prefix: String,
        partial: String,
    },
    VariantAccess {
        name: String,
        version: Option<i128>,
    },
    VFloatParam,
    BitsetLiteralField {
        bitset_name: String,
        used: HashSet<String>,
        version: Option<i128>,
    },
    BitsetLiteralValue,
    ArraySuffixModifier {
        delta_eligible: bool,
    },
    DefaultValue(DefaultKind),
    Unknown,
}

fn determine_ctx(state: &ScanState, idx: &SchemaIndex) -> Ctx {
    let pending = &state.pending;
    let stack = &state.stack;

    if let Some(type_tokens) = type_tokens_before_default(pending)
        && let Some(kind) = classify_default_type(type_tokens, idx, current_version(stack))
    {
        return Ctx::DefaultValue(kind);
    }

    if let [.., Tok::Ident(name), Tok::Punct('@')] = pending.as_slice() {
        let preceded_by_extends = pending.len() >= 3
            && matches!(&pending[pending.len() - 3], Tok::Ident(k) if k == "extends");

        if preceded_by_extends {
            return Ctx::ExtendsVersion {
                name: name.clone(),
                before_version: current_version(stack),
            };
        }
        if idx.import_aliases.contains(name) {
            return Ctx::ImportVersion {
                alias: name.clone(),
            };
        }
        return Ctx::Unknown;
    }

    if matches!(
        pending.as_slice(),
        [.., Tok::Ident(_), Tok::DoubleColon]
            | [.., Tok::Ident(_), Tok::DoubleColon, Tok::Ident(_)]
    ) {
        let name = match pending.as_slice() {
            [.., Tok::Ident(n), Tok::DoubleColon] => n.clone(),
            [.., Tok::Ident(n), Tok::DoubleColon, Tok::Ident(_)] => n.clone(),
            _ => unreachable!(),
        };
        return Ctx::VariantAccess {
            name,
            version: current_version(stack),
        };
    }

    if let Some(info) = array_suffix_info(pending) {
        let eligible = match &info.element {
            ArrayElement::Scalar(name) => is_delta_eligible_str(name),
            ArrayElement::Other => false,
        };
        return Ctx::ArraySuffixModifier {
            delta_eligible: eligible && !info.already_has_delta,
        };
    }

    if bitset_literal_value_info(pending, idx).is_some() {
        return Ctx::BitsetLiteralValue;
    }

    if let Some((owner, bracket, is_extends_args)) = enclosing_call(pending) {
        match (owner.as_deref(), bracket) {
            (Some("vfloat"), '(') => return Ctx::VFloatParam,
            (Some(name), '(') if idx.bitset_names.contains(name) => {
                let used = find_enclosing_open_paren(pending)
                    .map(|open_idx| used_bitset_flags(pending, open_idx))
                    .unwrap_or_default();
                return Ctx::BitsetLiteralField {
                    bitset_name: name.to_string(),
                    used,
                    version: current_version(stack),
                };
            }
            (_, '[') => {
                return Ctx::ArrayElementType {
                    delta: false,
                    version: current_version(stack),
                    own_params: enclosing_type_params(stack, idx),
                };
            }
            (_, '<') if is_extends_args => {
                return Ctx::ExtendsGenericArgs {
                    version: current_version(stack),
                    own_params: enclosing_type_params(stack, idx),
                };
            }
            // `type Name<...>` declaring its own parameter list — fresh
            // identifiers, not references, so there's nothing to suggest.
            (_, '<') if matches!(pending.first(), Some(Tok::Ident(k)) if k == "type") => {
                return Ctx::Unknown;
            }
            (_, '<') => {
                return Ctx::TypePosition {
                    version: current_version(stack),
                    own_params: enclosing_type_params(stack, idx),
                };
            }
            _ => {}
        }
    }

    if matches!(pending.last(), Some(Tok::Ident(k)) if k == "extends") {
        return Ctx::ExtendsName(current_version(stack));
    }
    if pending.len() >= 2
        && let (Tok::Ident(k), Tok::Ident(_)) =
            (&pending[pending.len() - 2], &pending[pending.len() - 1])
        && k == "extends"
    {
        return Ctx::ExtendsName(current_version(stack));
    }

    if matches!(pending.last(), Some(Tok::Punct(':'))) {
        return Ctx::TypePosition {
            version: current_version(stack),
            own_params: enclosing_type_params(stack, idx),
        };
    }

    if pending.len() >= 2
        && let (Tok::Punct(':'), Tok::Ident(_)) =
            (&pending[pending.len() - 2], &pending[pending.len() - 1])
    {
        return Ctx::TypePosition {
            version: current_version(stack),
            own_params: enclosing_type_params(stack, idx),
        };
    }

    if let Some((owner, version)) = find_diff_context(stack) {
        match pending.as_slice() {
            [] => return Ctx::DiffLineStart,
            [Tok::Punct('-' | '~')] => {
                return Ctx::DiffOldFieldName {
                    owner_type: owner,
                    version,
                    already_used: state.consumed.last().cloned().unwrap_or_default(),
                };
            }
            [Tok::Punct('-' | '~'), Tok::Ident(_)] => {
                return Ctx::DiffOldFieldName {
                    owner_type: owner,
                    version,
                    already_used: state.consumed.last().cloned().unwrap_or_default(),
                };
            }
            _ => {}
        }
    }

    if matches!(stack.last(), Some(BlockKind::Schema(_))) && pending.is_empty() {
        let next_version = idx
            .fields_before_version
            .keys()
            .max()
            .copied()
            .map(|v| v + 1)
            .unwrap_or(1);
        return Ctx::SchemaBody { next_version };
    }

    if let Some(BlockKind::Version(version)) = stack.last()
        && pending.is_empty()
    {
        return Ctx::VersionBody(*version);
    }

    if matches!(stack.last(), Some(BlockKind::Root)) && pending.is_empty() {
        return Ctx::FileRoot;
    }

    Ctx::Unknown
}

pub fn completions_for_position(
    text: &str,
    offset: usize,
    idx: &SchemaIndex,
    schema_path: Option<&Path>,
) -> Vec<CompletionItem> {
    // prevent IntelliSense in a comment
    if cursor_in_line_comment(text, offset) {
        return Vec::new();
    }

    let ctx = import_path_ctx(text, offset).unwrap_or_else(|| {
        let prefix = &text[..offset.min(text.len())];
        let tokens = tokenize_prefix(prefix);
        let state = scan(&tokens);
        let mut ctx = determine_ctx(&state, idx);
        if let Ctx::ArrayElementType {
            version,
            own_params,
            ..
        } = ctx
        {
            ctx = Ctx::ArrayElementType {
                delta: suffix_has_delta(text, offset),
                version,
                own_params,
            };
        }
        ctx
    });
    let has_value_after = cursor_already_has_value(text, offset);

    match ctx {
        Ctx::VersionBody(v) => {
            let mut snippets = vec![
                snippet("type", "type $1 {\n\t$0\n}", "define a type"),
                snippet("enum", "enum $1 {\n\t$0\n}", "define an enum"),
                snippet("union", "union $1 {\n\t$0\n}", "define a union"),
                snippet("bitset", "bitset $1 {\n\t$0\n}", "define a bitset"),
            ];

            if v == 1 {
                snippets.insert(
                    0,
                    snippet("fields", "fields {\n\t$0\n}", "field declarations block"),
                );
            } else if v > 1 {
                snippets.insert(0, snippet("diff", "diff {\n\t$0\n}", "schema diff block"));
            }

            snippets
        }

        Ctx::DiffLineStart => ["+", "-", "~"]
            .iter()
            .map(|op| CompletionItem {
                label: op.to_string(),
                kind: Some(CompletionItemKind::OPERATOR),
                detail: Some(match *op {
                    "+" => "add a field/variant".into(),
                    "-" => "remove a field/variant".into(),
                    _ => "rename or retype a field/variant".into(),
                }),
                ..Default::default()
            })
            .collect(),

        Ctx::DiffOldFieldName {
            owner_type,
            version,
            already_used,
        } => {
            let names: &[String] = match &owner_type {
                Some(ty) => idx
                    .type_fields_before_version
                    .get(&(ty.clone(), version))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                None => idx
                    .fields_before_version
                    .get(&version)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            };
            names
                .iter()
                .filter(|n| !already_used.contains(*n))
                .map(|n| CompletionItem {
                    label: n.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    documentation: idx
                        .field_docs
                        .get(&(owner_type.clone(), n.clone()))
                        .and_then(|d| doc_to_documentation(d)),
                    ..Default::default()
                })
                .collect()
        }

        Ctx::ExtendsName(version) => idx
            .type_like_names_at(version)
            .into_iter()
            .map(|n| CompletionItem {
                label: n.to_string(),
                kind: Some(CompletionItemKind::CLASS),
                documentation: type_like_doc(idx, n).and_then(|d| doc_to_documentation(d)),
                ..Default::default()
            })
            .collect(),

        Ctx::ExtendsVersion {
            name,
            before_version,
        } => idx
            .versions_for(&name)
            .iter()
            .filter(|v| before_version.is_none_or(|bv| **v < bv))
            .map(|v| CompletionItem {
                label: v.to_string(),
                kind: Some(CompletionItemKind::CONSTANT),
                ..Default::default()
            })
            .collect(),

        Ctx::ImportVersion { alias } => idx
            .import_versions
            .get(&alias)
            .into_iter()
            .flatten()
            .map(|v| CompletionItem {
                label: v.to_string(),
                kind: Some(CompletionItemKind::CONSTANT),
                detail: Some(format!("version of imported schema `{alias}`")),
                ..Default::default()
            })
            .collect(),

        Ctx::ImportPath {
            dir_prefix,
            partial,
        } => match schema_path {
            Some(path) => import_path_completions(path, &dir_prefix, &partial),
            None => Vec::new(),
        },

        Ctx::VariantAccess { name, version } => {
            let variants: &[String] = if idx.enum_names.contains(&name) {
                version
                    .and_then(|v| idx.enum_variants_at_version.get(&(name.clone(), v)))
                    .or_else(|| idx.enum_variants.get(&name))
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
            } else if idx.union_names.contains(&name) {
                version
                    .and_then(|v| idx.union_variants_at_version.get(&(name.clone(), v)))
                    .or_else(|| idx.union_variants.get(&name))
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
            } else {
                &[]
            };

            variants
                .iter()
                .map(|v| CompletionItem {
                    label: v.clone(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    documentation: idx
                        .variant_docs
                        .get(&(name.clone(), v.clone()))
                        .and_then(|d| doc_to_documentation(d)),
                    ..Default::default()
                })
                .collect()
        }

        Ctx::TypePosition {
            version,
            own_params,
        } => type_position_items(idx, version, &own_params),

        Ctx::ExtendsGenericArgs {
            version,
            own_params,
        } => {
            let mut items = type_position_items(idx, version, &own_params);
            items.push(CompletionItem {
                label: "_".into(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("drop this ancestor type parameter".into()),
                ..Default::default()
            });
            items
        }

        Ctx::ArrayElementType { delta: true, .. } => {
            const NUMBER_PRIMITIVES: &[&str] = &[
                "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "varint32",
                "varint64",
            ];
            NUMBER_PRIMITIVES
                .iter()
                .map(|p| CompletionItem {
                    label: p.to_string(),
                    kind: Some(CompletionItemKind::TYPE_PARAMETER),
                    ..Default::default()
                })
                .collect()
        }
        Ctx::ArrayElementType {
            delta: false,
            version,
            own_params,
        } => type_position_items(idx, version, &own_params),

        Ctx::VFloatParam => ["min", "max", "step"]
            .iter()
            .map(|p| CompletionItem {
                label: format!("{p}:"),
                insert_text: Some(format!("{p}: $0")),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                kind: Some(CompletionItemKind::PROPERTY),
                ..Default::default()
            })
            .collect(),

        Ctx::BitsetLiteralField {
            bitset_name,
            used,
            version,
        } => {
            let variants: &[String] = version
                .and_then(|v| {
                    idx.bitset_variants_at_version
                        .get(&(bitset_name.clone(), v))
                })
                .or_else(|| idx.bitset_variants.get(&bitset_name))
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            variants
                .iter()
                .filter(|v| !used.contains(*v))
                .map(|v| {
                    let documentation = idx
                        .variant_docs
                        .get(&(bitset_name.clone(), v.clone()))
                        .and_then(|d| doc_to_documentation(d));
                    if has_value_after {
                        CompletionItem {
                            label: v.clone(),
                            kind: Some(CompletionItemKind::PROPERTY),
                            documentation,
                            ..Default::default()
                        }
                    } else {
                        CompletionItem {
                            label: format!("{v}:"),
                            insert_text: Some(format!("{v}: $0")),
                            insert_text_format: Some(InsertTextFormat::SNIPPET),
                            kind: Some(CompletionItemKind::PROPERTY),
                            documentation,
                            ..Default::default()
                        }
                    }
                })
                .collect()
        }

        Ctx::ArraySuffixModifier { delta_eligible } => {
            if delta_eligible {
                vec![
                    CompletionItem {
                        label: "delta".to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        detail: Some("delta-encode this array (integer elements only)".into()),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "delta, size".to_string(),
                        insert_text: Some("delta, $1".into()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        kind: Some(CompletionItemKind::SNIPPET),
                        detail: Some("fixed-size delta array".into()),
                        ..Default::default()
                    },
                ]
            } else {
                Vec::new()
            }
        }
        Ctx::FileRoot => vec![snippet(
            "schema",
            "schema $1 {\n\t$0\n}",
            "define a new schema",
        )],
        Ctx::SchemaBody { next_version } => vec![
            snippet(
                &format!("version {next_version}"),
                &format!("version {next_version} {{\n\t$0\n}}"),
                &format!("add version {next_version} block"),
            ),
            snippet("import", "import \"$1\" as $2", "import another schema"),
        ],
        Ctx::DefaultValue(kind) => match kind {
            DefaultKind::Bool => ["true", "false"]
                .iter()
                .map(|v| CompletionItem {
                    label: v.to_string(),
                    kind: Some(CompletionItemKind::VALUE),
                    ..Default::default()
                })
                .collect(),

            DefaultKind::Str => vec![CompletionItem {
                label: "\"\"".into(),
                insert_text: Some("\"$0\"".into()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                kind: Some(CompletionItemKind::VALUE),
                ..Default::default()
            }],

            DefaultKind::Array => vec![CompletionItem {
                label: "[]".into(),
                insert_text: Some("[$0]".into()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                kind: Some(CompletionItemKind::VALUE),
                ..Default::default()
            }],

            DefaultKind::Map => vec![CompletionItem {
                label: "{}".into(),
                insert_text: Some("{$0}".into()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                kind: Some(CompletionItemKind::VALUE),
                ..Default::default()
            }],

            DefaultKind::Enum(name, version) => {
                let variants: &[String] = version
                    .and_then(|v| idx.enum_variants_at_version.get(&(name.clone(), v)))
                    .or_else(|| idx.enum_variants.get(&name))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);

                variants
                    .iter()
                    .map(|v| CompletionItem {
                        label: format!("{name}::{v}"),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        documentation: idx
                            .variant_docs
                            .get(&(name.clone(), v.clone()))
                            .and_then(|d| doc_to_documentation(d)),
                        ..Default::default()
                    })
                    .collect()
            }

            DefaultKind::Bitset(name) => vec![
                CompletionItem {
                    label: "0".into(),
                    detail: Some("no flags set".into()),
                    kind: Some(CompletionItemKind::VALUE),
                    ..Default::default()
                },
                CompletionItem {
                    label: format!("{name}(...)"),
                    insert_text: Some(format!("{name}($1: $2)")),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    kind: Some(CompletionItemKind::SNIPPET),
                    detail: Some("bitset literal".into()),
                    ..Default::default()
                },
            ],

            DefaultKind::VFloat(min) => {
                let v = min.unwrap_or_else(|| "0.0".into());
                vec![CompletionItem {
                    label: v.clone(),
                    insert_text: Some(v),
                    kind: Some(CompletionItemKind::VALUE),
                    detail: Some("min value".into()),
                    ..Default::default()
                }]
            }
        },
        Ctx::BitsetLiteralValue => ["true", "false"]
            .iter()
            .map(|v| CompletionItem {
                label: v.to_string(),
                kind: Some(CompletionItemKind::VALUE),
                ..Default::default()
            })
            .collect(),
        Ctx::Unknown => Vec::new(),
    }
}

fn snippet(label: &str, insert: &str, detail: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        insert_text: Some(insert.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(detail.to_string()),
        ..Default::default()
    }
}

/// Builds a `Box<$1>`/`Pair<$1, $2>`-style snippet for instantiating a
/// generic type, one tab stop per declared parameter (mirrors the
/// `map<$1, $2>` snippet below).
fn generic_instantiation_item(
    name: &str,
    params: &[String],
    documentation: Option<Documentation>,
) -> CompletionItem {
    let placeholders = (1..=params.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    CompletionItem {
        label: format!("{name}<{}>", params.join(", ")),
        insert_text: Some(format!("{name}<{placeholders}>")),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some("generic type".into()),
        documentation,
        ..Default::default()
    }
}

/// Doc comment lookup across every type-like namespace (structs, enums,
/// unions, bitsets all share one name space).
fn type_like_doc<'a>(idx: &'a SchemaIndex, name: &str) -> Option<&'a Vec<String>> {
    idx.type_docs
        .get(name)
        .or_else(|| idx.enum_docs.get(name))
        .or_else(|| idx.union_docs.get(name))
        .or_else(|| idx.bitset_docs.get(name))
}

fn doc_to_documentation(doc: &[String]) -> Option<Documentation> {
    if doc.is_empty() {
        None
    } else {
        Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc.join("\n"),
        }))
    }
}

fn type_position_items(
    idx: &SchemaIndex,
    version: Option<i128>,
    own_params: &[String],
) -> Vec<CompletionItem> {
    const PRIMITIVES: &[&str] = &[
        "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "varint32", "varint64",
        "bool", "string",
    ];

    // The enclosing generic type's own parameters (e.g. `T` inside
    // `type Box<T> { value: | }`) — offered first, since they're the most
    // contextually relevant.
    let mut items: Vec<CompletionItem> = own_params
        .iter()
        .map(|p| CompletionItem {
            label: p.clone(),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            detail: Some("type parameter".into()),
            ..Default::default()
        })
        .collect();

    items.extend(PRIMITIVES.iter().map(|p| CompletionItem {
        label: p.to_string(),
        kind: Some(CompletionItemKind::TYPE_PARAMETER),
        ..Default::default()
    }));

    items.extend(idx.type_like_names_at(version).into_iter().map(|n| {
        let documentation = type_like_doc(idx, n).and_then(|d| doc_to_documentation(d));
        match idx.generic_params.get(n) {
            Some(params) if !params.is_empty() => {
                generic_instantiation_item(n, params, documentation)
            }
            _ => CompletionItem {
                label: n.to_string(),
                kind: Some(CompletionItemKind::CLASS),
                documentation,
                ..Default::default()
            },
        }
    }));

    items.extend(idx.import_aliases.iter().map(|n| CompletionItem {
        label: n.clone() + "@",
        kind: Some(CompletionItemKind::MODULE),
        detail: Some("imported schema".into()),
        ..Default::default()
    }));

    items.push(CompletionItem {
        label: "lazy".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..Default::default()
    });
    items.push(CompletionItem {
        label: "const".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..Default::default()
    });
    items.push(CompletionItem {
        label: "vfloat(min, max, step)".into(),
        insert_text: Some("vfloat(min: $1, max: $2, step: $3)".into()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        kind: Some(CompletionItemKind::SNIPPET),
        ..Default::default()
    });
    items.push(CompletionItem {
        label: "map<K, V>".into(),
        insert_text: Some("map<$1, $2>".into()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        kind: Some(CompletionItemKind::SNIPPET),
        ..Default::default()
    });

    items
}

#[derive(Debug, Clone)]
enum ArrayElement {
    Scalar(String),
    Other,
}

struct ArraySuffixInfo {
    element: ArrayElement,
    already_has_delta: bool,
}

/// Detects `[T](...)` with the cursor inside the parens. Returns the
/// element type found between the matching `[`/`]` so delta-eligibility
/// can be checked, plus whether `delta` is already typed in there.
fn array_suffix_info(pending: &[Tok]) -> Option<ArraySuffixInfo> {
    let mut open_stack: Vec<(usize, char)> = Vec::new();
    for (i, tok) in pending.iter().enumerate() {
        match tok {
            Tok::Punct(c @ ('(' | '[' | '<')) => open_stack.push((i, *c)),
            Tok::Punct(')' | ']' | '>') => {
                open_stack.pop();
            }
            _ => {}
        }
    }
    let &(open_paren_idx, bracket) = open_stack.last()?;
    if bracket != '(' || open_paren_idx == 0 {
        return None;
    }
    if !matches!(&pending[open_paren_idx - 1], Tok::Punct(']')) {
        return None;
    }

    let mut depth = 0i32;
    let mut open_bracket_idx = None;
    for i in (0..open_paren_idx).rev() {
        match &pending[i] {
            Tok::Punct(']') => depth += 1,
            Tok::Punct('[') => {
                depth -= 1;
                if depth == 0 {
                    open_bracket_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let open_bracket_idx = open_bracket_idx?;

    let element = match &pending[open_bracket_idx + 1..open_paren_idx - 1] {
        [Tok::Ident(name)] => ArrayElement::Scalar(name.clone()),
        _ => ArrayElement::Other, // tuple, map, nested array, etc. — never delta-eligible
    };

    let already_has_delta = pending[open_paren_idx + 1..]
        .iter()
        .any(|t| matches!(t, Tok::Ident(s) if s == "delta"));

    Some(ArraySuffixInfo {
        element,
        already_has_delta,
    })
}

#[derive(Debug, Clone)]
enum DefaultKind {
    Bool,
    Str,
    Array,
    Map,
    Enum(String, Option<i128>),
    Bitset(String),
    VFloat(Option<String>), // Some(min) when extractable
}

/// Tokens between the field's `:` and the `=` it's about to default, if the
/// cursor is sitting right after that `=` (optionally with a partial value typed).
fn type_tokens_before_default(pending: &[Tok]) -> Option<&[Tok]> {
    let eq_idx = if matches!(pending.last(), Some(Tok::Punct('='))) {
        pending.len() - 1
    } else if pending.len() >= 2
        && matches!(pending[pending.len() - 1], Tok::Ident(_) | Tok::Number(_))
        && matches!(pending[pending.len() - 2], Tok::Punct('='))
    {
        pending.len() - 2
    } else {
        return None;
    };

    let mut depth = 0i32;
    let mut colon_idx = None;
    for i in (0..eq_idx).rev() {
        match &pending[i] {
            Tok::Punct(')' | ']' | '>') => depth += 1,
            Tok::Punct('(' | '[' | '<') => depth -= 1,
            Tok::Punct(':') if depth == 0 => {
                colon_idx = Some(i);
                break;
            }
            _ => {}
        }
    }
    Some(&pending[colon_idx? + 1..eq_idx])
}

fn extract_vfloat_min(tokens: &[Tok]) -> Option<String> {
    tokens.windows(3).find_map(|w| match w {
        [Tok::Ident(k), Tok::Punct(':'), Tok::Number(v)] if k == "min" => Some(v.clone()),
        _ => None,
    })
}

fn classify_default_type(
    tokens: &[Tok],
    idx: &SchemaIndex,
    version: Option<i128>,
) -> Option<DefaultKind> {
    let mut tokens = tokens;
    while let Some(Tok::Ident(k)) = tokens.first() {
        if k == "const" || k == "lazy" {
            tokens = &tokens[1..];
        } else {
            break;
        }
    }

    match tokens.first()? {
        Tok::Punct('[') => return Some(DefaultKind::Array),
        Tok::Ident(name) => match name.as_str() {
            "bool" => return Some(DefaultKind::Bool),
            "string" => return Some(DefaultKind::Str),
            "map" => return Some(DefaultKind::Map),
            "vfloat" => return Some(DefaultKind::VFloat(extract_vfloat_min(tokens))),
            _ if idx.enum_names.contains(name) => {
                return Some(DefaultKind::Enum(name.clone(), version));
            }
            _ if idx.bitset_names.contains(name) => return Some(DefaultKind::Bitset(name.clone())),
            _ => {}
        },
        _ => {}
    }
    None
}

/// True if the cursor sits right after `Flag:` (optionally with a partial
/// `true`/`false` already typed) inside a known bitset's `Name(...)` literal.
fn bitset_literal_value_info(pending: &[Tok], idx: &SchemaIndex) -> Option<String> {
    let n = pending.len();
    let colon_idx = if n >= 2 && matches!(pending[n - 1], Tok::Punct(':')) {
        n - 1
    } else if n >= 3
        && matches!(pending[n - 1], Tok::Ident(_))
        && matches!(pending[n - 2], Tok::Punct(':'))
    {
        n - 2
    } else {
        return None;
    };
    if colon_idx == 0 || !matches!(pending[colon_idx - 1], Tok::Ident(_)) {
        return None;
    }

    let mut open_stack: Vec<(usize, char)> = Vec::new();
    for (i, tok) in pending[..colon_idx].iter().enumerate() {
        match tok {
            Tok::Punct(c @ ('(' | '[' | '<')) => open_stack.push((i, *c)),
            Tok::Punct(')' | ']' | '>') => {
                open_stack.pop();
            }
            _ => {}
        }
    }
    let &(open_idx, bracket) = open_stack.last()?;
    if bracket != '(' || open_idx == 0 {
        return None;
    }

    match &pending[open_idx - 1] {
        Tok::Ident(name) if idx.bitset_names.contains(name) => Some(name.clone()),
        _ => None,
    }
}

/// True if, skipping any identifier chars still sitting past the cursor
/// (e.g. a selected word not yet replaced) and whitespace, the next
/// character is `:` — i.e. this flag already has a value following it.
fn cursor_already_has_value(text: &str, offset: usize) -> bool {
    let bytes = text.as_bytes();
    let mut i = offset;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_alphanumeric() || c == '_' {
            i += 1;
        } else {
            break;
        }
    }
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    i < bytes.len() && bytes[i] as char == ':'
}

/// Index of the innermost still-open `(` in `pending`, if any.
fn find_enclosing_open_paren(pending: &[Tok]) -> Option<usize> {
    let mut depth = 0i32;
    for i in (0..pending.len()).rev() {
        match &pending[i] {
            Tok::Punct(')' | ']' | '>') => depth += 1,
            Tok::Punct('(' | '[' | '<') => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Flag names already assigned (`Name: value,`) between the open paren
/// and the cursor. The entry currently being typed/edited is excluded
/// naturally, since its colon+value (if any) lies past the cursor.
fn used_bitset_flags(pending: &[Tok], open_idx: usize) -> HashSet<String> {
    let mut used = HashSet::new();
    let mut i = open_idx + 1;
    while i + 2 < pending.len() {
        if let (Tok::Ident(name), Tok::Punct(':'), Tok::Ident(_)) =
            (&pending[i], &pending[i + 1], &pending[i + 2])
        {
            used.insert(name.clone());
            i += 3;
            if matches!(pending.get(i), Some(Tok::Punct(','))) {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    used
}

fn import_path_ctx(text: &str, offset: usize) -> Option<Ctx> {
    let prefix = &text[..offset.min(text.len())];
    let bytes = prefix.as_bytes();

    let mut i = 0;
    let mut open_quote_idx: Option<usize> = None;
    let mut last_ident: Option<&str> = None;

    while i < bytes.len() {
        let c = bytes[i] as char;

        if open_quote_idx.is_none() && c == '/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        if c == '"' {
            open_quote_idx = match open_quote_idx {
                None => Some(i),
                Some(_) => None,
            };
            i += 1;
            continue;
        }

        if open_quote_idx.is_some() {
            if c == '\\' {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len()
                && ((bytes[i] as char) == '_' || (bytes[i] as char).is_alphanumeric())
            {
                i += 1;
            }
            last_ident = Some(&prefix[start..i]);
            continue;
        }

        if !c.is_whitespace() {
            last_ident = None;
        }
        i += 1;
    }

    let open_idx = open_quote_idx?;
    if last_ident != Some("import") {
        return None;
    }

    let typed = &prefix[open_idx + 1..];
    let (dir_prefix, partial) = match typed.rfind('/') {
        Some(i) => (typed[..=i].to_string(), typed[i + 1..].to_string()),
        None => (String::new(), typed.to_string()),
    };

    Some(Ctx::ImportPath {
        dir_prefix,
        partial,
    })
}

fn import_path_completions(
    schema_path: &Path,
    dir_prefix: &str,
    partial: &str,
) -> Vec<CompletionItem> {
    let Some(schema_dir) = schema_path.parent() else {
        return Vec::new();
    };
    let search_dir = schema_dir.join(dir_prefix);

    let Ok(entries) = fs::read_dir(&search_dir) else {
        return Vec::new();
    };
    let partial_lower = partial.to_lowercase(); // Windows paths are case-insensitive

    let mut items: Vec<CompletionItem> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            if !name.to_lowercase().starts_with(&partial_lower) {
                return None;
            }
            let file_type = entry.file_type().ok()?;

            if file_type.is_dir() {
                Some(CompletionItem {
                    label: format!("{name}/"),
                    insert_text: Some(format!("{name}/")),
                    kind: Some(CompletionItemKind::FOLDER),
                    ..Default::default()
                })
            } else if name.ends_with(".pojoc") {
                Some(CompletionItem {
                    label: name.clone(),
                    insert_text: Some(name),
                    kind: Some(CompletionItemKind::FILE),
                    ..Default::default()
                })
            } else {
                None
            }
        })
        .collect();

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn suffix_has_delta(text: &str, offset: usize) -> bool {
    let rest = &text[offset.min(text.len())..];
    let bytes = rest.as_bytes();
    let mut i = 0;

    // skip past the closing `]`, respecting nested brackets
    let mut depth = 0i32;
    while i < bytes.len() {
        match bytes[i] as char {
            '[' => depth += 1,
            ']' => {
                if depth == 0 {
                    i += 1;
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }

    // skip whitespace
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }

    // must be followed by `(`
    if i >= bytes.len() || bytes[i] as char != '(' {
        return false;
    }
    i += 1;

    // find matching `)`, collect inner slice
    let inner_start = i;
    let mut depth = 0i32;
    while i < bytes.len() {
        match bytes[i] as char {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }

    // look for a `delta` identifier token in there
    tokenize_prefix(&rest[inner_start..i])
        .iter()
        .any(|t| matches!(t, Tok::Ident(s) if s == "delta"))
}

pub(crate) fn cursor_in_line_comment(text: &str, offset: usize) -> bool {
    let prefix = &text[..offset.min(text.len())];
    let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = &prefix[line_start..];
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        match bytes[i] as char {
            '\\' if in_string => i += 1, // skip escaped char
            '"' => in_string = !in_string,
            '/' if !in_string && bytes.get(i + 1) == Some(&b'/') => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use pojoc_schema::{Lexer, Parser};

    /// `text_with_cursor` must contain exactly one `|`, and must still form a
    /// *complete, parseable* schema once it's stripped out (mirroring how the
    /// real server always indexes the last-known-good AST, never a
    /// mid-edit/incomplete one) — put the marker where completion should
    /// trigger, with enough valid text after it to close out the statement.
    fn complete(text_with_cursor: &str) -> Vec<CompletionItem> {
        let offset = text_with_cursor
            .find('|')
            .expect("test input must contain a `|` cursor marker");
        let text: String = text_with_cursor.replacen('|', "", 1);

        let tokens = Lexer::new(&text).tokenize().expect("lex failed");
        let ast = Parser::new(tokens)
            .parse_schema()
            .unwrap_or_else(|e| panic!("test schema must parse cleanly: {e}\n{text}"));
        let idx = SchemaIndex::build(&ast);

        completions_for_position(&text, offset, &idx, None)
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn suggests_generic_instantiation_snippet_at_type_position() {
        let items = complete(
            r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
    fields {
      x: |i32
    }
  }
}
"#,
        );
        let item = items
            .iter()
            .find(|i| i.label == "Box<T>")
            .expect("Box<T> snippet not offered");
        assert_eq!(item.insert_text.as_deref(), Some("Box<$1>"));
        assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
        // bare "Box" is never a valid type on its own, so it shouldn't be offered
        assert!(!labels(&items).contains(&"Box"));
    }

    #[test]
    fn multi_param_generic_snippet_has_one_tab_stop_per_param() {
        let items = complete(
            r#"
schema Test {
  version 1 {
    type Pair<A, B> {
      first: A
      second: B
    }
    fields {
      x: |i32
    }
  }
}
"#,
        );
        let item = items
            .iter()
            .find(|i| i.label == "Pair<A, B>")
            .expect("Pair<A, B> snippet not offered");
        assert_eq!(item.insert_text.as_deref(), Some("Pair<$1, $2>"));
    }

    #[test]
    fn extends_generic_args_offers_wildcard_and_types() {
        let items = complete(
            r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
  }
  version 2 {
    type Pair<A, B> extends Box<|i32>@1 {
      + other: B?
    }
  }
}
"#,
        );
        let ls = labels(&items);
        assert!(ls.contains(&"_"), "expected `_` wildcard, got {ls:?}");
        assert!(ls.contains(&"i32"), "expected primitive types, got {ls:?}");
    }

    #[test]
    fn type_param_declaration_offers_nothing() {
        let items = complete(
            r#"
schema Test {
  version 1 {
    type Box<|T> {
      value: T
    }
  }
}
"#,
        );
        assert!(
            items.is_empty(),
            "expected no completions while declaring type params, got {items:?}"
        );
    }

    #[test]
    fn non_generic_type_position_is_unaffected() {
        let items = complete(
            r#"
schema Test {
  version 1 {
    type Plain {
      x: i32
    }
    fields {
      y: |i32
    }
  }
}
"#,
        );
        let item = items
            .iter()
            .find(|i| i.label == "Plain")
            .expect("Plain not offered");
        assert_eq!(item.kind, Some(CompletionItemKind::CLASS));
        assert_eq!(item.insert_text, None);
    }

    #[test]
    fn suggests_own_type_params_inside_generic_body() {
        let items = complete(
            r#"
schema Test {
  version 1 {
    type Triple<A, B, C> {
      first: |i32
    }
  }
}
"#,
        );
        let ls = labels(&items);
        for p in ["A", "B", "C"] {
            assert!(ls.contains(&p), "expected type param `{p}`, got {ls:?}");
        }
        let a = items.iter().find(|i| i.label == "A").unwrap();
        assert_eq!(a.kind, Some(CompletionItemKind::TYPE_PARAMETER));
    }

    #[test]
    fn own_type_params_offered_in_extended_generic_body_too() {
        let items = complete(
            r#"
schema Test {
  version 1 {
    type Mono<A> {
      value: A
    }
  }
  version 2 {
    type Mono<A> extends Mono<A>@1 {
      + second: |A
    }
  }
}
"#,
        );
        assert!(labels(&items).contains(&"A"));
    }

    #[test]
    fn own_type_params_not_offered_outside_a_type_body() {
        let items = complete(
            r#"
schema Test {
  version 1 {
    type Box<T> {
      value: T
    }
    fields {
      x: |i32
    }
  }
}
"#,
        );
        assert!(!labels(&items).contains(&"T"));
    }
}
