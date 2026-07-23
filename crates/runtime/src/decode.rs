use crate::{Error, PojocResult, read_varint64};

#[inline]
pub fn read_u8(buf: &[u8], pos: &mut usize) -> PojocResult<u8> {
    let byte = *buf.get(*pos).ok_or(Error::UnexpectedEof)?;
    *pos += 1;
    Ok(byte)
}

#[inline]
pub fn read_bool(buf: &[u8], pos: &mut usize) -> PojocResult<bool> {
    Ok(read_u8(buf, pos)? != 0)
}

#[inline]
pub fn read_u16(buf: &[u8], pos: &mut usize) -> PojocResult<u16> {
    let end = pos.checked_add(2).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
}

#[inline]
pub fn read_u32(buf: &[u8], pos: &mut usize) -> PojocResult<u32> {
    let end = pos.checked_add(4).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

#[inline]
pub fn read_u64(buf: &[u8], pos: &mut usize) -> PojocResult<u64> {
    let end = pos.checked_add(8).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}

#[inline]
pub fn read_f32(buf: &[u8], pos: &mut usize) -> PojocResult<f32> {
    let end = pos.checked_add(4).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(f32::from_le_bytes(bytes.try_into().unwrap()))
}

#[inline]
pub fn read_f64(buf: &[u8], pos: &mut usize) -> PojocResult<f64> {
    let end = pos.checked_add(8).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(f64::from_le_bytes(bytes.try_into().unwrap()))
}

#[inline]
pub fn read_i8(buf: &[u8], pos: &mut usize) -> PojocResult<i8> {
    Ok(read_u8(buf, pos)? as i8)
}

#[inline]
pub fn read_i16(buf: &[u8], pos: &mut usize) -> PojocResult<i16> {
    let end = pos.checked_add(2).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(i16::from_le_bytes(bytes.try_into().unwrap()))
}

#[inline]
pub fn read_i32(buf: &[u8], pos: &mut usize) -> PojocResult<i32> {
    let end = pos.checked_add(4).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
}

#[inline]
pub fn read_i64(buf: &[u8], pos: &mut usize) -> PojocResult<i64> {
    let end = pos.checked_add(8).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(i64::from_le_bytes(bytes.try_into().unwrap()))
}

#[inline]
pub fn read_bytes<'a>(buf: &'a [u8], pos: &mut usize) -> PojocResult<&'a [u8]> {
    let len = read_varint64(buf, pos)? as usize;
    let end = pos.checked_add(len).ok_or(Error::InvalidLength)?;
    let slice = buf.get(*pos..end).ok_or(Error::InvalidLength)?;
    *pos = end;
    Ok(slice)
}

#[inline]
pub fn read_fixed_bytes<const N: usize>(buf: &[u8], pos: &mut usize) -> PojocResult<[u8; N]> {
    let end = pos.checked_add(N).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    Ok(bytes.try_into().unwrap())
}

#[inline]
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

// generated `decode_vN` groups all statically-fixed-size fields into one
// contiguous leading block, validates the whole span with a single
// `checked_add`/`len()` check, then reads through it with these unchecked
// accessors instead of bounds-checking each field

/// # Safety
/// caller must guarantee `*pos < buf.len()`
#[inline]
pub unsafe fn read_u8_unchecked(buf: &[u8], pos: &mut usize) -> u8 {
    let p = *pos;
    *pos = p + 1;
    unsafe { *buf.get_unchecked(p) }
}

/// # Safety
/// caller must guarantee `*pos < buf.len()`
#[inline]
pub unsafe fn read_bool_unchecked(buf: &[u8], pos: &mut usize) -> bool {
    unsafe { read_u8_unchecked(buf, pos) != 0 }
}

/// # Safety
/// caller must guarantee `*pos < buf.len()`
#[inline]
pub unsafe fn read_i8_unchecked(buf: &[u8], pos: &mut usize) -> i8 {
    unsafe { read_u8_unchecked(buf, pos) as i8 }
}

macro_rules! unchecked_le {
    ($name:ident, $ty:ty, $n:literal) => {
        /// # Safety
        /// caller must guarantee `*pos + N <= buf.len()`
        #[inline]
        pub unsafe fn $name(buf: &[u8], pos: &mut usize) -> $ty {
            let p = *pos;
            *pos = p + $n;
            // [u8; N] is align 1, so this raw-pointer read is sound
            let bytes = unsafe { *(buf.as_ptr().add(p) as *const [u8; $n]) };
            <$ty>::from_le_bytes(bytes)
        }
    };
}

unchecked_le!(read_u16_unchecked, u16, 2);
unchecked_le!(read_u32_unchecked, u32, 4);
unchecked_le!(read_u64_unchecked, u64, 8);
unchecked_le!(read_i16_unchecked, i16, 2);
unchecked_le!(read_i32_unchecked, i32, 4);
unchecked_le!(read_i64_unchecked, i64, 8);
unchecked_le!(read_f32_unchecked, f32, 4);
unchecked_le!(read_f64_unchecked, f64, 8);

