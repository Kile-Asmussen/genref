use std::cell::RefCell;

use parking_lot::RwLock;

use crate::{
    counters::{FastCounter, GenCounter, SyncCounter},
    mingleton, mingleton_mut, mingleton_ref,
    singleish::{MingleMut, Mingleton},
};

pub(crate) trait FreeList: MingleMut
{
    type Counter: GenCounter;

    fn free(c: Self::Counter) { Self::with_instance_mut(|fl| fl.release(c)) }

    fn release(&mut self, c: Self::Counter);

    fn realloc() -> Option<Self::Counter> { Self::with_instance_mut(Self::reallocate) }

    fn reallocate(&mut self) -> Option<Self::Counter>;
}

pub(crate) struct SpareCounters<C: GenCounter>(Vec<C>);

mingleton!(
    SPARE_FAST: SpareCounters<FastCounter> as RefCell<SpareCounters<FastCounter>> =
        RefCell::new(SpareCounters::new())
);
mingleton_ref!(SpareCounters<FastCounter> : |tls| tls.borrow());
mingleton_mut!(SpareCounters<FastCounter> : |tls| tls.borrow_mut());

mingleton!(
    SPARE_SYNC: SpareCounters<SyncCounter> as RwLock<SpareCounters<SyncCounter>> =
        RwLock::new(SpareCounters::new())
);
mingleton_ref!(SpareCounters<SyncCounter> : |gs| gs.read());
mingleton_mut!(SpareCounters<SyncCounter> : |gs| gs.write());

impl<C: GenCounter> SpareCounters<C>
{
    fn new() -> Self { Self(Vec::with_capacity(32)) }
}

impl<C: GenCounter> FreeList for SpareCounters<C>
where
    Self: MingleMut,
{
    type Counter = C;

    fn release(&mut self, c: Self::Counter) { self.0.push(c) }

    fn reallocate(&mut self) -> Option<Self::Counter> { self.0.pop() }
}
