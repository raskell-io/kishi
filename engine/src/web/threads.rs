//! Helper threads for the browser: Web Workers that share this module's
//! memory and run Lazy SMP alongside the engine worker.
//!
//! The page spawns each helper with its own stack and TLS block (allocated with
//! `kishi_alloc` by the engine worker), then calls `kishi_helper_run`, which
//! never returns: it sleeps on an atomic until a search starts, searches until
//! the main thread raises the stop flag, and sleeps again.

use crate::position::Position;
use crate::search::{Limits, Searcher, Shared};
use core::arch::wasm32::{memory_atomic_notify, memory_atomic_wait32};
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering::SeqCst};

pub struct Job {
    pub pos: Position,
    pub history: Vec<u64>,
    pub shared: Shared,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);
/// Bumped for every search; helpers sleep on it.
static GENERATION: AtomicI32 = AtomicI32::new(0);
/// Helpers currently searching.
static ACTIVE: AtomicI32 = AtomicI32::new(0);

/// Hands a search to the helpers and wakes them.
pub fn start(job: Job) {
    *JOB.lock().unwrap() = Some(job);
    GENERATION.fetch_add(1, SeqCst);
    unsafe { memory_atomic_notify(GENERATION.as_ptr(), u32::MAX) };
}

/// Waits until every helper has noticed the stop flag.
pub fn wait_idle() {
    loop {
        let active = ACTIVE.load(SeqCst);
        if active == 0 {
            return;
        }
        unsafe { memory_atomic_wait32(ACTIVE.as_ptr(), active, 1_000_000) };
    }
}

/// Memory for a helper's stack or TLS block, from the shared heap.
#[unsafe(no_mangle)]
pub extern "C" fn kishi_alloc(size: usize, align: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(size.max(1), align.max(16)).unwrap();
    unsafe { std::alloc::alloc_zeroed(layout) }
}

#[unsafe(no_mangle)]
pub extern "C" fn kishi_helper_run() {
    let mut searcher: Option<Searcher> = None;
    let mut seen = GENERATION.load(SeqCst);
    loop {
        while GENERATION.load(SeqCst) == seen {
            unsafe { memory_atomic_wait32(GENERATION.as_ptr(), seen, -1) };
        }
        seen = GENERATION.load(SeqCst);
        ACTIVE.fetch_add(1, SeqCst);
        let job = JOB.lock().unwrap().as_ref().map(|j| (j.pos, j.history.clone(), j.shared.clone()));
        if let Some((pos, history, shared)) = job {
            let s = searcher.get_or_insert_with(|| Searcher::new(0));
            s.attach_to(shared);
            s.search(&pos, &history, Limits::default(), &mut |_| {});
        }
        ACTIVE.fetch_sub(1, SeqCst);
        unsafe { memory_atomic_notify(ACTIVE.as_ptr(), u32::MAX) };
    }
}
