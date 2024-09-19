//! Statically allocated pool providing a std-like Box,
//! allowing to asynchronously await for a pool slot to become available.
//!
//! It is tailored to be used with no-std async runtimes, like [Embassy](https://embassy.dev/), but
//! can also be used in std environments (check examples).
//!
//! The most common use-case is sharing large memory regions on constrained
//! devices (e.g. microcontrollers), where multiple tasks may need to use the
//! memory for buffering an I/O or performing calculations, and having
//! separate static buffers would be too costly.
//!
//! It is important to know that waiting forever for a memory slot to be
//! available may dead-lock your code if done wrong. With that in mind,
//! you should consider using a timeout when allocating asynchronously (e.g. [embassy_time::with_timeout](https://docs.rs/embassy-time/0.3.2/embassy_time/fn.with_timeout.html)).
//!
//! #### Dependencies
//!
//! This crate requires a critical section implementation. Check [critical-section](https://crates.io/crates/critical-section).
//!
//! #### Example
//!
//! ```
//! use async_pool::{pool, Box};
//!
//!struct Buffer([u8; 256]);
//!
//!// A maximum of 2 Packet instances can be allocated at a time.
//!// A maximum of 3 futures can be waiting at a time.
//!pool!(BufferPool: [Buffer; 2], 3);
//!
//!async fn run() {
//!    // Allocate non-blocking (will return None if no data slot is available)
//!    let box1 = Box::<BufferPool>::new(Buffer([0; 256]));
//!
//!    // Allocate asynchronously (will wait if no data slot is available)
//!    // This can return None if all future slots are taken
//!    let box2 = Box::<BufferPool>::new_async(Buffer([0; 256])).await;
//!}
//! ```
#![cfg_attr(not(test), no_std)]

mod atomic_bitset;

use core::cell::UnsafeCell;
use core::future::{poll_fn, Future};
use core::hash::{Hash, Hasher};
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use core::task::Poll;
use core::{cmp, mem, ptr::NonNull};
use embassy_sync::waitqueue::AtomicWaker;
use portable_atomic::AtomicU32;

use crate::atomic_bitset::AtomicBitset;

/// Implementation detail. Not covered by semver guarantees.
#[doc(hidden)]
pub trait PoolStorage<T> {
    fn alloc(&self) -> Option<NonNull<T>>;
    fn alloc_async(&self) -> impl Future<Output = Option<NonNull<T>>>;
    unsafe fn free(&self, p: NonNull<T>);
}

/// Implementation detail. Not covered by semver guarantees.
#[doc(hidden)]
pub struct PoolStorageImpl<T, const N: usize, const K: usize, const WN: usize, const WK: usize>
where
    [AtomicU32; K]: Sized,
    [AtomicU32; WK]: Sized,
{
    used: AtomicBitset<N, K>,
    data: [UnsafeCell<MaybeUninit<T>>; N],
    wakers_used: AtomicBitset<WN, WK>,
    wakers: [AtomicWaker; WN],
}

unsafe impl<T, const N: usize, const K: usize, const WN: usize, const WK: usize> Send
    for PoolStorageImpl<T, N, K, WN, WK>
{
}
unsafe impl<T, const N: usize, const K: usize, const WN: usize, const WK: usize> Sync
    for PoolStorageImpl<T, N, K, WN, WK>
{
}

impl<T, const N: usize, const K: usize, const WN: usize, const WK: usize>
    PoolStorageImpl<T, N, K, WN, WK>
where
    [AtomicU32; K]: Sized,
    [AtomicU32; WK]: Sized,
{
    const UNINIT: UnsafeCell<MaybeUninit<T>> = UnsafeCell::new(MaybeUninit::uninit());

    const WAKER: AtomicWaker = AtomicWaker::new();

    pub const fn new() -> Self {
        Self {
            used: AtomicBitset::new(),
            data: [Self::UNINIT; N],
            wakers_used: AtomicBitset::new(),
            wakers: [Self::WAKER; WN],
        }
    }
}

impl<T, const N: usize, const K: usize, const WN: usize, const WK: usize> PoolStorage<T>
    for PoolStorageImpl<T, N, K, WN, WK>
where
    [AtomicU32; K]: Sized,
    [AtomicU32; WK]: Sized,
{
    /// Returns an item from the data pool, if available.
    /// Returns None if the data pool is full.
    fn alloc(&self) -> Option<NonNull<T>> {
        let n = self.used.alloc()?;
        let p = self.data[n].get() as *mut T;
        Some(unsafe { NonNull::new_unchecked(p) })
    }

    /// Wait until an item is available in the data pool, then return it.
    /// Returns None if the waker pool is full.
    fn alloc_async(&self) -> impl Future<Output = Option<NonNull<T>>> {
        let mut waker_slot = None;
        poll_fn(move |cx| {
            // Check if there is a free slot in the data pool
            if let Some(n) = self.used.alloc() {
                let p = self.data[n].get() as *mut T;
                return Poll::Ready(Some(unsafe { NonNull::new_unchecked(p) }));
            }

            // Try to allocate a waker slot if necessary
            if waker_slot.is_none() {
                waker_slot = self.wakers_used.alloc_droppable();
            }

            match &waker_slot {
                Some(bit) => {
                    self.wakers[bit.inner()].register(cx.waker());
                    Poll::Pending
                }
                None => Poll::Ready(None), // No waker slots available
            }
        })
    }

    /// safety: p must be a pointer obtained from self.alloc that hasn't been freed yet.
    unsafe fn free(&self, p: NonNull<T>) {
        let origin = self.data.as_ptr() as *mut T;
        let n = p.as_ptr().offset_from(origin);
        assert!(n >= 0);
        assert!((n as usize) < N);
        self.used.free(n as usize);

        // Wake up any wakers waiting for a slot
        for waker in self.wakers.iter() {
            waker.wake();
        }
    }
}

