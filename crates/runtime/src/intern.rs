use crate::decode::{read_array_len, read_string, skip_string};
use crate::encode::{write_envelope_header_with_hint, write_string};
use crate::error::{Error, PojocResult};
use crate::varint::{read_varint64, varint_size, write_varint64};
use smallvec::SmallVec;
use std::collections::HashMap;

const INLINE_ENTRIES: usize = 2;

/// Builds a message's string table, assigning indices in first-occurrence order.
/// Larger tables use [`crate::PojocHasher`] to resist hash flooding.
pub struct InternBuilder<'a> {
    inline: [&'a str; INLINE_ENTRIES],
    inline_len: usize,
    order: Vec<&'a str>,
    index: HashMap<&'a str, u32, crate::PojocHasher>,
}

impl<'a> InternBuilder<'a> {
    #[inline]
    pub fn new() -> Self {
        Self {
            inline: [""; INLINE_ENTRIES],
            inline_len: 0,
            order: Vec::new(),
            index: HashMap::default(),
        }
    }

    /// Returns the existing index or assigns one on the first occurrence.
    #[inline]
    pub fn intern(&mut self, s: &'a str) -> u32 {
        // An empty hash table owns no buckets. Try the usual lookup first so
        // repeated entries in a promoted table take the same hot path.
        if let Some(&idx) = self.index.get(s) {
            return idx;
        }
        // The order vector becomes nonempty once the inline entries spill.
        if self.order.is_empty() {
            if let Some(idx) = self.inline[..self.inline_len]
                .iter()
                .position(|&entry| entry == s)
            {
                return idx as u32;
            }
            if self.inline_len < INLINE_ENTRIES {
                let idx = self.inline_len;
                self.inline[idx] = s;
                self.inline_len += 1;
                return idx as u32;
            }
            self.promote();
        }
        let idx = self.order.len() as u32;
        self.order.push(s);
        self.index.insert(s, idx);
        idx
    }

    #[cold]
    fn promote(&mut self) {
        self.order.reserve(INLINE_ENTRIES * 2);
        self.order.extend(self.inline);
        self.index.reserve(INLINE_ENTRIES * 2);
        self.index.extend(
            self.inline
                .into_iter()
                .enumerate()
                .map(|(i, entry)| (entry, i as u32)),
        );
    }

    /// Consume the builder and keep a small table inline for writing.
    #[doc(hidden)]
    #[inline]
    pub fn finish_table(self) -> SmallVec<[&'a str; INLINE_ENTRIES]> {
        if self.order.is_empty() {
            SmallVec::from_slice(&self.inline[..self.inline_len])
        } else {
            SmallVec::from_vec(self.order)
        }
    }

    pub fn finish(self) -> Vec<&'a str> {
        self.finish_table().into_vec()
    }
}

impl Default for InternBuilder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// writes the table: [count:varint] [(len:varint, bytes)] * count
pub fn write_intern_table(buf: &mut Vec<u8>, table: &[&str]) {
    write_varint64(buf, table.len() as u64);
    for s in table {
        write_string(buf, s);
    }
}

/// Write staged interned fields and their table, with an envelope when a
/// preencoded version is supplied. A zero-width version means payload only.
#[doc(hidden)]
pub fn write_interned_payload<const VERSION_LEN: usize>(
    buf: &mut Vec<u8>,
    version: &[u8; VERSION_LEN],
    table: &[&str],
    fields: &[u8],
) {
    if VERSION_LEN != 0 {
        let payload_len = table.iter().fold(
            fields.len().saturating_add(varint_size(table.len())),
            |size, s| {
                size.saturating_add(varint_size(s.len()))
                    .saturating_add(s.len())
            },
        );
        let length = u32::try_from(payload_len).expect("envelope payload too large for u32 length");
        write_envelope_header_with_hint(buf, version, length as usize);
    }
    write_intern_table(buf, table);
    buf.extend_from_slice(fields);
}

/// reads the table written by [`write_intern_table`]
pub fn read_intern_table<'a>(buf: &'a [u8], pos: &mut usize) -> PojocResult<Vec<&'a str>> {
    let count = read_array_len(buf, pos)?;
    // Every table entry has at least a one-byte string length prefix.
    if count > buf.len().saturating_sub(*pos) {
        return Err(Error::InvalidLength);
    }
    let mut table = Vec::with_capacity(count);
    for _ in 0..count {
        table.push(read_string(buf, pos)?);
    }
    Ok(table)
}

/// advances past a table written by [`write_intern_table`] without materializing
/// it, for readers that don't resolve any interned string
pub fn skip_intern_table(buf: &[u8], pos: &mut usize) -> PojocResult<()> {
    let count = read_array_len(buf, pos)?;
    for _ in 0..count {
        skip_string(buf, pos)?;
    }
    Ok(())
}

/// resolves a single index against the table starting at `table_start`, walking
/// to that entry rather than materializing every one. `table_start` addresses the
/// table's count prefix, so this is independent of the caller's cursor.
pub fn read_interned_string_at(buf: &[u8], table_start: usize, idx: usize) -> PojocResult<&str> {
    let mut pos = table_start;
    let count = read_array_len(buf, &mut pos)?;
    if idx >= count {
        return Err(Error::InvalidInternIndex);
    }
    for _ in 0..idx {
        skip_string(buf, &mut pos)?;
    }
    read_string(buf, &mut pos)
}

/// reads a varint index into table, written by an intern-marked field
pub fn read_interned_string_ref<'a>(
    table: &[&'a str],
    buf: &[u8],
    pos: &mut usize,
) -> PojocResult<&'a str> {
    let idx = usize::try_from(read_varint64(buf, pos)?).map_err(|_| Error::InvalidInternIndex)?;
    table.get(idx).copied().ok_or(Error::InvalidInternIndex)
}
