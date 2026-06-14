use compact_str::CompactString;
use crate::error::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Keyword {
    Schema,
    Version,
    Type,
    Enum,
    Bitset,
    Fields,
    Diff,
    Extends,
    Const,
    Delta,
    True,
    False,
}

impl std::fmt::Display for Keyword {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Keyword::Schema => write!(f, "schema"),
            Keyword::Version => write!(f, "version"),
            Keyword::Type => write!(f, "type"),
            Keyword::Enum => write!(f, "enum"),
            Keyword::Bitset => write!(f, "bitset"),
            Keyword::Fields => write!(f, "fields"),
            Keyword::Diff => write!(f, "diff"),
            Keyword::Extends => write!(f, "extends"),
            Keyword::Const => write!(f, "const"),
            Keyword::Delta => write!(f, "delta"),
            Keyword::True => write!(f, "true"),
            Keyword::False => write!(f, "false"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    IntLiteral(CompactString),
    FloatLiteral(CompactString),
    Identifier(CompactString),
    StringLiteral(CompactString),
    Keyword(Keyword),
    Equals,       // =
    LBrace,       // {
    RBrace,       // }
    LBracket,     // [
    RBracket,     // ]
    LParen,       // (
    RParen,       // )
    LAngle,       // <
    RAngle,       // >
    Colon,        // :
    ColonColon,   // ::
    Arrow,        // ->
    Plus,         // +
    Minus,        // -
    Tilde,        // ~
    Comma,        // ,
    At,           // @
    QuestionMark, // ?
    Eof,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Token::IntLiteral(s) => write!(f, "{}", s),
            Token::FloatLiteral(s) => write!(f, "{}", s),
            Token::Identifier(s) => write!(f, "{}", s),
            Token::StringLiteral(s) => write!(f, "{}", s),
            Token::Keyword(k) => write!(f, "{}", k),
            Token::Equals => write!(f, "="),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LAngle => write!(f, "<"),
            Token::RAngle => write!(f, ">"),
            Token::Colon => write!(f, ":"),
            Token::ColonColon => write!(f, "::"),
            Token::Arrow => write!(f, "->"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Tilde => write!(f, "~"),
            Token::Comma => write!(f, ","),
            Token::At => write!(f, "@"),
            Token::QuestionMark => write!(f, "?"),
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
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            let mut skipped_space = false;
            while let Some(c) = self.peek() {
                if c == '\n' {
                    self.line += 1;
                    self.advance();
                    skipped_space = true;
                } else if c.is_whitespace() {
                    self.advance();
                    skipped_space = true;
                } else {
                    break;
                }
            }

            if self.peek() == Some('/') && self.peek_next() == Some('/') {
                self.advance();
                self.advance();

                while !matches!(self.peek(), Some('\n') | None) {
                    self.advance();
                }

                if self.peek() == Some('\n') {
                    self.line += 1;
                    self.advance();
                }
            } else if !skipped_space {
                break;
            }
        }
    }

    fn read_ident_or_keyword(&mut self) -> Token {
        let mut s = CompactString::new("");
        while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_') {
            s.push(self.advance().unwrap());
        }
        match s.as_str() {
            "schema" => Token::Keyword(Keyword::Schema),
            "version" => Token::Keyword(Keyword::Version),
            "type" => Token::Keyword(Keyword::Type),
            "enum" => Token::Keyword(Keyword::Enum),
            "bitset" => Token::Keyword(Keyword::Bitset),
            "fields" => Token::Keyword(Keyword::Fields),
            "diff" => Token::Keyword(Keyword::Diff),
            "extends" => Token::Keyword(Keyword::Extends),
            "const" => Token::Keyword(Keyword::Const),
            "delta" => Token::Keyword(Keyword::Delta),
            "true" => Token::Keyword(Keyword::True),
            "false" => Token::Keyword(Keyword::False),
            _ => Token::Identifier(s),
        }
    }

    fn read_number(&mut self) -> Token {
        let mut s = compact_str::CompactString::new("");
        let mut is_float = false;

        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            s.push(self.advance().unwrap());
        }

        if self.peek() == Some('.') {
            is_float = true;
            s.push(self.advance().unwrap());
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                s.push(self.advance().unwrap());
            }
        }

        if matches!(self.peek(), Some('e') | Some('E')) {
            is_float = true;
            s.push(self.advance().unwrap()); // consume 'e' or 'E'

            // Check for optional sign directly following 'e'/'E'
            if matches!(self.peek(), Some('+') | Some('-')) {
                s.push(self.advance().unwrap());
            }

            // Consume the exponent digits
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                s.push(self.advance().unwrap());
            }
        }

        if is_float {
            Token::FloatLiteral(s)
        } else {
            Token::IntLiteral(s)
        }
    }

    fn read_string_lit(&mut self) -> Result<Token, LexError> {
        self.advance(); // consume opening "
        let mut s = CompactString::new("");
        loop {
            match self.advance() {
                Some('"') => break,
                Some(c) => s.push(c),
                None => return Err(LexError::UnexpectedChar('"', self.line)),
            }
        }
        Ok(Token::StringLiteral(s))
    }

    pub fn tokenize(&mut self) -> Result<Vec<(Token, usize)>, LexError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            match self.peek() {
                None => {
                    tokens.push((Token::Eof, self.line));
                    break;
                }
                Some(c) => {
                    let tok = match c {
                        '{' => {
                            self.advance();
                            Token::LBrace
                        }
                        '}' => {
                            self.advance();
                            Token::RBrace
                        }
                        '[' => {
                            self.advance();
                            Token::LBracket
                        }
                        ']' => {
                            self.advance();
                            Token::RBracket
                        }
                        '(' => {
                            self.advance();
                            Token::LParen
                        }
                        ')' => {
                            self.advance();
                            Token::RParen
                        }
                        '<' => {
                            self.advance();
                            Token::LAngle
                        }
                        '>' => {
                            self.advance();
                            Token::RAngle
                        }
                        ':' => {
                            self.advance();
                            if self.peek() == Some(':') {
                                self.advance();
                                Token::ColonColon
                            } else {
                                Token::Colon
                            }
                        }
                        '+' => {
                            self.advance();
                            Token::Plus
                        }
                        '~' => {
                            self.advance();
                            Token::Tilde
                        }
                        '=' => {
                            self.advance();
                            Token::Equals
                        }
                        '"' => self.read_string_lit()?,
                        ',' => {
                            self.advance();
                            Token::Comma
                        }
                        '-' => {
                            self.advance();
                            if self.peek() == Some('>') {
                                self.advance();
                                Token::Arrow
                            } else {
                                Token::Minus
                            }
                        }
                        '@' => {
                            self.advance();
                            Token::At
                        }
                        '?' => {
                            self.advance();
                            Token::QuestionMark
                        }
                        c if c.is_alphabetic() || c == '_' => self.read_ident_or_keyword(),
                        c if c.is_ascii_digit() => self.read_number(),
                        c => return Err(LexError::UnexpectedChar(c, self.line)),
                    };
                    tokens.push((tok, self.line));
                }
            }
        }
        Ok(tokens)
    }
}
