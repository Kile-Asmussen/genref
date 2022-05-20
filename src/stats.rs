use std::{alloc::Layout, collections::HashMap};

use crate::GenerationLayout;

/// Heap memory usage statistics, for diagnosing memory leaks and the like.
#[derive(Default)]
#[no_coverage]
pub struct Stats
{
    /// Available freed allocations by layout.
    pub by_layout: HashMap<GenerationLayout, usize>,

    /// Allocations needing to be fried, prevented the presence of one or more
    /// active `Guard`s.
    pub drop_queue_info: HashMap<GenerationLayout, usize>,

    /// Number of active `Guard`s in this thread.
    pub guards: usize,
}

#[allow(dead_code)]
#[no_coverage]
impl Stats
{
    fn sum_sizes(map: &HashMap<GenerationLayout, usize>) -> usize
    {
        let mut res = 0;
        for (layout, amount) in map {
            res += Layout::from(*layout).size() * amount;
        }
        res
    }

    /// Number of freed allocations in this heap.
    pub fn free_objects(&self) -> usize { self.by_layout.values().sum() }

    /// Memory size of freed allocations in this heap.
    pub fn free_heap_size(&self) -> usize { Self::sum_sizes(&self.by_layout) }

    /// Number of allocations waiting to be freed.
    pub fn bound_objects(&self) -> usize { self.drop_queue_info.values().sum() }

    /// Memory size of allocations waiting to be freed.
    pub fn bound_heap_size(&self) -> usize { Self::sum_sizes(&self.drop_queue_info) }

    /// Approximate memory size of overhead objects: free lists and drop queue.
    ///
    /// Size of the internal hash tables is assumed to be negligible.
    pub fn overhead_size(&self) -> usize
    {
        self.drop_queue_info.values().sum::<usize>()
            * Layout::new::<crate::allocator::DropLater>().size()
            + self.by_layout.values().sum::<usize>()
                * Layout::new::<crate::generations::FreePtr>().size()
    }
}
