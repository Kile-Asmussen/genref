use std::cell::Cell;

use crate::allocator::*;
use crate::generations::*;
use crate::pointers::*;

#[test]
fn user_story()
{
    assert_eq!(thread_local_stats().guards, 0);

    let x = Owned::new(Cell::new(2i32));

    let y = x.alias();

    let z = y.try_ref();

    assert!(z.is_some());

    let z = z.unwrap();

    assert_eq!(thread_local_stats().guards, 1);

    assert_eq!(z.get(), 2);

    z.set(3i32);

    let q = x.alias();

    assert_eq!(q.try_ref().map(|z| z.get()), Some(3));

    let x = match x.try_into_inner() {
        Ok(_) => {
            assert!(false, "impossible");
            return;
        }
        Err(x) => x,
    };

    std::mem::drop(z);

    assert_eq!(thread_local_stats().guards, 0);

    let _ = match x.try_into_inner() {
        Ok(i) => i,
        Err(_) => {
            assert!(false, "impossible");
            return;
        }
    };

    assert!(y.try_ref().is_none());

    assert!(!thread_local_stats().by_layout.is_empty());
}

#[test]
fn stress_test()
{
    let n = 500;
    for _ in 0..n {
        let mut x = Uniq::<Vec<Owned<i32>>>::new(Vec::<Owned<i32>>::new());

        for j in 0..n {
            x.push(Owned::new(j));
        }
    }
    assert_eq!(thread_local_stats().free_objects(), n as usize + 1);
}

#[test]
fn stress_test2()
{
    let n = 10;
    for _ in 0..n {
        let mut x = Uniq::<Vec<Uniq<Vec<Uniq<i32>>>>>::new(Vec::new());

        for _ in 0..n {
            let mut y = Uniq::new(Vec::new());

            for j in 0..n {
                y.push(Uniq::new(j));
            }

            x.push(y);
        }
    }
    assert_eq!(
        thread_local_stats().free_objects(),
        (n * n + n + 1) as usize
    );
}
