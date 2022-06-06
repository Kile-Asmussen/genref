use std::{
    mem::ManuallyDrop,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use crate::{
    counters::GenCounter,
    dropqueue::DropQueue,
    worldlock::{DropLater, WorldLock},
};

pub struct Owned<T: 'static + DropLater, W: WorldLock>
{
    counter: W::Counter,
    data: ManuallyDrop<Box<T>>,
}

impl<T: 'static, W: WorldLock> Owned<T, W>
{
    pub fn new(it: T) -> Self
    {
        Self {
            counter: W::Counter::new(),
            data: ManuallyDrop::new(Box::new(it)),
        }
    }

    pub fn alias(&self) -> Weak<T, W>
    {
        Weak {
            truth: self.counter.read(),
            counter: self.counter,
            data: NonNull::from(self.data.as_ref()),
        }
    }

    pub fn take(mut self, l: &mut W::WriteLock) -> Box<T>
    {
        self.counter.invalidate_and_free();

        let res = unsafe { ManuallyDrop::take(&mut self.data) };
        ::std::mem::forget(self);
        res
    }
}

impl<T: 'static, W: WorldLock> Drop for Owned<T, W>
{
    fn drop(&mut self)
    {
        self.counter.invalidate_and_free();

        if let Some(l) = W::try_write() {
            let _bx = unsafe { ManuallyDrop::take(&mut self.data) };
            std::mem::drop(l);
        } else {
            let bx = unsafe { ManuallyDrop::take(&mut self.data) };
            <W::GarbageBin as DropQueue>::drop_later(bx);
        }
    }
}

#[derive(Clone, Copy)]
pub struct Weak<T: 'static, W: WorldLock>
{
    truth: NonZeroUsize,
    counter: W::Counter,
    data: NonNull<T>,
}

impl<T: 'static, W: WorldLock> Weak<T, W>
{
    pub fn read(self, r: &W::ReadLock) -> ReadGuard<T, W>
    {
        ReadGuard {
            data: self.data,
            lock: r,
        }
    }

    pub fn write(self, w: &mut W::WriteLock) -> WriteGuard<T, W>
    {
        WriteGuard {
            data: self.data,
            lock: w,
        }
    }
}
struct ReadGuard<'a, T, W: WorldLock>
{
    data: NonNull<T>,
    lock: &'a W::ReadLock,
}

struct WriteGuard<'a, T, W: WorldLock>
{
    data: NonNull<T>,
    lock: &'a mut W::WriteLock,
}

impl<'a, T: 'static, W: WorldLock> Deref for ReadGuard<'a, T, W>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.data.as_ref() } }
}

impl<'a, T, W: WorldLock> Deref for WriteGuard<'a, T, W>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.data.as_ref() } }
}

impl<'a, T, W: WorldLock> DerefMut for WriteGuard<'a, T, W>
{
    fn deref_mut(&mut self) -> &mut Self::Target { unsafe { self.data.as_mut() } }
}
