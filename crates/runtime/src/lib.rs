pub mod decode;
pub mod delta;
pub mod encode;
pub mod error;
pub mod varint;

pub use decode::*;
pub use delta::*;
pub use encode::*;
pub use error::*;
use std::borrow::Borrow;
pub use varint::*;

pub use compact_str::CompactString as PojocString;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

pub type PojocVec<T> = SmallVec<[T; 8]>;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PojocFixedMap<K, V, const N: usize = 0> {
    pub inner: PojocVec<(K, V)>,
}

impl<K, V, const N: usize> PojocFixedMap<K, V, N> {
    pub const fn new() -> Self {
        Self {
            inner: PojocVec::new_const(),
        }
    }

    pub fn with_capacity(n: usize) -> Self {
        Self {
            inner: PojocVec::with_capacity(n),
        }
    }

    pub fn push(&mut self, val: (K, V)) {
        self.inner.push(val);
    }
    pub fn iter(&self) -> impl Iterator<Item = &(K, V)> {
        self.inner.iter()
    }
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn clear(&mut self) {
        self.inner.clear();
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.inner.iter().map(|(k, _)| k)
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.iter().map(|(_, v)| v)
    }
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.inner.iter_mut().map(|(_, v)| v)
    }
}

impl<K: Eq, V, const N: usize> PojocFixedMap<K, V, N> {
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.inner
            .iter()
            .find(|(k, _)| k.borrow() == key)
            .map(|(_, v)| v)
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.inner
            .iter_mut()
            .find(|(k, _)| k.borrow() == key)
            .map(|(_, v)| v)
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.get(key).is_some()
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some((_, v)) = self.inner.iter_mut().find(|(k, _)| *k == key) {
            Some(std::mem::replace(v, value))
        } else {
            self.inner.push((key, value));
            None
        }
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        let index = self.inner.iter().position(|(k, _)| k.borrow() == key)?;
        Some(self.inner.swap_remove(index).1)
    }
}

impl<K: Default, V: Default, const N: usize> Default for PojocFixedMap<K, V, N> {
    fn default() -> Self {
        let mut map = Self::with_capacity(N);
        for _ in 0..N {
            map.push((K::default(), V::default()));
        }
        map
    }
}

impl<K, V, const N: usize> IntoIterator for PojocFixedMap<K, V, N> {
    type Item = (K, V);
    type IntoIter = smallvec::IntoIter<[(K, V); 8]>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, K, V, const N: usize> IntoIterator for &'a PojocFixedMap<K, V, N> {
    type Item = &'a (K, V);
    type IntoIter = std::slice::Iter<'a, (K, V)>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<'a, K, V, const N: usize> IntoIterator for &'a mut PojocFixedMap<K, V, N> {
    type Item = &'a mut (K, V);
    type IntoIter = std::slice::IterMut<'a, (K, V)>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter_mut()
    }
}

impl<K: Eq, V, const N: usize> FromIterator<(K, V)> for PojocFixedMap<K, V, N> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut map = Self::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

pub use std::collections::HashMap as PojocMap;

pub use serde_bytes::Bytes as SerdeBytes;

pub struct LazyView<'buf, T> {
    buf: &'buf [u8],
    decode_fn: fn(&[u8], &mut usize) -> PojocResult<T>,
}

impl<'buf, T> LazyView<'buf, T> {
    pub fn new(buf: &'buf [u8], decode_fn: fn(&[u8], &mut usize) -> PojocResult<T>) -> Self {
        Self { buf, decode_fn }
    }

    pub fn get(&self) -> PojocResult<T> {
        (self.decode_fn)(self.buf, &mut 0)
    }

    pub fn raw_bytes(&self) -> &'buf [u8] {
        self.buf
    }
}

impl<'buf, T> Clone for LazyView<'buf, T> {
    fn clone(&self) -> Self {
        Self {
            buf: self.buf,
            decode_fn: self.decode_fn,
        }
    }
}

impl<'buf, T> std::fmt::Debug for LazyView<'buf, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LazyView(<{} bytes, not yet decoded>)", self.buf.len())
    }
}

