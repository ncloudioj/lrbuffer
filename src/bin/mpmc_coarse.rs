use lrbuffer::RingBufferSafe as RingBuffer;

use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::thread;

fn main() {
    let buf = Arc::new(RingBuffer::<usize, 102400>::new());

    let n = 1_000_000;
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
