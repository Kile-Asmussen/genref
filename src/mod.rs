mod global_ledger;
mod local_ledger;
mod raw_ref;

use raw_ref::*;

struct Strong<T>(RawRef<T>);

impl<T> Strong<T>
{
    #[cfg(test)]
    fn invaraint(self) -> Self
    {
        assert!(
            self.0.is_non_nil(),
            "nil rawref encapsulated as strong reference"
        );
        match self.0.pointer() {
            PointerEnum::Strong(_) => (),
            _ => panic!("weak rawref encapsulated as strong reference"),
        }
    }

    #[cfg(not(test))]
    fn invariant(self) -> Self { self }

    fn new_from_box(it: Box<T>) { Self(RawRef::new_from_box(it)).invariant() }

    fn alias(&self) -> Weak<T> { Weak(self.0.as_weak()).invariant() }

    fn try_take(&self) -> Box<T> { todo!() }

    fn try_read(&self) -> Reading<T> { todo!() }

    fn try_write(&self) -> Writing<T> { todo!() }
}

#[derive(Clone, Copy)]
struct Weak<T>(RawRef<T>);

impl<T> Weak<T>
{
    fn invariant(&self) -> Self { todo!() }

    fn try_read(&self) -> Reading<T> { todo!() }

    fn try_write(&self) -> Writing<T> { todo!() }
}
struct GenRef<T>(RawRef<T>);
enum GenRefEnum<T>
{
    Weak(Weak<T>),
    Strong(Strong<T>),
}

struct Reading<T>(RawRef<T>);
struct Writing<T>(RawRef<T>);

struct Sendable<T>(Strong<T>);
struct Shareable<T>(Weak<T>);
struct Transferrable<T>(GenRef<T>);
enum TransferrableEnum<T>
{
    Sendable(Sendable<T>),
    Shareable(Shareable<T>),
}
