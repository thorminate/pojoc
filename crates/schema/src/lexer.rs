use crate::error::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Keyword {
    Schema,
    Version,
    Type,
    Fields,
    Diff,
    Extends,
    True,
    False,
}

impl std::fmt::Display for Keyword {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self { 
            Keyword::Schema => write!(f, "schema"),
            Keyword::Version => write!(f, "version"),
            Keyword::Type => write!(f, "type"),
            Keyword::Fields => write!(f, "fields"),
            Keyword::Diff => write!(f, "diff"),
            Keyword::Extends => write!(f, "extends"),
            Keyword::True => write!(f, "true"),
            Keyword::False => write!(f, "false"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Number(u32),
    Equals,
    Float(f64),
    StringLit(String),
    Keyword(Keyword),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Arrow,   // ->
    Plus,
    Minus,
    Tilde,
    Comma,
    Eof,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self { 
            Token::Ident(s) => write!(f, "{}", s),
            Token::Number(n) => write!(f, "{}", n),
            Token::Equals => write!(f, "="),
            Token::Float(n) => write!(f, "{}", n),
            Token::StringLit(s) => write!(f, "\"{}\"", s),
            Token::Keyword(k) => write!(f, "{}", k),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::Colon => write!(f, ":"),
            Token::Arrow => write!(f, "->"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Tilde => write!(f, "~"),
            Token::Comma => write!(f, ","),
            Token::Eof => write!(f, "EOF"),
        }
    }
}

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    pub line: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer { input: input.chars().collect(), pos: 0, line: 1 }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied();
        if ch == Some('\n') { self.line += 1; }
        self.pos += 1;
        ch
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // skip whitespace
            while matches!(self.peek(), Some(c) if c.is_whitespace()) {
                self.advance();
            }
            // skip // line comments
            if self.peek() == Some('/') && self.peek_next() == Some('/') {
                while !matches!(self.peek(), Some('\n') | None) {
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    fn read_ident_or_keyword(&mut self) -> Token {
        let mut s = String::new();
        while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_') {
            s.push(self.advance().unwrap());
        }
        match s.as_str() {
            "schema"  => Token::Keyword(Keyword::Schema),
            "version" => Token::Keyword(Keyword::Version),
            "type"    => Token::Keyword(Keyword::Type),
            "fields"  => Token::Keyword(Keyword::Fields),
            "diff"    => Token::Keyword(Keyword::Diff),
            "extends" => Token::Keyword(Keyword::Extends),
            "true"    => Token::Keyword(Keyword::True),
            "false"   => Token::Keyword(Keyword::False),
            _         => Token::Ident(s),
        }
    }

    fn read_number(&mut self) -> Token {
        let mut s = String::new();
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            s.push(self.advance().unwrap());
        }
        if self.peek() == Some('.') {
            s.push(self.advance().unwrap());
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                s.push(self.advance().unwrap());
            }
            Token::Float(s.parse().unwrap())
        } else {
            Token::Number(s.parse().unwrap())
        }
    }

    fn read_string_lit(&mut self) -> Result<Token, LexError> {
        self.advance(); // consume opening "
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') => break,
                Some(c)   => s.push(c),
                None      => return Err(LexError::UnexpectedChar('"', self.line)),
            }
        }
        Ok(Token::StringLit(s))
    }
    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            match self.peek() {
                None => { tokens.push(Token::Eof); break; }
                Some(c) => {
                    let tok = match c {
                        '{'  => { self.advance(); Token::LBrace }
                        '}'  => { self.advance(); Token::RBrace }
                        '['  => { self.advance(); Token::LBracket }
                        ']'  => { self.advance(); Token::RBracket }
                        ':'  => { self.advance(); Token::Colon }
                        '+'  => { self.advance(); Token::Plus }
                        '~'  => { self.advance(); Token::Tilde }
                        '=' => { self.advance(); Token::Equals }
                        '"' => self.read_string_lit()?,
                        ','  => { self.advance(); Token::Comma }
                        '-'  => {
                            self.advance();
                            // disambiguate: -> vs bare -
                            if self.peek() == Some('>') {
                                self.advance();
                                Token::Arrow
                            } else {
                                Token::Minus
                            }
                        }
                        c if c.is_alphabetic() || c == '_' => self.read_ident_or_keyword(),
                        c if c.is_ascii_digit()             => self.read_number(),
                        c => return Err(LexError::UnexpectedChar(c, self.line)),
                    };
                    tokens.push(tok);
                }
            }
        }
        Ok(tokens)
    }
}