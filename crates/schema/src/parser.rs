use crate::ast::*;
use crate::error::*;
use crate::lexer::{Keyword, Token};
use std::collections::HashSet;

pub struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<(Token, usize)>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).map(|(tok, _)| tok).unwrap_or(&Token::Eof)
    }
    fn current_line(&self) -> usize {
        self.tokens.get(self.pos).map(|(_, line)| *line).unwrap_or(0)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).map(|(tok, _)| tok.clone()).unwrap_or(Token::Eof);
        if !matches!(tok, Token::Eof) {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, want: Token, label: &'static str) -> Result<(), ParseError> {
        let got = self.advance();
        let line = self.current_line();
        if got == want {
            Ok(())
        } else {
            Err(ParseError::UnexpectedToken {
                got,
                expected: label,
                line,
            })
        }
    }

    fn expect_keyword(&mut self, kw: Keyword) -> Result<(), ParseError> {
        let got = self.advance();
        match &got {
            Token::Keyword(k) if *k == kw => Ok(()),
            _ => Err(ParseError::UnexpectedToken {
                got,
                expected: "keyword",
                line: self.current_line(),
            }),
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        let got = self.advance();
        match got {
            Token::Identifier(s) => Ok(s.to_string()),
            _ => Err(ParseError::UnexpectedToken {
                got,
                expected: "identifier",
                line: self.current_line(),
            }),
        }
    }

    fn expect_number(&mut self) -> Result<i128, ParseError> {
        let got = self.advance();
        match got {
            Token::IntLiteral(s) => s.parse::<i128>().map_err(|_| {
                ParseError::InvalidSyntax(format!("Number too large or invalid: {}", s), self.current_line())
            }),
            _ => Err(ParseError::UnexpectedToken {
                got,
                expected: "number",
                line: self.current_line(),
            }),
        }
    }

    pub fn parse_schema(&mut self) -> Result<SchemaAst, ParseError> {
        self.expect_keyword(Keyword::Schema)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace, "'{'")?;

        let mut versions = Vec::new();
        let mut seen_versions = HashSet::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let version_ast = self.parse_version()?;

            if !seen_versions.insert(version_ast.version) {
                return Err(ParseError::InvalidSyntax(format!(
                    "duplicate version: {}",
                    version_ast.version
                ), self.current_line()));
            }

            versions.push(version_ast);
        }

        self.expect(Token::RBrace, "'}'")?;
        Ok(SchemaAst { name, versions })
    }

    fn parse_version(&mut self) -> Result<VersionAst, ParseError> {
        self.expect_keyword(Keyword::Version)?;
        let version = self.expect_number()?;
        self.expect(Token::LBrace, "'{'")?;

        let mut seen_sections: HashSet<&'static str> = HashSet::new(); // fields / diff only
        let mut seen_type_names: HashSet<String> = HashSet::new();
        let mut seen_enum_names: HashSet<String> = HashSet::new();
        let mut seen_bitset_names: HashSet<String> = HashSet::new();
        let mut blocks = Vec::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let block = self.parse_version_block()?;

            match &block {
                VersionBlockAst::Fields(_) => {
                    if !seen_sections.insert("fields") {
                        return Err(ParseError::InvalidSyntax(format!(
                            "version {} has duplicate `fields` block",
                            version
                        ), self.current_line()));
                    }
                }
                VersionBlockAst::Diff(_) => {
                    if !seen_sections.insert("diff") {
                        return Err(ParseError::InvalidSyntax(format!(
                            "version {} has duplicate `diff` block",
                            version
                        ), self.current_line()));
                    }
                }
                VersionBlockAst::TypeDef(td) => {
                    if !seen_type_names.insert(td.name.clone()) {
                        return Err(ParseError::InvalidSyntax(format!(
                            "version {} has duplicate type `{}`",
                            version, td.name
                        ), self.current_line()));
                    }
                }
                VersionBlockAst::EnumDef(ed) => {
                    if !seen_enum_names.insert(ed.name().to_string()) {
                        return Err(ParseError::InvalidSyntax(format!(
                            "version {} has duplicate enum `{}`",
                            version,
                            ed.name()
                        ), self.current_line()));
                    }
                }
                VersionBlockAst::BitsetDef(bd) => {
                    if !seen_bitset_names.insert(bd.name().to_string()) {
                        return Err(ParseError::InvalidSyntax(format!(
                            "version {} has duplicate bitset `{}`",
                            version,
                            bd.name()
                        ), self.current_line()));
                    }
                }
            }

            blocks.push(block);
        }

        self.expect(Token::RBrace, "'}'")?;
        Ok(VersionAst { version, blocks })
    }

    fn parse_version_block(&mut self) -> Result<VersionBlockAst, ParseError> {
        match self.peek().clone() {
            Token::Keyword(Keyword::Enum) => Ok(VersionBlockAst::EnumDef(self.parse_enum_def()?)),
            Token::Keyword(Keyword::Type) => Ok(VersionBlockAst::TypeDef(self.parse_type_def()?)),
            Token::Keyword(Keyword::Bitset) => {
                Ok(VersionBlockAst::BitsetDef(self.parse_bitset_def()?))
            }
            Token::Keyword(Keyword::Fields) => {
                Ok(VersionBlockAst::Fields(self.parse_fields_block()?))
            }
            Token::Keyword(Keyword::Diff) => Ok(VersionBlockAst::Diff(self.parse_diff_block()?)),
            got => Err(ParseError::UnexpectedToken {
                got,
                expected: "enum / type / fields / diff",
                line: self.current_line(),
            }),
        }
    }

    fn parse_type_def(&mut self) -> Result<TypeDefAst, ParseError> {
        self.expect_keyword(Keyword::Type)?;
        let name = self.expect_ident()?;

        let extends = if matches!(self.peek(), Token::Keyword(Keyword::Extends)) {
            self.advance(); // consume 'extends'
            let parent_name = self.expect_ident()?;
            self.expect(Token::At, "'@'")?;
            let version = self.expect_number()?;
            Some(ExtendsAst {
                name: parent_name,
                version,
            })
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

        Ok(TypeDefAst {
            name,
            extends,
            body,
        })
    }

    fn parse_enum_def(&mut self) -> Result<EnumDefAst, ParseError> {
        self.expect_keyword(Keyword::Enum)?;
        let name = self.expect_ident()?;

        // optional: extends Status@1
        if matches!(self.peek(), Token::Keyword(Keyword::Extends)) {
            self.advance();
            let base_name = self.expect_ident()?;
            self.expect(Token::At, "'@'")?;
            let base_version = self.expect_number()?;
            let base = ExtendsAst {
                name: base_name,
                version: base_version,
            };

            self.expect(Token::LBrace, "'{'")?;
            let ops = self.parse_enum_ops()?;
            self.expect(Token::RBrace, "'}'")?;

            return Ok(EnumDefAst::Extension { name, base, ops });
        }

        // fresh definition
        self.expect(Token::LBrace, "'{'")?;
        let mut variants = Vec::new();
        let mut seen = HashSet::new();

        while matches!(self.peek(), Token::Identifier(_)) {
            let variant = self.expect_ident()?;
            if !seen.insert(variant.clone()) {
                return Err(ParseError::InvalidSyntax(format!(
                    "duplicate enum variant `{}`",
                    variant
                ), self.current_line()));
            }
            // optional trailing comma
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            }
            variants.push(variant);
        }

        self.expect(Token::RBrace, "'}'")?;
        Ok(EnumDefAst::Definition { name, variants })
    }

    fn parse_enum_ops(&mut self) -> Result<Vec<EnumVariantOpAst>, ParseError> {
        let mut ops = Vec::new();
        let mut seen = HashSet::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let op = match self.peek().clone() {
                Token::Plus => {
                    self.advance();
                    let name = self.expect_ident()?;
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }
                    EnumVariantOpAst::Add(name)
                }
                Token::Tilde => {
                    self.advance();
                    let from = self.expect_ident()?;
                    self.expect(Token::Arrow, "'->'")?;
                    let to = self.expect_ident()?;
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }
                    EnumVariantOpAst::Rename { from, to }
                }
                Token::Minus => {
                    return Err(ParseError::InvalidSyntax(
                        "enum variants cannot be removed — removing a variant \
                        makes old wire values undecodable".to_string(),
                        self.current_line()
                    ));
                }
                got => {
                    return Err(ParseError::UnexpectedToken {
                        got,
                        expected: "+ (add) or ~ (rename) in enum extension",
                        line: self.current_line()
                    })
                }
            };

            // dedup by source name
            let key = match &op {
                EnumVariantOpAst::Add(n) => n.clone(),
                EnumVariantOpAst::Rename { from, .. } => from.clone(),
            };
            if !seen.insert(key.clone()) {
                return Err(ParseError::InvalidSyntax(format!(
                    "variant `{}` appears more than once in enum ops",
                    key
                ), self.current_line()));
            }

            ops.push(op);
        }

        Ok(ops)
    }

    fn parse_bitset_def(&mut self) -> Result<BitsetDefAst, ParseError> {
        self.expect_keyword(Keyword::Bitset)?;
        let name = self.expect_ident()?;

        // Check for "extends Type@Version"
        if matches!(self.peek(), Token::Keyword(Keyword::Extends)) {
            self.advance();
            let base_name = self.expect_ident()?;
            self.expect(Token::At, "'@'")?;
            let base_version = self.expect_number()?;
            let base = ExtendsAst {
                name: base_name,
                version: base_version,
            };

            self.expect(Token::LBrace, "'{'")?;
            let ops = self.parse_bitset_ops()?;
            self.expect(Token::RBrace, "'}'")?;

            return Ok(BitsetDefAst::Extension { name, base, ops });
        }

        // Standard baseline definition
        self.expect(Token::LBrace, "'{'")?;
        let mut variants = Vec::new();
        let mut seen = HashSet::new();

        while matches!(self.peek(), Token::Identifier(_)) {
            let v = self.expect_ident()?;
            if !seen.insert(v.clone()) {
                return Err(ParseError::InvalidSyntax(format!(
                    "duplicate bitset variant `{}`",
                    v
                ), self.current_line()));
            }
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            }
            variants.push(v);
        }

        self.expect(Token::RBrace, "'}'")?;

        if variants.is_empty() {
            return Err(ParseError::InvalidSyntax(format!(
                "bitset `{}` must have at least one variant",
                name
            ), self.current_line()));
        }
        if variants.len() > 32 {
            return Err(ParseError::InvalidSyntax(format!(
                "bitset `{}` exceeds 32 variants",
                name
            ), self.current_line()));
        }

        Ok(BitsetDefAst::Definition { name, variants })
    }

    fn parse_bitset_ops(&mut self) -> Result<Vec<BitsetOpAst>, ParseError> {
        let mut ops = Vec::new();
        let mut seen = HashSet::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let op = match self.peek().clone() {
                Token::Plus => {
                    self.advance();
                    let name = self.expect_ident()?;
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }
                    BitsetOpAst::Add(name)
                }
                Token::Minus => {
                    self.advance();
                    let name = self.expect_ident()?;
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    }
                    BitsetOpAst::Remove(name)
                }
                got => {
                    return Err(ParseError::UnexpectedToken {
                        got,
                        expected: "+ (add) or - (remove) in bitset extension",
                        line: self.current_line()
                    })
                }
            };

            let key = match &op {
                BitsetOpAst::Add(n) => n.clone(),
                BitsetOpAst::Remove(n) => n.clone(),
            };
            if !seen.insert(key.clone()) {
                return Err(ParseError::InvalidSyntax(format!(
                    "variant `{}` appears more than once in bitset ops",
                    key
                ), self.current_line()));
            }
            ops.push(op);
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
            let name = self.expect_ident()?;

            if !seen_names.insert(name.clone()) {
                return Err(ParseError::InvalidSyntax(format!(
                    "duplicate field name: {}", name
                ), self.current_line()));
            }

            self.expect(Token::Colon, "':'")?;

            if matches!(self.peek(), Token::Keyword(Keyword::Const)) {
                self.advance(); // consume 'const'
                let ty = self.parse_type()?;
                self.expect(Token::Equals, "'='")?;
                let value = self.parse_default()?;
                const_fields.push(ConstFieldAst { name, ty, value });
            } else {
                let ty = self.parse_type()?;
                let default = if matches!(self.peek(), Token::Equals) {
                    self.advance();
                    Some(self.parse_default()?)
                } else {
                    None
                };
                fields.push(FieldAst { name, ty, default });
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
            Token::Identifier(ref s) if s == "vfloat" => {
                self.advance(); // consume 'vfloat'
                return self.parse_vfloat_params();
            }
            Token::Identifier(ref s) if s == "map" => {
                self.advance();
                self.expect(Token::LAngle, "'<'")?;
                let k = self.parse_type()?;
                self.expect(Token::Comma, "','")?;
                let v = self.parse_type()?;
                self.expect(Token::RAngle, "'>'")?;
                TypeAst::Map(Box::new(k), Box::new(v))
            }
            Token::Identifier(_) => TypeAst::Named(self.expect_ident()?),
            Token::LParen => {
                self.advance();
                let mut elements = Vec::new();

                loop {
                    elements.push(self.parse_type()?);
                    match self.peek().clone() {
                        Token::Comma => {
                            self.advance();
                        }
                        Token::RParen => {
                            self.advance();
                            break;
                        }
                        got => {
                            return Err(ParseError::UnexpectedToken {
                                got,
                                expected: "',' or ')'",
                                line: self.current_line()
                            })
                        }
                    }
                }

                if elements.len() < 2 {
                    return Err(ParseError::InvalidSyntax(
                        "tuple must have at least 2 elements".to_string(),
                        self.current_line()
                    ));
                }

                TypeAst::Tuple(elements)
            }

            got => {
                return Err(ParseError::UnexpectedToken {
                    got,
                    expected: "type name or '['",
                    line: self.current_line()
                })
            }
        };

        if matches!(self.peek(), Token::LParen) {
            self.advance();

            if matches!(self.peek(), Token::Keyword(Keyword::Delta)) {
                self.advance(); // consume 'delta'

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
                    other => return Err(ParseError::InvalidSyntax(format!(
                        "`(delta)` is only valid on integer arrays, not `{:?}`",
                        other
                    ), self.current_line())),
                };
            } else {
                let n = self.expect_number()? as usize;

                if matches!(self.peek(), Token::Comma) {
                    return Err(ParseError::InvalidSyntax(
                        "unexpected `,` after size; did you mean `(delta, N)`?".to_string(),
                        self.current_line(),
                    ));
                }

                self.expect(Token::RParen, "')'")?;

                base = match base {
                    TypeAst::Array(inner) => TypeAst::FixedArray(inner, n),
                    TypeAst::Named(ref name) if name == "string" || name == "str" => {
                        TypeAst::FixedString(n)
                    }
                    TypeAst::Map(k, v) => TypeAst::FixedMap(k, v, n),
                    other => return Err(ParseError::InvalidSyntax(format!(
                        "`(N)` suffix is only valid on arrays, maps and `string`, not `{:?}`",
                        other
                    ), self.current_line())),
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
                        return Err(ParseError::InvalidSyntax(format!(
                            "field `{}` has multiple diff operations",
                            name
                        ), self.current_line()));
                    }

                    ops.push(op);
                }
                Token::Minus => {
                    let op = self.parse_diff_remove()?;
                    let name = match &op {
                        DiffAst::Remove { name } => name,
                        _ => unreachable!(),
                    };

                    if !seen.insert(name.clone()) {
                        return Err(ParseError::InvalidSyntax(format!(
                            "field `{}` has multiple diff operations",
                            name
                        ), self.current_line()));
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
                        return Err(ParseError::InvalidSyntax(format!(
                            "field `{}` has multiple diff operations",
                            name
                        ), self.current_line()));
                    }

                    ops.push(op);
                }
                got => {
                    return Err(ParseError::UnexpectedToken {
                        got,
                        expected: "+ / - / ~",
                        line: self.current_line()
                    })
                }
            }
        }

        Ok(ops)
    }

    // + position: Vector3
    fn parse_diff_add(&mut self) -> Result<DiffAst, ParseError> {
        self.advance(); // consume '+'
        let name = self.expect_ident()?;
        self.expect(Token::Colon, "':'")?;

        if matches!(self.peek(), Token::Keyword(Keyword::Const)) {
            self.advance(); // consume 'const'
            let ty = self.parse_type()?;
            self.expect(Token::Equals, "'='")?;
            let value = self.parse_default()?;
            Ok(DiffAst::AddConst {
                field: ConstFieldAst { name, ty, value },
            })
        } else {
            let ty = self.parse_type()?;
            let default = if matches!(self.peek(), Token::Equals) {
                self.advance();
                Some(self.parse_default()?)
            } else {
                None
            };
            Ok(DiffAst::Add {
                field: FieldAst { name, ty, default },
            })
        }
    }

    // - name
    fn parse_diff_remove(&mut self) -> Result<DiffAst, ParseError> {
        self.advance(); // consume '-'
        let name = self.expect_ident()?;
        Ok(DiffAst::Remove { name })
    }

    // ~ id -> player_id        ← rename
    // ~ level: float           ← type update
    // ~ existed -> age: float  ← transform (rename and type update)
    fn parse_diff_tilde(&mut self) -> Result<DiffAst, ParseError> {
        self.advance(); // consume '~'

        let from = self.expect_ident()?;

        let mut rename_to: Option<String> = None;
        let mut ty: Option<TypeAst> = None;

        // optional rename
        if matches!(self.peek(), Token::Arrow) {
            self.advance(); // consume '->'
            rename_to = Some(self.expect_ident()?);
        }

        // optional type
        if matches!(self.peek(), Token::Colon) {
            self.advance(); // consume ':'
            ty = Some(self.parse_type()?);
        }

        match (rename_to, ty) {
            (Some(to), Some(ty)) => Ok(DiffAst::Transform {
                from,
                to,
                ty: Some(ty),
            }),

            (Some(to), None) => Ok(DiffAst::Rename { from, to }),

            (None, Some(ty)) => Ok(DiffAst::UpdateType { name: from, ty }),

            (None, None) => Err(ParseError::UnexpectedToken {
                got: self.peek().clone(),
                expected: "->, : or combination",
                line: self.current_line()
            }),
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
                    return Err(ParseError::InvalidSyntax(format!(
                        "unknown vfloat parameter `{}` (expected `min`, `max`, or `step`)",
                        other
                    ), self.current_line()))
                }
            };

            if slot.replace(value).is_some() {
                return Err(ParseError::InvalidSyntax(format!(
                    "duplicate `{}` in vfloat", key
                ), self.current_line()));
            }

            if matches!(self.peek(), Token::Comma) {
                self.advance();
            }
        }

        self.expect(Token::RParen, "')'")?;

        match (min, max, step) {
            (Some(min), Some(max), Some(step)) => Ok(TypeAst::VFloat { min, max, step }),
            _ => Err(ParseError::InvalidSyntax(
                "vfloat requires `min`, `max`, and `step`".to_string(),
                self.current_line()
            )),
        }
    }

    fn parse_vfloat_number(&mut self) -> Result<f64, ParseError> {
        let negative = if matches!(self.peek(), Token::Minus) {
            self.advance();
            true
        } else {
            false
        };

        let val = match self.advance() {
            Token::FloatLiteral(f) => f.parse::<f64>().map_err(|_| {
                ParseError::InvalidSyntax(format!("Invalid float literal context: {}", f), self.current_line())
            })?,
            Token::IntLiteral(n) => n.parse::<f64>().map_err(|_| {
                ParseError::InvalidSyntax(format!("Invalid number context for float translation: {}", n), self.current_line())
            })?,
            got => {
                return Err(ParseError::UnexpectedToken {
                    got,
                    expected: "number",
                    line: self.current_line()
                })
            }
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
                self.advance();
                let val = f.parse::<f64>().map_err(|_| {
                    ParseError::InvalidSyntax(format!("Invalid float: {}", f), self.current_line())
                })?;
                Ok(DefaultValueAst::Float(val))
            }
            Token::IntLiteral(n) => {
                self.advance();
                let val = n.parse::<i128>().map_err(|_| {
                    ParseError::InvalidSyntax(format!("Invalid integer (exceeds i64): {}", n), self.current_line())
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
            // negative numbers
            Token::Minus => {
                self.advance();
                match self.peek().clone() {
                    Token::FloatLiteral(f) => {
                        self.advance();
                        let val = f.parse::<f64>().map_err(|_| {
                            ParseError::InvalidSyntax(format!("Invalid float: {}", f), self.current_line())
                        })?;
                        Ok(DefaultValueAst::Float(-val))
                    }
                    Token::IntLiteral(n) => {
                        self.advance();
                        let val = n.parse::<i128>().map_err(|_| {
                            ParseError::InvalidSyntax(format!("Invalid integer (exceeds i64): {}", n), self.current_line())
                        })?;
                        Ok(DefaultValueAst::Int(-val))
                    }
                    got => Err(ParseError::UnexpectedToken {
                        got,
                        expected: "number after '-'",
                        line: self.current_line()
                    }),
                }
            }
            Token::LParen => {
                self.advance(); // consume '('
                let mut elements = Vec::new();

                loop {
                    elements.push(self.parse_default()?);
                    match self.peek().clone() {
                        Token::Comma => {
                            self.advance();
                        }
                        Token::RParen => {
                            self.advance();
                            break;
                        }
                        got => {
                            return Err(ParseError::UnexpectedToken {
                                got,
                                expected: "',' or ')'",
                                line: self.current_line()
                            })
                        }
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

                        let val = match self.advance() {
                            Token::Keyword(Keyword::True) => true,
                            Token::Keyword(Keyword::False) => false,
                            got => {
                                return Err(ParseError::UnexpectedToken {
                                    got,
                                    expected: "true or false",
                                    line: self.current_line()
                                })
                            }
                        };

                        if !seen.insert(flag_name.clone()) {
                            return Err(ParseError::InvalidSyntax(format!(
                                "duplicate default assignment for flag `{}`",
                                flag_name
                            ), self.current_line()));
                        }

                        kvs.push((flag_name, val));

                        if matches!(self.peek(), Token::Comma) {
                            self.advance();
                        }
                    }
                    self.expect(Token::RParen, "')'")?;
                    Ok(DefaultValueAst::BitsetLiteral {
                        ty: ty.to_string(),
                        kvs,
                    })
                } else {
                    self.expect(Token::ColonColon, "'::'")?;
                    let variant = self.expect_ident()?;
                    Ok(DefaultValueAst::EnumVariant {
                        ty: ty.to_string(),
                        variant,
                    })
                }
            }
            got => Err(ParseError::UnexpectedToken {
                got,
                expected: "default value",
                line: self.current_line()
            }),
        }
    }
}
