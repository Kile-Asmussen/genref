use std::{cell::Cell, num::NonZeroUsize, ptr::NonNull, sync::atomic::AtomicUsize};

use crate::{
    arena::{CounterArena, StaticArena, ThreadArena},
    freelists::{FreeList, SpareCounters},
    worldlock::{GlobalLock, ThreadlocalLock, WorldLock},
};

pub(crate) const INIT: usize = 1;

pub trait GenCounter: Sized + Copy
{
    type WideLock: WorldLock;
    type Arena: CounterArena<Counter = Self>;
    type Freebies: FreeList<Counter = Self>;

    fn new() -> Self { Self::Freebies::realloc().unwrap_or_else(Self::Arena::alloc) }

    fn invalidate_and_free(self)
    {
        if self.try_invalidate() {
            <Self::Freebies as FreeList>::free(self);
        } else {
            self.invalidate()
        }
    }

    fn invalidate(self);

    fn end_of_life(self) -> bool { self.read_raw() == usize::MAX }

    fn try_invalidate(self) -> bool
    {
        let res = !self.end_of_life();
        if res {
            self.invalidate();
        }
        res
    }

    fn read(self) -> NonZeroUsize
    {
        NonZeroUsize::new(self.read_raw()).expect("Overflowed generation counter.")
    }

    fn read_raw(self) -> usize;
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SyncCounter(pub(crate) &'static AtomicUsize);

impl GenCounter for SyncCounter
{
    type WideLock = GlobalLock;
    type Arena = StaticArena;
    type Freebies = SpareCounters<Self>;
    fn invalidate(self) { self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }

    fn read_raw(self) -> usize { self.0.load(std::sync::atomic::Ordering::Relaxed) }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct FastCounter(pub(crate) NonNull<Cell<usize>>);

impl GenCounter for FastCounter
{
    type WideLock = ThreadlocalLock;
    type Arena = ThreadArena;
    type Freebies = SpareCounters<Self>;
    fn invalidate(self)
    {
        unsafe {
            let c = self.0.as_ref();
            c.set(c.get().wrapping_add(1))
        }
    }

    fn read_raw(self) -> usize { unsafe { self.0.as_ref().get() } }
}