pub trait Pool: 'static {
    type Item: 'static;

    /// Implementation detail. Not covered by semver guarantees.
    #[doc(hidden)]
    type Storage: PoolStorage<Self::Item>;

    /// Implementation detail. Not covered by semver guarantees.
    #[doc(hidden)]
    fn get() -> &'static Self::Storage;
}

pub struct Box<P: Pool> {
    ptr: NonNull<P::Item>,
}

impl<P: Pool> Box<P> {
    /// Returns an item from the data pool, if available.
    /// Returns None if the data pool is full.
    pub fn new(item: P::Item) -> Option<Self> {
        let p = match P::get().alloc() {
            Some(p) => p,
            None => return None,
        };
        unsafe { p.as_ptr().write(item) };
        Some(Self { ptr: p })
    }

    /// Wait until an item is available in the data pool, then return it.
    /// Returns None if the waker pool is full.
    pub async fn new_async(item: P::Item) -> Option<Self> {
        let p = match P::get().alloc_async().await {
            Some(p) => p,
            None => return None,
        };
        unsafe { p.as_ptr().write(item) };
        Some(Self { ptr: p })
    }

    pub fn into_raw(b: Self) -> NonNull<P::Item> {
        let res = b.ptr;
        mem::forget(b);
        res
    }

    pub unsafe fn from_raw(ptr: NonNull<P::Item>) -> Self {
        Self { ptr }
    }
}

impl<P: Pool> Drop for Box<P> {
    fn drop(&mut self) {
        unsafe {
            //trace!("dropping {:u32}", self.ptr as u32);
            self.ptr.as_ptr().drop_in_place();
            P::get().free(self.ptr);
        };
    }
}

unsafe impl<P: Pool> Send for Box<P> where P::Item: Send {}

unsafe impl<P: Pool> Sync for Box<P> where P::Item: Sync {}

unsafe impl<P: Pool> stable_deref_trait::StableDeref for Box<P> {}

impl<P: Pool> as_slice_01::AsSlice for Box<P>
where
    P::Item: as_slice_01::AsSlice,
{
    type Element = <P::Item as as_slice_01::AsSlice>::Element;

    fn as_slice(&self) -> &[Self::Element] {
        self.deref().as_slice()
    }
}

impl<P: Pool> as_slice_01::AsMutSlice for Box<P>
where
    P::Item: as_slice_01::AsMutSlice,
{
    fn as_mut_slice(&mut self) -> &mut [Self::Element] {
        self.deref_mut().as_mut_slice()
    }
}

impl<P: Pool> as_slice_02::AsSlice for Box<P>
where
    P::Item: as_slice_02::AsSlice,
{
    type Element = <P::Item as as_slice_02::AsSlice>::Element;

    fn as_slice(&self) -> &[Self::Element] {
        self.deref().as_slice()
    }
}

impl<P: Pool> as_slice_02::AsMutSlice for Box<P>
where
    P::Item: as_slice_02::AsMutSlice,
{
    fn as_mut_slice(&mut self) -> &mut [Self::Element] {
        self.deref_mut().as_mut_slice()
    }
}

impl<P: Pool> Deref for Box<P> {
    type Target = P::Item;

    fn deref(&self) -> &P::Item {
        unsafe { self.ptr.as_ref() }
    }
}

impl<P: Pool> DerefMut for Box<P> {
    fn deref_mut(&mut self) -> &mut P::Item {
        unsafe { self.ptr.as_mut() }
    }
}

impl<P: Pool> core::fmt::Debug for Box<P>
where
    P::Item: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        <P::Item as core::fmt::Debug>::fmt(self, f)
    }
}

impl<P: Pool> core::fmt::Display for Box<P>
where
    P::Item: core::fmt::Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        <P::Item as core::fmt::Display>::fmt(self, f)
    }
}

impl<P: Pool> PartialEq for Box<P>
where
    P::Item: PartialEq,
{
    fn eq(&self, rhs: &Box<P>) -> bool {
        <P::Item as PartialEq>::eq(self, rhs)
    }
}

impl<P: Pool> Eq for Box<P> where P::Item: Eq {}

