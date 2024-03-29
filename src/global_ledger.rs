use lazy_static::lazy_static;
use lock_api::{RawRwLock, RawRwLockUpgrade};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::tracking::Tracking;

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct GlobalIndex(&'static GlobalAccount);

impl Tracking for GlobalIndex
{
    fn generation(&self) -> u64 { self.0.generation() }
    fn invalidate(&self) -> u64 { self.0.invalidate() }
    fn try_lock_exclusive(&self) -> bool { self.0.try_lock_exclusive() }
    fn lock_exclusive(&self) { self.0.lock_exclusive() }
    fn try_lock_shared(&self) -> bool { self.0.try_lock_shared() }
    fn try_upgrade(&self) -> bool { self.0.try_upgrade() }
    unsafe fn unlock_exclusive(&self) { self.0.unlock_exclusive() }
    unsafe fn unlock_shared(&self) { self.0.unlock_shared() }
}

struct GlobalAccount
{
    lock: parking_lot::RawRwLock,
    generation: AtomicU64,
}

impl std::fmt::Debug for GlobalAccount
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
    {
        f.debug_struct("GlobalAccount")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl Tracking for GlobalAccount
{
    fn generation(&self) -> u64
    {
        self.generation.load(Ordering::Relaxed) & RawRef::<()>::COUNTER_MASK
    }

    fn invalidate(&self) -> u64 { self.generation.fetch_add(1, Ordering::Relaxed) }

    fn try_lock_exclusive(&self) -> bool { self.lock.try_lock_exclusive() }

    fn lock_exclusive(&self) { self.lock.lock_exclusive() }

    fn try_lock_shared(&self) -> bool { self.lock.try_lock_shared() }

    fn try_upgrade(&self) -> bool
    {
        if self.lock.try_lock_upgradable() {
            unsafe {
                self.lock.unlock_shared();
            }
            if unsafe { self.lock.try_upgrade() } {
                return true;
            }
            if !self.lock.try_lock_shared() {
                panic!("failed to upgrade and then could not re-lock")
            }
            unsafe {
                self.lock.unlock_upgradable();
            }
        }
        return false;
    }

    unsafe fn unlock_exclusive(&self) { self.lock.unlock_exclusive() }

    unsafe fn unlock_shared(&self) { self.lock.unlock_shared() }
}

pub(crate) fn allocate() -> GlobalIndex { recycle().unwrap_or_else(fresh) }

fn fresh() -> GlobalIndex
{
    GlobalIndex(Box::leak(Box::new(GlobalAccount {
        lock: parking_lot::RawRwLock::INIT,
        generation: AtomicU64::new(RawRef::<()>::COUNTER_INIT),
    })) as &_)
}

lazy_static! {
    static ref FREE_LIST: parking_lot::RwLock<Vec<GlobalIndex>> =
        parking_lot::RwLock::new(Vec::new());
}

fn recycle() -> Option<GlobalIndex> { FREE_LIST.write().pop() }

/// assumes exclusive lock
pub(crate) unsafe fn free(gi: GlobalIndex)
{
    gi.invalidate();
    gi.unlock_exclusive();
    FREE_LIST.write().push(gi)
}
