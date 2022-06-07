#![feature(assert_matches)]

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

#[derive(Debug)]
pub struct Reading;
pub fn reading() -> Option<Reading> { Lock::reading() }

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

pub struct Strong<T: 'static>
{
    gen: Generation,
    ptr: ManuallyDrop<Box<T>>,
}

#[derive(Copy, Clone)]
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
    pub fn new(b: Box<T>) -> Self
    {
        Self {
            ptr: ManuallyDrop::new(b),
            gen: Generation::new(),
        }
    }

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
}

impl<T: 'static> Weak<T>
{
    pub fn as_ref(&self, _rl: &Reading) -> Option<&T>
    {
        if self.gen.get() == self.genref {
            Some(unsafe { self.ptr.as_ref() })
        } else {
            None
        }
    }

    pub fn as_mut(&mut self, _wl: &mut Writing) -> Option<&mut T>
    {
        if self.gen.get() == self.genref {
            Some(unsafe { self.ptr.as_mut() })
        } else {
            None
        }
    }
}
