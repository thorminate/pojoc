use crate::PojocVec;
use crate::error::Error;
use crate::varint::{
    read_varint32, read_varint64, skip_varint32, skip_varint64, varint_size, write_varint32,
    write_varint64,
};

#[inline]
pub fn zigzag_encode(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

#[inline]
pub fn zigzag_decode(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

#[inline]
pub fn write_signed_varint(out: &mut Vec<u8>, v: i64) {
    write_varint64(out, zigzag_encode(v));
}

#[inline]
pub fn read_signed_varint(buf: &[u8], pos: &mut usize) -> Result<i64, Error> {
    Ok(zigzag_decode(read_varint64(buf, pos)?))
}

#[inline]
pub fn skip_signed_varint(buf: &[u8], pos: &mut usize) -> Result<(), Error> {
    skip_varint64(buf, pos)
}

// first element of a delta array uses its natural representation (unsigned
// varint for unsigned types, zigzag for signed) so large unsigned values
// don't get doubled by zigzag; later elements are zigzag deltas from the
// previous element via wrapping arithmetic so u64 wraparound still round-trips
pub trait DeltaElement: Copy + Default {
    fn write_first(out: &mut Vec<u8>, value: Self);
    fn read_first(buf: &[u8], pos: &mut usize) -> Result<Self, Error>;
    fn skip_first(buf: &[u8], pos: &mut usize) -> Result<(), Error>;
    fn delta_to(self, prev: Self) -> i64;
    fn apply_delta(prev: Self, delta: i64) -> Self;
    // size of value as written by write_first, computed arithmetically to avoid a scratch alloc
    fn first_encoded_size(value: Self) -> usize;
}

macro_rules! impl_delta_unsigned32 {
    ($t:ty) => {
        impl DeltaElement for $t {
            #[inline]
            fn write_first(out: &mut Vec<u8>, value: Self) {
                write_varint32(out, value as u32);
            }
            #[inline]
            fn read_first(buf: &[u8], pos: &mut usize) -> Result<Self, Error> {
                Ok(read_varint32(buf, pos)? as $t)
            }
            #[inline]
            fn skip_first(buf: &[u8], pos: &mut usize) -> Result<(), Error> {
                skip_varint32(buf, pos)
            }
            #[inline]
            fn delta_to(self, prev: Self) -> i64 {
                self as i64 - prev as i64
            }
            #[inline]
            fn apply_delta(prev: Self, delta: i64) -> Self {
                (prev as i64 + delta) as $t
            }
            #[inline]
            fn first_encoded_size(value: Self) -> usize {
                varint_size(value as u32 as usize)
            }
        }
    };
}
impl_delta_unsigned32!(u8);
impl_delta_unsigned32!(u16);
impl_delta_unsigned32!(u32);

macro_rules! impl_delta_signed32 {
    ($t:ty) => {
        impl DeltaElement for $t {
            #[inline]
            fn write_first(out: &mut Vec<u8>, value: Self) {
                write_signed_varint(out, value as i64);
            }
            #[inline]
            fn read_first(buf: &[u8], pos: &mut usize) -> Result<Self, Error> {
                Ok(read_signed_varint(buf, pos)? as $t)
            }
            #[inline]
            fn skip_first(buf: &[u8], pos: &mut usize) -> Result<(), Error> {
                skip_signed_varint(buf, pos)
            }
            #[inline]
            fn delta_to(self, prev: Self) -> i64 {
                self as i64 - prev as i64
            }
            #[inline]
            fn apply_delta(prev: Self, delta: i64) -> Self {
                (prev as i64 + delta) as $t
            }
            #[inline]
            fn first_encoded_size(value: Self) -> usize {
                varint_size(zigzag_encode(value as i64) as usize)
            }
        }
    };
}
impl_delta_signed32!(i8);
impl_delta_signed32!(i16);
impl_delta_signed32!(i32);

impl DeltaElement for u64 {
    #[inline]
    fn write_first(out: &mut Vec<u8>, value: Self) {
        write_varint64(out, value);
    }
    #[inline]
    fn read_first(buf: &[u8], pos: &mut usize) -> Result<Self, Error> {
        read_varint64(buf, pos)
    }
    #[inline]
    fn skip_first(buf: &[u8], pos: &mut usize) -> Result<(), Error> {
        skip_varint64(buf, pos)
    }
    #[inline]
    fn delta_to(self, prev: Self) -> i64 {
        self.wrapping_sub(prev) as i64
    }
    #[inline]
    fn apply_delta(prev: Self, delta: i64) -> Self {
        prev.wrapping_add(delta as u64)
    }
    #[inline]
    fn first_encoded_size(value: Self) -> usize {
        varint_size(value as usize)
    }
}

impl DeltaElement for i64 {
    #[inline]
    fn write_first(out: &mut Vec<u8>, value: Self) {
        write_signed_varint(out, value);
    }
    #[inline]
    fn read_first(buf: &[u8], pos: &mut usize) -> Result<Self, Error> {
        read_signed_varint(buf, pos)
    }
    #[inline]
    fn skip_first(buf: &[u8], pos: &mut usize) -> Result<(), Error> {
        skip_signed_varint(buf, pos)
    }
    #[inline]
    fn delta_to(self, prev: Self) -> i64 {
        self.wrapping_sub(prev)
    }
    #[inline]
    fn apply_delta(prev: Self, delta: i64) -> Self {
        prev.wrapping_add(delta)
    }
    #[inline]
    fn first_encoded_size(value: Self) -> usize {
        varint_size(zigzag_encode(value) as usize)
    }
}

#[inline]
pub fn write_delta_array<T: DeltaElement>(out: &mut Vec<u8>, items: &[T]) {
    write_varint32(out, items.len() as u32);
    if items.is_empty() {
        return;
    }
    let mut prev = items[0];
    T::write_first(out, prev);
    for &item in &items[1..] {
        write_signed_varint(out, item.delta_to(prev));
        prev = item;
    }
}

#[inline]
pub fn read_delta_array<T: DeltaElement>(
    buf: &[u8],
    pos: &mut usize,
) -> Result<PojocVec<T>, Error> {
    let len = read_varint32(buf, pos)? as usize;
    let mut out = PojocVec::with_capacity(len);
    if len == 0 {
        return Ok(out);
    }
    let mut prev = T::read_first(buf, pos)?;
    out.push(prev);
    for _ in 1..len {
        let delta = read_signed_varint(buf, pos)?;
        prev = T::apply_delta(prev, delta);
        out.push(prev);
    }
    Ok(out)
}

#[inline]
pub fn skip_delta_array<T: DeltaElement>(buf: &[u8], pos: &mut usize) -> Result<(), Error> {
    let len = read_varint32(buf, pos)? as usize;
    if len == 0 {
        return Ok(());
    }
    T::skip_first(buf, pos)?;
    for _ in 1..len {
        skip_signed_varint(buf, pos)?;
    }
    Ok(())
}

#[inline]
pub fn write_fixed_delta_array<T: DeltaElement>(out: &mut Vec<u8>, items: &[T]) {
    if items.is_empty() {
        return;
    }
    let mut prev = items[0];
    T::write_first(out, prev);
    for &item in &items[1..] {
        write_signed_varint(out, item.delta_to(prev));
        prev = item;
    }
}

#[inline]
pub fn read_fixed_delta_array<T: DeltaElement, const N: usize>(
    buf: &[u8],
    pos: &mut usize,
) -> Result<[T; N], Error> {
    let mut out = [T::default(); N];
    if N == 0 {
        return Ok(out);
    }
    out[0] = T::read_first(buf, pos)?;
    for i in 1..N {
        let delta = read_signed_varint(buf, pos)?;
        out[i] = T::apply_delta(out[i - 1], delta);
    }
    Ok(out)
}

#[inline]
pub fn skip_fixed_delta_array<T: DeltaElement, const N: usize>(
    buf: &[u8],
    pos: &mut usize,
) -> Result<(), Error> {
    if N == 0 {
        return Ok(());
    }
    T::skip_first(buf, pos)?;
    for _ in 1..N {
        skip_signed_varint(buf, pos)?;
    }
    Ok(())
}

#[inline]
fn signed_varint_size(delta: i64) -> usize {
    varint_size(zigzag_encode(delta) as usize)
}

#[inline]
pub fn delta_array_size_hint<T: DeltaElement>(items: &[T]) -> usize {
    let mut size = varint_size(items.len());
    if items.is_empty() {
        return size;
    }
    size += T::first_encoded_size(items[0]);
    let mut prev = items[0];
    for &item in &items[1..] {
        size += signed_varint_size(item.delta_to(prev));
        prev = item;
    }
    size
}

#[inline]
pub fn fixed_delta_array_size_hint<T: DeltaElement>(items: &[T]) -> usize {
    if items.is_empty() {
        return 0;
    }
    let mut size = T::first_encoded_size(items[0]);
    let mut prev = items[0];
    for &item in &items[1..] {
        size += signed_varint_size(item.delta_to(prev));
        prev = item;
    }
    size
}
