use lrbuffer::RingBufferExt as RingBuffer;

use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};
use std::thread;

struct Data {
    data: i32,
}

impl Drop for Data {
    fn drop(&mut self) {
        println!("Dropped: {}", self.data);
    }
}

fn main() {
    let buf = Arc::new(Mutex::new(RingBuffer::<usize, 10240>::new()));

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
                    buf_clone.lock().unwrap().push(pc);
                    // println!(
                        // "Push thread: {:?}; PC: {} PUC: {}",
                        // thread::current().id(),
                        // guard,
                        // pc
                    // );
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
                // println!(
                    // "Pop thread: {:?}; Pop Count: {}",
                    // thread::current().id(),
                    // guard
                // );
                if guard >= n {
                    break;
                }
                let data = buf_clone.lock().unwrap().pop();
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

    let buf = RingBuffer::<Data, 10>::new();
    buf.push(Data { data: 1 });
    buf.push(Data { data: 2 });
    buf.push(Data { data: 3 });
}
