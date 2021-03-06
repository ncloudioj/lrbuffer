mod sync;
mod thread_safe;
mod turbo;

pub use thread_safe::RingBufferSafe;
pub use turbo::RingBufferTurbo;

use crate::sync::{Condvar, Mutex, MutexGuard};

use std::cell::UnsafeCell;
use std::mem::{self, MaybeUninit};

pub struct RingBuffer<T, const N: usize> {
    buf: UnsafeCell<[MaybeUninit<T>; N]>,
    head: UnsafeCell<usize>,
    tail: UnsafeCell<usize>,
    is_full: UnsafeCell<bool>,
}

impl<T, const N: usize> Default for RingBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> RingBuffer<T, N> {
    pub fn new() -> Self {
        Self {
            buf: UnsafeCell::new(unsafe { MaybeUninit::uninit().assume_init() }),
            head: UnsafeCell::new(0),
            tail: UnsafeCell::new(0),
            is_full: UnsafeCell::new(false),
        }
    }

    pub const fn capacity(&self) -> usize {
        N
    }

    pub fn push(&self, item: T) {
        unsafe {
            let head = *self.head.get();
            let mut new_head = head + 1;
            let mut tail = *self.tail.get();
            let is_full = *self.is_full.get();
            let data = &mut (*self.buf.get())[head];
            if new_head == N {
                new_head = 0;
            }
            if tail == head && is_full {
                let replaced = mem::replace(data, MaybeUninit::new(item));
                // This will drop the overwritten value.
                replaced.assume_init();
                // Advance the tail.
                tail += 1;
                if tail == N {
                    tail = 0;
                }
                *self.tail.get() = tail;
            } else {
                data.write(item);
            }
            *self.head.get() = new_head;
            *self.is_full.get() = (new_head == tail && head > tail) || is_full;
            println!(
                "Push: head: {}, new_head: {}, tail: {}, is_full: {}",
                head,
                new_head,
                tail,
                *self.is_full.get()
            );
        }
    }

    pub fn pop(&self) -> Option<T> {
        unsafe {
            let head = *self.head.get();
            let mut tail = *self.tail.get();
            let is_full = *self.is_full.get();
            println!("Pop: head: {}, tail: {}, is_full: {}", head, tail, is_full);
            if tail == head && !is_full {
                return None;
            }

            let data = &mut (*self.buf.get())[tail];
            let replaced = mem::replace(data, MaybeUninit::uninit());
            let item = replaced.assume_init();

            tail += 1;
            if tail == N {
                tail = 0;
            }

            *self.tail.get() = tail;
            *self.is_full.get() = false;

            Some(item)
        }
    }
}

pub struct RingBufferExt<T, const N: usize> {
    buf: UnsafeCell<[MaybeUninit<T>; N]>,
    push_count: UnsafeCell<usize>,
    pop_count: UnsafeCell<usize>,
}

impl<T, const N: usize> Default for RingBufferExt<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Drop for RingBufferExt<T, N> {
    fn drop(&mut self) {
        while let Some(_item) = self.pop() {}
    }
}

impl<T, const N: usize> RingBufferExt<T, N> {
    pub fn new() -> Self {
        Self {
            buf: UnsafeCell::new(unsafe { MaybeUninit::uninit().assume_init() }),
            push_count: UnsafeCell::new(0),
            pop_count: UnsafeCell::new(0),
        }
    }

    pub const fn capacity(&self) -> usize {
        N
    }

    pub fn push(&self, item: T) {
        unsafe {
            let mut n_pop = *self.pop_count.get();
            let mut n_push = *self.push_count.get();

            let total = if n_push >= n_pop {
                n_push - n_pop
            } else {
                self.capacity() * 2 - n_pop + n_push
            };

            let head = n_push % self.capacity();
            let data = &mut (*self.buf.get())[head];
            if total >= self.capacity() {
                // The buffer is full
                let replaced = mem::replace(data, MaybeUninit::new(item));
                // This will drop the overwritten value.
                replaced.assume_init();

                // Increment the pop counter.
                n_pop += 1;
                if n_pop >= self.capacity() * 2 {
                    n_pop = 0;
                }
                *self.pop_count.get() = n_pop;
            } else {
                data.write(item);
            }

            n_push += 1;
            if n_push >= self.capacity() * 2 {
                n_push = 0;
            }
            *self.push_count.get() = n_push;
        }
    }

    pub fn pop(&self) -> Option<T> {
        unsafe {
            let mut n_pop = *self.pop_count.get();
            let n_push = *self.push_count.get();

            let total = if n_push >= n_pop {
                n_push - n_pop
            } else {
                self.capacity() * 2 - n_pop + n_push
            };

            if total == 0 {
                return None;
            }

            let tail = n_pop % self.capacity();
            let data = &mut (*self.buf.get())[tail];
            let replaced = mem::replace(data, MaybeUninit::uninit());
            let item = replaced.assume_init();

            n_pop += 1;
            if n_pop >= self.capacity() * 2 {
                n_pop = 0;
            }
            *self.pop_count.get() = n_pop;

            Some(item)
        }
    }
}

