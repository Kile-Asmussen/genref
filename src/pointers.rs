#[cfg(test)]
use std::mem;
use std::ptr::NonNull;

use super::counter::*;

macro_rules! clone_copy {
    ($t:ident) => {
        impl<T: 'static> Copy for $t<T> {}
        impl<T: 'static> Clone for $t<T> {
            fn clone(&self) -> Self {
                *self
            }
        }
    };
}

#[repr(C)]
pub(crate) struct LocalRaw<T: 'static> {
    pub(crate) genptr: LocalGeneration,
    pub(crate) boxptr: NonNull<T>,
    pub(crate) genref: u32,
}
clone_copy!(LocalRaw);

impl<T: 'static> LocalRaw<T> {
    pub(crate) fn globalize(&self) -> GlobalRaw<T> {
        let LocalRaw {
            genref,
            genptr,
            boxptr,
        } = *self;
        let genptr = genptr.globalize();
        GlobalRaw {
            genref,
            genptr,
            boxptr,
        }
    }
}

#[repr(C)]
pub(crate) struct GlobalRaw<T: 'static> {
    pub(crate) genptr: GlobalGeneration,
    pub(crate) boxptr: NonNull<T>,
    pub(crate) genref: u32,
}
clone_copy!(GlobalRaw);

#[repr(C)]
pub(crate) struct RawRef<T: 'static> {
    pub(crate) genptr: GenerationUnion,
    pub(crate) boxptr: NonNull<T>,
    pub(crate) genref: u32,
    pub(crate) discriminant: LocalOrGlobal,
    pub(crate) ownership: OwnershipBit,
}
clone_copy!(RawRef);

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalOrGlobal {
    Neither = 0,
    Local = 1,
    Global = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnershipBit {
    Nil = 0,
    Weak = 1,
    Strong = 2,
    Inferred = 3,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) union GenerationUnion {
    pub(crate) local: LocalGeneration,
    pub(crate) global: GlobalGeneration,
}

impl From<LocalGeneration> for GenerationUnion {
    fn from(local: LocalGeneration) -> Self {
        Self { local }
    }
}

impl From<GlobalGeneration> for GenerationUnion {
    fn from(global: GlobalGeneration) -> Self {
        Self { global }
    }
}

impl<T: 'static> From<RawRefEnum<T>> for RawRef<T> {
    fn from(it: RawRefEnum<T>) -> Self {
        match it {
            RawRefEnum::Local(LocalRaw {
                genptr,
                boxptr,
                genref,
            }) => RawRef {
                discriminant: LocalOrGlobal::Local,
                ownership: OwnershipBit::Inferred,
                genptr: genptr.into(),
                boxptr,
                genref,
            },
            RawRefEnum::Global(GlobalRaw {
                genptr,
                boxptr,
                genref,
            }) => RawRef {
                discriminant: LocalOrGlobal::Global,
                ownership: OwnershipBit::Inferred,
                genptr: genptr.into(),
                boxptr,
                genref,
            },
        }
    }
}

pub(crate) enum RawRefEnum<T: 'static> {
    Local(LocalRaw<T>),
    Global(GlobalRaw<T>),
}

impl<T: 'static> From<RawRef<T>> for RawRefEnum<T> {
    fn from(it: RawRef<T>) -> Self {
        let RawRef {
            genptr,
            boxptr,
            genref,
            discriminant,
            ..
        } = it;
        match discriminant {
            LocalOrGlobal::Local => Self::Local(LocalRaw {
                genptr: unsafe { genptr.local },
                genref,
                boxptr,
            }),
            LocalOrGlobal::Global => Self::Global(GlobalRaw {
                genptr: unsafe { genptr.global },
                genref,
                boxptr,
            }),
            _ => panic!(),
        }
    }
}

impl<T: 'static> From<LocalRaw<T>> for RawRef<T> {
    fn from(it: LocalRaw<T>) -> Self {
        RawRefEnum::Local(it).into()
    }
}
impl<T: 'static> From<GlobalRaw<T>> for RawRef<T> {
    fn from(it: GlobalRaw<T>) -> Self {
        RawRefEnum::Global(it).into()
    }
}

pub(crate) trait Reference<T: 'static> {
    type Gen: Generation + GenerationCounter + AccessControl;
    fn pointer(&self) -> NonNull<T>;
    fn validity(&self) -> u32;
    fn generation(&self) -> Self::Gen;
}

impl<T: 'static> Reference<T> for LocalRaw<T> {
    type Gen = LocalGeneration;

    #[inline(always)]
    fn pointer(&self) -> NonNull<T> {
        self.boxptr
    }
    #[inline(always)]
    fn validity(&self) -> u32 {
        self.genref
    }
    #[inline(always)]
    fn generation(&self) -> Self::Gen {
        self.genptr
    }
}

impl<T: 'static> Reference<T> for GlobalRaw<T> {
    type Gen = GlobalGeneration;
    #[inline(always)]
    fn pointer(&self) -> NonNull<T> {
        self.boxptr
    }
    #[inline(always)]
    fn validity(&self) -> u32 {
        self.genref
    }
    #[inline(always)]
    fn generation(&self) -> Self::Gen {
        self.genptr
    }
}

impl<T: 'static> Reference<T> for RawRef<T> {
    type Gen = LocalOrGlobalGeneration;

    #[inline(always)]
    fn pointer(&self) -> NonNull<T> {
        match (*self).into() {
            RawRefEnum::Local(l) => l.pointer(),
            RawRefEnum::Global(g) => g.pointer(),
        }
    }

    #[inline(always)]
    fn validity(&self) -> u32 {
        match (*self).into() {
            RawRefEnum::Local(l) => l.validity(),
            RawRefEnum::Global(g) => g.validity(),
        }
    }

    #[inline(always)]
    fn generation(&self) -> Self::Gen {
        match (*self).into() {
            RawRefEnum::Local(l) => Self::Gen::Local(l.generation()),
            RawRefEnum::Global(g) => Self::Gen::Global(g.generation()),
        }
    }
}

#[test]
fn size_concerns() {
    assert_eq!(
        mem::size_of::<LocalRaw<String>>(),
        mem::size_of::<(usize, usize, u32)>()
    );

    assert_eq!(
        mem::size_of::<GlobalRaw<String>>(),
        mem::size_of::<(usize, usize, u32)>()
    );

    assert_eq!(
        mem::size_of::<RawRef<String>>(),
        mem::size_of::<(usize, usize, u32, u8, [u8; 3])>()
    );
}
