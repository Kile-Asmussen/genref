

//! A rust implementation of the Vale generational counter memory model.
//! 
//! [Vale](https://vale.dev/) boasts an innovative memory model that brokers
//! a compromise between Rust's static ownership model and reference counting.
//! Seeing as it is a good fit for rust's existing infrastructure of RAII libraries,
//! I decided to provide it as a library for programming language design.
//! 
//! In particular this is the memory model used in my upcoming project Aloxtalk.
//! 
//! This crate provides managed pointer types `Uniq`, `Owned`, `Weak`, and `Guard`,
//! as well as thread-local and global infrastructure for managing the allocations.
//! 
//! Caveat: the 'pointer' types are not actually pointers-as-such and do not support
//! unsized types. Due to implementation details, having this kind of support would
//! probably be detrimental to performance.
//! 
//!   

pub(crate) mod generations;
pub(crate) mod allocator;
pub mod pointers;

#[allow(unused_imports)]
use pointers::*;
#[allow(unused_imports)]
use allocator::{get_stats, get_global_stats};