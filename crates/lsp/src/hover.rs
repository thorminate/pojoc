use crate::completion::{
    BlockKind, SchemaIndex, Tok, cursor_in_line_comment, scan, tokenize_prefix,
};
use pojoc_core::types::{ResolvedTypeRef, TypeId};
use pojoc_schema::ir::ir_types::*;
use std::collections::HashMap;

/// What identifier the cursor is resolved to point at.
enum HoverTarget {
    Schema,
    Type(String),
    Enum(String),
    Union(String),
    Bitset(String),
    Field { owner: Option<String>, name: String },
    Variant { owner: String, name: String },
}

/// Resolves the hover markdown (already schema-flavored, doc-comment-aware)
/// for the identifier under `offset`, plus the byte range of that identifier
/// (for the caller to translate into an LSP `Range`). `resolved` is the last
/// successfully-analyzed schema, if any — richer signatures are only
/// available when analysis last succeeded; otherwise this falls back to
/// doc-comment-only hover sourced from the (always available) raw AST index.
pub fn hover_for_position(
    text: &str,
    offset: usize,
    idx: &SchemaIndex,
    resolved: Option<&ResolvedSchema>,
) -> Option<(String, usize, usize)> {
    if cursor_in_line_comment(text, offset) {
        return None;
    }
    let (word, start, end) = word_at(text, offset)?;
    let prefix = &text[..start];
    let tokens = tokenize_prefix(prefix);
    let state = scan(&tokens);
    let target = classify(&word, &state.pending, &state.stack, idx)?;
    let markdown = render_hover(target, idx, resolved)?;
    Some((markdown, start, end))
}

fn word_at(text: &str, offset: usize) -> Option<(String, usize, usize)> {
    let bytes = text.as_bytes();
    let offset = offset.min(bytes.len());
    let is_ident = |b: u8| (b as char).is_alphanumeric() || b == b'_';

    let mut start = offset;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_ident(bytes[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some((text[start..end].to_string(), start, end))
}

fn classify(
    word: &str,
    pending: &[Tok],
    stack: &[BlockKind],
    idx: &SchemaIndex,
) -> Option<HoverTarget> {
    // Immediately preceded by a declaring keyword -> this word IS the
    // type/enum/union/bitset/schema name being declared (or extended).
    if let Some(Tok::Ident(k)) = pending.last() {
        match k.as_str() {
            "type" => return Some(HoverTarget::Type(word.to_string())),
            "enum" => return Some(HoverTarget::Enum(word.to_string())),
            "union" => return Some(HoverTarget::Union(word.to_string())),
            "bitset" => return Some(HoverTarget::Bitset(word.to_string())),
            "schema" => return Some(HoverTarget::Schema),
            _ => {}
        }
    }

    // `Owner::Variant` access (default values, discriminant literals, etc).
    if let [.., Tok::Ident(owner), Tok::DoubleColon] = pending {
        return Some(HoverTarget::Variant {
            owner: owner.clone(),
            name: word.to_string(),
        });
    }

    // Field/const/variant *declaration* position: the word is the first
    // token on its line (no `:` seen yet this line — past the colon means
    // we're inside a type annotation/payload, i.e. a reference, not a
    // declaration).
    let past_colon = pending.iter().any(|t| matches!(t, Tok::Punct(':')));
    if !past_colon {
        match stack.last() {
            Some(BlockKind::TypeDef(owner)) => {
                return Some(HoverTarget::Field {
                    owner: Some(owner.clone()),
                    name: word.to_string(),
                });
            }
            Some(BlockKind::Fields) | Some(BlockKind::Diff) => {
                return Some(HoverTarget::Field {
                    owner: None,
                    name: word.to_string(),
                });
            }
            Some(BlockKind::EnumDef(owner))
            | Some(BlockKind::UnionDef(owner))
            | Some(BlockKind::BitsetDef(owner)) => {
                return Some(HoverTarget::Variant {
                    owner: owner.clone(),
                    name: word.to_string(),
                });
            }
            _ => {}
        }
    }

    // Fallback: a bare reference anywhere (field type annotation, `extends`
    // clause, generic argument, array/map element, ...) — types/enums/
    // unions/bitsets share one namespace, so a name match is unambiguous.
    if idx.type_names.contains(word) {
        return Some(HoverTarget::Type(word.to_string()));
    }
    if idx.enum_names.contains(word) {
        return Some(HoverTarget::Enum(word.to_string()));
    }
    if idx.union_names.contains(word) {
        return Some(HoverTarget::Union(word.to_string()));
    }
    if idx.bitset_names.contains(word) {
        return Some(HoverTarget::Bitset(word.to_string()));
    }

    // Last resort: a bare variant name with no `Owner::` qualifier (best
    // effort — picks whichever enum/union/bitset happens to declare it).
    idx.variant_docs
        .keys()
        .find(|(_, v)| v == word)
        .map(|(owner, _)| HoverTarget::Variant {
            owner: owner.clone(),
            name: word.to_string(),
        })
}

fn latest_by_name<'a, V>(map: &'a HashMap<TypeId, V>, name: &str) -> Option<&'a V> {
    map.iter()
        .filter(|(id, _)| id.name == name)
        .max_by_key(|(id, _)| id.version)
        .map(|(_, v)| v)
}