pub struct RingBufferUlt<T, const N: usize> {
    buf: UnsafeCell<[MaybeUninit<T>; N]>,
    push_count: Mutex<usize>,
    pop_count: Mutex<usize>,
    cvar: Condvar,
}

impl<T, const N: usize> Default for RingBufferUlt<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Drop for RingBufferUlt<T, N> {
    fn drop(&mut self) {
        while let Some(_item) = self.pop() {}
    }
}

#[allow(clippy::mutex_atomic)]
impl<T, const N: usize> RingBufferUlt<T, N> {
    pub fn new() -> Self {
        Self {
            buf: UnsafeCell::new(unsafe { MaybeUninit::uninit().assume_init() }),
            push_count: Mutex::new(0),
            pop_count: Mutex::new(0),
            cvar: Condvar::new(),
        }
    }

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        N
    }

    #[inline]
    fn get_pop_count(&self) -> usize {
        *self.pop_count.lock().unwrap()
    }

    #[inline]
    fn get_push_count(&self) -> usize {
        *self.push_count.lock().unwrap()
    }

    #[inline]
    fn len(&self, n_pop: usize, n_push: usize) -> usize {
        if n_push >= n_pop {
            n_push - n_pop
        } else {
            self.capacity() * 2 - n_pop + n_push
        }
    }

    pub fn push(&self, item: T) {
        let mut pop_guard = self.pop_count.lock().unwrap();
        let mut push_guard = self.push_count.lock().unwrap();
        let mut n_push = *push_guard;
        let mut n_pop = *pop_guard;

        let total = self.len(n_pop, n_push);
        let head = n_push % self.capacity();
        let data = unsafe { &mut (*self.buf.get())[head] };
        if total >= self.capacity() {
            // The buffer is full
            let replaced = mem::replace(data, MaybeUninit::new(item));
            // This will drop the overwritten value.
            unsafe { replaced.assume_init() };

            // Increment the pop counter.
            n_pop += 1;
            if n_pop >= self.capacity() * 2 {
                n_pop = 0;
            }
            *pop_guard = n_pop;
        } else {
            data.write(item);
        }
        drop(pop_guard);

        n_push += 1;
        if n_push >= self.capacity() * 2 {
            n_push = 0;
        }
        *push_guard = n_push;
        self.cvar.notify_one();
    }

    pub fn wait_and_pop(&self) -> T {
        let mut pop_guard = self.wait_for_push();
        let mut n_pop = *pop_guard;
        let tail = n_pop % self.capacity();
        let data = unsafe { &mut (*self.buf.get())[tail] };
        let replaced = mem::replace(data, MaybeUninit::uninit());
        let item = unsafe { replaced.assume_init() };

        n_pop += 1;
        if n_pop >= self.capacity() * 2 {
            n_pop = 0;
        }
        *pop_guard = n_pop;

        item
    }

    fn wait_for_push(&self) -> MutexGuard<usize> {
        let mut pop_guard = self.pop_count.lock().unwrap();
        loop {
            pop_guard = self.cvar.wait(pop_guard).unwrap();
            let n_pop = *pop_guard;
            let n_push = self.get_push_count();
            if self.len(n_pop, n_push) > 0 {
                break;
            }
        }
        pop_guard
    }

    pub fn pop(&self) -> Option<T> {
        let n_push = self.get_push_count();
        let mut pop_guard = self.pop_count.lock().unwrap();
        let mut n_pop = *pop_guard;

        let total = self.len(n_pop, n_push);

        if total == 0 {
            return None;
        }

        let tail = n_pop % self.capacity();
        let item = unsafe {
            let data = &mut (*self.buf.get())[tail];
            let replaced = mem::replace(data, MaybeUninit::uninit());
            replaced.assume_init()
        };

        n_pop += 1;
        if n_pop >= self.capacity() * 2 {
            n_pop = 0;
        }
        *pop_guard = n_pop;

        Some(item)
    }

    // fn wait_for_push(&self) -> MutexGuard<usize> {
    // let pop_guard = self.pop_count.lock().unwrap();
    // self.cvar
    // .wait_while(pop_guard, |pop_guard| {
    // let n_pop = *pop_guard;
    // let n_push = self.get_push_count();
    // let total = self.len(n_pop, n_push);
    // // Wait while the buffer is empty.
    // total == 0
    // })
    // .unwrap()
    // }
}

