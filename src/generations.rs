use std::{
    alloc::Layout,
    // any::type_name,
    fmt,
    hash::{self, Hasher},
    mem::MaybeUninit,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

#[repr(C)]
pub(crate) struct Generation<T: 'static>
{
    data: MaybeUninit<T>,
    gen: AtomicUsize,
}

impl<T: 'static> Generation<T>
{
    unsafe fn init_data(&mut self, init: T) { self.data.write(init); }

    unsafe fn drop_data(&mut self) { self.data.assume_init_drop(); }

    unsafe fn take_data(&mut self) -> T { self.data.assume_init_read() }

    fn generation(&self) -> usize { self.gen.load(Ordering::Relaxed) }

    fn bump_generation(&self) { self.gen.fetch_add(1, Ordering::Relaxed); }

    fn is_end_of_life(&self) -> bool { self.generation() == usize::MAX }
}

#[derive(Clone, Copy)]
pub(crate) struct FreePtr(pub(crate) NonNull<Generation<()>>);

impl FreePtr
{
    pub(crate) unsafe fn downcast<T: 'static>(self, it: T) -> InUsePtr<T>
    {
        let mut res = InUsePtr::<T>(self.0.cast());
        let alloc = res.0.as_mut();
        alloc.init_data(it);
        res
    }
}

unsafe impl Send for FreePtr {}

impl fmt::Debug for FreePtr
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        f.debug_tuple("FreePtr").field(&self.0).finish()
    }
}

pub(crate) struct InUsePtr<T: 'static>(pub(crate) NonNull<Generation<T>>);

impl<T: 'static> Clone for InUsePtr<T>
{
    fn clone(&self) -> Self { InUsePtr(self.0) }
}
impl<T: 'static> Copy for InUsePtr<T> {}

impl<T: 'static> fmt::Debug for InUsePtr<T>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        f.debug_tuple("InUsePtr").field(&self.0).finish()
    }
}

impl<T: 'static> InUsePtr<T>
{
    pub(crate) fn allocate(data: T) -> InUsePtr<T>
    {
        //dbg!(
        //    "InUsePtr::<{1}>::allocate(_) => {0:?}",
        Self(unsafe {
            NonNull::new_unchecked(Box::into_raw(Box::new(Generation {
                gen: AtomicUsize::new(1),
                data: MaybeUninit::new(data),
            })))
        })
        //,type_name::<T>() )
    }

    pub(crate) unsafe fn upcast(self) -> Option<FreePtr>
    {
        let mut ptr = self.0;
        let alloc = ptr.as_mut();
        let res = if alloc.is_end_of_life() {
            None
        } else {
            Some(FreePtr(ptr.cast()))
        };
        alloc.drop_data();
        res
    }

    pub(crate) fn invalidate_weaks(&self) { unsafe { self.0.as_ref().bump_generation() } }

    pub(crate) unsafe fn upcast_take(self) -> (T, Option<FreePtr>)
    {
        let mut ptr = self.0;
        let alloc = ptr.as_mut();
        let res = if alloc.is_end_of_life() {
            Some(FreePtr(ptr.cast()))
        } else {
            None
        };
        let t = alloc.take_data();
        (t, res)
    }

    pub(crate) unsafe fn data_ref(&self) -> &T { self.0.as_ref().data.assume_init_ref() }

    pub(crate) unsafe fn data_mut(&mut self) -> &mut T { self.0.as_mut().data.assume_init_mut() }

    pub(crate) fn generation(&self) -> usize { unsafe { self.0.as_ref().generation() } }
}

/// Newtype wrapper to make `std::alloc::Layout` implement `Hash` for use in the
/// managed heap.
///
/// Generational allocations are `#[repr(C)]` and store the generation counter
/// _after_ the embedded data, in case the alignment of the data is greater than
/// its in-memory size.

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GenerationLayout(Layout);

impl GenerationLayout
{
    /// Produces the layout of an generational allocation of `T`.
    pub fn of<T: 'static>() -> Self { GenerationLayout(Layout::new::<Generation<T>>()) }

    /// Delegates to underlying `Layout`
    pub fn size(&self) -> usize { self.0.size() }

    /// Delegates to underlying `Layout`
    pub fn align(&self) -> usize { self.0.align() }
}

impl hash::Hash for GenerationLayout
{
    fn hash<H: Hasher>(&self, state: &mut H)
    {
        self.0.size().hash(state);
        self.0.align().hash(state);
    }
}

impl From<GenerationLayout> for Layout
{
    fn from(it: GenerationLayout) -> Self { it.0 }
}

impl fmt::Debug for GenerationLayout
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        f.debug_struct("GenerationLayout")
            .field("size()", &self.size())
            .field("align()", &self.align())
            .finish()
    }
}
