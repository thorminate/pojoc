use crate::error::{Error, PojocResult};

/// max bytes a u64 varint can occupy (ceil(64/7))
const MAX_VARINT64_LEN: usize = 10;

/// max bytes a u32 varint can occupy (ceil(32/7))
const MAX_VARINT32_LEN: usize = 5;

#[inline]
pub fn write_varint64(buf: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        buf.push((value as u8) | 0x80);
        value >>= 7;
    }
    buf.push(value as u8);
}

#[cold]
#[inline(never)]
fn read_varint64_slow(first: u8, buf: &[u8], pos: &mut usize) -> PojocResult<u64> {
    let mut result = (first & 0x7F) as u64;
    let mut shift = 7u32;
    for _ in 1..MAX_VARINT64_LEN {
        let byte = *buf.get(*pos).ok_or(Error::UnexpectedEof)?;
        *pos += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
    }
    Err(Error::VarIntOverflow)
}

#[inline]
pub fn read_varint64(buf: &[u8], pos: &mut usize) -> PojocResult<u64> {
    let first = *buf.get(*pos).ok_or(Error::UnexpectedEof)?;
    *pos += 1;
    if first & 0x80 != 0 {
        return read_varint64_slow(first, buf, pos);
    }
    Ok(first as u64)
}

#[inline]
pub fn skip_varint64(buf: &[u8], pos: &mut usize) -> PojocResult<()> {
    for _ in 0..MAX_VARINT64_LEN {
        let byte = *buf.get(*pos).ok_or(Error::UnexpectedEof)?;
        *pos += 1;
        if byte & 0x80 == 0 {
            return Ok(());
        }
    }
    Err(Error::VarIntOverflow)
}

#[inline]
pub fn write_varint32(buf: &mut Vec<u8>, value: u32) {
    write_varint64(buf, value as u64);
}

#[cold]
#[inline(never)]
fn read_varint32_slow(first: u8, buf: &[u8], pos: &mut usize) -> PojocResult<u32> {
    let mut result = (first & 0x7F) as u64;
    let mut shift = 7u32;
    for _ in 1..MAX_VARINT32_LEN {
        let byte = *buf.get(*pos).ok_or(Error::UnexpectedEof)?;
        *pos += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return u32::try_from(result).map_err(|_| Error::VarIntOverflow);
        }
        shift += 7;
    }
    Err(Error::VarIntOverflow)
}

#[inline]
pub fn read_varint32(buf: &[u8], pos: &mut usize) -> PojocResult<u32> {
    let first = *buf.get(*pos).ok_or(Error::UnexpectedEof)?;
    *pos += 1;
    if first & 0x80 != 0 {
        return read_varint32_slow(first, buf, pos);
    }
    Ok(first as u32)
}

#[inline]
pub fn skip_varint32(buf: &[u8], pos: &mut usize) -> PojocResult<()> {
    for _ in 0..MAX_VARINT32_LEN {
        let byte = *buf.get(*pos).ok_or(Error::UnexpectedEof)?;
        *pos += 1;
        if byte & 0x80 == 0 {
            return Ok(());
        }
    }
    Err(Error::VarIntOverflow)
}

#[inline]
pub fn varint_size(n: usize) -> usize {
    match n {
        0..=0x7F => 1,
        0x80..=0x3FFF => 2,
        0x4000..=0x1FFFFF => 3,
        0x200000..=0xFFFFFFF => 4,
        0x10000000..=0x7FFFFFFFF => 5,
        0x800000000..=0x3FFFFFFFFFF => 6,
        0x40000000000..=0x1FFFFFFFFFFFF => 7,
        0x2000000000000..=0xFFFFFFFFFFFFFF => 8,
        0x100000000000000..=0x7FFFFFFFFFFFFFFF => 9,
        _ => 10,
    }
}
