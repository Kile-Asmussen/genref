use std::{cell::RefCell, marker::PhantomData, mem::swap};

use parking_lot::RwLock;

use crate::{
    singleish::{MingleMut, Mingleton},
    worldlock::{GlobalLock, ThreadlocalLock, WorldLock},
};

pub(crate) trait DropQueue: MingleMut
{
    type WideLock: WorldLock;

    fn drop_later(it: Box<<Self::WideLock as WorldLock>::Later>)
    {
        Self::with_instance_mut(|s| s.defer(it))
    }

    fn drop_now_if_possible()
    {
        if let Some(l) = Self::WideLock::try_write() {
            Self::with_instance_mut(|dq| dq.cleanup(&l))
        }
    }

    fn defer(&mut self, it: Box<<Self::WideLock as WorldLock>::Later>);

    fn underlying_vec(&mut self) -> &mut Vec<Box<<Self::WideLock as WorldLock>::Later>>;

    fn cleanup(&mut self, l: &<Self::WideLock as WorldLock>::WriteLock)
    {
        let mut vec = Vec::new();
        swap(&mut vec, self.underlying_vec());
        vec.clear();
        swap(&mut vec, self.underlying_vec());
    }
}

pub(crate) struct LockedQueue<W: WorldLock>(Vec<Box<<W as WorldLock>::Later>>);

mingleton!(
    THREAD_QUEUE: LockedQueue<ThreadlocalLock> as RefCell<LockedQueue<ThreadlocalLock>> =
        RefCell::new(LockedQueue::new())
);
mingleton_ref!(LockedQueue<ThreadlocalLock> : |tls| tls.borrow() );
mingleton_mut!(LockedQueue<ThreadlocalLock> : |tls| tls.borrow_mut() );

mingleton!(
    static GLBOAL_QUEUE: LockedQueue<GlobalLock> as RwLock<LockedQueue<GlobalLock>> =
        RwLock::new(LockedQueue::new())
);
mingleton_ref!(LockedQueue<GlobalLock> : |gs| gs.read() );
mingleton_mut!(LockedQueue<GlobalLock> : |gs| gs.write() );

impl<W: WorldLock> LockedQueue<W>
{
    fn new() -> Self { Self(Vec::with_capacity(32)) }
}

impl<W> DropQueue for LockedQueue<W>
where
    W: WorldLock,
    Self: MingleMut,
{
    type WideLock = W;

    fn defer(&mut self, it: Box<<W as WorldLock>::Later>) { self.0.push(it) }

    fn underlying_vec(&mut self) -> &mut Vec<Box<<W as WorldLock>::Later>> { &mut self.0 }
}
