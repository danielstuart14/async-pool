use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use crate::Box;
use crate::Pool;

/// A structure that wraps a boxed item from a pool and provides a mapped reference to it.
///
/// This is useful when a specific part of a pooled item needs to be accessed separately.
pub struct MappedBox<P: Pool, T: 'static + ?Sized> {
    inner: UnsafeCell<Box<P>>, // Inner storage of the pooled item.
    value_ref: &'static mut T, // Mapped reference to a part of the pooled item.
}

impl<P: Pool, T: 'static + ?Sized> MappedBox<P, T> {
    /// Asynchronously creates a new `MappedBox`.
    ///
    /// # Arguments
    /// * `item` - The item to be stored in the pool.
    ///
    /// # Returns
    /// An `Option<Result<MappedBox<P, T>>>`, `None` if there is no waker available, Some(Err(_)) if `&mut P::Item` cannot be converted into `&mut T`.
    pub async fn new_async<E>(item: T) -> Option<Result<Self, E>>
    where
        T: Into<P::Item>,
        for<'a> &'a mut T: TryFrom<&'a mut P::Item, Error = E>,
    {
        if let Some(b) = Box::<P>::new_async(item.into()).await {
            Some(b.convert())
        } else {
            None
        }
    }

    /// Synchronously creates a new `MappedBox`.
    ///
    /// # Arguments
    /// * `item` - The item to be stored in the pool.
    ///
    /// # Returns
    /// An `Option<Result<MappedBox<P, T>>>`, `None` if the pool is full, Some(Err(_)) if `&mut P::Item` cannot be converted into `&mut T`.
    pub async fn new<E>(item: T) -> Option<Result<Self, E>>
    where
        T: Into<P::Item>,
        for<'a> &'a mut T: TryFrom<&'a mut P::Item, Error = E>,
    {
        if let Some(b) = Box::<P>::new(item.into()) {
            Some(b.convert())
        } else {
            None
        }
    }

    /// Returns an immutable reference to the mapped value.
    pub fn as_ref<'a>(&'a self) -> &'a T {
        &self.value_ref
    }

    /// Returns a mutable reference to the mapped value.
    pub fn as_mut<'a>(&'a mut self) -> &'a mut T {
        &mut self.value_ref
    }

    pub fn into_box(self) -> Box<P> {
        self.into()
    }
}

/// Extension trait providing mapping capabilities to `Box<P>`.
pub trait BoxExt<P: Pool, T: 'static + ?Sized>: Sized {
    /// Maps the current item into a `MappedBox` by applying a mapping function.
    ///
    /// # Arguments
    /// * `mapper` - Function that maps the pooled item to another reference.
    ///
    /// # Returns
    /// A `MappedBox<P, M>` containing the mapped reference.
    fn map<'a: 'static, M: 'a + ?Sized, F: FnOnce(&'a mut T) -> &'a mut M>(
        self,
        mapper: F,
    ) -> MappedBox<P, M> {
        let Ok(m) = self.try_map(|item| Result::<_, !>::Ok(mapper(item)));
        m
    }

    /// Tries to map the current item into a `MappedBox`, returning an error if mapping fails.
    ///
    /// # Arguments
    /// * `mapper` - Function that maps the pooled item to another reference, returning a `Result`.
    ///
    /// # Returns
    /// A `Result<MappedBox<P, M>, E>` indicating success or failure.
    fn try_map<'a: 'static, M: 'a + ?Sized, E, F: FnOnce(&'a mut T) -> Result<&'a mut M, E>>(
        self,
        mapper: F,
    ) -> Result<MappedBox<P, M>, E>;

    /// Convert back into the original [`Box`]
    fn into_box(self) -> Box<P>;

    /// Converts the current item into a `MappedBox` if the conversion is possible.
    ///
    /// # Returns
    /// A `Result<MappedBox<P, M>, E>` indicating whether the conversion succeeded.
    fn convert<'a: 'static, M: 'a, E>(self) -> Result<MappedBox<P, M>, E>
    where
        &'a mut M: TryFrom<&'a mut T, Error = E>,
    {
        self.try_map(|i| i.try_into())
    }
}