fn render_hover(
    target: HoverTarget,
    idx: &SchemaIndex,
    resolved: Option<&ResolvedSchema>,
) -> Option<String> {
    match target {
        HoverTarget::Schema => {
            if let Some(r) = resolved {
                let latest = r.versions.last()?;
                let sig = render_type_signature(&r.name_hint, &latest.fields, &latest.const_fields);
                return Some(markdown_with_doc(&sig, &r.doc));
            }
            doc_only_markdown(&idx.schema_doc)
        }
        HoverTarget::Type(name) => {
            if let Some(r) = resolved {
                if let Some(t) = latest_by_name(&r.types.types, &name) {
                    let sig = render_type_signature(&name, &t.fields, &t.const_fields);
                    return Some(markdown_with_doc(&sig, &t.doc));
                }
                return None;
            }
            idx.type_docs.get(&name).and_then(|d| doc_only_markdown(d))
        }
        HoverTarget::Enum(name) => {
            if let Some(r) = resolved {
                if let Some(e) = latest_by_name(&r.enums.enums, &name) {
                    let sig = render_enum_signature(&name, e);
                    return Some(markdown_with_doc(&sig, &e.doc));
                }
                return None;
            }
            idx.enum_docs.get(&name).and_then(|d| doc_only_markdown(d))
        }
        HoverTarget::Union(name) => {
            if let Some(r) = resolved {
                if let Some(u) = latest_by_name(&r.unions.unions, &name) {
                    let sig = render_union_signature(&name, u);
                    return Some(markdown_with_doc(&sig, &u.doc));
                }
                return None;
            }
            idx.union_docs.get(&name).and_then(|d| doc_only_markdown(d))
        }
        HoverTarget::Bitset(name) => {
            if let Some(r) = resolved {
                if let Some(b) = latest_by_name(&r.bitsets.bitsets, &name) {
                    let sig = render_bitset_signature(&name, b);
                    return Some(markdown_with_doc(&sig, &b.doc));
                }
                return None;
            }
            idx.bitset_docs
                .get(&name)
                .and_then(|d| doc_only_markdown(d))
        }
        HoverTarget::Field { owner, name } => {
            if let Some(r) = resolved {
                let (fields, consts): (&[FieldIR], &[ResolvedConst]) = match &owner {
                    Some(o) => {
                        let t = latest_by_name(&r.types.types, o)?;
                        (&t.fields, &t.const_fields)
                    }
                    None => {
                        let latest = r.versions.last()?;
                        (&latest.fields, &latest.const_fields)
                    }
                };
                if let Some(f) = fields.iter().find(|f| f.name == name) {
                    let ty = render_type_ref(&f.ty);
                    let sig = if f.lazy {
                        format!("{name}: lazy {ty}")
                    } else {
                        format!("{name}: {ty}")
                    };
                    return Some(markdown_with_doc(&sig, &f.doc));
                }
                if let Some(c) = consts.iter().find(|c| c.name == name) {
                    let sig = format!(
                        "const {name}: {} = {}",
                        rust_type_to_schema(c.rust_type),
                        render_const_value(&c.value)
                    );
                    return Some(markdown_with_doc(&sig, &c.doc));
                }
                return None;
            }
            idx.field_docs
                .get(&(owner, name))
                .and_then(|d| doc_only_markdown(d))
        }
        HoverTarget::Variant { owner, name } => {
            if let Some(r) = resolved {
                if let Some(e) = latest_by_name(&r.enums.enums, &owner)
                    && let Some(v) = e.variants.iter().find(|v| v.name == name)
                {
                    let sig = format!("{owner}::{} = {}", v.name, v.wire_value);
                    return Some(markdown_with_doc(&sig, &v.doc));
                }
                if let Some(u) = latest_by_name(&r.unions.unions, &owner)
                    && let Some(v) = u.variants.iter().find(|v| v.name == name)
                {
                    let sig = format!("{owner}::{}({})", v.name, render_type_ref(&v.payload));
                    return Some(markdown_with_doc(&sig, &v.doc));
                }
                if let Some(b) = latest_by_name(&r.bitsets.bitsets, &owner)
                    && let Some(v) = b.variants.iter().find(|v| v.name == name)
                {
                    let sig = format!("{owner}::{}", v.name);
                    return Some(markdown_with_doc(&sig, &v.doc));
                }
                return None;
            }
            idx.variant_docs
                .get(&(owner, name))
                .and_then(|d| doc_only_markdown(d))
        }
    }
}

