use super::generations::{FreePtr, Generation, GenerationLayout, InUsePtr};
use lazy_static::lazy_static;
use parking_lot::Mutex;
// use std::any::type_name;
use crate::stats::*;
use std::cell::{Cell, RefCell};
use std::mem;
use std::{collections::HashMap, num::NonZeroUsize, ptr::NonNull};

struct FreeListPool(HashMap<GenerationLayout, Vec<FreePtr>>);

lazy_static! {
    static ref GLOBAL_POOL: Mutex<FreeListPool> = Mutex::new(FreeListPool(HashMap::new()));
}

impl FreeListPool
{
    fn free_list_of<T: 'static>(&mut self) -> &mut Vec<FreePtr>
    {
        //dbg_call!("FreeListPool.free_list_of::<{}>()", type_name::<T>());
        //dbg_return!("{:?}",
        self.free_list(GenerationLayout::of::<T>())
        //)
    }

    fn free_list(&mut self, layout: GenerationLayout) -> &mut Vec<FreePtr>
    {
        //dbg!(
        //    "free_list({1:?}) => {0:?}",
        self.0.entry(layout).or_default()
        //    ,layout)
    }

    fn locate<T: 'static>(&mut self) -> Option<FreePtr>
    {
        //dbg_call!("FreeListPool.locate::<{}>()", type_name::<T>());
        //dbg_return!("{:?}",
        self.free_list_of::<T>().pop()
        //)
    }

    fn fulfill_request<T: 'static>(&mut self, number: usize, target: &mut Vec<FreePtr>) -> usize
    {
        // dbg_call!(
        // "FreeListPool.fulfill_request::<{}>({}, {:?})",
        //type_name::<T>()
        // ,number, target );
        let vec = self.free_list_of::<T>();
        target.extend(vec.drain(vec.len() - number.min(vec.len())..));
        // dbg_return!("{}",
        vec.len()
        //)
    }

    pub fn get_stats(&self) -> Stats
    {
        let mut res = Stats::default();
        res.by_layout = self.0.iter().map(|(k, v)| (*k, v.len())).collect();
        res
    }
}

struct LocalFreeListPool
{
    pool: RefCell<FreeListPool>,
    request_sizes: RefCell<HashMap<GenerationLayout, Option<NonZeroUsize>>>,
    guards: Cell<usize>,
    dropq: RefCell<Vec<DropLater>>,
    dropq_info: RefCell<HashMap<GenerationLayout, usize>>,
}

thread_local! {
    static LOCAL_POOL: LocalFreeListPool = LocalFreeListPool {
        pool: RefCell::new(FreeListPool(HashMap::new())),
        request_sizes: RefCell::new(HashMap::new()),
        dropq: RefCell::new(Vec::new()),
        dropq_info: RefCell::new(HashMap::new()),
        guards: Cell::new(0)
    };
}

impl LocalFreeListPool
{
    fn is_safe(&self) -> bool
    {
        //dbg!("is_safe() => {}",
        self.guards.get() == 0
        //)
    }

    fn register_guard(&self)
    {
        // dbg_call!("LocalFreeListPool.register_guard()");
        let guards = self.guards.get() + 1;
        // dbg_println!("guards = {}", guards);
        self.guards.set(guards);
        // dbg_return!();
    }

    fn deregister_guard(&self)
    {
        // dbg_call!("LocalFreeListPool.deregister_guard()");
        let guards = self.guards.get() - 1;
        // dbg_println!("guards = {}", guards);
        self.guards.set(guards);
        self.purge_drop_queue();
        // dbg_return!();
    }

    fn reclaim<T: 'static>(&self, it: InUsePtr<T>)
    {
        // dbg_call!("LocalFreeListPool.reclaim<{}>({:?})", type_name::<T>(), it);
        if self.is_safe() {
            unsafe {
                self.free_now(it);
            }
        } else {
            self.drop_later(it);
        }
        // dbg_return!();
    }

    fn reallocate<T: 'static>(&self) -> Option<FreePtr>
    {
        //dbg_call!("LocalFreeListPool.reallocate<{}>()", type_name::<T>());
        let res = self.pool.borrow_mut().locate::<T>();
        //dbg_return!("{:?}",
        res.or_else(|| self.request::<T>())
        //)
    }

