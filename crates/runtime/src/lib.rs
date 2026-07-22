pub mod decode;
pub mod delta;
pub mod encode;
pub mod error;
pub mod intern;
pub mod varint;

pub use decode::*;
pub use delta::*;
pub use encode::*;
pub use error::*;
pub use intern::*;
use std::borrow::Borrow;
pub use varint::*;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// A vector that stores up to 8 elements inline before spilling to the heap.
pub type PojocVec<T> = SmallVec<[T; 8]>;

/// A map for `map<K, V>(N)` fields, backed by a linear `PojocVec<(K, V)>`
/// rather than hashing — lookups are O(n), which is fine for the small `N`
/// fixed maps are declared with.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PojocFixedMap<K, V, const N: usize = 0> {
    pub inner: PojocVec<(K, V)>,
}

impl<K, V, const N: usize> PojocFixedMap<K, V, N> {
    #[inline]
    pub const fn new() -> Self {
        Self {
            inner: PojocVec::new_const(),
        }
    }

    #[inline]
    pub fn with_capacity(n: usize) -> Self {
        Self {
            inner: PojocVec::with_capacity(n),
        }
    }

    #[inline]
    pub fn push(&mut self, val: (K, V)) {
        self.inner.push(val);
    }
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &(K, V)> {
        self.inner.iter()
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }
    #[inline]
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.inner.iter().map(|(k, _)| k)
    }
    #[inline]
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.iter().map(|(_, v)| v)
    }
    #[inline]
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.inner.iter_mut().map(|(_, v)| v)
    }
}

impl<K: Eq, V, const N: usize> PojocFixedMap<K, V, N> {
    #[inline]
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

    #[inline]
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

    #[inline]
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.get(key).is_some()
    }

    #[inline]
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some((_, v)) = self.inner.iter_mut().find(|(k, _)| *k == key) {
            Some(std::mem::replace(v, value))
        } else {
            self.inner.push((key, value));
            None
        }
    }

    #[inline]
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
    #[inline]
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
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, K, V, const N: usize> IntoIterator for &'a PojocFixedMap<K, V, N> {
    type Item = &'a (K, V);
    type IntoIter = std::slice::Iter<'a, (K, V)>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<'a, K, V, const N: usize> IntoIterator for &'a mut PojocFixedMap<K, V, N> {
    type Item = &'a mut (K, V);
    type IntoIter = std::slice::IterMut<'a, (K, V)>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter_mut()
    }
}

impl<K: Eq, V, const N: usize> FromIterator<(K, V)> for PojocFixedMap<K, V, N> {
    #[inline]
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut map = Self::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

/// The hash-map keying used by [`PojocMap`] and `InternBuilder`: a
/// per-process random seed, defending against hash-flooding when their keys
/// come from decoded wire data — see `docs/security.md`. Backed by
/// [`foldhash::fast::RandomState`] by default (the `foldhash` feature); with
/// that feature disabled, falls back to std's SipHash-based `RandomState`.
#[cfg(feature = "foldhash")]
pub type PojocHasher = foldhash::fast::RandomState;

/// The hash-map keying used by [`PojocMap`] and `InternBuilder` — std's
/// SipHash-based `RandomState` (the `foldhash` feature is disabled).
#[cfg(not(feature = "foldhash"))]
pub type PojocHasher = std::collections::hash_map::RandomState;

/// A map for `map<K, V>` fields, backed by `std::collections::HashMap`
/// keyed with [`PojocHasher`].
pub type PojocMap<K, V> = std::collections::HashMap<K, V, PojocHasher>;

#[cfg(feature = "serde")]
pub use serde_bytes::Bytes as SerdeBytes;

/// A `lazy` field's value: either unread bytes plus the decoder to run on
/// [`get`](Self::get), or an already-decoded/constructed value.
pub enum LazyView<'buf, T> {
    Raw {
        buf: &'buf [u8],
        // Input is `&'buf [u8]` (not an elided/fresh lifetime) so that a decoded
        // `T` which itself borrows the buffer (e.g. `Foo<'buf>` with zero-copy
        // strings) type-checks: a generic reader `for<'a> fn(&'a [u8]) -> Foo<'a>`
        // coerces to this fixed-`'buf` pointer, and owned readers coerce too.
        decode_fn: fn(&'buf [u8], &mut usize) -> PojocResult<T>,
    },
    Owned(T),
}

impl<'buf, T> LazyView<'buf, T> {
    #[inline]
    pub const fn new(
        buf: &'buf [u8],
        decode_fn: fn(&'buf [u8], &mut usize) -> PojocResult<T>,
    ) -> Self {
        Self::Raw { buf, decode_fn }
    }

    #[inline]
    pub fn get(self) -> PojocResult<T> {
        match self {
            Self::Raw { buf, decode_fn } => decode_fn(buf, &mut 0),
            Self::Owned(val) => Ok(val),
        }
    }

    #[inline]
    pub fn raw_bytes(&self) -> Option<&'buf [u8]> {
        match self {
            Self::Raw { buf, .. } => Some(buf),
            Self::Owned(_) => None,
        }
    }
}

impl<'buf, T: Clone> Clone for LazyView<'buf, T> {
    fn clone(&self) -> Self {
        match self {
            Self::Raw { buf, decode_fn } => Self::Raw {
                buf,
                decode_fn: *decode_fn,
            },
            Self::Owned(val) => Self::Owned(val.clone()),
        }
    }
}

impl<'buf, T> std::fmt::Debug for LazyView<'buf, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Raw { buf, .. } => write!(f, "LazyView::Raw(<{} bytes>)", buf.len()),
            Self::Owned(_) => write!(f, "LazyView::Owned"),
        }
    }
}