unsafe impl<T, const N: usize> Sync for RingBufferUlt<T, N> {}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    // use super::RingBuffer;
    // use super::RingBufferExt as RingBuffer;
    use super::RingBufferUlt as RingBuffer;

    #[test]
    fn test_capacity() {
        let buf: RingBuffer<i32, 42> = RingBuffer::new();
        assert_eq!(buf.capacity(), 42);
    }

    #[test]
    fn test_push_pop_no_overwrite() {
        let buf: RingBuffer<i32, 3> = RingBuffer::new();
        buf.push(1);
        buf.push(2);
        buf.push(3);
        assert_eq!(buf.pop().unwrap(), 1);
        assert_eq!(buf.pop().unwrap(), 2);
        assert_eq!(buf.pop().unwrap(), 3);
        assert!(buf.pop().is_none());
    }

    #[test]
    fn test_push_pop_overwrite() {
        let buf: RingBuffer<i32, 3> = RingBuffer::new();
        buf.push(1);
        buf.push(2);
        buf.push(3);
        buf.push(4);
        buf.push(5);
        assert_eq!(buf.pop().unwrap(), 3);
        assert_eq!(buf.pop().unwrap(), 4);
        assert_eq!(buf.pop().unwrap(), 5);
        assert!(buf.pop().is_none());
    }

    #[test]
    fn test_push_pop_double_overwrites() {
        let buf: RingBuffer<i32, 3> = RingBuffer::new();
        buf.push(1);
        buf.push(2);
        buf.push(3);
        buf.push(4);
        buf.push(5);
        buf.push(6);
        buf.push(7);
        buf.push(8);
        buf.push(9);
        assert_eq!(buf.pop().unwrap(), 7);
        assert_eq!(buf.pop().unwrap(), 8);
        assert_eq!(buf.pop().unwrap(), 9);
        assert!(buf.pop().is_none());
    }

    #[test]
    fn test_multi_thread_push_pop() {
        let buf = Arc::new(RingBuffer::<i32, 3>::new());
        let buf_clone = Arc::clone(&buf);

        let handle = thread::spawn(move || {
            let mut expected = 3;
            loop {
                match buf_clone.pop() {
                    None => println!("Pop: nothing"),
                    Some(data) => {
                        println!("Pop: {}", data);
                        expected -= 1;
                        if expected == 0 {
                            break;
                        }
                    }
                }
            }
        });

        buf.push(1);
        buf.push(2);
        buf.push(3);
        buf.push(4);

        handle.join().unwrap();
    }

    #[test]
    fn test_multi_thread_wait_and_pop() {
        let buf = Arc::new(RingBuffer::<i32, 3>::new());

        let n = 100;
        let n_threads = 5;
        let pop_count = Arc::new(AtomicUsize::new(0));
        // let pop_count = Arc::new(Mutex::new(0));
        let handles: Vec<_> = (0..n_threads)
            .map(|_| {
                let pc_clone = Arc::clone(&pop_count);
                let buf_clone = Arc::clone(&buf);
                thread::spawn(move || loop {
                    let guard = pc_clone.load(Ordering::Relaxed);
                    println!("Thread: {:?}; Count: {}", thread::current().id(), guard);
                    if guard >= 100 {
                        break;
                    }
                    let data = buf_clone.wait_and_pop();
                    let guard = pc_clone.fetch_add(1, Ordering::AcqRel);
                    println!(
                        "Thread: {:?}; Pop: {}, PCount: {}",
                        thread::current().id(),
                        data,
                        guard
                    );
                })
            })
            .collect();

        // Wait for pop thread to start.
        thread::sleep(std::time::Duration::from_millis(1));
        for i in 0..n + n_threads {
            buf.push(i);
            // Wait for pop thread to consume.
            thread::sleep(std::time::Duration::from_millis(1));
        }
        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_multi_thread_multi_proudcers_multi_consumers() {
        let buf = Arc::new(RingBuffer::<usize, 20>::new());

        let n = 10000;
        let n_producers = 5;
        let n_consumers = 5;
        let pop_count = Arc::new(AtomicUsize::new(0));
        let push_count = Arc::new(AtomicUsize::new(0));
        let push_handles: Vec<_> = (0..n_producers)
            .map(|_| {
                let pc_clone = Arc::clone(&pop_count);
                let ps_clone = Arc::clone(&push_count);
                let buf_clone = Arc::clone(&buf);
                thread::spawn(move || loop {
                    let guard = pc_clone.load(Ordering::SeqCst);
                    if guard < n + n_consumers - 1 {
                        let pc = ps_clone.fetch_add(1, Ordering::AcqRel);
                        buf_clone.push(pc);
                        println!(
                            "Push thread: {:?}; PC: {} PUC: {}",
                            thread::current().id(),
                            guard,
                            pc
                        );
                    } else {
                        println!("Push thread: {:?}; exiting", thread::current().id());
                        break;
                    }
                })
            })
            .collect();
        let pop_handles: Vec<_> = (0..n_consumers)
            .map(|_| {
                let pc_clone = Arc::clone(&pop_count);
                let buf_clone = Arc::clone(&buf);
                thread::spawn(move || loop {
                    let guard = pc_clone.load(Ordering::SeqCst);
                    println!(
                        "Pop thread: {:?}; Pop Count: {}",
                        thread::current().id(),
                        guard
                    );
                    if guard >= n {
                        break;
                    }
                    let data = buf_clone.wait_and_pop();
                    let guard = pc_clone.fetch_add(1, Ordering::AcqRel);
                    // println!(
                    // "Pop thread: {:?}; Pop: {}, PCount: {}",
                    // thread::current().id(),
                    // data,
                    // guard
                    // );
                })
            })
            .collect();

        for handle in push_handles.into_iter().chain(pop_handles) {
            handle.join().unwrap();
        }
    }
}
