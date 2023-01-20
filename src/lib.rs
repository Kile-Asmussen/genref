#![feature(local_key_cell_methods)]
#![feature(assert_matches)]

use std::{
    mem,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

pub(crate) mod counter;
pub(crate) mod pointers;
mod tests;

use counter::*;
use pointers::*;

/// Strong, owning reference to a heap allocation.
///
/// Behaves similar to `Box` in that when dropped, the underlying allocation is
/// deallocated.
#[repr(transparent)]
pub struct Strong<T: 'static>(RawRef<T>);

#[allow(dead_code)]
impl<T: 'static> Strong<T>
{
    /// Create a new allocation.
    pub fn new(it: T) -> Self { Self::from(Box::new(it)) }

    /// Create a weak alias.
    pub fn alias(&self) -> Weak<T> { Weak(self.0) }

    /// Try to deallocate and extract the allocated value.
    ///
    /// Fails if there are any active acessor locks.
    pub fn try_take(self) -> Result<Box<T>, Self>
    {
        let gen = self.0.generation();
        if gen.try_lock_exclusive() {
            gen.bump();
            let res = unsafe { Box::from_raw(self.0.pointer().as_ptr()) };
            unsafe {
                gen.unlock_exclusive();
            }
            LocalOrGlobalGeneration::free(gen);
            std::mem::forget(self);
            Ok(res)
        } else {
            Err(self)
        }
    }

    /// Try to obtain a reading acessor lock.
    pub fn try_read(&self) -> Option<Reading<T>>
    {
        if self.0.generation().try_lock_shared() {
            Some(Reading(self.0))
        } else {
            None
        }
    }

    /// Try to obtain a writing acessor lock.
    pub fn try_write(&self) -> Option<Writing<T>>
    {
        if self.0.generation().try_lock_exclusive() {
            Some(Writing(self.0))
        } else {
            None
        }
    }

    pub(crate) fn into_raw(self) -> RawRef<T>
    {
        let mut res = self.0;
        res.ownership = OwnershipBit::Strong;
        mem::forget(self);
        res
    }

    pub(crate) unsafe fn from_raw(it: RawRef<T>) -> Self { Strong(it) }
}

#[allow(dead_code)]
impl<T: Send + Sync + 'static> Strong<T>
{
    /// Transform this reference into a sendable form.
    pub fn send(mut self) -> Sending<T>
    {
        self = self.make_sharable();
        if let RawRefEnum::Global(res) = self.0.into() {
            std::mem::forget(self);
            Sending(res)
        } else {
            panic!()
        }
    }
}

#[allow(dead_code)]
impl<T: Sync + 'static> Strong<T>
{
    pub(crate) fn make_sharable(self) -> Self
    {
        let res = Self(
            match self.0.into() {
                RawRefEnum::Local(l) => l.globalize(),
                RawRefEnum::Global(g) => g,
            }
            .into(),
        );
        mem::forget(self);
        res
    }
}

impl<T: 'static> Drop for Strong<T>
{
    fn drop(&mut self)
    {
        let gen = self.0.generation();
        gen.bump();
        if gen.try_lock_exclusive() {
            std::mem::drop(unsafe { Box::from_raw(self.0.pointer().as_ptr()) });
            unsafe { gen.unlock_exclusive() }
            LocalOrGlobalGeneration::free(gen);
        }
    }
}

impl<T> From<Sending<T>> for Strong<T>
{
    fn from(it: Sending<T>) -> Self { Strong(it.0.into()) }
}

impl<T> From<Box<T>> for Strong<T>
{
    fn from(it: Box<T>) -> Self
    {
        let genptr = LocalGeneration::new();
        Self(
            LocalRaw {
                genref: genptr.count(),
                genptr,
                boxptr: unsafe { NonNull::new_unchecked(Box::into_raw(it)) },
            }
            .into(),
        )
    }
}

/// Sendable form of a `Strong` reference.
#[repr(transparent)]
pub struct Sending<T: 'static>(GlobalRaw<T>);
unsafe impl<T: 'static + Send + Sync> Send for Sending<T> {}
impl<T: 'static> Drop for Sending<T>
{
    fn drop(&mut self) { let _ = unsafe { Strong::from_raw(self.0.into()) }; }
}

#[repr(transparent)]
pub struct Sharing<T: 'static>(GlobalRaw<T>);
unsafe impl<T: 'static + Sync> Send for Sharing<T> {}

/// Wrapper enum for transferring either strong or weak references between
/// threads.
pub enum Transferrable<T: 'static>
{
    Send(Sending<T>),
    Sync(Sharing<T>),
}

/// Weak reference to a allocation, automatically becomes invalidated when its
/// parent `Strong` reference goes out of scope.
#[repr(transparent)]
pub struct Weak<T: 'static>(RawRef<T>);
impl<T: 'static> Copy for Weak<T> {}
impl<T: 'static> Clone for Weak<T>
{
    fn clone(&self) -> Self { *self }
}

#[allow(dead_code)]
impl<T: 'static + Sync> Weak<T>
{
    /// Create a sendable copy of this reference.
    pub fn share(self) -> Sharing<T>
    {
        if let RawRefEnum::Global(g) = self.make_sharable().0.into() {
            Sharing(g)
        } else {
            panic!()
        }
    }

    pub(crate) fn make_sharable(self) -> Self
    {
        Weak(
            match self.0.into() {
                RawRefEnum::Local(l) => l.globalize(),
                RawRefEnum::Global(g) => g,
            }
            .into(),
        )
    }
}

impl<T> From<Sharing<T>> for Weak<T>
{
    fn from(it: Sharing<T>) -> Self { Weak(it.0.into()) }
}

#[allow(dead_code)]
impl<T> Weak<T>
{
    /// Attempt to obtain a reading accessor lock for the underlying allocation.
    pub fn try_read(&self) -> Option<Reading<T>>
    {
        if self.is_valid() {
            if self.0.generation().try_lock_shared() {
                if self.is_valid() {
                    return Some(Reading(self.0));
                } else {
                    unsafe { self.0.generation().unlock_shared() }
                }
            }
        }
        None
    }

    /// Attempt to obtain a writing accessor lock for the underlying allocation.
    pub fn try_write(&self) -> Option<Writing<T>>
    {
        if self.is_valid() {
            if self.0.generation().try_lock_exclusive() {
                if self.is_valid() {
                    return Some(Writing(self.0));
                } else {
                    unsafe { self.0.generation().unlock_exclusive() }
                }
            }
        }
        None
    }

    /// Check if parent `Strong` reference still exists.
    pub fn is_valid(&self) -> bool { self.0.validity() == self.0.generation().count() }

    pub(crate) fn as_raw(self) -> RawRef<T>
    {
        let mut raw = self.0;
        raw.ownership = OwnershipBit::Weak;
        raw
    }

    pub(crate) unsafe fn from_raw(it: RawRef<T>) -> Self { Weak(it) }
}

/// Reading acessor for the allocated objects. Obeys read-write lock semantics.
pub struct Reading<T: 'static>(RawRef<T>);

impl<T: 'static> Deref for Reading<T>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.0.pointer().as_ref() } }
}

