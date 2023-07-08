use crate::{global_ledger, local_ledger};

use super::global_ledger::GlobalIndex;

use super::local_ledger::LocalIndex;

pub(crate) trait Tracking
{
    fn generation(&self) -> u64;
    fn invalidate(&self) -> u64;
    fn try_lock_exclusive(&self) -> bool;
    fn lock_exclusive(&self);
    fn try_lock_shared(&self) -> bool;
    fn try_upgrade(&self) -> bool;
    unsafe fn unlock_exclusive(&self);
    unsafe fn unlock_shared(&self);
}

#[derive(Clone, Copy)]
pub(crate) union Account
{
    pub(crate) local: LocalIndex,
    pub(crate) global: GlobalIndex,
}

#[derive(Clone, Copy)]
pub(crate) enum AccountEnum
{
    Local(LocalIndex),
    Global(GlobalIndex),
}

impl Tracking for AccountEnum
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
            Self::Local(l) => l.lock_exclusive(),
            Self::Global(l) => l.lock_exclusive(),
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

pub(crate) unsafe fn free(ac: AccountEnum)
{
    match ac {
        AccountEnum::Local(l) => local_ledger::free(l),
        AccountEnum::Global(g) => global_ledger::free(g),
    }
}
