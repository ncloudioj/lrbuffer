#![cfg(loom)]

use lrbuffer::RingBufferUlt as RingBuffer;

use loom::thread;
use loom::sync::atomic::{AtomicUsize, AtomicI32, Ordering};

use loom::sync::Arc;

#[test]
fn test_concurrent_logic() {
    loom::model(|| {
        let buf = Arc::new(RingBuffer::<i32, 20>::new());

        let n = 100;
        let pop_count = Arc::new(AtomicUsize::new(0));
        let pop_handles: Vec<_> = (0..1)
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
        let push_count = Arc::new(AtomicI32::new(0));
        let push_handles: Vec<_> = (0..1)
            .map(|_| {
                let pc_clone = Arc::clone(&pop_count);
                let ps_clone = Arc::clone(&push_count);
                let buf_clone = Arc::clone(&buf);
                thread::spawn(move || loop {
                    let guard = pc_clone.load(Ordering::AcqRel);
                    if guard <= n {
                        let pc = ps_clone.fetch_add(1, Ordering::AcqRel);
                        buf_clone.push(pc);
                        println!("Thread: {:?}; Push: {}", thread::current().id(), pc);
                    } else {
                        break;
                    }
                })
            })
            .collect();

        for handle in push_handles.into_iter().chain(pop_handles) {
            handle.join().unwrap();
        }
    });
}