    fn request<T: 'static>(&self) -> Option<FreePtr>
    {
        //dbg_call!("LocalFreeListPool.request::<{}>()", type_name::<T>());
        let layout = GenerationLayout::of::<T>();
        let mut rqsz = self.request_sizes.borrow_mut();
        let sz = rqsz.entry(layout).or_insert(NonZeroUsize::new(32));

        let s = (*sz)?.get();

        let mut fls = self.pool.borrow_mut();
        let fl = fls.free_list_of::<T>();

        *sz = NonZeroUsize::new(GLOBAL_POOL.lock().fulfill_request::<T>(s, fl).min(s * 2));

        let res = fl.pop();
        //dbg_return!("{:?}",
        res
        //)
    }

    fn drop_later<T: 'static>(&self, it: InUsePtr<T>)
    {
        // dbg_call!(
        //     "LocalFreeListPool.drop_later::<{}>({:?})",
        //     type_name::<T>(),
        //     it
        // );
        unsafe { it.invalidate() }
        self.dropq.borrow_mut().push(DropLater::new(it));
        *self
            .dropq_info
            .borrow_mut()
            .entry(GenerationLayout::of::<T>())
            .or_default() += 1;
        // dbg_return!();
    }

    fn purge_drop_queue(&self)
    {
        // dbg_call!("LocalFreeListPool.purge_drop_queue");
        if self.is_safe() {
            if !self.dropq.borrow().is_empty() {
                let mut dropq = Vec::new();
                mem::swap(&mut dropq, &mut self.dropq.borrow_mut());
                //dbg_println!("purging drop queue of {} elements", dropq.len());
                let mut pool = self.pool.borrow_mut();
                for dq in dropq.drain(..) {
                    dq.drop_it(&mut pool)
                }
                std::mem::drop(pool);
                self.dropq_info.borrow_mut().clear();
            } else {
                // dbg_println!("drop queue empty");
            }
        } else if !self.dropq.borrow().is_empty() {
            // dbg_println!("unsafe to purge");
        } else {
            // dbg_println!("nothing to purge");
        }
        // dbg_return!();
    }

    fn reset_requests(&self)
    {
        for (_, sz) in self.request_sizes.borrow_mut().iter_mut() {
            if sz.is_none() {
                *sz = NonZeroUsize::new(32)
            }
        }
    }

    unsafe fn free_now<T: 'static>(&self, it: InUsePtr<T>)
    {
        // dbg_call!(
        //     "LocalFreeListPool.free_now::<{}>({:?})",
        //     type_name::<T>(),
        //     it
        // );
        it.invalidate();
        self.free_now_unique(it);
        // dbg_return!();
    }

    unsafe fn free_now_unique<T: 'static>(&self, it: InUsePtr<T>)
    {
        // dbg_call!(
        //     "LocalFreeListPool.free_now_unchecked::<{}>({:?})",
        //     type_name::<T>(),
        //     it
        // );
        if let Some(it) = it.upcast_drop() {
            self.free_directly(GenerationLayout::of::<T>(), it)
        }
        // dbg_return!();
    }

    unsafe fn free_directly(&self, layout: GenerationLayout, it: FreePtr)
    {
        // dbg_call!("LocalFreeListPool.free_directly({:?}, {:?})", layout, it);
        self.pool.borrow_mut().free_list(layout).push(it);
        // dbg_return!();
    }

    fn get_stats(&self) -> Stats
    {
        let mut res = self.pool.borrow().get_stats();
        res.drop_queue_info = self.dropq_info.borrow().clone();
        res.guards = self.guards.get();
        res
    }
}

/// Reset allocation behavior to request items from the global pool.
#[allow(dead_code)]
pub fn reset_request_behavior() { LOCAL_POOL.with(|x| x.reset_requests()) }

/// Heap memory usage for thread-local allocation pool.
#[allow(dead_code)]
pub fn thread_local_stats() -> Stats { LOCAL_POOL.with(|x| x.get_stats()) }

