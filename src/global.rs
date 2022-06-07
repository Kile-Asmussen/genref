//! # Sharable generation counting
//!
//! This module implements the same functionality as the base module, but
//! implemented with locks and atomics. It is to `Arc` what base genrefs are to
//! `Rc`.

use std::{
    mem, ptr,
    sync::atomic::{AtomicU32, Ordering},
};

#[cfg(not(feature = "parking_lot"))]
use std::sync::{Mutex, RwLock};

#[cfg(feature = "parking_lot")]
use lock_api::{RawRwLock, RawRwLockRecursive, RawRwLockUpgrade};
#[cfg(feature = "parking_lot")]
use parking_lot::Mutex;

use lazy_static::lazy_static;

#[repr(transparent)]
#[derive(Clone, Copy)]
struct Generation(&'static AtomicU32);

impl Generation
{
    fn new() -> Self { FreeList::unfree().unwrap_or_else(FreshList::fresh) }
    fn get(&self) -> u32 { self.0.load(Ordering::Relaxed) }

    fn free(this: Self)
    {
        let c = unsafe { this.0.fetch_add(1, Ordering::Relaxed) };

        if c != u32::MAX {
            FreeList::free(this);
        }
    }
}

lazy_static! {
    static ref FREELIST: Mutex<FreeList> = Mutex::new(FreeList::new());
    static ref FRESHLIST: Mutex<FreshList> = Mutex::new(FreshList::new());
    static ref LOCK: Lock = Lock::new();
    static ref DROPQUEUE: Mutex<DropQueue> = Mutex::new(DropQueue::new());
}

struct Lock(parking_lot::RawRwLock);

/// Non-exclusive lock (ZST)
///
/// Used to create shared references to underlying objects,
/// its existence defers dropping of allocated objects.
#[derive(Debug)]
pub struct Reading(());
pub fn reading() -> Reading { Lock::reading() }

/// Exclusive lock (ZST)
///
/// Used to create mutable references to underlying objects,
/// its existence defers dropping of allocated objects.
#[derive(Debug)]
pub struct Writing(());
pub fn writing() -> Writing { Lock::writing() }

impl Lock
{
    fn new() -> Self { Self(lock_api::RawRwLock::INIT) }

    fn reading() -> Reading
    {
        LOCK.0.lock_shared_recursive();
        Reading(())
    }

    fn writing() -> Writing
    {
        LOCK.0.lock_exclusive();
        Writing(())
    }

    fn try_reading() -> Option<Reading>
    {
        if LOCK.0.try_lock_shared_recursive() {
            Some(Reading(()))
        } else {
            None
        }
    }

    fn try_writing() -> Option<Writing>
    {
        if LOCK.0.try_lock_exclusive() {
            Some(Writing(()))
        } else {
            None
        }
    }
}

impl Reading
{
    fn try_upgrade(self) -> Result<Writing, Self>
    {
        LOCK.0.lock_upgradable();
        unsafe { LOCK.0.unlock_shared() }
        if unsafe { LOCK.0.try_upgrade() } {
            mem::forget(self);
            Ok(Writing(()))
        } else {
            LOCK.0.lock_shared_recursive();
            unsafe { LOCK.0.unlock_upgradable() }
            Err(self)
        }
    }
}

impl Drop for Reading
{
    fn drop(&mut self)
    {
        let this = unsafe { ptr::read(self as *const Reading) };

        let dq;
        match this.try_upgrade() {
            Ok(mut wl) => dq = DropQueue::clear(&mut wl),
            Err(rl) => mem::forget(rl),
        }

        unsafe { LOCK.0.unlock_shared() }
    }
}

impl Clone for Reading
{
    fn clone(&self) -> Self { reading() }
}

impl Drop for Writing
{
    fn drop(&mut self)
    {
        let q = DropQueue::clear(self);
        unsafe { LOCK.0.unlock_shared() }
        mem::drop(q);
    }
}

struct FreeList(Vec<Generation>);
struct FreshList(usize, &'static [AtomicU32]);

impl FreeList
{
    fn new() -> Self { Self(Vec::with_capacity(32)) }

    fn free_(&mut self, gen: Generation) { self.0.push(gen) }
    fn free(gen: Generation) { FREELIST.lock().free_(gen) }

    fn unfree_(&mut self) -> Option<Generation> { self.0.pop() }
    fn unfree() -> Option<Generation> { FREELIST.lock().unfree_() }
}

impl FreshList
{
    const INIT: u32 = 1;
    fn new() -> Self { Self(0, Self::more(32)) }

    fn fresh_(&mut self) -> Generation
    {
        if self.0 == self.1.len() {
            self.refresh()
        }
        self.0 += 1;
        Generation(&self.1[self.0 - 1])
    }

    fn fresh() -> Generation { FRESHLIST.lock().fresh_() }

    fn refresh(&mut self)
    {
        self.1 = Self::more(self.0 + self.0 / 2);
        self.0 = 0;
    }

    fn more(n: usize) -> &'static [AtomicU32]
    {
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(AtomicU32::new(Self::INIT))
        }
        Vec::leak(v)
    }
}
trait DropLater: Send + Sync {}
impl<T: Send + Sync> DropLater for T {}
struct DropQueue(Vec<Box<dyn DropLater>>);

impl DropQueue
{
    fn new() -> Self { Self(Vec::with_capacity(32)) }

    fn clear_(&mut self, _wl: &mut Writing) -> impl Drop
    {
        let re = Vec::with_capacity(self.0.len());
        mem::replace(&mut self.0, re)
    }

    fn clear(wl: &mut Writing) -> impl Drop { DROPQUEUE.lock().clear_(wl) }

    fn defer_(&mut self, val: Box<dyn DropLater>) { self.0.push(val) }
    fn defer(val: Box<dyn DropLater>) { DROPQUEUE.lock().defer_(val) }
}

use std::{mem::ManuallyDrop, ptr::NonNull};

/// Strong reference
///
/// Owns its underlying allocation.
///
/// The generation counter is allocated separately, since it must persist for
/// the entire lifetime of all `Weak` references.
pub struct Strong<T: Sync + Send + 'static>
{
    gen: Generation,
    ptr: ManuallyDrop<Box<T>>,
}

/// Weak reference
///
/// Stores its reference generation locally and cross-checks it everytime an
/// access is made.
pub struct Weak<T: Sync + Send + 'static>
{
    genref: u32,
    gen: Generation,
    ptr: NonNull<T>,
}

impl<T: Sync + Send + 'static> Drop for Strong<T>
{
    fn drop(&mut self)
    {
        Generation::free(self.gen);
        if let Some(wl) = Lock::try_writing() {
            let d = unsafe { ManuallyDrop::take(&mut self.ptr) };
            mem::drop(wl);
            mem::drop(d);
        } else {
            DropQueue::defer(unsafe { ManuallyDrop::take(&mut self.ptr) } as Box<dyn DropLater>);
        }
    }
}

impl<T: Sync + Send + 'static> Strong<T>
{
    pub fn new(t: T) -> Self { Self::from(Box::new(t)) }

    pub fn alias(&self) -> Weak<T>
    {
        Weak {
            genref: self.gen.get(),
            gen: self.gen,
            ptr: NonNull::from((*self.ptr).as_ref()),
        }
    }

    pub fn take(mut self, _wl: &mut Writing) -> Box<T>
    {
        Generation::free(self.gen);
        let b = unsafe { ManuallyDrop::take(&mut self.ptr) };
        mem::forget(self);
        b
    }

    pub fn as_ref(&self, _rl: &Reading) -> &T { &self.ptr }
    pub fn as_mut(&mut self, _wl: &mut Writing) -> &mut T { &mut self.ptr }

    pub fn map<F, U>(&self, rl: &Reading, f: F) -> Weak<U>
    where
        for<'a> F: Fn(&'a T) -> &'a U,
        U: Sync + Send + 'static,
    {
        Weak {
            genref: self.gen.get(),
            gen: self.gen,
            ptr: NonNull::from(f(self.as_ref(rl))),
        }
    }
}

