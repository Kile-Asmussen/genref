#![feature(assert_matches)]

//! # Generational counting
//!
//! This crate implements Vale's generational reference counting memory
//! management. Intended as an alternative to Rc with slightly different
//! semantics.
//!
//! Advantages over `Rc`:
//! - Sharing references are `Copy` and therefore extremely cheap
//! - RAII semantics
//!
//! Disadvantages:
//! - Only one owned reference, requiring management
//! - Dereferencing returns `Option`
//! - Not `Deref`
//!
//! The locking system is non-granular for ease of implementation (and possibly
//! speed.)

#[cfg(test)]
mod tests;

use std::{
    cell::{Cell, RefCell},
    mem,
};

#[derive(Clone, Copy)]
struct Generation(NonNull<Cell<u32>>);

impl Generation
{
    fn new() -> Self { FreeList::unfree().unwrap_or_else(FreshList::fresh) }

    fn get(&self) -> u32 { unsafe { self.0.as_ref().get() } }

    fn free(this: Self)
    {
        let n = unsafe {
            let c = this.0.as_ref();
            c.set(c.get().wrapping_add(1));
            c.get()
        };

        if n != 0 {
            FreeList::free(this);
        }
    }
}

thread_local! {
    static FREELIST : RefCell<FreeList> = RefCell::new(FreeList::new());
    static FRESHLIST : RefCell<FreshList> = RefCell::new(FreshList::new());
    static LOCK : Lock = Lock::new();
    static DROPQUEUE : RefCell<DropQueue> = RefCell::new(DropQueue::new());
}

struct Lock(Cell<isize>);

/// Non-exclusive lock (ZST)
///
/// Used to create shared references to underlying objects,
/// its existence defers dropping of allocated objects.
#[derive(Debug)]
pub struct Reading;
pub fn reading() -> Option<Reading> { Lock::reading() }

/// Exclusive lock (ZST)
///
/// Used to create mutable references to underlying objects,
/// its existence defers dropping of allocated objects.
#[derive(Debug)]
pub struct Writing;
pub fn writing() -> Option<Writing> { Lock::writing() }

impl Lock
{
    fn new() -> Self { Self(Cell::new(0)) }
    fn reading() -> Option<Reading>
    {
        if Self::readable() {
            unsafe { Self::read() }
            Some(Reading)
        } else {
            None
        }
    }

    fn writing() -> Option<Writing>
    {
        if Self::writable() {
            unsafe { Self::write() }
            Some(Writing)
        } else {
            None
        }
    }

    fn writable() -> bool { LOCK.with(|l| l.0.get() == 0) }
    unsafe fn write() { LOCK.with(|l| l.0.set(-1)) }
    unsafe fn unwrite() { LOCK.with(|l| l.0.set(0)) }

    fn readable() -> bool { LOCK.with(|l| l.0.get() >= 0) }
    unsafe fn read() { LOCK.with(|l| l.0.set(l.0.get() + 1)) }
    unsafe fn unread() { LOCK.with(|l| l.0.set(l.0.get() - 1)) }
}

impl Drop for Reading
{
    fn drop(&mut self)
    {
        unsafe { Lock::unread() }
        if let Some(mut wl) = Lock::writing() {
            let d = DropQueue::clear(&mut wl);
            mem::drop(wl);
            mem::drop(d);
        }
    }
}

impl Clone for Reading
{
    fn clone(&self) -> Self
    {
        unsafe { Lock::read() }
        Self
    }
}

impl Drop for Writing
{
    fn drop(&mut self)
    {
        DropQueue::clear(self);
        unsafe { Lock::unwrite() }
    }
}

struct FreeList(Vec<Generation>);
struct FreshList(usize, Vec<Cell<u32>>, Vec<Vec<Cell<u32>>>);

impl FreeList
{
    fn new() -> Self { Self(Vec::with_capacity(32)) }

    fn free_(&mut self, gen: Generation) { self.0.push(gen) }
    fn free(gen: Generation) { FREELIST.with(|fl| fl.borrow_mut().free_(gen)) }

    fn unfree_(&mut self) -> Option<Generation> { self.0.pop() }
    fn unfree() -> Option<Generation> { FREELIST.with(|fl| fl.borrow_mut().unfree_()) }
}

impl FreshList
{
    fn new() -> Self { Self(0, vec![Cell::new(0); 32], vec![]) }

    fn fresh_(&mut self) -> Generation
    {
        if self.0 == self.1.len() {
            self.refresh()
        }
        self.0 += 1;
        Generation(NonNull::from(&self.1[self.0 - 1]))
    }

    fn fresh() -> Generation { FRESHLIST.with(|fl| fl.borrow_mut().fresh_()) }

    fn refresh(&mut self)
    {
        self.2.push(mem::replace(
            &mut self.1,
            vec![Cell::new(1); self.0 + self.0 / 2],
        ));
        self.0 = 0;
    }
}
trait DropLater {}
impl<T> DropLater for T {}
struct DropQueue(Vec<Box<dyn DropLater>>);

