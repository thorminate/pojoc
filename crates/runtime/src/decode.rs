use crate::{Error, PojocResult, PojocString, read_varint64};

/// Read a single `u8`.
pub fn read_u8(buf: &[u8], pos: &mut usize) -> PojocResult<u8> {
    let byte = *buf.get(*pos).ok_or(Error::UnexpectedEof)?;
    *pos += 1;
    Ok(byte)
}

/// Read a `bool` (0 = false, anything else = true).
pub fn read_bool(buf: &[u8], pos: &mut usize) -> PojocResult<bool> {
    Ok(read_u8(buf, pos)? != 0)
}

/// Read a fixed 2-byte little-endian `u16`.
pub fn read_u16(buf: &[u8], pos: &mut usize) -> PojocResult<u16> {
    let end = pos.checked_add(2).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
}

/// Read a fixed 4-byte little-endian `u32`.
pub fn read_u32(buf: &[u8], pos: &mut usize) -> PojocResult<u32> {
    let end = pos.checked_add(4).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

/// Read a fixed 8-byte little-endian `u64`.
pub fn read_u64(buf: &[u8], pos: &mut usize) -> PojocResult<u64> {
    let end = pos.checked_add(8).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}

/// Read a fixed 4-byte little-endian `f32`.
pub fn read_f32(buf: &[u8], pos: &mut usize) -> PojocResult<f32> {
    let end = pos.checked_add(4).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(f32::from_le_bytes(bytes.try_into().unwrap()))
}

/// Read a fixed 8-byte little-endian `f64`.
pub fn read_f64(buf: &[u8], pos: &mut usize) -> PojocResult<f64> {
    let end = pos.checked_add(8).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(f64::from_le_bytes(bytes.try_into().unwrap()))
}

/// Read a fixed 1-byte little-endian `i8`.
pub fn read_i8(buf: &[u8], pos: &mut usize) -> PojocResult<i8> {
    Ok(read_u8(buf, pos)? as i8)
}

/// Read a fixed 2-byte little-endian `i16`.
pub fn read_i16(buf: &[u8], pos: &mut usize) -> PojocResult<i16> {
    let end = pos.checked_add(2).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(i16::from_le_bytes(bytes.try_into().unwrap()))
}

/// Read a fixed 4-byte little-endian `i32`.
pub fn read_i32(buf: &[u8], pos: &mut usize) -> PojocResult<i32> {
    let end = pos.checked_add(4).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
}

/// Read a fixed 8-byte little-endian `i64`.
pub fn read_i64(buf: &[u8], pos: &mut usize) -> PojocResult<i64> {
    let end = pos.checked_add(8).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(i64::from_le_bytes(bytes.try_into().unwrap()))
}

/// Read a length-prefixed byte slice. Returns a slice into the original buffer.
pub fn read_bytes<'a>(buf: &'a [u8], pos: &mut usize) -> PojocResult<&'a [u8]> {
    let len = read_varint64(buf, pos)? as usize;
    let end = pos.checked_add(len).ok_or(Error::InvalidLength)?;
    let slice = buf.get(*pos..end).ok_or(Error::InvalidLength)?;
    *pos = end;
    Ok(slice)
}

/// Read a fixed-length byte array.
pub fn read_fixed_bytes<const N: usize>(buf: &[u8], pos: &mut usize) -> PojocResult<[u8; N]> {
    let end = pos.checked_add(N).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(bytes.try_into().unwrap())
}

/// Read a fixed-length array of values.
pub fn read_fixed_array<
    T: Copy + Default,
    F: Fn(&[u8], &mut usize) -> PojocResult<T>,
    const N: usize,
>(
    buf: &[u8],
    pos: &mut usize,
    f: F,
) -> PojocResult<[T; N]> {
    let mut arr = [T::default(); N];
    for slot in arr.iter_mut() {
        *slot = f(buf, pos)?;
    }
    Ok(arr)
}

/// Read a length-prefixed UTF-8 string.
pub fn read_string<'a>(buf: &'a [u8], pos: &mut usize) -> PojocResult<&'a str> {
    let bytes = read_bytes(buf, pos)?;
    std::str::from_utf8(bytes).map_err(|_| Error::InvalidLength)
}

/// Read a length prefix for an array.
pub fn read_array_len(buf: &[u8], pos: &mut usize) -> PojocResult<usize> {
    Ok(read_varint64(buf, pos)? as usize)
}

/// Read a length-prefixed UTF-8 string from `buf` at `*pos` and return it as a [`PojocString`].
pub fn read_pojoc_string(buf: &[u8], pos: &mut usize) -> PojocResult<PojocString> {
    Ok(PojocString::from(read_string(buf, pos)?))
}

/// A decoded message envelope.
#[derive(Debug)]
pub struct Envelope<'a> {
    pub version: u64,
    pub payload: &'a [u8],
}

/// Read one message envelope from `buf` at `*pos`:
///   [version:varint] [len:u32] [payload...]
/// Returns the version and a slice over the payload.
pub fn read_envelope<'a>(buf: &'a [u8], pos: &mut usize) -> PojocResult<Envelope<'a>> {
    let version = read_varint64(buf, pos)?;
    let len = read_u32(buf, pos)? as usize;
    let end = pos.checked_add(len).ok_or(Error::InvalidLength)?;
    let payload = buf.get(*pos..end).ok_or(Error::InvalidLength)?;
    *pos = end;
    Ok(Envelope { version, payload })
}

/// Iterate over a stream of concatenated envelopes in `buf`.
/// Calls `f(version, payload)` for each message. Stops at EOF.
pub fn read_envelope_stream<F>(buf: &[u8], mut f: F) -> PojocResult<()>
where
    F: FnMut(u64, &[u8]) -> PojocResult<()>,
{
    let mut pos = 0;
    while pos < buf.len() {
        let envelope = read_envelope(buf, &mut pos)?;
        f(envelope.version, envelope.payload)?;
    }
    Ok(())
}

/// Skip a length-prefixed string without UTF-8 validation.
pub fn skip_string(buf: &[u8], pos: &mut usize) -> PojocResult<()> {
    let len = read_varint64(buf, pos)? as usize;
    let end = pos.checked_add(len).ok_or(Error::InvalidLength)?;
    if end > buf.len() {
        return Err(Error::InvalidLength);
    }
    *pos = end;
    Ok(())
}
