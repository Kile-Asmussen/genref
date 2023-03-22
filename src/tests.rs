#[cfg(test)]
use std::mem;

#[cfg(test)]
use std::{assert_matches::assert_matches, thread};

#[cfg(test)]
use parking_lot::Mutex;

#[cfg(test)]
use crate::counter::LocalGeneration;

#[cfg(test)]
use crate::counter::*;

#[cfg(test)]
use crate::pointers::*;

#[cfg(test)]
use super::{Sending, Sharing, Strong, Transferrable, Universal, UniversalEnum, Weak};

#[test]
fn local_allocation_single()
{
    for _ in 0..100 {
        LocalGeneration::free(LocalGeneration::new());
    }

    assert_eq!(LocalGeneration::free_list_size(), 1);
}

#[test]
fn local_allocation_multi()
{
    let mut v = Vec::new();

    for _ in 0..100 {
        v.push(LocalGeneration::new())
    }

    for g in v.drain(..) {
        LocalGeneration::free(g)
    }

    for _ in 0..100 {
        v.push(LocalGeneration::new())
    }

    for g in v.drain(..) {
        LocalGeneration::free(g)
    }

    assert_eq!(LocalGeneration::free_list_size(), 100);
}

#[cfg(test)]
static GLOBAL_TEST: Mutex<()> = Mutex::new(());

#[test]
fn global_allocation_single()
{
    let _lock = GLOBAL_TEST.lock();

    fn thread()
    {
        for _ in 0..100 {
            GlobalGeneration::free(GlobalGeneration::new());
        }
    }

    let a = thread::spawn(thread);
    let b = thread::spawn(thread);

    let _ = a.join();
    let _ = b.join();

    assert_matches!(GlobalGeneration::free_list_size(), 1 | 2);

    GlobalGeneration::leak_all_and_reset();
}

#[test]
fn global_allocation_multi()
{
    let _lock = GLOBAL_TEST.lock();

    fn thread()
    {
        let mut v = Vec::new();

        for _ in 0..100 {
            v.push(GlobalGeneration::new())
        }

        for g in v.drain(..) {
            GlobalGeneration::free(g)
        }

        for _ in 0..100 {
            v.push(GlobalGeneration::new())
        }

        for g in v.drain(..) {
            GlobalGeneration::free(g)
        }
    }

    let a = thread::spawn(thread);
    let b = thread::spawn(thread);

    let _ = a.join();
    let _ = b.join();

    assert_matches!(GlobalGeneration::free_list_size(), 100..=200);

    GlobalGeneration::leak_all_and_reset();
}

#[test]
fn local_locking()
{
    unsafe {
        let x = LocalGeneration::new();

        assert!(x.try_lock_shared());
        assert!(x.try_lock_shared());
        assert!(!x.try_lock_exclusive());
        x.unlock_shared();
        x.unlock_shared();

        assert!(x.try_lock_exclusive());
        assert!(!x.try_lock_shared());
        assert!(!x.try_lock_exclusive());
        x.unlock_exclusive();

        assert!(x.try_lock_shared());
        assert!(x.try_shared_into_exclusive());
        assert!(!x.try_lock_shared());
        assert!(!x.try_lock_exclusive());
        x.unlock_exclusive();

        assert!(x.try_lock_exclusive());
        x.downgrade();
        assert!(x.try_lock_shared());
        x.unlock_shared();
    }
}

#[test]
fn global_locking()
{
    let _lock = GLOBAL_TEST.lock();
    unsafe {
        let x = GlobalGeneration::new();

        assert!(x.try_lock_shared());
        assert!(x.try_lock_shared());
        assert!(!x.try_lock_exclusive());
        x.unlock_shared();
        x.unlock_shared();

        assert!(x.try_lock_exclusive());
        assert!(!x.try_lock_shared());
        assert!(!x.try_lock_exclusive());
        x.unlock_exclusive();

        assert!(x.try_lock_shared());
        assert!(x.try_shared_into_exclusive());
        assert!(!x.try_lock_shared());
        assert!(!x.try_lock_exclusive());
        x.unlock_exclusive();

        assert!(x.try_lock_exclusive());
        x.downgrade();
        assert!(x.try_lock_shared());
        x.unlock_shared();
    }
    GlobalGeneration::leak_all_and_reset();
}

