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

#[repr(transparent)]
pub struct Strong<T: 'static>(RawRef<T>);

#[allow(dead_code)]
impl<T: 'static> Strong<T> {
    pub fn new(it: T) -> Self {
        Self::from(Box::new(it))
    }

    pub fn alias(&self) -> Weak<T> {
        Weak(self.0)
    }

    pub fn try_take(self) -> Result<Box<T>, Self> {
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

    fn try_read(&self) -> Option<Reading<T>> {
        if self.0.generation().try_lock_shared() {
            Some(Reading(self.0))
        } else {
            None
        }
    }

    fn try_write(&self) -> Option<Writing<T>> {
        if self.0.generation().try_lock_exclusive() {
            Some(Writing(self.0))
        } else {
            None
        }
    }

    pub(crate) fn into_raw(self) -> RawRef<T> {
        let mut res = self.0;
        res.ownership = OwnershipBit::Strong;
        mem::forget(self);
        res
    }

    pub(crate) unsafe fn from_raw(it: RawRef<T>) -> Self {
        Strong(it)
    }
}

#[allow(dead_code)]
impl<T: Send + Sync + 'static> Strong<T> {
    pub fn send(mut self) -> Sending<T> {
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
impl<T: Sync + 'static> Strong<T> {
    pub fn make_sharable(self) -> Self {
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

impl<T: 'static> Drop for Strong<T> {
    fn drop(&mut self) {
        let gen = self.0.generation();
        gen.bump();
        if gen.try_lock_exclusive() {
            std::mem::drop(unsafe { Box::from_raw(self.0.pointer().as_ptr()) });
            unsafe { gen.unlock_exclusive() }
            LocalOrGlobalGeneration::free(gen);
        }
    }
}

impl<T> From<Sending<T>> for Strong<T> {
    fn from(it: Sending<T>) -> Self {
        Strong(it.0.into())
    }
}

impl<T> From<Box<T>> for Strong<T> {
    fn from(it: Box<T>) -> Self {
        Self(
            LocalRaw {
                genref: COUNTER_INIT,
                genptr: LocalGeneration::new(),
                boxptr: unsafe { NonNull::new_unchecked(Box::into_raw(it)) },
            }
            .into(),
        )
    }
}

#[repr(transparent)]
pub struct Sending<T: 'static>(GlobalRaw<T>);
unsafe impl<T: 'static + Send + Sync> Send for Sending<T> {}
impl<T: 'static> Drop for Sending<T> {
    fn drop(&mut self) {
        let _ = unsafe { Strong::from_raw(self.0.into()) };
    }
}

#[repr(transparent)]
pub struct Sharing<T: 'static>(GlobalRaw<T>);
unsafe impl<T: 'static + Sync> Send for Sharing<T> {}

pub enum Transferrable<T: 'static> {
    Send(Sending<T>),
    Sync(Sharing<T>),
}

#[repr(transparent)]
pub struct Weak<T: 'static>(RawRef<T>);
impl<T: 'static> Copy for Weak<T> {}
impl<T: 'static> Clone for Weak<T> {
    fn clone(&self) -> Self {
        *self
    }
}

#[allow(dead_code)]
impl<T: 'static + Sync> Weak<T> {
    pub fn share(self) -> Sharing<T> {
        if let RawRefEnum::Global(g) = self.make_sharable().0.into() {
            Sharing(g)
        } else {
            panic!()
        }
    }

    pub fn make_sharable(self) -> Self {
        Weak(
            match self.0.into() {
                RawRefEnum::Local(l) => l.globalize(),
                RawRefEnum::Global(g) => g,
            }
            .into(),
        )
    }
}

impl<T> From<Sharing<T>> for Weak<T> {
    fn from(it: Sharing<T>) -> Self {
        Weak(it.0.into())
    }
}

#[allow(dead_code)]
impl<T> Weak<T> {
    fn try_read(&self) -> Option<Reading<T>> {
        let gen = self.0.generation();
        if self.0.validity() == gen.count() {
            if self.0.generation().try_lock_shared() {
                return Some(Reading(self.0));
            }
        }
        None
    }

    fn try_write(&self) -> Option<Writing<T>> {
        let gen = self.0.generation();
        if self.0.validity() == gen.count() {
            if self.0.generation().try_lock_exclusive() {
                return Some(Writing(self.0));
            }
        }
        None
    }

    pub(crate) fn as_raw(self) -> RawRef<T> {
        let mut raw = self.0;
        raw.ownership = OwnershipBit::Weak;
        raw
    }

    pub(crate) unsafe fn from_raw(it: RawRef<T>) -> Self {
        Weak(it)
    }
}

pub struct Reading<T: 'static>(RawRef<T>);

impl<T: 'static> Deref for Reading<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.0.pointer().as_ref() }
    }
}

impl<T: 'static> Clone for Reading<T> {
    fn clone(&self) -> Self {
        if !self.0.generation().try_lock_shared() {
            panic!()
        }
        Self(self.0)
    }
}

impl<T: 'static> Drop for Reading<T> {
    fn drop(&mut self) {
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

pub struct Writing<T: 'static>(RawRef<T>);

impl<T: 'static> Deref for Writing<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.0.pointer().as_ref() }
    }
}

impl<T: 'static> DerefMut for Writing<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.pointer().as_mut() }
    }
}

impl<T: 'static> Drop for Writing<T> {
    fn drop(&mut self) {
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