impl<T: Sync + Send + 'static> From<Box<T>> for Strong<T>
{
    fn from(b: Box<T>) -> Self
    {
        Self {
            gen: Generation::new(),
            ptr: ManuallyDrop::new(b),
        }
    }
}

impl<T: Sync + Send + 'static> Weak<T>
{
    pub fn dangling() -> Self
    {
        static ZERO: AtomicU32 = AtomicU32::new(0);
        Weak {
            genref: u32::MAX,
            gen: Generation(&ZERO),
            ptr: NonNull::dangling(),
        }
    }

    pub fn is_valid(&self) -> bool { self.genref == self.gen.get() }

    pub fn try_ref(&self, _rl: &Reading) -> Option<&T>
    {
        if self.is_valid() {
            Some(unsafe { self.ptr.as_ref() })
        } else {
            None
        }
    }

    pub fn try_mut(&mut self, _wl: &mut Writing) -> Option<&mut T>
    {
        if self.is_valid() {
            Some(unsafe { self.ptr.as_mut() })
        } else {
            None
        }
    }

    pub fn try_map<F, U>(&self, rl: &Reading, f: F) -> Option<Weak<U>>
    where
        for<'a> F: Fn(&'a T) -> &'a U,
        U: Sync + Send + 'static,
    {
        if let Some(a) = self.try_ref(rl) {
            Some(Weak {
                genref: self.genref,
                gen: self.gen,
                ptr: NonNull::from(f(a)),
            })
        } else {
            None
        }
    }
}