/// Build a [`PojocVec`], or a fixed-size array when a `; N` length is given,
/// converting each element with `Into` when a target element type is named.
///
/// ```
/// use pojoc::{PojocVec, pojvec};
///
/// let empty: PojocVec<i32> = pojvec![];
/// let typed_empty: PojocVec<u32> = pojvec![u32 =>];
/// let filled: [u32; 4] = pojvec![u32 =>; 4];
/// let typed: PojocVec<u32> = pojvec![u32 => 1, 2, 3];
/// let typed_arr: [u32; 3] = pojvec![u32 => 1, 2, 3; 3];
/// let arr: [i32; 3] = pojvec![1, 2, 3; 3];
/// let inferred: PojocVec<i32> = pojvec![1, 2, 3];
/// ```
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

/// Build a fixed-size byte array (`[u8; N]`) for a `FixedString` field from a
/// string literal, asserting the length matches at compile time. Non-fixed
/// `string` fields decode as borrowed `&str`, so a plain string literal is used
/// for those directly — there is no owned-string constructor.
///
/// ```
/// use pojoc::pojstr;
///
/// let name: [u8; 5] = pojstr!("hello", 5);
/// assert_eq!(&name, b"hello");
/// ```
#[macro_export]
macro_rules! pojstr {
    ($s:literal, $n:expr) => {{
        const _ASSERT: () = assert!(
            $s.as_bytes().len() == $n,
            "pojstr!: string length does not match expected size"
        );
        *$s.as_bytes().first_chunk::<$n>().unwrap()
    }};
}

/// Build a [`PojocMap`], or a [`PojocFixedMap`] when an `N` capacity is given,
/// converting each key/value with `Into`.
///
/// ```
/// use pojoc::{PojocFixedMap, PojocMap, pojmap};
///
/// let empty: PojocMap<i32, i32> = pojmap!();
/// let typed_empty: PojocMap<i32, i32> = pojmap!(i32, i32);
/// let map: PojocMap<i32, i32> = pojmap!(1 => 10, 2 => 20);
/// let fixed_empty: PojocFixedMap<i32, i32> = pojmap!(2);
/// let fixed_typed_empty: PojocFixedMap<i32, i32> = pojmap!(i32, i32; 2);
/// let fixed: PojocFixedMap<i32, i32> = pojmap!(1 => 10, 2 => 20; 2);
/// ```
#[macro_export]
macro_rules! pojmap {
    () => {{
        $crate::PojocMap::default()
    }};

    ($k:ty, $v:ty) => {{
        $crate::PojocMap::<$k, $v>::default()
    }};

    ($($k:expr => $v:expr),+ $(,)?) => {{
        let mut __m = $crate::PojocMap::default();
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

/// Build a tuple, converting each element with `Into`.
///
/// ```
/// use pojoc::pojtup;
///
/// let pair: (i64, i64) = pojtup!(1i32, 2i32);
/// let triple: (i64, i64, i64) = pojtup!(1i32, 2i32, 3i32);
/// assert_eq!(pair, (1, 2));
/// assert_eq!(triple, (1, 2, 3));
/// ```
#[macro_export]
macro_rules! pojtup {
    ($($x:expr),+ $(,)?) => {{
        ($(::core::convert::Into::into($x)),+)
    }};
}
