use pojoc_core::Type;
use crate::ast::*;
use crate::lexer::{Keyword, Token};

#[derive(Debug)]
pub enum ParseError {
    UnexpectedToken { got: Token, expected: &'static str, pos: usize },
    UnexpectedEof,
    InvalidSyntax(String),
}

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
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            versions.push(self.parse_version()?);
        }

        self.expect(Token::RBrace, "'}'")?;
        Ok(SchemaAst { name, versions })
    }

    fn parse_version(&mut self) -> Result<VersionAst, ParseError> {
        self.expect_keyword(Keyword::Version)?;
        let number = self.expect_number()?;
        self.expect(Token::LBrace, "'{'")?;

        let mut blocks = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            blocks.push(self.parse_version_block()?);
        }

        self.expect(Token::RBrace, "'}'")?;
        Ok(VersionAst { number, blocks })
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
        self.expect(Token::LBrace, "'{'")?;
        let fields = self.parse_field_list()?;
        self.expect(Token::RBrace, "'}'")?;
        Ok(TypeDefAst { name, fields })
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
        while matches!(self.peek(), Token::Ident(_)) {
            let name = self.expect_ident()?;
            self.expect(Token::Colon, "':'")?;
            let ty = self.parse_type()?;
            fields.push(FieldAst { name, ty });
        }
        Ok(fields)
    }

    fn parse_type(&mut self) -> Result<Type, ParseError> {
        match self.peek().clone() {
            Token::LBracket => {
                self.advance();
                let inner = self.parse_type()?;
                self.expect(Token::RBracket, "']'")?;
                Ok(Type::Array(Box::new(inner)))
            }
            Token::Ident(_) => {
                let name = self.expect_ident()?;
                Ok(Type::Named(name))
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

        let mut ops = Vec::new();
        loop {
            match self.peek().clone() {
                Token::RBrace | Token::Eof => break,
                Token::Plus  => ops.push(self.parse_diff_add()?),
                Token::Minus => ops.push(self.parse_diff_remove()?),
                Token::Tilde => ops.push(self.parse_diff_tilde()?),
                got => return Err(ParseError::UnexpectedToken {
                    got,
                    expected: "+ / - / ~",
                    pos: self.pos,
                }),
            }
        }

        self.expect(Token::RBrace, "'}'")?;
        Ok(ops)
    }

    // + position: Vector3
    fn parse_diff_add(&mut self) -> Result<DiffAst, ParseError> {
        self.advance(); // consume '+'
        let name = self.expect_ident()?;
        self.expect(Token::Colon, "':'")?;
        let ty = self.parse_type()?;
        Ok(DiffAst::Add { field: FieldAst { name, ty } })
    }

    // - name
    fn parse_diff_remove(&mut self) -> Result<DiffAst, ParseError> {
        self.advance(); // consume '-'
        let name = self.expect_ident()?;
        Ok(DiffAst::Remove { name })
    }

    // ~ id -> player_id   ← rename
    // ~ level: float      ← type update
    fn parse_diff_tilde(&mut self) -> Result<DiffAst, ParseError> {
        self.advance(); // consume '~'
        let name = self.expect_ident()?;

        match self.peek().clone() {
            Token::Arrow => {
                self.advance(); // consume '->'
                let to = self.expect_ident()?;
                Ok(DiffAst::Rename { from: name, to })
            }
            Token::Colon => {
                self.advance(); // consume ':'
                let ty = self.parse_type()?;
                Ok(DiffAst::UpdateType { name, ty })
            }
            got => Err(ParseError::UnexpectedToken {
                got,
                expected: "'->' for rename or ':' for type update",
                pos: self.pos,
            }),
        }
    }
}