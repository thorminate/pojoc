/// A byte-offset range into the original source string. This is the
/// canonical, precise location used for LSP diagnostics, hover ranges,
/// go-to-definition, and exact source slicing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// Smallest span covering both `self` and `other`. Used to build a
    /// parent AST node's span from its first and last child spans.
    pub fn join(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// LSP-style (line, character) position. `character` is a UTF-16 code unit
/// offset from the start of the line, per the LSP spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// Precomputed line-start byte offsets for a source string.
///
/// This is *not* used by the lexer/parser — they track `line` directly as
/// they scan, which is inexpensive and avoids needing the source text in scope
/// just to print "at line N". `LineIndex` is only for paths that have a
/// bare byte offset and no `line` already attached (e.g., converting a
/// `Span` from a `Diagnostic` into an LSP `Position` after the fact, or any
/// future consumer that only has an offset).
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        LineIndex { line_starts }
    }

    /// Converts a byte offset into a 0-indexed (line, character) position.
    pub fn position(&self, source: &str, offset: usize) -> Position {
        let line_idx = self
            .line_starts
            .binary_search(&offset)
            .unwrap_or_else(|insert_at| insert_at - 1);
        let line_start = self.line_starts[line_idx];
        let clamped_end = offset.min(source.len());

        let character = source[line_start..clamped_end].encode_utf16().count();

        Position {
            line: line_idx as u32,
            character: character as u32,
        }
    }
}
