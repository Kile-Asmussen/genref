
use std::mem;
use std::{alloc::Layout, collections::HashMap, num::NonZeroUsize, ptr::NonNull};
use lazy_static::lazy_static;
use ref_thread_local::{ref_thread_local, RefThreadLocal};
use parking_lot::Mutex;
use super::generations::{FreePtr, Generation, GenerationLayout, InUsePtr};

struct FreeListPool(HashMap<GenerationLayout, Vec<FreePtr>>);

lazy_static! {
    static ref GLOBAL_POOL: Mutex<FreeListPool> = Mutex::new(FreeListPool(HashMap::new()));
}

impl FreeListPool
{
    fn free_list_of<T: 'static>(&mut self) -> &mut Vec<FreePtr>
    {
        self.free_list(GenerationLayout::of::<T>())
    }

    fn free_list(&mut self, layout: GenerationLayout) -> &mut Vec<FreePtr>
    {
        self.0.entry(layout).or_default()
    }

    fn reallocate<T: 'static>(&mut self) -> Option<FreePtr> { self.free_list_of::<T>().pop() }

    fn request<T: 'static>(&mut self, number: usize, target: &mut Vec<FreePtr>) -> usize
    {
        let vec = self.free_list_of::<T>();
        target.extend(vec.drain(vec.len() - number.min(vec.len())..));
        vec.len()
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
    pool: FreeListPool,
    request_sizes: HashMap<GenerationLayout, Option<NonZeroUsize>>,
    guards: usize,
    dropq: Vec<DropLater>,
    dropq_info: HashMap<GenerationLayout, usize>,
}

ref_thread_local! {
    static managed LOCAL_POOL: LocalFreeListPool = LocalFreeListPool {
        pool: FreeListPool(HashMap::new()),
        request_sizes: HashMap::new(),
        dropq: Vec::new(),
        dropq_info: HashMap::new(),
        guards: 0
    };
}

impl LocalFreeListPool
{
    fn is_safe(&self) -> bool { self.guards > 0 }

    fn register_guard(&mut self) { self.guards += 1; }

    fn deregister_guard(&mut self)
    {
        self.guards -= 1;
        self.purge_drop_queue();
    }

    fn reallocate<T: 'static>(&mut self) -> Option<FreePtr>
    {
        self.pool.reallocate::<T>().or_else(|| self.request::<T>())
    }

    fn reclaim<T: 'static>(&mut self, it: InUsePtr<T>)
    {
        if self.is_safe() {
            unsafe {
                self.free_now(it);
            }
        } else {
            self.drop_later(it);
        }
    }

    fn drop_later<T: 'static>(&mut self, it: InUsePtr<T>)
    {
        self.dropq.push(DropLater::new(it));
        *self
            .dropq_info
            .entry(GenerationLayout::of::<T>())
            .or_default() += 1;
    }

    fn purge_drop_queue(&mut self)
    {
        if self.is_safe() && !self.dropq.is_empty() {
            let mut dropq = Vec::new();
            mem::swap(&mut dropq, &mut self.dropq);
            for dq in dropq.drain(..) {
                dq.drop_it(&mut self.pool)
            }
            self.dropq_info.clear();
        }
    }

    unsafe fn free_now<T: 'static>(&mut self, it: InUsePtr<T>)
    {
        if let Some(it) = it.upcast() {
            self.free(GenerationLayout::of::<T>(), it)
        }
    }

    unsafe fn free(&mut self, layout: GenerationLayout, it: FreePtr)
    {
        self.pool.free_list(layout).push(it)
    }

    fn request<T: 'static>(&mut self) -> Option<FreePtr>
    {
        let layout = GenerationLayout::of::<T>();
        let sz = self
            .request_sizes
            .entry(layout)
            .or_insert(NonZeroUsize::new(32));

        let s = (*sz)?.get();

        let fl = self.pool.free_list_of::<T>();

        *sz = NonZeroUsize::new(GLOBAL_POOL.lock().request::<T>(s, fl).min(s * 2));

        fl.pop()
    }