impl<T: Sync + Send + 'static> Clone for Weak<T>
{
    fn clone(&self) -> Self { *self }
}

impl<T: Sync + Send + 'static> Copy for Weak<T> {}

pub enum Ref<T: Sync + Send + 'static>
{
    Strong(Strong<T>),
    Weak(Weak<T>),
}

impl<T: Sync + Send + 'static> Ref<T>
{
    /// New strong reference
    pub fn new(t: T) -> Self { Self::Strong(Strong::new(t)) }

    pub fn try_as_ref(&self, rl: &Reading) -> Option<&T>
    {
        match self {
            Ref::Strong(s) => Some(s.as_ref(rl)),
            Ref::Weak(w) => w.try_ref(rl),
        }
    }

    pub fn try_mut(&mut self, wl: &mut Writing) -> Option<&mut T>
    {
        match self {
            Ref::Strong(s) => Some(s.as_mut(wl)),
            Ref::Weak(w) => w.try_mut(wl),
        }
    }

    pub fn try_map<F, U>(&self, rl: &Reading, f: F) -> Option<Ref<U>>
    where
        for<'a> F: Fn(&'a T) -> &'a U,
        U: Sync + Send + 'static,
    {
        match self {
            Ref::Strong(s) => Some(Ref::Weak(s.map(rl, f))),
            Ref::Weak(w) => w.try_map(rl, f).map(Ref::Weak),
        }
    }

    pub fn is_weak(&self) -> bool
    {
        match self {
            Ref::Strong(_) => false,
            Ref::Weak(_) => true,
        }
    }

    pub fn is_strong(&self) -> bool
    {
        match self {
            Ref::Strong(_) => false,
            Ref::Weak(_) => true,
        }
    }

    pub fn is_valid(&self) -> bool
    {
        match self {
            Ref::Strong(_) => true,
            Ref::Weak(w) => w.is_valid(),
        }
    }
}

impl<T: Sync + Send + 'static> Clone for Ref<T>
{
    fn clone(&self) -> Self
    {
        match self {
            Self::Strong(s) => Self::Weak(s.alias()),
            Self::Weak(w) => Self::Weak(*w),
        }
    }
}

impl<T: Sync + Send + 'static> From<Weak<T>> for Ref<T>
{
    fn from(w: Weak<T>) -> Self { Ref::Weak(w) }
}

impl<T: Sync + Send + 'static> From<Strong<T>> for Ref<T>
{
    fn from(s: Strong<T>) -> Self { Ref::Strong(s) }
}
