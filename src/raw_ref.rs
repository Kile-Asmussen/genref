use std::{mem, num::NonZeroU64, ptr::NonNull};

use crate::tracking::{self, Account, AccountEnum};

use super::{
    global_ledger::GlobalIndex,
    local_ledger::{self},
    tracking::*,
};

pub(crate) enum PointerEnum<T>
{
    Weak(NonNull<T>),
    Strong(NonNull<T>),
}

impl<T> Clone for PointerEnum<T>
{
    fn clone(&self) -> Self
    {
        match self {
            Self::Weak(arg0) => Self::Weak(arg0.clone()),
            Self::Strong(arg0) => Self::Strong(arg0.clone()),
        }
    }
}

impl<T> std::fmt::Debug for PointerEnum<T>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
    {
        match self {
            Self::Weak(arg0) => f.debug_tuple("Weak").field(arg0).finish(),
            Self::Strong(arg0) => f.debug_tuple("Strong").field(arg0).finish(),
        }
    }
}

impl<T> PointerEnum<T>
{
    pub(crate) fn as_ptr(self) -> NonNull<T>
    {
        match self {
            PointerEnum::Weak(p) => p,
            PointerEnum::Strong(p) => p,
        }
    }

    pub(crate) fn map<F, U>(self, f: F) -> PointerEnum<U>
    where
        F: FnOnce(NonNull<T>) -> NonNull<U>,
    {
        match self {
            Self::Weak(p) => PointerEnum::Weak(f(p)),
            Self::Strong(p) => PointerEnum::Strong(f(p)),
        }
    }
}

#[repr(C)]
pub(crate) struct RawRef<T>
{
    account: Account,
    pointer: NonNull<T>,
    generation: NonZeroU64,
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

impl<T> RawRef<T>
{
    #[cfg(test)]
    pub(crate) fn invariant(&self)
    {
        let reference = self.generation.get() & Self::REFERENCE_MASK;
        let account = self.generation.get() & Self::ACCOUNT_MASK;
        let counter = self.generation.get() & Self::COUNTER_MASK;

        assert_ne!(counter, 0, "flags set on nil generation count");
        assert_ne!(account, 0, "no account flag on positive generation count");
        assert_ne!(account, Self::ACCOUNT_MASK, "saturated account flags");
        assert_ne!(reference, 0, "no reference flag");
        assert_ne!(reference, Self::REFERENCE_MASK, "saturated reference flags");
    }

    #[cfg(not(test))]
    pub(crate) fn invariant(&self) {}

    fn new_from_parts(acc: AccountEnum, ptr: PointerEnum<T>) -> Self
    {
        let (account, acc_flag) = match acc {
            AccountEnum::Local(local) => (tracking::Account { local }, Self::LOCAL_ACCOUNT),
            AccountEnum::Global(global) => (tracking::Account { global }, Self::GLOBAL_ACCOUNT),
        };
        let (pointer, ref_flag) = match ptr {
            PointerEnum::Weak(p) => (p, Self::WEAK_REFERENCE),
            PointerEnum::Strong(p) => (p, Self::STRONG_REFERENCE),
        };
        let generation = NonZeroU64::new(acc.generation() | acc_flag | ref_flag).unwrap();
        let res = RawRef {
            account,
            pointer,
            generation,
        };
        res.invariant();
        res
    }

    pub(crate) fn from_box(mut it: Box<T>) -> Self
    {
        let res = Self::new_from_parts(
            AccountEnum::Local(local_ledger::allocate()),
            PointerEnum::Strong(NonNull::from(it.as_mut())),
        );
        mem::forget(it);
        res.invariant();
        res
    }

    unsafe fn try_consume(&self, locking_primitive: fn(&AccountEnum) -> bool) -> Option<Box<T>>
    {
        self.invariant();
        let account = self.account();
        if locking_primitive(&account) {
            tracking::free(account);
            Some(Box::from_raw(self.pointer().as_ptr().as_ptr()))
        } else {
            None
        }
    }

    pub(crate) unsafe fn try_consume_exclusive(&self) -> Option<Box<T>>
    {
        self.try_consume(AccountEnum::try_lock_exclusive)
    }

    pub(crate) unsafe fn try_consume_shared(&self) -> Option<Box<T>>
    {
        self.try_consume(AccountEnum::try_upgrade)
    }

    pub(crate) fn map<F, U>(self, f: F) -> RawRef<U>
    where
        F: FnOnce(NonNull<T>) -> NonNull<U>,
    {
        let res = RawRef::new_from_parts(self.account(), self.pointer().map(f));
        res.invariant();
        res
    }

    pub(crate) fn account(&self) -> AccountEnum
    {
        self.invariant();
        match self.generation.get() & Self::ACCOUNT_MASK {
            Self::GLOBAL_ACCOUNT => AccountEnum::Global(unsafe { self.account.global }),
            Self::LOCAL_ACCOUNT => AccountEnum::Local(unsafe { self.account.local }),
            _ => panic!(),
        }
    }

    pub(crate) fn pointer(&self) -> PointerEnum<T>
    {
        self.invariant();
        match self.generation.get() & Self::REFERENCE_MASK {
            Self::STRONG_REFERENCE => PointerEnum::Strong(self.pointer),
            Self::WEAK_REFERENCE => PointerEnum::Weak(self.pointer),
            _ => panic!(),
        }
    }

    pub(crate) fn set_weak(mut self) -> Self
    {
        self.invariant();
        self.generation =
            NonZeroU64::new((self.generation.get() & !Self::REFERENCE_MASK) | Self::WEAK_REFERENCE)
                .unwrap();
        self.invariant();
        self
    }

    fn set_global(mut self) -> Self
    {
        self.invariant();
        self.generation =
            NonZeroU64::new((self.generation.get() & !Self::ACCOUNT_MASK) | Self::GLOBAL_ACCOUNT)
                .unwrap();
        self.invariant();
        self
    }

    fn counter(self) -> u64 { self.generation.get() & Self::COUNTER_MASK }

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
