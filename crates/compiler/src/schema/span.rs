/// byte-offset range into the source string, used for LSP diagnostics/hover/goto
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// smallest span covering both, used to build a parent node's span from its children
    pub fn join(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// LSP-style (line, character) position, character is a UTF-16 code unit offset
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// precomputed line-start byte offsets. the lexer/parser track `line` directly
/// instead, this is only for paths that have a bare offset and no line attached
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

    /// converts a byte offset into a 0-indexed (line, character) position
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
