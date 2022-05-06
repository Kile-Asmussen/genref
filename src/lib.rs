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
//! count. Thus there are pathological cases where memory will leak,
//! particularly if a thread allocates a very large number of objects, this will
//! permanently increase the memory footprint of the program.
//!
//! Additionally if a generation counter would overflow, the allocation is
//! instead leaked. This is however unlikely to be a problem as it requires
//! allocating and freeing `usize::MAX` objects to leak one (1) allocation,
//! which takes a nontrivial amount of time.

pub(crate) mod allocator;
pub(crate) mod generations;
pub(crate) mod pointers;

#[allow(unused_imports)]
pub use allocator::{global_stats, thread_local_stats, Stats};
#[allow(unused_imports)]
pub use generations::GenerationLayout;
#[allow(unused_imports)]
pub use pointers::*;