impl<P: Pool> PartialOrd for Box<P>
where
    P::Item: PartialOrd,
{
    fn partial_cmp(&self, rhs: &Box<P>) -> Option<cmp::Ordering> {
        <P::Item as PartialOrd>::partial_cmp(self, rhs)
    }
}

impl<P: Pool> Ord for Box<P>
where
    P::Item: Ord,
{
    fn cmp(&self, rhs: &Box<P>) -> cmp::Ordering {
        <P::Item as Ord>::cmp(self, rhs)
    }
}

impl<P: Pool> Hash for Box<P>
where
    P::Item: Hash,
{
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        <P::Item as Hash>::hash(self, state)
    }
}

/// Create a item pool of a given type and size, as well as a waker pool of a given length.
///
/// The waker pool is used to wake up tasks waiting for an item to become available in the data pool.
/// Its length should be at least the number of tasks that can be waiting for an item at the same time.
/// Example:
/// ```
/// use async_pool::{pool, Box};
///
/// #[derive(Debug)]
/// #[allow(dead_code)]
/// struct Packet(u32);
///
/// pool!(PacketPool: [Packet; 4], 2); // Item pool of 4 Packet instances, waker pool of 2 wakers
/// ```
#[macro_export]
macro_rules! pool {
    ($vis:vis $name:ident: [$ty:ty; $n:expr], $wn:expr) => {
        $vis struct $name { _uninhabited: ::core::convert::Infallible }
        impl $crate::Pool for $name {
            type Item = $ty;
            type Storage = $crate::PoolStorageImpl<$ty, {$n}, {($n+31)/32}, {$wn}, {($wn+31)/32}>;
            fn get() -> &'static Self::Storage {
                static POOL: $crate::PoolStorageImpl<$ty, {$n}, {($n+31)/32}, {$wn}, {($wn+31)/32}> = $crate::PoolStorageImpl::new();
                &POOL
            }
        }
    };
}

#[cfg(test)]
mod test {
    use embassy_futures::yield_now;
    use tokio::{join, select};

    use super::*;
    use core::mem;

    pool!(TestPool: [u32; 4], 0);
    pool!(TestPool2: [u32; 4], 1);

    #[test]
    fn test_pool() {
        let b1 = Box::<TestPool>::new(111).unwrap();
        let b2 = Box::<TestPool>::new(222).unwrap();
        let b3 = Box::<TestPool>::new(333).unwrap();
        let b4 = Box::<TestPool>::new(444).unwrap();
        assert!(Box::<TestPool>::new(555).is_none());
        assert_eq!(*b1, 111);
        assert_eq!(*b2, 222);
        assert_eq!(*b3, 333);
        assert_eq!(*b4, 444);
        mem::drop(b3);
        let b5 = Box::<TestPool>::new(555).unwrap();
        assert!(Box::<TestPool>::new(666).is_none());
        assert_eq!(*b1, 111);
        assert_eq!(*b2, 222);
        assert_eq!(*b4, 444);
        assert_eq!(*b5, 555);
    }

    #[test]
    fn test_async_sizes() {
        let pool1 = <TestPool as Pool>::get();
        let pool2 = <TestPool2 as Pool>::get();
        assert!(mem::size_of_val(pool1) < mem::size_of_val(pool2));
    }

    #[tokio::test]
    async fn empty_async_pool() {
        let b1 = Box::<TestPool>::new(111).unwrap();
        let b2 = Box::<TestPool>::new_async(222).await.unwrap();
        let b3 = Box::<TestPool>::new(333).unwrap();
        let b4 = Box::<TestPool>::new_async(444).await.unwrap();
        assert!(Box::<TestPool>::new_async(555).await.is_none());
        assert_eq!(*b3, 333);
        mem::drop(b3);
        let b5 = Box::<TestPool>::new_async(555).await.unwrap();
        assert!(Box::<TestPool>::new_async(666).await.is_none());
        assert_eq!(*b1, 111);
        assert_eq!(*b2, 222);
        assert_eq!(*b4, 444);
        assert_eq!(*b5, 555);
    }

    #[tokio::test]
    async fn cancelled_future() {
        let b1 = Box::<TestPool2>::new_async(111).await.unwrap();
        let b2 = Box::<TestPool2>::new(222).unwrap();
        let b3 = Box::<TestPool2>::new_async(333).await.unwrap();
        let b4 = Box::<TestPool2>::new(444).unwrap();
        assert_eq!(*b1, 111);
        assert_eq!(*b2, 222);
        assert_eq!(*b3, 333);
        assert_eq!(*b4, 444);

        let fut1 = async {
            yield_now().await;
            yield_now().await;
        };

        let fut2 = async { Box::<TestPool2>::new_async(555).await.unwrap() };

        select! {
            _ = fut1 => {},
            v = fut2 => panic!("Future should have been cancelled: {:?}", v),
        }

        let fut3 = async {
            yield_now().await;
            yield_now().await;
            mem::drop(b1);
        };

        let fut4 = Box::<TestPool2>::new_async(666);

        let (b6, _) = join!(fut4, fut3);
        assert_eq!(*b6.unwrap(), 666);
    }
}