#[macro_export]
macro_rules! pojvec {
    // pojvec![]
    () => {
        $crate::PojocVec::new()
    };

    // pojvec![u32 =>]
    ($t:ty =>) => {
        $crate::PojocVec::<$t>::new()
    };

    // pojvec![u32 =>; 4]  →  [u32; 4] default-filled
    ($t:ty =>; $n:literal) => {
        ::core::array::from_fn::<$t, $n, _>(|_| <$t as ::core::default::Default>::default())
    };

    // pojvec![u32 => 1, 2, 3]  →  PojocVec<u32>
    ($t:ty => $($x:expr),+ $(,)?) => {{
        trait SafeCast<T> { fn cast(self) -> T; }
        impl<T, U> SafeCast<U> for T where T: ::core::convert::TryInto<U>, <T as ::core::convert::TryInto<U>>::Error: ::core::fmt::Debug {
            fn cast(self) -> U { self.try_into().expect("pojvec!: conversion failed") }
        }
        $crate::PojocVec::from_vec(vec![$( SafeCast::<$t>::cast($x) ),+])
    }};

    // pojvec![u32 => 1, 2, 3; 4]  →  [u32; 4]
    ($t:ty => $($x:expr),+ $(,)?; $n:literal) => {{
        trait SafeCast<T> { fn cast(self) -> T; }
        impl<T, U> SafeCast<U> for T where T: ::core::convert::TryInto<U>, <T as ::core::convert::TryInto<U>>::Error: ::core::fmt::Debug {
            fn cast(self) -> U { self.try_into().expect("pojvec!: conversion failed") }
        }
        let __arr: [$t; $n] = [$( SafeCast::<$t>::cast($x) ),+];
        __arr
    }};

    // pojvec![1, 2, 3; 3]  →  [T; 3] inferred
    ($($x:expr),+ $(,)?; $n:literal) => {{
        trait SafeCast<T> { fn cast(self) -> T; }
        impl<T, U> SafeCast<U> for T where T: ::core::convert::TryInto<U>, <T as ::core::convert::TryInto<U>>::Error: ::core::fmt::Debug {
            fn cast(self) -> U { self.try_into().expect("pojvec!: conversion failed") }
        }
        let __arr: [_; $n] = [$( SafeCast::cast($x) ),+];
        __arr
    }};

    // pojvec![1, 2, 3]  →  PojocVec<T> inferred
    ($($x:expr),+ $(,)?) => {{
        trait SafeCast<T> { fn cast(self) -> T; }
        impl<T, U> SafeCast<U> for T where T: ::core::convert::TryInto<U>, <T as ::core::convert::TryInto<U>>::Error: ::core::fmt::Debug {
            fn cast(self) -> U { self.try_into().expect("pojvec!: conversion failed") }
        }
        $crate::PojocVec::from_vec(vec![$( SafeCast::cast($x) ),+])
    }};
}

#[macro_export]
macro_rules! pojstr {
    ($s:literal, $n:expr) => {{
        const _ASSERT: () = assert!(
            $s.as_bytes().len() == $n,
            "pojstr!: string length does not match expected size"
        );
        *$s.as_bytes().first_chunk::<$n>().unwrap()
    }};

    ($s:expr) => {{ $crate::PojocString::from($s) }};
}

#[macro_export]
macro_rules! pojmap {
    () => {{
        $crate::PojocMap::new()
    }};

    ($k:ty, $v:ty) => {{
        $crate::PojocMap::<$k, $v>::new()
    }};

    ($($k:expr => $v:expr),+ $(,)?) => {{
        let mut __m = $crate::PojocMap::new();
        $(
            __m.insert(
                ::core::convert::Into::into($k),
                ::core::convert::Into::into($v),
            );
        )+
        __m
    }};

    ($n:literal) => {{
        $crate::PojocFixedMap::with_capacity($n)
    }};

    ($k:ty, $v:ty; $n:literal) => {{
        $crate::PojocFixedMap::<$k, $v>::with_capacity($n)
    }};

    ($($k:expr => $v:expr),+ $(,)?; $n:literal) => {{
        const __LEN: usize = [$(stringify!($k)),+].len();
        const _: () = assert!(__LEN == $n, "pojmap!: entry count does not match declared size");
        let mut __m = $crate::PojocFixedMap::with_capacity($n);
        $(
            __m.push((
                ::core::convert::Into::into($k),
                ::core::convert::Into::into($v),
            ));
        )+
        __m
    }};
}

#[macro_export]
macro_rules! pojtup {
    ($($x:expr),+ $(,)?) => {{
        ($(::core::convert::Into::into($x)),+)
    }};
}
