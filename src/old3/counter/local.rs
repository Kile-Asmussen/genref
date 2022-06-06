#[macro_use]
use crate::counter;

use std::{
    cell::{Cell, RefCell},
    ptr::NonNull,
};

trait Empty {}
impl<T> Empty for T {}

crate::counter::macros::counter_module!(
    scope: thread_local,
    Counter: NonNull<Cell<usize>>,
    ptr: ptr,
    val: unsafe { ptr.as_ref() }.get(),
    bump: {
        let c = unsafe { ptr.as_ref() };
        let res = c.get();
        c.set(res.wrapping_add(1));
        res
    },

    Allocator: Vec<Vec<Cell<usize>>>,
    name: alloc,
    init: vec![vec![Cell::new(1); 32]],
    len: alloc.queue[alloc.queue.len()-1].len(),
    expand: {
        let next = vec![Cell::new(1); alloc.next + alloc.next/2];
        alloc.queue.push(next);
    },
    next: NonNull::new(&alloc.queue[alloc.queue.len()-1][alloc.next] as *const _ as *mut _).unwrap(),

    Lock: Cell<isize>,
    init: Cell::new(0),
    name: l,

    Writing: {
        if l.0.get() == 0 {
            l.0.set(-1);
            true
        } else { false }
    },
    get: try_write().unwrap(),
    drop: l.0.set(0),

    Reading: {
        if l.0.get() >= 0 {
            l.0.set(l.0.get()+1);
            true
        } else { false }
    },
    get: try_read().unwrap(),
    drop: l.0.set(l.0.get()-1),
    bound: Empty,
    later: dyn Empty,
);
