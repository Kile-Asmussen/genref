use super::{
    allocator::{
        deregister_guard, free, guards_exist, reallocate, register_guard, try_free_and_take,
    },
    generations::InUsePtr,
};
use std::{
    marker::PhantomData,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
};

/// Owned allocation.
///
/// This is the aliasable handle for a concrete allocated object. Has similar
/// semantics to `Box` except in the case where there are active `Guard`s in the
/// thread, in which case `drop` of the object will only run when there is
/// nothing that can possibly access it.
///
/// Upon deallocation, the generation counter of the underlying allocation is
/// incremented, invalidating all `Weak` references.

#[repr(transparent)]
#[derive(Debug)]

pub struct Owned<T: 'static>
{
    ptr: InUsePtr<T>,
}

#[allow(dead_code)]
impl<T: 'static> Owned<T>
{
    /// Allocate an object on the managed heap. Attempts to claim
    /// a free object of appropriate layout from the heap, allocates new
    /// if there is none available.
    pub fn new(it: T) -> Self
    {
        if let Some(fp) = reallocate::<T>() {
            Owned {
                ptr: unsafe { fp.downcast(it) },
            }
        } else {
            Owned {
                ptr: InUsePtr::allocate(it),
            }
        }
    }

    /// Produce a weak alias.
    pub fn alias(&self) -> Weak<T>
    {
        Weak {
            ptr: self.ptr,
            gen: unsafe { NonZeroUsize::new_unchecked(self.ptr.generation()) },
        }
    }

    /// Attempt to free the underlying allocation and return the allocated
    /// object rather than dropping it.
    ///
    /// Fails if there are active `Guard`s.
    pub fn try_take(self) -> Result<T, Self> { try_free_and_take(self.ptr).ok_or(self) }

    /// Attempt to ensure uniqueness of this reference by invalidating all
    /// `Weak` references.
    ///
    /// Fails if there are active `Guard`s.
    ///
    /// Also avalable as `TryFrom<Owned<T>>` on `Uniq`.
    pub fn refine(self) -> Result<Uniq<T>, Self>
    {
        if guards_exist() {
            Err(self)
        } else {
            self.ptr.invalidate_weak();
            Ok(Uniq(self))
        }
    }
}

impl<T: 'static> Deref for Owned<T>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.ptr.data_ref() } }
}

impl<T: 'static> From<Uniq<T>> for Owned<T>
{
    fn from(it: Uniq<T>) -> Owned<T> { it.decay() }
}

impl<T: 'static> Drop for Owned<T>
{
    fn drop(&mut self) { free(self.ptr) }
}

/// A strongly unique reference to an allocated object.
///
/// `Uniq` is the _only_ way to to transfer generational references from
/// one thread to another.
#[repr(transparent)]
pub struct Uniq<T: 'static>(Owned<T>);
unsafe impl<T: 'static> Send for Uniq<T> {}

#[allow(dead_code)]
impl<T: 'static> Uniq<T>
{
    /// Allocate a new object on the managed heap. A wrapper for `Object::new`.
    pub fn new(it: T) -> Self { Uniq(Owned::new(it)) }

    /// Remove uniqueness status of the reference to allow aliasing.
    ///
    /// Also available as `From<Uniq<T>>` for `Owned`.
    pub fn decay(self) -> Owned<T> { self.0 }
}

impl<T: 'static> TryFrom<Owned<T>> for Uniq<T>
{
    type Error = Owned<T>;

    fn try_from(value: Owned<T>) -> Result<Self, Self::Error> { value.refine() }
}

impl<T: 'static> Deref for Uniq<T>
{
    type Target = <Owned<T> as Deref>::Target;

    fn deref(&self) -> &Self::Target { self.0.deref() }
}

impl<T: 'static> DerefMut for Uniq<T>
{
    fn deref_mut(&mut self) -> &mut Self::Target { unsafe { self.0.ptr.data_mut() } }
}

/// Weak reference to an allocation.
///
/// This reference type carries both a pointer and a local copy of the
/// allocation's generation counter. When acessing the underlying allocation,
/// the local generation count is compared to the allocation's generation. Only
/// if the two match, is the reference still valid.
///
/// Compared to `Rc`, this type is `Copy`, under the assumption that references
/// are copied more often than they are dereferenced.
#[derive(Clone, Copy, Debug)]
pub struct Weak<T: 'static>
{
    ptr: InUsePtr<T>,
    gen: NonZeroUsize,
}

#[allow(dead_code)]
impl<T: 'static> Weak<T>
{
    /// Attempt to reference the underlying allocated data.
    ///
    /// Returns `None` if the reference is no longer valid.
    pub fn try_ref(&self) -> Option<Guard<T>>
    {
        if self.gen.get() == self.ptr.generation() {
            register_guard();
            Some(Guard {
                ptr: self.ptr,
                _phantom: PhantomData,
            })
        } else {
            None
        }
    }
}

/// An actual reference obtained through a `Weak` reference.
///
/// Prevents _any_ allocations owned by the local thread from
/// invalidating weak references.
#[repr(transparent)]
#[derive(Debug)]
pub struct Guard<'a, T: 'static>
{
    ptr: InUsePtr<T>,
    _phantom: PhantomData<&'a ()>,
}

impl<'a, T: 'static> Clone for Guard<'a, T>
{
    fn clone(&self) -> Self
    {
        register_guard();
        Guard {
            ptr: self.ptr,
            _phantom: PhantomData,
        }
    }
}

impl<'a, T: 'static> Drop for Guard<'a, T>
{
    fn drop(&mut self) { deregister_guard() }
}

impl<'a, T: 'static> Deref for Guard<'a, T>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.ptr.data_ref() } }
}
