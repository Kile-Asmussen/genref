#![feature(local_key_cell_methods)]

mod global_ledger;
mod local_ledger;
mod raw_ref;

use raw_ref::*;

pub struct Strong<T>(RawRef<T>);

impl<T> Strong<T>
{
    #[cfg(test)]
    fn invariant(self) -> Self
    {
        assert!(
            self.0.is_non_nil(),
            "nil rawref encapsulated as strong reference"
        );
        match self.0.pointer() {
            PointerEnum::Strong(_) => {}
            _ => panic!("weak rawref encapsulated as strong reference"),
        };
        self
    }

    #[cfg(not(test))]
    fn invariant(self) -> Self { self }

    fn new_from_box(it: Box<T>) -> Self { Self(RawRef::new_from_box(it)).invariant() }

    fn alias(&self) -> Weak<T> { Weak(self.0.as_weak()).invariant() }

    fn try_take(&self) -> Box<T> { todo!() }

    fn try_read(&self) -> Reading<T> { todo!() }

    fn try_write(&self) -> Writing<T> { todo!() }
}

#[derive(Clone, Copy)]
pub struct Weak<T>(RawRef<T>);

impl<T> Weak<T>
{
    fn invariant(&self) -> Self { todo!() }

    fn try_read(&self) -> Reading<T> { todo!() }

    fn try_write(&self) -> Writing<T> { todo!() }
}

struct GenRef<T>(RawRef<T>);
pub enum GenRefEnum<T>
{
    Weak(Weak<T>),
    Strong(Strong<T>),
}

pub struct Reading<T>(RawRef<T>);
pub struct Writing<T>(RawRef<T>);

pub struct Sendable<T>(Strong<T>);
pub struct Shareable<T>(Weak<T>);
pub struct Transferrable<T>(GenRef<T>);
pub enum TransferrableEnum<T>
{
    Sendable(Sendable<T>),
    Shareable(Shareable<T>),
}
