use std::collections::HashSet;
use crate::ast::*;
use crate::lexer::{Keyword, Token};
use crate::error::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }
    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        if !matches!(tok, Token::Eof) { self.pos += 1; }
        tok
    }

    fn expect(&mut self, want: Token, label: &'static str) -> Result<(), ParseError> {
        let got = self.advance();
        if got == want {
            Ok(())
        } else {
            Err(ParseError::UnexpectedToken { got, expected: label, pos: self.pos })
        }
    }

    fn expect_keyword(&mut self, kw: Keyword) -> Result<(), ParseError> {
        let got = self.advance();
        match &got {
            Token::Keyword(k) if *k == kw => Ok(()),
            _ => Err(ParseError::UnexpectedToken { got, expected: "keyword", pos: self.pos }),
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        let got = self.advance();
        match got {
            Token::Ident(s) => Ok(s),
            _ => Err(ParseError::UnexpectedToken { got, expected: "identifier", pos: self.pos }),
        }
    }

    fn expect_number(&mut self) -> Result<u32, ParseError> {
        let got = self.advance();
        match got {
            Token::Number(n) => Ok(n),
            _ => Err(ParseError::UnexpectedToken { got, expected: "number", pos: self.pos }),
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
                )));
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

        let mut seen_sections = HashSet::new();
        let mut seen_types = HashSet::new();
        let mut blocks = Vec::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let block = self.parse_version_block()?;

            // track SECTION uniqueness (type/fields/diff)
            let section_kind = match &block {
                VersionBlockAst::TypeDef(_) => "type",
                VersionBlockAst::Fields(_) => "fields",
                VersionBlockAst::Diff(_) => "diff",
            };

            if !seen_sections.insert(section_kind) {
                return Err(ParseError::InvalidSyntax(format!(
                    "version {} contains duplicate `{}` section",
                    version,
                    section_kind
                )));
            }

            // track type uniqueness inside type section only
            if let VersionBlockAst::TypeDef(ref t) = block {
                if !seen_types.insert(t.name.clone()) {
                    return Err(ParseError::InvalidSyntax(format!(
                        "version {} contains duplicated type definitions: {}",
                        version,
                        t.name
                    )));
                }
            }

            blocks.push(block);
        }

        self.expect(Token::RBrace, "'}'")?;
        Ok(VersionAst { version, blocks })
    }

    fn parse_version_block(&mut self) -> Result<VersionBlockAst, ParseError> {
        match self.peek().clone() {
            Token::Keyword(Keyword::Type)   => Ok(VersionBlockAst::TypeDef(self.parse_type_def()?)),
            Token::Keyword(Keyword::Fields) => Ok(VersionBlockAst::Fields(self.parse_fields_block()?)),
            Token::Keyword(Keyword::Diff)   => Ok(VersionBlockAst::Diff(self.parse_diff_block()?)),
            got => Err(ParseError::UnexpectedToken {
                got,
                expected: "type / fields / diff",
                pos: self.pos,
            }),
        }
    }
    fn parse_type_def(&mut self) -> Result<TypeDefAst, ParseError> {
        self.expect_keyword(Keyword::Type)?;
        let name = self.expect_ident()?;

        // optional: extends ParentName
        let extends = if matches!(self.peek(), Token::Keyword(Keyword::Extends)) {
            self.advance();
            Some(self.expect_ident()?)
        } else {
            None
        };

        self.expect(Token::LBrace, "'{'")?;

        let body = if extends.is_some() {
            // body must be diff ops
            TypeBody::Diff(self.parse_diff_ops()?)
        } else {
            // body is plain fields
            TypeBody::Fields(self.parse_field_list()?)
        };

        self.expect(Token::RBrace, "'}'")?;

        Ok(TypeDefAst { name, extends, body })
    }

    fn parse_fields_block(&mut self) -> Result<FieldsAst, ParseError> {
        self.expect_keyword(Keyword::Fields)?;
        self.expect(Token::LBrace, "'{'")?;
        let fields = self.parse_field_list()?;
        self.expect(Token::RBrace, "'}'")?;
        Ok(FieldsAst { fields })
    }

    fn parse_field_list(&mut self) -> Result<Vec<FieldAst>, ParseError> {
        let mut fields = Vec::new();
        let mut seen_names = HashSet::new();

        while matches!(self.peek(), Token::Ident(_)) {
            let name = self.expect_ident()?;

            if !seen_names.insert(name.clone()) {
                return Err(ParseError::InvalidSyntax(format!(
                    "duplicate field name: {}", name
                )));
            }

            self.expect(Token::Colon, "':'")?;
            let ty = self.parse_type()?;

            let default = if matches!(self.peek(), Token::Equals) {
                self.advance();
                Some(self.parse_default()?)
            } else {
                None
            };

            fields.push(FieldAst { name, ty, default });
        }

        Ok(fields)
    }

    fn parse_type(&mut self) -> Result<TypeAst, ParseError> {
        match self.peek().clone() {
            Token::LBracket => {
                self.advance();
                let inner = self.parse_type()?;
                self.expect(Token::RBracket, "']'")?;
                Ok(TypeAst::Array(Box::new(inner)))
            }
            Token::Ident(_) => {
                let name = self.expect_ident()?;
                Ok(TypeAst::Named(name))
            }
            got => Err(ParseError::UnexpectedToken {
                got,
                expected: "type name or '['",
                pos: self.pos,
            }),
        }
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
                        _ => unreachable!(),
                    };

                    if !seen.insert(name.clone()) {
                        return Err(ParseError::InvalidSyntax(format!(
                            "field `{}` has multiple diff operations",
                            name
                        )));
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
                        )));
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
                        )));
                    }

                    ops.push(op);
                }
                got => {
                    return Err(ParseError::UnexpectedToken {
                        got,
                        expected: "+ / - / ~",
                        pos: self.pos,
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
        let ty = self.parse_type()?;

        let default = if matches!(self.peek(), Token::Equals) {
            self.advance();
            Some(self.parse_default()?)
        } else {
            None
        };

        Ok(DiffAst::Add { field: FieldAst { name, ty, default } })
    }

    // - name
    fn parse_diff_remove(&mut self) -> Result<DiffAst, ParseError> {
        self.advance(); // consume '-'
        let name = self.expect_ident()?;
        Ok(DiffAst::Remove { name })
    }

    // ~ id -> player_id ← rename
    // ~ level: float ← type update
    // ~ existed -> age: float ← transform (rename and type update)
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

            (Some(to), None) => Ok(DiffAst::Rename {
                from,
                to,
            }),

            (None, Some(ty)) => Ok(DiffAst::UpdateType {
                name: from,
                ty,
            }),

            (None, None) => Err(ParseError::UnexpectedToken {
                got: self.peek().clone(),
                expected: "->, : or combination",
                pos: self.pos,
            }),
        }
    }

    fn parse_default(&mut self) -> Result<DefaultValueAst, ParseError> {
        match self.peek().clone() {
            Token::Keyword(Keyword::True)  => { self.advance(); Ok(DefaultValueAst::Bool(true)) }
            Token::Keyword(Keyword::False) => { self.advance(); Ok(DefaultValueAst::Bool(false)) }
            Token::StringLit(s)            => { self.advance(); Ok(DefaultValueAst::Str(s)) }
            Token::Float(f)                => { self.advance(); Ok(DefaultValueAst::Float(f)) }
            Token::Number(n)               => { self.advance(); Ok(DefaultValueAst::Int(n as i64)) }
            Token::LBracket                => {
                self.advance();
                self.expect(Token::RBracket, "']'")?;
                Ok(DefaultValueAst::EmptyArray)
            }
            // negative numbers
            Token::Minus => {
                self.advance();
                match self.peek().clone() {
                    Token::Float(f)  => { self.advance(); Ok(DefaultValueAst::Float(-f)) }
                    Token::Number(n) => { self.advance(); Ok(DefaultValueAst::Int(-(n as i64))) }
                    got => Err(ParseError::UnexpectedToken {
                        got,
                        expected: "number after '-'",
                        pos: self.pos,
                    }),
                }
            }
            got => Err(ParseError::UnexpectedToken {
                got,
                expected: "default value",
                pos: self.pos,
            }),
        }
    }
}