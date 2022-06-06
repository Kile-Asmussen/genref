use lock_api::RawRwLock;
use parking_lot::RwLock;
use std::cell::Cell;

use crate::{
    counters::{FastCounter, GenCounter, SyncCounter},
    dropqueue::{DropQueue, LockedQueue},
    singleish::{MingleRef, Mingleton},
};

pub(crate) trait WorldLock: MingleRef + Mingleton<Instance = Self>
{
    type Later: ?Sized;
    type GarbageBin: DropQueue<WideLock = Self> + Mingleton + MingleRef;
    type Counter: GenCounter;

    fn reading() -> Self::ReadLock
    {
        Self::with_instance_ref(Self::acquire_read).expect("Cannot read")
    }

    fn writing() -> Self::WriteLock
    {
        Self::with_instance_ref(Self::acquire_write).expect("Cannot read")
    }

    fn try_read() -> Option<Self::ReadLock> { Self::with_instance_ref(Self::acquire_read) }

    fn try_write() -> Option<Self::WriteLock> { Self::with_instance_ref(Self::acquire_write) }

    type ReadLock: Clone;
    type WriteLock;

    fn acquire_read(&self) -> Option<Self::ReadLock>;
    fn acquire_write(&self) -> Option<Self::WriteLock>;
}

mingleton!(THREADLOCAL: ThreadlocalLock = ThreadlocalLock(Cell::new(0)));

pub(crate) trait DropLater {}
impl<T> DropLater for T {}
pub(crate) struct ThreadlocalLock(Cell<usize>);

impl WorldLock for ThreadlocalLock
{
    type Later = dyn DropLater;

    type GarbageBin = LockedQueue<Self>;

    type Counter = FastCounter;

    type ReadLock = ThreadlocalReadLock;

    type WriteLock = ThreadlocalWriteLock;

    fn acquire_read(&self) -> Option<Self::ReadLock>
    {
        if self.0.get() != usize::MAX {
            self.0.set(self.0.get() + 1);
            Some(ThreadlocalReadLock {})
        } else {
            None
        }
    }

    fn acquire_write(&self) -> Option<Self::WriteLock>
    {
        if self.0.get() == 0 {
            self.0.set(usize::MAX);
            Some(ThreadlocalWriteLock {})
        } else {
            None
        }
    }
}
pub(crate) struct ThreadlocalReadLock;

impl Clone for ThreadlocalReadLock
{
    fn clone(&self) -> Self
    {
        ThreadlocalLock::with_instance_ref(|c| c.0.set(c.0.get() + 1));
        Self {}
    }
}

impl Drop for ThreadlocalReadLock
{
    fn drop(&mut self) { ThreadlocalLock::with_instance_ref(|c| c.0.set(0)) }
}

pub struct ThreadlocalWriteLock;

impl Drop for ThreadlocalWriteLock
{
    fn drop(&mut self) { THREADLOCAL.with(|c| c.0.set(0)) }
}

pub(crate) trait SendSyncLater: Send + Sync + DropLater {}
impl<T: Send + Sync> SendSyncLater for T {}

pub struct GlobalLock(RwLock<()>);

mingleton!(static GLOBAL_LOCK : GlobalLock = GlobalLock(RwLock::new(())));

impl WorldLock for GlobalLock
{
    fn reading() -> Self::ReadLock { Self::with_instance(GlobalLock::wait_read) }

    fn writing() -> Self::WriteLock { Self::with_instance(GlobalLock::wait_write) }

    type ReadLock = GlobalReadLock;

    type WriteLock = GlobalWriteLock;

    type Counter = SyncCounter;

    type Later = dyn SendSyncLater;

    type GarbageBin = LockedQueue<Self>;

    fn acquire_read(&self) -> Option<Self::ReadLock>
    {
        if unsafe { self.0.raw() }.try_lock_shared() {
            Some(Self::ReadLock {})
        } else {
            None
        }
    }

    fn acquire_write(&self) -> Option<Self::WriteLock>
    {
        if unsafe { self.0.raw() }.try_lock_shared() {
            Some(Self::WriteLock {})
        } else {
            None
        }
    }
}

impl GlobalLock
{
    pub fn wait_read(&self) -> GlobalReadLock
    {
        unsafe { self.0.raw() }.lock_shared();
        GlobalReadLock {}
    }

    pub fn wait_write(&self) -> GlobalWriteLock
    {
        unsafe { self.0.raw() }.lock_shared();
        GlobalWriteLock {}
    }
}

pub struct GlobalReadLock;

impl Clone for GlobalReadLock
{
    fn clone(&self) -> Self { GlobalLock::reading() }
}

impl Drop for GlobalReadLock
{
    fn drop(&mut self) { GlobalLock::with_instance_ref(|gl| unsafe { gl.0.raw().unlock_shared() }) }
}

pub struct GlobalWriteLock;

impl Drop for GlobalWriteLock
{
    fn drop(&mut self)
    {
        GlobalLock::with_instance_ref(|gl| unsafe { gl.0.raw().unlock_exclusive() })
    }
}