    fn get_stats(&self) -> Stats
    {
        let mut res = self.pool.get_stats();
        res.drop_queue_info = self.dropq_info.clone();
        res.guards = self.guards;
        res
    }
}

#[derive(Default)]
pub struct Stats
{
    pub by_layout: HashMap<GenerationLayout, usize>,
    pub drop_queue_info: HashMap<GenerationLayout, usize>,
    pub guards: usize,
}

#[allow(dead_code)]
pub fn get_stats() -> Stats { LOCAL_POOL.borrow().get_stats() }

#[allow(dead_code)]
pub fn get_global_stats() -> Stats { GLOBAL_POOL.lock().get_stats() }

#[allow(dead_code)]
impl Stats
{
    pub fn sum_sizes(map: &HashMap<GenerationLayout, usize>) -> usize
    {
        let mut res = 0;
        for (layout, amount) in map {
            res += Layout::from(*layout).size() * amount;
        }
        res
    }

    pub fn free_objects(&self) -> usize { self.by_layout.values().sum() }

    pub fn free_heap_size(&self) -> usize { Self::sum_sizes(&self.by_layout) }

    pub fn bound_heap_size(&self) -> usize { Self::sum_sizes(&self.drop_queue_info) }

    pub fn bound_objects(&self) -> usize { self.drop_queue_info.values().sum() }

    pub fn overhead_size(&self) -> usize
    {
        self.drop_queue_info.values().sum::<usize>() * Layout::new::<DropLater>().size()
            + self.by_layout.values().sum::<usize>() * Layout::new::<FreePtr>().size()
    }
}

pub(crate) fn register_guard() { LOCAL_POOL.borrow_mut().register_guard() }
pub(crate) fn deregister_guard() { LOCAL_POOL.borrow_mut().deregister_guard() }
pub(crate) fn guards_exist() -> bool { LOCAL_POOL.borrow().is_safe() }
pub(crate) fn free<T>(it: InUsePtr<T>) { LOCAL_POOL.borrow_mut().reclaim(it) }
pub(crate) fn reallocate<T: 'static>() -> Option<FreePtr>
{
    LOCAL_POOL.borrow_mut().reallocate::<T>()
}
pub(crate) fn try_free_and_take<T>(it: InUsePtr<T>) -> Option<T>
{
    if guards_exist() {
        None
    } else {
        unsafe {
            let (res, it) = it.upcast_take();
            if let Some(it) = it {
                LOCAL_POOL
                    .borrow_mut()
                    .free(GenerationLayout::of::<T>(), it);
            }
            Some(res)
        }
    }
}

impl Drop for LocalFreeListPool
{
    fn drop(&mut self)
    {
        if !self.is_safe() {
            panic!("guards persisting past local free list pool");
        }
        self.purge_drop_queue();
        let mut global_pool = GLOBAL_POOL.lock();
        for (layout, mut free_list) in self.pool.0.drain() {
            global_pool.free_list(layout).append(&mut free_list);
        }
    }
}

struct DropLater
{
    ptr: NonNull<Generation<()>>,
    dropfn: unsafe fn(NonNull<Generation<()>>, &mut FreeListPool),
}

impl DropLater
{
    fn new<T>(iup: InUsePtr<T>) -> Self
    {
        DropLater {
            ptr: iup.0.cast(),
            dropfn: DropLater::drop_function::<T>,
        }
    }

    fn drop_it(self, flp: &mut FreeListPool) { unsafe { (self.dropfn)(self.ptr, flp) } }

    unsafe fn drop_function<T: 'static>(ptr: NonNull<Generation<()>>, flp: &mut FreeListPool)
    {
        if let Some(it) = InUsePtr::<T>(ptr.cast()).upcast() {
            flp.free_list_of::<T>().push(it)
        }
    }
}