/// # Safety
/// caller must guarantee `*pos + N <= buf.len()`
#[inline]
pub unsafe fn read_fixed_bytes_unchecked<const N: usize>(buf: &[u8], pos: &mut usize) -> [u8; N] {
    let p = *pos;
    *pos = p + N;
    unsafe { *(buf.as_ptr().add(p) as *const [u8; N]) }
}

// uses simdutf8 (SIMD-accelerated, falls back to scalar) instead of
// std::str::from_utf8 for faster validation on mostly-ASCII wire data
#[inline]
pub fn read_string<'a>(buf: &'a [u8], pos: &mut usize) -> PojocResult<&'a str> {
    let bytes = read_bytes(buf, pos)?;
    simdutf8::basic::from_utf8(bytes).map_err(|_| Error::InvalidLength)
}

#[inline]
pub fn read_array_len(buf: &[u8], pos: &mut usize) -> PojocResult<usize> {
    Ok(read_varint64(buf, pos)? as usize)
}

// lets whole arrays of these scalars be bulk-cast via bytemuck instead of
// read element-by-element; to_wire_le is a no-op on little-endian hosts
pub trait WireScalar: bytemuck::Pod + Copy {
    fn to_wire_le(self) -> Self;
}

macro_rules! impl_wire_scalar_int {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl WireScalar for $ty {
                #[inline]
                fn to_wire_le(self) -> Self {
                    self.to_le()
                }
            }
        )+
    };
}
impl_wire_scalar_int!(u8, i8, u16, i16, u32, i32, u64, i64);

impl WireScalar for f32 {
    #[inline]
    fn to_wire_le(self) -> Self {
        Self::from_bits(self.to_bits().to_le())
    }
}

impl WireScalar for f64 {
    #[inline]
    fn to_wire_le(self) -> Self {
        Self::from_bits(self.to_bits().to_le())
    }
}

#[inline]
pub fn read_pod_array<T: WireScalar>(
    buf: &[u8],
    pos: &mut usize,
) -> PojocResult<crate::PojocVec<T>> {
    let n = read_array_len(buf, pos)?;
    let byte_len = n
        .checked_mul(core::mem::size_of::<T>())
        .ok_or(Error::InvalidLength)?;
    let end = pos.checked_add(byte_len).ok_or(Error::InvalidLength)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    let mut v: crate::PojocVec<T> = crate::PojocVec::from_vec(bytemuck::pod_collect_to_vec(bytes));
    if !cfg!(target_endian = "little") {
        for x in v.iter_mut() {
            *x = x.to_wire_le();
        }
    }
    Ok(v)
}

#[inline]
pub fn read_fixed_pod_array<T: WireScalar, const N: usize>(
    buf: &[u8],
    pos: &mut usize,
) -> PojocResult<[T; N]> {
    let byte_len = N * core::mem::size_of::<T>();
    let end = pos.checked_add(byte_len).ok_or(Error::UnexpectedEof)?;
    let bytes = buf.get(*pos..end).ok_or(Error::UnexpectedEof)?;
    *pos = end;
    let mut arr = [<T as bytemuck::Zeroable>::zeroed(); N];
    bytemuck::cast_slice_mut::<T, u8>(&mut arr).copy_from_slice(bytes);
    if !cfg!(target_endian = "little") {
        for x in arr.iter_mut() {
            *x = x.to_wire_le();
        }
    }
    Ok(arr)
}

/// # Safety
/// caller must guarantee `*pos + N * size_of::<T>() <= buf.len()`
#[inline]
pub unsafe fn read_fixed_pod_array_unchecked<T: WireScalar, const N: usize>(
    buf: &[u8],
    pos: &mut usize,
) -> [T; N] {
    let p = *pos;
    let byte_len = N * core::mem::size_of::<T>();
    *pos = p + byte_len;
    let mut arr = [<T as bytemuck::Zeroable>::zeroed(); N];
    unsafe {
        core::ptr::copy_nonoverlapping(buf.as_ptr().add(p), arr.as_mut_ptr() as *mut u8, byte_len);
    }
    if !cfg!(target_endian = "little") {
        for x in arr.iter_mut() {
            *x = x.to_wire_le();
        }
    }
    arr
}

#[derive(Debug)]
pub struct Envelope<'a> {
    pub version: u64,
    pub payload: &'a [u8],
}

// envelope wire layout: [version:varint] [len:u32] [payload...]
pub fn read_envelope<'a>(buf: &'a [u8], pos: &mut usize) -> PojocResult<Envelope<'a>> {
    let version = read_varint64(buf, pos)?;
    let len = read_u32(buf, pos)? as usize;
    let end = pos.checked_add(len).ok_or(Error::InvalidLength)?;
    let payload = buf.get(*pos..end).ok_or(Error::InvalidLength)?;
    *pos = end;
    Ok(Envelope { version, payload })
}

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

// skips without UTF-8 validation, unlike read_string
pub fn skip_string(buf: &[u8], pos: &mut usize) -> PojocResult<()> {
    let len = read_varint64(buf, pos)? as usize;
    let end = pos.checked_add(len).ok_or(Error::InvalidLength)?;
    buf.get(*pos..end).ok_or(Error::InvalidLength)?;
    *pos = end;
    Ok(())
}
