#![feature(local_key_cell_methods, assert_matches)]
#![allow(unused)]

mod global_ledger;
mod local_ledger;
mod raw_ref;
mod tracking;

use std::{assert_matches::assert_matches, io::Read, ptr::NonNull};

use raw_ref::*;
use tracking::Tracking;

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

    unsafe fn try_consume_inplace(&self) -> Option<Box<T>>
    {
        self.invariant();
        let account = self.0.account();
        if account.try_lock_exclusive() {
            unsafe { tracking::free(account) }
            Some(match self.0.pointer() {
                PointerEnum::Strong(s) => unsafe { Box::from_raw(s.as_ptr()) },
                _ => panic!(),
            })
        } else {
            None
        }
    }

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
        Weak(
            self.0
                .clone()
                .set_weak()
                .map(|n| NonNull::from(unsafe { f(n.as_ref()) })),
        )
    }

    pub fn alias(&self) -> Weak<T> { self.alias_of(|x| x) }

    pub fn try_take(mut self) -> Result<Box<T>, Self>
    {
        unsafe { self.try_consume_inplace() }.ok_or_else(|| self)
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
        unsafe {
            self.try_consume_inplace();
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

    fn try_read(&self) -> Reading<T> {}

    fn try_write(&self) -> Writing<T> { todo!() }
}

struct GenRef<T>(RawRef<T>);
pub enum GenRefEnum<T>
{
    Weak(Weak<T>),
    Strong(Strong<T>),
}

pub struct Reading<T>(RawRef<T>);

impl<T> Reading<T>
{
    pub(crate) fn try_new(raw_ref: RawRef<T>) -> Option<Self>
    {
        raw_ref.invariant();
        if raw_ref.account().try_lock_shared() {
            Some(Self(raw_ref))
        } else {
            None
        }
    }
}

impl<T> Clone for Reading<T>
{
    fn clone(&self) -> Self
    {
        if !self.0.account().try_lock_shared() {
            panic!()
        }
        Self(self.0.clone())
    }
}

pub struct Writing<T>(RawRef<T>);

impl<T> Writing<T>
{
    pub(crate) fn try_new(raw_ref: RawRef<T>) -> Option<Self>
    {
        raw_ref.invariant();
        if raw_ref.account().try_lock_exclusive() {
            Some(Self(raw_ref))
        } else {
            None
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