/// Heap memory usage for global allocation pool.
///
/// The global memory pool does not have a drop queue and do not track guards,
/// so `drop_queue_info` is always empty and `guards` is always zero.
#[allow(dead_code)]
pub fn global_stats() -> Stats { GLOBAL_POOL.lock().get_stats() }

/// Collection of heap memory usage statistics.

pub(crate) fn guard_now_in_use()
{
    // dbg_call!("guard_now_in_use()");
    LOCAL_POOL.with(|x| x.register_guard());
    // dbg_return!();
}
pub(crate) fn guard_no_longer_in_use()
{
    // dbg_call!("guard_no_longer_in_use()");
    LOCAL_POOL.with(|x| x.deregister_guard());
    // dbg_return!();
}
pub(crate) fn guards_exist() -> bool
{
    // dbg_call!("guards_exist()");
    // dbg_return!("{}",
    !LOCAL_POOL.with(|x| x.is_safe())
    // )
}
pub(crate) unsafe fn free<T>(it: InUsePtr<T>)
{
    // dbg_call!("free<{}>({:?})", type_name::<T>(), it);
    LOCAL_POOL.with(|x| x.reclaim(it));
    // dbg_return!();
}
pub(crate) unsafe fn free_unique<T>(it: InUsePtr<T>)
{
    // dbg_call!("free_unchecked<{}>({:?})", type_name::<T>(), it);
    LOCAL_POOL.with(|x| x.free_now_unique(it));
    // dbg_return!();
}
pub(crate) fn allocate<T: 'static>() -> Option<FreePtr>
{
    // dbg_call!("allocate<{}>()", type_name::<T>());
    // dbg_return!("{:?}",
    LOCAL_POOL.with(|x| x.reallocate::<T>())
    // )
}
pub(crate) fn free_and_take<T>(it: InUsePtr<T>) -> Option<T>
{
    // dbg_call!("try_free_and_take<{}>({:?})", type_name::<T>(), it);
    let res = if guards_exist() {
        None
    } else {
        Some(unsafe { free_and_take_unchecked(it) })
    };
    if res.is_none() {
        // dbg_return!("None");
    } else {
        // dbg_return!("Some(_ : {})", type_name::<T>());
    }
    res
}

pub(crate) unsafe fn free_and_take_unchecked<T: 'static>(it: InUsePtr<T>) -> T
{
    // dbg_call!("free_and_take_unchecked<{}>({:?})", type_name::<T>(), it);
    it.invalidate();
    let (res, it) = it.upcast_take();
    if let Some(it) = it {
        LOCAL_POOL.with(|x| x.free_directly(GenerationLayout::of::<T>(), it));
    }
    // dbg_return!("_ : {}", type_name::<T>());
    res
}

impl Drop for LocalFreeListPool
{
    fn drop(&mut self)
    {
        self.guards.set(0);
        self.purge_drop_queue();
        let mut global_pool = GLOBAL_POOL.lock();
        for (layout, mut free_list) in self.pool.borrow_mut().0.drain() {
            global_pool.free_list(layout).append(&mut free_list);
        }
    }
}

pub(crate) struct DropLater
{
    ptr: NonNull<Generation<()>>,
    dropfn: unsafe fn(NonNull<Generation<()>>, &mut FreeListPool),
}

impl DropLater
{
    fn new<T>(iup: InUsePtr<T>) -> Self
    {
        unsafe { iup.invalidate() }
        DropLater {
            ptr: iup.0.cast(),
            dropfn: DropLater::drop_function::<T>,
        }
    }

    fn drop_it(self, flp: &mut FreeListPool) { unsafe { (self.dropfn)(self.ptr, flp) } }

    unsafe fn drop_function<T: 'static>(ptr: NonNull<Generation<()>>, flp: &mut FreeListPool)
    {
        // dbg_call!(
        //     "DropLater::drop_function::<{}>({:?}, _)",
        //     type_name::<T>(),
        //     ptr
        // );
        if let Some(it) = InUsePtr::<T>(ptr.cast()).upcast_drop_invalidated() {
            flp.free_list_of::<T>().push(it)
        }
        // dbg_return!();
    }
}
