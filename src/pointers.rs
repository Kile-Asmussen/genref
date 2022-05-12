use crate::allocator::{free_and_take_unchecked, free_unchecked};

use super::{
    allocator::{
        allocate, free, guard_no_longer_in_use, guard_now_in_use, guards_exist, try_free_and_take,
    },
    generations::InUsePtr,
};
use std::{
    // any::type_name,
    fmt,
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
        //dbg_call!("Owned::<{}>::new()", type_name::<T>());
        //dbg_return!("{:?}",
        Uniq::new(it).decay()
        //)
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
    /// Fails if there are live `Guard`s.
    pub fn try_into_inner(self) -> Result<T, Self>
    {
        if let Some(it) = try_free_and_take(self.ptr) {
            std::mem::forget(self);
            Ok(it)
        } else {
            Err(self)
        }
    }

    /// Attempt to ensure uniqueness of this reference by invalidating all
    /// `Weak` references.
    ///
    /// Fails if there are live `Guard`s.
    ///
    /// Also avalable as `TryFrom<Owned<T>>` on `Uniq`.
    pub fn refine(self) -> Result<Uniq<T>, Self>
    {
        if guards_exist() {
            Err(self)
        } else {
            self.ptr.invalidate_weaks();
            let ptr = self.ptr;
            std::mem::forget(self);
            Ok(Uniq { ptr })
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
    fn drop(&mut self)
    {
        //dbg_call!("Owned::<{}>.drop()", type_name::<T>());
        unsafe {
            free(self.ptr);
        }
        //dbg_return!();
    }
}

impl<T: 'static> fmt::Debug for Owned<T>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        f.debug_struct("Owned").field("ptr", &self.ptr).finish()
    }
}

/// A strongly unique reference to an allocated object.
///
/// `Uniq` is the _only_ way to to transfer generational references from
/// one thread to another.
#[repr(transparent)]
pub struct Uniq<T: 'static>
{
    ptr: InUsePtr<T>,
}
unsafe impl<T: 'static> Send for Uniq<T> {}

#[allow(dead_code)]
impl<T: 'static> Uniq<T>
{
    /// Allocate a new object on the managed heap. A wrapper for `Object::new`.
    pub fn new(it: T) -> Self
    {
        //dbg_call!("Uniq::<{}>::new(_)", type_name::<T>());
        let res = if let Some(fp) = allocate::<T>() {
            Self {
                ptr: unsafe { fp.downcast(it) },
            }
        } else {
            Self {
                ptr: InUsePtr::allocate(it),
            }
        };
        //dbg_return!("{:?}", res);
        res
    }

    /// Remove uniqueness status of the reference to allow aliasing.
    ///
    /// Also available as `From<Uniq<T>>` for `Owned`.
    pub fn decay(self) -> Owned<T>
    {
        let ptr = self.ptr;
        std::mem::forget(self);
        Owned { ptr }
    }

    /// Free allocation and return data content
    ///
    /// Cannot fail since there are no weak references
    pub fn into_inner(self) -> T
    {
        let ptr = self.ptr;
        std::mem::forget(self);
        unsafe { free_and_take_unchecked(ptr) }
    }
}

impl<T: 'static> TryFrom<Owned<T>> for Uniq<T>
{
    type Error = Owned<T>;

    fn try_from(value: Owned<T>) -> Result<Self, Self::Error> { value.refine() }
}

impl<T: 'static> Deref for Uniq<T>
{
    type Target = <Owned<T> as Deref>::Target;

    fn deref(&self) -> &Self::Target { unsafe { self.ptr.data_ref() } }
}

impl<T: 'static> DerefMut for Uniq<T>
{
    fn deref_mut(&mut self) -> &mut Self::Target { unsafe { self.ptr.data_mut() } }
}

impl<T: 'static> Drop for Uniq<T>
{
    fn drop(&mut self)
    {
        //dbg_call!("Uniq::<{}>.drop()", type_name::<T>());
        unsafe { free_unchecked(self.ptr) }
        //dbg_return!();
    }
}

impl<T: 'static> fmt::Debug for Uniq<T>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        f.debug_struct("Uniq").field("ptr", &self.ptr).finish()
    }
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
#[derive(Clone, Copy)]
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
        guard_now_in_use();
        if self.gen.get() == self.ptr.generation() {
            Some(Guard {
                ptr: self.ptr,
                _phantom: PhantomData,
            })
        } else {
            guard_no_longer_in_use();
            None
        }
    }
}

impl<T: 'static> fmt::Debug for Weak<T>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        f.debug_struct("Weak")
            .field("ptr", &self.ptr)
            .field("gen", &self.gen)
            .finish()
    }
}

/// An actual reference obtained through a `Weak` reference.
///
/// Prevents _any_ allocations owned by the local thread from
/// invalidating weak references.
#[repr(transparent)]
pub struct Guard<'a, T: 'static>
{
    ptr: InUsePtr<T>,
    _phantom: PhantomData<&'a ()>,
}

impl<'a, T: 'static> Clone for Guard<'a, T>
{
    fn clone(&self) -> Self
    {
        guard_now_in_use();
        Guard {
            ptr: self.ptr,
            _phantom: PhantomData,
        }
    }
}

impl<'a, T: 'static> Drop for Guard<'a, T>
{
    fn drop(&mut self) { guard_no_longer_in_use() }
}

impl<'a, T: 'static> Deref for Guard<'a, T>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.ptr.data_ref() } }
}

impl<'a, T: 'static> fmt::Debug for Guard<'a, T>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        f.debug_struct("Guard").field("ptr", &self.ptr).finish()
    }
}
