use crate::write_varint64;

#[inline]
pub fn write_u8(buf: &mut Vec<u8>, value: u8) {
    buf.push(value);
}

#[inline]
pub fn write_bool(buf: &mut Vec<u8>, value: bool) {
    buf.push(value as u8);
}

#[inline]
pub fn write_u16(buf: &mut Vec<u8>, value: u16) {
    buf.extend_from_slice(&value.to_le_bytes());
}

#[inline]
pub fn write_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

#[inline]
pub fn write_u64(buf: &mut Vec<u8>, value: u64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

#[inline]
pub fn write_f32(buf: &mut Vec<u8>, value: f32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

#[inline]
pub fn write_f64(buf: &mut Vec<u8>, value: f64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

#[inline]
pub fn write_i8(buf: &mut Vec<u8>, value: i8) {
    buf.push(value as u8);
}

#[inline]
pub fn write_i16(buf: &mut Vec<u8>, value: i16) {
    buf.extend_from_slice(&value.to_le_bytes());
}

#[inline]
pub fn write_i32(buf: &mut Vec<u8>, value: i32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

#[inline]
pub fn write_i64(buf: &mut Vec<u8>, value: i64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

#[inline]
pub fn write_bytes(buf: &mut Vec<u8>, value: &[u8]) {
    write_varint64(buf, value.len() as u64);
    buf.extend_from_slice(value);
}

#[inline]
pub fn write_fixed_bytes<const N: usize>(buf: &mut Vec<u8>, value: &[u8; N]) {
    buf.extend_from_slice(value);
}

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

#[inline]
pub fn write_string(buf: &mut Vec<u8>, value: &str) {
    write_bytes(buf, value.as_bytes());
}

#[inline]
pub fn write_array_len(buf: &mut Vec<u8>, len: usize) {
    write_varint64(buf, len as u64);
}

#[inline]
pub fn write_pod_array<T: crate::WireScalar>(buf: &mut Vec<u8>, values: &[T]) {
    write_array_len(buf, values.len());
    if cfg!(target_endian = "little") {
        buf.extend_from_slice(bytemuck::cast_slice(values));
    } else {
        let swapped: crate::PojocVec<T> = values.iter().map(|v| v.to_wire_le()).collect();
        buf.extend_from_slice(bytemuck::cast_slice(swapped.as_slice()));
    }
}

#[inline]
pub fn write_fixed_pod_array<T: crate::WireScalar, const N: usize>(
    buf: &mut Vec<u8>,
    arr: &[T; N],
) {
    if cfg!(target_endian = "little") {
        buf.extend_from_slice(bytemuck::cast_slice(arr));
    } else {
        let swapped: [T; N] = std::array::from_fn(|i| arr[i].to_wire_le());
        buf.extend_from_slice(bytemuck::cast_slice(&swapped));
    }
}

// writes [version:varint] [len:u32 placeholder]; returned offset is for patch_envelope_length
#[inline]
pub fn write_envelope_header(buf: &mut Vec<u8>, version: u64) -> usize {
    write_varint64(buf, version);
    let len_pos = buf.len();
    buf.extend_from_slice(&[0u8; 4]);
    len_pos
}

#[inline]
pub fn patch_envelope_length(buf: &mut [u8], len_pos: usize, payload_len: usize) {
    debug_assert!(
        payload_len <= u32::MAX as usize,
        "envelope payload too large for u32 length"
    );
    buf[len_pos..len_pos + 4].copy_from_slice(&(payload_len as u32).to_le_bytes());
}
