macro_rules! counter_module {
    (scope: $scope:ident,
     Counter: $counter_ptr:ty,
     ptr: $counter_name:ident,
     val: $counter_val:expr,
     bump: $counter_bump:expr,
     Allocator: $allocator_queue:ty,
     name: $alloc_name:ident,
     init: $queue_init:expr,
     len: $queue_len:expr,
     expand: $queue_expand:expr,
     next: $queue_next:expr,
     Lock: $lock_type:ty,
     init: $lock_init:expr,
     name: $lock_name:ident,
     Writing: $write_cond:expr,
     get: $write_get:expr,
     drop: $write_drop:expr,
     Reading: $read_cond:expr,
     get: $read_get:expr,
     drop: $read_drop:expr,
     bound: $drop_bound:ident,
     later: $drop_later:ty,
    ) => {

    use crate::{
        singleish::{MingleMut, MingleRef},
    };


    #[derive(Clone, Copy)]
    struct Counter($counter_ptr);

    impl Counter
    {
        fn val(self) -> usize {
            let $counter_name = self.0;
            $counter_val
        }

        fn alloc() -> Self
        {
            FreeList::with_instance_mut(|f| f.0.pop()).unwrap_or_else(Allocator::fresh)
        }

        fn free(self)
        {
            let $counter_name = self.0;
            if $counter_bump != usize::MAX {
                FreeList::with_instance_mut(|f| f.0.push(self));
            }
        }
    }

    struct Allocator
    {
        queue: $allocator_queue,
        next: usize,
    }
    mingleton_default!($scope
        Allocator = Allocator {
            queue: $queue_init,
            next: 0
        }
    );

    impl Allocator
    {
        fn fresh() -> Counter { Self::with_instance_mut(Allocator::next) }

        fn next(&mut self) -> Counter
        {
            let $alloc_name = self;
            if $alloc_name.next == $queue_len {
                $queue_expand;
                $alloc_name.next = 0;
            }

            let res = $queue_next;
            $alloc_name.next += 1;
            Counter(res)
        }
    }

    struct FreeList(Vec<Counter>);
    mingleton_default!($scope FreeList = FreeList(Vec::with_capacity(32)));

    struct Lock($lock_type);
    mingleton!($scope Lock = Lock($lock_init));

    pub struct Writing(());
    pub fn write() -> Writing { $write_get }
    pub fn try_write() -> Option<Writing> { Writing::try_new() }

    impl Writing
    {
        fn try_new() -> Option<Self>
        {
            Lock::with_instance_ref(|$lock_name| {
                if $write_cond {
                    Some(Writing(()))
                } else {
                    None
                }
            })
        }

        fn drop_queue(&self)
        {
            DropQueue::with_instance_mut(|dq| std::mem::replace(&mut dq.0, Vec::new()));
        }
    }

    impl Drop for Writing
    {
        fn drop(&mut self) { Lock::with_instance_ref(|$lock_name| $write_drop ) }
    }

    pub struct Reading(());
    pub fn read() -> Reading { $read_get }
    pub fn try_read() -> Option<Reading> { Reading::try_new() }

    impl Reading
    {
        fn try_new() -> Option<Self>
        {
            Lock::with_instance_ref(|$lock_name| {
                if $read_cond {
                    Some(Reading(()))
                } else {
                    None
                }
            })
        }

        fn drop_later<T: $drop_bound + 'static>(&self, it: Box<T>)
        {
            let it_dyn: Box<$drop_later> = it;
            DropQueue::with_instance_mut(|dq| dq.0.push(it_dyn));
        }
    }

    impl Drop for Reading
    {
        fn drop(&mut self) { Lock::with_instance_ref(|$lock_name| $write_drop) }
    }

    impl Clone for Reading
    {
        fn clone(&self) -> Self
        {
            Lock::with_instance_ref(|$lock_name| $read_drop);
            Reading(())
        }
    }

    struct DropQueue(Vec<Box<$drop_later>>);
    mingleton_default!($scope DropQueue = DropQueue(Vec::new()));
}; }

pub(crate) use counter_module;
