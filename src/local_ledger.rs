use super::global_ledger::*;
use super::*;
use std::{
    cell::{Cell, Ref, RefCell},
    ptr::NonNull,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalIndex(NonNull<RefCell<LocalAccount>>);

impl LocalIndex
{
    fn borrow(&self) -> Ref<LocalAccount> { unsafe { self.0.as_ref() }.borrow() }

    // assumes exclusive lock
    pub(crate) unsafe fn make_sharable(&self)
    {
        let mut cell = self.0.as_ref().borrow_mut();
        let acc = LocalAccount::Global(match &*cell {
            LocalAccount::Local(l) => {
                let res = global_ledger::allocate();
                if !res.try_lock_exclusive() {
                    panic!("failed to exclusive lock just-allocated global index")
                }
                res
            }
            LocalAccount::Global(g) => *g,
        });
    }
}

impl Tracking for LocalIndex
{
    fn generation(&self) -> u64 { self.borrow().generation() }
    fn invalidate(&self) -> u64 { self.borrow().invalidate() }
    fn try_lock_exclusive(&self) -> bool { self.borrow().try_lock_exclusive() }
    fn lock_exclusive(&self) { self.borrow().lock_exclusive() }
    fn try_lock_shared(&self) -> bool { self.borrow().try_lock_shared() }
    fn try_upgrade(&self) -> bool { self.borrow().try_upgrade() }
    unsafe fn unlock_exclusive(&self) { self.borrow().unlock_exclusive() }
    unsafe fn unlock_shared(&self) { self.borrow().unlock_shared() }
}

#[derive(Debug, Clone)]
pub(crate) enum LocalAccount
{
    Local(LocalCounter),
    Global(GlobalIndex),
}

impl Tracking for LocalAccount
{
    fn generation(&self) -> u64
    {
        match self {
            Self::Local(l) => l.generation(),
            Self::Global(g) => g.generation(),
        }
    }

    fn invalidate(&self) -> u64
    {
        match self {
            Self::Local(l) => l.invalidate(),
            Self::Global(g) => g.invalidate(),
        }
    }

    fn try_lock_exclusive(&self) -> bool
    {
        match self {
            Self::Local(l) => l.try_lock_exclusive(),
            Self::Global(g) => g.try_lock_exclusive(),
        }
    }

    fn lock_exclusive(&self)
    {
        match self {
            LocalAccount::Local(l) => l.lock_exclusive(),
            LocalAccount::Global(g) => g.lock_exclusive(),
        }
    }

    fn try_lock_shared(&self) -> bool
    {
        match self {
            Self::Local(l) => l.try_lock_shared(),
            Self::Global(g) => g.try_lock_shared(),
        }
    }

    fn try_upgrade(&self) -> bool
    {
        match self {
            Self::Local(l) => l.try_upgrade(),
            Self::Global(g) => g.try_upgrade(),
        }
    }

    unsafe fn unlock_exclusive(&self)
    {
        match self {
            Self::Local(l) => l.unlock_exclusive(),
            Self::Global(g) => g.unlock_exclusive(),
        }
    }

    unsafe fn unlock_shared(&self)
    {
        match self {
            Self::Local(l) => l.unlock_shared(),
            Self::Global(g) => g.unlock_shared(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct LocalCounter
{
    lock: Cell<i32>,
    generation: Cell<u64>,
}

impl Tracking for LocalCounter
{
    fn generation(&self) -> u64 { self.generation.get() & RawRef::<()>::COUNTER_MASK }

    fn invalidate(&self) -> u64
    {
        let current = self.generation.get();
        self.generation.set(current + 1);
        current & RawRef::<()>::COUNTER_MASK
    }

    fn try_lock_exclusive(&self) -> bool
    {
        if self.lock.get() == 0 {
            self.lock.set(-1);
            return true;
        } else {
            return false;
        }
    }

    fn lock_exclusive(&self)
    {
        if !self.try_lock_exclusive() {
            panic!("unconditional locking operation on locked local counter")
        }
    }

    fn try_lock_shared(&self) -> bool
    {
        if self.lock.get() >= 0 {
            self.lock.set(self.lock.get() + 1);
            return true;
        } else {
            return false;
        }
    }

    fn try_upgrade(&self) -> bool
    {
        if self.lock.get() == 1 {
            self.lock.set(-1);
            return true;
        } else {
            return false;
        }
    }

    unsafe fn unlock_exclusive(&self)
    {
        if self.lock.get() >= 1 {
            panic!("unlock_exclusive on share-locked local tracker");
        } else if self.lock.get() == 0 {
            panic!("unlock_exclusive on unlocked local tracker");
        }
        self.lock.set(0);
    }

    unsafe fn unlock_shared(&self)
    {
        if self.lock.get() < 0 {
            panic!("unlock_shared on exclusive-locked local tracker");
        } else if self.lock.get() == 0 {
            panic!("unlock_shared on unlocked local tracker");
        }
        self.lock.set(self.lock.get() - 1);
    }
}

use bumpalo::Bump;
thread_local! {
    static ARENA : RefCell<Bump> = RefCell::new(Bump::new());
    static FREE_LIST : RefCell<Vec<LocalIndex>> = RefCell::new(Vec::new());
}

pub(crate) fn allocate() -> LocalIndex { recycle().unwrap_or_else(fresh) }

fn fresh() -> LocalIndex
{
    ARENA.with_borrow_mut(|arena| {
        LocalIndex(NonNull::from(arena.alloc(RefCell::new(
            LocalAccount::Local(LocalCounter {
                lock: 0.into(),
                generation: RawRef::<()>::COUNTER_INIT.into(),
            }),
        ))))
    })
}

fn recycle() -> Option<LocalIndex> { FREE_LIST.with_borrow_mut(|vec| vec.pop()) }

pub(crate) fn free(li: LocalIndex) { FREE_LIST.with_borrow_mut(|vec| vec.push(li)) }
