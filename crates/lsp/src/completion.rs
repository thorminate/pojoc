use lsp_types::*;
use pojoc_schema::ast::*;
use std::collections::{HashMap, HashSet};

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
    pub fields_before_version: HashMap<i128, Vec<String>>,
    pub type_fields_before_version: HashMap<(String, i128), Vec<String>>,
    pub import_aliases: HashSet<String>,
    pub import_versions: HashMap<String, Vec<i128>>,
}

impl SchemaIndex {
    pub fn build(ast: &SchemaAst) -> Self {
        let mut idx = SchemaIndex::default();
        idx.import_aliases = ast.imports.iter().map(|i| i.alias.clone()).collect();
        let mut running_fields: Vec<String> = Vec::new();
        let mut type_running: HashMap<String, Vec<String>> = HashMap::new();

        for v in &ast.versions {
            idx.fields_before_version.insert(v.version, running_fields.clone());

            for block in &v.blocks {
                match block {
                    VersionBlockAst::TypeDef(t) => {
                        idx.type_names.insert(t.name.clone());
                        idx.declared_versions.entry(t.name.clone()).or_default().push(v.version);
                        match &t.body {
                            TypeBody::Fields(f) => {
                                let names = field_names(f);
                                type_running.insert(t.name.clone(), names);
                            }
                            TypeBody::Diff(ops) => {
                                let entry = type_running.entry(t.name.clone()).or_default();
                                idx.type_fields_before_version
                                    .insert((t.name.clone(), v.version), entry.clone());
                                apply_diff(entry, ops);
                            }
                        }
                    }
                    VersionBlockAst::EnumDef(e) => {
                        idx.enum_names.insert(e.name().to_string());
                        idx.declared_versions.entry(e.name().to_string()).or_default().push(v.version);
                        apply_enum(&mut idx, e);
                    }
                    VersionBlockAst::UnionDef(u) => {
                        idx.union_names.insert(u.name().to_string());
                        idx.declared_versions.entry(u.name().to_string()).or_default().push(v.version);
                        apply_union(&mut idx, u);
                    }
                    VersionBlockAst::BitsetDef(b) => {
                        idx.bitset_names.insert(b.name().to_string());
                        idx.declared_versions.entry(b.name().to_string()).or_default().push(v.version);
                        apply_bitset(&mut idx, b);
                    }
                    VersionBlockAst::Fields(f) => {
                        running_fields = field_names(f);
                    }
                    VersionBlockAst::Diff(ops) => {
                        apply_diff(&mut running_fields, ops);
                    }
                }
            }
        }

        idx
    }

    pub fn all_type_like_names(&self) -> impl Iterator<Item = &str> {
        self.type_names.iter().map(String::as_str)
            .chain(self.enum_names.iter().map(String::as_str))
            .chain(self.union_names.iter().map(String::as_str))
            .chain(self.bitset_names.iter().map(String::as_str))
    }

