use std::{mem, ptr::NonNull};

use super::{
    global_ledger::GlobalIndex,
    local_ledger::{self, LocalIndex},
};

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
union Account
{
    local: LocalIndex,
    global: GlobalIndex,
}

#[derive(Clone, Copy)]
pub(crate) enum AccountEnum
{
    Nil,
    Local(LocalIndex),
    Global(GlobalIndex),
}

impl Tracking for AccountEnum
{
    fn generation(&self) -> u64
    {
        match self {
            Nil => 0,
            Self::Local(l) => l.generation(),
            Self::Global(g) => g.generation(),
        }
    }

    fn invalidate(&self) -> u64
    {
        match self {
            Nil => 0,
            Self::Local(l) => l.invalidate(),
            Self::Global(g) => g.invalidate(),
        }
    }

    fn try_lock_exclusive(&self) -> bool
    {
        match self {
            Nil => false,
            Self::Local(l) => l.try_lock_exclusive(),
            Self::Global(g) => g.try_lock_exclusive(),
        }
    }

    fn lock_exclusive(&self)
    {
        match self {
            Nil => (),
            Self::Local(l) => l.lock_exclusive(),
            Self::Global(l) => l.lock_exclusive(),
        }
    }

    fn try_lock_shared(&self) -> bool
    {
        match self {
            Nil => false,
            Self::Local(l) => l.try_lock_shared(),
            Self::Global(g) => g.try_lock_shared(),
        }
    }

    fn try_upgrade(&self) -> bool
    {
        match self {
            Nil => false,
            Self::Local(l) => l.try_upgrade(),
            Self::Global(g) => g.try_upgrade(),
        }
    }

    unsafe fn unlock_exclusive(&self)
    {
        match self {
            Nil => (),
            Self::Local(l) => l.unlock_exclusive(),
            Self::Global(g) => g.unlock_exclusive(),
        }
    }

    unsafe fn unlock_shared(&self)
    {
        match self {
            Nil => (),
            Self::Local(l) => l.unlock_shared(),
            Self::Global(g) => g.unlock_shared(),
        }
    }
}

#[repr(C)]
pub(crate) struct RawRef<T>
{
    account: Option<Account>,
    pointer: Option<NonNull<T>>,
    generation: u64,
}

impl<T> Clone for RawRef<T>
{
    fn clone(&self) -> Self
    {
        Self {
            account: self.account.clone(),
            pointer: self.pointer.clone(),
            generation: self.generation.clone(),
        }
    }
}
impl<T> Copy for RawRef<T> {}

pub(crate) enum PointerEnum<T>
{
    Nil,
    Weak(NonNull<T>),
    Strong(NonNull<T>),
}

impl<T> RawRef<T>
{
    fn nil() -> Self
    {
        RawRef {
            account: None,
            pointer: None,
            generation: 0,
        }
    }

    fn is_nil(self) -> bool
    {
        self.generation == 0 && self.account.is_none() && self.pointer.is_none()
    }

    pub(crate) fn is_non_nil(self) -> bool
    {
        self.generation != 0 && self.account.is_some() && self.pointer.is_some()
    }

    #[cfg(test)]
    fn invariant(self) -> Self
    {
        if self.generation == 0 {
            assert!(
                self.account.is_none(),
                "zero generation on reference with non-nil account"
            );
            assert!(
                self.pointer.is_none(),
                "zero generation on reference with non-nil pointer"
            );
            return self;
        }

        let reference = self.generation & Self::REFERENCE_MASK;
        let account = self.generation & Self::ACCOUNT_MASK;
        let counter = self.generation & Self::COUNTER_MASK;

        assert!(counter != 0, "flags set on nil generation count");
        assert!(account != 0, "no account flag on positive generation count");
        assert!(account != Self::ACCOUNT_MASK, "saturated account flags");
        assert!(
            reference != 0,
            "no reference flag on positive generation count"
        );
        assert!(
            reference != Self::REFERENCE_MASK,
            "saturated reference flags"
        );
        assert!(
            self.account.is_some(),
            "nonzero generation on reference with nil account"
        );
        assert!(
            self.pointer.is_some(),
            "nonzero generation on reference with nil pointer"
        );
        self
    }

    #[cfg(not(test))]
    fn invariant(self) -> Self { self }

    fn new_from_parts(acc: AccountEnum, ptr: PointerEnum<T>) -> Self
    {
        let (account, acc_flag) = match acc {
            AccountEnum::Nil => (None, 0),
            AccountEnum::Local(local) => (Some(Account { local }), Self::LOCAL_ACCOUNT),
            AccountEnum::Global(global) => (Some(Account { global }), Self::GLOBAL_ACCOUNT),
        };
        let (pointer, ref_flag) = match ptr {
            PointerEnum::Nil => (None, 0),
            PointerEnum::Weak(p) => (Some(p), Self::WEAK_REFERENCE),
            PointerEnum::Strong(p) => (Some(p), Self::STRONG_REFERENCE),
        };
        let generation = acc.generation() | acc_flag | ref_flag;
        let res = RawRef {
            account,
            pointer,
            generation,
        };
        res.invariant()
    }

    pub(crate) fn new_from_box(mut it: Box<T>) -> Self
    {
        let res = Self::new_from_parts(
            AccountEnum::Local(local_ledger::allocate()),
            PointerEnum::Strong(NonNull::from(it.as_mut())),
        );
        mem::forget(it);
        res.invariant()
    }

    pub(crate) fn account(self) -> AccountEnum
    {
        self.invariant();
        if let Some(a) = self.account {
            match self.generation & Self::ACCOUNT_MASK {
                GLOBAL_ACCOUNT => AccountEnum::Global(unsafe { a.global }),
                LOCAL_ACCOUNT => AccountEnum::Local(unsafe { a.local }),
                _ => panic!(),
            }
        } else {
            AccountEnum::Nil
        }
    }

    pub(crate) fn pointer(self) -> PointerEnum<T>
    {
        if let Some(p) = self.invariant().pointer {
            match self.generation & Self::REFERENCE_MASK {
                STRONG_REFERENCE => PointerEnum::Strong(p),
                WEAK_REFERENCE => PointerEnum::Weak(p),
                _ => panic!(),
            }
        } else {
            PointerEnum::Nil
        }
    }

    pub(crate) fn as_weak(mut self) -> Self
    {
        self.invariant();
        self.generation &= !Self::REFERENCE_MASK;
        self.generation |= Self::WEAK_REFERENCE;
        self.invariant();
        self
    }

    fn as_global(mut self) -> Self
    {
        self.invariant();
        self.generation &= !Self::ACCOUNT_MASK;
        self.generation |= Self::GLOBAL_ACCOUNT;
        self.invariant()
    }

    fn counter(self) -> u64 { self.generation & Self::COUNTER_MASK }

    const FLAG_MASK: u64 = 0b1111u64.reverse_bits();
    pub(crate) const COUNTER_MASK: u64 = !Self::FLAG_MASK;
    pub(crate) const COUNTER_INIT: u64 = 1;
    const GLOBAL_ACCOUNT: u64 = 0b0001u64.reverse_bits();
    const LOCAL_ACCOUNT: u64 = 0b0010u64.reverse_bits();
    const ACCOUNT_MASK: u64 = Self::GLOBAL_ACCOUNT | Self::LOCAL_ACCOUNT;
    const STRONG_REFERENCE: u64 = 0b0100u64.reverse_bits();
    const WEAK_REFERENCE: u64 = 0b1000u64.reverse_bits();
    const REFERENCE_MASK: u64 = Self::STRONG_REFERENCE | Self::WEAK_REFERENCE;
}
