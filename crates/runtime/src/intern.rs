use crate::decode::read_string;
use crate::encode::write_string;
use crate::error::{Error, PojocResult};
use crate::varint::{read_varint64, write_varint64};
use std::collections::HashMap;

/// Builds a message's shared string-interning table during encode: strings
/// are deduped by first-occurrence order as `intern`-marked fields are
/// visited, and referenced elsewhere by their assigned index.
pub struct InternBuilder<'a> {
    order: Vec<&'a str>,
    index: HashMap<&'a str, u32>,
}

impl<'a> InternBuilder<'a> {
    pub fn new() -> Self {
        Self {
            order: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Returns `s`'s index in the table, assigning it a new one (appended to
    /// the table) the first time this exact string is seen.
    pub fn intern(&mut self, s: &'a str) -> u32 {
        if let Some(&idx) = self.index.get(s) {
            return idx;
        }
        let idx = self.order.len() as u32;
        self.order.push(s);
        self.index.insert(s, idx);
        idx
    }

    pub fn finish(self) -> Vec<&'a str> {
        self.order
    }
}

impl Default for InternBuilder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes the table: `[count:varint] [(len:varint, bytes)] * count`.
pub fn write_intern_table(buf: &mut Vec<u8>, table: &[&str]) {
    write_varint64(buf, table.len() as u64);
    for s in table {
        write_string(buf, s);
    }
}

/// Reads the table written by [`write_intern_table`].
pub fn read_intern_table<'a>(buf: &'a [u8], pos: &mut usize) -> PojocResult<Vec<&'a str>> {
    let count = read_varint64(buf, pos)? as usize;
    let mut table = Vec::with_capacity(count);
    for _ in 0..count {
        table.push(read_string(buf, pos)?);
    }
    Ok(table)
}

/// Reads a varint index into `table`, written by an `intern`-marked field.
pub fn read_interned_string_ref<'a>(
    table: &[&'a str],
    buf: &[u8],
    pos: &mut usize,
) -> PojocResult<&'a str> {
    let idx = read_varint64(buf, pos)? as usize;
    table.get(idx).copied().ok_or(Error::InvalidInternIndex)
}
