use crate::ast::*;
use crate::error::*;
use crate::lexer::{Keyword, SpannedToken, Token};
use crate::span::Span;
use std::collections::HashSet;

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).map(|t| &t.token).unwrap_or(&Token::Eof)
    }

    fn current_line(&self) -> u32 {
        self.tokens.get(self.pos).map(|t| t.line).unwrap_or(0)
    }

    fn current_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| t.span)
            .or_else(|| self.tokens.last().map(|t| t.span))
            .unwrap_or(Span::new(0, 0))
    }

    fn last_consumed_span(&self) -> Span {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or_else(|| self.current_span())
    }

    /// Span and line of whatever `peek()`/`advance()` would currently
    /// return. Call this BEFORE advancing — that's the whole fix: read
    /// location first, consume second, so errors point at the offending
    /// token instead of whatever follows it.
    fn here(&self) -> (Span, u32) {
        (self.current_span(), self.current_line())
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).map(|t| t.token.clone()).unwrap_or(Token::Eof);
        if !matches!(tok, Token::Eof) {
            self.pos += 1;
        }
        tok
    }

    /// Advances and returns the consumed token along with the span/line
    /// it started at, captured before the advance.
    fn advance_spanned(&mut self) -> (Token, Span, u32) {
        let (span, line) = self.here();
        (self.advance(), span, line)
    }

    fn err_unexpected_at(&self, got: Token, expected: &'static str, span: Span, line: u32) -> ParseError {
        ParseError::UnexpectedToken { got, expected, span, line }
    }

    fn err_invalid_at(&self, message: impl Into<String>, span: Span, line: u32) -> ParseError {
        ParseError::InvalidSyntax { message: message.into(), span, line }
    }

    /// For error sites built straight off `peek()` with no advance yet —
    /// `current_span`/`current_line` are already correct there.
    fn err_unexpected(&self, got: Token, expected: &'static str) -> ParseError {
        self.err_unexpected_at(got, expected, self.current_span(), self.current_line())
    }

    fn err_invalid(&self, message: impl Into<String>) -> ParseError {
        self.err_invalid_at(message, self.current_span(), self.current_line())
    }

    fn expect(&mut self, want: Token, label: &'static str) -> Result<(), ParseError> {
        let (got, span, line) = self.advance_spanned();
        if got == want {
            Ok(())
        } else {
            Err(self.err_unexpected_at(got, label, span, line))
        }
    }

    fn expect_keyword(&mut self, kw: Keyword) -> Result<(), ParseError> {
        let (got, span, line) = self.advance_spanned();
        match &got {
            Token::Keyword(k) if *k == kw => Ok(()),
            _ => Err(self.err_unexpected_at(got, "keyword", span, line)),
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        let (got, span, line) = self.advance_spanned();
        match got {
            Token::Identifier(s) => Ok(s.to_string()),
            _ => Err(self.err_unexpected_at(got, "identifier", span, line)),
        }
    }

    fn expect_number(&mut self) -> Result<i128, ParseError> {
        let (got, span, line) = self.advance_spanned();
        match got {
            Token::IntLiteral(s) => s.parse::<i128>().map_err(|_| {
                self.err_invalid_at(format!("Number too large or invalid: {}", s), span, line)
            }),
            _ => Err(self.err_unexpected_at(got, "number", span, line)),
        }
    }

    pub fn parse_schema(&mut self) -> Result<SchemaAst, ParseError> {
        let (start_span, start_line) = self.here();
        self.expect_keyword(Keyword::Schema)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace, "'{'")?;

        let mut imports = Vec::new();
        while matches!(self.peek(), Token::Keyword(Keyword::Import)) {
            let (imp_span, imp_line) = self.here();
            self.advance();

            let path = match self.advance_spanned() {
                (Token::StringLiteral(s), _, _) => s.to_string(),
                (got, span, line) => return Err(self.err_unexpected_at(got, "string path", span, line)),
            };

            self.expect_keyword(Keyword::As)?;
            let alias = self.expect_ident()?;

            let span = imp_span.join(self.last_consumed_span());
            imports.push(ImportDeclAst { path, alias, span, line: imp_line });
        }

        let mut versions = Vec::new();
        let mut seen_versions = HashSet::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let version_ast = self.parse_version()?;
            if !seen_versions.insert(version_ast.version) {
                return Err(self.err_invalid(format!("duplicate version: {}", version_ast.version)));
            }
            versions.push(version_ast);
        }

        self.expect(Token::RBrace, "'}'")?;
        let span = start_span.join(self.last_consumed_span());
        Ok(SchemaAst { name, imports, versions, span, line: start_line })
    }

    fn parse_version(&mut self) -> Result<VersionAst, ParseError> {
        let (start_span, start_line) = self.here();
        self.expect_keyword(Keyword::Version)?;
        let version = self.expect_number()?;
        self.expect(Token::LBrace, "'{'")?;

        let mut seen_sections: HashSet<&'static str> = HashSet::new();
        let mut seen_type_names: HashSet<String> = HashSet::new();
        let mut seen_union_names: HashSet<String> = HashSet::new();
        let mut seen_enum_names: HashSet<String> = HashSet::new();
        let mut seen_bitset_names: HashSet<String> = HashSet::new();
        let mut blocks = Vec::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let block = self.parse_version_block()?;

            match &block {
                VersionBlockAst::Fields(_) => {
                    if !seen_sections.insert("fields") {
                        return Err(self.err_invalid(format!("version {} has duplicate `fields` block", version)));
                    }
                }
                VersionBlockAst::Diff(_) => {
                    if !seen_sections.insert("diff") {
                        return Err(self.err_invalid(format!("version {} has duplicate `diff` block", version)));
                    }
                }
                VersionBlockAst::TypeDef(td) => {
                    if !seen_type_names.insert(td.name.clone()) {
                        return Err(self.err_invalid(format!("version {} has duplicate type `{}`", version, td.name)));
                    }
                }
                VersionBlockAst::EnumDef(ed) => {
                    if !seen_enum_names.insert(ed.name().to_string()) {
                        return Err(self.err_invalid(format!("version {} has duplicate enum `{}`", version, ed.name())));
                    }
                }
                VersionBlockAst::UnionDef(ud) => {
                    if !seen_union_names.insert(ud.name().to_string()) {
                        return Err(self.err_invalid(format!("version {} has duplicate union `{}`", version, ud.name())));
                    }
                }
                VersionBlockAst::BitsetDef(bd) => {
                    if !seen_bitset_names.insert(bd.name().to_string()) {
                        return Err(self.err_invalid(format!("version {} has duplicate bitset `{}`", version, bd.name())));
                    }
                }
            }

            blocks.push(block);
        }

        self.expect(Token::RBrace, "'}'")?;
        let span = start_span.join(self.last_consumed_span());
        Ok(VersionAst { version, blocks, span, line: start_line })
    }

    fn parse_version_block(&mut self) -> Result<VersionBlockAst, ParseError> {
        match self.peek().clone() {
            Token::Keyword(Keyword::Enum) => Ok(VersionBlockAst::EnumDef(self.parse_enum_def()?)),
            Token::Keyword(Keyword::Union) => Ok(VersionBlockAst::UnionDef(self.parse_union_def()?)),
            Token::Keyword(Keyword::Type) => Ok(VersionBlockAst::TypeDef(self.parse_type_def()?)),
            Token::Keyword(Keyword::Bitset) => Ok(VersionBlockAst::BitsetDef(self.parse_bitset_def()?)),
            Token::Keyword(Keyword::Fields) => Ok(VersionBlockAst::Fields(self.parse_fields_block()?)),
            Token::Keyword(Keyword::Diff) => Ok(VersionBlockAst::Diff(self.parse_diff_block()?)),
            got => Err(self.err_unexpected(got, "enum / union / type / fields / diff")),
        }
    }

    fn parse_type_def(&mut self) -> Result<TypeDefAst, ParseError> {
        let (start_span, start_line) = self.here();
        self.expect_keyword(Keyword::Type)?;
        let name = self.expect_ident()?;

        let extends = if matches!(self.peek(), Token::Keyword(Keyword::Extends)) {
            let (ext_span, ext_line) = self.here();
            self.advance();
            let parent_name = self.expect_ident()?;
            self.expect(Token::At, "'@'")?;
            let version = self.expect_number()?;
            let span = ext_span.join(self.last_consumed_span());
            Some(ExtendsAst { name: parent_name, version, span, line: ext_line })
        } else {
            None
        };

        self.expect(Token::LBrace, "'{'")?;
        let body = if extends.is_some() {
            TypeBody::Diff(self.parse_diff_ops()?)
        } else {
            TypeBody::Fields(self.parse_field_list()?)
        };
        self.expect(Token::RBrace, "'}'")?;

        let span = start_span.join(self.last_consumed_span());
        Ok(TypeDefAst { name, extends, body, span, line: start_line })
    }

    fn parse_enum_def(&mut self) -> Result<EnumDefAst, ParseError> {
        let (start_span, start_line) = self.here();
        self.expect_keyword(Keyword::Enum)?;
        let name = self.expect_ident()?;

        if matches!(self.peek(), Token::Keyword(Keyword::Extends)) {
            let (ext_span, ext_line) = self.here();
            self.advance();
            let base_name = self.expect_ident()?;
            self.expect(Token::At, "'@'")?;
            let base_version = self.expect_number()?;
            let base_span = ext_span.join(self.last_consumed_span());
            let base = ExtendsAst { name: base_name, version: base_version, span: base_span, line: ext_line };

            self.expect(Token::LBrace, "'{'")?;
            let ops = self.parse_enum_ops()?;
            self.expect(Token::RBrace, "'}'")?;

            let span = start_span.join(self.last_consumed_span());
            return Ok(EnumDefAst::Extension { name, base, ops, span, line: start_line });
        }

        self.expect(Token::LBrace, "'{'")?;
        let mut variants = Vec::new();
        let mut seen = HashSet::new();
        while matches!(self.peek(), Token::Identifier(_)) {
            let (v_span, v_line) = self.here();
            let variant = self.expect_ident()?;
            if !seen.insert(variant.clone()) {
                return Err(self.err_invalid(format!("duplicate enum variant `{}`", variant)));
            }
            if matches!(self.peek(), Token::Comma) { self.advance(); }
            variants.push(EnumVariantNode { name: variant, span: v_span, line: v_line });
        }
        self.expect(Token::RBrace, "'}'")?;
        let span = start_span.join(self.last_consumed_span());
        Ok(EnumDefAst::Definition { name, variants, span, line: start_line })
    }

    fn parse_enum_ops(&mut self) -> Result<Vec<EnumVariantOpAst>, ParseError> {
        let mut ops = Vec::new();
        let mut seen = HashSet::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let (op_span, op_line) = self.here();
            let op = match self.peek().clone() {
                Token::Plus => {
                    self.advance();
                    let name = self.expect_ident()?;
                    if matches!(self.peek(), Token::Comma) { self.advance(); }
                    EnumVariantOpAst::Add { name, span: op_span.join(self.last_consumed_span()), line: op_line }
                }
                Token::Tilde => {
                    self.advance();
                    let from = self.expect_ident()?;
                    self.expect(Token::Arrow, "'->'")?;
                    let to = self.expect_ident()?;
                    if matches!(self.peek(), Token::Comma) { self.advance(); }
                    EnumVariantOpAst::Rename { from, to, span: op_span.join(self.last_consumed_span()), line: op_line }
                }
                Token::Minus => return Err(self.err_invalid(
                    "enum variants cannot be removed — removing a variant makes old wire values undecodable",
                )),
                got => return Err(self.err_unexpected(got, "+ (add) or ~ (rename) in enum extension")),
            };

            let key = match &op {
                EnumVariantOpAst::Add { name, .. } => name.clone(),
                EnumVariantOpAst::Rename { from, .. } => from.clone(),
            };
            if !seen.insert(key.clone()) {
                return Err(self.err_invalid(format!("variant `{}` appears more than once in enum ops", key)));
            }
            ops.push(op);
        }
        Ok(ops)
    }

    fn parse_bitset_def(&mut self) -> Result<BitsetDefAst, ParseError> {
        let (start_span, start_line) = self.here();
        self.expect_keyword(Keyword::Bitset)?;
        let name = self.expect_ident()?;

        if matches!(self.peek(), Token::Keyword(Keyword::Extends)) {
            let (ext_span, ext_line) = self.here();
            self.advance();
            let base_name = self.expect_ident()?;
            self.expect(Token::At, "'@'")?;
            let base_version = self.expect_number()?;
            let base_span = ext_span.join(self.last_consumed_span());
            let base = ExtendsAst { name: base_name, version: base_version, span: base_span, line: ext_line };

            self.expect(Token::LBrace, "'{'")?;
            let ops = self.parse_bitset_ops()?;
            self.expect(Token::RBrace, "'}'")?;

            let span = start_span.join(self.last_consumed_span());
            return Ok(BitsetDefAst::Extension { name, base, ops, span, line: start_line });
        }

        self.expect(Token::LBrace, "'{'")?;
        let mut variants = Vec::new();
        let mut seen = HashSet::new();

        while matches!(self.peek(), Token::Identifier(_)) {
            let v = self.expect_ident()?;
            if !seen.insert(v.clone()) {
                return Err(self.err_invalid(format!("duplicate bitset variant `{}`", v)));
            }
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            }
            variants.push(v);
        }

        self.expect(Token::RBrace, "'}'")?;

        if variants.is_empty() {
            return Err(self.err_invalid(format!("bitset `{}` must have at least one variant", name)));
        }
        if variants.len() > 32 {
            return Err(self.err_invalid(format!("bitset `{}` exceeds 32 variants", name)));
        }

        let span = start_span.join(self.last_consumed_span());
        Ok(BitsetDefAst::Definition { name, variants, span, line: start_line })
    }

    fn parse_bitset_ops(&mut self) -> Result<Vec<BitsetOpAst>, ParseError> {
        let mut ops = Vec::new();
        let mut seen = HashSet::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let (op_span, op_line) = self.here();
            let op = match self.peek().clone() {
                Token::Plus => {
                    self.advance();
                    let name = self.expect_ident()?;
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }
                    BitsetOpAst::Add {name, span: op_span.join(self.last_consumed_span()), line: op_line}
                }
                Token::Minus => {
                    self.advance();
                    let name = self.expect_ident()?;
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }
                    BitsetOpAst::Remove {name, span: op_span.join(self.last_consumed_span()), line: op_line}
                }
                got => return Err(self.err_unexpected(got, "+ (add) or - (remove) in bitset extension")),
            };

            let key = match &op {
                BitsetOpAst::Add { name, .. } => name.clone(),
                BitsetOpAst::Remove { name, .. } => name.clone(),
            };
            if !seen.insert(key.clone()) {
                return Err(self.err_invalid(format!("variant `{}` appears more than once in bitset ops", key)));
            }
            ops.push(op);
        }
        Ok(ops)
    }

    fn parse_union_def(&mut self) -> Result<UnionDefAst, ParseError> {
        let (start_span, start_line) = self.here();
        self.expect_keyword(Keyword::Union)?;
        let name = self.expect_ident()?;

        if matches!(self.peek(), Token::Keyword(Keyword::Extends)) {
            let (ext_span, ext_line) = self.here();
            self.advance();
            let base_name = self.expect_ident()?;
            self.expect(Token::At, "'@'")?;
            let base_version = self.expect_number()?;
            let base_span = ext_span.join(self.last_consumed_span());
            let base = ExtendsAst { name: base_name, version: base_version, span: base_span, line: ext_line };

            self.expect(Token::LBrace, "'{'")?;
            let ops = self.parse_union_ops()?;
            self.expect(Token::RBrace, "'}'")?;

            let span = start_span.join(self.last_consumed_span());
            return Ok(UnionDefAst::Extension { name, base, ops, span, line: start_line });
        }

        self.expect(Token::LBrace, "'{'")?;
        let mut variants = Vec::new();
        let mut seen = HashSet::new();

        while matches!(self.peek(), Token::Identifier(_)) {
            let (v_span, v_line) = self.here();
            let variant_name = self.expect_ident()?;
            if !seen.insert(variant_name.clone()) {
                return Err(self.err_invalid(format!("duplicate union variant `{}`", variant_name)));
            }

            self.expect(Token::Colon, "':'")?;
            let payload_ty = self.expect_ident()?;

            if matches!(self.peek(), Token::Comma) {
                self.advance();
            }

            let span = v_span.join(self.last_consumed_span());
            variants.push(UnionVariantAst { name: variant_name, payload_ty, span, line: v_line });
        }

        self.expect(Token::RBrace, "'}'")?;

        if variants.is_empty() {
            return Err(self.err_invalid(format!("union `{}` must have at least one variant", name)));
        }

        let span = start_span.join(self.last_consumed_span());
        Ok(UnionDefAst::Definition { name, variants, span, line: start_line })
    }

    fn parse_union_ops(&mut self) -> Result<Vec<UnionVariantOpAst>, ParseError> {
        let mut ops = Vec::new();
        let mut seen = HashSet::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let (op_span, op_line) = self.here();
            match self.peek().clone() {
                Token::Plus => {
                    self.advance();
                    let name = self.expect_ident()?;
                    self.expect(Token::Colon, "':'")?;
                    let payload_ty = self.expect_ident()?;

                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }

                    if !seen.insert(name.clone()) {
                        return Err(self.err_invalid(format!("variant `{}` appears more than once in union ops", name)));
                    }

                    ops.push(UnionVariantOpAst::Add { name, payload_ty, span: op_span.join(self.last_consumed_span()), line: op_line });
                }
                Token::Minus => {
                    return Err(self.err_invalid("union variants cannot be removed"));
                }
                got => return Err(self.err_unexpected(got, "+ (add) in union extension")),
            }
        }

        Ok(ops)
    }

    fn parse_fields_block(&mut self) -> Result<FieldsAst, ParseError> {
        self.expect_keyword(Keyword::Fields)?;
        self.expect(Token::LBrace, "'{'")?;
        let fields = self.parse_field_list()?;
        self.expect(Token::RBrace, "'}'")?;
        Ok(fields)
    }

    fn parse_field_list(&mut self) -> Result<FieldsAst, ParseError> {
        let mut fields = Vec::new();
        let mut const_fields = Vec::new();
        let mut seen_names: HashSet<String> = HashSet::new();

        while matches!(self.peek(), Token::Identifier(_)) {
            let (start_span, start_line) = self.here();
            let name = self.expect_ident()?;
            if !seen_names.insert(name.clone()) {
                return Err(self.err_invalid(format!("duplicate field name: {}", name)));
            }
            self.expect(Token::Colon, "':'")?;

            let is_const = matches!(self.peek(), Token::Keyword(Keyword::Const));
            let is_lazy = matches!(self.peek(), Token::Keyword(Keyword::Lazy));
            if is_const && is_lazy {
                return Err(self.err_invalid("a field cannot be both `const` and `lazy`"));
            }

            if is_const {
                self.advance();
                let ty = self.parse_type()?;
                self.expect(Token::Equals, "'='")?;
                let value = self.parse_default()?;
                let span = start_span.join(self.last_consumed_span());
                const_fields.push(ConstFieldAst { name, ty, value, span, line: start_line });
            } else {
                let lazy = if is_lazy { self.advance(); true } else { false };
                let ty = self.parse_type()?;
                let default = if matches!(self.peek(), Token::Equals) {
                    self.advance();
                    Some(self.parse_default()?)
                } else { None };
                let span = start_span.join(self.last_consumed_span());
                fields.push(FieldAst { name, ty, default, lazy, span, line: start_line });
            }
        }
        Ok(FieldsAst { fields, const_fields })
    }

    fn parse_type(&mut self) -> Result<TypeAst, ParseError> {
        let mut base = match self.peek().clone() {
            Token::LBracket => {
                self.advance();
                let inner = self.parse_type()?;
                self.expect(Token::RBracket, "']'")?;
                TypeAst::Array(Box::new(inner))
            }
            Token::Identifier(_) => {
                let name = self.expect_ident()?;

                if matches!(self.peek(), Token::At) {
                    self.advance(); // consume @
                    let version = self.expect_number()?;
                    TypeAst::Imported { alias: name, version }
                } else if name == "vfloat" {
                    self.parse_vfloat_params()?
                } else if name == "map" {
                    self.expect(Token::LAngle, "'<'")?;
                    let k = self.parse_type()?;
                    self.expect(Token::Comma, "','")?;
                    let v = self.parse_type()?;
                    self.expect(Token::RAngle, "'>'")?;
                    TypeAst::Map(Box::new(k), Box::new(v))
                } else {
                    TypeAst::Named(name)
                }
            }
            Token::LParen => {
                self.advance();
                let mut elements = Vec::new();

                loop {
                    elements.push(self.parse_type()?);
                    match self.peek().clone() {
                        Token::Comma => { self.advance(); }
                        Token::RParen => { self.advance(); break; }
                        got => return Err(self.err_unexpected(got, "',' or ')'")),
                    }
                }

                if elements.len() < 2 {
                    return Err(self.err_invalid("tuple must have at least 2 elements"));
                }

                TypeAst::Tuple(elements)
            }

            got => return Err(self.err_unexpected(got, "type name or '['")),
        };

        if matches!(self.peek(), Token::LParen) {
            self.advance();

            if matches!(self.peek(), Token::Keyword(Keyword::Delta)) {
                self.advance();

                let fixed_len = if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    Some(self.expect_number()? as usize)
                } else {
                    None
                };

                self.expect(Token::RParen, "')'")?;

                base = match base {
                    TypeAst::Array(inner) => match fixed_len {
                        Some(n) => TypeAst::FixedDeltaArray(inner, n),
                        None => TypeAst::DeltaArray(inner),
                    },
                    other => {
                        return Err(self.err_invalid(format!(
                            "`(delta)` is only valid on integer arrays, not `{:?}`",
                            other
                        )))
                    }
                };
            } else {
                let n = self.expect_number()? as usize;

                if matches!(self.peek(), Token::Comma) {
                    return Err(self.err_invalid("unexpected `,` after size; did you mean `(delta, N)`?"));
                }

                self.expect(Token::RParen, "')'")?;

                base = match base {
                    TypeAst::Array(inner) => TypeAst::FixedArray(inner, n),
                    TypeAst::Named(ref name) if name == "string" || name == "str" => TypeAst::FixedString(n),
                    TypeAst::Map(k, v) => TypeAst::FixedMap(k, v, n),
                    other => {
                        return Err(self.err_invalid(format!(
                            "`(N)` suffix is only valid on arrays, maps and `string`, not `{:?}`",
                            other
                        )))
                    }
                };
            }
        }

        if matches!(self.peek(), Token::QuestionMark) {
            self.advance();
            base = TypeAst::Optional(Box::new(base));
        }

        Ok(base)
    }

    fn parse_diff_block(&mut self) -> Result<Vec<DiffAst>, ParseError> {
        self.expect_keyword(Keyword::Diff)?;
        self.expect(Token::LBrace, "'{'")?;
        let ops = self.parse_diff_ops()?;
        self.expect(Token::RBrace, "'}'")?;
        Ok(ops)
    }

    fn parse_diff_ops(&mut self) -> Result<Vec<DiffAst>, ParseError> {
        let mut ops = Vec::new();
        let mut seen = HashSet::new();

        loop {
            match self.peek().clone() {
                Token::RBrace | Token::Eof => break,
                Token::Plus => {
                    let op = self.parse_diff_add()?;
                    let name = match &op {
                        DiffAst::Add { field } => &field.name,
                        DiffAst::AddConst { field } => &field.name,
                        _ => unreachable!(),
                    };
                    if !seen.insert(name.clone()) {
                        return Err(self.err_invalid(format!("field `{}` has multiple diff operations", name)));
                    }
                    ops.push(op);
                }
                Token::Minus => {
                    let op = self.parse_diff_remove()?;
                    let name = match &op {
                        DiffAst::Remove { name, .. } => name,
                        _ => unreachable!(),
                    };
                    if !seen.insert(name.clone()) {
                        return Err(self.err_invalid(format!("field `{}` has multiple diff operations", name)));
                    }
                    ops.push(op);
                }
                Token::Tilde => {
                    let op = self.parse_diff_tilde()?;
                    let name = match &op {
                        DiffAst::Rename { from, .. } => from,
                        DiffAst::UpdateType { name, .. } => name,
                        DiffAst::Transform { from, .. } => from,
                        _ => unreachable!(),
                    };
                    if !seen.insert(name.clone()) {
                        return Err(self.err_invalid(format!("field `{}` has multiple diff operations", name)));
                    }
                    ops.push(op);
                }
                got => return Err(self.err_unexpected(got, "+ / - / ~")),
            }
        }

        Ok(ops)
    }

    fn parse_diff_add(&mut self) -> Result<DiffAst, ParseError> {
        let (start_span, start_line) = self.here();
        self.advance();
        let name = self.expect_ident()?;
        self.expect(Token::Colon, "':'")?;

        let is_const = matches!(self.peek(), Token::Keyword(Keyword::Const));
        let is_lazy = matches!(self.peek(), Token::Keyword(Keyword::Lazy));

        if is_const && is_lazy {
            return Err(self.err_invalid("a field cannot be both `const` and `lazy`"));
        }

        if is_const {
            self.advance();
            let ty = self.parse_type()?;
            self.expect(Token::Equals, "'='")?;
            let value = self.parse_default()?;
            Ok(DiffAst::AddConst { field: ConstFieldAst { name, ty, value, span: start_span.join(self.last_consumed_span()), line: start_line } })
        } else {
            let lazy = if is_lazy { self.advance(); true } else { false };
            let ty = self.parse_type()?;
            let default = if matches!(self.peek(), Token::Equals) {
                self.advance();
                Some(self.parse_default()?)
            } else {
                None
            };
            Ok(DiffAst::Add { field: FieldAst { name, ty, default, lazy, span: start_span.join(self.last_consumed_span()), line: start_line } })
        }
    }

    fn parse_diff_remove(&mut self) -> Result<DiffAst, ParseError> {
        let (start_span, start_line) = self.here();
        self.advance();
        let name = self.expect_ident()?;
        Ok(DiffAst::Remove { name, span: start_span.join(self.last_consumed_span()), line: start_line })
    }

    fn parse_diff_tilde(&mut self) -> Result<DiffAst, ParseError> {
        let (start_span, start_line) = self.here();
        self.advance();

        let from = self.expect_ident()?;

        let mut rename_to: Option<String> = None;
        let mut ty: Option<TypeAst> = None;
        let mut lazy = false;

        if matches!(self.peek(), Token::Arrow) {
            self.advance();
            rename_to = Some(self.expect_ident()?);
        }

        if matches!(self.peek(), Token::Colon) {
            self.advance();

            if matches!(self.peek(), Token::Keyword(Keyword::Const)) {
                self.advance();
                let ty = self.parse_type()?;
                self.expect(Token::Equals, "'='")?;
                let value = self.parse_default()?;
                return Ok(match rename_to {
                    Some(to) => DiffAst::TransformConst { from, to, ty, value, span: start_span.join(self.last_consumed_span()), line: start_line },
                    None => DiffAst::UpdateConst { name: from, ty, value, span: start_span.join(self.last_consumed_span()), line: start_line },
                });
            }

            if matches!(self.peek(), Token::Keyword(Keyword::Lazy)) {
                self.advance();
                lazy = true;
            }
            ty = Some(self.parse_type()?);
        }

        match (rename_to, ty) {
            (Some(to), Some(ty)) => Ok(DiffAst::Transform { from, to, ty: Some(ty), lazy, span: start_span.join(self.last_consumed_span()), line: start_line }),
            (Some(to), None) => Ok(DiffAst::Rename { from, to, span: start_span.join(self.last_consumed_span()), line: start_line }),
            (None, Some(ty)) => Ok(DiffAst::UpdateType { name: from, ty, lazy, span: start_span.join(self.last_consumed_span()), line: start_line }),
            (None, None) => Err(self.err_unexpected(self.peek().clone(), "->, : or combination")),
        }
    }

    fn parse_vfloat_params(&mut self) -> Result<TypeAst, ParseError> {
        self.expect(Token::LParen, "'('")?;

        let mut min: Option<f64> = None;
        let mut max: Option<f64> = None;
        let mut step: Option<f64> = None;

        while !matches!(self.peek(), Token::RParen | Token::Eof) {
            let key = self.expect_ident()?;
            self.expect(Token::Colon, "':'")?;
            let value = self.parse_vfloat_number()?;

            let slot = match key.as_str() {
                "min" => &mut min,
                "max" => &mut max,
                "step" => &mut step,
                other => {
                    return Err(self.err_invalid(format!(
                        "unknown vfloat parameter `{}` (expected `min`, `max`, or `step`)",
                        other
                    )))
                }
            };

            if slot.replace(value).is_some() {
                return Err(self.err_invalid(format!("duplicate `{}` in vfloat", key)));
            }

            if matches!(self.peek(), Token::Comma) {
                self.advance();
            }
        }

        self.expect(Token::RParen, "')'")?;

        match (min, max, step) {
            (Some(min), Some(max), Some(step)) => Ok(TypeAst::VFloat { min, max, step }),
            _ => Err(self.err_invalid("vfloat requires `min`, `max`, and `step`")),
        }
    }

    fn parse_vfloat_number(&mut self) -> Result<f64, ParseError> {
        let negative = if matches!(self.peek(), Token::Minus) {
            self.advance();
            true
        } else {
            false
        };

        let (span, line) = self.here();
        let val = match self.advance() {
            Token::FloatLiteral(f) => f.parse::<f64>().map_err(|_| {
                self.err_invalid_at(format!("Invalid float literal context: {}", f), span, line)
            })?,
            Token::IntLiteral(n) => n.parse::<f64>().map_err(|_| {
                self.err_invalid_at(format!("Invalid number context for float translation: {}", n), span, line)
            })?,
            got => return Err(self.err_unexpected_at(got, "number", span, line)),
        };

        Ok(if negative { -val } else { val })
    }

    fn parse_default(&mut self) -> Result<DefaultValueAst, ParseError> {
        match self.peek().clone() {
            Token::Keyword(Keyword::True) => {
                self.advance();
                Ok(DefaultValueAst::Bool(true))
            }
            Token::Keyword(Keyword::False) => {
                self.advance();
                Ok(DefaultValueAst::Bool(false))
            }
            Token::StringLiteral(s) => {
                self.advance();
                Ok(DefaultValueAst::Str(s.to_string()))
            }
            Token::FloatLiteral(f) => {
                let (span, line) = self.here();
                self.advance();
                let val = f.parse::<f64>().map_err(|_| self.err_invalid_at(format!("Invalid float: {}", f), span, line))?;
                Ok(DefaultValueAst::Float(val))
            }
            Token::IntLiteral(n) => {
                let (span, line) = self.here();
                self.advance();
                let val = n.parse::<i128>().map_err(|_| {
                    self.err_invalid_at(format!("Invalid integer (exceeds i64): {}", n), span, line)
                })?;
                Ok(DefaultValueAst::Int(val))
            }
            Token::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                while !matches!(self.peek(), Token::RBracket | Token::Eof) {
                    elements.push(self.parse_default()?);
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }
                }
                self.expect(Token::RBracket, "']'")?;
                Ok(DefaultValueAst::Array(elements))
            }
            Token::LBrace => {
                self.advance();
                let mut pairs = Vec::new();
                while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                    let k = self.parse_default()?;
                    self.expect(Token::Colon, "':'")?;
                    let v = self.parse_default()?;
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }
                    pairs.push((k, v));
                }
                self.expect(Token::RBrace, "'}'")?;
                Ok(DefaultValueAst::Map(pairs))
            }
            Token::Minus => {
                self.advance();
                let (span, line) = self.here();
                match self.peek().clone() {
                    Token::FloatLiteral(f) => {
                        self.advance();
                        let val = f.parse::<f64>().map_err(|_| self.err_invalid_at(format!("Invalid float: {}", f), span, line))?;
                        Ok(DefaultValueAst::Float(-val))
                    }
                    Token::IntLiteral(n) => {
                        self.advance();
                        let val = n.parse::<i128>().map_err(|_| {
                            self.err_invalid_at(format!("Invalid integer (exceeds i64): {}", n), span, line)
                        })?;
                        Ok(DefaultValueAst::Int(-val))
                    }
                    got => Err(self.err_unexpected_at(got, "number after '-'", span, line)),
                }
            }
            Token::LParen => {
                self.advance();
                let mut elements = Vec::new();

                loop {
                    elements.push(self.parse_default()?);
                    match self.peek().clone() {
                        Token::Comma => { self.advance(); }
                        Token::RParen => { self.advance(); break; }
                        got => return Err(self.err_unexpected(got, "',' or ')'")),
                    }
                }

                Ok(DefaultValueAst::Tuple(elements))
            }
            Token::Identifier(ty) => {
                self.advance();

                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let mut kvs = Vec::new();
                    let mut seen = HashSet::new();

                    while !matches!(self.peek(), Token::RParen | Token::Eof) {
                        let flag_name = self.expect_ident()?;
                        self.expect(Token::Colon, "':'")?;

                        let (span, line) = self.here();
                        let val = match self.advance() {
                            Token::Keyword(Keyword::True) => true,
                            Token::Keyword(Keyword::False) => false,
                            got => return Err(self.err_unexpected_at(got, "true or false", span, line)),
                        };

                        if !seen.insert(flag_name.clone()) {
                            return Err(self.err_invalid(format!("duplicate default assignment for flag `{}`", flag_name)));
                        }

                        kvs.push((flag_name, val));

                        if matches!(self.peek(), Token::Comma) {
                            self.advance();
                        }
                    }
                    self.expect(Token::RParen, "')'")?;
                    Ok(DefaultValueAst::BitsetLiteral { ty: ty.to_string(), kvs })
                } else {
                    self.expect(Token::ColonColon, "'::'")?;
                    let variant = self.expect_ident()?;
                    Ok(DefaultValueAst::EnumVariant { ty: ty.to_string(), variant })
                }
            }
            got => Err(self.err_unexpected(got, "default value")),
        }
    }
}