impl DropQueue
{
    fn new() -> Self { Self(Vec::with_capacity(32)) }

    fn clear_(&mut self, _wl: &mut Writing) -> impl Drop
    {
        mem::replace(&mut self.0, Vec::with_capacity(32))
    }

    fn clear(wl: &mut Writing) -> impl Drop { DROPQUEUE.with(|dq| dq.borrow_mut().clear_(wl)) }

    fn defer_(&mut self, val: Box<dyn DropLater>) { self.0.push(val) }
    fn defer(val: Box<dyn DropLater>) { DROPQUEUE.with(|dq| dq.borrow_mut().defer_(val)) }
}

pub fn deferred() -> usize { DROPQUEUE.with(|dq| dq.borrow().0.len()) }

use std::{mem::ManuallyDrop, ptr::NonNull};

/// Strong reference
///
/// Owns its underlying allocation.
///
/// The generation counter is allocated separately, since it must persist for
/// the entire lifetime of all `Weak` references.
pub struct Strong<T: 'static>
{
    gen: Generation,
    ptr: ManuallyDrop<Box<T>>,
}

/// Weak reference
///
/// Stores its reference generation locally and cross-checks it everytime an
/// access is made.
pub struct Weak<T: 'static>
{
    genref: u32,
    gen: Generation,
    ptr: NonNull<T>,
}

impl<T: 'static> Drop for Strong<T>
{
    fn drop(&mut self)
    {
        Generation::free(self.gen);
        if let Some(wl) = Lock::writing() {
            let d = unsafe { ManuallyDrop::take(&mut self.ptr) };
            mem::drop(wl);
            mem::drop(d);
        } else {
            DropQueue::defer(unsafe { ManuallyDrop::take(&mut self.ptr) } as Box<dyn DropLater>);
        }
    }
}

impl<T: 'static> Strong<T>
{
    pub fn new(t: T) -> Self { Self::from(Box::new(t)) }

    pub fn alias(&self) -> Weak<T>
    {
        Weak {
            genref: self.gen.get(),
            gen: self.gen,
            ptr: NonNull::from(self.ptr.as_ref()),
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
    {
        Weak {
            genref: self.gen.get(),
            gen: self.gen,
            ptr: NonNull::from(f(self.as_ref(rl))),
        }
    }
}

impl<T: 'static> From<Box<T>> for Strong<T>
{
    fn from(b: Box<T>) -> Self
    {
        Self {
            gen: Generation::new(),
            ptr: ManuallyDrop::new(b),
        }
    }
}

impl<T: 'static> Weak<T>
{
    pub fn dangling() -> Self
    {
        let gen = Generation::new();
        let res = Weak {
            genref: gen.get() - 1,
            gen: gen,
            ptr: NonNull::dangling(),
        };
        Generation::free(gen);
        res
    }

    pub fn is_valid(&self) -> bool { self.genref == self.gen.get() }

    pub fn try_as_ref(&self, _rl: &Reading) -> Option<&T>
    {
        if self.is_valid() {
            Some(unsafe { self.ptr.as_ref() })
        } else {
            None
        }
    }

    pub fn try_as_mut(&mut self, _wl: &mut Writing) -> Option<&mut T>
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
    {
        if let Some(a) = self.try_as_ref(rl) {
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

impl<T: 'static> Clone for Weak<T>
{
    fn clone(&self) -> Self { *self }
}

impl<T: 'static> Copy for Weak<T> {}

pub enum Ref<T: 'static>
{
    Strong(Strong<T>),
    Weak(Weak<T>),
}

impl<T: 'static> Ref<T>
{
    pub fn try_as_ref(&self, rl: &Reading) -> Option<&T>
    {
        match self {
            Ref::Strong(s) => Some(s.as_ref(rl)),
            Ref::Weak(w) => w.try_as_ref(rl),
        }
    }

    pub fn try_as_mut(&mut self, wl: &mut Writing) -> Option<&mut T>
    {
        match self {
            Ref::Strong(s) => Some(s.as_mut(wl)),
            Ref::Weak(w) => w.try_as_mut(wl),
        }
    }

    pub fn try_map<F, U>(&self, rl: &Reading, f: F) -> Option<Ref<U>>
    where
        for<'a> F: Fn(&'a T) -> &'a U,
    {
        match self {
            Ref::Strong(s) => Some(Ref::Weak(s.map(rl, f))),
            Ref::Weak(w) => w.try_map(rl, f).map(Ref::Weak),
        }
    }
}

impl<T: 'static> Clone for Ref<T>
{
    fn clone(&self) -> Self
    {
        match self {
            Self::Strong(s) => Self::Weak(s.alias()),
            Self::Weak(w) => Self::Weak(*w),
        }
    }
}
