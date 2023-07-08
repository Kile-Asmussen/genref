#![feature(local_key_cell_methods, assert_matches)]
#![allow(unused)]

mod global_ledger;
mod local_ledger;
mod raw_ref;
mod tracking;

use std::{
    assert_matches::assert_matches,
    io::Read,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    os::linux::raw,
    ptr::NonNull,
};

use raw_ref::*;
use tracking::{AccountEnum, Tracking};

pub struct Strong<T>(RawRef<T>);

impl<T> Strong<T>
{
    #[cfg(test)]
    fn invariant(&self)
    {
        self.0.invariant();
        assert_matches!(
            self.0.pointer(),
            PointerEnum::Strong(_),
            "strong reference without strong flag"
        );
    }

    #[cfg(not(test))]
    fn invariant(&self) {}

    pub fn from_box(it: Box<T>) -> Self
    {
        let res = Self(RawRef::from_box(it));
        res.invariant();
        res
    }

    pub fn alias_of<F, U>(&self, f: F) -> Weak<U>
    where
        for<'a> F: FnOnce(&'a T) -> &'a U,
    {
        let acc = self.0.account();
        let ptr = self.0.pointer();
        Weak::new(
            self.0
                .clone()
                .set_weak()
                .map(|n| NonNull::from(unsafe { f(n.as_ref()) })),
        )
    }

    pub fn alias(&self) -> Weak<T> { self.alias_of(|x| x) }

    pub fn try_take(mut self) -> Result<Box<T>, Self>
    {
        self.invariant();
        if let Some(b) = unsafe { self.0.try_consume_exclusive() } {
            std::mem::forget(self);
            Ok(b)
        } else {
            Err(self)
        }
    }

    fn try_read(&self) -> Option<Reading<T>>
    {
        self.invariant();
        Reading::try_new(self.0.clone())
    }

    fn try_write(&self) -> Option<Writing<T>>
    {
        self.invariant();
        Writing::try_new(self.0.clone())
    }
}

impl<T> Drop for Strong<T>
{
    fn drop(&mut self)
    {
        self.invariant();
        unsafe {
            self.0.try_consume_exclusive();
        }
    }
}

pub struct Weak<T>(RawRef<T>);
impl<T> Clone for Weak<T>
{
    fn clone(&self) -> Self { Self(self.0.clone()) }
}

impl<T> Weak<T>
{
    fn invariant(&self)
    {
        self.0.invariant();
        assert_matches!(
            self.0.pointer(),
            PointerEnum::Weak(_),
            "weak reference without weak flag"
        )
    }

    fn new(raw_ref: RawRef<T>) -> Self
    {
        let res = Weak(raw_ref);
        res.invariant();
        res
    }

    pub fn try_read(&self) -> Option<Reading<T>> { Reading::try_new(self.0.clone()) }

    pub fn try_write(&self) -> Option<Writing<T>> { Writing::try_new(self.0.clone()) }
}

struct GenRef<T>(RawRef<T>);
pub enum GenRefEnum<T>
{
    Weak(Weak<T>),
    Strong(Strong<T>),
}

pub struct Reading<'a, T>(RawRef<T>, PhantomData<&'a ()>);

impl<'a, T> Reading<'a, T>
{
    fn invariant(&self) { self.0.invariant(); }

    pub(crate) fn try_new(raw_ref: RawRef<T>) -> Option<Self>
    {
        raw_ref.invariant();
        if raw_ref.account().try_lock_shared() {
            let res = Self(raw_ref, PhantomData);
            res.invariant();
            Some(res)
        } else {
            None
        }
    }
}

impl<'a, T> Deref for Reading<'a, T>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.0.pointer().as_ptr().as_ref() } }
}

impl<'a, T> Drop for Reading<'a, T>
{
    fn drop(&mut self)
    {
        unsafe {
            self.0.try_consume_shared();
        }
    }
}

impl<'a, T> Clone for Reading<'a, T>
{
    fn clone(&self) -> Self
    {
        if !self.0.account().try_lock_shared() {
            panic!()
        }
        Self(self.0.clone(), PhantomData)
    }
}

pub struct Writing<'a, T>(RawRef<T>, PhantomData<&'a ()>);

impl<'a, T> Writing<'a, T>
{
    fn invariant(&self) { self.0.invariant(); }

    pub(crate) fn try_new(raw_ref: RawRef<T>) -> Option<Self>
    {
        raw_ref.invariant();
        if raw_ref.account().try_lock_exclusive() {
            let res = Self(raw_ref, PhantomData);
            res.invariant();
            Some(res)
        } else {
            None
        }
    }
}

impl<'a, T> Deref for Writing<'a, T>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.0.pointer().as_ptr().as_ref() } }
}

impl<'a, T> DerefMut for Writing<'a, T>
{
    fn deref_mut(&mut self) -> &mut Self::Target { unsafe { self.0.pointer().as_ptr().as_mut() } }
}

impl<'a, T> Drop for Writing<'a, T>
{
    fn drop(&mut self)
    {
        unsafe {
            self.0.try_consume_exclusive();
        }
    }
}

pub struct Sendable<T>(Strong<T>);
pub struct Shareable<T>(Weak<T>);
pub struct Transferrable<T>(GenRef<T>);
pub enum TransferrableEnum<T>
{
    Sendable(Sendable<T>),
    Shareable(Shareable<T>),
}