fn doc_only_markdown(doc: &[String]) -> Option<String> {
    if doc.is_empty() {
        None
    } else {
        Some(doc.join("\n"))
    }
}

fn markdown_with_doc(sig: &str, doc: &[String]) -> String {
    if doc.is_empty() {
        format!("```pojoc\n{sig}\n```")
    } else {
        format!("```pojoc\n{sig}\n```\n\n---\n\n{}", doc.join("\n"))
    }
}

fn rust_type_to_schema(rust_type: &str) -> &str {
    match rust_type {
        "&'static str" => "string",
        other => other,
    }
}

fn render_const_value(value: &DefaultValue) -> String {
    match value {
        DefaultValue::Bool(b) => b.to_string(),
        DefaultValue::Int(i) => i.to_string(),
        DefaultValue::Float(f) => {
            if f.fract() == 0.0 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        DefaultValue::Str(s) => format!("\"{s}\""),
        _ => String::new(),
    }
}

fn render_type_ref(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Scalar(id) | ResolvedTypeRef::Enum(id) | ResolvedTypeRef::Union(id) => {
            id.name.clone()
        }
        ResolvedTypeRef::Bitset(id, _) => id.name.clone(),
        ResolvedTypeRef::Array(inner) => format!("[{}]", render_type_ref(inner)),
        ResolvedTypeRef::FixedArray(inner, n) => format!("[{}]({n})", render_type_ref(inner)),
        ResolvedTypeRef::DeltaArray(inner) => format!("[{}](delta)", render_type_ref(inner)),
        ResolvedTypeRef::FixedDeltaArray(inner, n) => {
            format!("[{}](delta, {n})", render_type_ref(inner))
        }
        ResolvedTypeRef::FixedString(n) => format!("string({n})"),
        ResolvedTypeRef::Map(k, v) => {
            format!("map<{}, {}>", render_type_ref(k), render_type_ref(v))
        }
        ResolvedTypeRef::FixedMap(k, v, n) => {
            format!("map<{}, {}>({n})", render_type_ref(k), render_type_ref(v))
        }
        ResolvedTypeRef::Tuple(elems) => format!(
            "({})",
            elems
                .iter()
                .map(render_type_ref)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ResolvedTypeRef::VFloat { min, max, step, .. } => {
            format!("vfloat(min: {min}, max: {max}, step: {step})")
        }
        ResolvedTypeRef::Optional(inner) => format!("{}?", render_type_ref(inner)),
        ResolvedTypeRef::ImportedSchema { alias, version, .. } => format!("{alias}@{version}"),
    }
}

fn render_type_signature(name: &str, fields: &[FieldIR], consts: &[ResolvedConst]) -> String {
    let mut lines = vec![format!("type {name} {{")];
    for c in consts {
        lines.push(format!(
            "    const {}: {} = {},",
            c.name,
            rust_type_to_schema(c.rust_type),
            render_const_value(&c.value)
        ));
    }
    for f in fields {
        let ty = render_type_ref(&f.ty);
        if f.lazy {
            lines.push(format!("    {}: lazy {ty},", f.name));
        } else {
            lines.push(format!("    {}: {ty},", f.name));
        }
    }
    lines.push("}".to_string());
    lines.join("\n")
}

fn render_enum_signature(name: &str, e: &ResolvedEnum) -> String {
    let mut lines = vec![format!("enum {name} {{")];
    for v in &e.variants {
        lines.push(format!("    {} = {},", v.name, v.wire_value));
    }
    lines.push("}".to_string());
    lines.join("\n")
}

fn render_union_signature(name: &str, u: &ResolvedUnion) -> String {
    let mut lines = vec![format!("union {name} {{")];
    for v in &u.variants {
        lines.push(format!("    {}: {},", v.name, render_type_ref(&v.payload)));
    }
    lines.push("}".to_string());
    lines.join("\n")
}

fn render_bitset_signature(name: &str, b: &ResolvedBitset) -> String {
    let mut lines = vec![format!("bitset {name} {{")];
    for v in &b.variants {
        if v.name.starts_with("__deprecated_") {
            continue;
        }
        lines.push(format!("    {},", v.name));
    }
    lines.push("}".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pojoc_schema::ir::analyzer::SchemaAnalyzer;
    use pojoc_schema::{Lexer, Parser};

    /// `text_with_cursor` must contain exactly one `|` marking the hover
    /// position, and must form a complete, analyzable schema once stripped.
    fn hover(text_with_cursor: &str) -> Option<String> {
        let offset = text_with_cursor
            .find('|')
            .expect("test input must contain a `|` cursor marker");
        let text: String = text_with_cursor.replacen('|', "", 1);

        let tokens = Lexer::new(&text).tokenize().expect("lex failed");
        let ast = Parser::new(tokens)
            .parse_schema()
            .unwrap_or_else(|e| panic!("test schema must parse cleanly: {e}\n{text}"));
        let idx = SchemaIndex::build(&ast);

        let mut analyzer = SchemaAnalyzer::new(&ast, HashMap::new());
        analyzer
            .run()
            .unwrap_or_else(|e| panic!("test schema must analyze cleanly: {e}\n{text}"));
        let resolved = analyzer
            .finish()
            .unwrap_or_else(|e| panic!("test schema must finish cleanly: {e}\n{text}"));

        hover_for_position(&text, offset, &idx, Some(&resolved)).map(|(md, _, _)| md)
    }

    #[test]
    fn hover_on_type_declaration_shows_fields_and_doc() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    /// A small widget.
    type |Widget {
      /// The widget's name.
      name: string = "w"
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("type Widget {"), "{md}");
        assert!(md.contains("name: string"), "{md}");
        assert!(md.contains("A small widget."), "{md}");
    }

    #[test]
    fn hover_on_field_name_shows_its_own_doc_not_the_whole_type() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    /// A small widget.
    type Widget {
      /// The widget's name.
      |name: string = "w"
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("name: string"), "{md}");
        assert!(md.contains("The widget's name."), "{md}");
        assert!(!md.contains("type Widget"), "{md}");
    }

    #[test]
    fn hover_on_const_field_shows_value_and_doc() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    type Widget {
      /// Pi, to f64 precision.
      |pi: const f64 = 3.14
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("const pi: f64 = 3.14"), "{md}");
        assert!(md.contains("Pi, to f64 precision."), "{md}");
    }

    #[test]
    fn hover_on_field_type_reference_resolves_the_referenced_type() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    /// Boundary values.
    enum Color {
      Red,
      Green,
    }
    type Widget {
      color: |Color = Color::Red
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("enum Color {"), "{md}");
        assert!(md.contains("Boundary values."), "{md}");
    }

    #[test]
    fn hover_on_enum_declaration_lists_variants() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    /// Boundary values.
    enum |Color {
      Red,
      Green,
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("enum Color {"), "{md}");
        assert!(md.contains("Red = 1"), "{md}");
        assert!(md.contains("Green = 2"), "{md}");
        assert!(md.contains("Boundary values."), "{md}");
    }

    #[test]
    fn hover_on_enum_variant_declaration_shows_its_own_doc() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    enum Color {
      /// The first color.
      |Red,
      Green,
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("Color::Red = 1"), "{md}");
        assert!(md.contains("The first color."), "{md}");
    }

    #[test]
    fn hover_on_enum_variant_access_resolves_owner_qualified() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    enum Color {
      /// The first color.
      Red,
      Green,
    }
    type Widget {
      color: Color = Color::|Red
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("Color::Red = 1"), "{md}");
        assert!(md.contains("The first color."), "{md}");
    }

    #[test]
    fn hover_on_bitset_declaration_lists_flags() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    /// Coarse permission flags.
    bitset |Flags {
      Read,
      Write,
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("bitset Flags {"), "{md}");
        assert!(md.contains("Read"), "{md}");
        assert!(md.contains("Write"), "{md}");
        assert!(md.contains("Coarse permission flags."), "{md}");
    }

    #[test]
    fn hover_on_union_declaration_lists_variants_with_payload() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    type Payload {
      x: i32 = 0
    }
    /// Every action a player can take.
    union |Action {
      Move: Payload,
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("union Action {"), "{md}");
        assert!(md.contains("Move: Payload"), "{md}");
        assert!(md.contains("Every action a player can take."), "{md}");
    }

    #[test]
    fn hover_on_union_variant_declaration_shows_its_own_doc() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    type Payload {
      x: i32 = 0
    }
    union Action {
      /// A move action.
      |Move: Payload,
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("Action::Move(Payload)"), "{md}");
        assert!(md.contains("A move action."), "{md}");
    }

    #[test]
    fn hover_on_root_field_resolves_against_root_struct() {
        let md = hover(
            r#"
schema Test {
  version 1 {
    type Widget {
      x: i32 = 0
    }
    fields {
      |root_widget: Widget
    }
  }
}
"#,
        )
        .expect("expected hover");
        assert!(md.contains("root_widget: Widget"), "{md}");
    }

    #[test]
    fn hover_on_unknown_identifier_returns_none() {
        // `true` here is a bool literal, not a declared name anywhere in
        // the schema — no type/field/variant matches it, so no hover.
        let md = hover(
            r#"
schema Test {
  version 1 {
    fields {
      x: bool = |true
    }
  }
}
"#,
        );
        assert!(md.is_none());
    }
}
