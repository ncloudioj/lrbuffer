#[cfg(loom)]
pub(crate) use loom::sync::{
    atomic::{AtomicUsize, Ordering},
    Condvar, Mutex, MutexGuard
};

#[cfg(not(loom))]
pub(crate) use std:: sync::{
    atomic::{AtomicUsize, Ordering},
    Condvar, Mutex, MutexGuard
};
