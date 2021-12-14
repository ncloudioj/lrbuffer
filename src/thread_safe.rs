use crate::sync::{AtomicUsize, Condvar, Mutex, MutexGuard, Ordering};

use std::cell::UnsafeCell;
use std::mem::{self, MaybeUninit};

pub struct RingBufferSafe<T, const N: usize> {
    buf: UnsafeCell<[MaybeUninit<T>; N]>,
    push_count: Mutex<usize>,
    pop_count: Mutex<usize>,
    len: AtomicUsize,
    cvar: Condvar,
}

impl<T, const N: usize> Default for RingBufferSafe<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::mutex_atomic)]
impl<T, const N: usize> RingBufferSafe<T, N> {
    pub fn new() -> Self {
        Self {
            buf: UnsafeCell::new(unsafe { MaybeUninit::uninit().assume_init() }),
            push_count: Mutex::new(0),
            pop_count: Mutex::new(0),
            len: AtomicUsize::new(0),
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
        let mut push_guard = self.push_count.lock().unwrap();
        let mut n_push = *push_guard;

        let total = self.len.load(Ordering::Acquire);
        if total >= self.capacity() {
            // The buffer is full, delegate this to `push_ex` as it will do it
            // with the heavier double lock push. Note that the `push_guard`
            // needs to be dropped before the call.
            drop(push_guard);
            return self.push_ex(item);
        }

        let head = n_push % self.capacity();
        let data = unsafe { &mut (*self.buf.get())[head] };
        // The `data` is guaranteed to be "uninit", so just write it directly.
        data.write(item);

        n_push += 1;
        if n_push >= self.capacity() * 2 {
            n_push = 0;
        }
        *push_guard = n_push;
        self.len.fetch_add(1, Ordering::AcqRel);
        self.cvar.notify_one();
    }

    fn push_ex(&self, item: T) {
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
        self.len.fetch_add(1, Ordering::AcqRel);
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
        self.len.fetch_sub(1, Ordering::AcqRel);

        item
    }

    fn wait_for_push(&self) -> MutexGuard<usize> {
        let mut pop_guard = self.pop_count.lock().unwrap();
        loop {
            pop_guard = self.cvar.wait(pop_guard).unwrap();
            if self.len.load(Ordering::Acquire) > 0 {
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
        self.len.fetch_sub(1, Ordering::AcqRel);

        Some(item)
    }
}

unsafe impl<T, const N: usize> Sync for RingBufferSafe<T, N> {}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    // use super::RingBuffer;
    // use super::RingBufferExt as RingBuffer;
    use super::RingBufferSafe as RingBuffer;

    #[test]
    fn test_safe_capacity() {
        let buf: RingBuffer<i32, 42> = RingBuffer::new();
        assert_eq!(buf.capacity(), 42);
    }

    #[test]
    fn test_safe_push_pop_no_overwrite() {
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
    fn test_safe_push_pop_overwrite() {
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
    fn test_safe_push_pop_double_overwrites() {
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
    fn test_safe_multi_thread_push_pop() {
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
    fn test_safe_multi_thread_wait_and_pop() {
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
    fn test_safe_multi_thread_multi_proudcers_multi_consumers() {
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
