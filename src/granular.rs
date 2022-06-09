#[derive(Clone)]
struct Generation
{
    locks: Cell<u16>,
    high: Cell<u16>,
    low: Cell<u32>,
}

impl Generation
{
    const INIT: u32 = 32;

    const fn new() -> Self
    {
        Self {
            locks: Cell::new(0),
            high: Cell::new(0),
            low: Cell::new(Self::INIT),
        }
    }

    const unsafe fn invalid() -> Self
    {
        Self {
            locks: Cell::new(0),
            high: Cell::new(0),
            low: Cell::new(0),
        }
    }

    fn maxxed(&self) -> bool { self.high.get() == u16::MAX && self.low.get() == u32::MAX }

    fn locked(&self) -> bool { self.locks.get() != 0 }
    fn saturated(&self) -> bool { self.locks.get() != u16::MAX }

    fn bump(&self)
    {
        if let Some(low) = self.low.get().checked_add(1) {
            self.low.set(low);
        } else {
            if let Some(high) = self.high.get().checked_add(1) {
                self.high.set(high);
                self.low.set(0);
            }
        }
    }

    unsafe fn read(&self) { self.locks.set(self.locks.get() + 1) }
    fn try_read(&self) -> bool
    {
        if !self.saturated() {
            unsafe { self.read() }
            true
        } else {
            false
        }
    }

    unsafe fn unread(&self) { self.locks.set(self.locks.get() - 1) }

    unsafe fn write(&self) { self.locks.set(u16::MAX) }
    fn try_write(&self) -> bool
    {
        if !self.locked() {
            unsafe { self.write() }
            true
        } else {
            false
        }
    }

    unsafe fn unwrite(&self) { self.locks.set(0); }

    fn generation(&self) -> u64 { ((self.high.get() as u64) << 32 | self.low.get() as u64) }
}

#[repr(transparent)]
#[derive(PartialEq, Eq, Clone, Copy)]
struct Counter(NonNull<Generation>);

impl hash::Hash for Counter
{
    fn hash<H: hash::Hasher>(&self, state: &mut H) { ptr::hash(self.0.as_ptr(), state) }
}

impl Counter
{
    fn new() -> Self { FreeList::unfree().unwrap_or_else(FreshList::fresh) }

    fn generation(&self) -> u64 { unsafe { self.0.as_ref() }.generation() }

    unsafe fn free(this: Self)
    {
        let c = this.0.as_ref();

        c.bump();
        if !c.maxxed() {
            FreeList::free(this);
        }
    }

    fn try_free(this: Self) -> bool
    {
        if !this.locked() {
            unsafe { Self::free(this) }
            true
        } else {
            false
        }
    }

    fn locked(&self) -> bool { unsafe { self.0.as_ref() }.locked() }

    unsafe fn read(&self) { self.0.as_ref().read() }
    unsafe fn write(&self) { self.0.as_ref().write() }

    fn try_read(&self) -> bool { unsafe { self.0.as_ref() }.try_read() }
    fn try_write(&self) -> bool { unsafe { self.0.as_ref() }.try_write() }

    unsafe fn unread(&self) { self.0.as_ref().unread() }
    unsafe fn unwrite(&self) { self.0.as_ref().unwrite() }
}

thread_local! {
    static FREELIST : RefCell<FreeList>  = RefCell::new(FreeList::new());
    static FRESHLIST : RefCell<FreshList> = RefCell::new(FreshList::new());
    static DROPQUEUE : RefCell<DropQueue> = RefCell::new(DropQueue::new());
}

struct FreeList(Vec<Counter>);
struct FreshList(usize, Vec<Generation>, Vec<Vec<Generation>>);

impl FreeList
{
    fn new() -> Self { Self(Vec::with_capacity(32)) }

    fn free_(&mut self, c: Counter) { self.0.push(c) }
    fn free(c: Counter) { FREELIST.with(|fl| fl.borrow_mut().free_(c)) }

    fn unfree_(&mut self) -> Option<Counter> { self.0.pop() }
    fn unfree() -> Option<Counter> { FREELIST.with(|fl| fl.borrow_mut().unfree_()) }
}