    pub fn versions_for(&self, name: &str) -> &[i128] {
        self.declared_versions.get(name).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn field_names(f: &FieldsAst) -> Vec<String> {
    f.fields.iter().map(|fld| fld.name.clone())
        .chain(f.const_fields.iter().map(|c| c.name.clone()))
        .collect()
}

fn apply_enum(idx: &mut SchemaIndex, e: &EnumDefAst) {
    match e {
        EnumDefAst::Definition { name, variants, .. } => {
            idx.enum_variants.insert(name.clone(), variants.iter().map(|v| v.name.clone()).collect());
        }
        EnumDefAst::Extension { name, ops, .. } => {
            let list = idx.enum_variants.entry(name.clone()).or_default();
            for op in ops {
                match op {
                    EnumVariantOpAst::Add { name: n, .. } => list.push(n.clone()),
                    EnumVariantOpAst::Rename { from, to, .. } => {
                        if let Some(slot) = list.iter_mut().find(|v| *v == from) {
                            *slot = to.clone();
                        }
                    }
                }
            }
        }
    }
}

fn apply_bitset(idx: &mut SchemaIndex, b: &BitsetDefAst) {
    match b {
        BitsetDefAst::Definition { name, variants, .. } => {
            idx.bitset_variants.insert(name.clone(), variants.clone());
        }
        BitsetDefAst::Extension { name, ops, .. } => {
            let list = idx.bitset_variants.entry(name.clone()).or_default();
            for op in ops {
                match op {
                    BitsetOpAst::Add { name: n, .. } => list.push(n.clone()),
                    BitsetOpAst::Remove { name: n, .. } => list.retain(|v| v != n),
                }
            }
        }
    }
}

fn apply_union(idx: &mut SchemaIndex, u: &UnionDefAst) {
    match u {
        UnionDefAst::Definition { name, variants, .. } => {
            idx.union_variants.insert(name.clone(), variants.iter().map(|v| v.name.clone()).collect());
        }
        UnionDefAst::Extension { name, ops, .. } => {
            let list = idx.union_variants.entry(name.clone()).or_default();
            for op in ops {
                match op {
                    UnionVariantOpAst::Add { name: n, .. } => list.push(n.clone()),
                }
            }
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

// --- cursor-position context detection ---------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Number(String),
    Punct(char),
    DoubleColon,
    Newline,
}

fn tokenize_prefix(src: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '\n' { toks.push(Tok::Newline); i += 1; continue; }
        if c.is_whitespace() { i += 1; continue; }
        if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        if c == '"' {
            i += 1;
            while i < bytes.len() && bytes[i] as char != '"' {
                if bytes[i] as char == '\\' { i += 1; }
                i += 1;
            }
            i += 1;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len() && (bytes[i] as char == '_' || (bytes[i] as char).is_alphanumeric()) { i += 1; }
            toks.push(Tok::Ident(src[start..i].to_string()));
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && ((bytes[i] as char).is_ascii_digit() || bytes[i] as char == '.') { i += 1; }
            toks.push(Tok::Number(src[start..i].to_string()));
            continue;
        }
        if c == ':' && bytes.get(i + 1) == Some(&b':') {
            toks.push(Tok::DoubleColon); i += 2; continue;
        }
        toks.push(Tok::Punct(c));
        i += 1;
    }
    toks
}

#[derive(Debug, Clone)]
enum BlockKind {
    Root,
    #[allow(dead_code)]
    Schema(String),
    Version(i128),
    TypeDef(String),
    Fields,
    Diff,
    Other,
}

struct ScanState {
    stack: Vec<BlockKind>,
    pending: Vec<Tok>,
}

fn scan(tokens: &[Tok]) -> ScanState {
    let mut stack = vec![BlockKind::Root];
    let mut pending: Vec<Tok> = Vec::new();
    let mut depth = 0i32; // tracks ( [ < so newlines mid-call don't reset `pending`

    for tok in tokens {
        match tok {
            Tok::Punct('{') => {
                stack.push(classify_header(&pending));
                pending.clear();
                depth = 0;
            }
            Tok::Punct('}') => {
                if stack.len() > 1 { stack.pop(); }
                pending.clear();
                depth = 0;
            }
            Tok::Newline => {
                if depth == 0 { pending.clear(); }
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
        if pending.len() > 16 { pending.remove(0); }
    }

    ScanState { stack, pending }
}

fn classify_header(pending: &[Tok]) -> BlockKind {
    match pending {
        [Tok::Ident(k), Tok::Ident(name), ..] if k == "schema"  => BlockKind::Schema(name.clone()), 
        [Tok::Ident(k), Tok::Number(n),    ..] if k == "version" => BlockKind::Version(n.parse().unwrap_or(0)),
        [Tok::Ident(k), Tok::Ident(name),  ..] if k == "type"    => BlockKind::TypeDef(name.clone()),
        [Tok::Ident(k), ..] if k == "fields" => BlockKind::Fields,
        [Tok::Ident(k), ..] if k == "diff"   => BlockKind::Diff,
        _ => BlockKind::Other,
    }
}

fn current_version(stack: &[BlockKind]) -> Option<i128> {
    stack.iter().rev().find_map(|f| if let BlockKind::Version(v) = f { Some(*v) } else { None })
}

fn find_diff_context(stack: &[BlockKind]) -> Option<(Option<String>, i128)> {
    if !matches!(stack.last(), Some(BlockKind::Diff)) { return None; }
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

/// Identifier immediately preceding the innermost still-open bracket, if any.
fn enclosing_call(pending: &[Tok]) -> Option<(Option<String>, char)> {
    let mut stack: Vec<(Option<String>, char)> = Vec::new();
    let mut last_ident: Option<String> = None;
    for tok in pending {
        match tok {
            Tok::Ident(s) => last_ident = Some(s.clone()),
            Tok::Punct(c @ ('(' | '[' | '<')) => stack.push((last_ident.take(), *c)),
            Tok::Punct(')' | ']' | '>') => { stack.pop(); }
            Tok::Punct(',') | Tok::Punct(':') => {}
            _ => last_ident = None,
        }
    }
    stack.last().cloned()
}

enum Ctx {
    FileRoot,
    SchemaBody { next_version: i128 },
    VersionBody,
    DiffLineStart,
    DiffOldFieldName { owner_type: Option<String>, version: i128 },
    TypePosition,
    ExtendsName,
    ExtendsVersion { name: String, before_version: Option<i128> },
    ImportVersion { alias: String },
    EnumVariantAccess { name: String },
    VFloatParam,
    BitsetLiteralField { bitset_name: String },
    ArraySuffixModifier { delta_eligible: bool },
    Unknown,
}

fn determine_ctx(state: &ScanState, idx: &SchemaIndex) -> Ctx {
    let pending = &state.pending;
    let stack = &state.stack;

    if let [.., Tok::Ident(name), Tok::Punct('@')] = pending.as_slice() {
        let preceded_by_extends = pending.len() >= 3
            && matches!(&pending[pending.len() - 3], Tok::Ident(k) if k == "extends");

        if preceded_by_extends {
            return Ctx::ExtendsVersion { name: name.clone(), before_version: current_version(stack) };
        }
        if idx.import_aliases.contains(name) {
            return Ctx::ImportVersion { alias: name.clone() };
        }
        return Ctx::Unknown;
    }

    if matches!(pending.as_slice(), [.., Tok::Ident(_), Tok::DoubleColon]
        | [.., Tok::Ident(_), Tok::DoubleColon, Tok::Ident(_)])
    {
        let name = match pending.as_slice() {
            [.., Tok::Ident(n), Tok::DoubleColon] => n.clone(),
            [.., Tok::Ident(n), Tok::DoubleColon, Tok::Ident(_)] => n.clone(),
            _ => unreachable!(),
        };
        return Ctx::EnumVariantAccess { name };
    }

    if let Some(info) = array_suffix_info(pending) {
        let eligible = match &info.element {
            ArrayElement::Scalar(name) => is_delta_eligible_name(name),
            ArrayElement::Other => false,
        };
        return Ctx::ArraySuffixModifier { delta_eligible: eligible && !info.already_has_delta };
    }

    if let Some((owner, bracket)) = enclosing_call(pending) {
        match (owner.as_deref(), bracket) {
            (Some("vfloat"), '(') => return Ctx::VFloatParam,
            (Some(name), '(') if idx.bitset_names.contains(name) => {
                return Ctx::BitsetLiteralField { bitset_name: name.to_string() };
            }
            (_, '[' | '<') => return Ctx::TypePosition,
            _ => {}
        }
    }

    if matches!(pending.last(), Some(Tok::Ident(k)) if k == "extends") {
        return Ctx::ExtendsName;
    }
    if pending.len() >= 2 {
        if let (Tok::Ident(k), Tok::Ident(_)) = (&pending[pending.len() - 2], &pending[pending.len() - 1]) {
            if k == "extends" { return Ctx::ExtendsName; }
        }
    }

    if matches!(pending.last(), Some(Tok::Punct(':'))) {
        return Ctx::TypePosition;
    }
    if pending.len() >= 2 {
        if let (Tok::Punct(':'), Tok::Ident(_)) = (&pending[pending.len() - 2], &pending[pending.len() - 1]) {
            return Ctx::TypePosition;
        }
    }

    if let Some((owner, version)) = find_diff_context(stack) {
        match pending.as_slice() {
            [] => return Ctx::DiffLineStart,
            [Tok::Punct(op)] if matches!(op, '-' | '~') => {
                return Ctx::DiffOldFieldName { owner_type: owner, version };
            }
            [Tok::Punct(op), Tok::Ident(_)] if matches!(op, '-' | '~') => {
                return Ctx::DiffOldFieldName { owner_type: owner, version };
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

    if matches!(stack.last(), Some(BlockKind::Version(_))) && pending.is_empty() {
        return Ctx::VersionBody;
    }

    if matches!(stack.last(), Some(BlockKind::Root)) && pending.is_empty() {
        return Ctx::FileRoot;
    }

    Ctx::Unknown
}

pub fn completions_for_position(text: &str, offset: usize, idx: &SchemaIndex) -> Vec<CompletionItem> {
    let prefix = &text[..offset.min(text.len())];
    let tokens = tokenize_prefix(prefix);
    let state = scan(&tokens);
    let ctx = determine_ctx(&state, idx);

    match ctx {
        Ctx::VersionBody => vec![
            snippet("fields", "fields {\n\t$0\n}", "field declarations block"),
            snippet("diff", "diff {\n\t$0\n}", "schema diff block"),
            snippet("type", "type $1 {\n\t$0\n}", "define a type"),
            snippet("enum", "enum $1 {\n\t$0\n}", "define an enum"),
            snippet("union", "union $1 {\n\t$0\n}", "define a union"),
            snippet("bitset", "bitset $1 {\n\t$0\n}", "define a bitset"),
        ],

        Ctx::DiffLineStart => ["+", "-", "~"].iter().map(|op| CompletionItem {
            label: op.to_string(),
            kind: Some(CompletionItemKind::OPERATOR),
            detail: Some(match *op {
                "+" => "add a field/variant".into(),
                "-" => "remove a field/variant".into(),
                _ => "rename or retype a field/variant".into(),
            }),
            ..Default::default()
        }).collect(),

        Ctx::DiffOldFieldName { owner_type, version } => {
            let names: &[String] = match &owner_type {
                Some(ty) => idx.type_fields_before_version
                    .get(&(ty.clone(), version)).map(Vec::as_slice).unwrap_or(&[]),
                None => idx.fields_before_version.get(&version).map(Vec::as_slice).unwrap_or(&[]),
            };
            names.iter().map(|n| CompletionItem {
                label: n.clone(),
                kind: Some(CompletionItemKind::FIELD),
                ..Default::default()
            }).collect()
        }

        Ctx::ExtendsName => idx.all_type_like_names().map(|n| CompletionItem {
            label: n.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            ..Default::default()
        }).collect(),

        Ctx::ExtendsVersion { name, before_version } => idx.versions_for(&name).iter()
            .filter(|v| before_version.map_or(true, |bv| **v < bv))
            .map(|v| CompletionItem {
                label: v.to_string(),
                kind: Some(CompletionItemKind::CONSTANT),
                ..Default::default()
            }).collect(),

        Ctx::ImportVersion { alias } => idx.import_versions.get(&alias).into_iter().flatten()
            .map(|v| CompletionItem {
                label: v.to_string(),
                kind: Some(CompletionItemKind::CONSTANT),
                detail: Some(format!("version of imported schema `{alias}`")),
                ..Default::default()
            }).collect(),

        Ctx::EnumVariantAccess { name } => idx.enum_variants.get(&name).into_iter().flatten()
            .map(|v| CompletionItem {
                label: v.clone(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                ..Default::default()
            }).collect(),

        Ctx::TypePosition => type_position_items(idx),

        Ctx::VFloatParam => ["min", "max", "step"].iter().map(|p| CompletionItem {
            label: format!("{p}:"),
            insert_text: Some(format!("{p}: $0")),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            kind: Some(CompletionItemKind::PROPERTY),
            ..Default::default()
        }).collect(),

        Ctx::BitsetLiteralField { bitset_name } => idx.bitset_variants.get(&bitset_name).into_iter().flatten()
            .map(|v| CompletionItem {
                label: format!("{v}:"),
                insert_text: Some(format!("{v}: $0")),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                kind: Some(CompletionItemKind::PROPERTY),
                ..Default::default()
            }).collect(),

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
        Ctx::FileRoot => vec![
            snippet("schema", "schema $1 {\n\t$0\n}", "define a new schema"),
        ],
        Ctx::SchemaBody { next_version } => vec![
            snippet(
                &format!("version {next_version}"),
                &format!("version {next_version} {{\n\t$0\n}}"),
                &format!("add version {next_version} block"),
            ),
            snippet("import", "import \"$1\" as $2", "import another schema"),
        ],
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

fn type_position_items(idx: &SchemaIndex) -> Vec<CompletionItem> {
    const PRIMITIVES: &[&str] = &[
        "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64",
        "varint32", "varint64", "bool", "string",
    ];

    let mut items: Vec<CompletionItem> = PRIMITIVES.iter().map(|p| CompletionItem {
        label: p.to_string(),
        kind: Some(CompletionItemKind::TYPE_PARAMETER),
        ..Default::default()
    }).collect();

    items.extend(idx.all_type_like_names().map(|n| CompletionItem {
        label: n.to_string(),
        kind: Some(CompletionItemKind::CLASS),
        ..Default::default()
    }));

    items.extend(idx.import_aliases.iter().map(|n| CompletionItem {
        label: n.clone(),
        kind: Some(CompletionItemKind::MODULE),
        detail: Some("imported schema".into()),
        ..Default::default()
    }));

    items.push(CompletionItem { label: "lazy".into(), kind: Some(CompletionItemKind::KEYWORD), ..Default::default() });
    items.push(CompletionItem { label: "const".into(), kind: Some(CompletionItemKind::KEYWORD), ..Default::default() });
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
            Tok::Punct(')' | ']' | '>') => { open_stack.pop(); }
            _ => {}
        }
    }
    let &(open_paren_idx, bracket) = open_stack.last()?;
    if bracket != '(' || open_paren_idx == 0 { return None; }
    if !matches!(&pending[open_paren_idx - 1], Tok::Punct(']')) { return None; }

    let mut depth = 0i32;
    let mut open_bracket_idx = None;
    for i in (0..open_paren_idx).rev() {
        match &pending[i] {
            Tok::Punct(']') => depth += 1,
            Tok::Punct('[') => {
                depth -= 1;
                if depth == 0 { open_bracket_idx = Some(i); break; }
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

    Some(ArraySuffixInfo { element, already_has_delta })
}

// Mirrors pojoc_core::types::is_delta_eligible on the raw type-name token,
// since completion runs ahead of resolution and never sees a ResolvedTypeRef.
// Keep this list in sync with that function if it changes.
fn is_delta_eligible_name(name: &str) -> bool {
    matches!(name, "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64")
}