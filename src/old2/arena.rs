use std::{
    cell::{Cell, RefCell},
    iter,
    mem::{size_of, swap},
    ptr::NonNull,
    slice,
    sync::atomic::AtomicUsize,
};

use parking_lot::RwLock;

use crate::{
    counters::{self, FastCounter, GenCounter, SyncCounter},
    mingleton, mingleton_mut, mingleton_ref,
    singleish::{MingleMut, Mingleton},
};

const MAX_CHUNK: usize = (16 * 1024 * 1024) / size_of::<usize>();

pub(crate) trait CounterArena: MingleMut
{
    type Counter: GenCounter<Arena = Self>;

    fn alloc() -> Self::Counter { Self::with_instance_mut(Self::allocate) }

    fn allocate(&mut self) -> Self::Counter;

    fn grow(n: usize) -> usize { (n + n / 2).max(MAX_CHUNK) }
}

pub(crate) struct ThreadArena
{
    baggage: Vec<Box<[Cell<usize>]>>,
    current: Box<[Cell<usize>]>,
    index: usize,
}

mingleton!(THREAD_ARENA: ThreadArena as RefCell<ThreadArena> = RefCell::new(ThreadArena::new()));
mingleton_ref!(ThreadArena : |tls| tls.borrow());
mingleton_mut!(ThreadArena : |tls| tls.borrow_mut());

impl ThreadArena
{
    fn new() -> Self
    {
        Self {
            baggage: vec![],
            current: Self::reserve(512),
            index: 0,
        }
    }

    fn reserve(sz: usize) -> Box<[Cell<usize>]> { vec![Cell::new(0); sz].into_boxed_slice() }
}

impl CounterArena for ThreadArena
{
    type Counter = FastCounter;

    fn allocate(&mut self) -> Self::Counter
    {
        if self.index < self.current.len() {
            self.index += 1;
            FastCounter(NonNull::from(&self.current[self.index - 1]))
        } else {
            let mut next = Self::reserve(Self::grow(self.current.len()));
            swap(&mut self.current, &mut next);
            self.baggage.push(next);
            self.index = 1;
            FastCounter(NonNull::from(&self.current[0]))
        }
    }
}

pub(crate) struct StaticArena
{
    pool: slice::Iter<'static, AtomicUsize>,
    next_alloc: usize,
}

impl StaticArena
{
    fn new() -> Self
    {
        Self {
            pool: Self::reserve(512),
            next_alloc: 512 + 256,
        }
    }

    fn reserve(sz: usize) -> slice::Iter<'static, AtomicUsize>
    {
        let mut res = Vec::with_capacity(sz);
        res.extend(iter::repeat_with(|| AtomicUsize::new(counters::INIT)).take(sz));
        res.leak().iter()
    }
}

mingleton!(static STATIC_ARENA: StaticArena as RwLock<StaticArena> = RwLock::new(StaticArena::new()));
mingleton_ref!(StaticArena : |gs| gs.read());
mingleton_mut!(StaticArena : |gs| gs.write());

impl CounterArena for StaticArena
{
    type Counter = counters::SyncCounter;

    fn allocate(&mut self) -> Self::Counter
    {
        if let Some(c) = self.pool.next() {
            SyncCounter(c)
        } else {
            self.pool = Self::reserve(self.next_alloc);
            self.next_alloc += self.next_alloc / 2;
            self.next_alloc = MAX_CHUNK.max(self.next_alloc);

            SyncCounter(self.pool.next().unwrap())
        }
    }
}