impl FreshList
{
    fn new() -> Self { Self(0, Self::more(32), vec![]) }

    fn fresh_(&mut self) -> Counter
    {
        if self.0 == self.1.len() {
            self.refresh()
        }
        self.0 += 1;
        Counter(NonNull::from(&self.1[self.0 - 1]))
    }

    fn fresh() -> Counter { FRESHLIST.with(|fl| fl.borrow_mut().fresh_()) }

    fn refresh(&mut self)
    {
        self.2
            .push(mem::replace(&mut self.1, Self::more(self.0 + self.0 / 2)));
        self.0 = 0;
    }

    fn more(n: usize) -> Vec<Generation> { vec![Generation::new(); n] }
}

trait DropLater {}
impl<T> DropLater for T {}
struct DropQueue(HashMap<Counter, Box<dyn DropLater>>);

impl DropQueue
{
    fn new() -> Self { Self(HashMap::with_capacity(32)) }

    unsafe fn clear_(&mut self, ctr: Counter) -> Box<dyn DropLater>
    {
        if let Some(dl) = self.0.remove(&ctr) {
            dl
        } else {
            panic!()
        }
    }

    unsafe fn clear(ctr: Counter) -> Box<dyn DropLater>
    {
        DROPQUEUE.with(|dq| dq.borrow_mut().clear_(ctr))
    }

    fn defer_(&mut self, ctr: Counter, val: Box<dyn DropLater>)
    {
        if let Some(_) = self.0.insert(ctr, val) {
            panic!()
        }
    }
    fn defer(ctr: Counter, val: Box<dyn DropLater>)
    {
        DROPQUEUE.with(|dq| dq.borrow_mut().defer_(ctr, val))
    }
}

struct Writing<'a, T>
{
    ptr: NonNull<T>,
    _ref: PhantomData<&'a mut T>,
    ctr: Counter,
}

impl<'a, T> Deref for Writing<'a, T>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.ptr.as_ref() } }
}

impl<'a, T> DerefMut for Writing<'a, T>
{
    fn deref_mut(&mut self) -> &mut Self::Target { unsafe { self.ptr.as_mut() } }
}

impl<'a, T> Drop for Writing<'a, T>
{
    fn drop(&mut self)
    {
        unsafe { self.ctr.unwrite() }
        Counter::try_free(self.ctr);
    }
}

impl<'a, T> Writing<'a, T>
{
    fn downgrade(self) -> Reading<'a, T>
    {
        unsafe {
            self.ctr.unwrite();
            self.ctr.read();
        }
        let r = Reading {
            ctr: self.ctr,
            _ref: PhantomData,
            ptr: self.ptr,
        };
        mem::forget(self);
        r
    }
}

struct Reading<'a, T>
{
    ptr: NonNull<T>,
    _ref: PhantomData<&'a T>,
    ctr: Counter,
}

impl<'a, T> Deref for Reading<'a, T>
{
    type Target = T;

    fn deref(&self) -> &Self::Target { unsafe { self.ptr.as_ref() } }
}

impl<'a, T> Clone for Reading<'a, T>
{
    fn clone(&self) -> Self
    {
        unsafe { self.ctr.read() }
        Self {
            _ref: PhantomData,
            ptr: self.ptr,
            ctr: self.ctr,
        }
    }
}

impl<'a, T> Drop for Reading<'a, T>
{
    fn drop(&mut self)
    {
        unsafe { self.ctr.unread() }
        Counter::try_free(self.ctr);
    }
}

impl<'a, T> Reading<'a, T>
{
    fn try_upgrade(self) -> Result<Writing<'a, T>, Self>
    {
        unsafe { self.ctr.unread() }
        if self.ctr.try_write() {
            let r = Writing {
                ptr: self.ptr,
                ctr: self.ctr,
                _ref: PhantomData,
            };
            mem::forget(self);
            Ok(r)
        } else {
            Err(self)
        }
    }