#[test]
fn lock_state_transfers()
{
    let _lock = GLOBAL_TEST.lock();

    let l = LocalGeneration::new();

    l.try_lock_shared();

    let g = l.globalize();

    assert!(!g.try_lock_exclusive());
    assert!(g.try_lock_upgradable());
    assert!(g.try_lock_shared());

    let l = LocalGeneration::new();
    l.try_lock_upgradable();
    let g = l.globalize();

    assert!(!g.try_lock_exclusive());
    assert!(!g.try_lock_upgradable());
    assert!(g.try_lock_shared());

    let l = LocalGeneration::new();
    l.try_lock_exclusive();
    let g = l.globalize();

    assert!(!g.try_lock_exclusive());
    assert!(!g.try_lock_upgradable());
    assert!(!g.try_lock_shared());

    GlobalGeneration::leak_all_and_reset();
}

#[test]
fn globalize_memoizes()
{
    let _lock = GLOBAL_TEST.lock();

    let l = LocalGeneration::new();

    let g1 = l.globalize();
    let g2 = l.globalize();

    assert_eq!(g1.0 as *const _ as usize, g2.0 as *const _ as usize);

    GlobalGeneration::leak_all_and_reset();
}

#[test]
fn globalized_local_redirects()
{
    let _lock = GLOBAL_TEST.lock();

    let l = LocalGeneration::new();
    let g = l.globalize();

    l.try_lock_shared();
    assert!(g.try_lock_upgradable());
    assert!(!g.try_lock_exclusive());

    let l = LocalGeneration::new();
    let g = l.globalize();

    l.try_lock_exclusive();
    assert!(!g.try_lock_shared());
    assert!(!g.try_lock_upgradable());
    assert!(!g.try_lock_exclusive());

    let l = LocalGeneration::new();
    let g = l.globalize();

    l.try_lock_upgradable();
    assert!(g.try_lock_shared());
    assert!(!g.try_lock_upgradable());
    assert!(!g.try_lock_exclusive());

    let l = LocalGeneration::new();
    let g = l.globalize();

    l.try_lock_exclusive();
    assert!(!g.try_lock_shared());
    assert!(!g.try_lock_upgradable());
    assert!(!g.try_lock_exclusive());

    GlobalGeneration::leak_all_and_reset();
}

#[test]
fn strong_reading()
{
    let s = Strong::new(1u32);

    let p = s.try_read().unwrap();

    let q = s.try_read().unwrap();

    assert_eq!(*p, *q);

    assert!(s.try_write().is_none());
}

#[test]
fn strong_writing()
{
    let s = Strong::new(1u32);

    let mut p = s.try_write().unwrap();

    assert_eq!(*p, 1);

    assert!(s.try_read().is_none());

    *p = 2;

    mem::drop(p);

    let q = s.try_read().unwrap();

    assert_eq!(*q, 2);
}

#[test]
fn weak_reading()
{
    let _s = Strong::new(1u32);
    let s = _s.alias();

    let p = s.try_read().unwrap();

    let q = s.try_read().unwrap();

    assert_eq!(*p, *q);

    assert!(s.try_write().is_none());
}

#[test]
fn weak_writing()
{
    let _s = Strong::new(1u32);
    let s = _s.alias();

    let mut p = s.try_write().unwrap();

    assert_eq!(*p, 1);

    assert!(s.try_read().is_none());

    *p = 2;

    mem::drop(p);

    let q = s.try_read().unwrap();

    assert_eq!(*q, 2);
}

#[test]
fn shared_access()
{
    let s = Strong::new(1);
    let w = s.alias();

    let p = s.try_read().unwrap();
    let q = w.try_read().unwrap();

    assert_eq!(*p, *q);
}

#[test]
fn exclusive_access()
{
    let s = Strong::new(1);
    let w = s.alias();

    {
        let _p = s.try_read().unwrap();
        let _q = w.try_read().unwrap();
    }

    {
        let _p = w.try_read().unwrap();
        let _q = s.try_read().unwrap();
    }

    {
        let _p = s.try_write().unwrap();
        assert!(w.try_read().is_none());
    }

    {
        let _p = w.try_write().unwrap();
        assert!(s.try_read().is_none());
    }

    {
        let _p = s.try_read().unwrap();
        assert!(w.try_write().is_none());
    }

    {
        let _p = w.try_read().unwrap();
        assert!(s.try_write().is_none());
    }
}

#[test]
fn ownership_bit_is_correct()
{
    let _lock = GLOBAL_TEST.lock();

    let s = Strong::new(0);
    let q = Strong::new(0);
    let w = q.alias();

    assert_eq!(s.0.ownership(), OwnershipBit::Strong);
    assert_eq!(w.0.ownership(), OwnershipBit::Weak);

    assert_eq!(Strong::from(s.send()).0.ownership(), OwnershipBit::Strong);
    assert_eq!(Weak::from(w.share()).0.ownership(), OwnershipBit::Weak);
}
