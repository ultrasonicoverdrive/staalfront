//! Follow-up 16 of phase 1: headless GPU devices must not overlap.
//!
//! The DX12 software adapter (WARP) crashed on Windows with
//! STATUS_ACCESS_VIOLATION when several screenshot tests created and used a
//! device at the same time. `Renderer::headless` now hands out one device at a
//! time per process (`HeadlessSlot`); this test starts several threads that
//! each build a renderer and draw, the way parallel test threads do, and
//! checks that never two of them hold a renderer at once and that all of them
//! finish. On the Windows CI machine it is also the case that used to crash.
//! Adapter rules as in `screenshot.rs`: software adapter preferred, a missing
//! one fails on CI (`CI` set) and skips the test locally.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use sf_render::{GpuOptions, Renderer};

static HOLDING: AtomicUsize = AtomicUsize::new(0);
static MOST: AtomicUsize = AtomicUsize::new(0);

#[test]
fn headless_renderers_in_parallel_threads_take_turns() {
    let threads: Vec<_> = (0..6)
        .map(|i| {
            thread::spawn(move || -> Result<(), String> {
                let mut r = match Renderer::headless(64, 64, &GpuOptions::software()) {
                    Ok(r) => r,
                    Err(e) if std::env::var_os("CI").is_none() => {
                        return Err(format!("no software adapter here: {e}"));
                    }
                    Err(e) => panic!("NO GPU ADAPTER AVAILABLE: {e}"),
                };
                let now = HOLDING.fetch_add(1, Ordering::SeqCst) + 1;
                MOST.fetch_max(now, Ordering::SeqCst);
                for _ in 0..3 {
                    let img = r.render_to_image().expect("render");
                    assert_eq!((img.width, img.height), (64, 64), "thread {i}");
                }
                HOLDING.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            })
        })
        .collect();
    let mut skipped = false;
    for t in threads {
        if let Err(why) = t.join().expect("no thread panicked") {
            eprintln!("{why}");
            skipped = true;
        }
    }
    if !skipped {
        assert_eq!(MOST.load(Ordering::SeqCst), 1, "two devices overlapped");
    }
}