    /// Reference contained type
    ///
    /// Creates a weak reference to a contained field or other
    /// derived quantity.
    pub fn map<F, U>(&self, f: F) -> Weak<U>
    where
        for<'b> F: Fn(&'b T) -> &'b U,
    {
        Weak {
            gen: self.ctr.generation(),
            ctr: self.ctr,
            ptr: NonNull::from(f(&self)),
        }
    }
}

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    hash,
    io::Read,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    ops::{Deref, DerefMut},
    ptr::{self, NonNull},
};

/// Strong reference
///
/// Owns its underlying allocation.
///
/// The generation counter is allocated separately, since it must persist for
/// the entire lifetime of all `Weak` references.
pub struct Strong<T: 'static>
{
    ctr: Counter,
    ptr: ManuallyDrop<Box<T>>,
}

impl<T: 'static> Drop for Strong<T>
{
    fn drop(&mut self) {}
}

impl<T: 'static> Strong<T>
{
    pub fn new(t: T) -> Self { Self::from(Box::new(t)) }

    /// Generate a weak alias
    ///
    /// Reads the current generation to use as future reference
    pub fn alias(&self) -> Weak<T>
    {
        Weak {
            gen: self.ctr.generation(),
            ctr: self.ctr,
            ptr: NonNull::from((*self.ptr).as_ref()),
        }
    }

    /// Extract underlying box
    ///
    /// This can potentially be used to deallocate the box, so
    /// it requires writing privileges
    pub fn try_into_inner(mut self) -> Option<Box<T>>
    {
        if !self.ctr.locked() {
            let r = unsafe { ManuallyDrop::take(&mut self.ptr) };
            mem::forget(self);
            Some(r)
        } else {
            None
        }
    }

    /// Obtain reading permissions
    ///
    /// Read lock is necessary as otherwise a mutable alias could
    /// be created using a `Weak`.
    pub fn try_borrow(&self) -> Option<Reading<T>>
    {
        if self.ctr.try_read() {
            Some(Reading {
                ptr: NonNull::from(self.ptr.as_ref()),
                _ref: PhantomData,
                ctr: self.ctr,
            })
        } else {
            None
        }
    }

    /// Obtain writing permissions
    pub fn try_borrow_mut(&mut self) -> Option<Writing<T>>
    {
        if self.ctr.try_write() {
            Some(Writing {
                ptr: NonNull::from(self.ptr.as_mut()),
                _ref: PhantomData,
                ctr: self.ctr,
            })
        } else {
            None
        }
    }
}

impl<T: 'static> From<Box<T>> for Strong<T>
{
    fn from(b: Box<T>) -> Self
    {
        Self {
            ctr: Counter::new(),
            ptr: ManuallyDrop::new(b),
        }
    }
}

/// Weak reference
///
/// Stores its reference generation locally and cross-checks it everytime an
/// access is made.
pub struct Weak<T: 'static>
{
    gen: u64,
    ctr: Counter,
    ptr: NonNull<T>,
}

impl<T: 'static> Weak<T>
{
    /// Creates an invalid reference.
    pub fn dangling() -> Self
    {
        static mut ZERO: Generation = unsafe { Generation::invalid() };
        Weak {
            gen: u64::MAX,
            ctr: Counter(NonNull::from(unsafe { &ZERO })),
            ptr: NonNull::dangling(),
        }
    }

    /// Check if this reference is currently valid
    pub fn is_valid(&self) -> bool { self.gen == self.ctr.generation() }

    /// Attempt to dereference
    ///
    /// Returns `None` if the reference is invalid, otherwise
    /// functions as `Strong::as_ref`
    pub fn try_borrow(&self) -> Option<Reading<T>>
    {
        if self.is_valid() && self.ctr.try_read() {
            Some(Reading {
                ptr: self.ptr,
                _ref: PhantomData,
                ctr: self.ctr,
            })
        } else {
            None
        }
    }

    /// Attempt to mutate
    ///
    /// Returns `None` if the reference is invalid, otherwise
    /// functions as `Strong::as_mut`
    pub fn try_borrow_mut(&mut self) -> Option<Writing<T>>
    {
        if self.is_valid() && self.ctr.try_write() {
            Some(Writing {
                ptr: self.ptr,
                _ref: PhantomData,
                ctr: self.ctr,
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
