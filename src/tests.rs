
use std::{cell::Cell};

use crate::genref::{generations::InUsePtr, pointers::Owned, allocator};

#[test]
fn generation_marker() {
    let alloc = InUsePtr::allocate(0);
    let gen1 = alloc.generation();
    unsafe { alloc.upcast().unwrap().downcast::<i32>(2); }
    let gen2 = alloc.generation();
    assert_eq!(gen1 + 2, gen2);
} 

#[test]
fn allocation() {

    assert_eq!(allocator::get_stats().guards, 0);

    let x = Owned::new(Cell::new(2i32));

    let y = x.alias();

    let z = y.try_ref();
    assert!(z.is_some());
    let z = z.unwrap();

    assert_eq!(allocator::get_stats().guards, 1);

    assert_eq!(z.get(), 2);

    z.set(3i32);

    let q = x.alias();
    assert_eq!(q.try_ref().map(|z| z.get()), Some(3));

    let x = match x.try_take() {
        Ok(_) => { assert!(false, "impossible"); return },
        Err(x) => x
    };

    std::mem::drop(z);
    assert_eq!(allocator::get_stats().guards, 0);

    let _ = match x.try_take() {
        Ok(i) => i,
        Err(_) => { assert!(false, "impossible"); return }
    };

    assert!(y.try_ref().is_none());

    assert!(!allocator::get_stats().by_layout.is_empty());
}
