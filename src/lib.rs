//! A rust implementation of the Vale generational counter memory model.
//!
//! [Vale](https://vale.dev/) boasts an innovative memory model that brokers
//! a compromise between Rust's static ownership model and reference counting.
//! Seeing as it is a good fit for rust's existing infrastructure of RAII
//! libraries, I decided to provide it as a library for programming language
//! design.
//!
//! This crate provides managed pointer types `Uniq`, `Owned`, `Weak`, and
//! `Guard`, as well as thread-local and global infrastructure for managing the
//! allocations.
//!
//! Caveat: the 'pointer' types are not actually pointers-as-such and do not
//! support unsized types. Due to implementation details, having this kind of
//! support would probably be detrimental to performance.
//!
//! A generational allocation consists of a generation counter and the allocated
//! data. The generation counter is only incremented when an allocation is freed
//! (and the data thus `drop`ped.) Weak references keep a local copy of the
//! generation for which they are valid, and are invalidated if their generation
//! counter does not match that of their allocation.
//!
//! Allocations, by necessity, persist forever (they have `'static` lifetime)
//! since a weak reference might persist indefinitely and check the generation
//! count.
//!
//! Additionally if a generation counter would overflow, the allocation is
//! instead leaked. This is however unlikely to be a problem as it requires
//! allocating and freeing `usize::MAX` objects to leak one (1) allocation,
//! which takes a nontrivial amount of time.
//!
//! There are a number of pathological cases one needs to be aware of, however:
//!
//! - It is inadvisable to persist a `Guard` for longer than strictly necessary,
//!   since it will prevent running `drop` on deallocated objects and prevent
//!   RAII mechanics.
//! - This means keeping a `Guard` in scope over an `await` call can potentially
//!   affect multiple unrelated futures' allocation behavior.
//! - Since memory is never tuly freed, allocating a very large number of
//!   objects will cause the memory footprint of the program to bloat.
//! - Threads will attempt to request batches of free objects from the global
//!   pool until such a time as the global pool is empty, and from then on will
//!   allocate new or re-use only locally allocated objects. This may lead to a
//!   thread allocating new despite the global pool being glutted with free
//!   objects.

#![feature(assert_matches)]

#[macro_use]
#[allow(unused_macros, dead_code)]
pub(crate) mod debug;
pub(crate) mod allocator;
pub(crate) mod axioms;
pub(crate) mod generations;
pub(crate) mod pointers;

#[cfg(test)]
#[allow(unused_imports)]
mod tests;

#[allow(unused_imports)]
pub use allocator::{global_stats, reset_request_behavior, thread_local_stats, Stats};
#[allow(unused_imports)]
pub use generations::GenerationLayout;
#[allow(unused_imports)]
pub use pointers::{Owned, Uniq, Weak};

#[allow(unused_imports)]
pub use axioms::Axioms;
