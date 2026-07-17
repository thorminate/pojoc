use crate::schema::error::*;
use crate::schema::span::Span;
use compact_str::CompactString;

#[derive(Debug, Clone, PartialEq)]
pub enum Keyword {
    Schema,
    Version,
    Type,
    Enum,
    Union,
    Bitset,
    Fields,
    Diff,
    Extends,
    Const,
    Delta,
    Lazy,
    True,
    False,
    Import,
    As,
}

impl std::fmt::Display for Keyword {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Keyword::Schema => write!(f, "schema"),
            Keyword::Version => write!(f, "version"),
            Keyword::Type => write!(f, "type"),
            Keyword::Enum => write!(f, "enum"),
            Keyword::Union => write!(f, "union"),
            Keyword::Bitset => write!(f, "bitset"),
            Keyword::Fields => write!(f, "fields"),
            Keyword::Diff => write!(f, "diff"),
            Keyword::Extends => write!(f, "extends"),
            Keyword::Const => write!(f, "const"),
            Keyword::Delta => write!(f, "delta"),
            Keyword::Lazy => write!(f, "lazy"),
            Keyword::True => write!(f, "true"),
            Keyword::False => write!(f, "false"),
            Keyword::Import => write!(f, "import"),
            Keyword::As => write!(f, "as"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    IntLiteral(CompactString),
    FloatLiteral(CompactString),
    Identifier(CompactString),
    StringLiteral(CompactString),
    /// A `/// text` doc comment line — one token per line, text already
    /// stripped of the leading `///` and (if present) one following space.
    /// Plain `//` comments (and `////`+) are not doc comments and never
    /// produce a token; they're discarded like whitespace.
    DocComment(CompactString),
    Keyword(Keyword),
    Equals,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    LAngle,
    RAngle,
    Colon,
    ColonColon,
    Arrow,
    Plus,
    Minus,
    Tilde,
    Comma,
    At,
    QuestionMark,
    DotDot,
    Eof,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Token::IntLiteral(s) => write!(f, "{}", s),
            Token::FloatLiteral(s) => write!(f, "{}", s),
            Token::Identifier(s) => write!(f, "{}", s),
            Token::StringLiteral(s) => write!(f, "{}", s),
            Token::DocComment(s) => write!(f, "///{}", s),
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
            Token::DotDot => write!(f, ".."),
            Token::Eof => write!(f, "EOF"),
        }
    }
}

/// A `Token` plus its precise byte-offset `Span` and the 1-indexed source
/// line it starts on. `line` is tracked by the lexer's existing newline
/// counter as it scans — not derived from `span` — so `Display`-facing
/// error messages don't need a `LineIndex` or the source text on hand.
#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
    pub line: u32,
}

pub struct Lexer {
    input: Vec<char>,
    pos: usize,      // char index into `input`
    byte_pos: usize, // byte offset into the original source string
    pub line: u32,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            byte_pos: 0,
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
        if let Some(c) = ch {
            self.pos += 1;
            self.byte_pos += c.len_utf8(); // chars can be multi-byte in UTF-8
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

            if self.at_doc_comment() {
                // Leave it for `tokenize()` to read as a real `DocComment` token.
                break;
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

    /// True at a `///` doc comment — exactly three slashes. A fourth (as in
    /// `////`) makes it a plain, non-doc comment, matching rustdoc's own rule.
    fn at_doc_comment(&self) -> bool {
        self.input.get(self.pos) == Some(&'/')
            && self.input.get(self.pos + 1) == Some(&'/')
            && self.input.get(self.pos + 2) == Some(&'/')
            && self.input.get(self.pos + 3) != Some(&'/')
    }

    /// Reads a `/// text` line (the lexer must already be positioned at the
    /// leading `/`), stripping the `///` and one following space if present.
    fn read_doc_comment(&mut self) -> Token {
        self.advance();
        self.advance();
        self.advance();
        if self.peek() == Some(' ') {
            self.advance();
        }
        let mut s = CompactString::new("");
        while !matches!(self.peek(), Some('\n') | None) {
            s.push(self.advance().unwrap());
        }
        Token::DocComment(s)
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
            "union" => Token::Keyword(Keyword::Union),
            "bitset" => Token::Keyword(Keyword::Bitset),
            "fields" => Token::Keyword(Keyword::Fields),
            "diff" => Token::Keyword(Keyword::Diff),
            "extends" => Token::Keyword(Keyword::Extends),
            "const" => Token::Keyword(Keyword::Const),
            "delta" => Token::Keyword(Keyword::Delta),
            "lazy" => Token::Keyword(Keyword::Lazy),
            "true" => Token::Keyword(Keyword::True),
            "false" => Token::Keyword(Keyword::False),
            "import" => Token::Keyword(Keyword::Import),
            "as" => Token::Keyword(Keyword::As),
            _ => Token::Identifier(s),
        }
    }

    fn read_number(&mut self) -> Token {
        let mut s = CompactString::new("");
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
            s.push(self.advance().unwrap());

            if matches!(self.peek(), Some('+') | Some('-')) {
                s.push(self.advance().unwrap());
            }

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
        let start_byte = self.byte_pos;
        let start_line = self.line;
        self.advance(); // consume opening "
        let mut s = CompactString::new("");
        loop {
            match self.advance() {
                Some('"') => break,
                Some(c) => s.push(c),
                None => {
                    return Err(LexError::UnexpectedChar {
                        ch: '"',
                        span: Span::new(start_byte, self.byte_pos),
                        line: start_line,
                    });
                }
            }
        }
        Ok(Token::StringLiteral(s))
    }

    pub fn tokenize(&mut self) -> Result<Vec<SpannedToken>, LexError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            let start_byte = self.byte_pos;
            let start_line = self.line;

            match self.peek() {
                None => {
                    tokens.push(SpannedToken {
                        token: Token::Eof,
                        span: Span::new(start_byte, start_byte),
                        line: start_line,
                    });
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
                        '.' => {
                            self.advance();
                            if self.peek() == Some('.') {
                                self.advance();
                                Token::DotDot
                            } else {
                                return Err(LexError::UnexpectedChar {
                                    ch: '.',
                                    span: Span::new(start_byte, self.byte_pos),
                                    line: start_line,
                                });
                            }
                        }
                        '/' => self.read_doc_comment(),
                        c if c.is_alphabetic() || c == '_' => self.read_ident_or_keyword(),
                        c if c.is_ascii_digit() => self.read_number(),
                        c => {
                            return Err(LexError::UnexpectedChar {
                                ch: c,
                                span: Span::new(start_byte, start_byte + c.len_utf8()),
                                line: start_line,
                            });
                        }
                    };

                    tokens.push(SpannedToken {
                        token: tok,
                        span: Span::new(start_byte, self.byte_pos),
                        line: start_line,
                    });
                }
            }
        }
        Ok(tokens)
    }
}