impl<T: 'static> Clone for Reading<T>
{
    fn clone(&self) -> Self
    {
        if !self.0.generation().try_lock_shared() {
            panic!()
        }
        Self(self.0)
    }
}

impl<T: 'static> Drop for Reading<T>
{
    fn drop(&mut self)
    {
        let gen = self.0.generation();
        if self.0.validity() != gen.count() {
            if unsafe { gen.try_shared_into_exclusive() } {
                std::mem::drop(unsafe { Box::from_raw(self.0.pointer().as_ptr()) });
                unsafe { gen.unlock_exclusive() }
                LocalOrGlobalGeneration::free(gen);
                return;
            }
        }
        unsafe { gen.unlock_shared() }
    }
}

/// Writing acessor for the allocated objects. Obeys read-write lock semantics.
pub struct Writing<T: 'static>(RawRef<T>);

impl<T: 'static> Deref for Writing<T>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.0.pointer().as_ref() } }
}

impl<T: 'static> DerefMut for Writing<T>
{
    fn deref_mut(&mut self) -> &mut Self::Target { unsafe { self.0.pointer().as_mut() } }
}

impl<T: 'static> Drop for Writing<T>
{
    fn drop(&mut self)
    {
        let gen = self.0.generation();
        if self.0.validity() != gen.count() {
            std::mem::drop(unsafe { Box::from_raw(self.0.pointer().as_ptr()) });
            unsafe { gen.unlock_exclusive() }
            LocalOrGlobalGeneration::free(gen);
        } else {
            unsafe { gen.unlock_exclusive() }
        }
    }
}

/// A compact union of either a `Strong` or `Weak` reference.
struct Universal<T: 'static>(RawRef<T>);

impl<T: 'static> From<Strong<T>> for Universal<T>
{
    fn from(strong: Strong<T>) -> Self { Universal(strong.into_raw()) }
}

impl<T: 'static> From<Weak<T>> for Universal<T>
{
    fn from(weak: Weak<T>) -> Self { Universal(weak.as_raw()) }
}

impl<T: 'static> From<UniversalEnum<T>> for Universal<T>
{
    fn from(uni: UniversalEnum<T>) -> Self { 
        match uni {
            UniversalEnum::Strong(s) => s.into(),
            UniversalEnum::Weak(w) => w.into()
        }
    }
}

impl<T: 'static> Clone for Universal<T> {
    fn clone(&self) -> Self {
        match self.0.ownership {
            OwnershipBit::Nil => panic!(),
            OwnershipBit::Weak => Universal(self.0),
            OwnershipBit::Strong => ,
            OwnershipBit::Inferred => panic!(),
        }
    }
}

/// A matchable version of `Universal`.
pub enum UniversalEnum<T: 'static>
{
    Strong(Strong<T>),
    Weak(Weak<T>),
}

impl<T: 'static> Clone for UniversalEnum<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Strong(s) => s.alias().into(),
            Self::Weak(w) => w.into(),
        }
    }
}

impl<T: 'static> From<Universal<T>> for UniversalEnum<T>
{
    fn from(uni: Universal<T>) -> Self
    {
        match uni.0.ownership {
            OwnershipBit::Nil => panic!(),
            OwnershipBit::Weak => Self::Weak(unsafe { Weak::from_raw(uni.0) }),
            OwnershipBit::Strong => Self::Strong(unsafe { Strong::from_raw(uni.0) }),
            OwnershipBit::Inferred => panic!(),
        }
    }
}