impl<P: Pool> BoxExt<P, P::Item> for Box<P> {
    fn try_map<
        'a: 'static,
        M: 'a + ?Sized,
        E,
        F: FnOnce(&'a mut P::Item) -> Result<&'a mut M, E>,
    >(
        self,
        mapper: F,
    ) -> Result<MappedBox<P, M>, E> {
        let inner = UnsafeCell::new(self);
        Ok(MappedBox {
            value_ref: unsafe { mapper(NonNull::new_unchecked(inner.get()).as_mut())? },
            inner,
        })
    }

    fn into_box(self) -> Box<P> {
        self
    }
}

impl<P: Pool, T> BoxExt<P, T> for MappedBox<P, T> {
    fn try_map<'a: 'static, M: 'a + ?Sized, E, F: FnOnce(&'a mut T) -> Result<&'a mut M, E>>(
        self,
        mapper: F,
    ) -> Result<MappedBox<P, M>, E> {
        Ok(MappedBox {
            value_ref: mapper(self.value_ref)?,
            inner: self.inner,
        })
    }

    fn into_box(self) -> Box<P> {
        self.inner.into_inner()
    }
}

/// Allows conversion of `MappedBox` into its original boxed form.
impl<P: Pool, T: ?Sized> Into<Box<P>> for MappedBox<P, T> {
    fn into(self) -> Box<P> {
        self.inner.into_inner()
    }
}

/// Implements `Deref` for `MappedBox`, allowing access to the mapped value.
impl<P: Pool, T: 'static + ?Sized> Deref for MappedBox<P, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value_ref
    }
}

/// Implements `DerefMut` for `MappedBox`, allowing mutable access to the mapped value.
impl<P: Pool, T: 'static + ?Sized> DerefMut for MappedBox<P, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value_ref
    }
}

impl<P: Pool, T: 'static + ?Sized> AsRef<T> for MappedBox<P, T> {
    fn as_ref(&self) -> &T {
        &self.value_ref
    }
}
impl<P: Pool, T: 'static + ?Sized> AsMut<T> for MappedBox<P, T> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.value_ref
    }
}

#[cfg(test)]
mod tests {
    use crate::pool;

    use super::*;

    #[derive(Debug)]
    enum AB {
        A(u32),
        B(i32),
    }

    impl<'a> TryFrom<&'a mut AB> for &'a mut u32 {
        type Error = ();

        fn try_from(value: &'a mut AB) -> Result<Self, Self::Error> {
            match value {
                AB::A(u) => Ok(u),
                _ => Err(()),
            }
        }
    }
    impl<'a> TryFrom<&'a mut AB> for &'a mut i32 {
        type Error = ();

        fn try_from(value: &'a mut AB) -> Result<Self, Self::Error> {
            match value {
                AB::B(i) => Ok(i),
                _ => Err(()),
            }
        }
    }

    impl From<u32> for AB {
        fn from(value: u32) -> Self {
            AB::A(value)
        }
    }
    impl From<i32> for AB {
        fn from(value: i32) -> Self {
            AB::B(value)
        }
    }

    pool!(TestPool: [AB; 4], 3);

    #[tokio::test]
    async fn mapped() {
        let a = MappedBox::<TestPool, u32>::new_async(42u32)
            .await
            .expect("slot")
            .expect("conversion");
        assert_eq!(*a, 42u32);
        let b: MappedBox<TestPool, i32> = a.into_box().map::<i32, _>(|item| {
            *item = AB::B(-1);
            item.try_into().unwrap()
        });
        assert_eq!(*b, -1);
    }

    pool!(TestBufs: [[u8; 64]; 2], 3);

    #[tokio::test]
    async fn map_to_slice() {
        let mut array = Box::<TestBufs>::new_async([0u8; 64]).await.expect("slot");
        // write some bytes
        array[4..32].copy_from_slice(&[42u8; 28]);
        let slice = array.map(|array| &mut array[4..32]);
        assert_eq!(slice.as_ref(), &[42u8; 28]);
    }
}
