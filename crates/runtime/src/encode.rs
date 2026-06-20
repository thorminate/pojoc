use crate::{write_varint64, PojocString};

/// Write a single `u8`.
#[inline]
pub fn write_u8(buf: &mut Vec<u8>, value: u8) {
    buf.push(value);
}

/// Write a `bool` (0 = false, 1 = true).
#[inline]
pub fn write_bool(buf: &mut Vec<u8>, value: bool) {
    buf.push(value as u8);
}

/// Write a fixed 2-byte little-endian `u16`.
#[inline]
pub fn write_u16(buf: &mut Vec<u8>, value: u16) {
    buf.extend_from_slice(&value.to_le_bytes());
}

/// Write a fixed 4-byte little-endian `u32`.
#[inline]
pub fn write_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

/// Write a fixed 8-byte little-endian `u64`.
#[inline]
pub fn write_u64(buf: &mut Vec<u8>, value: u64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

/// Write a fixed 4-byte little-endian `f32`.
#[inline]
pub fn write_f32(buf: &mut Vec<u8>, value: f32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

/// Write a fixed 8-byte little-endian `f64`.
#[inline]
pub fn write_f64(buf: &mut Vec<u8>, value: f64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

/// Write a fixed 1-byte `i8`.
#[inline]
pub fn write_i8(buf: &mut Vec<u8>, value: i8) {
    buf.push(value as u8);
}

/// Write a fixed 2-byte little-endian `i16`.
#[inline]
pub fn write_i16(buf: &mut Vec<u8>, value: i16) {
    buf.extend_from_slice(&value.to_le_bytes());
}

/// Write a fixed 4-byte little-endian `i32`.
#[inline]
pub fn write_i32(buf: &mut Vec<u8>, value: i32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

/// Write a fixed 8-byte little-endian `i64`.
#[inline]
pub fn write_i64(buf: &mut Vec<u8>, value: i64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

/// Write a length-prefixed byte slice (varint len + raw bytes).
#[inline]
pub fn write_bytes(buf: &mut Vec<u8>, value: &[u8]) {
    write_varint64(buf, value.len() as u64);
    buf.extend_from_slice(value);
}

/// Write a fixed-length byte array (no length prefix).
#[inline]
pub fn write_fixed_bytes<const N: usize>(buf: &mut Vec<u8>, value: &[u8; N]) {
    buf.extend_from_slice(value);
}

/// Write a fixed-length array of values via a per-element writer.
#[inline]
pub fn write_fixed_array<T: Copy, F: Fn(&mut Vec<u8>, T), const N: usize>(
    buf: &mut Vec<u8>,
    arr: &[T; N],
    f: F,
) {
    for &item in arr.iter() {
        f(buf, item);
    }
}

/// Write a length-prefixed UTF-8 string.
#[inline]
pub fn write_string(buf: &mut Vec<u8>, value: &str) {
    write_bytes(buf, value.as_bytes());
}

/// Write a length prefix for an array.
#[inline]
pub fn write_array_len(buf: &mut Vec<u8>, len: usize) {
    write_varint64(buf, len as u64);
}

/// Write a [`PojocString`] as a length-prefixed UTF-8 string.
#[inline]
pub fn write_pojoc_string(buf: &mut Vec<u8>, value: &PojocString) {
    write_string(buf, value.as_str());
}

/// Write the start of a message envelope: `[version:varint] [len:u32 placeholder]`.
/// Returns the buffer offset of the length placeholder for `patch_envelope_length`.
#[inline]
pub fn write_envelope_header(buf: &mut Vec<u8>, version: u64) -> usize {
    write_varint64(buf, version);
    let len_pos = buf.len();
    buf.extend_from_slice(&[0u8; 4]);
    len_pos
}

/// Patch the `u32` length placeholder at `len_pos` with the actual payload length.
#[inline]
pub fn patch_envelope_length(buf: &mut Vec<u8>, len_pos: usize, payload_len: usize) {
    debug_assert!(payload_len <= u32::MAX as usize, "envelope payload too large for u32 length");
    buf[len_pos..len_pos + 4].copy_from_slice(&(payload_len as u32).to_le_bytes());
}