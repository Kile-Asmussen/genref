use lock_api::RawRwLock;
use parking_lot::Mutex;
use std::sync::atomic::{self, AtomicUsize, Ordering::*};

trait Empty: Send + Sync {}
impl<T: Send + Sync> Empty for T {}

crate::counter::macros::counter_module!(
    scope: thread_local,
    Counter: &'static AtomicUsize,
    ptr: ptr,
    val: ptr.load(Relaxed),
    bump: ptr.fetch_add(1, Relaxed),

    Allocator: &'static [AtomicUsize],
    name: alloc,
    init: {
        let mut v = Vec::with_capacity(32);
        for _ in 0..32 { v.push(AtomicUsize::new(1)) }
        v.leak()
    },
    len: alloc.queue.len(),
    expand: {
        let mut v = Vec::with_capacity(alloc.next + alloc.next/2);
        for _ in 0..v.capacity() { v.push(AtomicUsize::new(1)) }
    },
    next: &alloc.queue[alloc.next],

    Lock: parking_lot::RawRwLock,
    init: parking_lot::RawRwLock::INIT,
    name: l,

    Writing: l.0.try_lock_exclusive(),
    get: { Lock::with_instance_ref(|l| l.0.lock_exclusive()); Writing(()) },
    drop: unsafe { l.0.unlock_exclusive() },

    Reading: l.0.try_lock_shared(),
    get: { Lock::with_instance_ref(|l| l.0.lock_shared()); Reading(()) },
    drop: unsafe { l.0.unlock_shared() },
    bound: Empty,
    later: dyn Empty,
);
