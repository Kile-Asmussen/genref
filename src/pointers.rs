


use super::{
    generations::InUsePtr,
    allocator::{register_guard, deregister_guard, reallocate, guards_exist, try_free_and_take, free},
};
use std::{num::NonZeroUsize, ops::{Deref, DerefMut}, marker::PhantomData};

#[repr(transparent)]
#[derive(Debug)]

pub struct Owned<T: 'static>
{
    ptr: InUsePtr<T>,
}

#[allow(dead_code)]
impl<T: 'static> Owned<T>
{
    pub fn new(it: T) -> Self
    {
        if let Some(fp) = reallocate::<T>() {
            Owned { ptr: unsafe { fp.downcast(it) } }
        } else {
            Owned { ptr: InUsePtr::allocate(it) }
        }
    }

    pub fn alias(&self) -> Weak<T> {
        Weak {
            ptr: self.ptr,
            gen: unsafe { NonZeroUsize::new_unchecked(self.ptr.generation()) }
        } 
    }

    pub fn try_take(self) -> Result<T, Self> {
        try_free_and_take(self.ptr).ok_or(self)
    }

    pub fn refine(self) -> Result<Uniq<T>, Self> {
        if guards_exist() {
            Err(self)
        } else {
            self.ptr.invalidate_weak();
            Ok(Uniq(self))
        }
    }
}

impl<T:'static> Deref for Owned<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.data_ref() }
    }
}

impl<T:'static> From<Uniq<T>> for Owned<T> {
    fn from(it: Uniq<T>) -> Owned<T> {
        it.decay()
    }
}

impl<T:'static> Drop for Owned<T>
{
    fn drop(&mut self) {
        free(self.ptr)
    }
}

pub struct Uniq<T:'static>(Owned<T>);
unsafe impl<T:'static> Send for Uniq<T> {}

#[allow(dead_code)]
impl<T:'static> Uniq<T> {
    pub fn new(it: T) -> Self {
        Uniq(Owned::new(it))
    }

    pub fn decay(self) -> Owned<T> {
        self.0
    }
}

impl<T:'static> TryFrom<Owned<T>> for Uniq<T> {
    type Error = Owned<T>;

    fn try_from(value: Owned<T>) -> Result<Self, Self::Error> {
        value.refine()
    }
}

impl<T:'static> Deref for Uniq<T> {
    type Target = <Owned<T> as Deref>::Target;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T:'static> DerefMut for Uniq<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.ptr.data_mut() }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Weak<T: 'static>
{
    ptr: InUsePtr<T>,
    gen: NonZeroUsize,
}

#[allow(dead_code)]
impl<T: 'static> Weak<T> {
    pub fn try_ref(&self) -> Option<Guard<T>> {
        if self.gen.get() == self.ptr.generation() {
            register_guard();
            Some(Guard { ptr: self.ptr, _phantom: PhantomData })
        } else {
            None
        }
    }
}

#[repr(transparent)]
#[derive(Debug)]
pub struct Guard<'a, T: 'static>
{
    ptr: InUsePtr<T>,
    _phantom: PhantomData<&'a ()>
}

impl<'a, T:'static> Clone for Guard<'a, T> {
    fn clone(&self) -> Self {
        register_guard();
        Guard { ptr: self.ptr, _phantom: PhantomData }
    }
}

impl<'a, T:'static> Drop for Guard<'a, T> {
    fn drop(&mut self) {
        deregister_guard()
    }
}

impl<'a, T:'static> Deref for Guard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.data_ref() }
    }
}