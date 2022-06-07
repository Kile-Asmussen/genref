use crate::*;
use std::{assert_matches::assert_matches, mem, rc::Rc};

#[test]
fn many_readers()
{
    let a = reading();
    let b = reading();
    assert_matches!(a, Some(_));
    assert_matches!(b, Some(_));
}

#[test]
fn one_writer()
{
    let a = writing();
    let b = writing();
    assert_matches!(a, Some(_));
    assert_matches!(b, None);
}

#[test]
fn writer_excludes_reading()
{
    let a = writing();
    let b = reading();
    assert_matches!(a, Some(_));
    assert_matches!(b, None);
}

#[test]
fn reading_excludes_writing()
{
    let a = reading();
    let b = writing();
    assert_matches!(a, Some(_));
    assert_matches!(b, None);
}

#[test]
fn strong_take()
{
    let a = Strong::new(1);
    let mut w = writing().unwrap();

    assert_eq!(a.take(&mut w), Box::new(1));
}

#[test]
fn strong_reading()
{
    let a = Strong::new(1);
    let r = reading().unwrap();

    assert_eq!(*a.as_ref(&r), 1);
}

#[test]
fn weak_reading()
{
    let a = Strong::new(1);
    let b = a.alias();
    let r = reading().unwrap();

    assert_eq!(*b.try_as_ref(&r).unwrap(), 1);
}

#[test]
fn strong_writing()
{
    let mut a = Strong::new(1);

    let mut w = writing().unwrap();
    *a.as_mut(&mut w) = 2;

    assert_eq!(a.take(&mut w), Box::new(2));
}

#[test]
fn strong_writing_weak_reading()
{
    let mut a = Strong::new(1);
    let b = a.alias();

    let mut w = writing().unwrap();
    *a.as_mut(&mut w) = 2;
    mem::drop(w);

    let r = reading().unwrap();
    assert_eq!(*b.try_as_ref(&r).unwrap(), 2);
}

#[test]
fn weak_writing_strong_reading()
{
    let a = Strong::new(1);
    let mut b = a.alias();

    let mut w = writing().unwrap();
    *b.try_as_mut(&mut w).unwrap() = 2;
    mem::drop(w);

    let r = reading().unwrap();
    assert_eq!(*a.as_ref(&r), 2);
}

#[test]
fn weak_writing_weak_reading()
{
    let a = Strong::new(1);
    let mut b = a.alias();
    let c = b;

    let mut w = writing().unwrap();
    *b.try_as_mut(&mut w).unwrap() = 2;
    mem::drop(w);

    let r = reading().unwrap();
    assert_eq!(*c.try_as_ref(&r).unwrap(), 2);
}

struct DropInc(Rc<Cell<i32>>);
impl Drop for DropInc
{
    fn drop(&mut self) { self.0.set(self.0.get() + 1) }
}

#[test]
fn drop_later_if_reading()
{
    let x = Rc::new(Cell::new(3));
    let a = Strong::new(DropInc(x.clone()));

    let r = reading().unwrap();

    mem::drop(a);

    assert_eq!(x.get(), 3);

    mem::drop(r);

    assert_eq!(x.get(), 4);
}

#[test]
fn drop_later_if_writing()
{
    let x = Rc::new(Cell::new(3));
    let a = Strong::new(DropInc(x.clone()));

    let w = writing().unwrap();

    mem::drop(a);

    assert_eq!(x.get(), 3);

    mem::drop(w);

    assert_eq!(x.get(), 4);
}

#[test]
fn drop_invalidates()
{
    let a = Strong::new(1);
    let b = a.alias();

    mem::drop(a);

    assert!(!b.is_valid());
}

#[test]
fn reading_invalid_weak_fails()
{
    let a = Strong::new(1);
    let b = a.alias();

    mem::drop(a);

    let r = reading().unwrap();

    assert_matches!(b.try_as_ref(&r), None);
}

#[test]
fn writing_invalid_weak_fails()
{
    let a = Strong::new(1);
    let mut b = a.alias();

    mem::drop(a);

    let mut w = writing().unwrap();

    assert_matches!(b.try_as_mut(&mut w), None);
}
